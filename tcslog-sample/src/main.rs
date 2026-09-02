//! Sample binary: create a chain of log files using the `tcslog_sample` helper.
//!
//! Each log file is capped at 10 KiB and filled with small ASCII messages, so
//! many messages fit per file and the log rolls over into segment files as
//! they fill.
//!
//! Run with:
//!
//! ```text
//! cargo run -p tcslog-sample -- sample- .tcslog        # just the root file
//! ```

use std::error::Error;
use std::fs;
use std::path::PathBuf;

use clap::Parser;

use tcslog_sample::create_sample_logs;

/// Create a chain of sample log files.
#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Log file name prefix.
    prefix: String,

    /// Log file name suffix.
    suffix: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    // Choose an OS-independent directory to hold the log files. Start fresh so a
    // re-run does not collide with files left by a previous run.
    let dir: PathBuf = std::env::temp_dir().join("tcslog-sample");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)?;
    let dir_name = dir.to_str().expect("temp dir path is valid UTF-8");

    let result = create_sample_logs(dir_name, &args.prefix, &args.suffix)?;

    println!(
        "wrote {} message(s) in {} (root: {})",
        result.message_count,
        dir.display(),
        result.root_file,
    );

    Ok(())
}
