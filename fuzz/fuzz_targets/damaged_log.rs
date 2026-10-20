//! Writes a real log, damages it, and reads it back.
//!
//! The fuzzer chooses the format, the segment file size, the records and
//! the sessions they fall into, the damage, and the size of the reader's
//! buffer. Whatever it chooses, the reader must not panic and must reach
//! the end of the log within a bounded number of reads.
//!
//! When the damage only takes bytes away -- a segment file deleted, cut
//! short, or with its header made unreadable -- the reader must also
//! invent nothing, which is the promise the library is built around:
//!
//! * every record a read returns whole is exactly one that was written,
//!   and they come back in the order they were written;
//! * the bytes a `ReadTruncated` or `ReadOverflow` leaves in the buffer
//!   are the front of a record that was written.
//!
//! Bytes overwritten inside a record cannot be told from telemetry --
//! the format carries no checksum -- so when the damage overwrites bytes
//! only the first two requirements are checked.

#![no_main]

use std::fs;
use std::path::PathBuf;

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use tcslog::{Format, LogError, LogRead, LogWrite, SEGMENT_FILE_HEADER_LEN};

const PREFIX: &str = "seg-";
const SUFFIX: &str = ".log";

#[derive(Arbitrary, Debug)]
enum FormatChoice {
    Fixed(u8),
    VariableSimple,
    VariableTsRc,
}

#[derive(Arbitrary, Debug)]
enum Damage {
    /// Removes the segment file at this position in name order.
    Delete(u8),
    /// Cuts the segment file at this position short.
    Truncate(u8, u16),
    /// Overwrites the first bytes of the file, so its header is no
    /// longer one.
    SpoilHeader(u8),
    /// Overwrites one byte anywhere in the file. The only damage that
    /// can change bytes rather than remove them.
    Overwrite(u8, u16, u8),
}

#[derive(Arbitrary, Debug)]
struct Input {
    format: FormatChoice,
    /// Data section size, before clamping to something a record fits.
    data_bytes: u8,
    /// Payload lengths, one list per session.
    sessions: Vec<Vec<u16>>,
    damage: Vec<Damage>,
    /// The reader's buffer size, before clamping.
    buf_len: u16,
}

/// The payload of record `index`: its index first, so that a payload of
/// four bytes or more names the record it belongs to, then a pattern
/// that differs from record to record.
fn payload(index: usize, len: usize) -> Vec<u8> {
    let tag = (index as u32).to_le_bytes();
    (0..len)
        .map(|j| {
            if j < 4 {
                tag[j]
            } else {
                (index.wrapping_mul(31).wrapping_add(j) & 0xff) as u8
            }
        })
        .collect()
}

fn segment_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with(PREFIX) && n.ends_with(SUFFIX))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

fuzz_target!(|input: Input| {
    let format = match input.format {
        FormatChoice::Fixed(n) => Format::Fixed(u32::from(n % 32) + 1),
        FormatChoice::VariableSimple => Format::VariableSimple,
        FormatChoice::VariableTsRc => Format::VariableTsRc,
    };
    let data_bytes = u32::from(input.data_bytes % 64) + format.data_header_len() + 1;
    let seg_size_max = SEGMENT_FILE_HEADER_LEN + data_bytes;

    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().to_str().expect("a UTF-8 temporary path");

    // Write the sessions. Records are numbered across the whole log, so
    // that the order they come back in can be checked across sessions.
    let mut written: Vec<Vec<u8>> = Vec::new();
    for session in input.sessions.iter().take(4) {
        let Ok(mut log) = LogWrite::new(path, PREFIX, SUFFIX, seg_size_max, format, ()) else {
            return;
        };
        for &len in session.iter().take(24) {
            let len = match format {
                Format::Fixed(n) => n as usize,
                _ => usize::from(len % 300),
            };
            let p = payload(written.len(), len);
            log.write(&p).expect("a payload the format accepts");
            written.push(p);
        }
    }

    let mut overwrote = false;
    for damage in input.damage.iter().take(6) {
        let files = segment_files(dir.path());
        if files.is_empty() {
            break;
        }
        let pick = |i: u8| &files[usize::from(i) % files.len()];
        match *damage {
            Damage::Delete(i) => {
                let _ = fs::remove_file(pick(i));
            }
            Damage::Truncate(i, at) => {
                let file = pick(i);
                let len = fs::metadata(file).map(|m| m.len()).unwrap_or(0);
                if len > 0 {
                    let f = fs::OpenOptions::new().write(true).open(file).unwrap();
                    f.set_len(u64::from(at) % len).unwrap();
                }
            }
            Damage::SpoilHeader(i) => {
                let file = pick(i);
                let mut bytes = fs::read(file).unwrap();
                for b in bytes.iter_mut().take(8) {
                    *b = 0;
                }
                fs::write(file, bytes).unwrap();
            }
            Damage::Overwrite(i, at, value) => {
                let file = pick(i);
                let mut bytes = fs::read(file).unwrap();
                if !bytes.is_empty() {
                    let at = usize::from(at) % bytes.len();
                    overwrote |= bytes[at] != value;
                    bytes[at] = value;
                    fs::write(file, bytes).unwrap();
                }
            }
        }
    }

    let Ok(mut log) = LogRead::new(path, PREFIX, SUFFIX) else {
        return;
    };
    let mut buf = vec![0u8; usize::from(input.buf_len % 512) + 1];

    // Each read either returns a record, consumes a record, crosses a
    // segment file, or reports a loss, so a reader that is making
    // progress is done long before this.
    let files = segment_files(dir.path()).len();
    let bound = 4 * (written.len() + files) + 16;
    // The index of the last record returned whole, which the next one
    // returned whole must follow.
    let mut last: Option<usize> = None;

    for _ in 0..bound {
        match log.read(&mut buf) {
            Ok(result) => {
                if overwrote {
                    continue;
                }
                let got = &buf[..result.n as usize];
                let start = last.map_or(0, |l| l + 1);
                let found = (start..written.len()).find(|&i| written[i] == got);
                assert!(
                    found.is_some(),
                    "a record no session wrote, or one out of order: {got:?}"
                );
                last = found;
            }
            Err(LogError::ReadOverflow(n) | LogError::ReadTruncated { n, .. }) => {
                if overwrote || n == 0 {
                    continue;
                }
                let got = &buf[..n as usize];
                assert!(
                    written.iter().any(|w| w.starts_with(got)),
                    "partial bytes that are the front of no record: {got:?}"
                );
            }
            Err(LogError::Eof) => return,
            Err(_) => {}
        }
    }
    panic!("the reader did not reach the end of the log in {bound} reads");
});
