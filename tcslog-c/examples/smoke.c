/* Drives the C binding through the ABI a consumer uses.
 *
 * This is the only test that proves the generated header and the
 * compiled library agree: the Rust unit tests in src/lib.rs call the
 * same functions, but they call them as Rust, so a header that
 * described the wrong argument order or the wrong struct layout would
 * still pass them. Here a C compiler reads the header and the linker
 * resolves against the staticlib, which is what a consumer does.
 *
 * Each scenario works in its own subdirectory of the directory given on
 * the command line, so one cannot leave state another depends on. The
 * callbacks are process-wide, so each scenario sets or unsets the ones
 * it needs rather than inheriting them.
 *
 * Built and run by `make capi-test` from the repository root; `make
 * capi-memcheck` runs the same program under the address and
 * undefined-behaviour sanitizers.
 */

#include <dirent.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#include "tcslog.h"

#define BUF_LEN 256
#define NAME_LEN 256
#define MAX_SEGMENTS 64
#define PATH_LEN 4096

static const char *PREFIX = "seg-";
static const char *SUFFIX = ".log";

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
        printf("FAILED   %s: got %d (%s), wanted %d (%s)\n", what, (int)got,
               tcslog_status_str(got), (int)want, tcslog_status_str(want));
        failures++;
    }
}

/* A directory of its own for one scenario. */
static int scenario_dir(const char *base, const char *name, char *out, size_t cap) {
    int n = snprintf(out, cap, "%s/%s", base, name);
    if (n < 0 || (size_t)n >= cap) {
        fprintf(stderr, "scenario path too long: %s\n", name);
        return -1;
    }
    if (mkdir(out, 0755) != 0) {
        perror("mkdir");
        return -1;
    }
    return 0;
}

static int by_name(const void *a, const void *b) {
    return strcmp((const char *)a, (const char *)b);
}

/* Collects this log's segment file names, sorted. The identifiers are
 * times, and their hexadecimal form sorts in the order the files were
 * created, so sorted order is creation order. */
static int segment_files(const char *dir, char names[][NAME_LEN], int max) {
    DIR *d = opendir(dir);
    if (d == NULL) {
        perror("opendir");
        return -1;
    }
    int count = 0;
    const size_t plen = strlen(PREFIX), slen = strlen(SUFFIX);
    struct dirent *e;
    while ((e = readdir(d)) != NULL) {
        const size_t len = strlen(e->d_name);
        if (len <= plen + slen) {
            continue;
        }
        if (strncmp(e->d_name, PREFIX, plen) != 0) {
            continue;
        }
        if (strcmp(e->d_name + len - slen, SUFFIX) != 0) {
            continue;
        }
        if (count == max) {
            fprintf(stderr, "more than %d segment files\n", max);
            closedir(d);
            return -1;
        }
        snprintf(names[count++], NAME_LEN, "%s", e->d_name);
    }
    closedir(d);
    qsort(names, (size_t)count, NAME_LEN, by_name);
    return count;
}

/* What the callbacks count, one per writer. Nothing here is file-scope
 * state: each scenario keeps its own on the stack and hands a pointer
 * to the writer, which is what carrying a context is for. */
typedef struct {
    int sends;
    int completes;
    int adjustments;
    /* Non-zero makes send refuse, so one callback serves both the
     * counting scenarios and the refusing one. */
    int refuse_send;
} Counters;

static int on_send(void *ctx, const char *path) {
    Counters *c = ctx;
    (void)path;
    if (c == NULL) {
        return 0;
    }
    c->sends++;
    return c->refuse_send;
}

static int on_record_complete(void *ctx, int fd) {
    Counters *c = ctx;
    (void)fd;
    if (c != NULL) {
        c->completes++;
    }
    return 0;
}

static void on_timer_resolution_adjusted(void *ctx, uint64_t resolution_ns) {
    Counters *c = ctx;
    (void)resolution_ns;
    if (c != NULL) {
        c->adjustments++;
    }
}

/* All three callbacks, aimed at one scenario's counters. */
static TcslogCallbacks callbacks_for(Counters *c) {
    TcslogCallbacks cb;
    cb.send = on_send;
    cb.record_complete = on_record_complete;
    cb.timer_resolution_adjusted = on_timer_resolution_adjusted;
    cb.ctx = c;
    return cb;
}

/* Opens a writer with the callbacks given, or none when cb is NULL. */
static TcslogStatus open_writer(const char *dir, uint32_t data_bytes, uint32_t format_tag,
                                uint32_t fixed_len, const TcslogCallbacks *cb,
                                TcslogWrite **w) {
    return tcslog_write_open(dir, PREFIX, SUFFIX,
                             tcslog_segment_file_header_len() + data_bytes, format_tag,
                             fixed_len, cb, w);
}

/* ---------------------------------------------------------- scenarios */

/* Writes records and reads them back, with the metadata the format
 * carries. */
static void scenario_round_trip(const char *base) {
    char dir[PATH_LEN];
    if (scenario_dir(base, "round-trip", dir, sizeof dir) != 0) {
        failures++;
        return;
    }
    printf("-- round trip, variable-tsrc\n");

    /* send counts but does not move the file, so the reader below
      * still finds it. A consumer's send must take the file; this one
      * departs from that deliberately, there being nothing to read back
      * otherwise. */
    Counters counters = {0, 0, 0, 0};
    TcslogCallbacks cb = callbacks_for(&counters);

    TcslogWrite *w = NULL;
    TcslogStatus s = open_writer(dir, 4096, TCSLOG_FORMAT_VARIABLE_TS_RC, 0, &cb, &w);
    check_status(s, TCSLOG_STATUS_OK, "a writer opens");
    if (s != TCSLOG_STATUS_OK) {
        return;
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
        check(written == strlen(msgs[i]) + 20,
              "the record occupied its payload plus a header");
    }

    uint64_t resolution = 0;
    check_status(tcslog_write_timer_resolution(w, &resolution), TCSLOG_STATUS_OK,
                 "the timer resolution is reported");
    check(resolution > 0, "the timer resolution is not zero");

    check_status(tcslog_write_flush(w), TCSLOG_STATUS_OK, "the writer flushes");
    tcslog_write_close(w);
    check(counters.completes == (int)msg_count,
          "record_complete ran once per record, counted in the context");
    check(counters.sends == 1, "the close handed the open segment file over");

    TcslogRead *r = NULL;
    s = tcslog_read_open(dir, PREFIX, SUFFIX, &r);
    check_status(s, TCSLOG_STATUS_OK, "a reader opens");
    if (s != TCSLOG_STATUS_OK) {
        return;
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
        check(result.record_count == last_count + 1, "the record count steps by one");
        last_count = result.record_count;
        read_count++;
    }
    check(read_count == msg_count, "every record written was read back");

    uint64_t opened = 0;
    check_status(tcslog_read_segments_opened(r, &opened), TCSLOG_STATUS_OK,
                 "the segment count is reported");
    check(opened >= 1, "at least one segment file was opened");
    tcslog_read_close(r);
}

/* The fixed format, which carries no data header and takes one payload
 * length only. */
static void scenario_fixed_format(const char *base) {
    char dir[PATH_LEN];
    if (scenario_dir(base, "fixed", dir, sizeof dir) != 0) {
        failures++;
        return;
    }
    printf("-- fixed format\n");

    TcslogWrite *w = NULL;
    TcslogStatus s = open_writer(dir, 4096, TCSLOG_FORMAT_FIXED, 8, NULL, &w);
    check_status(s, TCSLOG_STATUS_OK, "a fixed-format writer opens");
    if (s != TCSLOG_STATUS_OK) {
        return;
    }

    uint32_t written = 0;
    s = tcslog_write_record(w, (const uint8_t *)"01234567", 8, &written);
    check_status(s, TCSLOG_STATUS_OK, "a record of exactly the fixed length is written");
    check(written == 8, "a fixed record occupies its payload and no header");

    /* Anything but the fixed length has nowhere to record its length,
     * so it cannot be stored at all. */
    s = tcslog_write_record(w, (const uint8_t *)"0123456", 7, NULL);
    check_status(s, TCSLOG_STATUS_FIXED_LEN_MISMATCH, "a short record is refused");
    s = tcslog_write_record(w, (const uint8_t *)"012345678", 9, NULL);
    check_status(s, TCSLOG_STATUS_FIXED_LEN_MISMATCH, "a long record is refused");

    tcslog_write_close(w);

    TcslogRead *r = NULL;
    s = tcslog_read_open(dir, PREFIX, SUFFIX, &r);
    check_status(s, TCSLOG_STATUS_OK, "a fixed-format log opens for reading");
    if (s != TCSLOG_STATUS_OK) {
        return;
    }
    uint8_t buf[BUF_LEN];
    TcslogReadResult result;
    memset(&result, 0, sizeof result);
    s = tcslog_read_record(r, buf, sizeof buf, &result);
    check_status(s, TCSLOG_STATUS_OK, "the fixed record is read");
    check(result.n == 8 && memcmp(buf, "01234567", 8) == 0,
          "the fixed record read back is the one written");
    check(result.meta == TCSLOG_META_FIXED, "a fixed record carries no metadata");
    check(result.timestamp == 0 && result.record_count == 0,
          "the metadata fields are zero rather than left as they were");
    tcslog_read_close(r);
}

/* A format tag outside the three, which must be refused rather than
 * constructed. */
static void scenario_invalid_format(const char *base) {
    char dir[PATH_LEN];
    if (scenario_dir(base, "invalid-format", dir, sizeof dir) != 0) {
        failures++;
        return;
    }
    printf("-- an unrecognized format tag\n");

    TcslogWrite *w = NULL;
    TcslogStatus s = open_writer(dir, 4096, 3, 0, NULL, &w);
    check_status(s, TCSLOG_STATUS_INVALID_FORMAT, "a tag outside the three is refused");
    check(w == NULL, "a refused open leaves the handle null");

    /* A segment size with no room for a record is the other thing an
     * open has to refuse. */
    s = tcslog_write_open(dir, PREFIX, SUFFIX, 1, TCSLOG_FORMAT_VARIABLE_SIMPLE, 0, NULL,
                          &w);
    check_status(s, TCSLOG_STATUS_SEG_SIZE_TOO_SMALL, "too small a segment size is refused");
    check(w == NULL, "a refused open leaves the handle null");
}

/* A record larger than the buffer offered. The front of it reaches the
 * buffer and the rest is skipped, so the next read returns the record
 * after it -- which is the part worth asserting, the name inviting the
 * opposite reading. */
static void scenario_read_overflow(const char *base) {
    char dir[PATH_LEN];
    if (scenario_dir(base, "overflow", dir, sizeof dir) != 0) {
        failures++;
        return;
    }
    printf("-- a record larger than the buffer\n");

    uint8_t payload[200];
    for (size_t i = 0; i < sizeof payload; i++) {
        payload[i] = (uint8_t)('a' + (i % 26));
    }

    TcslogWrite *w = NULL;
    TcslogStatus s = open_writer(dir, 4096, TCSLOG_FORMAT_VARIABLE_SIMPLE, 0, NULL, &w);
    check_status(s, TCSLOG_STATUS_OK, "a writer opens");
    if (s != TCSLOG_STATUS_OK) {
        return;
    }
    check_status(tcslog_write_record(w, payload, sizeof payload, NULL), TCSLOG_STATUS_OK,
                 "a 200-byte record is written");
    /* A second record, so the read after the overflow has something to
     * return. Without it an end-of-log would be indistinguishable from
     * the reader having skipped too far. */
    static const char *next_msg = "the record after it";
    check_status(tcslog_write_record(w, (const uint8_t *)next_msg, strlen(next_msg), NULL),
                 TCSLOG_STATUS_OK, "a second record is written");
    tcslog_write_close(w);

    TcslogRead *r = NULL;
    s = tcslog_read_open(dir, PREFIX, SUFFIX, &r);
    check_status(s, TCSLOG_STATUS_OK, "a reader opens");
    if (s != TCSLOG_STATUS_OK) {
        return;
    }

    uint8_t small[16];
    TcslogReadResult result;
    memset(&result, 0, sizeof result);
    s = tcslog_read_record(r, small, sizeof small, &result);
    check_status(s, TCSLOG_STATUS_READ_OVERFLOW, "too small a buffer reports an overflow");
    check(result.n == sizeof small, "the result holds the bytes that were captured");
    check(memcmp(small, payload, sizeof small) == 0,
          "the captured bytes are the front of the record");

    /* The rest of the record was skipped, so this is the next record
     * and not another attempt at the one that overflowed. */
    uint8_t big[BUF_LEN];
    memset(&result, 0, sizeof result);
    s = tcslog_read_record(r, big, sizeof big, &result);
    check_status(s, TCSLOG_STATUS_OK, "reading on returns the following record");
    check(result.n == strlen(next_msg) && memcmp(big, next_msg, result.n) == 0,
          "the following record is whole, the overflowed one having been skipped");
    tcslog_read_close(r);
}

/* A segment file deleted under the reader, which is what damaged
 * storage looks like. The loss has to be reported, with how many files
 * went missing, and the records after it still returned. */
static void scenario_truncation(const char *base) {
    char dir[PATH_LEN];
    if (scenario_dir(base, "truncation", dir, sizeof dir) != 0) {
        failures++;
        return;
    }
    printf("-- a segment file deleted from the middle of the log\n");

    /* No send, so every filled file stays where the reader will find
     * it. A small data section makes several files out of few records. */
    TcslogWrite *w = NULL;
    TcslogStatus s = open_writer(dir, 32, TCSLOG_FORMAT_VARIABLE_SIMPLE, 0, NULL, &w);
    check_status(s, TCSLOG_STATUS_OK, "a writer opens");
    if (s != TCSLOG_STATUS_OK) {
        return;
    }
    const int record_count = 12;
    for (int i = 0; i < record_count; i++) {
        char msg[16];
        int n = snprintf(msg, sizeof msg, "record%02d", i);
        if (n < 0) {
            failures++;
            tcslog_write_close(w);
            return;
        }
        if (tcslog_write_record(w, (const uint8_t *)msg, (size_t)n, NULL) !=
            TCSLOG_STATUS_OK) {
            check(0, "every record is written");
            tcslog_write_close(w);
            return;
        }
    }
    tcslog_write_close(w);

    char names[MAX_SEGMENTS][NAME_LEN];
    int count = segment_files(dir, names, MAX_SEGMENTS);
    check(count >= 4, "the records filled several segment files");
    if (count < 4) {
        return;
    }

    /* The second file, so the loss is in the middle rather than at
     * either end: a reader that mishandled the first or the last would
     * not be caught by this. */
    char victim[PATH_LEN];
    int n = snprintf(victim, sizeof victim, "%s/%s", dir, names[1]);
    if (n < 0 || (size_t)n >= sizeof victim) {
        failures++;
        return;
    }
    check(unlink(victim) == 0, "a segment file is deleted");

    TcslogRead *r = NULL;
    s = tcslog_read_open(dir, PREFIX, SUFFIX, &r);
    check_status(s, TCSLOG_STATUS_OK, "a damaged log still opens");
    if (s != TCSLOG_STATUS_OK) {
        return;
    }

    int records = 0, truncations = 0;
    uint64_t lost_total = 0;
    for (;;) {
        uint8_t buf[BUF_LEN];
        TcslogReadResult result;
        memset(&result, 0, sizeof result);
        s = tcslog_read_record(r, buf, sizeof buf, &result);
        if (s == TCSLOG_STATUS_EOF) {
            break;
        }
        if (s == TCSLOG_STATUS_OK) {
            records++;
            continue;
        }
        if (s == TCSLOG_STATUS_READ_TRUNCATED) {
            truncations++;
            lost_total += result.lost;
            continue;
        }
        if (s == TCSLOG_STATUS_SESSION_END) {
            continue;
        }
        check_status(s, TCSLOG_STATUS_OK, "reading a damaged log reports only losses");
        break;
    }
    tcslog_read_close(r);

    check(truncations >= 1, "the loss is reported rather than hidden");
    check(lost_total == 1, "the result says one segment file went missing");
    check(records > 0, "records after the gap are still returned");
    check(records < record_count, "the records in the deleted file are not invented");
    printf("         %d of %d records survived, %d truncation(s), %lu file(s) lost\n",
           records, record_count, truncations, (unsigned long)lost_total);
}

/* A send callback that refuses. The write that triggers the handover
 * has to report it: silence would leave the caller believing the
 * storage bound still holds. */
static void scenario_send_failure(const char *base) {
    char dir[PATH_LEN];
    if (scenario_dir(base, "send-failure", dir, sizeof dir) != 0) {
        failures++;
        return;
    }
    printf("-- a send callback that refuses the file\n");

    Counters counters = {0, 0, 0, 1};
    TcslogCallbacks cb = callbacks_for(&counters);

    TcslogWrite *w = NULL;
    TcslogStatus s = open_writer(dir, 32, TCSLOG_FORMAT_VARIABLE_SIMPLE, 0, &cb, &w);
    check_status(s, TCSLOG_STATUS_OK, "a writer opens");
    if (s != TCSLOG_STATUS_OK) {
        return;
    }

    /* The handover happens when a data section fills, so it takes a few
     * records to reach one. Bounded so a binding that never called the
     * callback would fail rather than loop. */
    TcslogStatus last = TCSLOG_STATUS_OK;
    int writes = 0;
    for (int i = 0; i < 32 && last == TCSLOG_STATUS_OK; i++) {
        last = tcslog_write_record(w, (const uint8_t *)"0123456789", 10, NULL);
        writes++;
    }
    check_status(last, TCSLOG_STATUS_IO_ERROR, "a refusing send reaches the caller");
    check(writes < 32, "the refusal came within the bound");
    check(counters.sends >= 1, "the refusing send was reached through its context");
    tcslog_write_close(w);
}

/* Clearing a log, which must leave nothing for a reader to find. */
static void scenario_clear(const char *base) {
    char dir[PATH_LEN];
    if (scenario_dir(base, "clear", dir, sizeof dir) != 0) {
        failures++;
        return;
    }
    printf("-- clearing a log\n");

    TcslogWrite *w = NULL;
    TcslogStatus s = open_writer(dir, 32, TCSLOG_FORMAT_VARIABLE_SIMPLE, 0, NULL, &w);
    check_status(s, TCSLOG_STATUS_OK, "a writer opens");
    if (s != TCSLOG_STATUS_OK) {
        return;
    }
    for (int i = 0; i < 8; i++) {
        tcslog_write_record(w, (const uint8_t *)"0123456789", 10, NULL);
    }
    char names[MAX_SEGMENTS][NAME_LEN];
    check(segment_files(dir, names, MAX_SEGMENTS) > 0, "the log holds segment files");

    check_status(tcslog_write_clear(w), TCSLOG_STATUS_OK, "the log clears");
    check(segment_files(dir, names, MAX_SEGMENTS) == 0, "no segment file is left");
    tcslog_write_close(w);

    TcslogRead *r = NULL;
    s = tcslog_read_open(dir, PREFIX, SUFFIX, &r);
    check_status(s, TCSLOG_STATUS_NO_SEGMENT_FILES, "a cleared log has nothing to read");
    check(r == NULL, "a refused open leaves the handle null");
}

/* The third callback. A widening cannot be provoked on demand -- it
 * takes a machine whose clock is coarser than the build-time value --
 * so what is checked is that registering it changes nothing and that it
 * does not fire when the resolution is adequate. */
static void scenario_timer_resolution_callback(const char *base) {
    char dir[PATH_LEN];
    if (scenario_dir(base, "resolution", dir, sizeof dir) != 0) {
        failures++;
        return;
    }
    printf("-- the timer resolution callback\n");

    Counters counters = {0, 0, 0, 0};
    TcslogCallbacks cb = callbacks_for(&counters);

    TcslogWrite *w = NULL;
    TcslogStatus s = open_writer(dir, 4096, TCSLOG_FORMAT_VARIABLE_SIMPLE, 0, &cb, &w);
    check_status(s, TCSLOG_STATUS_OK, "a writer opens with the callback set");
    if (s != TCSLOG_STATUS_OK) {
        return;
    }
    check_status(tcslog_write_record(w, (const uint8_t *)"telemetry", 9, NULL),
                 TCSLOG_STATUS_OK, "a record is written with the callback set");
    tcslog_write_close(w);
    check(counters.adjustments == 0,
          "no widening is reported when the resolution is adequate");
}

/* Two logs open at once, each with its own callbacks and its own
 * context. This is what a context is for and what three process-wide
 * function pointers could not express: before, both writers would have
 * called the same functions with no way to say which log a call was
 * about. */
static void scenario_two_contexts(const char *base) {
    char dir_a[PATH_LEN], dir_b[PATH_LEN];
    if (scenario_dir(base, "two-a", dir_a, sizeof dir_a) != 0 ||
        scenario_dir(base, "two-b", dir_b, sizeof dir_b) != 0) {
        failures++;
        return;
    }
    printf("-- two writers, two contexts\n");

    Counters a = {0, 0, 0, 0};
    Counters b = {0, 0, 0, 0};
    TcslogCallbacks cb_a = callbacks_for(&a);
    TcslogCallbacks cb_b = callbacks_for(&b);

    TcslogWrite *wa = NULL, *wb = NULL;
    TcslogStatus s = open_writer(dir_a, 4096, TCSLOG_FORMAT_VARIABLE_SIMPLE, 0, &cb_a, &wa);
    check_status(s, TCSLOG_STATUS_OK, "the first writer opens");
    if (s != TCSLOG_STATUS_OK) {
        return;
    }
    s = open_writer(dir_b, 4096, TCSLOG_FORMAT_VARIABLE_SIMPLE, 0, &cb_b, &wb);
    check_status(s, TCSLOG_STATUS_OK, "the second writer opens");
    if (s != TCSLOG_STATUS_OK) {
        tcslog_write_close(wa);
        return;
    }

    /* Three records to one log and one to the other, so an exchanged
     * context would show as the wrong count rather than as no count. */
    for (int i = 0; i < 3; i++) {
        check_status(tcslog_write_record(wa, (const uint8_t *)"aaa", 3, NULL),
                     TCSLOG_STATUS_OK, "a record goes to the first log");
    }
    check_status(tcslog_write_record(wb, (const uint8_t *)"b", 1, NULL), TCSLOG_STATUS_OK,
                 "a record goes to the second log");

    check(a.completes == 3, "the first context counted its own records");
    check(b.completes == 1, "the second context counted its own records");

    /* The structure passed to open was copied, so overwriting it now
     * must not reach the writer. */
    cb_a.ctx = &b;
    cb_a.record_complete = NULL;
    check_status(tcslog_write_record(wa, (const uint8_t *)"aaa", 3, NULL), TCSLOG_STATUS_OK,
                 "a record goes to the first log after its callbacks were overwritten");
    check(a.completes == 4 && b.completes == 1,
          "the writer kept the callbacks it was opened with");

    tcslog_write_close(wa);
    tcslog_write_close(wb);
    check(a.sends == 1 && b.sends == 1, "each close handed over its own log's file");
}

/* Arguments the binding has to refuse rather than dereference. */
static void scenario_null_arguments(void) {
    printf("-- null arguments\n");

    uint8_t buf[8];
    TcslogReadResult result;
    check_status(tcslog_read_record(NULL, buf, sizeof buf, &result),
                 TCSLOG_STATUS_NULL_ARGUMENT, "a null reader is refused");
    check_status(tcslog_write_flush(NULL), TCSLOG_STATUS_NULL_ARGUMENT,
                 "a null writer is refused");
    check_status(tcslog_write_clear(NULL), TCSLOG_STATUS_NULL_ARGUMENT,
                 "a null writer cannot be cleared");
    check_status(tcslog_write_record(NULL, buf, sizeof buf, NULL),
                 TCSLOG_STATUS_NULL_ARGUMENT, "a null writer cannot be written to");

    uint64_t out = 0;
    check_status(tcslog_write_timer_resolution(NULL, &out), TCSLOG_STATUS_NULL_ARGUMENT,
                 "a null writer has no resolution");
    check_status(tcslog_read_segments_opened(NULL, &out), TCSLOG_STATUS_NULL_ARGUMENT,
                 "a null reader has no segment count");

    /* Null is accepted by both closers, as free() accepts it. */
    tcslog_write_close(NULL);
    tcslog_read_close(NULL);
    check(1, "closing null does nothing");
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: %s <directory>\n", argv[0]);
        return 2;
    }
    const char *base = argv[1];

    uint32_t major = 99, minor = 99, patch = 99;
    tcslog_format_version(&major, &minor, &patch);
    printf("         stored format version %u.%u.%u, segment header %u bytes\n", major,
           minor, patch, tcslog_segment_file_header_len());
    check(major == 0, "the stored format major version is readable");

    scenario_round_trip(base);
    scenario_fixed_format(base);
    scenario_invalid_format(base);
    scenario_read_overflow(base);
    scenario_truncation(base);
    scenario_send_failure(base);
    scenario_clear(base);
    scenario_timer_resolution_callback(base);
    scenario_two_contexts(base);
    scenario_null_arguments();

    if (failures == 0) {
        printf("OK: the C binding passed every check\n");
        return 0;
    }
    printf("FAIL: %d check(s) failed\n", failures);
    return 1;
}
