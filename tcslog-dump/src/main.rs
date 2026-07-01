//! Creates a chain of seven log files with the `tcslog_sample` helper, then
//! reads the entire chain back from the root file, printing each log file's
//! header and every log message.
//!
//! The log file name prefix, suffix, and timestamp are required arguments. The
//! timestamp is given in the same format used in log file names — six
//! underscore-separated groups of four hex digits (microseconds) — and is used
//! to seed log creation, so the root file is named with exactly that timestamp.
//!
//! Run with:
//!
//! ```text
//! cargo run -p tcslog-dump -- sample- .tcslog 0000_0000_0006_18bd_f941_4276
//! ```

use std::error::Error;
use std::fs;
use std::path::PathBuf;

use clap::Parser;

use tcslog::{Header, TcsLog, TcsLogError, Timestamp};
use tcslog_sample::{create_sample_logs_with, SequentialTimestamper, MAX_MESSAGE_SIZE};

/// Number of rollover files to create. With the root file this makes seven
/// files in total.
const ROLLOVERS: u32 = 6;

/// Create a chain of log files, then read the whole chain back, printing each
/// file's header and every log message.
#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Log file name prefix.
    prefix: String,

    /// Log file name suffix.
    suffix: String,

    /// Starting timestamp, in log-file name format: six underscore-separated
    /// groups of four hex digits, in microseconds
    /// (e.g. 0000_0000_0006_18bd_f941_4276).
    #[arg(value_parser = parse_timestamp)]
    timestamp: Timestamp,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    // Use an OS-independent directory, cleared first so a re-run is repeatable.
    let dir: PathBuf = std::env::temp_dir().join("tcslog-dump");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)?;
    let dir_name = dir.to_str().expect("temp dir path is valid UTF-8");

    // Create the seven-file chain, seeded with the supplied timestamp so the
    // root file is named with exactly that timestamp.
    let mut timestamper = SequentialTimestamper::new(args.timestamp);
    let sample =
        create_sample_logs_with(dir_name, &args.prefix, &args.suffix, ROLLOVERS, &mut timestamper)?;
    println!(
        "created {} log file(s) ({} message(s)); root = {}\n",
        sample.file_count, sample.message_count, sample.root_file,
    );

    // Open the root file and walk the whole chain. `read` follows the chain into
    // each successor file automatically, so a change in `file_name()` tells us
    // when we have entered a new file.
    let root_path = dir.join(&sample.root_file);
    let mut log = TcsLog::open_path(&root_path)?;

    print_header(&log);
    let mut current_file = log.file_name().to_string();

    let mut timestamp = Timestamp::ZERO;
    let mut buf = vec![0u8; MAX_MESSAGE_SIZE];
    let mut total = 0u64;
    loop {
        match log.read(&mut timestamp, &mut buf) {
            Ok(n) => {
                // A new file was entered while following the chain.
                if log.file_name() != current_file {
                    current_file = log.file_name().to_string();
                    println!();
                    print_header(&log);
                }
                total += 1;
                let text = String::from_utf8_lossy(&buf[..n]);
                println!(
                    "    msg {total}: ts={} {:?}",
                    timestamp.as_nanos(),
                    text.trim_end(),
                );
            }
            Err(TcsLogError::EOF) => break,
            Err(e) => return Err(e.into()),
        }
    }

    println!("\nread {total} message(s) across {} file(s)", sample.file_count);
    Ok(())
}

/// Parses a timestamp written in the same format used in log file names: six
/// underscore-separated groups of four lowercase hex digits, giving the
/// timestamp in microseconds. Returns an error describing the expected format
/// on bad input.
fn parse_timestamp(s: &str) -> Result<Timestamp, String> {
    let groups: Vec<&str> = s.split('_').collect();
    let well_formed = groups.len() == 6
        && groups
            .iter()
            .all(|g| g.len() == 4 && g.bytes().all(|b| b.is_ascii_hexdigit()));
    if !well_formed {
        return Err(format!(
            "invalid timestamp {s:?}: expected six underscore-separated groups of four hex \
             digits, e.g. 0000_0000_0006_18bd_f941_4276"
        ));
    }

    let hex: String = groups.concat();
    let micros =
        u128::from_str_radix(&hex, 16).map_err(|e| format!("invalid timestamp {s:?}: {e}"))?;

    // Guard against values too large to represent (Timestamp stores seconds as a u64).
    if micros / 1_000_000 >= u64::MAX as u128 {
        return Err(format!("timestamp {s:?} is out of range"));
    }

    Ok(Timestamp::from_micros(micros))
}

/// Prints the header of the log file `log` currently refers to.
fn print_header(log: &TcsLog) {
    let h: &Header = log.header();
    println!("=== log file: {} ===", log.file_name());
    println!("    type:         {:?}", String::from_utf8_lossy(&h.file_type));
    println!("    version:      {:?}", String::from_utf8_lossy(&h.version));
    println!("    timestamp:    {} ns", h.timestamp.as_nanos());
    println!("    index_offset: {}", h.index_offset);
    println!("    data_offset:  {}", h.data_offset);
    println!("    chain_count:  {}", h.chain_count);
}
