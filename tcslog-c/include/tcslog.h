/* Generated from tcslog-c/src/lib.rs by cbindgen. Do not edit.
   Run `make header` from the repository root to regenerate. */

#ifndef TCSLOG_H
#define TCSLOG_H

#include <stdint.h>
#include <stddef.h>

/**
 * What a call reported.
 *
 * `Ok` is zero and every failure is positive, so `if (status)` reads
 * as "something happened". The values are ABI: see the module
 * documentation.
 */
enum TcslogStatus
#if defined(__cplusplus) || __STDC_VERSION__ >= 202311L
  : int32_t
#endif // defined(__cplusplus) || __STDC_VERSION__ >= 202311L
 {
    /**
     * The call did what was asked.
     */
    TCSLOG_STATUS_OK = 0,
    /**
     * The log has no more records. Not a failure: it is how a read
     * loop ends.
     */
    TCSLOG_STATUS_EOF = 1,
    /**
     * Writing stopped and started again here, so record numbering
     * restarts. Read again to continue with the next session.
     */
    TCSLOG_STATUS_SESSION_END = 2,
    /**
     * Telemetry was lost. `n` in the result holds the bytes of a
     * cut-short record that were recovered, and `lost` the number of
     * segment files missing. Read again to continue.
     */
    TCSLOG_STATUS_READ_TRUNCATED = 3,
    /**
     * The record is larger than the buffer offered. `n` in the result
     * holds the size the record needs. Nothing was consumed, so the
     * same record is returned by the next read.
     */
    TCSLOG_STATUS_READ_OVERFLOW = 4,
    /**
     * The real-time clock does not read later than the UNIX epoch,
     * which is what an unset clock reads on most systems.
     */
    TCSLOG_STATUS_CLOCK_ERROR = 5,
    /**
     * A `Fixed` log was given a payload that is not exactly its record
     * length, or a record length of zero.
     */
    TCSLOG_STATUS_FIXED_LEN_MISMATCH = 6,
    /**
     * A segment file's header could not be read.
     */
    TCSLOG_STATUS_INVALID_HEADER = 7,
    /**
     * The directory, prefix, or suffix cannot name a log.
     */
    TCSLOG_STATUS_INVALID_PATHNAME = 8,
    /**
     * The operating system reported an error.
     */
    TCSLOG_STATUS_IO_ERROR = 9,
    /**
     * The directory holds no segment file of this log.
     */
    TCSLOG_STATUS_NO_SEGMENT_FILES = 10,
    /**
     * A prefix or suffix contains the path separator.
     */
    TCSLOG_STATUS_PATH_DELIMITER_NOT_ALLOWED = 11,
    /**
     * The payload is larger than a record can hold.
     */
    TCSLOG_STATUS_PAYLOAD_TOO_LARGE = 12,
    /**
     * `seg_size_max` leaves no room for a data record.
     */
    TCSLOG_STATUS_SEG_SIZE_TOO_SMALL = 13,
    /**
     * The library was built without `TIMER_RESOLUTION`, so a writer
     * cannot name segment files. See the library's Setup documentation.
     */
    TCSLOG_STATUS_TIMER_RESOLUTION_ZERO = 14,
    /**
     * A segment file was written by a version this build cannot read.
     */
    TCSLOG_STATUS_VERSION_MISMATCH = 15,
    /**
     * A pointer argument that must not be null was null.
     */
    TCSLOG_STATUS_NULL_ARGUMENT = 16,
    /**
     * A string argument is not valid UTF-8, which a log's directory,
     * prefix, and suffix must be.
     */
    TCSLOG_STATUS_NOT_UTF8 = 17,
    /**
     * A panic was caught at the boundary rather than let unwind into C.
     * The handle it happened on is no longer usable.
     */
    TCSLOG_STATUS_PANIC = 18,
    /**
     * `format_tag` is not one of the three formats.
     */
    TCSLOG_STATUS_INVALID_FORMAT = 19,
};
#ifndef __cplusplus
#if __STDC_VERSION__ >= 202311L
typedef enum TcslogStatus TcslogStatus;
#else
typedef int32_t TcslogStatus;
#endif // __STDC_VERSION__ >= 202311L
#endif // __cplusplus

/**
 * Which metadata a record carried, mirroring [`Meta`].
 */
enum TcslogMeta
#if defined(__cplusplus) || __STDC_VERSION__ >= 202311L
  : int32_t
#endif // defined(__cplusplus) || __STDC_VERSION__ >= 202311L
 {
    /**
     * Fixed-length records, which carry no metadata.
     */
    TCSLOG_META_FIXED = 0,
    /**
     * Variable-length, carrying no metadata beyond the length.
     */
    TCSLOG_META_VARIABLE_SIMPLE = 1,
    /**
     * Variable-length, carrying the time written and the position in
     * the session. `timestamp` and `record_count` in the result hold
     * them; with any other value those two are zero.
     */
    TCSLOG_META_VARIABLE_TS_RC = 2,
};
#ifndef __cplusplus
#if __STDC_VERSION__ >= 202311L
typedef enum TcslogMeta TcslogMeta;
#else
typedef int32_t TcslogMeta;
#endif // __STDC_VERSION__ >= 202311L
#endif // __cplusplus

/**
 * The record layouts a log may use, as `format_tag` values for
 * [`tcslog_write_open`].
 *
 * These are the library's own tags, so a log written by a Rust caller
 * and one written through this binding agree on them. The argument is
 * a plain `uint32_t` rather than this type: a value outside the three
 * would be an invalid enum, and refusing it as
 * [`TcslogStatus::InvalidFormat`] is better than the undefined
 * behaviour of constructing one.
 */
enum TcslogFormat
#if defined(__cplusplus) || __STDC_VERSION__ >= 202311L
  : uint32_t
#endif // defined(__cplusplus) || __STDC_VERSION__ >= 202311L
 {
    /**
     * Every record holds exactly `fixed_len` payload bytes and carries
     * no data header at all. The most compact, at the price of a
     * length fixed for the life of the log.
     */
    TCSLOG_FORMAT_FIXED = 0,
    /**
     * Records vary in length, with a four-byte data header giving it.
     */
    TCSLOG_FORMAT_VARIABLE_SIMPLE = 1,
    /**
     * As `VariableSimple`, and the data header also carries the time
     * the record was written and its position in the session.
     */
    TCSLOG_FORMAT_VARIABLE_TS_RC = 2,
};
#ifndef __cplusplus
#if __STDC_VERSION__ >= 202311L
typedef enum TcslogFormat TcslogFormat;
#else
typedef uint32_t TcslogFormat;
#endif // __STDC_VERSION__ >= 202311L
#endif // __cplusplus

/**
 * A reader. Opaque to C: made by [`tcslog_read_open`] and released by
 * [`tcslog_read_close`].
 */
typedef struct TcslogRead TcslogRead;

/**
 * A writer. Opaque to C: made by [`tcslog_write_open`] and released by
 * [`tcslog_write_close`].
 */
typedef struct TcslogWrite TcslogWrite;

/**
 * What a read produced.
 *
 * Which fields mean anything depends on the status the read returned,
 * and each field says which.
 */
typedef struct TcslogReadResult {
    /**
     * Payload bytes placed in the caller's buffer. On
     * [`TcslogStatus::ReadTruncated`] the bytes that were recovered of
     * a record cut short; on [`TcslogStatus::ReadOverflow`] the size
     * the record needs, with nothing placed in the buffer.
     */
    uint32_t n;
    /**
     * Which of the three metadata shapes the record had.
     */
    TcslogMeta meta;
    /**
     * Nanoseconds since the UNIX epoch, when `meta` is
     * [`TcslogMeta::VariableTsRc`]. Zero otherwise.
     */
    uint64_t timestamp;
    /**
     * Position of the record within its session, counting from one,
     * when `meta` is [`TcslogMeta::VariableTsRc`]. Zero otherwise.
     */
    uint64_t record_count;
    /**
     * Segment files found missing, on [`TcslogStatus::ReadTruncated`].
     * Zero otherwise -- including where a record was cut short with no
     * file missing at all, which is a truncation the sequence cannot
     * explain.
     */
    uint64_t lost;
} TcslogReadResult;

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * Stores the `send` callback for every writer opened afterwards.
 *
 * Passing null unsets it, which is the default and means a filled
 * segment file is left in the log's directory.///
 * The pointer passed must match [`TcslogSendFn`]. It is spelled out in
 * the signature rather than named, because the generated header can
 * only render a nullable function pointer from the literal form.
 */
void tcslog_set_send_callback(int (*f)(const char *path));

/**
 * Stores the `record_complete` callback for every writer opened
 * afterwards. Passing null unsets it.
 */
void tcslog_set_record_complete_callback(int (*f)(int fd));

/**
 * Stores the `timer_resolution_adjusted` callback for every writer
 * opened afterwards. Passing null unsets it.
 */
void tcslog_set_timer_resolution_adjusted_callback(void (*f)(uint64_t resolution_ns));

/**
 * Writes the stored format version this build reads and writes.
 *
 * A build reads a segment file whose major version matches this one
 * and whose minor version is no greater. Any of the three pointers may
 * be null.
 */
void tcslog_format_version(uint32_t *major, uint32_t *minor, uint32_t *patch);

/**
 * The bytes a segment file's own header occupies.
 *
 * `seg_size_max` must leave room for this and for a data header, so
 * this is the floor a caller computes from.
 */
uint32_t tcslog_segment_file_header_len(void);

/**
 * A short description of a status, as a static NUL-terminated string.
 *
 * Never null, and never needs freeing. An unrecognized value gives
 * "unknown status" rather than nothing, so a caller that was compiled
 * against an older header still prints something.
 */
const char *tcslog_status_str(TcslogStatus status);

/**
 * Opens a writer on the log in `dir` whose segment files are named
 * `prefix` + identifier + `suffix`.
 *
 * `seg_size_max` is the most bytes a segment file may occupy, its own
 * header included. `format_tag` is a [`TcslogFormat`] value, and
 * `fixed_len` is the record length, read only for
 * [`TcslogFormat::Fixed`]. A tag outside the three is refused as
 * [`TcslogStatus::InvalidFormat`].
 *
 * On success `*out` holds a writer to pass to
 * [`tcslog_write_close`]. On failure `*out` is left null.
 *
 * # Safety
 *
 * The three strings must be NUL-terminated, and `out` must point to
 * writable storage for one pointer.
 */
TcslogStatus tcslog_write_open(const char *dir,
                               const char *prefix,
                               const char *suffix,
                               uint32_t seg_size_max,
                               uint32_t format_tag,
                               uint32_t fixed_len,
                               struct TcslogWrite **out);

/**
 * Writes one record, `len` bytes from `data`.
 *
 * `*written`, when `written` is not null, is left holding the bytes
 * the record occupied in the log, its data header included.
 *
 * # Safety
 *
 * `h` must come from [`tcslog_write_open`] and not yet be closed, and
 * `data` must point to `len` readable bytes. `len` of zero is allowed
 * and `data` may then be null.
 */
TcslogStatus tcslog_write_record(struct TcslogWrite *h,
                                 const uint8_t *data,
                                 uintptr_t len,
                                 uint32_t *written);

/**
 * Flushes the segment file now being written.
 *
 * # Safety
 *
 * `h` must come from [`tcslog_write_open`] and not yet be closed.
 */
TcslogStatus tcslog_write_flush(struct TcslogWrite *h);

/**
 * Removes every segment file of this log, leaving it empty.
 *
 * # Safety
 *
 * `h` must come from [`tcslog_write_open`] and not yet be closed.
 */
TcslogStatus tcslog_write_clear(struct TcslogWrite *h);

/**
 * Writes the timer resolution this writer is working with, in
 * nanoseconds.
 *
 * This is the build-time `TIMER_RESOLUTION` unless the writer found it
 * too small and widened it, in which case it is the figure to build
 * with next time.
 *
 * # Safety
 *
 * `h` must come from [`tcslog_write_open`] and not yet be closed, and
 * `out` must point to writable storage for one `uint64_t`.
 */
TcslogStatus tcslog_write_timer_resolution(struct TcslogWrite *h, uint64_t *out);

/**
 * Closes a writer and releases it. Null is accepted and does nothing,
 * as `free` does.
 *
 * The segment file being written is flushed and, if it holds any
 * records at all, handed to `send` -- so the records written last are
 * not stranded in a file the caller was never told about. That file is
 * short, unlike every other file `send` is given, which is the one
 * place a `send` that cares about size will see one.
 *
 * A close cannot report a failure, so an error from that flush or from
 * `send` is discarded. A caller that needs to know the last records
 * reached storage calls [`tcslog_write_flush`] first, which does
 * report.
 *
 * # Safety
 *
 * `h` must come from [`tcslog_write_open`] and must not be used again
 * after this returns.
 */
void tcslog_write_close(struct TcslogWrite *h);

/**
 * Opens a reader on the log in `dir` whose segment files are named
 * `prefix` + identifier + `suffix`.
 *
 * On success `*out` holds a reader to pass to [`tcslog_read_close`].
 * On failure `*out` is left null.
 *
 * # Safety
 *
 * The three strings must be NUL-terminated, and `out` must point to
 * writable storage for one pointer.
 */
TcslogStatus tcslog_read_open(const char *dir,
                              const char *prefix,
                              const char *suffix,
                              struct TcslogRead **out);

/**
 * Reads the next record into `buf`, which holds `cap` bytes.
 *
 * `*result` is filled whatever the status, so the fields a status
 * describes can be read without checking for null first. The statuses
 * that are news about the telemetry rather than a failure --
 * [`TcslogStatus::SessionEnd`] and [`TcslogStatus::ReadTruncated`] --
 * are followed by reading again; the loop ends at
 * [`TcslogStatus::Eof`].
 *
 * # Safety
 *
 * `h` must come from [`tcslog_read_open`] and not yet be closed, `buf`
 * must point to `cap` writable bytes, and `result` must point to
 * writable storage for one [`TcslogReadResult`].
 */
TcslogStatus tcslog_read_record(struct TcslogRead *h,
                                uint8_t *buf,
                                uintptr_t cap,
                                struct TcslogReadResult *result);

/**
 * Writes the number of segment files this reader has opened.
 *
 * # Safety
 *
 * `h` must come from [`tcslog_read_open`] and not yet be closed, and
 * `out` must point to writable storage for one `uint64_t`.
 */
TcslogStatus tcslog_read_segments_opened(struct TcslogRead *h, uint64_t *out);

/**
 * Closes a reader and releases it. Null is accepted and does nothing,
 * as `free` does.
 *
 * # Safety
 *
 * `h` must come from [`tcslog_read_open`] and must not be used again
 * after this returns.
 */
void tcslog_read_close(struct TcslogRead *h);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* TCSLOG_H */
