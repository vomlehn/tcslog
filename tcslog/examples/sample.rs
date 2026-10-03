//! Writes a small demonstration log, so that this example and
//! `tcslog-dump` form a runnable pair for someone meeting the crate for
//! the first time.
//!
//! The segment size is deliberately tiny, so the log rolls over after a
//! handful of records and the resulting chain is interesting to inspect.
//! It takes the log directory, prefix, and suffix as positional
//! arguments -- the same three `tcslog-dump` takes, so a log written
//! here is read back by that:
//!
//! ```text
//! cargo run --example sample -- /tmp/demo demo- .seg --verbose
//! ./bin/tcslog-tool tcslog-dump /tmp/demo demo- .seg --verbose
//! ```

use std::error::Error;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;

use tcslog::{Format, LogError, LogWrite, WriteCallbacks, SEGMENT_FILE_HEADER_LEN};

/// Maximum size in bytes of any single segment file. Sized to force
/// rollover after a small handful of `VariableTsRc` records so the
/// resulting chain is interesting to inspect with `tcslog-dump`.
const SEG_SIZE_MAX: u32 = SEGMENT_FILE_HEADER_LEN + 36;

/// Minimum number of messages to write
const MAX_MESSAGES: u64 = 3;

/// Summary of a created sample log chain.
struct SampleLogs {
    /// Base name of the first (root) segment file in the chain.
    root_file: String,
    /// Total number of messages written across the chain.
    message_count: u64,
}

// Both keep the fallible signatures declared by `WriteCallbacks` so
// they can be stored in those function-pointer fields.
#[allow(clippy::unnecessary_wraps)]
fn noop_record_complete(_f: &mut File) -> std::io::Result<()> {
    Ok(())
}

#[allow(clippy::unnecessary_wraps)]
fn send(p: &Path) -> std::io::Result<()> {
    println!("--> Send file {}", p.display());
    Ok(())
}

const SAMPLE_WRITE_CALLBACKS: WriteCallbacks = WriteCallbacks {
    record_complete: noop_record_complete,
    send,
};

/// Creates a sample log chain in `dir_name` using the given file-name
/// `prefix` and `suffix`: a root segment file plus successor segment
/// files, filled with small ASCII messages.
///
/// # Errors
///
/// Returns any [`LogError`] produced while creating segment files or
/// writing records.
///
/// # Panics
///
/// Panics if the system clock is set before the UNIX epoch.
fn create_sample_logs(
    dir_name: &str,
    prefix: &str,
    suffix: &str,
    verbose: bool,
) -> Result<SampleLogs, LogError> {
    if verbose {
        println!("Segment file size: {SEG_SIZE_MAX}");
        println!("Segment file header length: {SEGMENT_FILE_HEADER_LEN}");
        println!(
            "Data section length: {}",
            SEG_SIZE_MAX - SEGMENT_FILE_HEADER_LEN
        );
        println!();
    }

    let mut log = LogWrite::new(
        dir_name,
        prefix,
        suffix,
        SEG_SIZE_MAX,
        Format::VariableTsRc,
        SAMPLE_WRITE_CALLBACKS,
    )?;

    let session_id = log.session_id();
    let root_file = format!("{prefix}{session_id}{suffix}");

    if verbose {
        for i in 0..40 {
            if (i + 1) % 10 == 0 {
                print!("{}", (i + 1) / 10);
            } else {
                print!(" ");
            }
        }
        println!();

        for i in 0..40 {
            print!("{}", (i + 1) % 10);
        }
        println!();
    }

    let mut message_count: u64 = 0;
    loop {
        let dur = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is before 1970");

        let secs = dur.as_secs(); // u64, whole seconds
        let nanos = dur.subsec_nanos(); // u32, 0..1_000_000_000

        let content = format!("time={secs}.{nanos:09}s message={}", message_count + 1,);
        println!("{content}");
        log.write(content.as_bytes())?;
        message_count += 1;
        if message_count >= MAX_MESSAGES {
            break;
        }
    }

    if verbose {
        println!();
    }

    Ok(SampleLogs {
        root_file,
        message_count,
    })
}

/// Create a chain of sample log files.
#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Directory in which to create the log chain.
    dir: String,

    /// Log file name prefix.
    prefix: String,

    /// Log file name suffix.
    suffix: String,

    /// Print diagnostic information in addition to record data.
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::try_parse().unwrap_or_else(|e| e.exit());

    let dir = PathBuf::from(args.dir);
    fs::create_dir_all(&dir)?;
    let dir_name = dir.to_str().expect("temp dir path is valid UTF-8");

    let result = create_sample_logs(dir_name, &args.prefix, &args.suffix, args.verbose)?;

    if args.verbose {
        println!(
            "wrote {} message(s) in {} (root: {})",
            result.message_count,
            dir.display(),
            result.root_file,
        );
    }

    Ok(())
}
