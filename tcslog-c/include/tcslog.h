/* Generated from tcslog-c/src/lib.rs by cbindgen. Do not edit.
   Run `make header` from the repository root to regenerate. */

#ifndef TCSLOG_H
#define TCSLOG_H

#include <stdint.h>
#include <stddef.h>

/**
 * The characters a segment identifier renders as, without the NUL.
 */
#define TCSLOG_SEG_ID_STR_LEN 19

/**
 * The characters a timestamp renders as, without the NUL.
 */
#define TCSLOG_TIMESTAMP_STR_LEN 30

/**
 * The largest record count a data header holds, 2^32 - 1. The record
 * after a session's last count begins a new session.
 */
#define TCSLOG_RECORD_COUNT_MAX 4294967295

/**
 * The most characters a prefix or a suffix may have.
 */
#define TCSLOG_PREFIX_SUFFIX_MAX_CHARS 16

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
	 * The record was larger than the buffer offered. `n` in the result
	 * holds how many bytes reached the front of the buffer, and they
	 * are real telemetry. The rest of the record was skipped, so the
	 * next read starts at the record after it rather than at this one:
	 * a buffer this read overflowed is too small for this log, and
	 * reading again does not recover what was dropped.
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
	/**
	 * A buffer offered for text is too short for it and its NUL.
	 * Nothing was written to it.
	 */
	TCSLOG_STATUS_BUFFER_TOO_SMALL = 20,
	/**
	 * A prefix or suffix is longer than
	 * [`TCSLOG_PREFIX_SUFFIX_MAX_CHARS`] characters.
	 */
	TCSLOG_STATUS_NAME_TOO_LONG = 21,
	/**
	 * The clock is past the last time a timestamp holds,
	 * 2554-07-21T23:34:33.709551615Z.
	 */
	TCSLOG_STATUS_TIMESTAMP_OVERFLOW = 22,
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
 * The callbacks a writer is given, and the context it hands back to
 * each of them.
 *
 * Any of the three function pointers may be null, which is that
 * callback unset. `ctx` is passed to each one and is never examined
 * here: it may be null, a pointer to anything, or an integer cast to a
 * pointer. The library does not own it, so it must outlive the writer.
 */
typedef struct TcslogCallbacks {
	/**
	 * Called with the path of a segment file being handed over.
	 * Returning non-zero makes the write that triggered it report
	 * [`TcslogStatus::IoError`].
	 */
	int (*send)(void *ctx, const char *path);
	/**
	 * Called with the descriptor of the segment file a record ended
	 * in, which the library still owns: do not close it. On a platform
	 * other than Unix, where a file is not a descriptor, it is -1.
	 * Returning non-zero makes the write report
	 * [`TcslogStatus::IoError`].
	 */
	int (*record_complete)(void *ctx, int fd);
	/**
	 * Called with the widened timer resolution, in nanoseconds, when
	 * the build-time value turned out too small for this machine.
	 */
	void (*timer_resolution_adjusted)(void *ctx, uint64_t resolution_ns);
	/**
	 * Handed to each callback above, untouched.
	 */
	void *ctx;
} TcslogCallbacks;

/**
 * A record's metadata, mirroring [`Meta`]: what
 * [`tcslog_write_last_meta`] and [`tcslog_read_last_meta`] report.
 */
typedef struct TcslogRecordMeta {
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
} TcslogRecordMeta;

/**
 * What a read produced.
 *
 * Which fields mean anything depends on the status the read returned,
 * and each field says which.
 */
typedef struct TcslogReadResult {
	/**
	 * Payload bytes placed in the caller's buffer, which are real
	 * telemetry whichever status came with them: the whole record on
	 * [`TcslogStatus::Ok`], the bytes recovered of a record cut short
	 * on [`TcslogStatus::ReadTruncated`], and the front of a record too
	 * large for the buffer on [`TcslogStatus::ReadOverflow`].
	 */
	uint32_t n;
	/**
	 * Which of the three metadata shapes the record had. Filled
	 * whenever `n` is, so the front of a record that came with
	 * [`TcslogStatus::ReadTruncated`] or [`TcslogStatus::ReadOverflow`]
	 * is described as a whole one is. A record whose data header was
	 * lost reports [`TcslogMeta::VariableSimple`].
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

/**
 * The header that opens a segment file, mirroring [`SegmentHeader`]:
 * what [`tcslog_read_current_header`] and
 * [`tcslog_read_take_opened_headers`] report.
 */
typedef struct TcslogSegmentHeader {
	/**
	 * Identifier of this segment file, which [`tcslog_seg_id_format`]
	 * renders as it appears in the file's name.
	 */
	uint64_t segment_id;
	/**
	 * Identifier of the first segment file of this session.
	 */
	uint64_t session_id;
	/**
	 * Bytes at the start of the data section that are the tail of a
	 * record begun in an earlier segment file, its data header counted.
	 */
	uint64_t remaining;
	/**
	 * Zero-based position of this segment file within its session.
	 */
	uint64_t sequence;
	/**
	 * The maximum size, in bytes, of a segment file of this log.
	 */
	uint32_t max_size;
	/**
	 * How records in this file are laid out, as a [`TcslogFormat`]
	 * value.
	 */
	uint32_t format_tag;
	/**
	 * The record length when `format_tag` is
	 * [`TcslogFormat::Fixed`]. Zero otherwise.
	 */
	uint32_t fixed_len;
} TcslogSegmentHeader;

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * Writes the stored format version this build reads and writes.
 *
 * A build reads a segment file whose major version matches this one
 * and whose minor version is no greater.
 *
 * @param major  Receives the major version, unless null.
 * @param minor  Receives the minor version, unless null.
 * @param patch  Receives the patch version, unless null.
 */
void tcslog_format_version(uint32_t *major, uint32_t *minor, uint32_t *patch);

/**
 * Gives the bytes a segment file's own header occupies.
 *
 * `seg_size_max` must leave room for this and for a data header, so
 * this is the floor a caller computes from.
 *
 * @return The length of a segment file header, in bytes.
 */
uint32_t tcslog_segment_file_header_len(void);

/**
 * Describes a status briefly.
 *
 * @param status  The status to describe.
 * @return        A static NUL-terminated string, never null and never needing
 *                freeing. An unrecognized value gives "unknown status" rather
 *                than nothing, so a caller that was compiled against an older
 *                header still prints something.
 */
const char *tcslog_status_str(TcslogStatus status);

/**
 * Renders a segment identifier as it appears in a segment file's name:
 * four dash-separated groups of four lower-case hexadecimal digits.
 *
 * # Safety
 *
 * `buf` must point to `cap` writable bytes.
 *
 * @param id   The segment identifier.
 * @param buf  Receives the text and its NUL. It must hold at least
 *             [`TCSLOG_SEG_ID_STR_LEN`] + 1 bytes.
 * @param cap  The bytes `buf` holds.
 * @return     [`TcslogStatus::Ok`]; [`TcslogStatus::NullArgument`] if `buf` is
 *             null; or [`TcslogStatus::BufferTooSmall`], leaving `buf`
 *             untouched, if `cap` is too small.
 */
TcslogStatus tcslog_seg_id_format(uint64_t id, char *buf, uintptr_t cap);

/**
 * Parses a segment identifier in the form [`tcslog_seg_id_format`]
 * renders.
 *
 * # Safety
 *
 * `text` must be a NUL-terminated string, and `out` must point to
 * writable storage for one `uint64_t`.
 *
 * @param text  The identifier's text.
 * @param out   Receives the identifier on success.
 * @return      [`TcslogStatus::Ok`]; [`TcslogStatus::NullArgument`] if either
 *              pointer is null; [`TcslogStatus::NotUtf8`] if `text` is not
 *              UTF-8; or [`TcslogStatus::InvalidPathname`] for text in any
 *              other form, as the library refuses it, since it names no segment
 *              file.
 */
TcslogStatus tcslog_seg_id_parse(const char *text, uint64_t *out);

/**
 * Renders a timestamp as an ISO 8601 UTC date and time with nanosecond
 * precision, for example `2026-09-21T16:45:12.123456789Z`.
 *
 * # Safety
 *
 * `buf` must point to `cap` writable bytes.
 *
 * @param ts   The timestamp, in nanoseconds since the UNIX epoch.
 * @param buf  Receives the text and its NUL. It must hold at least
 *             [`TCSLOG_TIMESTAMP_STR_LEN`] + 1 bytes.
 * @param cap  The bytes `buf` holds.
 * @return     [`TcslogStatus::Ok`]; [`TcslogStatus::NullArgument`] if `buf` is
 *             null; or [`TcslogStatus::BufferTooSmall`], leaving `buf`
 *             untouched, if `cap` is too small.
 */
TcslogStatus tcslog_format_timestamp(uint64_t ts, char *buf, uintptr_t cap);

/**
 * Opens a writer on the log in `dir` whose segment files are named
 * `prefix` + identifier + `suffix`.
 *
 * # Safety
 *
 * The three strings must be NUL-terminated, `cb` must be null or point
 * to a readable [`TcslogCallbacks`], and `out` must point to writable
 * storage for one pointer.
 *
 * @param dir           The directory holding the log.
 * @param prefix        What each segment file's name starts with.
 * @param suffix        What each segment file's name ends with.
 * @param seg_size_max  The most bytes a segment file may occupy, its own header
 *                      included.
 * @param format_tag    A [`TcslogFormat`] value.
 * @param fixed_len     The record length, read only for
 *                      [`TcslogFormat::Fixed`].
 * @param cb            The callbacks this writer is to use and the context to
 *                      hand them, or null for none. It is copied, so the
 *                      structure itself need not outlive the call -- but the
 *                      `ctx` it holds is used until the writer is closed, so
 *                      that must outlive the writer. Each writer has its own,
 *                      so two logs in one process can have different callbacks
 *                      and different contexts.
 * @param out           Receives a writer to pass to [`tcslog_write_close`] on
 *                      success, and is left null on failure.
 * @return              [`TcslogStatus::Ok`]; [`TcslogStatus::InvalidFormat`]
 *                      for a tag outside the three;
 *                      [`TcslogStatus::NullArgument`] or
 *                      [`TcslogStatus::NotUtf8`] for a bad pointer or string;
 *                      or the status of the library's refusal to open the log.
 */
TcslogStatus tcslog_write_open(const char *dir,
			       const char *prefix,
			       const char *suffix,
			       uint32_t seg_size_max,
			       uint32_t format_tag,
			       uint32_t fixed_len,
			       const struct TcslogCallbacks *cb,
			       struct TcslogWrite **out);

/**
 * Writes one record, `len` bytes from `data`.
 *
 * # Safety
 *
 * `h` must come from [`tcslog_write_open`] and not yet be closed, and
 * `data` must point to `len` readable bytes.
 *
 * @param h        The writer.
 * @param data     The record's payload.
 * @param len      The payload's length in bytes; zero is allowed, and `data`
 *                 may then be null.
 * @param written  Unless null, receives the bytes the record occupied in the
 *                 log, its data header included.
 * @return         [`TcslogStatus::Ok`]; [`TcslogStatus::NullArgument`] for a
 *                 null handle, or null `data` with a non-zero `len`;
 *                 [`TcslogStatus::PayloadTooLarge`] if `len` does not fit a
 *                 record; or the status of the library's failure to write.
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
 *
 * @param h  The writer.
 * @return   [`TcslogStatus::Ok`]; [`TcslogStatus::NullArgument`] if `h` is
 *           null; or the status of the library's failure to flush.
 */
TcslogStatus tcslog_write_flush(struct TcslogWrite *h);

/**
 * Removes every segment file of this log, leaving it empty.
 *
 * # Safety
 *
 * `h` must come from [`tcslog_write_open`] and not yet be closed.
 *
 * @param h  The writer.
 * @return   [`TcslogStatus::Ok`]; [`TcslogStatus::NullArgument`] if `h` is
 *           null; or the status of the library's failure to clear the log.
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
 *
 * @param h    The writer.
 * @param out  Receives the resolution, in nanoseconds.
 * @return     [`TcslogStatus::Ok`], or [`TcslogStatus::NullArgument`] if either
 *             pointer is null.
 */
TcslogStatus tcslog_write_timer_resolution(struct TcslogWrite *h,
					   uint64_t *out);

/**
 * Writes the identifier of the session being written, which is the
 * identifier of its first segment file.
 *
 * After [`tcslog_write_clear`] this still names the cleared session
 * until the next write starts one.
 *
 * # Safety
 *
 * `h` must come from [`tcslog_write_open`] and not yet be closed, and
 * `out` must point to writable storage for one `uint64_t`.
 *
 * @param h    The writer.
 * @param out  Receives the session's identifier.
 * @return     [`TcslogStatus::Ok`], or [`TcslogStatus::NullArgument`] if either
 *             pointer is null.
 */
TcslogStatus tcslog_write_session_id(struct TcslogWrite *h, uint64_t *out);

/**
 * Writes the identifier of the segment file being written.
 *
 * # Safety
 *
 * `h` must come from [`tcslog_write_open`] and not yet be closed, and
 * `out` must point to writable storage for one `uint64_t`.
 *
 * @param h    The writer.
 * @param out  Receives the segment file's identifier.
 * @return     [`TcslogStatus::Ok`], or [`TcslogStatus::NullArgument`] if either
 *             pointer is null.
 */
TcslogStatus tcslog_write_current_segment_id(struct TcslogWrite *h,
					     uint64_t *out);

/**
 * Writes the metadata minted for the most recently written record,
 * which is how a caller learns the timestamp and record count a
 * variable-tsrc record was stamped with.
 *
 * # Safety
 *
 * `h` must come from [`tcslog_write_open`] and not yet be closed, and
 * `out` must point to writable storage for one [`TcslogRecordMeta`].
 *
 * @param h    The writer.
 * @param out  Receives the record's metadata.
 * @return     [`TcslogStatus::Ok`], or [`TcslogStatus::NullArgument`] if either
 *             pointer is null.
 */
TcslogStatus tcslog_write_last_meta(struct TcslogWrite *h,
				    struct TcslogRecordMeta *out);

/**
 * Closes a writer and releases it.
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
 *
 * @param h  The writer, or null, which is accepted and does nothing, as `free`
 *           does.
 */
void tcslog_write_close(struct TcslogWrite *h);

/**
 * Opens a reader on the log in `dir` whose segment files are named
 * `prefix` + identifier + `suffix`.
 *
 * # Safety
 *
 * The three strings must be NUL-terminated, and `out` must point to
 * writable storage for one pointer.
 *
 * @param dir     The directory holding the log.
 * @param prefix  What each segment file's name starts with.
 * @param suffix  What each segment file's name ends with.
 * @param out     Receives a reader to pass to [`tcslog_read_close`] on success,
 *                and is left null on failure.
 * @return        [`TcslogStatus::Ok`]; [`TcslogStatus::NullArgument`] or
 *                [`TcslogStatus::NotUtf8`] for a bad pointer or string; or the
 *                status of the library's refusal to open the log, such as
 *                [`TcslogStatus::NoSegmentFiles`].
 */
TcslogStatus tcslog_read_open(const char *dir,
			      const char *prefix,
			      const char *suffix,
			      struct TcslogRead **out);

/**
 * Reads the next record into `buf`, which holds `cap` bytes.
 *
 * # Safety
 *
 * `h` must come from [`tcslog_read_open`] and not yet be closed, `buf`
 * must point to `cap` writable bytes, and `result` must point to
 * writable storage for one [`TcslogReadResult`].
 *
 * @param h       The reader.
 * @param buf     Receives the record's payload; may be null when `cap` is zero.
 * @param cap     The bytes `buf` holds.
 * @param result  Filled whatever the status, so the fields a status describes
 *                can be read without checking for null first.
 * @return        [`TcslogStatus::Ok`] for a whole record. The statuses that are
 *                news about the telemetry rather than a failure --
 *                [`TcslogStatus::SessionEnd`], [`TcslogStatus::ReadTruncated`]
 *                and [`TcslogStatus::ReadOverflow`] -- are followed by reading
 *                again; the loop ends at [`TcslogStatus::Eof`].
 *                [`TcslogStatus::NullArgument`] for a null handle or `result`,
 *                or a null `buf` with a non-zero `cap`; otherwise the status of
 *                the library's failure to read.
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
 *
 * @param h    The reader.
 * @param out  Receives the number of segment files opened.
 * @return     [`TcslogStatus::Ok`], or [`TcslogStatus::NullArgument`] if either
 *             pointer is null.
 */
TcslogStatus tcslog_read_segments_opened(struct TcslogRead *h, uint64_t *out);

/**
 * Writes the metadata of the record most recently decoded, which is
 * what describes the bytes a [`TcslogStatus::ReadOverflow`] or a
 * [`TcslogStatus::ReadTruncated`] left in the buffer.
 *
 * # Safety
 *
 * `h` must come from [`tcslog_read_open`] and not yet be closed, and
 * `out` must point to writable storage for one [`TcslogRecordMeta`].
 *
 * @param h    The reader.
 * @param out  Receives the record's metadata.
 * @return     [`TcslogStatus::Ok`], or [`TcslogStatus::NullArgument`] if either
 *             pointer is null.
 */
TcslogStatus tcslog_read_last_meta(struct TcslogRead *h,
				   struct TcslogRecordMeta *out);

/**
 * Writes the header of the segment file now open.
 *
 * None is open before the first read, and after a read that returned
 * [`TcslogStatus::Eof`], [`TcslogStatus::SessionEnd`] or
 * [`TcslogStatus::ReadTruncated`].
 *
 * # Safety
 *
 * `h` must come from [`tcslog_read_open`] and not yet be closed, `out`
 * must point to writable storage for one [`TcslogSegmentHeader`], and
 * `present` to writable storage for one `int`.
 *
 * @param h        The reader.
 * @param out      Receives the header when a segment file is open, and is left
 *                 alone when none is.
 * @param present  Set to 1 when a segment file is open, and to 0 when none is.
 * @return         [`TcslogStatus::Ok`], or [`TcslogStatus::NullArgument`] if
 *                 any pointer is null.
 */
TcslogStatus tcslog_read_current_header(struct TcslogRead *h,
					struct TcslogSegmentHeader *out,
					int *present);

/**
 * Asks the reader to retain the header of every segment file it opens,
 * or to stop.
 *
 * Retention is off by default and must be: the headers accumulate,
 * allocating as they do, until taken by
 * [`tcslog_read_take_opened_headers`].
 *
 * # Safety
 *
 * `h` must come from [`tcslog_read_open`] and not yet be closed.
 *
 * @param h       The reader.
 * @param enable  Non-zero to retain headers, zero to stop.
 * @return        [`TcslogStatus::Ok`], or [`TcslogStatus::NullArgument`] if `h`
 *                is null.
 */
TcslogStatus tcslog_read_collect_opened_headers(struct TcslogRead *h,
						int enable);

/**
 * Moves the oldest of the headers retained since the last take into
 * the `cap` entries at `out`, and writes how many to `*n`.
 *
 * Those that do not fit stay retained, in order, so a caller drains
 * them by calling until `*n` comes back less than `cap`.
 *
 * # Safety
 *
 * `h` must come from [`tcslog_read_open`] and not yet be closed, `out`
 * must point to `cap` writable [`TcslogSegmentHeader`] entries, and
 * `n` to writable storage for one `size_t`.
 *
 * @param h    The reader.
 * @param out  Receives the headers, oldest first; may be null when `cap` is
 *             zero.
 * @param cap  The entries `out` holds.
 * @param n    Receives the number of headers moved into `out`.
 * @return     [`TcslogStatus::Ok`], or [`TcslogStatus::NullArgument`] for a
 *             null handle or `n`, or a null `out` with a non-zero `cap`.
 */
TcslogStatus tcslog_read_take_opened_headers(struct TcslogRead *h,
					     struct TcslogSegmentHeader *out,
					     uintptr_t cap,
					     uintptr_t *n);

/**
 * Closes a reader and releases it.
 *
 * # Safety
 *
 * `h` must come from [`tcslog_read_open`] and must not be used again
 * after this returns.
 *
 * @param h  The reader, or null, which is accepted and does nothing, as `free`
 *           does.
 */
void tcslog_read_close(struct TcslogRead *h);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* TCSLOG_H */
