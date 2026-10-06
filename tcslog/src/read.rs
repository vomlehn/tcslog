//! Reading a log: [`LogRead`], and the resynchronization it performs
//! when segment files are missing or damaged.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::error::LogError;
use crate::format::{Format, Meta};
use crate::header::{SegmentHeader, SEGMENT_FILE_HEADER_LEN};
use crate::segid::SegId;
use crate::seq_id::SeqId;
use crate::util::{build_name, check_log_location, scan_segment_ids};
use crate::{RecSize, RecordCount, Timestamp};

/// Size of the buffer [`LogRead::iter`] reads into.
const ITER_BUF_LEN: usize = 64 * 1024;

/// What one successful read produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadResult {
    /// Number of payload bytes placed in the caller's buffer, which may
    /// be fewer than the buffer holds.
    pub n: RecSize,

    /// The metadata the record carried, which depends on the log's
    /// format.
    pub meta: Meta,
}

/// One record's payload together with its metadata, as produced by
/// [`LogRead::iter`].
///
/// This is the one public structure in the crate that owns heap memory,
/// which is why the iterator that yields it is offered alongside
/// [`LogRead::read`] rather than in place of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// The metadata the record carried.
    pub meta: Meta,

    /// The record's payload bytes.
    pub payload: Vec<u8>,

    /// Whether this is the front of a record rather than the whole of
    /// one.
    ///
    /// `false` for a record handed over entire, which is the ordinary
    /// case. `true` when the bytes in `payload` are real telemetry from
    /// a record whose remainder did not reach the iterator -- either
    /// because the segment file carrying it was lost or damaged, or
    /// because the record was longer than the iterator's own buffer.
    /// Either way the rest of it is gone, and no later record makes it
    /// up.
    ///
    /// The field exists because the bytes themselves cannot be told
    /// apart from a whole record's: this crate reports a loss rather
    /// than hiding it, and an unmarked partial record handed back
    /// through an iterator would be exactly the quiet substitution of
    /// a smaller log for the real one that [`LogRead::read`] refuses to
    /// make. A caller that must distinguish *why* the record is short,
    /// or that wants the loss counted, uses
    /// [`read`](LogRead::read) and reads the error.
    pub truncated: bool,
}

/// The segment file currently open.
struct Segment {
    /// Identifier of this segment file.
    id: SegId,
    /// The open file, positioned `pos` bytes into the data section.
    file: File,
    /// This file's decoded header.
    header: SegmentHeader,
    /// Data section bytes the file actually holds, which is less than
    /// the header's maximum implies when the file was cut short.
    data_len: u64,
    /// Read position within the data section.
    pos: u64,
}

/// State of the logical data record being read, carried across the
/// segment files it may span.
#[derive(Default)]
struct Cursor {
    /// Bytes of this record consumed so far, data header included.
    consumed: u64,
    /// Total size of this record, known once its data header has been
    /// decoded. `Format::Fixed` knows it from the start.
    total: Option<u64>,
    /// Payload length implied by a crossing that happened part way
    /// through this record's data header.
    implied_payload: Option<u64>,
    /// Payload bytes placed in the caller's buffer so far, which is what
    /// a truncated read reports.
    payload_in_buf: RecSize,
    /// Whether the bytes now being read are payload rather than header.
    in_payload: bool,
}

/// Where a resynchronization left the read position.
enum Start {
    /// At the first byte of a fresh data record.
    Fresh,
    /// At the first payload byte of a record whose data header was lost
    /// with a missing segment file, and whose payload is this many bytes
    /// long.
    Recovered(RecSize),
}

/// What is known about a record interrupted by a gap of exactly one
/// segment file, which is what a recovery attempt needs.
struct Recovery {
    /// The segment file the rejected crossing opened, and the only one
    /// this may be applied to.
    segment: SegId,
    /// Bytes the interrupted record still owed when the gap was found.
    owed: u64,
}

/// Reads a log, oldest record first.
///
/// Construction scans the directory and sorts the segment files into the
/// order they were written, but opens none of them: the first read finds
/// the start of the first record by the same path a read recovering from
/// a fault takes. There is one way into the data rather than two, and a
/// log whose opening segment files are already gone needs no special
/// case.
///
/// # Faults
///
/// A reader must tolerate an I/O error on the file it is reading and a
/// gap in the segment files it was given, and in either case it
/// discards the record in progress, resynchronizes on the next segment
/// file that opens cleanly, and returns the records after it. It may
/// report an error for the record it lost, but it stays usable: the very
/// next call recovers rather than repeating the failure. The only thing
/// that ends reading is running out of segment files.
pub struct LogRead {
    /// First part of every segment file name.
    prefix: String,
    /// Last part of every segment file name.
    suffix: String,
    /// Segment files not yet opened, oldest first.
    pending: VecDeque<SegId>,
    /// The segment file now open, if any.
    current: Option<Segment>,
    /// Set when the next read must find a record boundary before reading
    /// anything. True on construction, so the first read takes the same
    /// path a recovery does.
    resync: bool,
    /// Session the records read so far belong to.
    session: Option<SegId>,
    /// Sequence of the segment file most recently opened, against which
    /// the next crossing is checked.
    prev_seq: Option<SeqId>,
    /// Segment files lost ahead of the first surviving segment of the
    /// session now being read, waiting to be reported.
    session_start_lost: u64,
    /// Number of segment files opened, counting each one once.
    segments_opened: u64,
    /// Whether to retain the header of each segment file opened.
    collect_headers: bool,
    /// Headers retained since the last time they were taken.
    opened_headers: Vec<SegmentHeader>,
    /// A segment file handed back after a failed crossing, which has
    /// already been counted and must not be counted again.
    reopened: Option<SegId>,
    /// What a resynchronization needs in order to try recovering a
    /// record whose data header was lost.
    recovery: Option<Recovery>,
    /// Metadata of the record most recently decoded, which the iterator
    /// needs on the overflow path where `read` returns an error rather
    /// than a [`ReadResult`].
    last_meta: Meta,
    /// Reusable file name buffer, so that opening one segment file after
    /// another does not allocate.
    name_buf: String,
    /// Reusable path buffer, for the same reason. Its last component is
    /// replaced with each segment file name in turn.
    path_buf: PathBuf,
}

impl LogRead {
    /// Opens an existing log for reading.
    ///
    /// * `dir` -- directory holding the segment files. It must already
    ///   exist and hold at least one segment file of this log.
    /// * `prefix` -- first part of the segment file names. Must hold no
    ///   path separator.
    /// * `suffix` -- last part of the segment file names. Must hold no
    ///   path separator.
    ///
    /// Returns a reader positioned before the first record, with no
    /// segment file open.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::PathDelimiterNotAllowed`] for a prefix or
    /// suffix holding a path separator, [`LogError::InvalidPathname`]
    /// when `dir` does not name a directory,
    /// [`LogError::NoSegmentFiles`] when no file in it matches the
    /// pattern, and [`LogError::IoError`] if the directory cannot be
    /// enumerated.
    pub fn new(dir: &str, prefix: &str, suffix: &str) -> Result<Self, LogError> {
        let dir = check_log_location(dir, prefix, suffix)?;
        let ids = scan_segment_ids(&dir, prefix, suffix)?;
        if ids.is_empty() {
            return Err(LogError::NoSegmentFiles);
        }
        let name_buf = String::with_capacity(prefix.len() + SegId::STR_LEN + suffix.len());
        // The last component has to exist for `set_file_name` to
        // replace: given a path ending in a separator it would replace
        // the directory instead.
        // The directory is not kept beyond this: every path the reader
        // builds replaces this buffer's last component, so the buffer
        // carries the directory from here on.
        let path_buf = dir.join("placeholder");
        Ok(Self {
            prefix: prefix.to_string(),
            suffix: suffix.to_string(),
            pending: VecDeque::from(ids),
            current: None,
            resync: true,
            session: None,
            prev_seq: None,
            session_start_lost: 0,
            segments_opened: 0,
            collect_headers: false,
            opened_headers: Vec::new(),
            reopened: None,
            recovery: None,
            last_meta: Meta::VariableSimple,
            name_buf,
            path_buf,
        })
    }

    /// Reads the next record's payload into `buf`.
    ///
    /// If the payload is longer than `buf`, the first `buf.len()` bytes
    /// are copied and [`LogError::ReadOverflow`] is returned carrying
    /// that same count, so that a caller can tell a filled buffer from a
    /// complete record. The rest of the payload is discarded, so the
    /// following read begins at the next record rather than in the
    /// middle of this one.
    ///
    /// * `buf` -- buffer to receive the payload.
    ///
    /// Returns how many bytes were placed in `buf` and the metadata the
    /// record carried.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::Eof`] when the segment file list is
    /// exhausted, [`LogError::SessionEnd`] once at each session
    /// boundary, [`LogError::ReadTruncated`] when a mid-record gap or
    /// corruption is found, [`LogError::ReadOverflow`] when the record
    /// did not fit in `buf`, and [`LogError::IoError`] on an underlying
    /// failure.
    ///
    /// On any failure other than `Eof`, `SessionEnd`, and `ReadOverflow`
    /// the reader arms its resynchronization flag and gives up any
    /// partly opened segment file, so that the next call recovers.
    /// `ReadOverflow` is not a failure of that kind: the segment file
    /// stays open and the read position stays valid.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<ReadResult, LogError> {
        // Every read begins at a record boundary, so an exhausted data
        // section with no segment file left to open is the end of the
        // log rather than a record running off the end of one. The
        // segment file is given up along with the news, so that
        // `current_header` reports none once the log has ended.
        if self.pending.is_empty() && self.exhausted() {
            self.current = None;
            return Err(LogError::Eof);
        }
        if self.current.is_none() {
            self.resync = true;
        }

        let mut cursor = Cursor::default();
        let mut start = Start::Fresh;
        if self.resync {
            // On failure the flag stays set, so a caller that retries
            // resumes the search rather than reading from a position
            // nothing vouches for.
            start = match self.find_record_start() {
                Ok(start) => start,
                Err(e) => return Err(self.lose_position(e)),
            };
            self.resync = false;

            // Nothing crosses into a session's first surviving segment
            // file, so a loss ahead of it is reported here, before any
            // record of the session is handed back. The segment file
            // itself is sound and the position within it is the one
            // just found, so neither is given up: the next read returns
            // the record that starts there.
            if self.session_start_lost > 0 {
                let lost = std::mem::take(&mut self.session_start_lost);
                return Err(LogError::ReadTruncated { lost, n: 0 });
            }
        }

        // Both paths above leave a segment file open: the branch that
        // resynchronized succeeded, and the branch that did not was
        // entered only because one was already open.
        let Some(format) = self.current.as_ref().map(|segment| segment.header.format) else {
            return Err(self.lose_position(LogError::Eof));
        };

        let (payload_len, meta) = match start {
            Start::Recovered(n) => {
                // The data header was in the segment file that went
                // missing, so its bytes count as consumed: a further
                // crossing inside this record is then checked against
                // the record as a whole.
                cursor.consumed = u64::from(format.data_header_len());
                cursor.total = Some(cursor.consumed + u64::from(n));
                (n, Meta::VariableSimple)
            }
            Start::Fresh => match self.read_data_header(&mut cursor, format) {
                Ok(decoded) => decoded,
                Err(e) => return Err(self.lose_position(e)),
            },
        };
        self.last_meta = meta;

        cursor.in_payload = true;
        let want = payload_len as usize;
        let take = want.min(buf.len());
        if let Err(e) = self.fill(&mut cursor, &mut buf[..take]) {
            return Err(self.lose_position(e));
        }
        let n = RecSize::try_from(take).unwrap_or(RecSize::MAX);

        if want > buf.len() {
            // Skip what did not fit so that the next read starts at the
            // next record. The caller already has valid bytes for this
            // one, so a failure here does not disturb the result: the
            // resynchronization flag guarantees the next call recovers.
            let rest = (want - buf.len()) as u64;
            if self.skip(&mut cursor, rest).is_err() {
                self.resync = true;
                self.current = None;
            }
            return Err(LogError::ReadOverflow(n));
        }

        Ok(ReadResult { n, meta })
    }

    /// Gives up the read position after a fault, so that the next read
    /// resynchronizes rather than reading on from somewhere nothing
    /// vouches for.
    ///
    /// Returns the error to report, unchanged, so that a caller can
    /// write `return Err(self.lose_position(e))` and have the state
    /// effect of the error visible where the error is raised.
    fn lose_position(&mut self, e: LogError) -> LogError {
        self.resync = true;
        self.current = None;
        e
    }

    /// Reads the next record's payload into `buf`, for callers whose
    /// payloads are UTF-8 text.
    ///
    /// Equivalent to [`read`](Self::read), and takes the same byte
    /// buffer rather than a string, since a payload recovered in part
    /// need not be valid UTF-8.
    ///
    /// * `buf` -- buffer to receive the payload.
    ///
    /// Returns what [`read`](Self::read) returns.
    ///
    /// # Errors
    ///
    /// The same errors as [`read`](Self::read).
    pub fn read_str(&mut self, buf: &mut [u8]) -> Result<ReadResult, LogError> {
        self.read(buf)
    }

    /// An iterator over the remaining records.
    ///
    /// A [`Record`] owns its payload, so this is a convenience that
    /// gives up the crate's no-allocation guarantee;
    /// [`read`](Self::read) is what a caller bound by that guarantee
    /// uses. A payload longer than the iterator's internal buffer of
    /// 64 KiB is yielded cut to it, for the same reason a truncated
    /// read's bytes are still returned: they are real telemetry. A
    /// caller with records that long should size its own buffer and use
    /// [`read`](Self::read), which reports the overflow.
    ///
    /// Session boundaries are skipped, as are records of which no byte
    /// survived. A record of which some bytes did survive -- cut short
    /// by a loss, or longer than the buffer above -- is yielded with
    /// [`Record::truncated`] set, so that the front of a record cannot
    /// be mistaken here for the whole of one. What this cannot report
    /// is how much was lost: the file count a
    /// [`LogError::ReadTruncated`] carries has no place on a record, so
    /// a caller that must account for the loss rather than merely
    /// notice it uses [`read`](Self::read).
    ///
    /// Iteration ends at the end of the log, and also at an
    /// [`io`](LogError::IoError) failure, which the iterator has no way
    /// to report. The two are indistinguishable through this interface.
    /// A caller that must tell a finished log from failed storage uses
    /// [`read`](Self::read), where the storage error arrives as an
    /// error and the reader stays fit to continue past it.
    ///
    /// Returns the iterator, which borrows the reader for its lifetime.
    pub fn iter(&mut self) -> LogReadIter<'_> {
        LogReadIter {
            log: self,
            buf: vec![0u8; ITER_BUF_LEN],
        }
    }

    /// The header of the segment file now open.
    ///
    /// Returns that header, or `None` whenever no segment file is open:
    /// before the first read, and after a read that returned
    /// [`LogError::Eof`], [`LogError::SessionEnd`], or
    /// [`LogError::ReadTruncated`]. A [`LogError::ReadOverflow`] leaves
    /// the segment file open.
    #[must_use]
    pub fn current_header(&self) -> Option<&SegmentHeader> {
        self.current.as_ref().map(|segment| &segment.header)
    }

    /// The number of segment files this reader has opened.
    ///
    /// A record spanning several segment files only ever reports the one
    /// it ended in, so this tally, rather than a count of the headers the
    /// caller has seen, is what describes a log's extent.
    ///
    /// Returns that count.
    #[must_use]
    pub fn segments_opened(&self) -> u64 {
        self.segments_opened
    }

    /// Asks the reader to retain the header of every segment file it
    /// opens.
    ///
    /// Retention is off by default and must be: the headers accumulate
    /// until taken, so a caller that never took them would grow the
    /// buffer without bound.
    ///
    /// * `enable` -- whether to retain headers from now on.
    pub fn collect_opened_headers(&mut self, enable: bool) {
        self.collect_headers = enable;
    }

    /// Removes and returns the headers retained since the last call.
    ///
    /// Taking them before examining a read's result puts each header
    /// ahead of the records it carried, and lets a read that ended the
    /// log still report the segment files it opened.
    ///
    /// Returns the headers, oldest first.
    pub fn take_opened_headers(&mut self) -> Vec<SegmentHeader> {
        std::mem::take(&mut self.opened_headers)
    }

    /// Reads and decodes one data record's data header.
    ///
    /// Returns the payload length it declares and the metadata it
    /// carries. `Format::Fixed` has no data header, so its length comes
    /// from the format itself.
    fn read_data_header(
        &mut self,
        cursor: &mut Cursor,
        format: Format,
    ) -> Result<(RecSize, Meta), LogError> {
        match format {
            Format::Fixed(n) => {
                cursor.total = Some(u64::from(n));
                Ok((n, Meta::Fixed))
            }
            Format::VariableSimple => {
                let mut head = [0u8; 4];
                self.fill(cursor, &mut head)?;
                let n = self.settle_payload_len(cursor, RecSize::from_le_bytes(head))?;
                cursor.total = Some(u64::from(format.data_header_len()) + u64::from(n));
                Ok((n, Meta::VariableSimple))
            }
            Format::VariableTsRc => {
                let mut head = [0u8; 20];
                self.fill(cursor, &mut head)?;
                let mut four = [0u8; 4];
                let mut eight = [0u8; 8];
                four.copy_from_slice(&head[0..4]);
                let n = self.settle_payload_len(cursor, RecSize::from_le_bytes(four))?;
                eight.copy_from_slice(&head[4..12]);
                let ts = Timestamp::from_le_bytes(eight);
                eight.copy_from_slice(&head[12..20]);
                let rc = RecordCount::from_le_bytes(eight);
                cursor.total = Some(u64::from(format.data_header_len()) + u64::from(n));
                Ok((n, Meta::VariableTsRc(ts, rc)))
            }
        }
    }

    /// Checks a freshly decoded payload length against the one a
    /// crossing inside the data header implied.
    ///
    /// A crossing part way through a data header has no owed-byte count
    /// to check against, so it is checked afterwards instead: the
    /// surviving segment's `remaining` field fixes the payload length
    /// exactly, and a header that decodes to anything else was finished
    /// with bytes that were not this record's.
    ///
    /// Returns the payload length when the two agree.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::ReadTruncated`] when they disagree, having
    /// handed the offending segment file back for the resynchronization
    /// to re-examine.
    fn settle_payload_len(
        &mut self,
        cursor: &Cursor,
        decoded: RecSize,
    ) -> Result<RecSize, LogError> {
        if let Some(implied) = cursor.implied_payload {
            if implied != u64::from(decoded) {
                return Err(self.reject_current(0, 0));
            }
        }
        Ok(decoded)
    }

    /// Finds the first byte of the next fresh data record.
    ///
    /// Opens segment files until one offers a record boundary. A file
    /// whose `remaining` field covers its whole data section offers
    /// none, because every byte of it belongs to a record that began in a
    /// file which is not available; such a file is discarded and the
    /// search moves on.
    ///
    /// Returns where the read position was left.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::Eof`] when the pending list runs out, and
    /// [`LogError::SessionEnd`] when the next segment file belongs to a
    /// different session.
    fn find_record_start(&mut self) -> Result<Start, LogError> {
        debug_assert!(
            self.current.is_none(),
            "a resynchronization must not leave a segment file open"
        );
        loop {
            // The hint is good for one attempt: it describes the segment
            // file the rejected crossing opened, which is the one at the
            // front of the pending list.
            let recovery = self.recovery.take();

            if !self.open_next()? {
                return Err(LogError::Eof);
            }
            let Some(segment) = self.current.as_ref() else {
                return Err(LogError::Eof);
            };
            let id = segment.id;
            let remaining = segment.header.remaining;
            let data_len = segment.data_len;
            let format = segment.header.format;
            let nominal = u64::from(segment.header.data_len());

            if let Some(recovery) = recovery {
                if let Some(n) = recovered_payload_len(&recovery, id, format, remaining, nominal) {
                    return Ok(Start::Recovered(n));
                }
            }

            if remaining >= data_len {
                // Every byte here is the tail of a record whose opening
                // is gone, and `remaining` says how many of its bytes
                // are still to come but not where it started. Without
                // its data header a whole payload cannot be told from
                // the tail of one, so the file offers nothing.
                self.current = None;
                continue;
            }

            // Skipping zero bytes is not a special case: a `remaining` of
            // zero leaves the position exactly at the fresh record's
            // first byte.
            let Some(segment) = self.current.as_mut() else {
                return Err(LogError::Eof);
            };
            segment.file.seek(SeekFrom::Current(as_i64(remaining)))?;
            segment.pos = remaining;
            return Ok(Start::Fresh);
        }
    }

    /// Opens the next segment file that can be used, making it current.
    ///
    /// A file that fails validation is skipped silently and the search
    /// moves on: to a reader, a segment file it cannot use is a segment
    /// file it does not have, and the loss surfaces the way a deleted
    /// file's does, through the sequence gap its neighbours show.
    ///
    /// Returns true when a segment file became current, and false when
    /// the pending list is exhausted.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::SessionEnd`] when the next usable segment
    /// file belongs to a different session than the records read so far.
    /// The file is handed back unopened-for-accounting, so the call
    /// after that opens it as the first segment file of its session.
    fn open_next(&mut self) -> Result<bool, LogError> {
        while let Some(id) = self.pending.pop_front() {
            let Some(segment) = self.open_validated(id) else {
                continue;
            };

            if let Some(session) = self.session {
                if segment.header.session_id != session {
                    self.pending.push_front(id);
                    self.session = None;
                    self.prev_seq = None;
                    self.recovery = None;
                    return Err(LogError::SessionEnd);
                }
            }

            // Count each segment file once. A failed crossing hands one
            // back to be opened again, and counting that twice would
            // overstate the log's extent and repeat its header.
            if self.reopened == Some(id) {
                self.reopened = None;
            } else {
                self.segments_opened += 1;
                if self.collect_headers {
                    self.opened_headers.push(segment.header);
                }
            }

            if self.session.is_none() {
                self.session = Some(segment.header.session_id);
                // No crossing reaches a session's first surviving
                // segment file, so its own sequence is the only evidence
                // of files lost ahead of it.
                self.session_start_lost = segment.header.sequence.as_u64();
            }
            self.prev_seq = Some(segment.header.sequence);
            self.current = Some(segment);
            return Ok(true);
        }
        Ok(false)
    }

    /// Opens one segment file and checks everything that must hold before
    /// it may be read.
    ///
    /// The file must open and report its length; the length must be at
    /// least a header, since a file too short to hold one cannot be a
    /// segment file; the type, version, and data format fields must be
    /// ones this build accepts; the stored segment ID must match the one
    /// in the name, which catches a renamed or copied file; and the file
    /// must be no larger than the maximum size its own header declares.
    ///
    /// Returns the open segment file, or `None` if any check failed. No
    /// distinction is drawn between the reasons, because none would
    /// change what the reader does.
    fn open_validated(&mut self, id: SegId) -> Option<Segment> {
        let mut file = {
            let path = self.segment_path(id);
            File::open(path).ok()?
        };
        let file_len = file.metadata().ok()?.len();
        if file_len < u64::from(SEGMENT_FILE_HEADER_LEN) {
            return None;
        }
        let header = SegmentHeader::read_from(&mut file).ok()?;
        if header.segment_id != id {
            return None;
        }
        if file_len > u64::from(header.max_size) {
            return None;
        }
        Some(Segment {
            id,
            file,
            header,
            data_len: file_len - u64::from(SEGMENT_FILE_HEADER_LEN),
            pos: 0,
        })
    }

    /// Whether the current segment file has no more data section left,
    /// which includes there being no current segment file at all.
    fn exhausted(&self) -> bool {
        match self.current.as_ref() {
            None => true,
            Some(segment) => segment.pos >= segment.data_len,
        }
    }

    /// Crosses from an exhausted segment file into the next one, and
    /// checks that the next one really does continue this record.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::ReadTruncated`] when the crossing cannot be
    /// allowed, having handed the offending segment file back for the
    /// resynchronization to re-examine, and [`LogError::SessionEnd`] at
    /// a session boundary that falls on a record boundary.
    fn cross(&mut self, cursor: &mut Cursor) -> Result<(), LogError> {
        let prev_seq = self.prev_seq;
        let prev_session = self.session;
        self.current = None;

        let opened = match self.open_next() {
            Ok(opened) => opened,
            Err(LogError::SessionEnd) if cursor.consumed > 0 => {
                // Part way through a record, a session boundary is the
                // end of that record rather than a tidy end of session.
                // Put the session back so the boundary is still reported
                // once the reader has resynchronized.
                self.session = prev_session;
                self.prev_seq = prev_seq;
                return Err(LogError::ReadTruncated {
                    lost: 0,
                    n: cursor.payload_in_buf,
                });
            }
            Err(e) => return Err(e),
        };
        if !opened {
            // The record runs off the end of the log, so its tail was in
            // a segment file that is gone.
            return Err(LogError::ReadTruncated {
                lost: 0,
                n: cursor.payload_in_buf,
            });
        }

        let Some((id, remaining, sequence, header_len)) = self.current.as_ref().map(|segment| {
            (
                segment.id,
                segment.header.remaining,
                segment.header.sequence,
                u64::from(segment.header.format.data_header_len()),
            )
        }) else {
            return Err(LogError::ReadTruncated {
                lost: 0,
                n: cursor.payload_in_buf,
            });
        };

        let mut reject = false;
        if cursor.consumed == 0 {
            // The previous segment file ended exactly at a record
            // boundary, so this one must announce a fresh record.
            reject = remaining != 0;
        } else if let Some(total) = cursor.total {
            reject = remaining != total - cursor.consumed;
        } else {
            // Part way through a data header there is no owed-byte count
            // yet, and this is the one crossing the writer does produce.
            // Every byte consumed is a header byte, so `remaining` must
            // cover at least the rest of the header; what it holds
            // beyond that is the payload, which fixes the payload length
            // for `settle_payload_len` to check the decoded header
            // against. An intact log satisfies this by construction.
            let rest = header_len.saturating_sub(cursor.consumed);
            if remaining < rest {
                reject = true;
            } else {
                let implied = remaining - rest;
                match cursor.implied_payload {
                    // A second crossing inside one header that implies a
                    // different length cannot be believed either.
                    Some(earlier) if earlier != implied => reject = true,
                    _ => cursor.implied_payload = Some(implied),
                }
            }
        }

        // The sequence check is what sees a segment file lost on a
        // record boundary, where `remaining` is zero on both sides of
        // the gap and the check above is satisfied.
        let lost = match prev_seq.map(|prev| (prev.as_u64(), sequence.as_u64())) {
            Some((prev, seq)) => match seq.cmp(&(prev + 1)) {
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => {
                    reject = true;
                    seq - prev - 1
                }
                // A sequence that does not advance cannot follow, and
                // names no count of lost files either.
                std::cmp::Ordering::Less => {
                    reject = true;
                    0
                }
            },
            None => 0,
        };

        if reject {
            // A single missing segment file held exactly one data
            // section, which is sometimes enough to place the record
            // continuing out of it. Leave what the attempt needs.
            if lost == 1 {
                if let Some(total) = cursor.total {
                    self.recovery = Some(Recovery {
                        segment: id,
                        owed: total - cursor.consumed,
                    });
                }
            }
            return Err(self.reject_current(lost, cursor.payload_in_buf));
        }
        Ok(())
    }

    /// Gives up the current segment file, handing it back to the pending
    /// list so that the resynchronization re-examines it as the start of
    /// a fresh record.
    ///
    /// Returns the error to report for the record that was lost.
    fn reject_current(&mut self, lost: u64, n: RecSize) -> LogError {
        if let Some(segment) = self.current.take() {
            self.pending.push_front(segment.id);
            self.reopened = Some(segment.id);
        }
        LogError::ReadTruncated { lost, n }
    }

    /// Reads exactly `dst.len()` bytes of the record in progress,
    /// crossing into further segment files as needed.
    ///
    /// # Errors
    ///
    /// Returns whatever [`cross`](Self::cross) reports at a boundary,
    /// and [`LogError::IoError`] if a read fails.
    fn fill(&mut self, cursor: &mut Cursor, dst: &mut [u8]) -> Result<(), LogError> {
        let mut off = 0usize;
        while off < dst.len() {
            if self.exhausted() {
                self.cross(cursor)?;
            }
            let Some(segment) = self.current.as_mut() else {
                return Err(LogError::ReadTruncated {
                    lost: 0,
                    n: cursor.payload_in_buf,
                });
            };
            let avail = usize::try_from(segment.data_len - segment.pos).unwrap_or(usize::MAX);
            let take = avail.min(dst.len() - off);
            segment.file.read_exact(&mut dst[off..off + take])?;
            segment.pos += take as u64;
            cursor.consumed += take as u64;
            if cursor.in_payload {
                cursor.payload_in_buf = cursor
                    .payload_in_buf
                    .saturating_add(RecSize::try_from(take).unwrap_or(RecSize::MAX));
            }
            off += take;
        }
        Ok(())
    }

    /// Advances past `count` bytes of the record in progress, crossing
    /// into further segment files as needed.
    ///
    /// This is how a read whose buffer was too small reaches the end of
    /// the record, so that the next read begins at the following one.
    ///
    /// # Errors
    ///
    /// Returns whatever [`cross`](Self::cross) reports at a boundary,
    /// and [`LogError::IoError`] if a seek fails.
    fn skip(&mut self, cursor: &mut Cursor, mut count: u64) -> Result<(), LogError> {
        while count > 0 {
            if self.exhausted() {
                self.cross(cursor)?;
            }
            let Some(segment) = self.current.as_mut() else {
                return Err(LogError::ReadTruncated {
                    lost: 0,
                    n: cursor.payload_in_buf,
                });
            };
            let step = (segment.data_len - segment.pos).min(count);
            segment.file.seek(SeekFrom::Current(as_i64(step)))?;
            segment.pos += step;
            cursor.consumed += step;
            count -= step;
        }
        Ok(())
    }

    /// Builds the full path of the segment file with the given ID into a
    /// buffer this reader owns.
    ///
    /// The buffer is reused, so walking a log opens one segment file
    /// after another without allocating. Its last component is always a
    /// segment file name of the right length after the first call, so
    /// replacing it does not grow it either.
    fn segment_path(&mut self, id: SegId) -> &Path {
        build_name(&mut self.name_buf, &self.prefix, id, &self.suffix);
        self.path_buf.set_file_name(&self.name_buf);
        &self.path_buf
    }
}

/// Decides whether a record whose data header was lost with a single
/// missing segment file can be placed after all.
///
/// The gap held one full data section, since every segment file but a
/// session's last is exactly the maximum size. Take out what the
/// interrupted record still owed, and what is left is the room the gap
/// had for records of its own. If that is exactly one data header, the
/// gap held the data header of the record continuing into this segment
/// file and nothing else of it: no payload byte of that record, and no
/// complete record ahead of it, because even an empty record costs a
/// data header. The payload therefore begins at the first byte of this
/// data section and is `remaining` bytes long.
///
/// * `recovery` -- what was known when the gap was found.
/// * `id` -- the segment file now being examined.
/// * `format` -- that file's data record layout.
/// * `remaining` -- that file's `remaining` field.
/// * `data_section` -- the data section size the file's header implies,
///   which is the size of the one the gap swallowed.
///
/// Returns the payload length, or `None` when the arithmetic does not
/// settle it. Any other value for the room leaves a choice between
/// payload bytes lost from the front of this record and whole records
/// lost ahead of it, which nothing on disk resolves. Room smaller than a
/// data header is one of those values: the gap held the leading bytes of
/// a header whose tail begins this file, and neither piece can be read
/// without the other.
fn recovered_payload_len(
    recovery: &Recovery,
    id: SegId,
    format: Format,
    remaining: u64,
    data_section: u64,
) -> Option<RecSize> {
    if recovery.segment != id || remaining == 0 {
        return None;
    }
    // Only VariableSimple: its data header holds nothing but the payload
    // length, which `remaining` supplies. A VariableTsRc header also
    // carries a timestamp and a record count, which cannot be
    // reconstructed, and Fixed has no data header for a gap to swallow.
    if format != Format::VariableSimple {
        return None;
    }
    let room = data_section.checked_sub(recovery.owed)?;
    if room != u64::from(format.data_header_len()) {
        return None;
    }
    RecSize::try_from(remaining).ok()
}

/// Narrows a byte count to the signed offset a seek takes.
///
/// The counts are all bounded by a file length, so the saturation is
/// unreachable in practice; it is written out rather than asserted so
/// that no stored value can panic.
fn as_i64(count: u64) -> i64 {
    i64::try_from(count).unwrap_or(i64::MAX)
}

/// An iterator over a log's remaining records, as returned by
/// [`LogRead::iter`].
pub struct LogReadIter<'a> {
    /// The reader being walked.
    log: &'a mut LogRead,
    /// Buffer each record is read into before being copied out.
    buf: Vec<u8>,
}

impl Iterator for LogReadIter<'_> {
    type Item = Record;

    /// Returns the next record, skipping session boundaries and records
    /// of which no byte survived, and stopping at the end of the log or
    /// at an I/O failure.
    fn next(&mut self) -> Option<Record> {
        loop {
            match self.log.read(&mut self.buf) {
                Ok(result) => {
                    return Some(Record {
                        meta: result.meta,
                        payload: self.buf[..result.n as usize].to_vec(),
                        truncated: false,
                    })
                }
                // Bytes captured before an overflow or a gap are real
                // telemetry, so they are handed over rather than
                // dropped -- marked, because they are the front of a
                // record and not the whole of one.
                Err(LogError::ReadOverflow(n) | LogError::ReadTruncated { n, .. }) if n > 0 => {
                    return Some(Record {
                        meta: self.log.last_meta,
                        payload: self.buf[..n as usize].to_vec(),
                        truncated: true,
                    })
                }
                Err(LogError::SessionEnd | LogError::ReadTruncated { .. }) => (),
                Err(_) => return None,
            }
        }
    }
}
