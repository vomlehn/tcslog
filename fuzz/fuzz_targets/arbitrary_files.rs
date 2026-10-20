//! Reads a log of up to eight segment files whose contents the fuzzer
//! chooses.
//!
//! A file is either raw bytes, which exercises the header checks, or a
//! header the reader will accept -- its identifier matching the file's
//! name and its size within its maximum -- whose other fields, and the
//! data section after it, the fuzzer chooses. The second is what reaches
//! the data-section parsing: random bytes almost never form a header the
//! reader accepts, so without it the fuzzer would spend nearly all its
//! time being turned away at the door. Either way the reader meets
//! `remaining` fields, sequence numbers, sessions, and data headers that
//! no writer would produce.
//!
//! The reader must not panic, and must reach the end of the log within
//! a bounded number of reads. Nothing can be said about what it returns:
//! there is no telemetry here to compare it with.

#![no_main]

use std::fs;

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use tcslog::{Format, LogError, LogRead, SegId, SegmentHeader, SeqId, SEGMENT_FILE_HEADER_LEN};

const PREFIX: &str = "seg-";
const SUFFIX: &str = ".log";

#[derive(Arbitrary, Debug)]
enum FormatChoice {
    Fixed(u8),
    VariableSimple,
    VariableTsRc,
}

#[derive(Arbitrary, Debug)]
enum File {
    Raw(Vec<u8>),
    Headed {
        session: u8,
        /// Data section size, before clamping.
        data_bytes: u8,
        remaining: u16,
        format: FormatChoice,
        sequence: u8,
        data: Vec<u8>,
    },
}

fuzz_target!(|files: Vec<File>| {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().to_str().expect("a UTF-8 temporary path");

    let files = &files[..files.len().min(8)];
    let mut total = 0usize;
    for (i, file) in files.iter().enumerate() {
        let id = SegId::from_u64(i as u64 + 1);
        let bytes = match file {
            File::Raw(bytes) => bytes[..bytes.len().min(4096)].to_vec(),
            File::Headed {
                session,
                data_bytes,
                remaining,
                format,
                sequence,
                data,
            } => {
                let format = match format {
                    FormatChoice::Fixed(n) => Format::Fixed(u32::from(*n % 32) + 1),
                    FormatChoice::VariableSimple => Format::VariableSimple,
                    FormatChoice::VariableTsRc => Format::VariableTsRc,
                };
                let max_size = SEGMENT_FILE_HEADER_LEN + u32::from(*data_bytes) + 1;
                let header = SegmentHeader {
                    segment_id: id,
                    // Sessions named by the identifier of a file in this
                    // log, as a writer's are, or by one that is not.
                    session_id: SegId::from_u64(u64::from(*session % 10)),
                    max_size,
                    remaining: u64::from(*remaining),
                    format,
                    sequence: SeqId::from_u64(u64::from(*sequence % 10)),
                };
                let room = (max_size - SEGMENT_FILE_HEADER_LEN) as usize;
                let mut bytes = header.to_bytes().to_vec();
                bytes.extend_from_slice(&data[..data.len().min(room)]);
                bytes
            }
        };
        total += bytes.len();
        let name = format!("{PREFIX}{id}{SUFFIX}");
        fs::write(dir.path().join(name), bytes).expect("a writable temporary directory");
    }

    let Ok(mut log) = LogRead::new(path, PREFIX, SUFFIX) else {
        return;
    };
    // A read returns or skips at least one byte of some file, or crosses
    // from one file to the next, or reports a loss, so this many reads is
    // more than any amount of these bytes can need.
    let bound = total + 4 * files.len() + 16;
    let mut buf = [0u8; 256];
    for _ in 0..bound {
        if let Err(LogError::Eof) = log.read(&mut buf) {
            return;
        }
    }
    panic!("the reader did not reach the end of the log in {bound} reads");
});
