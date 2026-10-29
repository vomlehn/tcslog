/* C++ interface to tcslog: the C ABI in tcslog.h, wrapped in classes
 * that close their handles themselves.
 *
 * The names are the Rust crate's: LogWrite, LogRead, WriteCallbacks,
 * Format, and methods write, read, flush, and the rest. Every method
 * returns a Status, as the C functions do, and nothing here throws, uses
 * RTTI, or allocates, so it builds with -fno-exceptions -fno-rtti and
 * suits a program with no heap to spare. What allocates underneath is
 * what tcslog.h documents: opening a log, clearing one, and retaining
 * segment headers when asked to.
 *
 * Header-only and C++11. Link against libtcslog_c as for tcslog.h.
 */

#ifndef TCSLOG_HPP
#define TCSLOG_HPP

#include <cstddef>
#include <cstdint>
#include <type_traits>

#include "tcslog.h"

namespace tcslog {

/* The C types, under the Rust crate's names. */
typedef TcslogStatus Status;
typedef TcslogMeta Meta;
typedef TcslogReadResult ReadResult;
typedef TcslogRecordMeta RecordMeta;
typedef TcslogSegmentHeader SegmentHeader;

/* A log's record layout: the Rust Format. */
struct Format {
	uint32_t tag;
	uint32_t fixed_len;

	/* Every record holds exactly n payload bytes and no data header. */
	static Format fixed(uint32_t n) {
		Format f = {TCSLOG_FORMAT_FIXED, n};
		return f;
	}
	/* Records vary in length, with a header giving it. */
	static Format variable_simple() {
		Format f = {TCSLOG_FORMAT_VARIABLE_SIMPLE, 0};
		return f;
	}
	/* As variable_simple, and each record also carries the time it was
	 * written and its position in the session. */
	static Format variable_ts_rc() {
		Format f = {TCSLOG_FORMAT_VARIABLE_TS_RC, 0};
		return f;
	}
};

/* What a writer calls back into: the Rust WriteCallbacks trait.
 *
 * Derive from it and define the methods wanted, with these signatures.
 * The methods are not virtual: LogWrite::open sees the handler's own
 * type, and installs only the methods that type defines itself, so one
 * left as it is here costs nothing per record. Returning non-zero from
 * send or record_complete makes the write that triggered it report
 * TCSLOG_STATUS_IO_ERROR.
 */
struct WriteCallbacks {
	/* A segment file at path is being handed over. When this returns
	 * there must be no file left at path and none matching the log's
	 * naming pattern. */
	int send(const char *path) {
		(void)path;
		return 0;
	}
	/* A record ended in the segment file open on fd, which the library
	 * owns: do not close it. -1 on a platform other than Unix. */
	int record_complete(int fd) {
		(void)fd;
		return 0;
	}
	/* The build-time timer resolution was too small for this machine
	 * and was widened to resolution_ns. */
	void timer_resolution_adjusted(uint64_t resolution_ns) {
		(void)resolution_ns;
	}
};

namespace detail {

/* Whether H defines name itself, rather than inheriting
 * WriteCallbacks' do-nothing one: &H::name then has H, not
 * WriteCallbacks, as its class. */
template <class H>
struct Overrides {
	static const bool send =
		!std::is_same<decltype(&H::send),
		              int (WriteCallbacks::*)(const char *)>::value;
	static const bool record_complete =
		!std::is_same<decltype(&H::record_complete),
		              int (WriteCallbacks::*)(int)>::value;
	static const bool timer_resolution_adjusted =
		!std::is_same<decltype(&H::timer_resolution_adjusted),
		              void (WriteCallbacks::*)(uint64_t)>::value;
};

template <class H>
int send(void *ctx, const char *path) {
	return static_cast<H *>(ctx)->send(path);
}

template <class H>
int record_complete(void *ctx, int fd) {
	return static_cast<H *>(ctx)->record_complete(fd);
}

template <class H>
void timer_resolution_adjusted(void *ctx, uint64_t resolution_ns) {
	static_cast<H *>(ctx)->timer_resolution_adjusted(resolution_ns);
}

}  // namespace detail

/* A writer: the Rust LogWrite. Closed until open() succeeds, closed again
 * by close() or when destroyed, and movable but not copyable, so one
 * handle is closed exactly once. */
class LogWrite {
public:
	LogWrite() : h_(nullptr) {}
	~LogWrite() { close(); }
	LogWrite(LogWrite &&other) : h_(other.h_) { other.h_ = nullptr; }
	LogWrite &operator=(LogWrite &&other) {
		if (this != &other) {
			close();
			h_ = other.h_;
			other.h_ = nullptr;
		}
		return *this;
	}
	LogWrite(const LogWrite &) = delete;
	LogWrite &operator=(const LogWrite &) = delete;

	/** Opens a log with no callbacks. Closes any log already open first.
	 *
	 * @param dir           The log's directory.
	 * @param prefix        What each segment file's name begins with.
	 * @param suffix        What each segment file's name ends with.
	 * @param seg_size_max  The most bytes a segment file may occupy, its
	 *                      own header included.
	 * @param format        The log's record layout.
	 * @return              TCSLOG_STATUS_OK, or why tcslog_write_open
	 *                      refused; the writer is then closed. */
	Status open(const char *dir, const char *prefix, const char *suffix,
	            uint32_t seg_size_max, Format format) {
		close();
		return tcslog_write_open(dir, prefix, suffix,
		                         seg_size_max, format.tag,
		                         format.fixed_len, nullptr, &h_);
	}

	/** Opens a log whose callbacks are handler's methods, installing
	 * only those H defines itself. Closes any log already open first.
	 *
	 * @param dir           The log's directory.
	 * @param prefix        What each segment file's name begins with.
	 * @param suffix        What each segment file's name ends with.
	 * @param seg_size_max  The most bytes a segment file may occupy, its
	 *                      own header included.
	 * @param format        The log's record layout.
	 * @param handler       The callbacks' object, derived from
	 *                      WriteCallbacks. It is not owned and must
	 *                      outlive the writer, as ctx must in C.
	 * @return              TCSLOG_STATUS_OK, or why tcslog_write_open
	 *                      refused; the writer is then closed. */
	template <class H>
	Status open(const char *dir, const char *prefix, const char *suffix,
	            uint32_t seg_size_max, Format format, H *handler) {
		static_assert(std::is_base_of<WriteCallbacks, H>::value,
		              "a handler derives from tcslog::WriteCallbacks");
		close();
		TcslogCallbacks cb;
		cb.send = detail::Overrides<H>::send ? &detail::send<H>
		                                     : nullptr;
		cb.record_complete =
			detail::Overrides<H>::record_complete
				? &detail::record_complete<H>
				: nullptr;
		cb.timer_resolution_adjusted =
			detail::Overrides<H>::timer_resolution_adjusted
				? &detail::timer_resolution_adjusted<H>
				: nullptr;
		cb.ctx = handler;
		return tcslog_write_open(dir, prefix, suffix,
		                         seg_size_max, format.tag,
		                         format.fixed_len, &cb, &h_);
	}

	bool is_open() const { return h_ != nullptr; }

	/* Writes len bytes from data as one record; *written, when given,
	 * is what the record occupied, its data header included. */
	Status write(const void *data, size_t len,
	             uint32_t *written = nullptr) {
		return tcslog_write_record(h_,
		                           static_cast<const uint8_t *>(data),
		                           len, written);
	}

	/** Writes a NUL-terminated string, without its NUL, as one record.
	 *
	 * @param msg      The string; null writes an empty record.
	 * @param written  When not null, set to the bytes the record
	 *                 occupied, its data header included.
	 * @return         TCSLOG_STATUS_OK, or why the write failed. */
	Status write_str(const char *msg, uint32_t *written = nullptr) {
		size_t len = 0;
		while (msg != nullptr && msg[len] != '\0') {
			len++;
		}
		return write(msg, len, written);
	}

	Status flush() { return tcslog_write_flush(h_); }
	Status clear() { return tcslog_write_clear(h_); }
	Status timer_resolution(uint64_t &out) {
		return tcslog_write_timer_resolution(h_, &out);
	}
	Status session_id(uint64_t &out) {
		return tcslog_write_session_id(h_, &out);
	}
	Status current_segment_id(uint64_t &out) {
		return tcslog_write_current_segment_id(h_, &out);
	}
	Status last_meta(RecordMeta &out) {
		return tcslog_write_last_meta(h_, &out);
	}

	/* Flushes and hands the last segment file to send, as dropping a
	 * Rust LogWrite does. Cannot report a failure; flush() first to
	 * know. Closing a closed writer does nothing. */
	void close() {
		tcslog_write_close(h_);
		h_ = nullptr;
	}

private:
	TcslogWrite *h_;
};

/* A reader: the Rust LogRead. Closed until open() succeeds, closed
 * again by close() or when destroyed, and movable but not copyable. */
class LogRead {
public:
	LogRead() : h_(nullptr) {}
	~LogRead() { close(); }
	LogRead(LogRead &&other) : h_(other.h_) { other.h_ = nullptr; }
	LogRead &operator=(LogRead &&other) {
		if (this != &other) {
			close();
			h_ = other.h_;
			other.h_ = nullptr;
		}
		return *this;
	}
	LogRead(const LogRead &) = delete;
	LogRead &operator=(const LogRead &) = delete;

	/* Opens a log for reading. Closes any log already open first. */
	Status open(const char *dir, const char *prefix, const char *suffix) {
		close();
		return tcslog_read_open(dir, prefix, suffix, &h_);
	}

	bool is_open() const { return h_ != nullptr; }

	/* Reads the next record into the cap bytes at buf. The rule is the
	 * Rust one: read again until TCSLOG_STATUS_EOF. */
	Status read(void *buf, size_t cap, ReadResult &result) {
		return tcslog_read_record(h_, static_cast<uint8_t *>(buf), cap,
		                          &result);
	}

	/* As read, for callers whose payloads are text. Not NUL-terminated:
	 * result.n says how many bytes are the record. */
	Status read_str(char *buf, size_t cap, ReadResult &result) {
		return read(buf, cap, result);
	}

	Status segments_opened(uint64_t &out) {
		return tcslog_read_segments_opened(h_, &out);
	}
	Status last_meta(RecordMeta &out) {
		return tcslog_read_last_meta(h_, &out);
	}

	/** Reports the header of the segment file being read.
	 *
	 * @param out      Filled with the header when a segment file is
	 *                 open.
	 * @param present  Set when a segment file is open, and cleared when
	 *                 none is.
	 * @return         TCSLOG_STATUS_OK, or why the query failed. */
	Status current_header(SegmentHeader &out, bool &present) {
		int p = 0;
		Status s = tcslog_read_current_header(h_, &out, &p);
		present = p != 0;
		return s;
	}

	Status collect_opened_headers(bool enable) {
		return tcslog_read_collect_opened_headers(h_, enable ? 1 : 0);
	}

	/* Moves up to cap retained headers into out; call until n < cap. */
	Status take_opened_headers_into(SegmentHeader *out, size_t cap,
	                                size_t &n) {
		return tcslog_read_take_opened_headers(h_, out, cap, &n);
	}

	void close() {
		tcslog_read_close(h_);
		h_ = nullptr;
	}

private:
	TcslogRead *h_;
};

/* The functions taking no handle. */

inline const char *status_str(Status s) { return tcslog_status_str(s); }

inline uint32_t segment_file_header_len() {
	return tcslog_segment_file_header_len();
}

inline void format_version(uint32_t &major, uint32_t &minor, uint32_t &patch) {
	tcslog_format_version(&major, &minor, &patch);
}

/* Renders a segment identifier into a buffer of any size; one too short
 * for TCSLOG_SEG_ID_STR_LEN characters and a NUL is refused. */
template <size_t N>
Status seg_id_format(uint64_t id, char (&buf)[N]) {
	return tcslog_seg_id_format(id, buf, N);
}

inline Status seg_id_parse(const char *text, uint64_t &out) {
	return tcslog_seg_id_parse(text, &out);
}

/* Renders a timestamp; the buffer needs TCSLOG_TIMESTAMP_STR_LEN + 1. */
template <size_t N>
Status format_timestamp(uint64_t ts, char (&buf)[N]) {
	return tcslog_format_timestamp(ts, buf, N);
}

}  // namespace tcslog

#endif /* TCSLOG_HPP */
