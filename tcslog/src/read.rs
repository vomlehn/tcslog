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

/// One record's payload plus its metadata, produced by [`LogRead::iter`].
///
/// This convenience type owns its payload and is therefore not
/// allocation-free; use [`LogRead::read`] directly when the no-allocation
/// contract must be preserved.
#[derive(Debug, Clone)]
pub struct Record {
    /// Format-specific per-record metadata.
    pub meta: Meta,
    /// The record's payload bytes.
    pub payload: Vec<u8>,
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
    /// When true, the next read must locate a fresh record boundary via
    /// "Find the Next Data Record Start" before decoding anything.
    /// Initialized to true so the very first read finds a record start
    /// rather than assuming the first pending segment's data section
    /// already begins on a record boundary.
    resync: bool,
    /// Bytes consumed of the current data record (header + payload) so
    /// far. Reset to zero at the start of every read.
    record_bytes_consumed: u64,
    /// Total bytes in the current data record (header + payload) once
    /// the data header has been decoded. `None` until then. Reset at
    /// the start of every read.
    record_total_bytes: Option<u64>,
}

impl LogRead {
    /// Opens the log identified by `dir`, `prefix`, and `suffix` for
    /// reading. The directory must contain at least one segment file
    /// matching the pattern.
    ///
    /// # Errors
    ///
    /// * [`LogError::PathDelimiterNotAllowed`] if `prefix` or `suffix`
    ///   contains a `/` or `\`.
    /// * [`LogError::InvalidPathname`] if `dir` does not name a
    ///   directory.
    /// * [`LogError::NoSegmentFiles`] if the directory contains no
    ///   file whose name matches the segment-file pattern.
    /// * [`LogError::IoError`] on directory enumeration failure.
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
            resync: true,
            record_bytes_consumed: 0,
            record_total_bytes: None,
        })
    }

    /// The header of the segment file that supplied the most recent
    /// record, or `None` if no record has been read yet.
    #[must_use]
    pub fn current_header(&self) -> Option<&SegmentHeader> {
        self.current.as_ref().map(|c| &c.header)
    }

    /// Reads the next record's payload into `buf` (which may be UTF-8
    /// text) and returns its length and metadata. Equivalent to
    /// [`LogRead::read`].
    ///
    /// # Errors
    ///
    /// See [`LogRead::read`].
    pub fn read_str(&mut self, buf: &mut [u8]) -> Result<ReadResult, LogError> {
        self.read(buf)
    }

    /// Reads the next record's payload into `buf`.
    ///
    /// If the payload is larger than `buf`, the first `buf.len()` bytes
    /// are copied and [`LogError::ReadOverflow`] is returned so the
    /// caller can detect truncation. The truncated tail is silently
    /// discarded so subsequent reads pick up at the next record.
    ///
    /// On any read failure other than [`LogError::Eof`],
    /// [`LogError::SessionEnd`], or [`LogError::ReadOverflow`], the
    /// reader arms the resync flag and discards any partially opened
    /// segment so the next call recovers via "Find the Next Data
    /// Record Start."
    ///
    /// # Errors
    ///
    /// * [`LogError::Eof`] when the segment list is exhausted.
    /// * [`LogError::SessionEnd`] once, at each session boundary.
    /// * [`LogError::ReadOverflow`] when the record's payload is
    ///   larger than `buf`.
    /// * [`LogError::ReadTruncated`] when a mid-record segment gap or
    ///   corruption is detected. The reader recovers on the next call.
    /// * [`LogError::IoError`] on underlying I/O failure.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<ReadResult, LogError> {
        let result = self.read_inner(buf);
        if let Err(ref e) = result {
            match e {
                LogError::Eof | LogError::SessionEnd | LogError::ReadOverflow(_) => {}
                _ => self.arm_resync(),
            }
        }
        result
    }

    fn read_inner(&mut self, buf: &mut [u8]) -> Result<ReadResult, LogError> {
        if self.pending_session_end {
            self.pending_session_end = false;
            self.session_id = None;
            self.resync = true;
        }

        self.record_bytes_consumed = 0;
        self.record_total_bytes = None;

        if self.resync {
            loop {
                if self.current.is_none() {
                    match self.open_next_ready_segment()? {
                        None => return Err(LogError::Eof),
                        Some(()) => {}
                    }
                }
                if let Some(offset) = self.locate_first_record_offset()? {
                    let cur = self.current.as_mut().unwrap();
                    cur.file_pos = offset;
                    self.resync = false;
                    break;
                }
                self.current = None;
            }
        } else if self.current.is_none() {
            match self.open_next_ready_segment()? {
                None => return Err(LogError::Eof),
                Some(()) => {}
            }
        }

        let format = self.current.as_ref().unwrap().header.format;
        let (payload_len, meta) = self.read_data_header(format)?;

        let take = (payload_len as usize).min(buf.len());
        self.read_exact_spanning(&mut buf[..take])?;
        if (payload_len as usize) > buf.len() {
            let extra = u64::from(payload_len) - buf.len() as u64;
            if self.skip_spanning(extra).is_err() {
                self.arm_resync();
            }
            return Err(LogError::ReadOverflow(take as u32));
        }
        Ok(ReadResult {
            n: payload_len,
            meta,
        })
    }

    /// Puts the reader into resync mode, discarding any current segment
    /// and pushing its identifier back onto the front of the pending
    /// list so it will be reopened cleanly on the next attempt.
    fn arm_resync(&mut self) {
        self.resync = true;
        if let Some(cur) = self.current.take() {
            self.pending.push_front(cur.header.segment_id);
        }
    }

    /// Returns an [`Iterator`] over the remaining records in the log.
    ///
    /// Each iterator step allocates a fresh [`Vec<u8>`] to own the
    /// record's payload; if the allocation-free contract must be
    /// preserved, call [`LogRead::read`] directly with a caller-supplied
    /// buffer instead.
    ///
    /// The iterator stops at end of log and, at session boundaries,
    /// yields the [`LogError::SessionEnd`] marker as a single item
    /// before continuing with the next session.
    pub fn iter(&mut self) -> LogReadIter<'_> {
        LogReadIter { reader: self }
    }

    fn read_data_header(&mut self, format: Format) -> Result<(RecSize, Meta), LogError> {
        let (payload_len, meta, header_size) = match format {
            Format::Fixed(n) => (n, Meta::Fixed, 0u64),
            Format::VariableSimple => {
                let n = self.read_u32_spanning()?;
                (n, Meta::VariableSimple, 4u64)
            }
            Format::VariableTsRc => {
                let n = self.read_u32_spanning()?;
                let ts = self.read_u64_spanning()?;
                let rc = self.read_u64_spanning()?;
                (n, Meta::VariableTsRc(ts, rc), 20u64)
            }
        };
        self.record_total_bytes = Some(header_size + u64::from(payload_len));
        Ok((payload_len, meta))
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
    /// boundaries as needed. Every mid-record crossing is validated
    /// against the new segment's `remaining` field; a mismatch arms
    /// resync and returns [`LogError::ReadTruncated`].
    fn read_exact_spanning(&mut self, buf: &mut [u8]) -> Result<(), LogError> {
        let mut filled = 0usize;
        while filled < buf.len() {
            if self.current.is_none() && self.open_next_ready_segment()?.is_none() {
                return Err(LogError::Eof);
            }
            let cur = self.current.as_mut().unwrap();
            let avail = cur.bytes_left_in_segment() as usize;
            if avail == 0 {
                self.cross_to_next_in_record()?;
                continue;
            }
            let want = (buf.len() - filled).min(avail);
            cur.file
                .read_exact(&mut buf[filled..filled + want])
                .map_err(LogError::IoError)?;
            cur.file_pos += want as u32;
            filled += want;
            self.record_bytes_consumed += want as u64;
        }
        Ok(())
    }

    /// Skips exactly `count` bytes, moving forward across segment
    /// boundaries as needed. Crossings are validated identically to
    /// [`read_exact_spanning`].
    fn skip_spanning(&mut self, mut count: u64) -> Result<(), LogError> {
        let mut scratch = [0u8; 512];
        while count > 0 {
            if self.current.is_none() && self.open_next_ready_segment()?.is_none() {
                return Err(LogError::Eof);
            }
            let cur = self.current.as_mut().unwrap();
            let avail = u64::from(cur.bytes_left_in_segment());
            if avail == 0 {
                self.cross_to_next_in_record()?;
                continue;
            }
            let want = count.min(avail).min(scratch.len() as u64) as usize;
            cur.file
                .read_exact(&mut scratch[..want])
                .map_err(LogError::IoError)?;
            cur.file_pos += want as u32;
            count -= want as u64;
            self.record_bytes_consumed += want as u64;
        }
        Ok(())
    }

    /// Opens the next segment as a continuation of the current data
    /// record and validates its `remaining` field against the bytes
    /// still owed. On any inconsistency the offending segment is
    /// pushed back onto the front of the pending list, resync is
    /// armed, and [`LogError::ReadTruncated`] is returned. If the
    /// total record size is not yet known (mid-header crossing),
    /// only the crossing itself is performed; the check is deferred
    /// to the next crossing after the header has been decoded.
    ///
    /// Three crossing shapes are validated:
    ///
    /// * **Sequence continuity**: the new segment must be the
    ///   immediate successor of the one just left, i.e. its
    ///   `sequence` must be the previous segment's plus one. A record
    ///   can only continue into the very next segment, so any jump
    ///   means at least one segment between them is missing. This is
    ///   checked first because it holds whether or not the record's
    ///   header has been decoded yet, and so it also covers crossings
    ///   that occur part way through a data header - the case the
    ///   `remaining` checks below cannot see.
    /// * **Mid-record**: some bytes of the current record were read
    ///   from the previous segment (`record_bytes_consumed > 0`). The
    ///   new segment must declare the remaining tail with
    ///   `remaining == total - consumed`.
    /// * **Boundary**: nothing has been read of the current record
    ///   (`record_bytes_consumed == 0`), so the previous segment ended
    ///   exactly at a record boundary and the new segment starts a
    ///   fresh record. The new segment must declare `remaining == 0`.
    fn cross_to_next_in_record(&mut self) -> Result<(), LogError> {
        let prev_sequence = self.current.as_ref().map(|c| c.header.sequence);
        self.current = None;
        match self.open_next_ready_segment()? {
            None => return Err(LogError::Eof),
            Some(()) => {}
        }
        if let Some(prev) = prev_sequence {
            let cur = self.current.as_ref().unwrap();
            if cur.header.sequence != prev.saturating_next() {
                return Err(self.reject_crossing());
            }
        }
        if let Some(total) = self.record_total_bytes {
            let expected = if self.record_bytes_consumed == 0 {
                0
            } else {
                total - self.record_bytes_consumed
            };
            let cur = self.current.as_ref().unwrap();
            if cur.header.remaining != expected {
                return Err(self.reject_crossing());
            }
        }
        Ok(())
    }

    /// Rejects the segment just opened by [`cross_to_next_in_record`]:
    /// pushes it back onto the front of the pending list so it is
    /// reconsidered as a resync candidate, arms resync, and yields the
    /// error to report.
    fn reject_crossing(&mut self) -> LogError {
        let bad_id = self.current.as_ref().expect("segment open").header.segment_id;
        self.current = None;
        self.pending.push_front(bad_id);
        self.resync = true;
        LogError::ReadTruncated
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
                Ok(m) => u32::try_from(m.len().min(u64::from(u32::MAX))).unwrap_or(u32::MAX),
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
            // Spec: "The segment file size is less than or equal to the
            // value of max size read from the segment file header." A
            // file that exceeds its own declared cap has been tampered
            // with or is otherwise corrupt; skip it silently.
            if file_len > header.max_size {
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
        let data_len = u64::from(cur.header.data_capacity());
        let remaining = cur.header.remaining;
        if remaining >= data_len {
            let available = u64::from(cur.bytes_left_in_segment());
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
        let mut left = remaining.min(u64::from(cur.bytes_left_in_segment()));
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

/// Per-record buffer size used by [`LogReadIter`]. Records larger than
/// this are yielded as [`LogError::ReadOverflow`].
const ITER_BUFFER_LEN: usize = 65_536;

/// Iterator over the remaining records of a [`LogRead`].
///
/// Yields `Ok(record)` for each successive record; `Err(SessionEnd)`
/// once each time the reader crosses a session boundary; and `None` at
/// end of log. Any other error terminates iteration after the error is
/// yielded. Records larger than [`ITER_BUFFER_LEN`] are truncated and
/// reported as [`LogError::ReadOverflow`] - use [`LogRead::read`]
/// directly with a larger caller-supplied buffer when that matters.
pub struct LogReadIter<'a> {
    reader: &'a mut LogRead,
}

impl<'a> Iterator for LogReadIter<'a> {
    type Item = Result<Record, LogError>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut buf = vec![0u8; ITER_BUFFER_LEN];
        match self.reader.read(&mut buf) {
            Ok(res) => {
                buf.truncate(res.n as usize);
                Some(Ok(Record {
                    meta: res.meta,
                    payload: buf,
                }))
            }
            Err(LogError::Eof) => None,
            Err(e) => Some(Err(e)),
        }
    }
}
