//! Reads a chain of segment files back and prints each segment file's
//! header (with `--verbose`) and every log message.
//!
//! Run with:
//!
//! ```text
//! cargo run -p tcslog-dump -- <dir> <prefix> <suffix>
//! ```

use std::error::Error;

use clap::{CommandFactory, Parser};

use tcslog::{record_trailer, LogError, LogRead, Meta, SegmentHeader};

const MAX_MESSAGE_SIZE: usize = 256;

/// Print a tcslog file
#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Directory in which the log file lives
    dirname: String,

    /// Segment file name prefix.
    prefix: String,

    /// Segment file name suffix.
    suffix: String,

    /// Print as hex bytes or text (default)
    #[arg(short, long)]
    text: bool,

    /// Print segment headers, session boundaries, and summary information
    /// in addition to log messages.
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::try_parse().unwrap_or_else(|e| {
        eprintln!("{e}");
        let _ = Args::command().print_help();
        eprintln!();
        std::process::exit(2);
    });

    let mut log = LogRead::new(&args.dirname, &args.prefix, &args.suffix)?;
    // Only collect segment headers when they will be printed: the
    // reader buffers them until drained, and a non-verbose run never
    // drains.
    log.collect_opened_headers(args.verbose);
    let mut printed_header = false;
    let mut files_lost = 0u64;
    let mut total = 0u64;
    let mut buf = vec![0u8; MAX_MESSAGE_SIZE];

    loop {
        let read_result = log.read(&mut buf);
        // Report every segment this read traversed, not just the one a
        // record ended in: a record spanning several segments starts in
        // one that `current_header` never names. Drained before the
        // result is examined so the headers precede the record they
        // carried, and so a read that ends the loop still reports the
        // segments it opened.
        for h in log.take_opened_headers() {
            if printed_header {
                println!();
            }
            printed_header = true;
            print_header(&args.prefix, &args.suffix, &h);
        }
        match read_result {
            Ok(res) => {
                total += 1;
                print_record(args.text, res.meta, &buf[..res.n as usize]);
                println!();
            }
            Err(LogError::Eof) => break,
            Err(LogError::ReadTruncated(lost)) => {
                files_lost += lost;
                // A gap that falls on a record boundary loses whole
                // records rather than truncating one, and a gap before a
                // session's first surviving segment truncates nothing at
                // all, so the wording reports the loss without claiming
                // which.
                if args.verbose {
                    if lost > 0 {
                        println!(
                            "    -- {lost} missing segment file(s); \
                             resynchronizing --"
                        );
                    } else {
                        println!(
                            "    -- corrupted or truncated segment file; \
                             resynchronizing --"
                        );
                    }
                }
            }
            Err(LogError::ReadOverflow(n)) => {
                total += 1;
                // The captured bytes are real payload. Print them
                // instead of dropping them on the floor, marked so they
                // cannot be mistaken for a whole record.
                print_partial_record(args.text, &buf[..n as usize]);
                println!();
                if args.verbose {
                    println!(
                        "    (payload larger than {MAX_MESSAGE_SIZE}-byte buffer; \
                        {n} bytes captured, remainder discarded)"
                    );
                }
            }
            Err(LogError::SessionEnd) => {
                if args.verbose {
                    println!();
                    println!("--- End of Session---");
                }
            }
            Err(e) => {
                return Err(e.into());
            }
        }
    }

    if args.verbose {
        // Ask the reader how many segment files it traversed rather
        // than counting the headers printed above: a record spanning
        // several segments only ever reports the one it ended in, so
        // counting those undercounts the log's extent.
        println!(
            "\nread {total} message(s) across {} file(s)",
            log.segments_opened()
        );
        if files_lost > 0 {
            println!("{files_lost} segment file(s) lost");
        }
    }
    Ok(())
}

fn print_record(text: bool, meta: Meta, buf: &[u8]) {
    let msg = format_msg(text, buf);
    print!("    {msg} {}", record_trailer(buf.len(), meta));
}

/// Prints the leading bytes of a record too large for the read buffer.
/// The record's own length and metadata are not available on this path,
/// so the trailer states only what was captured.
fn print_partial_record(text: bool, buf: &[u8]) {
    let msg = format_msg(text, buf);
    print!("    {msg} ({} bytes captured, record truncated)", buf.len());
}

fn format_msg(as_text: bool, buf: &[u8]) -> String {
    if as_text {
        String::from_utf8_lossy(buf).to_string()
    } else {
        let mut text = String::new();
        for item in buf {
            text.push(*item as char);
        }
        text
    }
}

fn print_header(prefix: &str, suffix: &str, h: &SegmentHeader) {
    println!("=== segment file: {}{}{} ===", prefix, h.segment_id, suffix);
    println!("    segment_id: {}", h.segment_id);
    println!("    session_id: {}", h.session_id);
    println!("    max_size:   {}", h.max_size);
    println!("    remaining:  {}", h.remaining);
    println!("    format:     {:?}", h.format);
    println!("    sequence:   {}", h.sequence);
}
