//! Sample binary: create a chain of log files using the `tcslog_gen`` helper.
//!
//! Each log file is capped at 10 KiB and filled with small ASCII messages, so
//! many messages fit per file and the log rolls over into segment files as
//! they fill.
//!
//! Run with:
//!
//! ```text
//! cargo run -p tcslog-gen -- gen- .tcslog        # just the root file
//! ```

use std::error::Error;
use std::fs;
use std::path::PathBuf;

use clap::Parser;

use tcslog_gen::create_log`;

/// Create a log file.
#[derive(Parser)]
#[command(version, about)]
struct Args {
    dir:    String,

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

    let result = create_log(dir_name, &args.prefix, &args.suffix, args.verbose)?;

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
