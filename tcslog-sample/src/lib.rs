//! Helper library shared by the `tcslog-sample` binary and `tcslog-dump`:
//! creates a chain of sample log files filled with small ASCII messages.

use std::time::{SystemTime, UNIX_EPOCH};

use tcslog::{TcsLog, TcsLogError, Uid, Uidable, UidableError, Uider};

/// Each log file is limited to this many bytes.
pub const MAX_SIZE: u64 = 10_240;

/// Upper bound on the size of a single log message.
pub const MAX_MESSAGE_SIZE: usize = 1025;

/// Minimum number of messages to write, so the root file is populated even when
/// no rollover files are requested.
pub const MIN_MESSAGES: u64 = 5;

/// A deterministic Uider that starts at a given Uid and advances by
/// one microsecond per call. Useful for creating logs with predictable, known
/// file names. It steps by a microsecond (not a nanosecond) because log file
/// names have microsecond resolution, so distinct calls must map to distinct
/// file names to avoid collisions between successive files.
pub struct SequentialUider {
    next: Uid,
}

impl SequentialUider {
    /// Creates a Uider whose first Uid is `start`.
    pub fn new(start: Uid) -> Self {
        SequentialUider { next: start }
    }
}

impl Uidable for SequentialUider {
    fn Uid(&mut self) -> Result<Uid, UidableError> {
        let current = self.next;
        self.next = Uid::from_nanos(self.next.as_nanos() + 1_000);
        Ok(current)
    }
}

/// Summary of a created sample log chain.
pub struct SampleLogs {
    /// Name of the root (head) log file.
    pub root_file: String,
    /// Total number of messages written across the chain.
    pub message_count: u64,
    /// Total number of log files in the chain.
    pub file_count: u32,
}

/// Creates a sample log chain in `dir_name`, using the given file-name `prefix`
/// and `suffix` and the system clock for Uids: a root file plus
/// `rollovers` successor files filled with small ASCII messages.
///
/// Returns a summary including the name of the root file (the head of the
/// chain), which can be opened to read the whole chain back.
pub fn create_sample_logs(
    dir_name: &str,
    prefix: &str,
    suffix: &str,
    rollovers: u32,
) -> Result<SampleLogs, TcsLogError<'static>> {
    let mut Uider = Uider::new();
    create_sample_logs_with(dir_name, prefix, suffix, rollovers, &mut Uider)
}

/// Like [`create_sample_logs`], but uses the supplied `Uider` instead of
/// the system clock. Passing a [`SequentialUider`] makes the file names
/// deterministic, so the root file's Uid is known in advance.
pub fn create_sample_logs_with(
    dir_name: &str,
    prefix: &str,
    suffix: &str,
    rollovers: u32,
    Uider: &mut dyn Uidable,
) -> Result<SampleLogs, TcsLogError<'static>> {
    // Create the root log file and remember its name before any rollover.
    let mut log = TcsLog::new_with_Uid(dir_name, prefix, Uider, suffix, MAX_SIZE)?;
    let root_file = log.file_name().to_string();

    // Append messages until the requested number of rollover files exist (and
    // at least MIN_MESSAGES have been written, so the root file is populated).
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
        log.write(Uider, data)?;
    }
    log.flush()?;

    Ok(SampleLogs {
        root_file,
        message_count: log.message_count(),
        file_count: log.chain_count() + 1,
    })
}
