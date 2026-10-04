/* Writes a log through the C binding, reads it back, and checks that
 * what came out is what went in.
 *
 * This is the only test that proves the generated header and the
 * compiled library agree: the Rust unit tests in src/lib.rs call the
 * same functions, but they call them as Rust, so a header that
 * described the wrong argument order or the wrong struct layout would
 * still pass them. Here a C compiler reads the header and the linker
 * resolves against the staticlib, which is what a consumer does.
 *
 * Built and run by `make capi-test` from the repository root. Takes a
 * directory to write the log into; it is left holding the segment
 * files, so the caller is the one to clean up.
 */

#include <stdio.h>
#include <string.h>
#include <stdlib.h>

#include "tcslog.h"

#define SEG_SIZE_MAX_EXTRA 4096
#define BUF_LEN 256

static int failures = 0;

/* Reports a failed check and carries on, so one run says everything
 * that is wrong rather than only the first thing. */
static void check(int ok, const char *what) {
    if (ok) {
        printf("ok       %s\n", what);
    } else {
        printf("FAILED   %s\n", what);
        failures++;
    }
}

static void check_status(TcslogStatus got, TcslogStatus want, const char *what) {
    if (got == want) {
        printf("ok       %s\n", what);
    } else {
        printf("FAILED   %s: got %d (%s), wanted %d (%s)\n", what,
               (int)got, tcslog_status_str(got),
               (int)want, tcslog_status_str(want));
        failures++;
    }
}

/* Counts the segment files handed over, which is the callback shape a
 * real consumer uses to ship telemetry down. Returning zero says the
 * file was dealt with; the file is deliberately left in place here, so
 * the reader below still has it. */
static int sent = 0;
static int on_send(const char *path) {
    (void)path;
    sent++;
    return 0;
}

static int record_completions = 0;
static int on_record_complete(int fd) {
    (void)fd;
    record_completions++;
    return 0;
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: %s <directory>\n", argv[0]);
        return 2;
    }
    const char *dir = argv[1];
    const char *prefix = "seg-";
    const char *suffix = ".log";

    uint32_t major = 99, minor = 99, patch = 99;
    tcslog_format_version(&major, &minor, &patch);
    printf("         stored format version %u.%u.%u, segment header %u bytes\n",
           major, minor, patch, tcslog_segment_file_header_len());
    check(major == 0, "the stored format major version is readable");

    tcslog_set_send_callback(on_send);
    tcslog_set_record_complete_callback(on_record_complete);

    TcslogWrite *w = NULL;
    TcslogStatus s = tcslog_write_open(
        dir, prefix, suffix,
        tcslog_segment_file_header_len() + SEG_SIZE_MAX_EXTRA,
        TCSLOG_FORMAT_VARIABLE_TS_RC,
        0, &w);
    check_status(s, TCSLOG_STATUS_OK, "a writer opens");
    if (s != TCSLOG_STATUS_OK) {
        return 1;
    }
    check(w != NULL, "the writer handle is not null");

    static const char *msgs[] = {
        "attitude nominal",
        "battery 71 percent",
        "downlink window in 420s",
    };
    const size_t msg_count = sizeof msgs / sizeof msgs[0];

    for (size_t i = 0; i < msg_count; i++) {
        uint32_t written = 0;
        s = tcslog_write_record(w, (const uint8_t *)msgs[i], strlen(msgs[i]), &written);
        check_status(s, TCSLOG_STATUS_OK, "a record is written");
        /* A VariableTsRc record carries a twenty-byte data header. */
        check(written == strlen(msgs[i]) + 20, "the record occupied its payload plus a header");
    }

    uint64_t resolution = 0;
    check_status(tcslog_write_timer_resolution(w, &resolution),
                 TCSLOG_STATUS_OK, "the timer resolution is reported");
    check(resolution > 0, "the timer resolution is not zero");

    check_status(tcslog_write_flush(w), TCSLOG_STATUS_OK, "the writer flushes");
    tcslog_write_close(w);
    check(record_completions == (int)msg_count,
          "record_complete ran once per record");

    TcslogRead *r = NULL;
    s = tcslog_read_open(dir, prefix, suffix, &r);
    check_status(s, TCSLOG_STATUS_OK, "a reader opens");
    if (s != TCSLOG_STATUS_OK) {
        return 1;
    }

    size_t read_count = 0;
    uint64_t last_count = 0;
    for (;;) {
        uint8_t buf[BUF_LEN];
        TcslogReadResult result;
        memset(&result, 0, sizeof result);
        s = tcslog_read_record(r, buf, sizeof buf, &result);
        if (s == TCSLOG_STATUS_EOF) {
            break;
        }
        if (s == TCSLOG_STATUS_SESSION_END) {
            continue;
        }
        check_status(s, TCSLOG_STATUS_OK, "a record is read");
        if (s != TCSLOG_STATUS_OK) {
            break;
        }
        check(read_count < msg_count, "no more records came back than went in");
        if (read_count < msg_count) {
            check(result.n == strlen(msgs[read_count]) &&
                      memcmp(buf, msgs[read_count], result.n) == 0,
                  "the record read back is the record written");
        }
        check(result.meta == TCSLOG_META_VARIABLE_TS_RC,
              "the record carries the format's metadata");
        check(result.timestamp > 0, "the record carries a timestamp");
        check(result.record_count == last_count + 1,
              "the record count steps by one");
        last_count = result.record_count;
        read_count++;
    }
    check(read_count == msg_count, "every record written was read back");

    uint64_t opened = 0;
    check_status(tcslog_read_segments_opened(r, &opened),
                 TCSLOG_STATUS_OK, "the segment count is reported");
    check(opened >= 1, "at least one segment file was opened");
    tcslog_read_close(r);

    /* Null is accepted by both closers, as free() accepts it. */
    tcslog_write_close(NULL);
    tcslog_read_close(NULL);
    check_status(tcslog_write_flush(NULL), TCSLOG_STATUS_NULL_ARGUMENT,
                 "a null handle is refused rather than dereferenced");

    if (failures == 0) {
        printf("OK: the C binding round-tripped %zu record(s)\n", read_count);
        return 0;
    }
    printf("FAIL: %d check(s) failed\n", failures);
    return 1;
}
