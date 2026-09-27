//! Prints the header of a single segment file on one line.
//!
//! Reads the header from the segment file named on the command line, or
//! from standard input when no file is named. Unlike `tcslog-dump`,
//! which walks a whole log and needs the directory, prefix, and suffix
//! that identify one, this tool looks at exactly the bytes handed to it
//! and never consults the file's name, so it also serves files that
//! have been renamed or copied out of their log.
//!
//! Run with:
//!
//! ```text
//! cargo run -p tcslog-dumphdr -- <file>
//! cargo run -p tcslog-dumphdr < <file>
//! ```

use std::fs::File;
use std::io;

use clap::{CommandFactory, Parser};

use tcslog::{format_timestamp, SegId, SegmentHeader};

/// Print a tcslog segment file header on a single line
#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Segment file to read. Standard input is read when omitted.
    filename: Option<String>,
}

fn main() {
    let args = Args::try_parse().unwrap_or_else(|e| {
        eprintln!("{e}");
        let _ = Args::command().print_help();
        eprintln!();
        std::process::exit(2);
    });

    if let Err(e) = run(&args) {
        eprintln!("tcslog-dumphdr: {e}");
        std::process::exit(1);
    }
}

/// Reads the header named by `args` and prints it.
fn run(args: &Args) -> Result<(), String> {
    // Read only the header: the rest of the file is the data section,
    // which this tool has no use for, and leaving it unread lets the
    // stdin form work on a pipe whose writer is still going.
    let header = if let Some(name) = &args.filename {
        let mut f = File::open(name).map_err(|e| format!("{name}: {e}"))?;
        SegmentHeader::read_from(&mut f).map_err(|e| format!("{name}: {e}"))?
    } else {
        let mut stdin = io::stdin().lock();
        SegmentHeader::read_from(&mut stdin).map_err(|e| format!("<stdin>: {e}"))?
    };

    println!("{}", header_line(&header));
    Ok(())
}

/// Renders every header field as comma-separated `field=value` pairs on
/// one line.
fn header_line(h: &SegmentHeader) -> String {
    format!(
        "segment_id={}, session_id={}, max_size={}, remaining={}, \
         format={:?}, sequence={}",
        seg_id_date(h.segment_id),
        seg_id_date(h.session_id),
        h.max_size,
        h.remaining,
        h.format,
        h.sequence,
    )
}

/// Renders a segment identifier as a date and time. Segment IDs are
/// wall-clock timestamps in nanoseconds since the UNIX epoch, so the
/// date is the more informative rendering here; the dashed hex form
/// that names the file on disk is what `tcslog-dump` prints.
fn seg_id_date(id: SegId) -> String {
    format_timestamp(id.as_u64())
}

#[cfg(test)]
mod tests {
    use super::*;

    use tcslog::{Format, SeqId};

    #[test]
    fn line_renders_dates_and_decimals() {
        let h = SegmentHeader {
            segment_id: SegId::from_u64(1_234_567_890_000_000_000),
            session_id: SegId::from_u64(1_234_567_800_000_000_000),
            max_size: 4096,
            remaining: 17,
            format: Format::Fixed(64),
            sequence: SeqId::from_u64(3),
        };
        assert_eq!(
            header_line(&h),
            "segment_id=2009-02-13T23:31:30.000000000Z, \
             session_id=2009-02-13T23:30:00.000000000Z, \
             max_size=4096, remaining=17, format=Fixed(64), sequence=3"
        );
    }

    #[test]
    fn round_trips_a_serialized_header() {
        let h = SegmentHeader {
            segment_id: SegId::from_u64(0),
            session_id: SegId::from_u64(0),
            max_size: 256,
            remaining: 0,
            format: Format::VariableTsRc,
            sequence: SeqId::ZERO,
        };
        let bytes = h.to_bytes();
        let back = SegmentHeader::from_bytes(&bytes).unwrap();
        assert_eq!(
            header_line(&back),
            "segment_id=1970-01-01T00:00:00.000000000Z, \
             session_id=1970-01-01T00:00:00.000000000Z, \
             max_size=256, remaining=0, format=VariableTsRc, sequence=0"
        );
    }
}
