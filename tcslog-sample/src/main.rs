//! Sample binary: create a chain of log files using the `tcslog_sample` helper.
//!
//! Each log file is capped at 10 KiB and filled with small ASCII messages, so
//! many messages fit per file and the log rolls over into successor files as
//! they fill.
//!
//! The log file name prefix and suffix are required arguments. The number of
//! rollover (successor) files is optional and defaults to 0 (root file only).
//!
//! Run with:
//!
//! ```text
//! cargo run -p tcslog-sample -- sample- .tcslog        # just the root file
//! cargo run -p tcslog-sample -- sample- .tcslog 2      # root + 2 rollover files
//! ```

use std::error::Error;
use std::fs;
use std::path::PathBuf;

use tcslog_sample::create_sample_logs;

const USAGE: &str = "usage: tcslog-sample <prefix> <suffix> [rollovers]";

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();

    // Required arguments: the file-name prefix and suffix.
    let prefix = args.get(1).ok_or(USAGE)?;
    let suffix = args.get(2).ok_or(USAGE)?;

    // Optional third argument: the number of rollover files to create. When it
    // is absent we default to 0, leaving just the root log file.
    let rollovers: u32 = match args.get(3) {
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

    let result = create_sample_logs(dir_name, prefix, suffix, rollovers)?;

    println!(
        "wrote {} message(s) across {} file(s) in {} (root: {})",
        result.message_count,
        result.file_count,
        dir.display(),
        result.root_file,
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
