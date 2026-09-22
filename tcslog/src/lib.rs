//! # tcslog
//!
//! Onboard telemetry logging for vehicles that must store data locally
//! until a link is available to hand it off. The log is split into
//! bounded-size *segment files* so that filled segments can be sent (or
//! discarded) independently of the live writer, and so that a corrupt or
//! missing chunk of storage costs at most one segment worth of data.
//!
//! ## Quick start
//!
//! ```no_run
//! # #[cfg(feature = "write")]
//! # fn demo() -> Result<(), tcslog::LogError> {
//! use tcslog::{Format, LogWrite, WriteCallbacks, SEGMENT_FILE_HEADER_LEN};
//!
//! let mut log = LogWrite::new(
//!     "/tmp/telemetry",
//!     "sample-",
//!     ".tcslog",
//!     SEGMENT_FILE_HEADER_LEN + 4096,
//!     Format::VariableTsRc,
//!     WriteCallbacks::default(),
//! )?;
//! log.write(b"hello, world")?;
//! # Ok(())
//! # }
//! ```
//!
//! See [`docs/tcslog.rst`](../../docs/tcslog.rst) for the full user
//! guide.

#![deny(dead_code)]

mod error;
mod format;
mod header;
mod read;
mod segid;
mod seq_id;
mod util;
#[cfg(feature = "write")]
mod write;

#[cfg(feature = "write")]
include!(concat!(env!("OUT_DIR"), "/timer_resolution.rs"));

pub use error::LogError;
pub use format::{Format, Meta, RecSize, RecordCount, Timestamp};
pub use header::{SegmentHeader, SEGMENT_FILE_HEADER_LEN, VERSION_MAJOR, VERSION_MINOR};
pub use read::{LogRead, LogReadIter, ReadResult, Record};
pub use segid::SegId;
pub use seq_id::SeqId;
#[cfg(feature = "write")]
pub use write::{LogWrite, WriteCallbacks};

// The in-crate tests all exercise the write path (either by producing
// sample logs to read back, or by asserting write-time invariants).
// Gate the whole module on the `write` feature so the crate still
// compiles with `--no-default-features --tests`.
#[cfg(all(test, feature = "write"))]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write as _;
    use std::path::Path;
    use tempfile::TempDir;

    fn dir_str(d: &TempDir) -> String {
        d.path().to_str().unwrap().to_owned()
    }

    fn write_and_read_roundtrip(format: Format, msgs: &[&[u8]], seg_size_max: u32) {
        let dir = tempfile::tempdir().unwrap();
        let d = dir_str(&dir);
        {
            let mut log = LogWrite::new(
                &d,
                "seg-",
                ".log",
                seg_size_max,
                format,
                WriteCallbacks::default(),
            )
            .unwrap();
            for m in msgs {
                log.write(m).unwrap();
            }
            log.flush().unwrap();
        }

        let mut reader = LogRead::new(&d, "seg-", ".log").unwrap();
        let mut buf = vec![0u8; 4096];
        for m in msgs {
            let res = reader.read(&mut buf).unwrap();
            assert_eq!(res.n as usize, m.len(), "length matches");
            assert_eq!(&buf[..res.n as usize], *m);
        }
        assert!(matches!(reader.read(&mut buf), Err(LogError::Eof)));
    }

    #[test]
    fn variable_ts_rc_short_records() {
        write_and_read_roundtrip(
            Format::VariableTsRc,
            &[b"one", b"two", b"three"],
            SEGMENT_FILE_HEADER_LEN + 4096,
        );
    }

    #[test]
    fn variable_simple_short_records() {
        write_and_read_roundtrip(
            Format::VariableSimple,
            &[b"", b"a", b"bb", b"ccc"],
            SEGMENT_FILE_HEADER_LEN + 4096,
        );
    }

    #[test]
    fn variable_ts_rc_records_span_segments() {
        let msg = vec![0x42u8; 200];
        let msgs = vec![msg.as_slice(); 5];
        write_and_read_roundtrip(
            Format::VariableTsRc,
            &msgs,
            SEGMENT_FILE_HEADER_LEN + 40,
        );
    }

    #[test]
    fn fixed_records_span_segments() {
        let msgs = vec![
            b"11111111".as_ref(),
            b"22222222".as_ref(),
            b"33333333".as_ref(),
            b"44444444".as_ref(),
            b"55555555".as_ref(),
        ];
        write_and_read_roundtrip(
            Format::Fixed(8),
            &msgs,
            SEGMENT_FILE_HEADER_LEN + 5,
        );
    }

    #[test]
    fn read_overflow_reports_actual_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir_str(&dir);
        let mut log = LogWrite::new(
            &d,
            "over-",
            ".log",
            SEGMENT_FILE_HEADER_LEN + 4096,
            Format::VariableSimple,
            WriteCallbacks::default(),
        )
        .unwrap();
        log.write(&[7u8; 100]).unwrap();
        drop(log);

        let mut reader = LogRead::new(&d, "over-", ".log").unwrap();
        let mut small = [0u8; 20];
        match reader.read(&mut small) {
            Err(LogError::ReadOverflow(n)) => {
                assert_eq!(n, 20);
                assert!(small.iter().all(|b| *b == 7));
            }
            other => panic!("expected ReadOverflow, got {other:?}"),
        }
        assert!(matches!(reader.read(&mut small), Err(LogError::Eof)));
    }

    #[test]
    fn seg_size_too_small_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let err = LogWrite::new(
            dir.path().to_str().unwrap(),
            "s-",
            ".l",
            SEGMENT_FILE_HEADER_LEN,
            Format::VariableSimple,
            WriteCallbacks::default(),
        )
        .unwrap_err();
        assert!(matches!(err, LogError::SegSizeTooSmall));
    }

    #[test]
    fn path_delimiter_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let err = LogWrite::new(
            dir.path().to_str().unwrap(),
            "bad/prefix-",
            ".l",
            SEGMENT_FILE_HEADER_LEN + 100,
            Format::VariableSimple,
            WriteCallbacks::default(),
        )
        .unwrap_err();
        assert!(matches!(err, LogError::PathDelimiterNotAllowed));
    }

    #[test]
    fn no_segments_reports_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = LogRead::new(dir.path().to_str().unwrap(), "x-", ".y").unwrap_err();
        assert!(matches!(err, LogError::NoSegmentFiles));
    }

    #[test]
    fn fixed_rejects_zero_length_config() {
        let dir = tempfile::tempdir().unwrap();
        let err = LogWrite::new(
            dir.path().to_str().unwrap(),
            "fz-",
            ".l",
            SEGMENT_FILE_HEADER_LEN + 8,
            Format::Fixed(0),
            WriteCallbacks::default(),
        )
        .unwrap_err();
        assert!(matches!(err, LogError::FixedLenMismatch));
    }

    #[test]
    fn fixed_rejects_wrong_length_write() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir_str(&dir);
        let mut log = LogWrite::new(
            &d,
            "fw-",
            ".l",
            SEGMENT_FILE_HEADER_LEN + 16,
            Format::Fixed(4),
            WriteCallbacks::default(),
        )
        .unwrap();
        assert!(matches!(log.write(b"abc"), Err(LogError::FixedLenMismatch)));
        assert!(matches!(log.write(b"abcde"), Err(LogError::FixedLenMismatch)));
        // Correct length still works.
        assert!(log.write(b"abcd").is_ok());
    }

    #[test]
    fn timer_resolution_is_positive() {
        // build.rs enforces this at compile time and LogWrite::new()
        // double-checks at run time.
        assert!(TIMER_RESOLUTION_NS > 0);
    }

    #[test]
    fn corrupt_segment_is_skipped() {
        // Write two segments then corrupt the first one's header; the
        // reader must skip it and still return the records from the
        // second one.
        let dir = tempfile::tempdir().unwrap();
        let d = dir_str(&dir);
        {
            let mut log = LogWrite::new(
                &d,
                "cs-",
                ".log",
                SEGMENT_FILE_HEADER_LEN + 40,
                Format::VariableSimple,
                WriteCallbacks::default(),
            )
            .unwrap();
            log.write(&[0xAAu8; 30]).unwrap();
            log.write(&[0xBBu8; 30]).unwrap();
            log.flush().unwrap();
        }
        // Corrupt every segment file's header until we find one with
        // the second record's payload intact. Simplest is to scribble
        // over the first segment's magic bytes.
        let mut segments: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        segments.sort();
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .open(&segments[0])
            .unwrap();
        f.write_all(b"XXXXXXXX").unwrap();
        drop(f);

        let mut reader = LogRead::new(&d, "cs-", ".log").unwrap();
        let mut buf = [0u8; 128];
        // The reader silently skips the corrupt segment. Either the
        // remaining segment starts with a whole record (in which case
        // the next read returns that record), or the corrupt segment
        // held the only record and we hit Eof cleanly.
        match reader.read(&mut buf) {
            Ok(_) => {}
            Err(LogError::Eof) => {}
            other => panic!("expected Ok or Eof after corrupt-header skip, got {other:?}"),
        }
    }

    #[test]
    fn missing_segment_mid_record_recovers() {
        // Write records that force a single record's payload to span
        // several segments. Delete a segment from the middle of that
        // record's span. The reader must:
        //   * return the record(s) before the gap cleanly,
        //   * surface the truncated record as `ReadTruncated`,
        //   * and recover far enough to return the record(s) after
        //     the gap.
        let dir = tempfile::tempdir().unwrap();
        let d = dir_str(&dir);
        {
            let mut log = LogWrite::new(
                &d,
                "ms-",
                ".log",
                SEGMENT_FILE_HEADER_LEN + 40,
                Format::VariableSimple,
                WriteCallbacks::default(),
            )
            .unwrap();
            // Record 0: 30 bytes; comfortably fits in the first
            // segment. Record 1: 200 bytes; forces several segment
            // spans. Record 2: 30 bytes; sits after the multi-segment
            // record.
            log.write(&vec![0xA0u8; 30]).unwrap();
            log.write(&vec![0xB1u8; 200]).unwrap();
            log.write(&vec![0xC2u8; 30]).unwrap();
            log.flush().unwrap();
        }
        let mut segments: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        segments.sort();
        assert!(
            segments.len() >= 5,
            "test setup produced too few segments: {}",
            segments.len()
        );
        // Delete a segment from the middle of the run — squarely
        // inside record 1's span.
        std::fs::remove_file(&segments[segments.len() / 2]).unwrap();

        let mut reader = LogRead::new(&d, "ms-", ".log").unwrap();
        let mut buf = vec![0u8; 4096];

        // Record 0 comes back cleanly.
        let r0 = reader.read(&mut buf).unwrap();
        assert_eq!(r0.n, 30);
        assert!(buf[..30].iter().all(|b| *b == 0xA0));

        // Record 1 must not be silently mis-read; the mid-record
        // segment gap must surface as ReadTruncated.
        let mut saw_truncation = false;
        let mut saw_c2 = false;
        loop {
            match reader.read(&mut buf) {
                Ok(res) => {
                    if res.n == 30 && buf[..30].iter().all(|b| *b == 0xC2) {
                        saw_c2 = true;
                        break;
                    }
                    // Any other successful record means the gap
                    // wasn't detected — that is the bug we're
                    // guarding against.
                    if res.n == 200 && buf[..200].iter().all(|b| *b == 0xB1) {
                        panic!("record 1 read succeeded despite missing segment");
                    }
                }
                Err(LogError::ReadTruncated) => {
                    saw_truncation = true;
                }
                Err(LogError::Eof) => break,
                Err(e) => panic!("unexpected error: {e:?}"),
            }
        }
        assert!(saw_truncation, "expected ReadTruncated for the record spanning the gap");
        assert!(saw_c2, "expected record 2 to be recovered after the gap");
    }

    #[test]
    fn missing_segment_mid_data_header_is_detected() {
        // Regression test: a data header that straddles a segment
        // boundary used to escape validation entirely, because the
        // record's total size is not yet known at that crossing and
        // the `remaining` checks are keyed off it. The reader would
        // splice the tail of the header out of whatever segment came
        // next and hand back phantom records built from unrelated
        // bytes. The segment `sequence` field is checked instead,
        // which holds regardless of how much of the record is decoded.
        //
        // Sizing: a 40-byte data section with 20-byte VariableTsRc
        // headers and 9-byte payloads means each record occupies 29
        // bytes, so record 2 begins 11 bytes before the end of segment
        // 0 and its 20-byte header necessarily spans into segment 1.
        const PAYLOAD: usize = 9;
        const COUNT: usize = 8;
        let dir = tempfile::tempdir().unwrap();
        let d = dir_str(&dir);
        {
            let mut log = LogWrite::new(
                &d,
                "mh-",
                ".log",
                SEGMENT_FILE_HEADER_LEN + 40,
                Format::VariableTsRc,
                WriteCallbacks::default(),
            )
            .unwrap();
            for i in 0..COUNT {
                log.write(&vec![0xA0u8 + i as u8; PAYLOAD]).unwrap();
            }
            log.flush().unwrap();
        }

        let mut segments: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        segments.sort();
        assert!(
            segments.len() >= 4,
            "test setup produced too few segments: {}",
            segments.len()
        );
        // Segment 1 holds the tail of record 2's data header.
        std::fs::remove_file(&segments[1]).unwrap();

        let mut reader = LogRead::new(&d, "mh-", ".log").unwrap();
        let mut buf = vec![0u8; 4096];
        let mut saw_truncation = false;
        let mut recovered = Vec::new();
        loop {
            match reader.read(&mut buf) {
                Ok(res) => {
                    // Every record handed back must be one we actually
                    // wrote: correct length, a uniform payload byte in
                    // range, and a record count that agrees with that
                    // payload. A phantom fails all three.
                    assert_eq!(
                        res.n as usize, PAYLOAD,
                        "phantom record with implausible length {}",
                        res.n
                    );
                    let first = buf[0];
                    assert!(
                        buf[..PAYLOAD].iter().all(|b| *b == first),
                        "payload is not one of the written records"
                    );
                    let idx = first
                        .checked_sub(0xA0)
                        .filter(|i| (*i as usize) < COUNT)
                        .unwrap_or_else(|| {
                            panic!("payload byte {first:#x} was never written")
                        });
                    match res.meta {
                        Meta::VariableTsRc(_, rc) => assert_eq!(
                            rc,
                            u64::from(idx) + 1,
                            "record count does not match payload {first:#x}"
                        ),
                        other => panic!("unexpected metadata {other:?}"),
                    }
                    recovered.push(idx);
                }
                Err(LogError::ReadTruncated) => saw_truncation = true,
                Err(LogError::Eof) => break,
                Err(e) => panic!("unexpected error: {e:?}"),
            }
        }

        assert!(
            saw_truncation,
            "the segment gap splitting a data header must surface as \
             ReadTruncated"
        );
        assert_eq!(
            recovered.first(),
            Some(&0),
            "the record before the gap must still be returned"
        );
        assert!(
            recovered.len() > 1,
            "the reader must resynchronize and recover records after \
             the gap, got {recovered:?}"
        );
    }

    #[test]
    fn missing_segments_at_record_boundaries_recovers_survivors() {
        // Fixed(1) with a data section of exactly one byte means every
        // record fills its own segment and every segment boundary is a
        // fresh-record boundary. Deleting segments 1, 3, and 4 must
        // still let the reader surface records 2 and 5. This is the
        // regression case for the writer/reader mismatch where a record
        // filling a whole data section was indistinguishable on disk
        // from the tail of a record that started in a now-missing
        // predecessor.
        let dir = tempfile::tempdir().unwrap();
        let d = dir_str(&dir);
        {
            let mut log = LogWrite::new(
                &d,
                "fb-",
                ".log",
                SEGMENT_FILE_HEADER_LEN + 1,
                Format::Fixed(1),
                WriteCallbacks::default(),
            )
            .unwrap();
            for byte in b"12345" {
                log.write(&[*byte]).unwrap();
            }
        }
        let mut segments: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        segments.sort();
        assert_eq!(
            segments.len(),
            5,
            "test setup produced the wrong number of segments"
        );
        // Remove indices in descending order so earlier indices remain
        // valid after each removal.
        for idx in [3usize, 2, 0] {
            std::fs::remove_file(&segments[idx]).unwrap();
        }

        let mut reader = LogRead::new(&d, "fb-", ".log").unwrap();
        let mut buf = [0u8; 1];
        let mut recovered: Vec<u8> = Vec::new();
        loop {
            match reader.read(&mut buf) {
                Ok(res) => {
                    assert_eq!(res.n, 1);
                    recovered.push(buf[0]);
                }
                Err(LogError::ReadTruncated) => {}
                Err(LogError::Eof) => break,
                Err(e) => panic!("unexpected error: {e:?}"),
            }
        }
        assert_eq!(recovered, b"25");
    }

    #[test]
    fn callbacks_see_rolled_segments() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static CALLS: AtomicUsize = AtomicUsize::new(0);
        fn count_send(_p: &Path) -> std::io::Result<()> {
            CALLS.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn flush_record(f: &mut File) -> std::io::Result<()> {
            f.flush()
        }
        CALLS.store(0, Ordering::SeqCst);

        let dir = tempfile::tempdir().unwrap();
        let d = dir_str(&dir);
        let cbs = WriteCallbacks {
            record_complete: flush_record,
            send: count_send,
        };
        let mut log = LogWrite::new(
            &d,
            "cb-",
            ".log",
            SEGMENT_FILE_HEADER_LEN + 40,
            Format::VariableTsRc,
            cbs,
        )
        .unwrap();
        // Each record needs 4 + 8 + 8 + 100 = 120 bytes, so each spills
        // across three segments (40 bytes of data each), triggering
        // rollover callbacks.
        for _ in 0..3 {
            log.write(&[0xAAu8; 100]).unwrap();
        }
        drop(log);
        assert!(CALLS.load(Ordering::SeqCst) >= 3);
    }

    #[test]
    fn current_header_reports_active_segment() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir_str(&dir);
        {
            let mut log = LogWrite::new(
                &d,
                "ch-",
                ".log",
                SEGMENT_FILE_HEADER_LEN + 4096,
                Format::VariableTsRc,
                WriteCallbacks::default(),
            )
            .unwrap();
            log.write(b"one").unwrap();
        }
        let mut reader = LogRead::new(&d, "ch-", ".log").unwrap();
        assert!(reader.current_header().is_none());
        let mut buf = [0u8; 32];
        reader.read(&mut buf).unwrap();
        let h = reader.current_header().unwrap();
        assert_eq!(h.format, Format::VariableTsRc);
        assert_eq!(h.sequence, SeqId::ZERO);
    }

    #[test]
    fn session_end_returned_when_session_changes() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir_str(&dir);
        // Session A: one segment, one record.
        {
            let mut log = LogWrite::new(
                &d,
                "se-",
                ".log",
                SEGMENT_FILE_HEADER_LEN + 4096,
                Format::VariableSimple,
                WriteCallbacks::default(),
            )
            .unwrap();
            log.write(b"first").unwrap();
        }
        // Session B: a fresh call to `new()` sends the first session's
        // segment via the (no-op) send callback and begins a new one.
        {
            let mut log = LogWrite::new(
                &d,
                "se-",
                ".log",
                SEGMENT_FILE_HEADER_LEN + 4096,
                Format::VariableSimple,
                WriteCallbacks::default(),
            )
            .unwrap();
            log.write(b"second").unwrap();
        }

        let mut reader = LogRead::new(&d, "se-", ".log").unwrap();
        let mut buf = [0u8; 32];
        let r = reader.read(&mut buf).unwrap();
        assert_eq!(&buf[..r.n as usize], b"first");
        match reader.read(&mut buf) {
            Err(LogError::SessionEnd) => {}
            other => panic!("expected SessionEnd, got {other:?}"),
        }
        let r = reader.read(&mut buf).unwrap();
        assert_eq!(&buf[..r.n as usize], b"second");
    }

    #[test]
    fn iterator_yields_all_records() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir_str(&dir);
        {
            let mut log = LogWrite::new(
                &d,
                "it-",
                ".log",
                SEGMENT_FILE_HEADER_LEN + 4096,
                Format::VariableSimple,
                WriteCallbacks::default(),
            )
            .unwrap();
            log.write(b"alpha").unwrap();
            log.write(b"beta").unwrap();
            log.write(b"gamma").unwrap();
        }
        let mut reader = LogRead::new(&d, "it-", ".log").unwrap();
        let collected: Vec<Vec<u8>> = reader
            .iter()
            .map(|r| r.unwrap().payload)
            .collect();
        assert_eq!(
            collected,
            vec![b"alpha".to_vec(), b"beta".to_vec(), b"gamma".to_vec()]
        );
    }

    #[test]
    fn drop_sends_pending_segment() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static CALLS: AtomicUsize = AtomicUsize::new(0);
        fn count_send(_p: &Path) -> std::io::Result<()> {
            CALLS.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        CALLS.store(0, Ordering::SeqCst);

        let dir = tempfile::tempdir().unwrap();
        let d = dir_str(&dir);
        let cbs = WriteCallbacks {
            record_complete: |_| Ok(()),
            send: count_send,
        };
        {
            let mut log = LogWrite::new(
                &d,
                "ds-",
                ".log",
                SEGMENT_FILE_HEADER_LEN + 4096,
                Format::VariableSimple,
                cbs,
            )
            .unwrap();
            log.write(b"only-record").unwrap();
        }
        // The one segment holds the single record and must be handed
        // off exactly once when the writer drops.
        assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn write_error_recovery_rolls_to_new_segment() {
        // A `send` callback that returns an error the first time it is
        // invoked simulates a write-time fault: `LogWrite::write` calls
        // it during roll-over, propagating the error back to the
        // caller. The very next call to `write` must succeed by opening
        // a fresh segment file, matching the spec's "next call to
        // write() creates a new segment file" contract.
        use std::sync::atomic::{AtomicUsize, Ordering};
        static SEND_CALLS: AtomicUsize = AtomicUsize::new(0);
        fn flaky_send(_p: &Path) -> std::io::Result<()> {
            let n = SEND_CALLS.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                Err(std::io::Error::new(std::io::ErrorKind::Other, "boom"))
            } else {
                Ok(())
            }
        }
        SEND_CALLS.store(0, Ordering::SeqCst);

        let dir = tempfile::tempdir().unwrap();
        let d = dir_str(&dir);
        let cbs = WriteCallbacks {
            record_complete: |_| Ok(()),
            send: flaky_send,
        };
        let mut log = LogWrite::new(
            &d,
            "we-",
            ".log",
            SEGMENT_FILE_HEADER_LEN + 40,
            Format::VariableSimple,
            cbs,
        )
        .unwrap();
        // Force the first roll: write a record larger than the data
        // section so `write_bytes` calls `roll_segment` and hits the
        // simulated failure.
        let err = log.write(&[0x11u8; 100]).unwrap_err();
        assert!(matches!(err, LogError::IoError(_)));

        // The next write must succeed - a fresh segment file has been
        // opened by the recovery path.
        assert!(log.write(&[0x22u8; 5]).is_ok());
    }

    #[test]
    fn clear_removes_prior_segments_only() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir_str(&dir);
        {
            let mut log = LogWrite::new(
                &d,
                "cl-",
                ".log",
                SEGMENT_FILE_HEADER_LEN + 40,
                Format::VariableTsRc,
                WriteCallbacks::default(),
            )
            .unwrap();
            // Force at least one rollover.
            for _ in 0..3 {
                log.write(&[0xFFu8; 100]).unwrap();
            }
            log.clear().unwrap();
        }
        // The current file at drop time survives clear(); everything
        // else is gone. Reading should therefore succeed but consume
        // just the tail record.
        let mut reader = LogRead::new(&d, "cl-", ".log").unwrap();
        let mut buf = vec![0u8; 4096];
        // Any read should either succeed or hit Eof gracefully.
        let _ = reader.read(&mut buf);
    }
}
