//! Helper library for the `tcslog-sample` binary: creates a chain of sample
//! segment files filled with small ASCII messages.

use std::time::{SystemTime, UNIX_EPOCH};

use tcslog::{Format, LogError, LogWrite, SEGMENT_FILE_HEADER_LEN,
    WriteCallbacks};

/// Maximum size in bytes of any single segment file. Sized to force
/// rollover after a small handful of `VariableTsRc` records so the
/// resulting chain is interesting to inspect with `tcslog-dump`.
pub const SEG_SIZE_MAX: u32 = SEGMENT_FILE_HEADER_LEN + 36;

/// Minimum number of messages to write
pub const MAX_MESSAGES: u64 = 3;

/// Summary of a created sample log chain.
pub struct SampleLogs {
    /// Base name of the first (root) segment file in the chain.
    pub root_file: String,
    /// Total number of messages written across the chain.
    pub message_count: u64,
}

/// Creates a sample log chain in `dir_name` using the given file-name
/// `prefix` and `suffix`: a root segment file plus `rollovers` successor
/// segment files, filled with small ASCII messages.
pub fn create_sample_logs(
    dir_name: &str,
    prefix: &str,
    suffix: &str,
) -> Result<SampleLogs, LogError> {
    println!("Segment file size: {}", SEG_SIZE_MAX);
    println!("Segment file header length: {}", SEGMENT_FILE_HEADER_LEN);
    println!("Data section length: {}", SEG_SIZE_MAX - SEGMENT_FILE_HEADER_LEN);
    println!();

    let mut log = LogWrite::new(
        dir_name,
        prefix,
        suffix,
        SEG_SIZE_MAX,
        Format::VariableTsRc,
        WriteCallbacks::default(),
    )?;

    let session_id = log.session_id();
    let root_file = format!("{prefix}{session_id}{suffix}");

    for i in 0..40 {
        if (i + 1) % 10 == 0 {
            print!("{}", (i + 1) / 10);
        } else {
            print!("{}", " ");
        }   
    }
    println!();

    for i in 0..40 {
        print!("{}", (i + 1) % 10);
    }
    println!();

    let mut message_count: u64 = 0;
    loop {
        let dur = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is before 1970");

        let secs = dur.as_secs();          // u64, whole seconds
        let nanos = dur.subsec_nanos();    // u32, 0..1_000_000_000

        let content = format!(
            "time={}.{:09}s message={}",
            secs,
            nanos,
            message_count + 1,
        );
        println!("{}", content);
        log.write(content.as_bytes())?;
        message_count += 1;
        if message_count >= MAX_MESSAGES {
            break;
        }
    }

    println!();

    Ok(SampleLogs {
        root_file,
        message_count,
    })
}
