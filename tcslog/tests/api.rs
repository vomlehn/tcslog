//! Tests that reach the library the way a caller does, through its
//! public API, with faults injected directly rather than through the
//! support binaries.
//!
//! The suite under `test/` covers the same ground from the outside, by
//! damaging bytes on disk and comparing the tools' output against stored
//! files. These reach places that suite cannot: a callback's return
//! value, the exact size of every segment file, and the state a writer
//! is left in by a failure.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tempfile::TempDir;

use tcslog::{
    Format, LogError, LogRead, LogWrite, Meta, RecSize, SegId, SegmentHeader, WriteCallbacks,
    SEGMENT_FILE_HEADER_LEN,
};

const PREFIX: &str = "seg-";
const SUFFIX: &str = ".log";

/// A log directory and the geometry of the log in it.
struct Log {
    dir: TempDir,
    seg_size_max: u32,
}

impl Log {
    /// Creates an empty log directory whose segment files will have a
    /// data section of `data_size` bytes.
    fn new(data_size: u32) -> Self {
        Self {
            dir: tempfile::tempdir().expect("a temporary directory"),
            seg_size_max: SEGMENT_FILE_HEADER_LEN + data_size,
        }
    }

    /// The directory name, as the API takes it.
    fn path(&self) -> &str {
        self.dir.path().to_str().expect("a UTF-8 temporary path")
    }

    /// Opens a writer on this log with callbacks that do nothing, so
    /// that the segment files stay put for a reader to find.
    fn writer(&self, format: Format) -> LogWrite {
        LogWrite::new(self.path(), PREFIX, SUFFIX, self.seg_size_max, format, ())
            .expect("a writer on a fresh directory")
    }

    /// Writes one session of `payloads` and closes it, so that every
    /// byte has reached the directory.
    fn write_session(&self, format: Format, payloads: &[Vec<u8>]) {
        let mut log = self.writer(format);
        for payload in payloads {
            log.write(payload).expect("a payload the format accepts");
        }
        drop(log);
    }

    /// This log's segment files, oldest first. The identifier is
    /// fixed-width zero-filled hexadecimal, so sorting the names sorts
    /// the files into the order they were written.
    fn segment_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = fs::read_dir(self.dir.path())
            .expect("a readable directory")
            .map(|e| e.expect("a readable entry").path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(PREFIX) && n.ends_with(SUFFIX))
            })
            .collect();
        files.sort();
        files
    }

    /// The header of every segment file, oldest first.
    fn headers(&self) -> Vec<SegmentHeader> {
        self.segment_files()
            .iter()
            .map(|p| {
                let mut f = File::open(p).expect("an openable segment file");
                SegmentHeader::read_from(&mut f).expect("a valid segment header")
            })
            .collect()
    }

    /// Opens a reader on this log.
    fn reader(&self) -> Result<LogRead, LogError> {
        LogRead::new(self.path(), PREFIX, SUFFIX)
    }

    /// Reads the whole log with a buffer large enough for every record.
    fn read_all(&self) -> Outcome {
        self.read_with(4096)
    }

    /// Reads the whole log into a buffer of `buf_len` bytes, collecting
    /// what came back and what was reported.
    fn read_with(&self, buf_len: usize) -> Outcome {
        let mut log = self.reader().expect("a readable log");
        let mut out = Outcome::default();
        let mut buf = vec![0u8; buf_len];
        loop {
            match log.read(&mut buf) {
                Ok(result) => {
                    out.payloads.push(buf[..result.n as usize].to_vec());
                    out.metas.push(result.meta);
                }
                Err(LogError::Eof) => break,
                Err(LogError::SessionEnd) => out.session_ends += 1,
                Err(LogError::ReadTruncated { lost, n }) => {
                    out.truncations += 1;
                    out.lost += lost;
                    if n > 0 {
                        out.partials.push(buf[..n as usize].to_vec());
                    }
                }
                Err(LogError::ReadOverflow(n)) => {
                    out.overflows += 1;
                    out.overflow_counts.push(n);
                    out.partials.push(buf[..n as usize].to_vec());
                }
                // Damage is reported through the variants above. Anything
                // else is a reader failing in a way nothing here asked
                // for, so it fails the test rather than being tallied.
                Err(e) => panic!("unexpected error from read: {e}"),
            }
        }
        out.segments_opened = log.segments_opened();
        out
    }
}

/// Everything a full read of a log reported.
#[derive(Default)]
struct Outcome {
    payloads: Vec<Vec<u8>>,
    metas: Vec<Meta>,
    partials: Vec<Vec<u8>>,
    session_ends: usize,
    truncations: usize,
    overflows: usize,
    overflow_counts: Vec<RecSize>,
    lost: u64,
    segments_opened: u64,
}

/// Payloads that name themselves, so that a record recovered from a
/// damaged log can be identified rather than merely counted.
fn payloads(count: usize, len: usize) -> Vec<Vec<u8>> {
    (0..count)
        .map(|i| {
            let mut buf = format!("#{} ", i + 1).into_bytes();
            while buf.len() < len {
                buf.extend_from_slice(b"123456789 ");
            }
            buf.truncate(len);
            buf
        })
        .collect()
}

/// Cuts a segment file down to `data_bytes` bytes of data section,
/// leaving its header intact.
fn truncate_to(path: &Path, data_bytes: u64) {
    let f = OpenOptions::new()
        .write(true)
        .open(path)
        .expect("a writable segment file");
    f.set_len(u64::from(SEGMENT_FILE_HEADER_LEN) + data_bytes)
        .expect("a truncatable segment file");
}

/// Overwrites a segment file's eight-byte type field, so that it cannot
/// be opened at all. The file stays in place, so its neighbours' sequence
/// numbers still step over it.
fn corrupt_magic(path: &Path) {
    use std::io::{Seek, SeekFrom, Write};
    let mut f = OpenOptions::new()
        .write(true)
        .open(path)
        .expect("a writable segment file");
    f.seek(SeekFrom::Start(0)).expect("a seekable segment file");
    f.write_all(&[0u8; 8]).expect("a writable header");
}

// ---------------------------------------------------------------- Fixed

#[test]
fn fixed_records_within_one_segment_file() {
    // Two 4-byte records per 8-byte data section, so no record spans.
    let log = Log::new(8);
    let written = payloads(6, 4);
    log.write_session(Format::Fixed(4), &written);

    let out = log.read_all();
    assert_eq!(out.payloads, written);
    assert_eq!(out.truncations, 0);
    assert_eq!(out.metas.iter().filter(|m| **m == Meta::Fixed).count(), 6);
}

#[test]
fn fixed_records_spanning_one_segment_boundary() {
    // 5-byte records in an 8-byte data section: every other record
    // straddles a boundary, and none reaches a third file.
    let log = Log::new(8);
    let written = payloads(6, 5);
    log.write_session(Format::Fixed(5), &written);

    let out = log.read_all();
    assert_eq!(out.payloads, written);
    assert_eq!(out.truncations, 0);
}

#[test]
fn fixed_records_spanning_several_segment_files() {
    // A 20-byte record in an 8-byte data section covers three files.
    let log = Log::new(8);
    let written = payloads(4, 20);
    log.write_session(Format::Fixed(20), &written);

    assert!(log.segment_files().len() > 6, "expected many segment files");
    let out = log.read_all();
    assert_eq!(out.payloads, written);
    assert_eq!(out.truncations, 0);
}

#[test]
fn fixed_rejects_every_wrong_payload_length() {
    let log = Log::new(16);
    let mut w = log.writer(Format::Fixed(4));
    assert!(matches!(w.write(b"abc"), Err(LogError::FixedLenMismatch)));
    assert!(matches!(w.write(b"abcde"), Err(LogError::FixedLenMismatch)));
    // Zero is a wrong length like any other, not an error of its own.
    assert!(matches!(w.write(b""), Err(LogError::FixedLenMismatch)));
    assert!(w.write(b"abcd").is_ok());
}

#[test]
fn a_log_may_not_use_fixed_zero() {
    let log = Log::new(16);
    assert!(matches!(
        LogWrite::new(
            log.path(),
            PREFIX,
            SUFFIX,
            log.seg_size_max,
            Format::Fixed(0),
            (),
        ),
        Err(LogError::FixedLenMismatch)
    ));
}

// ------------------------------------------------------------- Variable

/// The two variable formats, which every variable-length case runs
/// against both of.
const VARIABLE_FORMATS: [Format; 2] = [Format::VariableSimple, Format::VariableTsRc];

#[test]
fn zero_length_records_round_trip_in_both_variable_formats() {
    for format in VARIABLE_FORMATS {
        let log = Log::new(40);
        let written = vec![Vec::new(), b"between".to_vec(), Vec::new()];
        log.write_session(format, &written);

        let out = log.read_all();
        assert_eq!(out.payloads, written, "format {format:?}");
        assert_eq!(out.truncations, 0, "format {format:?}");
    }
}

#[test]
fn records_within_one_segment_file_round_trip_in_both_variable_formats() {
    for format in VARIABLE_FORMATS {
        // A data section with room for several whole records.
        let log = Log::new(400);
        let written = payloads(8, 12);
        log.write_session(format, &written);

        let out = log.read_all();
        assert_eq!(out.payloads, written, "format {format:?}");
        assert_eq!(out.truncations, 0, "format {format:?}");
    }
}

#[test]
fn very_long_records_spanning_many_segment_files_round_trip() {
    for format in VARIABLE_FORMATS {
        // Wider than a VariableTsRc data header, so both formats fit.
        let log = Log::new(24);
        let written = payloads(3, 200);
        log.write_session(format, &written);

        assert!(
            log.segment_files().len() > 10,
            "format {format:?}: expected many segment files"
        );
        let out = log.read_all();
        assert_eq!(out.payloads, written, "format {format:?}");
        assert_eq!(out.truncations, 0, "format {format:?}");
    }
}

#[test]
fn variable_tsrc_numbers_its_records_from_one() {
    let log = Log::new(400);
    let written = payloads(5, 6);
    log.write_session(Format::VariableTsRc, &written);

    let out = log.read_all();
    assert_eq!(out.payloads, written);
    let counts: Vec<u64> = out
        .metas
        .iter()
        .map(|m| match m {
            Meta::VariableTsRc(_, rc) => *rc,
            other => panic!("expected VariableTsRc metadata, got {other:?}"),
        })
        .collect();
    assert_eq!(counts, vec![1, 2, 3, 4, 5]);
}

#[test]
fn last_meta_reports_what_was_stored() {
    let log = Log::new(400);
    let mut w = log.writer(Format::VariableTsRc);
    w.write(b"first").expect("a writable payload");
    let first = w.last_meta();
    w.write(b"second").expect("a writable payload");
    let second = w.last_meta();
    drop(w);

    let (ts1, rc1) = match first {
        Meta::VariableTsRc(ts, rc) => (ts, rc),
        other => panic!("expected VariableTsRc metadata, got {other:?}"),
    };
    let (ts2, rc2) = match second {
        Meta::VariableTsRc(ts, rc) => (ts, rc),
        other => panic!("expected VariableTsRc metadata, got {other:?}"),
    };
    assert_eq!((rc1, rc2), (1, 2));
    assert!(ts1 > 0 && ts2 >= ts1);

    // What `last_meta` reported is what a reader finds on disk.
    let out = log.read_all();
    assert_eq!(out.metas, vec![first, second]);
}

// -------------------------------------------------- Segment file sizing

#[test]
fn every_segment_file_but_a_sessions_last_is_exactly_seg_size_max() {
    for format in VARIABLE_FORMATS {
        // Wider than a VariableTsRc data header, so both formats fit,
        // and narrower than a record, so the log rolls repeatedly.
        let log = Log::new(24);
        log.write_session(format, &payloads(12, 9));

        let files = log.segment_files();
        assert!(files.len() > 3, "format {format:?}: too few segment files");
        let (last, full) = files.split_last().expect("at least one segment file");
        for path in full {
            let len = fs::metadata(path).expect("a segment file").len();
            assert_eq!(
                len,
                u64::from(log.seg_size_max),
                "format {format:?}: {} is not exactly seg_size_max",
                path.display()
            );
        }
        let last_len = fs::metadata(last).expect("a segment file").len();
        assert!(
            last_len <= u64::from(log.seg_size_max),
            "format {format:?}: the last segment file is over the maximum"
        );
    }
}

#[test]
fn a_data_header_straddles_a_boundary_that_falls_inside_it() {
    // A record of 9 payload bytes plus a 4-byte data header is 13 bytes,
    // against a data section of 10: the record start walks across the
    // boundary until a header falls astride one. Nothing is padded to
    // avoid it, which is what lets every file be exactly full.
    let log = Log::new(10);
    let format = Format::VariableSimple;
    let record_total = u64::from(format.data_header_len()) + 9;
    log.write_session(format, &payloads(12, 9));

    let header_len = u64::from(format.data_header_len());
    // A segment whose `remaining` leaves fewer than a whole data header
    // consumed in the previous file is one the header straddles.
    let straddled = log.headers().iter().any(|h| {
        let consumed = record_total.saturating_sub(h.remaining);
        h.remaining > 0 && consumed > 0 && consumed < header_len
    });
    assert!(
        straddled,
        "no data header straddled a boundary; headers: {:?}",
        log.headers()
            .iter()
            .map(|h| h.remaining)
            .collect::<Vec<_>>()
    );

    // The log is intact, so every record still reads back.
    let out = log.read_all();
    assert_eq!(out.payloads, payloads(12, 9));
    assert_eq!(out.truncations, 0);
}

#[test]
fn sequence_numbers_start_at_zero_and_step_by_one() {
    let log = Log::new(10);
    log.write_session(Format::VariableSimple, &payloads(10, 9));

    let sequences: Vec<u64> = log.headers().iter().map(|h| h.sequence.as_u64()).collect();
    let expected: Vec<u64> = (0..sequences.len() as u64).collect();
    assert_eq!(sequences, expected);

    // Every segment file of a session carries the first one's ID.
    let headers = log.headers();
    let session = headers[0].segment_id;
    assert!(headers.iter().all(|h| h.session_id == session));
}

#[test]
fn seg_size_max_must_leave_room_for_a_data_header() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().to_str().expect("a UTF-8 path");
    for format in [
        Format::Fixed(1),
        Format::VariableSimple,
        Format::VariableTsRc,
    ] {
        let floor = SEGMENT_FILE_HEADER_LEN + format.data_header_len();
        for too_small in [0, floor] {
            assert!(
                matches!(
                    LogWrite::new(path, PREFIX, SUFFIX, too_small, format, ()),
                    Err(LogError::SegSizeTooSmall)
                ),
                "format {format:?} accepted seg_size_max {too_small}"
            );
        }
        // One byte more than the floor is enough.
        assert!(
            LogWrite::new(path, PREFIX, SUFFIX, floor + 1, format, ()).is_ok(),
            "format {format:?} refused the smallest workable seg_size_max"
        );
    }
}

// ------------------------------------------------------------- Callbacks

/// The callbacks as plain function pointers.
///
/// `WriteCallbacks` was a structure of these three fields before it
/// became a trait, and these tests are what wanted it that way: each
/// installs one or two free functions, which count in statics because
/// the suite runs in parallel and a `fn` has nowhere to put per-test
/// state. The tests that do have state use a handler with fields, as
/// `a_handler_carries_its_own_context` does.
#[derive(Clone, Copy)]
struct Fns {
    record_complete: fn(&mut File) -> io::Result<()>,
    send: fn(&Path) -> io::Result<()>,
    timer_resolution_adjusted: fn(u64),
}

impl Default for Fns {
    /// Callbacks that do nothing, including the resolution report: a
    /// test that is not about the report does not want it on stderr.
    fn default() -> Self {
        Self {
            record_complete: |_| Ok(()),
            send: |_| Ok(()),
            timer_resolution_adjusted: |_| {},
        }
    }
}

impl WriteCallbacks for Fns {
    fn record_complete(&mut self, file: &mut File) -> io::Result<()> {
        (self.record_complete)(file)
    }

    fn send(&mut self, path: &Path) -> io::Result<()> {
        (self.send)(path)
    }

    fn timer_resolution_adjusted(&mut self, resolution_ns: u64) {
        (self.timer_resolution_adjusted)(resolution_ns);
    }
}

static ROLL_SENDS: AtomicUsize = AtomicUsize::new(0);
static ROLL_COMPLETES: AtomicUsize = AtomicUsize::new(0);

// The counting callbacks below keep the fallible signatures
// `WriteCallbacks` declares, so that they can be stored in `Fns`.
#[allow(clippy::unnecessary_wraps)]
fn roll_send(_p: &Path) -> io::Result<()> {
    ROLL_SENDS.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

#[allow(clippy::unnecessary_wraps)]
fn roll_complete(_f: &mut File) -> io::Result<()> {
    ROLL_COMPLETES.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

#[test]
fn send_runs_once_per_segment_file_with_data_and_complete_once_per_record() {
    let log = Log::new(10);
    let written = payloads(12, 9);
    {
        let mut w = LogWrite::new(
            log.path(),
            PREFIX,
            SUFFIX,
            log.seg_size_max,
            Format::VariableSimple,
            Fns {
                record_complete: roll_complete,
                send: roll_send,
                ..Fns::default()
            },
        )
        .expect("a writer on a fresh directory");
        for payload in &written {
            w.write(payload).expect("a writable payload");
        }
        // Every file but the one still open has been sent by now; the
        // last one is sent by the drop below.
        assert_eq!(
            ROLL_SENDS.load(Ordering::Relaxed),
            log.segment_files().len() - 1,
            "a filled segment file went unsent"
        );
    }

    // Nothing here deletes the files, so the count on disk is the number
    // of segment files the writer produced, every one of which holds
    // data.
    assert_eq!(
        ROLL_SENDS.load(Ordering::Relaxed),
        log.segment_files().len(),
        "send did not run once per segment file holding data"
    );
    assert_eq!(
        ROLL_COMPLETES.load(Ordering::Relaxed),
        written.len(),
        "record_complete did not run once per record"
    );
}

/// Counts what it was asked to do, in itself rather than in a static.
///
/// The test above has to count in `AtomicUsize`es because a
/// `Fns` field is a bare `fn` with nowhere to put a context,
/// and the suite runs in parallel so one test's counters would reach
/// another's. A handler needs neither: each writer owns its own.
struct Counting<'a> {
    sends: usize,
    completes: usize,
    widenings: usize,
    /// Borrowed, so the last file -- handed over by the writer's own
    /// drop -- is still counted somewhere the test can read.
    sends_after_drop: &'a mut usize,
}

impl WriteCallbacks for Counting<'_> {
    fn record_complete(&mut self, _f: &mut File) -> io::Result<()> {
        self.completes += 1;
        Ok(())
    }

    fn send(&mut self, _p: &Path) -> io::Result<()> {
        self.sends += 1;
        *self.sends_after_drop += 1;
        Ok(())
    }

    fn timer_resolution_adjusted(&mut self, _ns: u64) {
        self.widenings += 1;
    }
}

#[test]
fn a_handler_carries_its_own_context() {
    let log = Log::new(10);
    let written = payloads(12, 9);
    let mut sends_after_drop = 0;
    {
        let mut w = LogWrite::new(
            log.path(),
            PREFIX,
            SUFFIX,
            log.seg_size_max,
            Format::VariableSimple,
            Counting {
                sends: 0,
                completes: 0,
                widenings: 0,
                sends_after_drop: &mut sends_after_drop,
            },
        )
        .expect("a writer on a fresh directory");
        for payload in &written {
            w.write(payload).expect("a writable payload");
        }

        // The context is reachable while the writer is open, and holds
        // what this writer did rather than what every writer did.
        let seen = w.handler();
        assert_eq!(
            seen.completes,
            written.len(),
            "record_complete did not run once per record"
        );
        assert_eq!(
            seen.sends,
            log.segment_files().len() - 1,
            "a filled segment file went unsent"
        );
        assert_eq!(seen.widenings, 0, "the timer resolution needed no widening");

        // And it is reachable mutably, which is what lets a handler be
        // reset or re-aimed between records.
        w.handler_mut().completes = 0;
        assert_eq!(w.handler().completes, 0);
    }

    // The drop handed over the file that was still open, and the
    // borrowed counter saw it where the handler's own field could no
    // longer be read.
    assert_eq!(
        sends_after_drop,
        log.segment_files().len(),
        "the file still open at the drop was not handed over"
    );
}

#[test]
fn a_borrowed_handler_stays_with_the_caller() {
    // The handler itself is the caller's, lent to the writer, so it is
    // readable once the writer is gone -- which is when the last
    // segment file has been handed over.
    struct Counting {
        sends: usize,
    }

    impl WriteCallbacks for Counting {
        fn send(&mut self, _p: &Path) -> io::Result<()> {
            self.sends += 1;
            Ok(())
        }
    }

    let log = Log::new(10);
    let written = payloads(12, 9);
    let mut handler = Counting { sends: 0 };
    {
        let mut w = LogWrite::new(
            log.path(),
            PREFIX,
            SUFFIX,
            log.seg_size_max,
            Format::VariableSimple,
            &mut handler,
        )
        .expect("a writer on a fresh directory");
        for payload in &written {
            w.write(payload).expect("a writable payload");
        }
    }
    assert_eq!(
        handler.sends,
        log.segment_files().len(),
        "a borrowed handler did not see every file, the drop's included"
    );
}

#[test]
fn the_unit_handler_is_no_callbacks_at_all() {
    // `()` says what `WriteCallbacks::default()` said, in a form that
    // has to be written down: filled files are left in the directory.
    let log = Log::new(10);
    let written = payloads(12, 9);
    {
        let mut w = LogWrite::new(
            log.path(),
            PREFIX,
            SUFFIX,
            log.seg_size_max,
            Format::VariableSimple,
            (),
        )
        .expect("a writer on a fresh directory");
        for payload in &written {
            w.write(payload).expect("a writable payload");
        }
    }
    assert!(
        !log.segment_files().is_empty(),
        "a log with no send callback should have kept its segment files"
    );

    // And they are readable, so nothing about the log itself differs.
    let mut r = LogRead::new(log.path(), PREFIX, SUFFIX).expect("a readable log");
    let mut buf = [0u8; 64];
    assert!(r.read(&mut buf).is_ok(), "the log did not read back");
}

/// A handler that reports a failure, which must reach the caller of the
/// write that triggered it rather than being swallowed.
struct Refusing;

impl WriteCallbacks for Refusing {
    fn send(&mut self, _p: &Path) -> io::Result<()> {
        Err(io::Error::other("the downlink queue is full"))
    }
}

#[test]
fn a_handler_that_refuses_a_file_stops_the_write() {
    let log = Log::new(10);
    let mut w = LogWrite::new(
        log.path(),
        PREFIX,
        SUFFIX,
        log.seg_size_max,
        Format::VariableSimple,
        Refusing,
    )
    .expect("a writer on a fresh directory");

    // Writing until a segment file fills is what reaches `send`. A
    // refusal there means the storage bound has stopped holding, so it
    // is reported rather than hidden.
    let mut outcome = Ok(0);
    for _ in 0..16 {
        outcome = w.write(b"0123456789");
        if outcome.is_err() {
            break;
        }
    }
    assert!(
        matches!(outcome, Err(LogError::IoError(_))),
        "a refusing send was swallowed: {outcome:?}"
    );
}

static DROP_SENDS: AtomicUsize = AtomicUsize::new(0);

#[allow(clippy::unnecessary_wraps)]
fn drop_send(_p: &Path) -> io::Result<()> {
    DROP_SENDS.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

#[test]
fn drop_sends_the_segment_file_still_being_written() {
    let log = Log::new(4096);
    {
        let mut w = LogWrite::new(
            log.path(),
            PREFIX,
            SUFFIX,
            log.seg_size_max,
            Format::VariableSimple,
            Fns {
                send: drop_send,
                ..Fns::default()
            },
        )
        .expect("a writer on a fresh directory");
        w.write(b"one record, nowhere near a roll")
            .expect("a writable payload");
        // The data section is nowhere near full, so nothing has rolled
        // and nothing has been sent.
        assert_eq!(DROP_SENDS.load(Ordering::Relaxed), 0);
    }
    assert_eq!(
        DROP_SENDS.load(Ordering::Relaxed),
        1,
        "drop did not send the segment file holding the last records"
    );
}

static EXISTING_SENDS: AtomicUsize = AtomicUsize::new(0);

#[allow(clippy::unnecessary_wraps)]
fn existing_send(_p: &Path) -> io::Result<()> {
    EXISTING_SENDS.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

#[test]
fn a_new_writer_hands_over_what_it_finds_rather_than_appending() {
    let log = Log::new(10);
    log.write_session(Format::VariableSimple, &payloads(8, 9));
    let existing = log.segment_files().len();
    assert!(existing > 1);

    let w = LogWrite::new(
        log.path(),
        PREFIX,
        SUFFIX,
        log.seg_size_max,
        Format::VariableSimple,
        Fns {
            send: existing_send,
            ..Fns::default()
        },
    )
    .expect("a writer on a directory holding a log");
    assert_eq!(
        EXISTING_SENDS.load(Ordering::Relaxed),
        existing,
        "a pre-existing segment file was not handed over"
    );
    // It started a session of its own rather than continuing the one it
    // found.
    let headers = log.headers();
    assert_ne!(w.session_id(), headers[0].session_id);
}

static FAILING_COMPLETES: AtomicUsize = AtomicUsize::new(0);

fn failing_complete(_f: &mut File) -> io::Result<()> {
    FAILING_COMPLETES.fetch_add(1, Ordering::Relaxed);
    Err(io::Error::other("record_complete refused"))
}

#[test]
fn a_failing_record_complete_propagates_and_leaves_the_writer_usable() {
    let log = Log::new(4096);
    {
        let mut w = LogWrite::new(
            log.path(),
            PREFIX,
            SUFFIX,
            log.seg_size_max,
            Format::VariableSimple,
            Fns {
                record_complete: failing_complete,
                ..Fns::default()
            },
        )
        .expect("a writer on a fresh directory");
        assert!(matches!(w.write(b"first"), Err(LogError::IoError(_))));
        // The bytes reached the file before the callback ran, and the
        // writer is still usable afterwards.
        assert!(matches!(w.write(b"second"), Err(LogError::IoError(_))));
        assert_eq!(FAILING_COMPLETES.load(Ordering::Relaxed), 2);
    }
    let out = log.read_all();
    assert_eq!(out.payloads, vec![b"first".to_vec(), b"second".to_vec()]);
}

fn failing_send(_p: &Path) -> io::Result<()> {
    Err(io::Error::other("send refused"))
}

#[test]
fn a_failing_send_propagates_from_the_roll_that_invoked_it() {
    let log = Log::new(8);
    let mut w = LogWrite::new(
        log.path(),
        PREFIX,
        SUFFIX,
        log.seg_size_max,
        Format::Fixed(4),
        Fns {
            send: failing_send,
            ..Fns::default()
        },
    )
    .expect("a writer on a fresh directory");
    // Two 4-byte records fill the 8-byte data section; the third forces
    // the roll whose send fails.
    assert!(w.write(b"aaaa").is_ok());
    assert!(w.write(b"bbbb").is_ok());
    assert!(matches!(w.write(b"cccc"), Err(LogError::IoError(_))));
}

// ------------------------------------------------- Writer fault recovery

#[test]
fn a_failure_to_create_a_segment_file_propagates_and_the_next_write_recovers() {
    let log = Log::new(8);
    let dir = log.dir.path().to_path_buf();
    let mut w = log.writer(Format::Fixed(4));

    // Fill the data section, so that the next record must roll, then
    // take the directory away so the roll cannot create a file. The
    // segment file already open stays usable, which is what makes this a
    // creation failure rather than a write failure.
    assert!(w.write(b"aaaa").is_ok());
    assert!(w.write(b"bbbb").is_ok());
    fs::remove_dir_all(&dir).expect("a removable directory");

    assert!(
        matches!(w.write(b"cccc"), Err(LogError::IoError(_))),
        "a roll that could not create a segment file did not report it"
    );

    // With somewhere to put them again, the next write creates a new
    // segment file and succeeds.
    fs::create_dir(&dir).expect("a creatable directory");
    assert!(
        w.write(b"dddd").is_ok(),
        "the writer did not create a new segment file after the failure"
    );
    drop(w);

    assert_eq!(log.segment_files().len(), 1);
    let out = log.read_all();
    assert_eq!(out.payloads, vec![b"dddd".to_vec()]);
}

// --------------------------------------------------------------- Clearing

#[test]
fn clear_removes_every_segment_file_including_the_open_one() {
    let log = Log::new(10);
    let mut w = log.writer(Format::VariableSimple);
    for payload in &payloads(8, 9) {
        w.write(payload).expect("a writable payload");
    }
    assert!(
        log.segment_files().len() > 1,
        "expected a closed segment file as well as the open one"
    );

    w.clear().expect("a clearable log");
    assert!(
        log.segment_files().is_empty(),
        "clear left a segment file behind"
    );

    // With nothing there, a reader finds no log at all rather than an
    // empty one.
    assert!(matches!(log.reader(), Err(LogError::NoSegmentFiles)));
    drop(w);
    assert!(
        log.segment_files().is_empty(),
        "dropping a cleared writer recreated a segment file"
    );
}

#[test]
fn the_first_write_after_clear_starts_a_new_session() {
    let log = Log::new(400);
    let mut w = log.writer(Format::VariableTsRc);
    for payload in &payloads(3, 12) {
        w.write(payload).expect("a writable payload");
    }
    let cleared_session = w.session_id();
    w.clear().expect("a clearable log");

    // Until the next write, the identifiers still name the cleared
    // session, whose files are gone.
    assert_eq!(w.session_id(), cleared_session);

    let after = payloads(2, 7);
    for payload in &after {
        w.write(payload).expect("a writable payload");
    }
    assert_ne!(
        w.session_id(),
        cleared_session,
        "the write after clear continued the cleared session"
    );
    drop(w);

    // The log reads back as exactly the records written after the clear,
    // numbered from one again, with nothing reported lost. Continuing the
    // cleared sequence would have left a segment file claiming a position
    // with nothing before it, which a reader must report as a loss.
    let out = log.read_all();
    assert_eq!(out.payloads, after);
    assert_eq!(
        out.truncations, 0,
        "an emptied log came back as a damaged one"
    );
    assert_eq!(out.lost, 0);
    assert_eq!(out.session_ends, 0);
    let counts: Vec<u64> = out
        .metas
        .iter()
        .map(|m| match m {
            Meta::VariableTsRc(_, rc) => *rc,
            other => panic!("expected VariableTsRc metadata, got {other:?}"),
        })
        .collect();
    assert_eq!(counts, vec![1, 2]);
    assert_eq!(log.headers()[0].sequence.as_u64(), 0);
}

// --------------------------------------------------------------- Sessions

#[test]
fn sessions_are_reported_once_each_at_their_boundary() {
    for format in VARIABLE_FORMATS {
        let log = Log::new(40);
        let first = payloads(4, 8);
        let second = payloads(3, 6);
        // `send` does nothing here, so the first session's files stay for
        // the second session to be read after.
        log.write_session(format, &first);
        log.write_session(format, &second);

        let out = log.read_all();
        let mut expected = first.clone();
        expected.extend(second.clone());
        assert_eq!(out.payloads, expected, "format {format:?}");
        assert_eq!(
            out.session_ends, 1,
            "format {format:?}: expected exactly one session boundary"
        );
        assert_eq!(out.truncations, 0, "format {format:?}");
    }
}

#[test]
fn a_loss_is_attributed_to_the_session_it_fell_in() {
    let log = Log::new(10);
    let first = payloads(6, 9);
    let second = payloads(6, 9);
    log.write_session(Format::VariableSimple, &first);
    let after_first = log.segment_files().len();
    log.write_session(Format::VariableSimple, &second);

    // Drop a segment file from the middle of the second session.
    let files = log.segment_files();
    fs::remove_file(&files[after_first + 1]).expect("a removable segment file");

    let out = log.read_all();
    assert_eq!(out.session_ends, 1, "the session boundary went unreported");
    assert!(
        out.truncations > 0,
        "the missing segment file went unreported"
    );
    assert!(out.lost > 0);
    // Everything from the first session survives, since the damage was
    // all in the second.
    for payload in &first {
        assert!(
            out.payloads.contains(payload),
            "lost a record from the undamaged session: {:?}",
            String::from_utf8_lossy(payload)
        );
    }
    assert_invented_nothing(&out, &[first.clone(), second.clone()].concat());
}

// ------------------------------------------------------------ Read faults

/// Asserts that every record and partial record a read produced is a
/// prefix of something that was actually written.
///
/// A reader that spliced bytes from either side of a gap onto a partial
/// data header would return a record the writer never wrote, and would
/// otherwise pass a test that only counted what came back.
fn assert_invented_nothing(out: &Outcome, written: &[Vec<u8>]) {
    for got in out.payloads.iter().chain(out.partials.iter()) {
        assert!(
            written.iter().any(|w| w == got || w.starts_with(got)),
            "reader produced a record that was never written: {:?}",
            String::from_utf8_lossy(got)
        );
    }
}

#[test]
fn a_missing_segment_file_is_reported_and_the_reader_carries_on() {
    for format in VARIABLE_FORMATS {
        let log = Log::new(24);
        let written = payloads(14, 9);
        log.write_session(format, &written);

        let files = log.segment_files();
        assert!(files.len() > 4, "format {format:?}: too few segment files");
        fs::remove_file(&files[2]).expect("a removable segment file");

        let out = log.read_all();
        assert!(
            out.truncations > 0,
            "format {format:?}: a missing segment file went unreported"
        );
        assert_eq!(out.lost, 1, "format {format:?}: wrong lost count");
        assert!(
            !out.payloads.is_empty(),
            "format {format:?}: the reader stopped at the gap"
        );
        // The last record is behind the gap, so a reader that recovered
        // returns it.
        assert_eq!(
            out.payloads.last(),
            written.last(),
            "format {format:?}: the reader did not recover past the gap"
        );
        assert_invented_nothing(&out, &written);
    }
}

#[test]
fn an_unopenable_segment_file_is_skipped_like_a_missing_one() {
    for format in VARIABLE_FORMATS {
        let log = Log::new(24);
        let written = payloads(14, 9);
        log.write_session(format, &written);

        let files = log.segment_files();
        corrupt_magic(&files[2]);

        let out = log.read_all();
        // The file is still there, so its neighbours' sequence numbers
        // step over it and the reader sees the gap a deleted file leaves.
        assert!(out.truncations > 0, "format {format:?}");
        assert_eq!(
            out.lost, 1,
            "format {format:?}: a corrupt segment file was not counted as lost"
        );
        assert_eq!(out.payloads.last(), written.last(), "format {format:?}");
        assert_invented_nothing(&out, &written);
        // The reader never opened the corrupt file.
        assert_eq!(
            out.segments_opened,
            (files.len() - 1) as u64,
            "format {format:?}"
        );
    }
}

#[test]
fn a_short_data_section_is_reported_with_no_lost_segment_file() {
    for format in VARIABLE_FORMATS {
        let log = Log::new(24);
        let written = payloads(14, 9);
        log.write_session(format, &written);

        // Halve one file's data section, leaving its header intact: the
        // sequence stays readable, so a record running off the short end
        // is damage the sequence cannot explain.
        let files = log.segment_files();
        truncate_to(&files[2], 6);

        let out = log.read_all();
        assert!(
            out.truncations > 0,
            "format {format:?}: a short data section went unreported"
        );
        assert_eq!(
            out.lost, 0,
            "format {format:?}: a short file was reported as a lost one"
        );
        assert_eq!(out.payloads.last(), written.last(), "format {format:?}");
        assert_invented_nothing(&out, &written);
    }
}

#[test]
fn a_file_cut_short_inside_a_data_header_yields_no_record() {
    // A 13-byte record against a 10-byte data section drives a data
    // header across a boundary. Cutting the file that holds the tail of
    // one leaves the payload length spread across two files, and the
    // surviving tail of a little-endian length cannot be told from the
    // tail of a longer one, so the record must be reported lost rather
    // than guessed at.
    let log = Log::new(10);
    let written = payloads(12, 9);
    log.write_session(Format::VariableSimple, &written);

    let files = log.segment_files();
    let headers = log.headers();
    let record_total = u64::from(Format::VariableSimple.data_header_len()) + 9;
    let header_len = u64::from(Format::VariableSimple.data_header_len());
    // Find a file a data header straddles into, and cut it inside that
    // header's tail.
    let straddled = headers
        .iter()
        .position(|h| {
            let consumed = record_total.saturating_sub(h.remaining);
            h.remaining > 0 && consumed > 0 && consumed < header_len
        })
        .expect("a straddled data header in this geometry");
    truncate_to(&files[straddled], 1);

    let out = log.read_all();
    assert!(
        out.truncations > 0,
        "a header cut in half was not reported as a truncation"
    );
    assert_invented_nothing(&out, &written);
    // The reader stayed usable: records after the damage still come back.
    assert_eq!(
        out.payloads.last(),
        written.last(),
        "the reader did not recover past the cut header"
    );
}

#[test]
fn a_read_failure_inside_a_record_that_does_not_span_files_recovers() {
    // Records well inside one data section, with the tail of one file cut
    // off: the record it held is lost and the files after it still read.
    let log = Log::new(60);
    let written = payloads(12, 8);
    log.write_session(Format::VariableSimple, &written);

    let files = log.segment_files();
    assert!(files.len() > 2);
    truncate_to(&files[1], 30);

    let out = log.read_all();
    assert!(out.truncations > 0 || out.payloads.len() < written.len());
    assert_invented_nothing(&out, &written);
    assert_eq!(out.payloads.last(), written.last());
}

#[test]
fn a_gap_at_the_start_middle_and_end_of_a_spanning_record_all_recover() {
    // One record covers many files, so deleting the file that holds its
    // first, middle, or last third damages that record at that point.
    for which in 0..3 {
        let log = Log::new(16);
        let written = payloads(6, 100);
        log.write_session(Format::VariableSimple, &written);

        let files = log.segment_files();
        let per_record = files.len() / written.len();
        assert!(per_record >= 3, "expected a record to span several files");
        // Records are numbered from one, so this lands inside the third
        // record wherever `which` points.
        let target = 2 * per_record + which * (per_record / 3);
        fs::remove_file(&files[target]).expect("a removable segment file");

        let out = log.read_all();
        assert!(
            out.truncations > 0,
            "which {which}: a gap inside a record went unreported"
        );
        assert_invented_nothing(&out, &written);
        // The last record is well past the damage, so a reader that
        // resynchronized returns it.
        assert_eq!(
            out.payloads.last(),
            written.last(),
            "which {which}: the reader did not recover"
        );
    }
}

#[test]
fn a_session_whose_opening_segment_files_are_gone_is_reported() {
    let log = Log::new(10);
    let written = payloads(10, 9);
    log.write_session(Format::VariableSimple, &written);

    // Nothing crosses into a session's first surviving segment file, so
    // only its own sequence field can show what went missing ahead of it.
    let files = log.segment_files();
    fs::remove_file(&files[0]).expect("a removable segment file");
    fs::remove_file(&files[1]).expect("a removable segment file");

    let out = log.read_all();
    assert!(out.truncations > 0, "the opening gap went unreported");
    assert_eq!(out.lost, 2, "wrong count of segment files lost ahead");
    assert_invented_nothing(&out, &written);
    assert_eq!(out.payloads.last(), written.last());
}

#[test]
fn losing_the_last_segment_file_is_not_mistaken_for_the_end_of_the_log() {
    let log = Log::new(10);
    let written = payloads(10, 9);
    log.write_session(Format::VariableSimple, &written);

    let files = log.segment_files();
    fs::remove_file(files.last().expect("a segment file")).expect("a removable segment file");

    let out = log.read_all();
    assert!(
        out.truncations > 0,
        "the loss of the last segment file was read as a clean end of log"
    );
    assert_invented_nothing(&out, &written);
}

// ------------------------------------- Recovering a lost record header

/// Builds a log in which one record's whole data header, and nothing
/// else of it, lands inside the second segment file.
///
/// A first record of `first_len` payload bytes ends
/// `header + first_len - data_size` bytes into the second segment file,
/// and what is left of that file after those bytes is the room the
/// recovery weighs against a data header. Two geometries leave exactly
/// one: a four-byte `VariableSimple` header with a ten-byte data
/// section and a `first_len` of 12, which owes six bytes and leaves
/// four; and a twenty-byte `VariableTsRc` header with a
/// twenty-four-byte data section and a `first_len` of 8, which owes
/// four and leaves twenty. In both the next record's data header fills
/// the rest of the second segment file and its payload begins at the
/// third.
fn lost_header_log(format: Format, data_size: u32, first_len: usize) -> (Log, Vec<Vec<u8>>) {
    let log = Log::new(data_size);
    let written = vec![
        b"#1 123456789".to_vec()[..first_len].to_vec(),
        b"#2 12".to_vec(),
        b"#3 12".to_vec(),
        b"#4 12".to_vec(),
    ];
    log.write_session(format, &written);
    (log, written)
}

#[test]
fn a_record_whose_header_filled_a_single_gap_is_still_recovered() {
    let (log, written) = lost_header_log(Format::VariableSimple, 10, 12);
    // The geometry the recovery depends on: the second file owes six
    // bytes, so the four bytes after them are one whole data header.
    assert_eq!(log.headers()[1].remaining, 6);

    let files = log.segment_files();
    fs::remove_file(&files[1]).expect("a removable segment file");

    let out = log.read_all();
    // The first record is cut short by the gap, and six of its payload
    // bytes had already reached the caller.
    assert_eq!(out.truncations, 1);
    assert_eq!(out.lost, 1);
    assert_eq!(out.partials, vec![written[0][..6].to_vec()]);

    // The second record's data header went with the gap, but nothing
    // else of it did: the gap had room for exactly that header, so the
    // record's payload must begin at the third file and be as long as
    // that file says it is owed. It is therefore returned in full,
    // rather than discarded with the record that was cut short.
    assert_eq!(
        out.payloads,
        vec![written[1].clone(), written[2].clone(), written[3].clone()],
        "the record whose header filled the gap was not recovered"
    );
    assert_invented_nothing(&out, &written);
}

#[test]
fn a_record_is_not_recovered_when_the_gap_had_room_for_more_than_a_header() {
    // One byte shorter, so the gap had five bytes spare rather than
    // four. That leaves a choice between a payload byte lost from the
    // front of the next record and a whole record lost ahead of it, and
    // nothing on disk settles which, so the record must be given up
    // rather than guessed at.
    let (log, written) = lost_header_log(Format::VariableSimple, 10, 11);
    assert_eq!(log.headers()[1].remaining, 5);

    let files = log.segment_files();
    fs::remove_file(&files[1]).expect("a removable segment file");

    let out = log.read_all();
    assert_eq!(out.truncations, 1);
    assert_eq!(out.lost, 1);
    assert!(
        !out.payloads.contains(&written[1]),
        "a record was reconstructed from a gap that did not settle it"
    );
    // The records after it still come back.
    assert_eq!(
        out.payloads,
        vec![written[2].clone(), written[3].clone()],
        "the reader did not resynchronize past the unrecoverable record"
    );
    assert_invented_nothing(&out, &written);
}

#[test]
fn a_variable_tsrc_record_is_not_recovered_from_a_gap_that_held_its_header() {
    // The geometry the VariableSimple recovery turns on, in the format
    // whose data header holds more than a length: the first record owes
    // four bytes of the second file, leaving twenty, which is exactly
    // one VariableTsRc data header.
    let (log, written) = lost_header_log(Format::VariableTsRc, 24, 8);
    assert_eq!(log.headers()[1].remaining, 4);
    // The next record's payload begins at the third file, so its
    // `remaining` is that whole payload rather than the tail of one.
    assert_eq!(log.headers()[2].remaining, written[1].len() as u64);

    let files = log.segment_files();
    fs::remove_file(&files[1]).expect("a removable segment file");

    let out = log.read_all();
    assert_eq!(out.truncations, 1);
    assert_eq!(out.lost, 1);

    // `remaining` fixes the payload length here as readily as it does
    // for VariableSimple, but the timestamp and record count went into
    // the gap with the rest of the data header and nothing on disk
    // rebuilds them. Recovering the record would mean handing back
    // metadata that was never written, so the record is given up.
    assert!(
        !out.payloads.contains(&written[1]),
        "a VariableTsRc record was recovered, so its metadata was invented"
    );
    // The records after it still come back.
    assert_eq!(
        out.payloads,
        vec![written[2].clone(), written[3].clone()],
        "the reader did not resynchronize past the unrecoverable record"
    );
    assert_invented_nothing(&out, &written);
}

// -------------------------------------------------------------- Overflow

#[test]
fn a_record_larger_than_the_buffer_reports_the_payload_bytes_captured() {
    // The count reported is of payload bytes, so a data header charged
    // against the caller's buffer would show up as a differing count.
    // The two formats' headers differ in width, so both are checked.
    for format in VARIABLE_FORMATS {
        let log = Log::new(64);
        let written = payloads(3, 300);
        log.write_session(format, &written);

        let out = log.read_with(256);
        assert_eq!(
            out.overflows, 3,
            "format {format:?}: wrong number of overflows"
        );
        assert_eq!(
            out.overflow_counts,
            vec![256, 256, 256],
            "format {format:?}: wrong captured count"
        );
        assert_eq!(
            out.truncations, 0,
            "format {format:?}: an overflow was reported as damage"
        );
        // The bytes captured are the front of the record, and the read
        // after an overflow starts at the next record rather than inside
        // this one.
        for (got, want) in out.partials.iter().zip(written.iter()) {
            assert_eq!(got.as_slice(), &want[..256], "format {format:?}");
        }
    }
}

// ----------------------------------------------------- Rejected segments

#[test]
fn a_file_larger_than_its_own_max_size_is_refused() {
    let log = Log::new(12);
    let written = payloads(8, 9);
    log.write_session(Format::VariableSimple, &written);

    let files = log.segment_files();
    let f = OpenOptions::new()
        .write(true)
        .open(&files[1])
        .expect("a writable segment file");
    f.set_len(u64::from(log.seg_size_max) + 1)
        .expect("an extendable segment file");

    let out = log.read_all();
    assert!(out.truncations > 0, "an oversized segment file was read");
    assert_eq!(out.segments_opened, (files.len() - 1) as u64);
    assert_invented_nothing(&out, &written);
}

#[test]
fn a_file_whose_stored_identifier_does_not_match_its_name_is_refused() {
    use std::io::{Seek, SeekFrom, Write};

    let log = Log::new(12);
    let written = payloads(8, 9);
    log.write_session(Format::VariableSimple, &written);

    // A renamed or copied segment file is exactly the case in which the
    // name cannot be trusted, so the stored identifier is what decides.
    let files = log.segment_files();
    let mut f = OpenOptions::new()
        .write(true)
        .open(&files[1])
        .expect("a writable segment file");
    f.seek(SeekFrom::Start(12))
        .expect("a seekable segment file");
    f.write_all(&[0xFF]).expect("a writable header");
    drop(f);

    let out = log.read_all();
    assert!(out.truncations > 0, "a renamed segment file was read");
    assert_eq!(out.segments_opened, (files.len() - 1) as u64);
    assert_invented_nothing(&out, &written);
}

#[test]
fn a_file_written_by_an_unreadable_version_is_refused() {
    use std::io::{Seek, SeekFrom, Write};

    let log = Log::new(12);
    let written = payloads(8, 9);
    log.write_session(Format::VariableSimple, &written);

    let files = log.segment_files();
    let mut f = OpenOptions::new()
        .write(true)
        .open(&files[1])
        .expect("a writable segment file");
    f.seek(SeekFrom::Start(8)).expect("a seekable segment file");
    // Major 99, which this build cannot read.
    f.write_all(b"9900").expect("a writable header");
    drop(f);

    let out = log.read_all();
    assert!(
        out.truncations > 0,
        "a future-version segment file was read"
    );
    assert_eq!(out.segments_opened, (files.len() - 1) as u64);
    assert_invented_nothing(&out, &written);
}

// ------------------------------------------------------- Argument checks

#[test]
fn a_prefix_or_suffix_holding_a_path_separator_is_refused() {
    let log = Log::new(64);
    for (prefix, suffix) in [("a/b", ".log"), ("seg-", "x/y"), ("a\\b", ".log")] {
        assert!(
            matches!(
                LogWrite::new(
                    log.path(),
                    prefix,
                    suffix,
                    log.seg_size_max,
                    Format::VariableSimple,
                    (),
                ),
                Err(LogError::PathDelimiterNotAllowed)
            ),
            "writer accepted prefix {prefix:?} suffix {suffix:?}"
        );
        assert!(
            matches!(
                LogRead::new(log.path(), prefix, suffix),
                Err(LogError::PathDelimiterNotAllowed)
            ),
            "reader accepted prefix {prefix:?} suffix {suffix:?}"
        );
    }
}

#[test]
fn a_directory_that_is_not_one_is_refused() {
    let log = Log::new(64);
    let file = log.dir.path().join("not-a-directory");
    fs::write(&file, b"x").expect("a writable file");
    let path = file.to_str().expect("a UTF-8 path");

    assert!(matches!(
        LogWrite::new(
            path,
            PREFIX,
            SUFFIX,
            log.seg_size_max,
            Format::VariableSimple,
            ()
        ),
        Err(LogError::InvalidPathname)
    ));
    assert!(matches!(
        LogRead::new(path, PREFIX, SUFFIX),
        Err(LogError::InvalidPathname)
    ));
}

#[test]
fn an_empty_directory_holds_no_log_to_read() {
    let log = Log::new(64);
    assert!(matches!(log.reader(), Err(LogError::NoSegmentFiles)));
}

// `LogError::PayloadTooLarge` is raised for a payload past
// `RecSize::MAX`, or one whose total with its data header would exceed
// the u32 `write` returns. Both need a payload of about four gibibytes,
// so neither is exercised here: a test that allocated one would be a
// test of the machine it ran on. The variant is constructed by the two
// checks at the head of `write`.

// --------------------------------------------------------- Reader extras

#[test]
fn the_iterator_yields_every_record_with_its_metadata() {
    let log = Log::new(400);
    let written = payloads(5, 11);
    log.write_session(Format::VariableTsRc, &written);

    let mut reader = log.reader().expect("a readable log");
    let records: Vec<_> = reader.iter().collect();
    assert_eq!(records.len(), written.len());
    for (record, want) in records.iter().zip(written.iter()) {
        assert_eq!(&record.payload, want);
        assert!(matches!(record.meta, Meta::VariableTsRc(_, _)));
    }
}

#[test]
fn current_header_names_the_open_segment_file_and_nothing_before_the_first_read() {
    let log = Log::new(400);
    log.write_session(Format::VariableSimple, &payloads(2, 8));

    let mut reader = log.reader().expect("a readable log");
    assert!(
        reader.current_header().is_none(),
        "a segment file was open before the first read"
    );

    let mut buf = [0u8; 64];
    reader.read(&mut buf).expect("a readable record");
    let header = reader
        .current_header()
        .expect("a segment file open after a read");
    assert_eq!(header.max_size, log.seg_size_max);

    // Read to the end; the reader gives up its segment file at Eof.
    while reader.read(&mut buf).is_ok() {}
    assert!(reader.current_header().is_none());
}

#[test]
fn headers_are_retained_only_when_asked_for() {
    let log = Log::new(10);
    log.write_session(Format::VariableSimple, &payloads(8, 9));
    let total = log.segment_files().len() as u64;

    // Off by default, because the headers accumulate until taken and a
    // caller that never took them would grow the buffer without bound.
    let mut reader = log.reader().expect("a readable log");
    let mut buf = [0u8; 64];
    while reader.read(&mut buf).is_ok() {}
    assert!(reader.take_opened_headers().is_empty());

    let mut reader = log.reader().expect("a readable log");
    reader.collect_opened_headers(true);
    let mut seen = Vec::new();
    loop {
        let done = matches!(reader.read(&mut buf), Err(LogError::Eof));
        seen.extend(reader.take_opened_headers());
        if done {
            break;
        }
    }
    // Every segment file traversed is reported, not merely the one each
    // record ended in.
    assert_eq!(seen.len() as u64, total);
    assert_eq!(reader.segments_opened(), total);
    // Taking them leaves nothing behind.
    assert!(reader.take_opened_headers().is_empty());
}

#[test]
fn read_str_reads_what_write_str_wrote() {
    let log = Log::new(400);
    let mut w = log.writer(Format::VariableSimple);
    let n = w.write_str("hello, telemetry").expect("a writable string");
    assert_eq!(n, 4 + 16);
    drop(w);

    let mut reader = log.reader().expect("a readable log");
    let mut buf = [0u8; 64];
    let result = reader.read_str(&mut buf).expect("a readable record");
    assert_eq!(&buf[..result.n as usize], b"hello, telemetry");
}

#[test]
fn write_returns_the_payload_plus_its_data_header() {
    for (format, header_len) in [
        (Format::Fixed(6), 0u32),
        (Format::VariableSimple, 4),
        (Format::VariableTsRc, 20),
    ] {
        let log = Log::new(400);
        let mut w = log.writer(format);
        let n = w.write(b"abcdef").expect("a writable payload");
        assert_eq!(n, header_len + 6, "format {format:?}");
    }
}

#[test]
fn segment_ids_are_unix_epoch_times_in_creation_order() {
    let before = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("a clock set past the epoch")
        .as_nanos();

    // Eight segment files, each holding one record, written as fast as
    // the writer will go.
    let log = Log::new(8);
    log.write_session(Format::VariableSimple, &vec![b"abcd".to_vec(); 8]);

    let after = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("a clock set past the epoch")
        .as_nanos();

    // Sorting the names sorted the files by identifier, so the sequence
    // numbers coming back in order is what says that identifier order
    // and creation order are the same thing.
    let headers = log.headers();
    assert_eq!(headers.len(), 8);
    for (i, h) in headers.iter().enumerate() {
        assert_eq!(h.sequence.as_u64(), i as u64, "file {i} is out of order");
    }

    // An identifier is still nanoseconds since the UNIX epoch, not a
    // reading of the monotonic clock the writer advances it with.
    for h in &headers {
        let id = u128::from(h.segment_id.as_u64());
        assert!(
            id >= before && id <= after,
            "{} is outside the window the log was written in",
            h.segment_id
        );
    }

    let ids: Vec<u64> = headers.iter().map(|h| h.segment_id.as_u64()).collect();
    for pair in ids.windows(2) {
        assert!(pair[1] > pair[0], "{} does not follow {}", pair[1], pair[0]);
    }
}

#[test]
fn a_new_writer_mints_identifiers_above_what_the_directory_holds() {
    let log = Log::new(8);
    log.write_session(Format::VariableSimple, &vec![b"abcd".to_vec(); 2]);

    // Rename the newest file to an identifier an hour ahead of now,
    // which is what a writer reading a real-time clock an hour later
    // would have minted -- and so what a backward step of that clock
    // between two writers leaves behind. Only the name matters here: a
    // writer seeds itself from the names in the directory, not from the
    // headers inside them.
    let now = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("a clock set past the epoch")
            .as_nanos(),
    )
    .expect("nanoseconds that fit in a u64");
    let planted = SegId::from_u64(now + 3_600_000_000_000);
    let newest = log.segment_files().pop().expect("a written segment file");
    fs::rename(
        &newest,
        newest.with_file_name(format!("{PREFIX}{planted}{SUFFIX}")),
    )
    .expect("a renamable segment file");
    let existing = log.segment_files();

    // This writer anchors on the real-time clock, which reads an hour
    // behind the planted identifier, and has to step past it anyway.
    let mut w = log.writer(Format::VariableSimple);
    w.write(b"abcd").expect("a writable payload");
    drop(w);

    let fresh: Vec<SegId> = log
        .segment_files()
        .into_iter()
        .filter(|p| !existing.contains(p))
        .map(|p| {
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .expect("a UTF-8 name");
            SegId::parse(&name[PREFIX.len()..name.len() - SUFFIX.len()])
                .expect("a well-formed segment file name")
        })
        .collect();

    assert!(!fresh.is_empty(), "the new writer created no segment file");
    for id in fresh {
        assert!(id > planted, "{id} does not follow the planted {planted}");
    }
}

#[test]
fn the_timer_resolution_starts_at_the_build_value_and_never_shrinks() {
    let log = Log::new(8);
    let mut w = log.writer(Format::VariableSimple);
    let supplied = w.timer_resolution();
    assert!(supplied > 0, "a zero resolution would have failed `new`");

    // Sixty-four segment files, each named from the clock. Whether any
    // pair of them lands in one tick of it depends on how coarse that
    // clock is on this machine, so the resolution is not asserted to be
    // unchanged -- widening is the mechanism working. What must hold is
    // that it only ever grows, since a resolution that shrank would make
    // a later nap shorter than one that had already proved too short.
    let mut last = supplied;
    for _ in 0..64 {
        w.write(b"abcd").expect("a writable payload");
        let now = w.timer_resolution();
        assert!(now >= last, "{now} is below the earlier {last}");
        last = now;
    }
}

/// How many times `count_adjustment` has been told of a new resolution,
/// and the last value it was told.
static ADJUSTMENTS: AtomicUsize = AtomicUsize::new(0);
static ADJUSTED_TO: AtomicU64 = AtomicU64::new(0);

fn count_adjustment(ns: u64) {
    ADJUSTMENTS.fetch_add(1, Ordering::Relaxed);
    ADJUSTED_TO.store(ns, Ordering::Relaxed);
}

#[test]
fn the_default_adjustment_callback_reports_and_carries_on() {
    // The default must not stop the program: a deployed system is meant
    // to keep logging through a widening, so this call has to return.
    let mut none = ();
    none.timer_resolution_adjusted(4_096);
}

#[test]
fn an_adjustment_is_reported_exactly_when_the_resolution_changes() {
    ADJUSTMENTS.store(0, Ordering::Relaxed);
    ADJUSTED_TO.store(0, Ordering::Relaxed);

    let log = Log::new(8);
    let mut w = LogWrite::new(
        log.path(),
        PREFIX,
        SUFFIX,
        log.seg_size_max,
        Format::VariableSimple,
        Fns {
            timer_resolution_adjusted: count_adjustment,
            ..Fns::default()
        },
    )
    .expect("a writer on a fresh directory");

    let supplied = w.timer_resolution();
    for _ in 0..64 {
        w.write(b"abcd").expect("a writable payload");
    }
    let in_force = w.timer_resolution();
    drop(w);

    // Whether this machine's clock is coarse enough to widen anything is
    // not the point. What must hold is that the callback and the value
    // agree: no report without a change, and no change unreported.
    let reports = ADJUSTMENTS.load(Ordering::Relaxed);
    if in_force == supplied {
        assert_eq!(reports, 0, "reported an adjustment that did not happen");
    } else {
        assert!(reports > 0, "widened to {in_force} without reporting it");
        assert_eq!(
            ADJUSTED_TO.load(Ordering::Relaxed),
            in_force,
            "the last value reported is not the one in force"
        );
    }
}
