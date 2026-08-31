//! Creates a chain of sample segment files with the `tcslog_sample` helper,
//! then reads the entire chain back, printing each segment file's header and
//! every log message.
//!
//! Run with:
//!
//! ```text
//! cargo run -p tcslog-dump -- sample- .tcslog
//! ```

use std::error::Error;
use std::fs;
use std::path::PathBuf;

use clap::Parser;

use tcslog::{LogError, LogRead, Meta, SegmentHeader};
use tcslog_sample::{create_sample_logs, MAX_MESSAGE_SIZE};

/// Create a chain of segment files, then read the whole chain back and print
/// every header and message.
#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Segment file name prefix.
    prefix: String,

    /// Segment file name suffix.
    suffix: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    let dir: PathBuf = std::env::temp_dir().join("tcslog-dump");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)?;
    let dir_name = dir.to_str().expect("temp dir path is valid UTF-8");

    let sample = create_sample_logs(dir_name, &args.prefix, &args.suffix, args.rollovers)?;
    println!(
        "created {} segment file(s) ({} message(s)); root = {}\n",
        sample.file_count, sample.message_count, sample.root_file,
    );

    let mut log = LogRead::new(dir_name, &args.prefix, &args.suffix)?;
    let mut current_seg = None;
    let mut files_seen = 0u32;
    let mut total = 0u64;
    let mut buf = vec![0u8; MAX_MESSAGE_SIZE];

    loop {
        match log.read(&mut buf) {
            Ok(res) => {
                if let Some(h) = log.current_header() {
                    if current_seg != Some(h.segment_id) {
                        if current_seg.is_some() {
                            println!();
                        }
                        current_seg = Some(h.segment_id);
                        files_seen += 1;
                        print_header(&args.prefix, &args.suffix, h);
                    }
                }
                total += 1;
                let text = String::from_utf8_lossy(&buf[..res.n as usize]);
                match res.meta {
                    Meta::VariableTsrn(ts, rn) => {
                        println!("    msg {rn}: ts={ts} {:?}", text.trim_end());
                    }
                    Meta::VariableSimple | Meta::Fixed => {
                        println!("    msg {total}: {:?}", text.trim_end());
                    }
                }
            }
            Err(LogError::Eof) => break,
            Err(e) => return Err(e.into()),
        }
    }

    println!("\nread {total} message(s) across {} file(s)", files_seen);
    Ok(())
}

fn print_header(prefix: &str, suffix: &str, h: &SegmentHeader) {
    println!("=== segment file: {}{}{} ===", prefix, h.segment_id, suffix);
    println!("    segment_id: {}", h.segment_id);
    println!("    session_id: {}", h.session_id);
    println!("    max_size:   {}", h.max_size);
    println!("    remaining:  {}", h.remaining);
    println!("    format:     {:?}", h.format);
}
