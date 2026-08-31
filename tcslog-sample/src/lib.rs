//! Helper library for the `tcslog-sample` binary: creates a chain of sample
//! segment files filled with small ASCII messages.

use std::time::{SystemTime, UNIX_EPOCH};

use tcslog::{Format, LogError, LogWrite};

/// Maximum size in bytes of any single segment file.
pub const SEG_SIZE_MAX: u32 = 100;

/// Maximum number of segment files kept on disk at once.
pub const N_SEG: u32 = 3;

/// Upper bound on the size of a single log message.
pub const MAX_MESSAGE_SIZE: usize = 40;

/// Minimum number of messages to write
pub const MAX_MESSAGES: u64 = 5;

/// Summary of a created sample log chain.
pub struct SampleLogs {
    /// Base name of the first (root) segment file in the chain.
    pub root_file: String,
    /// Total number of messages written across the chain.
    pub message_count: u64,
    /// Total number of segment files in the chain.
    pub file_count: u32,
}

/// Creates a sample log chain in `dir_name` using the given file-name
/// `prefix` and `suffix`: a root segment file plus `rollovers` successor
/// segment files, filled with small ASCII messages.
pub fn create_sample_logs(
    dir_name: &str,
    prefix: &str,
    suffix: &str,
) -> Result<SampleLogs, LogError> {
    let mut log = LogWrite::new(
        dir_name,
        prefix,
        suffix,
        SEG_SIZE_MAX,
        N_SEG,
        Format::VariableTsRn,
    )?;

    let session_id = log.session_id();
    let root_file = format!("{prefix}{session_id}{suffix}");

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
        let data = &content.as_bytes()[..content.len().min(MAX_MESSAGE_SIZE)];
        log.write(data)?;
        message_count += 1;
        if message_count >= MAX_MESSAGES {
            break;
        }
    }

    let file_count = (log.current_segment_id().as_u64() - session_id.as_u64() + 1) as u32;

    Ok(SampleLogs {
        root_file,
        message_count,
        file_count,
    })
}
