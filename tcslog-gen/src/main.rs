//! Binary: create a chain of log files using the `tcslog_gen` helper.
//!
//! Run with:
//!
//! ```text
//! cargo run -p tcslog-gen -- \
//!     --format variable-ts-rc:16..64 \
//!     ./out prefix- .tcslog
//! ```

use std::error::Error;
use std::fs;
use std::path::PathBuf;

use clap::Parser;

use tcslog_gen::{create_log, RecordFormatSpec};

/// Create a log file.
#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Directory in which to create the log chain.
    dir: String,

    /// Log file name prefix.
    prefix: String,

    /// Log file name suffix.
    suffix: String,

    /// Data record format. Syntax: `KIND:LEN` or `KIND:MIN..MAX` where
    /// KIND is one of `fixed`, `variable-simple`, `variable-ts-rc`.
    /// `fixed` requires a single length; the variable kinds accept either
    /// a single length or a `MIN..MAX` range.
    #[arg(short = 'f', long, default_value = "variable-simple:10")]
    format: RecordFormatSpec,

    /// Data section size, in bytes, of each segment file (excludes the
    /// segment header).
    #[arg(short = 'd', long, default_value_t = 16)]
    data_size: u32,

    /// Number of data records to write.
    #[arg(short = 'n', long = "number", default_value_t = 5)]
    number: u64,

    /// Print diagnostic information in addition to record data.
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::try_parse().unwrap_or_else(|e| e.exit());

    let dir = PathBuf::from(args.dir);
    fs::create_dir_all(&dir)?;
    let dir_name = dir.to_str().expect("temp dir path is valid UTF-8");

    let result = create_log(
        dir_name,
        &args.prefix,
        &args.suffix,
        args.format,
        args.data_size,
        args.number,
        args.verbose,
    )?;

    if args.verbose {
        println!(
            "wrote {} record(s) in {} (root: {})",
            result.message_count,
            dir.display(),
            result.root_file,
        );
    }

    Ok(())
}
