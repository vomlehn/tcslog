//! Log writer implementation.

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::LogError;
use crate::format::{Format, RecSize};
use crate::header::{SegmentHeader, SEGMENT_FILE_HEADER_LEN};
use crate::segid::SegId;
use crate::util::{
    check_no_path_delim, enumerate_segments, segment_file_name, segment_path,
};
use crate::TIMER_RESOLUTION_NS;

/// Largest per-record data header any [`Format`] produces. Sized to the
/// [`Format::VariableTsRc`] layout (`RecSize` + `Timestamp` +
/// `RecordCount` = 4 + 8 + 8 bytes) so that the record-header build path
/// can avoid heap allocation.
const MAX_DATA_HEADER_LEN: usize = 20;

/// User-supplied callbacks invoked while writing.
///
/// Function pointers (not trait objects or closures) are used so that
/// [`WriteCallbacks`] can be stored inline in [`LogWrite`] without heap
/// allocation and without dynamic dispatch.
#[derive(Copy, Clone, Debug)]
pub struct WriteCallbacks {
    /// Called after every data record has been fully written. Typical
    /// implementations either flush the underlying file or leave it
    /// untouched, trading durability for throughput.
    pub record_complete: fn(&mut File) -> std::io::Result<()>,
    /// Invoked with the full path of a segment file whose data section
    /// has filled, and again with any pre-existing segment files
    /// discovered by [`LogWrite::new`].
    ///
    /// Upon return, the named file must either be deleted or renamed so
    /// that it no longer matches the segment-file pattern for this log.
    /// The [`WriteCallbacks::default`] implementation is a no-op suitable
    /// for local development; production users should replace it.
    pub send: fn(&Path) -> std::io::Result<()>,
}

fn noop_record_complete(_f: &mut File) -> std::io::Result<()> {
    Ok(())
}

fn noop_send(_p: &Path) -> std::io::Result<()> {
    Ok(())
}

impl Default for WriteCallbacks {
    fn default() -> WriteCallbacks {
        WriteCallbacks {
            record_complete: noop_record_complete,
            send: noop_send,
        }
    }
}

/// Handle for writing telemetry data into a segmented log.
#[derive(Debug)]
pub struct LogWrite {
    dir: PathBuf,
    prefix: String,
    suffix: String,
    seg_size_max: u32,
    format: Format,
    callbacks: WriteCallbacks,
    session_id: SegId,
    segment_id: SegId,
    current_path: PathBuf,
    file: Option<File>,
    file_pos: u32,
    record_bytes_left: u64,
    record_count: u64,
}

impl LogWrite {
    /// Creates or extends the log identified by `dir`, `prefix`, and
    /// `suffix`, writing records in `format`.
    ///
    /// * `dir` - Directory in which segment files live. Must already
    ///   exist.
    /// * `prefix` - Prefix that appears at the start of every segment
    ///   file's name. Must not contain a path separator.
    /// * `suffix` - Suffix that appears at the end of every segment
    ///   file's name. Must not contain a path separator.
    /// * `seg_size_max` - Maximum size, in bytes, of any single segment
    ///   file. Must be at least [`SEGMENT_FILE_HEADER_LEN`] plus one
    ///   data-record header.
    /// * `format` - Layout used to store records.
    /// * `callbacks` - User callbacks invoked at various points; see
    ///   [`WriteCallbacks`].
    ///
    /// Every segment file already present in `dir` that matches the
    /// prefix and suffix is handed to `callbacks.send` before the new
    /// session's first segment file is created.
    pub fn new(
        dir: &str,
        prefix: &str,
        suffix: &str,
        seg_size_max: u32,
        format: Format,
        callbacks: WriteCallbacks,
    ) -> Result<LogWrite, LogError> {
        // Runtime belt-and-suspenders: build.rs already refuses a
        // set-but-zero value, but a caller could still land here with
        // TIMER_RESOLUTION_NS==0 if the default in build.rs is ever
        // relaxed. Fail cleanly rather than looping in
        // create_segment_file.
        if TIMER_RESOLUTION_NS == 0 {
            return Err(LogError::TimerResolutionZero);
        }

        check_no_path_delim(prefix)?;
        check_no_path_delim(suffix)?;

        let min_size = SEGMENT_FILE_HEADER_LEN + format.data_header_len();
        if seg_size_max <= min_size {
            return Err(LogError::SegSizeTooSmall);
        }
        if let Format::Fixed(n) = format {
            if n == 0 {
                return Err(LogError::FixedLenMismatch);
            }
        }

        let dir_path = PathBuf::from(dir);
        if !dir_path.is_dir() {
            return Err(LogError::InvalidPathname);
        }

        for id in enumerate_segments(&dir_path, prefix, suffix)? {
            let path = segment_path(&dir_path, prefix, id, suffix);
            (callbacks.send)(&path).map_err(LogError::IoError)?;
        }

        let (segment_id, current_path, mut file) =
            create_segment_file(&dir_path, prefix, suffix)?;
        let session_id = segment_id;

        let header = SegmentHeader {
            segment_id,
            session_id,
            max_size: seg_size_max,
            remaining: 0,
            format,
        };
        header.write_to(&mut file)?;

        Ok(LogWrite {
            dir: dir_path,
            prefix: prefix.to_string(),
            suffix: suffix.to_string(),
            seg_size_max,
            format,
            callbacks,
            session_id,
            segment_id,
            current_path,
            file: Some(file),
            file_pos: SEGMENT_FILE_HEADER_LEN,
            record_bytes_left: 0,
            record_count: 0,
        })
    }

    /// The session identifier of this writer. Equals the segment
    /// identifier of the first segment file that was created for this
    /// session.
    pub fn session_id(&self) -> SegId {
        self.session_id
    }

    /// The segment identifier of the segment file the next byte will be
    /// written into.
    pub fn current_segment_id(&self) -> SegId {
        self.segment_id
    }

    /// Writes the UTF-8 bytes of `msg` as a single record.
    pub fn write_str(&mut self, msg: &str) -> Result<u32, LogError> {
        self.write(msg.as_bytes())
    }

    /// Writes `msg` as a single record.
    ///
    /// The record may span multiple segment files; each time the
    /// current file fills, `callbacks.send` is invoked with its path and
    /// a fresh segment file is opened.
    ///
    /// Returns the total number of bytes written, including the
    /// per-record data header.
    pub fn write(&mut self, msg: &[u8]) -> Result<u32, LogError> {
        if let Format::Fixed(n) = self.format {
            if msg.len() as u64 != n as u64 {
                return Err(LogError::FixedLenMismatch);
            }
        }
        if msg.len() > RecSize::MAX as usize {
            return Err(LogError::PayloadTooLarge);
        }

        let mut header_buf = [0u8; MAX_DATA_HEADER_LEN];
        let header_len = self.build_data_header(msg.len() as u32, &mut header_buf)?;
        let total = header_len + msg.len();
        self.record_bytes_left = total as u64;

        if header_len > 0 {
            self.write_bytes(&header_buf[..header_len])?;
        }
        self.write_bytes(msg)?;

        let file = self.file.as_mut().expect("file present after write");
        (self.callbacks.record_complete)(file).map_err(LogError::IoError)?;

        Ok(total as u32)
    }

    /// Flushes any buffered data in the current segment file.
    pub fn flush(&mut self) -> Result<(), LogError> {
        if let Some(f) = self.file.as_mut() {
            f.flush().map_err(LogError::IoError)?;
        }
        Ok(())
    }

    /// Removes every segment file for this log from `dir` except the
    /// current one. The current segment is preserved because the file
    /// handle is still open; on platforms that do not permit deleting
    /// an open file, removing it would fail and leave the writer in an
    /// inconsistent state.
    pub fn clear(&mut self) -> Result<(), LogError> {
        let current_name = segment_file_name(&self.prefix, self.segment_id, &self.suffix);
        for id in enumerate_segments(&self.dir, &self.prefix, &self.suffix)? {
            let name = segment_file_name(&self.prefix, id, &self.suffix);
            if name == current_name {
                continue;
            }
            let path = self.dir.join(&name);
            fs::remove_file(&path).map_err(LogError::IoError)?;
        }
        Ok(())
    }

    /// Fills the leading bytes of `out` with the per-record header for
    /// the active format and returns how many bytes were written.
    fn build_data_header(
        &mut self,
        payload_len: u32,
        out: &mut [u8; MAX_DATA_HEADER_LEN],
    ) -> Result<usize, LogError> {
        match self.format {
            Format::Fixed(_) => Ok(0),
            Format::VariableSimple => {
                out[..4].copy_from_slice(&payload_len.to_le_bytes());
                Ok(4)
            }
            Format::VariableTsRc => {
                let ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| LogError::ClockError)?
                    .as_nanos();
                let ts = ts.min(u128::from(u64::MAX)) as u64;
                self.record_count = self.record_count.saturating_add(1);
                out[0..4].copy_from_slice(&payload_len.to_le_bytes());
                out[4..12].copy_from_slice(&ts.to_le_bytes());
                out[12..20].copy_from_slice(&self.record_count.to_le_bytes());
                Ok(20)
            }
        }
    }

    fn write_bytes(&mut self, data: &[u8]) -> Result<(), LogError> {
        let mut written = 0usize;
        while written < data.len() {
            let available =
                self.seg_size_max.saturating_sub(self.file_pos) as usize;
            if available == 0 {
                self.roll_segment()?;
                continue;
            }
            let chunk = (data.len() - written).min(available);
            let file = self.file.as_mut().expect("file present in write_bytes");
            file.write_all(&data[written..written + chunk])
                .map_err(LogError::IoError)?;
            self.file_pos += chunk as u32;
            self.record_bytes_left = self.record_bytes_left.saturating_sub(chunk as u64);
            written += chunk;
        }
        Ok(())
    }

    fn roll_segment(&mut self) -> Result<(), LogError> {
        if let Some(mut f) = self.file.take() {
            f.flush().map_err(LogError::IoError)?;
            drop(f);
        }
        let sent_path = self.current_path.clone();
        (self.callbacks.send)(&sent_path).map_err(LogError::IoError)?;

        let (new_id, new_path, mut new_file) =
            create_segment_file(&self.dir, &self.prefix, &self.suffix)?;
        self.segment_id = new_id;
        self.current_path = new_path;

        let header = SegmentHeader {
            segment_id: new_id,
            session_id: self.session_id,
            max_size: self.seg_size_max,
            remaining: self.record_bytes_left,
            format: self.format,
        };
        header.write_to(&mut new_file)?;

        self.file = Some(new_file);
        self.file_pos = SEGMENT_FILE_HEADER_LEN;
        Ok(())
    }
}

impl Drop for LogWrite {
    fn drop(&mut self) {
        // When the writer is dropped, the currently open segment file
        // may still contain data that user code has never received via
        // `send`. Flush and hand it off. Errors are ignored because
        // Drop must not panic and there is no meaningful error path
        // from a destructor.
        if let Some(mut f) = self.file.take() {
            let _ = f.flush();
            let has_data = self.file_pos > SEGMENT_FILE_HEADER_LEN;
            drop(f);
            if has_data {
                let _ = (self.callbacks.send)(&self.current_path);
            }
        }
    }
}

/// Repeatedly reads the wall clock and attempts to create a segment
/// file whose name derives from the current time. Retries until an
/// unused name is found or a non-`AlreadyExists` error is returned.
fn create_segment_file(
    dir: &Path,
    prefix: &str,
    suffix: &str,
) -> Result<(SegId, PathBuf, File), LogError> {
    let sleep = Duration::from_nanos(TIMER_RESOLUTION_NS.saturating_mul(2));
    loop {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| LogError::ClockError)?
            .as_nanos();
        let ns = now.min(u128::from(u64::MAX)) as u64;
        let seg_id = SegId::from_u64(ns);
        let path = segment_path(dir, prefix, seg_id, suffix);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(f) => return Ok((seg_id, path, f)),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                thread::sleep(sleep);
                continue;
            }
            Err(e) => return Err(LogError::IoError(e)),
        }
    }
}
