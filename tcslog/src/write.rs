//! Log writer.
//!
//! [`LogWrite`] owns a chain of segment files, rotating to a new segment when
//! the current one reaches its size cap. It exposes a byte-oriented `write`
//! API (plus a `write_str` convenience wrapper) and delegates two decisions to
//! caller-supplied callbacks packaged in [`WriteCallbacks`]:
//!
//! * `record_complete` — invoked after a full data record has been written,
//!   which is the natural point to `flush()` when durability matters.
//! * `send` — invoked with the path of every completed segment file. When it
//!   returns the file at that path must no longer exist, so that tcslog is
//!   free to consider that storage available again.

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::format::{Format, RecordCount};
use crate::header::{SegmentHeader, SEGMENT_HEADER_SIZE};
use crate::io_util::{is_segment_name, validate_prefix_suffix};
use crate::segid::SegId;
use crate::LogError;

/// Assumed timer resolution in nanoseconds. Successive reads of the wall
/// clock are guaranteed to differ after sleeping for `2 * TIMER_RESOLUTION`
/// nanoseconds, which is how tcslog produces unique segment IDs.
///
/// Set at compile time via the `timer_resolution_ns` env var if a platform's
/// timer is coarser than 1 microsecond; the default is 1_000 ns (1 µs).
pub const TIMER_RESOLUTION: u64 = 1_000;

/// User-supplied callbacks invoked by [`LogWrite`] at well-defined points.
///
/// Both fields are optional. When they are `None` tcslog behaves as if the
/// user supplied a no-op callback: nothing is flushed and completed segment
/// files are left on disk under their original names.
pub struct WriteCallbacks {
    /// Called after every full data record has been written. A typical
    /// implementation flushes the file to reduce the window of data loss on
    /// crash; the default implementation does nothing.
    pub record_complete: Option<Box<dyn Fn(&File) -> std::io::Result<()> + Send + Sync>>,
    /// Called with the path of every segment file that has just filled up.
    /// When `send` returns, the file at that path must not exist any more
    /// (it may have been renamed, moved, or deleted).
    pub send: Option<Box<dyn Fn(&str) -> std::io::Result<()> + Send + Sync>>,
}

impl Default for WriteCallbacks {
    fn default() -> Self {
        WriteCallbacks {
            record_complete: None,
            send: None,
        }
    }
}

impl std::fmt::Debug for WriteCallbacks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WriteCallbacks")
            .field("record_complete", &self.record_complete.is_some())
            .field("send", &self.send.is_some())
            .finish()
    }
}

/// A tcslog log open for writing.
#[derive(Debug)]
pub struct LogWrite {
    dir: PathBuf,
    prefix: String,
    suffix: String,
    seg_size_max: u32,
    format: Format,
    callbacks: WriteCallbacks,

    // Currently-open segment file and where the next byte will be written.
    file: File,
    current_path: PathBuf,
    current_segment_id: SegId,
    current_len: u32,

    session_id: SegId,
    record_count: RecordCount,

    // Scratch buffer big enough for any data-header this crate defines.
    header_buf: [u8; 20],

    // Highest wall-clock nanosecond value already used as a segment ID, so
    // successive segments are guaranteed unique.
    last_time_ns: u64,
}

impl LogWrite {
    /// Open a new log for writing.
    ///
    /// * `dir` — directory that will hold the segment files. Created if
    ///   missing.
    /// * `prefix`, `suffix` — segment file name fixtures wrapped around the
    ///   generated segment ID. Neither may contain a path separator.
    /// * `seg_size_max` — cap on the size in bytes of any single segment
    ///   file. Must leave room for the segment header plus at least one
    ///   data header byte and one payload byte.
    /// * `format` — record format applied to every write on this log.
    /// * `write_callbacks` — user hooks invoked after each completed record
    ///   and each rotated-out segment file. Pass `WriteCallbacks::default()`
    ///   for the "leave every completed segment on disk" behavior.
    ///
    /// Per the tcslog specification, every pre-existing segment file whose
    /// name matches `<prefix><id><suffix>` is handed to the user's `send`
    /// callback (if any) before the first new segment is opened.
    ///
    /// Returns the newly-created [`LogWrite`], or a [`LogError`] if
    /// validation, directory setup, or the initial segment write fails.
    pub fn new(
        dir: &str,
        prefix: &str,
        suffix: &str,
        seg_size_max: u32,
        format: Format,
        write_callbacks: WriteCallbacks,
    ) -> Result<Self, LogError> {
        validate_prefix_suffix(prefix, suffix)?;
        validate_format(format)?;

        let min_size = SEGMENT_HEADER_SIZE + format.header_size() + 1;
        if (seg_size_max as usize) < min_size {
            return Err(LogError::SegSizeTooSmall(seg_size_max));
        }

        let dir_pb = PathBuf::from(dir);
        fs::create_dir_all(&dir_pb)?;

        // Per spec: hand off every pre-existing segment file to the user
        // before creating any new ones.
        if let Some(cb) = write_callbacks.send.as_ref() {
            for entry in fs::read_dir(&dir_pb)? {
                let entry = entry?;
                let name_os = entry.file_name();
                let name = name_os.to_string_lossy();
                if is_segment_name(&name, prefix, suffix) {
                    let path = entry.path();
                    let path_str = path.to_string_lossy();
                    cb(&path_str)?;
                }
            }
        }

        let (file, seg_id, path, last_time_ns) =
            create_new_segment_file(&dir_pb, prefix, suffix, 0)?;

        let mut me = LogWrite {
            dir: dir_pb,
            prefix: prefix.to_string(),
            suffix: suffix.to_string(),
            seg_size_max,
            format,
            callbacks: write_callbacks,
            file,
            current_path: path,
            current_segment_id: seg_id,
            current_len: 0,
            session_id: seg_id,
            record_count: 0,
            header_buf: [0u8; 20],
            last_time_ns,
        };

        let hdr = SegmentHeader {
            segment_id: seg_id,
            session_id: seg_id,
            max_size: seg_size_max,
            remaining: 0,
            format,
        };
        hdr.write_to(&mut me.file)?;
        me.current_len = SEGMENT_HEADER_SIZE as u32;

        Ok(me)
    }

    /// Segment ID of the first segment file created for this session.
    pub fn session_id(&self) -> SegId {
        self.session_id
    }

    /// Segment ID of the segment file currently being written to.
    pub fn current_segment_id(&self) -> SegId {
        self.current_segment_id
    }

    /// Convenience wrapper: write the bytes of `msg` as a single record.
    /// Returns the number of payload bytes written.
    pub fn write_str(&mut self, msg: &str) -> Result<u32, LogError> {
        self.write(msg.as_bytes())
    }

    /// Write `data` as a single record, rotating to a new segment file
    /// (possibly more than once) if necessary. Returns the number of payload
    /// bytes written, which is always `data.len()` on success.
    pub fn write(&mut self, data: &[u8]) -> Result<u32, LogError> {
        // Fixed(n) enforces a per-record size.
        if let Format::Fixed(n) = self.format {
            if data.len() as u64 != n as u64 {
                return Err(LogError::InvalidConfig(
                    "payload length does not match Fixed(n)",
                ));
            }
        }

        let hdr_len = self.build_data_header(data.len());
        // Ensure the data header fits contiguously in one segment. Readers
        // rely on this invariant so they only have to worry about payload
        // bytes crossing segment boundaries, not header bytes.
        let space = self.seg_size_max.saturating_sub(self.current_len) as usize;
        if space < hdr_len {
            self.rotate(0)?;
        }
        // Copy out the header bytes so we don't hold a borrow on `self`
        // across the write.
        let mut hdr_copy = [0u8; 20];
        hdr_copy[..hdr_len].copy_from_slice(&self.header_buf[..hdr_len]);
        self.file.write_all(&hdr_copy[..hdr_len])?;
        self.current_len += hdr_len as u32;

        // Payload may span segments; each rotation advertises the
        // still-to-write payload length in the new segment header.
        let mut written = 0usize;
        let mut bytes = data;
        while !bytes.is_empty() {
            let space = self.seg_size_max as usize - self.current_len as usize;
            if space == 0 {
                let left = (data.len() - written) as u32;
                self.rotate(left)?;
                continue;
            }
            let n = space.min(bytes.len());
            self.file.write_all(&bytes[..n])?;
            self.current_len += n as u32;
            written += n;
            bytes = &bytes[n..];
        }

        if let Some(cb) = self.callbacks.record_complete.as_ref() {
            cb(&self.file)?;
        }

        Ok(data.len() as u32)
    }

    /// Delete every existing segment file for this log from disk. The log
    /// remains open on its current segment; further writes go into that
    /// (possibly newly-empty) segment.
    pub fn clear(&mut self) -> Result<(), LogError> {
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let name_os = entry.file_name();
            let name = name_os.to_string_lossy();
            if is_segment_name(&name, &self.prefix, &self.suffix) {
                fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }

    // ----- internals ------------------------------------------------------

    /// Encode the data header for a record with `payload_len` payload bytes
    /// into `self.header_buf`, returning the number of header bytes.
    fn build_data_header(&mut self, payload_len: usize) -> usize {
        let n = payload_len as u32;
        match self.format {
            Format::Fixed(_) => 0,
            Format::VariableSimple => {
                self.header_buf[0..4].copy_from_slice(&n.to_le_bytes());
                4
            }
            Format::VariableTsRc => {
                let ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0);
                self.record_count = self.record_count.wrapping_add(1);
                self.header_buf[0..4].copy_from_slice(&n.to_le_bytes());
                self.header_buf[4..12].copy_from_slice(&ts.to_le_bytes());
                self.header_buf[12..20].copy_from_slice(&self.record_count.to_le_bytes());
                20
            }
        }
    }

    /// Close the current segment file (notifying the user via `send`) and
    /// open a new one whose header has `remaining_in_record` in the
    /// `remaining` field.
    fn rotate(&mut self, remaining_in_record: u32) -> Result<(), LogError> {
        // Best-effort flush before handing the file off to user code.
        let _ = self.file.flush();
        let old_path = self.current_path.clone();

        let (new_file, new_seg_id, new_path, last_time_ns) = create_new_segment_file(
            &self.dir,
            &self.prefix,
            &self.suffix,
            self.last_time_ns,
        )?;
        self.last_time_ns = last_time_ns;

        // Drop the old handle before invoking the user's `send` callback so
        // the callback is free to rename or delete the file even on
        // platforms that disallow it while a handle is open.
        let old_file = std::mem::replace(&mut self.file, new_file);
        drop(old_file);
        self.current_path = new_path;
        self.current_segment_id = new_seg_id;
        self.current_len = 0;

        let hdr = SegmentHeader {
            segment_id: new_seg_id,
            session_id: self.session_id,
            max_size: self.seg_size_max,
            remaining: remaining_in_record,
            format: self.format,
        };
        hdr.write_to(&mut self.file)?;
        self.current_len = SEGMENT_HEADER_SIZE as u32;

        if let Some(cb) = self.callbacks.send.as_ref() {
            let s = old_path.to_string_lossy();
            cb(&s)?;
        }

        Ok(())
    }
}

/// Reject formats whose parameters are outside the supported range.
fn validate_format(format: Format) -> Result<(), LogError> {
    if let Format::Fixed(n) = format {
        if n == 0 {
            return Err(LogError::InvalidConfig("Fixed(0) is not allowed"));
        }
    }
    Ok(())
}

/// Try to open a fresh segment file, retrying if the wall clock hasn't
/// advanced past `last_time_ns` or if the generated name is already taken.
fn create_new_segment_file(
    dir: &Path,
    prefix: &str,
    suffix: &str,
    last_time_ns: u64,
) -> Result<(File, SegId, PathBuf, u64), LogError> {
    let sleep = Duration::from_nanos(TIMER_RESOLUTION.saturating_mul(2));
    loop {
        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| LogError::InvalidConfig("system clock before UNIX epoch"))?
            .as_nanos() as u64;
        if now_ns > last_time_ns {
            let seg_id = SegId::new(now_ns);
            let name = format!("{}{}{}", prefix, seg_id, suffix);
            let path = dir.join(&name);
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(f) => return Ok((f, seg_id, path, now_ns)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => { /* retry */ }
                Err(e) => return Err(e.into()),
            }
        }
        thread::sleep(sleep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use tempfile::tempdir;

    #[test]
    fn seg_size_too_small() {
        let dir = tempdir().unwrap();
        let err = LogWrite::new(
            dir.path().to_str().unwrap(),
            "p_",
            "_s",
            8,
            Format::VariableSimple,
            WriteCallbacks::default(),
        )
        .unwrap_err();
        assert!(matches!(err, LogError::SegSizeTooSmall(8)));
    }

    #[test]
    fn prefix_delimiter_rejected() {
        let dir = tempdir().unwrap();
        let err = LogWrite::new(
            dir.path().to_str().unwrap(),
            "bad/pre",
            "_s",
            1024,
            Format::VariableSimple,
            WriteCallbacks::default(),
        )
        .unwrap_err();
        assert!(matches!(err, LogError::HasDelimiter));
    }

    #[test]
    fn fixed_zero_rejected() {
        let dir = tempdir().unwrap();
        let err = LogWrite::new(
            dir.path().to_str().unwrap(),
            "p_",
            "_s",
            1024,
            Format::Fixed(0),
            WriteCallbacks::default(),
        )
        .unwrap_err();
        assert!(matches!(err, LogError::InvalidConfig(_)));
    }

    #[test]
    fn writes_first_segment_header() {
        let dir = tempdir().unwrap();
        let mut w = LogWrite::new(
            dir.path().to_str().unwrap(),
            "p_",
            "_s",
            1024,
            Format::VariableSimple,
            WriteCallbacks::default(),
        )
        .unwrap();
        w.write(b"hi").unwrap();
        let files: Vec<_> = fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(files.len(), 1);
    }
}
