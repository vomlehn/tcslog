//! Sample: create a log file capped at 10 KiB and append small log messages.
//! Each message is an ASCII string holding the current time, the zero-based
//! number of log files created so far, and the running total of messages
//! written. Messages are well under the 1025-byte limit, so many fit per file
//! and the log rolls over into successor files as they fill.
//!
//! The number of rollover (successor) files is an optional command-line
//! argument. If omitted it defaults to 0, so only the root log file is created
//! (and filled with a few messages).
//!
//! Run with:
//!
//! ```text
//! cargo run -p tcslog-sample          # just the root file
//! cargo run -p tcslog-sample -- 2     # root + 2 rollover files
//! ```

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use tcslog::{TcsLog, Timestamper};

/// Each log file is limited to this many bytes.
const MAX_SIZE: u64 = 10_240;

/// Upper bound on the size of a single log message.
const MAX_MESSAGE_SIZE: usize = 1025;

/// Minimum number of messages to write, so the root file is populated even when
/// no rollover files are requested.
const MIN_MESSAGES: u64 = 5;

fn main() -> Result<(), Box<dyn Error>> {
    // Optional first argument: the number of rollover files to create. When it
    // is absent we default to 0, leaving just the root log file.
    let rollovers: u32 = match std::env::args().nth(1) {
        Some(arg) => arg
            .parse()
            .map_err(|_| format!("invalid rollover count {arg:?}: expected a non-negative integer"))?,
        None => 0,
    };

    // Choose an OS-independent directory to hold the log files. Start fresh so a
    // re-run does not collide with files left by a previous run.
    let dir: PathBuf = std::env::temp_dir().join("tcslog-sample");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)?;
    let dir_name = dir.to_str().expect("temp dir path is valid UTF-8");

    let prefix = "sample-";
    let suffix = ".tcslog";

    // The timestamper supplies the monotonically increasing timestamps used for
    // both the file names and the per-record metadata.
    let mut timestamper = Timestamper::new();

    // Create the root log file.
    let mut log = TcsLog::new_with_timestamp(dir_name, prefix, &mut timestamper, suffix, MAX_SIZE)?;
    println!("created root log file: {}", log.file_name());

    // Append messages until the requested number of rollover files exist (and
    // at least MIN_MESSAGES have been written, so the root file is populated).
    // `chain_count()` is the number of rollover files created so far and
    // `message_count()` is the running total of messages written.
    while log.message_count() < MIN_MESSAGES || log.chain_count() < rollovers {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        let content = format!(
            "time={}.{:09}s files={} message={}",
            now.as_secs(),
            now.subsec_nanos(),
            log.chain_count(),
            log.message_count() + 1,
        );
        let data = &content.as_bytes()[..content.len().min(MAX_MESSAGE_SIZE)];

        log.write(&mut timestamper, data)?;
        // file_name() reflects the file the message landed in (after any rollover).
        println!("{} <- {content}", log.file_name());
    }
    log.flush()?;

    println!(
        "\ndone: {} message(s) across {} file(s) in {}",
        log.message_count(),
        log.chain_count() + 1,
        dir.display(),
    );

    let mut names: Vec<String> = fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for name in names {
        println!("  {name}");
    }

    Ok(())
}
