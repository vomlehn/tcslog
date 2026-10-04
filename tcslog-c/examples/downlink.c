/* Writes telemetry through the C binding the way a vehicle would, then
 * reads back what is left in the log.
 *
 * The point of the example is the `send` callback. The library hands
 * over each segment file whose data section has filled, and when the
 * callback returns there must be no file left at that path and none
 * matching the log's naming pattern -- the space is no longer being
 * accounted for. A real vehicle would compress the file, queue it for
 * the next downlink pass, or write it to removable media. Here it is
 * renamed into a "downlinked" subdirectory, which is the cheapest way
 * to satisfy the contract: a rename within a filesystem is a metadata
 * operation, and the new name does not both begin with the prefix and
 * end with the suffix, so a later reader and a later writer no longer
 * see the file at all.
 *
 * The example then does the same thing with no `send` callback at all,
 * which is what a log on a vehicle that has not reached a ground
 * station looks like: filled segment files accumulate in the directory,
 * and the storage bound the log was given is what keeps that from
 * growing without end. Those are the records the read at the end finds.
 *
 * The two phases together are why closing matters. A close flushes the
 * segment file being written and, if it holds any records, hands that
 * one to `send` as well -- short, unlike every other file `send` sees --
 * so the records written last are not stranded in a file the caller was
 * never told about. With `send` set, that leaves the log empty; with it
 * unset, everything stays.
 *
 * Build and run it with bin/run-capi-example, which compiles it
 * against the generated header and gives it a temporary directory.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

#include "tcslog.h"

/* Small enough that a few records fill a segment file, so the example
 * actually exercises a handover rather than describing one. */
#define SEG_DATA_BYTES 128
#define RECORD_COUNT 40
#define BUF_LEN 256

static const char *log_dir;
static const char *downlink_dir;
static int downlinked = 0;

/* Takes a filled segment file out of the log's namespace.
 *
 * Returns zero on success. Anything else makes the write that
 * triggered this report TCSLOG_STATUS_IO_ERROR, which is the right
 * answer: if the file cannot be taken, the storage bound the log was
 * given has stopped holding and the caller needs to know.
 */
static int on_send(const char *path) {
    const char *base = strrchr(path, '/');
    base = base ? base + 1 : path;

    char dest[4096];
    int n = snprintf(dest, sizeof dest, "%s/%s.sent", downlink_dir, base);
    if (n < 0 || (size_t)n >= sizeof dest) {
        fprintf(stderr, "send: path too long for %s\n", base);
        return 1;
    }
    if (rename(path, dest) != 0) {
        perror("send: rename");
        return 1;
    }
    downlinked++;
    return 0;
}

/* Reports that TIMER_RESOLUTION was too small for this machine and has
 * been widened. The value passed is the figure to build with next time,
 * so a deployed system records it and carries on. */
static void on_timer_resolution_adjusted(uint64_t resolution_ns) {
    fprintf(stderr,
            "note: timer resolution widened to %lu ns; build with "
            "TIMER_RESOLUTION=%lu next time\n",
            (unsigned long)resolution_ns, (unsigned long)resolution_ns);
}

/* Writes RECORD_COUNT records, which is enough to fill several segment
 * files and so to hand several over. */
static int write_telemetry(void) {
    TcslogWrite *w = NULL;
    TcslogStatus s = tcslog_write_open(
        log_dir, "seg-", ".tcslog",
        tcslog_segment_file_header_len() + SEG_DATA_BYTES,
        TCSLOG_FORMAT_VARIABLE_TS_RC, 0, &w);
    if (s != TCSLOG_STATUS_OK) {
        fprintf(stderr, "open for writing: %s\n", tcslog_status_str(s));
        return 1;
    }

    uint64_t resolution = 0;
    if (tcslog_write_timer_resolution(w, &resolution) == TCSLOG_STATUS_OK) {
        printf("timer resolution in force: %lu ns\n", (unsigned long)resolution);
    }

    for (int i = 1; i <= RECORD_COUNT; i++) {
        char msg[64];
        int n = snprintf(msg, sizeof msg, "batt=%d.%02dV temp=%dC", 24 - i / 20,
                         (i * 7) % 100, -40 + i);
        if (n < 0) {
            fprintf(stderr, "formatting record %d failed\n", i);
            tcslog_write_close(w);
            return 1;
        }

        s = tcslog_write_record(w, (const uint8_t *)msg, (size_t)n, NULL);
        if (s != TCSLOG_STATUS_OK) {
            /* A write that fails is worth stopping for: unlike a read,
             * there is no later call that recovers the record. */
            fprintf(stderr, "writing record %d: %s\n", i, tcslog_status_str(s));
            tcslog_write_close(w);
            return 1;
        }
    }

    s = tcslog_write_flush(w);
    if (s != TCSLOG_STATUS_OK) {
        fprintf(stderr, "flush: %s\n", tcslog_status_str(s));
        tcslog_write_close(w);
        return 1;
    }

    /* Closing flushes the segment file being written and hands that
     * one to `send` too, if it holds any records. It is the only short
     * file `send` is given. With no `send` set it simply stays in the
     * log, which is what the second phase relies on.
     */
    tcslog_write_close(w);
    printf("wrote %d records; %d segment file(s) handed to send so far\n",
           RECORD_COUNT, downlinked);
    return 0;
}

/* Reads back whatever is still in the log, which is the records that
 * have not been downlinked. */
static int read_remaining(void) {
    TcslogRead *r = NULL;
    TcslogStatus s = tcslog_read_open(log_dir, "seg-", ".tcslog", &r);
    if (s == TCSLOG_STATUS_NO_SEGMENT_FILES) {
        printf("nothing left in the log: every segment file was downlinked\n");
        return 0;
    }
    if (s != TCSLOG_STATUS_OK) {
        fprintf(stderr, "open for reading: %s\n", tcslog_status_str(s));
        return 1;
    }

    int records = 0, sessions = 0;
    uint64_t lost_files = 0;
    for (;;) {
        uint8_t buf[BUF_LEN];
        TcslogReadResult result;
        s = tcslog_read_record(r, buf, sizeof buf, &result);

        /* The rule is to read again until EOF. Everything else is news
         * about the telemetry rather than a failure of the reader. */
        if (s == TCSLOG_STATUS_EOF) {
            break;
        }
        switch (s) {
        case TCSLOG_STATUS_OK:
            records++;
            printf("  #%lu %.*s (%u bytes, ts=%lu)\n",
                   (unsigned long)result.record_count, (int)result.n, buf,
                   result.n, (unsigned long)result.timestamp);
            continue;
        case TCSLOG_STATUS_SESSION_END:
            /* Writing stopped and started again here, so record
             * numbering restarts. Nothing is wrong. */
            sessions++;
            continue;
        case TCSLOG_STATUS_READ_TRUNCATED:
            /* The bytes that did arrive are real measurements, so they
             * are reported rather than dropped. */
            lost_files += result.lost;
            printf("  -- %u byte(s) of a record recovered; %lu file(s) lost\n",
                   result.n, (unsigned long)result.lost);
            continue;
        case TCSLOG_STATUS_READ_OVERFLOW:
            /* The buffer holds the front of the record and the rest was
             * skipped, so reading on returns the record after it rather
             * than this one again. The bytes that did arrive are real
             * telemetry, so they are reported; the remedy is a bigger
             * buffer next time, this log having a record that does not
             * fit in this one. */
            printf("  -- %u byte(s) of a record too large for a %zu-byte "
                   "buffer; the rest was skipped\n",
                   result.n, sizeof buf);
            continue;
        default:
            fprintf(stderr, "read: %s\n", tcslog_status_str(s));
            tcslog_read_close(r);
            return 1;
        }
    }

    uint64_t opened = 0;
    tcslog_read_segments_opened(r, &opened);
    tcslog_read_close(r);

    printf("read %d record(s) from %lu segment file(s)", records,
           (unsigned long)opened);
    if (sessions > 0) {
        printf(", across %d session boundary/boundaries", sessions);
    }
    if (lost_files > 0) {
        printf(", with %lu file(s) lost", (unsigned long)lost_files);
    }
    printf("\n");
    return 0;
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: %s <directory>\n", argv[0]);
        return 2;
    }
    log_dir = argv[1];

    static char sent[4096];
    int n = snprintf(sent, sizeof sent, "%s/downlinked", log_dir);
    if (n < 0 || (size_t)n >= sizeof sent) {
        fprintf(stderr, "directory name too long\n");
        return 2;
    }
    if (mkdir(sent, 0755) != 0) {
        perror("mkdir");
        return 1;
    }
    downlink_dir = sent;

    uint32_t major = 0, minor = 0, patch = 0;
    tcslog_format_version(&major, &minor, &patch);
    printf("tcslog stored format %u.%u.%u\n", major, minor, patch);

    /* Set before opening a writer: a writer takes the callbacks as they
     * stand when it is opened. */
    tcslog_set_send_callback(on_send);
    tcslog_set_timer_resolution_adjusted_callback(on_timer_resolution_adjusted);

    printf("\n-- with a send callback: filled files leave the log\n");
    if (write_telemetry() != 0) {
        return 1;
    }
    printf("%d segment file(s) are in %s, out of the log's namespace\n",
           downlinked, downlink_dir);

    /* Unsetting it is what null is for. Everything written from here
     * stays in the log, which is what the reader below needs. */
    printf("\n-- with no send callback: filled files stay in the log\n");
    tcslog_set_send_callback(NULL);
    const int before = downlinked;
    if (write_telemetry() != 0) {
        return 1;
    }
    if (downlinked != before) {
        fprintf(stderr, "send ran after being unset\n");
        return 1;
    }

    printf("\n-- reading what is still in the log\n");
    if (read_remaining() != 0) {
        return 1;
    }
    return 0;
}
