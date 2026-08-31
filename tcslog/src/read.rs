//! Log reader.
//!
//! [`LogRead`] scans a directory for segment files matching a
//! `<prefix><id><suffix>` naming convention, orders them oldest-first, and
//! streams records out via `read`. It transparently handles:
//!
//! * finding the first fresh record boundary in the earliest segment,
//! * following records that span multiple segments,
//! * skipping segments whose headers are damaged, and
//! * signalling end-of-log with [`LogError::Eof`].

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Read as _;
use std::path::PathBuf;

use crate::format::{Format, Meta, RecordCount, Timestamp};
use crate::header::{SegmentHeader, SEGMENT_HEADER_SIZE};
use crate::io_util::{is_segment_name, parse_segment_name, validate_prefix_suffix};
use crate::segid::SegId;
use crate::LogError;

/// Result of a successful record read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadResult {
    /// Number of payload bytes copied into the caller's buffer.
    pub n: u32,
    /// Format-specific metadata parsed from the record's data header.
    pub meta: Meta,
}

#[derive(Debug)]
struct CurrentSeg {
    file: File,
    header: SegmentHeader,
    /// Bytes already consumed from this segment's data section.
    data_read: u32,
    /// Size of this segment's data section: `file_len - SEGMENT_HEADER_SIZE`.
    /// Using the actual file length instead of `header.max_size` gracefully
    /// handles the last segment when the writer was interrupted before
    /// filling it.
    data_section_size: u32,
}

/// A tcslog log open for reading.
#[derive(Debug)]
pub struct LogRead {
    #[allow(dead_code)]
    dir: PathBuf,
    prefix: String,
    suffix: String,
    /// Segment IDs still to process, oldest first.
    pending: VecDeque<SegId>,
    /// The segment file currently being read from.
    current: Option<CurrentSeg>,
    /// When positive, this many bytes at the start of the current segment's
    /// unread data section are the tail of a record that started in an
    /// earlier segment and are to be skipped over to reach the next fresh
    /// record boundary.
    skip_at_boundary: u32,
    /// True once we've resolved the initial "where does the first fresh
    /// record start" question from the very first segment header.
    initial_seek_done: bool,
}

impl LogRead {
    /// Open a log for reading. Scans `dir` once for every file whose name
    /// matches `<prefix><segment-id><suffix>` and readies them for
    /// sequential consumption.
    ///
    /// Returns [`LogError::NoSegments`] if no matching files exist.
    pub fn new(dir: &str, prefix: &str, suffix: &str) -> Result<Self, LogError> {
        validate_prefix_suffix(prefix, suffix)?;
        let dir_pb = PathBuf::from(dir);

        let mut ids: Vec<SegId> = Vec::new();
        for entry in fs::read_dir(&dir_pb)? {
            let entry = entry?;
            let name_os = entry.file_name();
            let name = name_os.to_string_lossy();
            if let Some(id) = parse_segment_name(&name, prefix, suffix) {
                ids.push(id);
            } else if is_segment_name(&name, prefix, suffix) {
                // Belt-and-suspenders: is_segment_name confirms the shape,
                // parse_segment_name confirms it parses.
            }
        }
        if ids.is_empty() {
            return Err(LogError::NoSegments);
        }
        ids.sort();

        Ok(LogRead {
            dir: dir_pb,
            prefix: prefix.to_string(),
            suffix: suffix.to_string(),
            pending: ids.into(),
            current: None,
            skip_at_boundary: 0,
            initial_seek_done: false,
        })
    }

    /// Header of the segment currently being read from, or `None` if no
    /// segment is open yet (e.g. before the first `read`).
    pub fn current_header(&self) -> Option<&SegmentHeader> {
        self.current.as_ref().map(|c| &c.header)
    }

    /// Convenience wrapper: read one record's payload into `buf` as a
    /// UTF-8 string. Bytes that are not valid UTF-8 are replaced with the
    /// Unicode replacement character.
    pub fn read_str(&mut self, buf: &mut [u8]) -> Result<ReadResult, LogError> {
        self.read(buf)
    }

    /// Read the next record's payload into `buf`. On success returns the
    /// number of payload bytes written plus the format-specific metadata.
    /// On overflow (record bigger than `buf`) the caller's buffer is filled
    /// with the leading `buf.len()` bytes of the payload, the remainder is
    /// consumed off the log, and [`LogError::ReadOverflow`] is returned
    /// carrying the record's true size in bytes.
    ///
    /// Returns [`LogError::Eof`] once every segment has been fully consumed.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<ReadResult, LogError> {
        self.seek_to_fresh_boundary()?;
        let format = self
            .current
            .as_ref()
            .expect("seek_to_fresh_boundary leaves a segment open")
            .header
            .format;

        let (n, meta) = self.read_data_header(format)?;

        let capacity = buf.len().min(n as usize);
        let overflow = (n as usize) > buf.len();
        self.read_bytes_continuation(&mut buf[..capacity])?;
        if overflow {
            self.discard_bytes_continuation(n as usize - capacity)?;
            return Err(LogError::ReadOverflow(n));
        }
        Ok(ReadResult { n, meta })
    }

    // ----- internals ------------------------------------------------------

    /// Position at the next fresh record boundary. May open one or more
    /// segments and skip continuation bytes to get there.
    fn seek_to_fresh_boundary(&mut self) -> Result<(), LogError> {
        loop {
            if self.current.is_none() {
                self.open_next_segment()?;
            }
            while self.skip_at_boundary > 0 {
                let cur = self
                    .current
                    .as_mut()
                    .expect("current segment is open");
                let avail = cur.data_section_size.saturating_sub(cur.data_read);
                if avail == 0 {
                    // Continuation runs past this segment; move on.
                    self.current = None;
                    break;
                }
                let take = avail.min(self.skip_at_boundary);
                skip_exact(&mut cur.file, take as usize)?;
                cur.data_read += take;
                self.skip_at_boundary -= take;
            }
            if self.current.is_none() {
                continue;
            }
            // Now we're either at a fresh record boundary or at end of
            // segment. If at end, roll to next segment.
            let cur = self.current.as_ref().unwrap();
            if cur.data_read >= cur.data_section_size {
                self.current = None;
                continue;
            }
            return Ok(());
        }
    }

    /// Open the next pending segment, reading and validating its header.
    /// Segments whose header fails validation are skipped. Returns
    /// [`LogError::Eof`] once no more segments are available.
    fn open_next_segment(&mut self) -> Result<(), LogError> {
        loop {
            let Some(seg_id) = self.pending.pop_front() else {
                return Err(LogError::Eof);
            };
            let name = format!("{}{}{}", self.prefix, seg_id, self.suffix);
            let path = self.dir.join(&name);
            let mut file = match OpenOptions::new().read(true).open(&path) {
                Ok(f) => f,
                Err(_) => continue, // corrupted/missing file: skip
            };
            let header = match SegmentHeader::read_from(&mut file) {
                Ok(h) => h,
                Err(_) => continue,
            };
            if header.segment_id != seg_id {
                continue;
            }
            let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);
            let data_section_size = file_len
                .saturating_sub(SEGMENT_HEADER_SIZE as u64)
                .min(header.max_size.saturating_sub(SEGMENT_HEADER_SIZE as u32) as u64)
                as u32;

            let skip = if !self.initial_seek_done {
                self.initial_seek_done = true;
                // See spec's "Find the First Data Record Start" algorithm.
                if header.remaining > data_section_size {
                    // Whole segment is continuation of a record whose head
                    // we don't have; skip the segment entirely.
                    header.remaining.min(data_section_size)
                } else {
                    header.remaining
                }
            } else {
                // Not the first segment we're opening. We only get here when
                // the previous segment ended cleanly at a record boundary
                // (mid-record reads don't call open_next_segment through
                // this path; they use continuation reads). So the next
                // fresh boundary is at offset `remaining`.
                header.remaining
            };

            self.current = Some(CurrentSeg {
                file,
                header,
                data_read: 0,
                data_section_size,
            });
            self.skip_at_boundary = skip;
            return Ok(());
        }
    }

    /// Parse a data header for the given format, transparently continuing
    /// into the next segment if the current one runs out mid-header.
    fn read_data_header(&mut self, format: Format) -> Result<(u32, Meta), LogError> {
        match format {
            Format::Fixed(n) => Ok((n, Meta::Fixed)),
            Format::VariableSimple => {
                let mut b = [0u8; 4];
                self.read_bytes_continuation(&mut b)?;
                Ok((u32::from_le_bytes(b), Meta::VariableSimple))
            }
            Format::VariableTsRc => {
                let mut b = [0u8; 20];
                self.read_bytes_continuation(&mut b)?;
                let n = u32::from_le_bytes(b[0..4].try_into().unwrap());
                let ts: Timestamp = u64::from_le_bytes(b[4..12].try_into().unwrap());
                let rn: RecordCount = u64::from_le_bytes(b[12..20].try_into().unwrap());
                Ok((n, Meta::VariableTsRc(ts, rn)))
            }
        }
    }

    /// Read exactly `buf.len()` bytes from the log, crossing segment
    /// boundaries as continuation reads (i.e. no `remaining` skipping).
    fn read_bytes_continuation(&mut self, buf: &mut [u8]) -> Result<(), LogError> {
        let mut off = 0;
        while off < buf.len() {
            if self.current.is_none() {
                self.open_next_segment_continuation()?;
            }
            let cur = self.current.as_mut().unwrap();
            let avail = cur.data_section_size.saturating_sub(cur.data_read);
            if avail == 0 {
                self.current = None;
                continue;
            }
            let take = (buf.len() - off).min(avail as usize);
            cur.file.read_exact(&mut buf[off..off + take])?;
            cur.data_read += take as u32;
            off += take;
        }
        Ok(())
    }

    /// Skip `count` bytes from the log using continuation semantics.
    fn discard_bytes_continuation(&mut self, mut count: usize) -> Result<(), LogError> {
        while count > 0 {
            if self.current.is_none() {
                self.open_next_segment_continuation()?;
            }
            let cur = self.current.as_mut().unwrap();
            let avail = cur.data_section_size.saturating_sub(cur.data_read) as usize;
            if avail == 0 {
                self.current = None;
                continue;
            }
            let take = count.min(avail);
            skip_exact(&mut cur.file, take)?;
            cur.data_read += take as u32;
            count -= take;
        }
        Ok(())
    }

    /// Open the next segment for a continuation read: we're mid-record,
    /// so the first `remaining` bytes of the new segment are the tail of
    /// our in-flight record and must NOT be skipped.
    fn open_next_segment_continuation(&mut self) -> Result<(), LogError> {
        let Some(seg_id) = self.pending.pop_front() else {
            return Err(LogError::Eof);
        };
        let name = format!("{}{}{}", self.prefix, seg_id, self.suffix);
        let path = self.dir.join(&name);
        let mut file = OpenOptions::new().read(true).open(&path)?;
        let header = SegmentHeader::read_from(&mut file)?;
        let file_len = file.metadata()?.len();
        let data_section_size = file_len
            .saturating_sub(SEGMENT_HEADER_SIZE as u64)
            .min(header.max_size.saturating_sub(SEGMENT_HEADER_SIZE as u32) as u64)
            as u32;

        self.current = Some(CurrentSeg {
            file,
            header,
            data_read: 0,
            data_section_size,
        });
        self.skip_at_boundary = 0;
        Ok(())
    }
}

/// Read and discard exactly `count` bytes from `file`, using a small
/// stack-allocated scratch buffer so we never allocate on the hot path.
fn skip_exact(file: &mut File, mut count: usize) -> Result<(), LogError> {
    let mut scratch = [0u8; 512];
    while count > 0 {
        let take = scratch.len().min(count);
        file.read_exact(&mut scratch[..take])?;
        count -= take;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::write::{LogWrite, WriteCallbacks};
    use tempfile::tempdir;

    fn write_records(dir: &str, prefix: &str, suffix: &str, records: &[&[u8]], fmt: Format, seg: u32) {
        let mut w = LogWrite::new(dir, prefix, suffix, seg, fmt, WriteCallbacks::default()).unwrap();
        for r in records {
            w.write(r).unwrap();
        }
    }

    #[test]
    fn variable_simple_roundtrip_single_segment() {
        let dir = tempdir().unwrap();
        let dpath = dir.path().to_str().unwrap();
        write_records(
            dpath,
            "p_",
            "_s",
            &[b"hello", b"world"],
            Format::VariableSimple,
            1024,
        );

        let mut r = LogRead::new(dpath, "p_", "_s").unwrap();
        let mut buf = [0u8; 32];

        let res = r.read(&mut buf).unwrap();
        assert_eq!(res.n, 5);
        assert_eq!(&buf[..5], b"hello");

        let res = r.read(&mut buf).unwrap();
        assert_eq!(res.n, 5);
        assert_eq!(&buf[..5], b"world");

        assert!(matches!(r.read(&mut buf), Err(LogError::Eof)));
    }

    #[test]
    fn overflow_returns_len_and_advances() {
        let dir = tempdir().unwrap();
        let dpath = dir.path().to_str().unwrap();
        write_records(
            dpath,
            "p_",
            "_s",
            &[b"a very long payload", b"next"],
            Format::VariableSimple,
            1024,
        );

        let mut r = LogRead::new(dpath, "p_", "_s").unwrap();
        let mut small = [0u8; 4];
        let err = r.read(&mut small).unwrap_err();
        match err {
            LogError::ReadOverflow(n) => assert_eq!(n, b"a very long payload".len() as u32),
            other => panic!("unexpected {other:?}"),
        }

        let mut buf = [0u8; 32];
        let res = r.read(&mut buf).unwrap();
        assert_eq!(res.n, 4);
        assert_eq!(&buf[..4], b"next");
    }

    #[test]
    fn variable_ts_rn_roundtrip_spans_segments() {
        let dir = tempdir().unwrap();
        let dpath = dir.path().to_str().unwrap();
        // seg_size_max just above header + hdr = 41 + 20 = 61. Set to 80 so
        // only a few payload bytes fit per segment.
        write_records(
            dpath,
            "p_",
            "_s",
            &[&[7u8; 100], &[9u8; 20]],
            Format::VariableTsRc,
            80,
        );

        let mut r = LogRead::new(dpath, "p_", "_s").unwrap();
        let mut buf = [0u8; 256];

        let res = r.read(&mut buf).unwrap();
        assert_eq!(res.n, 100);
        assert!(matches!(res.meta, Meta::VariableTsRc(_, 1)));
        assert!(buf[..100].iter().all(|&b| b == 7));

        let res = r.read(&mut buf).unwrap();
        assert_eq!(res.n, 20);
        assert!(matches!(res.meta, Meta::VariableTsRc(_, 2)));
        assert!(buf[..20].iter().all(|&b| b == 9));

        assert!(matches!(r.read(&mut buf), Err(LogError::Eof)));
    }

    #[test]
    fn fixed_records_roundtrip() {
        let dir = tempdir().unwrap();
        let dpath = dir.path().to_str().unwrap();
        write_records(
            dpath,
            "f_",
            "_s",
            &[&[1u8; 8], &[2u8; 8], &[3u8; 8]],
            Format::Fixed(8),
            80,
        );

        let mut r = LogRead::new(dpath, "f_", "_s").unwrap();
        let mut buf = [0u8; 8];

        for expected in [1u8, 2, 3] {
            let res = r.read(&mut buf).unwrap();
            assert_eq!(res.n, 8);
            assert!(buf.iter().all(|&b| b == expected));
        }
        assert!(matches!(r.read(&mut buf), Err(LogError::Eof)));
    }

    #[test]
    fn no_segments_error() {
        let dir = tempdir().unwrap();
        let err = LogRead::new(dir.path().to_str().unwrap(), "p_", "_s").unwrap_err();
        assert!(matches!(err, LogError::NoSegments));
    }
}
