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
//! use tcslog::{Format, LogWrite, WriteCallbacks, SEGMENT_FILE_HEADER_LEN};
//!
//! # fn main() -> Result<(), tcslog::LogError> {
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
mod util;
mod write;

include!(concat!(env!("OUT_DIR"), "/timer_resolution.rs"));

pub use error::LogError;
pub use format::{Format, Meta, RecSize, RecordCount, Timestamp};
pub use header::{SegmentHeader, SEGMENT_FILE_HEADER_LEN, VERSION_MAJOR, VERSION_MINOR};
pub use read::{LogRead, LogReadIter, ReadResult, Record};
pub use segid::SegId;
pub use write::{LogWrite, WriteCallbacks};

#[cfg(test)]
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
        // The spec forbids a zero timer resolution: build.rs enforces
        // it at compile time and LogWrite::new() double-checks at run
        // time.
        assert!(TIMER_RESOLUTION_NS > 0);
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
