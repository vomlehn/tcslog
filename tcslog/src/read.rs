//! Log reader implementation.

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::PathBuf;

use crate::error::LogError;
use crate::format::{Format, Meta, RecSize};
use crate::header::{SegmentHeader, SEGMENT_FILE_HEADER_LEN};
use crate::segid::SegId;
use crate::util::{
    check_no_path_delim, enumerate_segments, segment_path,
};

/// One record's worth of data returned by [`LogRead::read`].
#[derive(Debug, Clone)]
pub struct ReadResult {
    /// Number of bytes stored in the caller's buffer. This is the same
    /// as the record's payload length when the payload fit; otherwise
    /// the reader returns [`LogError::ReadOverflow`] with the same
    /// count.
    pub n: RecSize,
    /// Format-specific per-record metadata.
    pub meta: Meta,
}

#[derive(Debug)]
struct OpenSegment {
    header: SegmentHeader,
    file: File,
    /// Byte position within the segment file. Always at least
    /// `SEGMENT_FILE_HEADER_LEN` once the header has been read.
    file_pos: u32,
    /// Total size of the segment file on disk. Usually equals
    /// `header.max_size` for filled segments, but may be smaller for
    /// the final segment of a session or for segments truncated by a
    /// fault.
    file_len: u32,
}

impl OpenSegment {
    fn data_end(&self) -> u32 {
        self.header.max_size.min(self.file_len)
    }

    fn bytes_left_in_segment(&self) -> u32 {
        self.data_end().saturating_sub(self.file_pos)
    }
}

/// Handle for reading records back out of a segmented log.
#[derive(Debug)]
pub struct LogRead {
    dir: PathBuf,
    prefix: String,
    suffix: String,
    pending: VecDeque<SegId>,
    current: Option<OpenSegment>,
    session_id: Option<SegId>,
    pending_session_end: bool,
    at_first_record: bool,
}

impl LogRead {
    /// Opens the log identified by `dir`, `prefix`, and `suffix` for
    /// reading. The directory must contain at least one segment file
    /// matching the pattern.
    pub fn new(dir: &str, prefix: &str, suffix: &str) -> Result<LogRead, LogError> {
        check_no_path_delim(prefix)?;
        check_no_path_delim(suffix)?;
        let dir_path = PathBuf::from(dir);
        if !dir_path.is_dir() {
            return Err(LogError::InvalidPathname);
        }
        let ids = enumerate_segments(&dir_path, prefix, suffix)?;
        if ids.is_empty() {
            return Err(LogError::NoSegmentFiles);
        }
        Ok(LogRead {
            dir: dir_path,
            prefix: prefix.to_string(),
            suffix: suffix.to_string(),
            pending: ids.into_iter().collect(),
            current: None,
            session_id: None,
            pending_session_end: false,
            at_first_record: true,
        })
    }

    /// The header of the segment file that supplied the most recent
    /// record, or `None` if no record has been read yet.
    pub fn current_header(&self) -> Option<&SegmentHeader> {
        self.current.as_ref().map(|c| &c.header)
    }

    /// Reads the next record's payload into `buf` (which may be UTF-8
    /// text) and returns its length and metadata.
    pub fn read_str(&mut self, buf: &mut [u8]) -> Result<ReadResult, LogError> {
        self.read(buf)
    }

    /// Reads the next record's payload into `buf`.
    ///
    /// If the payload is larger than `buf`, the first `buf.len()` bytes
    /// are copied and [`LogError::ReadOverflow`] is returned so the
    /// caller can detect truncation. The truncated tail is silently
    /// discarded so subsequent reads pick up at the next record.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<ReadResult, LogError> {
        if self.pending_session_end {
            self.pending_session_end = false;
            self.session_id = None;
            self.at_first_record = true;
        }

        loop {
            if self.current.is_none() {
                match self.open_next_ready_segment()? {
                    None => return Err(LogError::Eof),
                    Some(()) => {}
                }
            }

            if self.at_first_record {
                self.at_first_record = false;
                if let Some(offset) = self.locate_first_record_offset()? {
                    let cur = self.current.as_mut().unwrap();
                    cur.file_pos = offset;
                } else {
                    self.current = None;
                    continue;
                }
            }

            break;
        }

        let format = self.current.as_ref().unwrap().header.format;
        let (payload_len, meta) = self.read_data_header(format)?;

        let take = (payload_len as usize).min(buf.len());
        self.read_exact_spanning(&mut buf[..take])?;
        if (payload_len as usize) > buf.len() {
            let extra = (payload_len as u64) - (buf.len() as u64);
            self.skip_spanning(extra)?;
            return Err(LogError::ReadOverflow(take as u32));
        }
        Ok(ReadResult {
            n: payload_len,
            meta,
        })
    }

    fn read_data_header(&mut self, format: Format) -> Result<(RecSize, Meta), LogError> {
        match format {
            Format::Fixed(n) => Ok((n, Meta::Fixed)),
            Format::VariableSimple => {
                let n = self.read_u32_spanning()?;
                Ok((n, Meta::VariableSimple))
            }
            Format::VariableTsRc => {
                let n = self.read_u32_spanning()?;
                let ts = self.read_u64_spanning()?;
                let rc = self.read_u64_spanning()?;
                Ok((n, Meta::VariableTsRc(ts, rc)))
            }
        }
    }

    fn read_u32_spanning(&mut self) -> Result<u32, LogError> {
        let mut buf = [0u8; 4];
        self.read_exact_spanning(&mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    fn read_u64_spanning(&mut self) -> Result<u64, LogError> {
        let mut buf = [0u8; 8];
        self.read_exact_spanning(&mut buf)?;
        Ok(u64::from_le_bytes(buf))
    }

    /// Reads exactly `buf.len()` bytes, moving forward across segment
    /// boundaries as needed.
    fn read_exact_spanning(&mut self, buf: &mut [u8]) -> Result<(), LogError> {
        let mut filled = 0usize;
        while filled < buf.len() {
            if self.current.is_none() {
                if self.open_next_ready_segment()?.is_none() {
                    return Err(LogError::Eof);
                }
            }
            let cur = self.current.as_mut().unwrap();
            let avail = cur.bytes_left_in_segment() as usize;
            if avail == 0 {
                self.current = None;
                continue;
            }
            let want = (buf.len() - filled).min(avail);
            cur.file
                .read_exact(&mut buf[filled..filled + want])
                .map_err(LogError::IoError)?;
            cur.file_pos += want as u32;
            filled += want;
        }
        Ok(())
    }

    /// Skips exactly `count` bytes, moving forward across segment
    /// boundaries as needed.
    fn skip_spanning(&mut self, mut count: u64) -> Result<(), LogError> {
        let mut scratch = [0u8; 512];
        while count > 0 {
            if self.current.is_none() {
                if self.open_next_ready_segment()?.is_none() {
                    return Err(LogError::Eof);
                }
            }
            let cur = self.current.as_mut().unwrap();
            let avail = cur.bytes_left_in_segment() as u64;
            if avail == 0 {
                self.current = None;
                continue;
            }
            let want = count.min(avail).min(scratch.len() as u64) as usize;
            cur.file
                .read_exact(&mut scratch[..want])
                .map_err(LogError::IoError)?;
            cur.file_pos += want as u32;
            count -= want as u64;
        }
        Ok(())
    }

    /// Opens the next segment whose header we can read and whose
    /// session identifier is compatible with the current one. Returns
    /// `Ok(None)` when the pending queue is exhausted.
    fn open_next_ready_segment(&mut self) -> Result<Option<()>, LogError> {
        while let Some(id) = self.pending.pop_front() {
            let path = segment_path(&self.dir, &self.prefix, id, &self.suffix);
            let mut file = match OpenOptions::new().read(true).open(&path) {
                Ok(f) => f,
                Err(_) => continue,
            };
            let file_len = match file.metadata() {
                Ok(m) => m.len().min(u32::MAX as u64) as u32,
                Err(_) => continue,
            };
            if file_len < SEGMENT_FILE_HEADER_LEN {
                continue;
            }
            let header = match SegmentHeader::read_from(&mut file) {
                Ok(h) => h,
                Err(_) => continue,
            };
            if header.segment_id != id {
                continue;
            }
            match self.session_id {
                None => {
                    self.session_id = Some(header.session_id);
                }
                Some(active) if active == header.session_id => {}
                Some(_) => {
                    self.pending.push_front(id);
                    self.pending_session_end = true;
                    return Err(LogError::SessionEnd);
                }
            }
            self.current = Some(OpenSegment {
                header,
                file,
                file_pos: SEGMENT_FILE_HEADER_LEN,
                file_len,
            });
            return Ok(Some(()));
        }
        Ok(None)
    }

    /// After opening the first segment of a session, discards the
    /// leading `remaining` bytes and returns the file offset at which
    /// the first fresh data-record header begins. Returns `Ok(None)` if
    /// this segment does not begin a new record and we need to skip to
    /// the next segment.
    fn locate_first_record_offset(&mut self) -> Result<Option<u32>, LogError> {
        let cur = self.current.as_mut().unwrap();
        let data_len = cur.header.data_capacity() as u64;
        let remaining = cur.header.remaining;
        if remaining >= data_len {
            let available = cur.bytes_left_in_segment() as u64;
            let mut left = available;
            let mut scratch = [0u8; 512];
            while left > 0 {
                let want = left.min(scratch.len() as u64) as usize;
                cur.file
                    .read_exact(&mut scratch[..want])
                    .map_err(LogError::IoError)?;
                cur.file_pos += want as u32;
                left -= want as u64;
            }
            return Ok(None);
        }
        let mut left = remaining.min(cur.bytes_left_in_segment() as u64);
        let mut scratch = [0u8; 512];
        while left > 0 {
            let want = left.min(scratch.len() as u64) as usize;
            cur.file
                .read_exact(&mut scratch[..want])
                .map_err(LogError::IoError)?;
            cur.file_pos += want as u32;
            left -= want as u64;
        }
        Ok(Some(cur.file_pos))
    }
}
