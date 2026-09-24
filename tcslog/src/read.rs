//! Log reader implementation.
//
// Every byte count in this module is bounded by a segment file's
// `max_size`, which is a `u32`, and by the length of a caller-supplied
// buffer. The `usize`/`u32` conversions below therefore cannot lose
// information: `usize` is at least 32 bits wide on every target tcslog
// supports, and no value converted to `u32` can exceed a segment's
// `max_size`. Each site notes the bound it relies on.
#![allow(clippy::cast_possible_truncation)]

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::PathBuf;

use crate::error::LogError;
use crate::format::{Format, Meta, RecSize};
use crate::header::{SegmentHeader, SEGMENT_FILE_HEADER_LEN};
use crate::segid::SegId;
use crate::util::{check_no_path_delim, enumerate_segments, segment_path};

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
    /// Count of distinct segment files this reader has opened and
    /// accepted. See [`LogRead::segments_opened`].
    segments_opened: u64,
    /// Identifier of the most recently opened segment, used to keep
    /// `segments_opened` from counting a re-open twice.
    last_opened: Option<SegId>,
    /// Headers of segments opened since the last
    /// [`LogRead::take_opened_headers`], collected only while
    /// `collect_opened_headers` is set.
    opened_headers: Vec<SegmentHeader>,
    /// Whether to accumulate `opened_headers`. Off by default so a
    /// caller that never drains pays nothing.
    collect_opened_headers: bool,
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
            segments_opened: 0,
            last_opened: None,
            opened_headers: Vec::new(),
            collect_opened_headers: false,
        })
    }

    /// Enables or disables collection of the headers of segments as
    /// they are opened, for callers that want to report every segment a
    /// read traversed rather than only the one a record ended in.
    ///
    /// Off by default: while enabled, headers accumulate until
    /// [`LogRead::take_opened_headers`] drains them, so a caller that
    /// enables collection must drain, or the buffer grows with the log.
    /// Takes effect for segments opened after this call.
    pub fn collect_opened_headers(&mut self, enable: bool) {
        self.collect_opened_headers = enable;
        if !enable {
            self.opened_headers = Vec::new();
        }
    }

    /// Removes and returns the headers of the segments opened since the
    /// previous call, in the order they were opened.
    ///
    /// Always empty unless [`LogRead::collect_opened_headers`] has been
    /// enabled. A segment re-opened after a rejected crossing yields
    /// its header once, matching [`LogRead::segments_opened`].
    pub fn take_opened_headers(&mut self) -> Vec<SegmentHeader> {
        std::mem::take(&mut self.opened_headers)
    }

    /// The number of distinct segment files this reader has opened and
    /// accepted so far.
    ///
    /// This counts every segment traversed, including those that only
    /// held the middle or the start of a record spanning several files
    /// and so never appeared in [`LogRead::current_header`]. A segment
    /// re-opened after a rejected crossing is counted once. Segments
    /// skipped because they were missing, unreadable, or corrupt are
    /// not counted, nor is a segment belonging to a later session.
    #[must_use]
    pub fn segments_opened(&self) -> u64 {
        self.segments_opened
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
                if self.current.is_none() && !self.open_next_ready_segment()? {
                    return Err(LogError::Eof);
                }
                if let Some(offset) = self.locate_first_record_offset()? {
                    let cur = self.current.as_mut().unwrap();
                    cur.file_pos = offset;
                    self.resync = false;
                    break;
                }
                self.current = None;
            }
        } else if self.current.is_none() && !self.open_next_ready_segment()? {
            return Err(LogError::Eof);
        }

        let format = self.current.as_ref().unwrap().header.format;
        let (payload_len, meta) = self.read_data_header(format)?;

        // Clamp the caller's capacity into `RecSize` so the comparison
        // below happens entirely in the payload-length type. A buffer
        // longer than `RecSize::MAX` can always hold any record.
        let cap = RecSize::try_from(buf.len()).unwrap_or(RecSize::MAX);
        let take = payload_len.min(cap);
        self.read_exact_spanning(&mut buf[..take as usize])?;
        if payload_len > cap {
            let extra = u64::from(payload_len - cap);
            if self.skip_spanning(extra).is_err() {
                self.arm_resync();
            }
            return Err(LogError::ReadOverflow(take));
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
            if self.current.is_none() && !self.open_next_ready_segment()? {
                return Err(self.exhausted_mid_record());
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
            if self.current.is_none() && !self.open_next_ready_segment()? {
                return Err(self.exhausted_mid_record());
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
    /// armed, and [`LogError::ReadTruncated`] is returned.
    ///
    /// Three crossing shapes are validated:
    ///
    /// * **Sequence continuity**: the new segment must be the
    ///   immediate successor of the one just left, i.e. its
    ///   `sequence` must be the previous segment's plus one. A record
    ///   can only continue into the very next segment, so any jump
    ///   means at least one segment between them is missing.
    /// * **Boundary**: nothing has been read of the current record
    ///   (`record_bytes_consumed == 0`), so the previous segment ended
    ///   at a record boundary and the new segment starts a fresh
    ///   record. The new segment must declare `remaining == 0`. This
    ///   case does not need the record's total size and so is checked
    ///   even before the data header has been decoded - which is where
    ///   every boundary crossing lands, since the first thing a read
    ///   does is ask for the header.
    /// * **Mid-record**: some bytes of the current record were read
    ///   from the previous segment (`record_bytes_consumed > 0`). The
    ///   new segment must declare the remaining tail with
    ///   `remaining == total - consumed`. The writer never splits a
    ///   data header across segments, so by the time any byte of a
    ///   record has been consumed its total size is known and this
    ///   check always has a value to compare against.
    fn cross_to_next_in_record(&mut self) -> Result<(), LogError> {
        let prev_sequence = self.current.as_ref().map(|c| c.header.sequence);
        self.current = None;
        if !self.open_next_ready_segment()? {
            return Err(self.exhausted_mid_record());
        }
        if let Some(prev) = prev_sequence {
            let cur = self.current.as_ref().unwrap();
            if cur.header.sequence != prev.saturating_next() {
                // The segments between the two sequence numbers are
                // the ones that went missing. saturating_sub keeps a
                // sequence that failed to advance (which should not
                // happen, but would otherwise wrap) reported as zero.
                let lost = cur
                    .header
                    .sequence
                    .as_u64()
                    .saturating_sub(prev.saturating_next().as_u64());
                return Err(self.reject_crossing(lost));
            }
        }
        let expected = if self.record_bytes_consumed == 0 {
            Some(0)
        } else {
            self.record_total_bytes
                .map(|total| total - self.record_bytes_consumed)
        };
        if let Some(expected) = expected {
            let cur = self.current.as_ref().unwrap();
            if cur.header.remaining != expected {
                return Err(self.reject_crossing(0));
            }
        }
        Ok(())
    }

    /// Classifies running out of segment files while a read is in
    /// flight.
    ///
    /// If no byte of the current record has been consumed the reader was
    /// sitting on a record boundary, so the log simply ended:
    /// [`LogError::Eof`]. If bytes have already been consumed, the
    /// record's remaining tail lived in a segment file that is not
    /// there -- the session's last segment was lost or truncated
    /// mid-record -- and the spec requires that be reported as a
    /// truncated read rather than a clean end of log, so the caller can
    /// tell a complete log from one whose tail is missing. The lost
    /// count is zero because with no following segment there is no
    /// `sequence` field to measure the gap against.
    fn exhausted_mid_record(&self) -> LogError {
        if self.record_bytes_consumed == 0 {
            LogError::Eof
        } else {
            LogError::ReadTruncated(0)
        }
    }

    /// Rejects the segment just opened by [`cross_to_next_in_record`]:
    /// pushes it back onto the front of the pending list so it is
    /// reconsidered as a resync candidate, arms resync, and yields the
    /// error to report. `lost` is the number of segment files the
    /// sequence gap accounts for, or zero when the sequence is intact.
    fn reject_crossing(&mut self, lost: u64) -> LogError {
        let bad_id = self
            .current
            .as_ref()
            .expect("segment open")
            .header
            .segment_id;
        self.current = None;
        self.pending.push_front(bad_id);
        self.resync = true;
        LogError::ReadTruncated(lost)
    }

    /// Opens the next segment whose header we can read and whose
    /// session identifier is compatible with the current one. Returns
    /// `Ok(false)` when the pending queue is exhausted.
    fn open_next_ready_segment(&mut self) -> Result<bool, LogError> {
        while let Some(id) = self.pending.pop_front() {
            let path = segment_path(&self.dir, &self.prefix, id, &self.suffix);
            let Ok(mut file) = OpenOptions::new().read(true).open(&path) else {
                continue;
            };
            let Ok(meta) = file.metadata() else {
                continue;
            };
            let file_len = u32::try_from(meta.len().min(u64::from(u32::MAX))).unwrap_or(u32::MAX);
            if file_len < SEGMENT_FILE_HEADER_LEN {
                continue;
            }
            let Ok(header) = SegmentHeader::read_from(&mut file) else {
                continue;
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
            // `reject_crossing` pushes the segment it rejected back onto
            // the front of the queue, so the very next open re-opens the
            // same file. That is one file read twice, not two files.
            if self.last_opened != Some(id) {
                self.segments_opened += 1;
                self.last_opened = Some(id);
                if self.collect_opened_headers {
                    self.opened_headers.push(header.clone());
                }
            }
            self.current = Some(OpenSegment {
                header,
                file,
                file_pos: SEGMENT_FILE_HEADER_LEN,
                file_len,
            });
            return Ok(true);
        }
        Ok(false)
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

impl Iterator for LogReadIter<'_> {
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
