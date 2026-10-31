// Drives the C++ interface, tcslog.hpp, through the scenarios that
// matter to it: a round trip with a handler, a handler that overrides
// only some callbacks, a refusing send, the queries, moving a handle,
// and the text forms.
//
// It is built with -fno-exceptions -fno-rtti, which is what proves the
// header needs neither, and it replaces the global operator new with
// one that counts, which is what proves the wrapper allocates nothing:
// the library underneath allocates through malloc, not new, so any
// count here is the wrapper's own.
//
// Built and run by `make capi-test` from the repository root, and under
// the sanitizers by `make capi-memcheck`.

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <new>

#include <sys/stat.h>

#include "tcslog.hpp"

static int news = 0;

/** Replaces the global operator new with one that counts each call in
 * `news`, so the test can show the wrapper allocates nothing.
 *
 * @param n  The bytes wanted; zero is treated as one.
 * @return   The storage, from malloc. Aborts if there is none, as this
 *           test is built without exceptions to throw bad_alloc. */
void *operator new(std::size_t n) {
	news++;
	void *p = std::malloc(n == 0 ? 1 : n);
	if (p == nullptr) {
		std::abort();
	}
	return p;
}

void operator delete(void *p) noexcept { std::free(p); }
void operator delete(void *p, std::size_t) noexcept { std::free(p); }

static int failures = 0;

/** Reports a check's outcome, counting it in `failures` if it failed,
 * and carries on.
 *
 * @param ok    Whether the check passed.
 * @param what  What was checked, for the report. */
static void check(bool ok, const char *what) {
	std::printf("%s %s\n", ok ? "ok      " : "not ok  ", what);
	if (!ok) {
		failures++;
	}
}

static const char *PREFIX = "seg-";
static const char *SUFFIX = ".log";

static bool subdir(const char *base, const char *name, char (&out)[4096]) {
	int n = std::snprintf(out, sizeof out, "%s/%s", base, name);
	return n > 0 && static_cast<size_t>(n) < sizeof out &&
	       mkdir(out, 0755) == 0;
}

// Counts every callback, and refuses send when asked to.
struct Counting : tcslog::WriteCallbacks {
	int sends = 0;
	int completes = 0;
	bool refuse = false;

	int send(const char *path) {
		(void)path;
		sends++;
		return refuse ? 1 : 0;
	}
	int record_complete(int fd) {
		(void)fd;
		completes++;
		return 0;
	}
};

// Overrides send alone, so record_complete must not be installed.
struct SendOnly : tcslog::WriteCallbacks {
	int sends = 0;
	int send(const char *path) {
		(void)path;
		sends++;
		return 0;
	}
};

static_assert(tcslog::detail::Overrides<SendOnly>::send,
              "send is SendOnly's own");
static_assert(!tcslog::detail::Overrides<SendOnly>::record_complete,
              "record_complete is inherited, and so not installed");

/** Writes records through a handler, moves the writer, and reads the
 * records back.
 *
 * @param base  The directory in which the scenario makes its own
 *              subdirectory. */
static void round_trip(const char *base) {
	std::printf("-- round trip through the C++ interface\n");
	char dir[4096];
	if (!subdir(base, "round-trip", dir)) {
		failures++;
		return;
	}
	const uint32_t seg = tcslog::segment_file_header_len() + 4096;
	static const char *msgs[] = {"attitude nominal", "battery 71 percent"};

	Counting handler;
	{
		tcslog::LogWrite w;
		check(!w.is_open(), "a writer is closed until opened");
		check(w.open(dir, PREFIX, SUFFIX, seg,
		             tcslog::Format::variable_ts_rc(), &handler) ==
		          TCSLOG_STATUS_OK,
		      "a writer opens with a handler");
		for (const char *m : msgs) {
			uint32_t written = 0;
			check(w.write_str(m, &written) == TCSLOG_STATUS_OK &&
			          written == std::strlen(m) + 18,
			      "a record is written");
		}
		tcslog::RecordMeta meta;
		check(w.last_meta(meta) == TCSLOG_STATUS_OK &&
		          meta.record_count == 2,
		      "the last record's metadata is reported");
		uint64_t session = 0;
		check(w.session_id(session) == TCSLOG_STATUS_OK && session != 0,
		      "the session identifier is reported");

		// Moving hands the log over; only the destination closes it.
		tcslog::LogWrite moved(static_cast<tcslog::LogWrite &&>(w));
		check(!w.is_open() && moved.is_open(),
		      "a move transfers the handle");
	}
	check(handler.completes == 2, "record_complete ran once per record");
	check(handler.sends == 1,
	      "destroying the writer handed the open file over");

	tcslog::LogRead r;
	check(r.open(dir, PREFIX, SUFFIX) == TCSLOG_STATUS_OK,
	      "a reader opens");
	char buf[256];
	tcslog::ReadResult res;
	int count = 0;
	for (;;) {
		tcslog::Status s = r.read(buf, sizeof buf, res);
		if (s == TCSLOG_STATUS_EOF) {
			break;
		}
		if (s != TCSLOG_STATUS_OK) {
			check(false, "a record is read");
			break;
		}
		check(count < 2 && res.n == std::strlen(msgs[count]) &&
		          std::memcmp(buf, msgs[count], res.n) == 0,
		      "the record read back is the record written");
		count++;
	}
	check(count == 2, "every record written was read back");
}

/** Writes through a handler that overrides send alone, which must still
 * be called for each filled file and at close.
 *
 * @param base  The directory in which the scenario makes its own
 *              subdirectory. */
static void partial_handler(const char *base) {
	std::printf("-- a handler overriding send alone\n");
	char dir[4096];
	if (!subdir(base, "send-only", dir)) {
		failures++;
		return;
	}
	SendOnly handler;
	tcslog::LogWrite w;
	check(w.open(dir, PREFIX, SUFFIX,
	             tcslog::segment_file_header_len() + 32,
	             tcslog::Format::variable_simple(), &handler) ==
	          TCSLOG_STATUS_OK,
	      "a writer opens");
	for (int i = 0; i < 12; i++) {
		w.write("record", 6);
	}
	w.close();
	check(handler.sends >= 2, "send ran for each filled file and at close");
}

/** Writes through a handler whose send refuses, which the write that
 * triggered it must report as an I/O error.
 *
 * @param base  The directory in which the scenario makes its own
 *              subdirectory. */
static void refusing_send(const char *base) {
	std::printf("-- a send that refuses\n");
	char dir[4096];
	if (!subdir(base, "refuse", dir)) {
		failures++;
		return;
	}
	Counting handler;
	handler.refuse = true;
	tcslog::LogWrite w;
	w.open(dir, PREFIX, SUFFIX, tcslog::segment_file_header_len() + 32,
	       tcslog::Format::variable_simple(), &handler);
	tcslog::Status s = TCSLOG_STATUS_OK;
	for (int i = 0; i < 32 && s == TCSLOG_STATUS_OK; i++) {
		s = w.write("record", 6);
	}
	check(s == TCSLOG_STATUS_IO_ERROR,
	      "the write that triggered it reports an I/O error");
}

/** Checks the text forms of identifiers and timestamps, and the
 * segment headers a reader retains, taken a few at a time.
 *
 * @param base  The directory in which the scenario makes its own
 *              subdirectory. */
static void headers_and_text(const char *base) {
	std::printf("-- headers and text forms\n");
	char dir[4096];
	if (!subdir(base, "headers", dir)) {
		failures++;
		return;
	}
	uint64_t session = 0;
	{
		tcslog::LogWrite w;
		w.open(dir, PREFIX, SUFFIX,
		       tcslog::segment_file_header_len() + 32,
		       tcslog::Format::fixed(8));
		for (int i = 0; i < 20; i++) {
			w.write("01234567", 8);
		}
		w.session_id(session);
	}
	char text[TCSLOG_SEG_ID_STR_LEN + 1];
	uint64_t parsed = 0;
	check(tcslog::seg_id_format(session, text) == TCSLOG_STATUS_OK &&
	          tcslog::seg_id_parse(text, parsed) == TCSLOG_STATUS_OK &&
	          parsed == session,
	      "an identifier renders and parses back");
	char too_small[4];
	check(tcslog::seg_id_format(session, too_small) ==
	          TCSLOG_STATUS_BUFFER_TOO_SMALL,
	      "too small a buffer is refused");
	char when[TCSLOG_TIMESTAMP_STR_LEN + 1];
	check(tcslog::format_timestamp(0, when) == TCSLOG_STATUS_OK &&
	          std::strcmp(when, "1970-01-01T00:00:00.000000000Z") == 0,
	      "the epoch renders");

	tcslog::LogRead r;
	r.open(dir, PREFIX, SUFFIX);
	tcslog::SegmentHeader header;
	bool present = true;
	r.current_header(header, present);
	check(!present, "no header before the first read");
	r.collect_opened_headers(true);
	char buf[16];
	tcslog::ReadResult res;
	while (r.read(buf, sizeof buf, res) != TCSLOG_STATUS_EOF) {
	}
	tcslog::SegmentHeader headers[3];
	size_t n = 0, taken = 0;
	do {
		r.take_opened_headers_into(headers, 3, n);
		for (size_t i = 0; i < n; i++) {
			check(headers[i].session_id == session &&
			          headers[i].fixed_len == 8,
			      "a retained header is this log's");
		}
		taken += n;
	} while (n == 3);
	uint64_t opened = 0;
	r.segments_opened(opened);
	check(taken == opened && taken > 3,
	      "every header was taken, a few at a time");
}

/** Runs every scenario and reports whether every check passed and the
 * C++ interface allocated nothing.
 *
 * @param argc  The number of arguments, which must be 2.
 * @param argv  The program's name and an existing directory in which
 *              each scenario makes its own subdirectory.
 * @return      Zero if every check passed, 1 if any failed, or 2 for a
 *              usage error. */
int main(int argc, char **argv) {
	if (argc != 2) {
		std::fprintf(stderr, "usage: %s <directory>\n", argv[0]);
		return 2;
	}
	const int before = news;
	round_trip(argv[1]);
	partial_handler(argv[1]);
	refusing_send(argv[1]);
	headers_and_text(argv[1]);
	check(news == before, "the C++ interface allocated nothing");

	if (failures == 0) {
		std::printf("OK: the C++ interface passed every check\n");
		return 0;
	}
	std::printf("FAIL: %d check(s) failed\n", failures);
	return 1;
}
