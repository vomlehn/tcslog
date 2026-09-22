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
    let mut current_seg = None;
    let mut files_seen = 0u32;
    let mut files_lost = 0u64;
    let mut total = 0u64;
    let mut buf = vec![0u8; MAX_MESSAGE_SIZE];

    loop {
        let read_result = log.read(&mut buf);
        match read_result {
            Ok(res) => {
                if let Some(h) = log.current_header() {
                    if current_seg != Some(h.segment_id) {
                        if args.verbose && current_seg.is_some() {
                            println!();
                        }
                        current_seg = Some(h.segment_id);
                        files_seen += 1;
                        if args.verbose {
                            print_header(&args.prefix, &args.suffix, h);
                        }
                    }
                }
                total += 1;
                print_record(args.text, res.meta, &buf[..res.n as usize]);
                println!();
            }
            Err(LogError::Eof) => break,
            Err(LogError::ReadTruncated(lost)) => {
                files_lost += lost;
                if args.verbose {
                    if lost > 0 {
                        println!(
                            "    -- record truncated by {lost} missing \
                             segment file(s); resynchronizing --"
                        );
                    } else {
                        println!(
                            "    -- record truncated by a corrupted \
                             segment; resynchronizing --"
                        );
                    }
                }
            }
            Err(LogError::ReadOverflow(n)) => {
                total += 1;
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
                continue;
            }
            Err(e) => {
                return Err(e.into());
            }
        }
    }

    if args.verbose {
        println!("\nread {total} message(s) across {} file(s)", files_seen);
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
