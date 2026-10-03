//! Writing a log: [`LogWrite`] and the callbacks it invokes.

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::error::LogError;
use crate::format::{Format, Meta};
use crate::header::{SegmentHeader, SEGMENT_FILE_HEADER_LEN};
use crate::segid::SegId;
use crate::seq_id::SeqId;
use crate::util::{build_name, check_log_location, scan_segment_ids};
use crate::{RecSize, RecordCount, Timestamp};

include!(concat!(env!("OUT_DIR"), "/timer_resolution.rs"));

/// Largest data header this crate has, which is
/// [`Format::VariableTsRc`]'s. A buffer of this size holds any format's
/// data header, so building one allocates nothing.
const MAX_DATA_HEADER_LEN: usize = 20;

/// Callbacks a [`LogWrite`] invokes as it works.
///
/// The members are plain function pointers rather than trait objects or
/// closures, so that the structure sits inline in a `LogWrite` with no
/// heap allocation and no dynamic dispatch. That also lets a caller
/// build one in a `const`.
///
/// The [`Default`] implementation does nothing in either callback, which
/// suits local development. Storing telemetry for real means replacing
/// [`send`](Self::send): without it, segment files accumulate in the
/// directory and the storage bound the log was given stops holding.
#[derive(Clone, Copy)]
pub struct WriteCallbacks {
    /// Called after each data record has been written, with the segment
    /// file the record ended in.
    ///
    /// What it does is the caller's choice of priority: flushing the
    /// file trades throughput for a smaller window in which a restart
    /// loses the record, and doing nothing makes the opposite trade.
    pub record_complete: fn(&mut File) -> std::io::Result<()>,

    /// Transfers ownership of a segment file from this library to the
    /// caller.
    ///
    /// Called with the full path of a segment file whose data section
    /// has filled, and also with each pre-existing segment file that
    /// [`LogWrite::new`] finds. The caller may compress it, move it,
    /// send it down, or announce it.
    ///
    /// When it returns there must be no file at the path it was given,
    /// and none matching this log's naming pattern, because the space is
    /// no longer being accounted for by this library.
    ///
    /// The cheapest way to satisfy that is to rename the file to
    /// something that cannot be a segment file name of this log -- a
    /// name that does not both begin with the prefix and end with the
    /// suffix -- and deal with it under the new name. A rename within a
    /// directory is a metadata operation, where a copy is not.
    ///
    /// It also takes the file out of what [`LogWrite::new`] has to look
    /// at. Constructing a writer enumerates the log's directory, parses
    /// the identifier out of every name matching the pattern, and sorts
    /// them: that one scan is both what finds the files to hand over
    /// here and what seeds the writer's clock, and it costs time in
    /// proportion to how many such files there are. A `send` that leaves
    /// them in the log's namespace makes every later `LogWrite::new`
    /// pay for all of them, and hands each of them over again.
    ///
    /// A file renamed out of the namespace no longer seeds the clock a
    /// later writer starts from, which is the contract working as
    /// intended rather than a loss: once this callback has taken a file,
    /// the library accounts for it no further.
    pub send: fn(&Path) -> std::io::Result<()>,
}

impl Default for WriteCallbacks {
    /// Callbacks that do nothing.
    fn default() -> Self {
        Self {
            record_complete: |_| Ok(()),
            send: |_| Ok(()),
        }
    }
}

impl std::fmt::Debug for WriteCallbacks {
    /// Function pointers have nothing worth printing, so this reports
    /// only that the structure is one of these.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WriteCallbacks { .. }")
    }
}

/// The segment file being written.
struct Current {
    /// Identifier of this segment file, and of the file it is named for.
    id: SegId,
    /// Full path, kept so that it can be handed to `send` without being
    /// rebuilt.
    path: PathBuf,
    /// The open file, positioned after everything written so far.
    file: File,
    /// Bytes written to the data section, header excluded.
    data_written: u32,
}

/// Writes a log, one session at a time.
///
/// Constructing a `LogWrite` starts a session: every pre-existing
/// segment file matching the prefix and suffix is handed to
/// [`WriteCallbacks::send`], and a fresh segment file is created. A new
/// `LogWrite` therefore never appends to what it finds; it takes the
/// older files off this library's hands and starts afresh.
pub struct LogWrite {
    /// Directory the segment files live in.
    dir: PathBuf,
    /// First part of every segment file name.
    prefix: String,
    /// Last part of every segment file name.
    suffix: String,
    /// Largest a segment file of this log may grow.
    seg_size_max: u32,
    /// How data records are laid out.
    format: Format,
    /// Callbacks supplied by the caller.
    callbacks: WriteCallbacks,
    /// The clock segment IDs and record timestamps are minted from,
    /// anchored when the writer was constructed. It is deliberately not
    /// re-anchored by `clear`: a session that began after a backward
    /// step of the real-time clock would otherwise take identifiers
    /// below those of the files already in the directory.
    clock: Clock,
    /// The segment file being written, absent after `clear`.
    current: Option<Current>,
    /// Identifier of this session's first segment file.
    session_id: SegId,
    /// Position of the current segment file within the session.
    sequence: SeqId,
    /// Position of the record last written within the session.
    record_count: RecordCount,
    /// Bytes of the record in progress not yet written. This is what a
    /// new segment file's `remaining` field is set from.
    record_bytes_left: u64,
    /// Metadata minted for the record last written.
    last_meta: Meta,
    /// Reusable data header buffer, so that building a record allocates
    /// nothing.
    header_buf: [u8; MAX_DATA_HEADER_LEN],
    /// Reusable file name buffer, for the same reason.
    name_buf: String,
    /// A path buffer not currently naming anything, handed back and
    /// forth with the open segment file so that a roll allocates no
    /// path. Absent only between a failed creation and the next one.
    spare_path: Option<PathBuf>,
}

impl LogWrite {
    /// The length of the segment file header, in bytes.
    ///
    /// An alias for the crate-level [`SEGMENT_FILE_HEADER_LEN`],
    /// repeated here because `seg_size_max` is specified relative to it.
    pub const SEGMENT_FILE_HEADER_LEN: u32 = SEGMENT_FILE_HEADER_LEN;

    /// Begins writing a log, in a new session.
    ///
    /// Every segment file already in `dir` whose name matches `prefix`
    /// and `suffix` is handed to [`WriteCallbacks::send`] before this
    /// session's first segment file is created.
    ///
    /// * `dir` -- directory to hold the segment files. It must already
    ///   exist; this function does not create it.
    /// * `prefix` -- first part of every segment file name. Must hold no
    ///   path separator.
    /// * `suffix` -- last part of every segment file name. Must hold no
    ///   path separator.
    /// * `seg_size_max` -- largest a segment file may grow, in bytes.
    ///   Must be strictly greater than the segment file header plus one
    ///   data header for `format`: a file that could not hold a single
    ///   data header would leave the writer nowhere to put a record.
    /// * `format` -- how data records are to be laid out.
    /// * `callbacks` -- functions to invoke as segment files fill and
    ///   records complete.
    ///
    /// Returns the new writer, with its first segment file created and
    /// its header written.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::TimerResolutionZero`] when the build-time
    /// timer resolution is zero, [`LogError::ClockError`] when the
    /// real-time clock does not read later than the UNIX epoch, which is
    /// what an unset clock reads on most systems,
    /// [`LogError::PathDelimiterNotAllowed`]
    /// for a prefix or suffix holding a path separator,
    /// [`LogError::SegSizeTooSmall`] for a `seg_size_max` that is not
    /// strictly greater than the segment header plus one data header,
    /// [`LogError::FixedLenMismatch`] for `Format::Fixed(0)`,
    /// [`LogError::InvalidPathname`] when `dir` does not name a
    /// directory, and [`LogError::IoError`] from enumerating the
    /// directory, from the `send` callback, from creating the segment
    /// file, or from writing its header.
    pub fn new(
        dir: &str,
        prefix: &str,
        suffix: &str,
        seg_size_max: u32,
        format: Format,
        callbacks: WriteCallbacks,
    ) -> Result<Self, LogError> {
        if TIMER_RESOLUTION_NS == 0 {
            return Err(LogError::TimerResolutionZero);
        }
        if seg_size_max <= SEGMENT_FILE_HEADER_LEN.saturating_add(format.data_header_len()) {
            return Err(LogError::SegSizeTooSmall);
        }
        if format == Format::Fixed(0) {
            return Err(LogError::FixedLenMismatch);
        }
        // Anchored before the directory is touched: an unset real-time
        // clock cannot be corrected once the anchor is taken, so it is
        // refused here rather than left to misdate segment files.
        let clock = Clock::new()?;
        let dir = check_log_location(dir, prefix, suffix)?;

        let mut log = Self {
            dir,
            prefix: prefix.to_string(),
            suffix: suffix.to_string(),
            seg_size_max,
            format,
            callbacks,
            clock,
            current: None,
            session_id: SegId::from_u64(0),
            sequence: SeqId::ZERO,
            record_count: 0,
            record_bytes_left: 0,
            last_meta: match format {
                Format::Fixed(_) => Meta::Fixed,
                Format::VariableSimple => Meta::VariableSimple,
                Format::VariableTsRc => Meta::VariableTsRc(0, 0),
            },
            header_buf: [0u8; MAX_DATA_HEADER_LEN],
            name_buf: String::with_capacity(prefix.len() + SegId::STR_LEN + suffix.len()),
            spare_path: None,
        };

        // Hand over what is already there before writing anything, so
        // that the storage the older files occupy stops being this
        // library's concern before more is committed to.
        // Seeded from the files already there, before any is created:
        // this writer anchored its clock on a fresh reading of the
        // real-time clock, which may have been stepped backwards since
        // the writer that minted those identifiers read it.
        if let Some(highest) = log.send_existing()? {
            log.clock.advance_past(highest);
        }
        log.start_session()?;
        Ok(log)
    }

    /// The segment ID of this session's first segment file.
    ///
    /// This is the value written to the session ID field of every
    /// segment file of the session, and so the value a reader watches
    /// for a change in.
    ///
    /// Returns the session identifier. After [`clear`](Self::clear) this
    /// still names the cleared session, whose files no longer exist,
    /// until the next [`write`](Self::write) starts a session.
    #[must_use]
    pub fn session_id(&self) -> SegId {
        self.session_id
    }

    /// The segment ID of the segment file being written.
    ///
    /// Returns that identifier. After [`clear`](Self::clear) this still
    /// names the last file of the cleared session, which no longer
    /// exists, until the next [`write`](Self::write) creates one.
    #[must_use]
    pub fn current_segment_id(&self) -> SegId {
        self.current
            .as_ref()
            .map_or(self.session_id, |current| current.id)
    }

    /// The metadata minted for the most recently written record.
    ///
    /// For [`Format::VariableTsRc`] this is how a caller learns the
    /// timestamp and record count that were stored, since both are
    /// generated as the data header is built rather than supplied by the
    /// caller.
    ///
    /// Returns that metadata. Before the first write it reports the
    /// format with zeroed values.
    #[must_use]
    pub fn last_meta(&self) -> Meta {
        self.last_meta
    }

    /// Writes the UTF-8 bytes of a string as one data record.
    ///
    /// * `msg` -- the text to store.
    ///
    /// Returns what [`write`](Self::write) returns: the total bytes
    /// written, data header included.
    ///
    /// # Errors
    ///
    /// The same errors as [`write`](Self::write).
    pub fn write_str(&mut self, msg: &str) -> Result<u32, LogError> {
        self.write(msg.as_bytes())
    }

    /// Writes a byte array to the log as one data record.
    ///
    /// The record may span segment files. Each time the current file
    /// fills, [`WriteCallbacks::send`] is invoked with its path and a
    /// fresh segment file is opened. A record begins wherever the
    /// current file has room for a byte of it, so a data header can
    /// straddle a boundary; that is what lets every segment file but a
    /// session's last be exactly `seg_size_max` bytes.
    ///
    /// When there is no current segment file -- the state
    /// [`clear`](Self::clear) leaves behind -- a new session is started
    /// before the record is built. It must happen in that order:
    /// starting a session restarts the record count, so building the
    /// data header first would stamp this record with the cleared
    /// session's count and then issue that same number again to the
    /// record after it.
    ///
    /// [`WriteCallbacks::record_complete`] is invoked once every byte
    /// has been written.
    ///
    /// * `msg` -- the payload bytes to store.
    ///
    /// Returns the total number of bytes written, counting the
    /// per-record data header as well as the payload.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::FixedLenMismatch`] if the format is
    /// `Fixed(n)` and `msg.len()` is anything other than `n`, a
    /// zero-length payload included.
    /// [`LogError::PayloadTooLarge`] if `msg.len()` exceeds
    /// [`RecSize::MAX`](crate::RecSize), or if the payload plus its data
    /// header would exceed the `u32` this function returns -- a count
    /// that wrapped would understate what was written.
    /// [`LogError::IoError`] as encountered, including from creating a
    /// segment file on a roll. A timestamp cannot fail here: the clock a
    /// record is stamped from was validated when the writer was
    /// constructed.
    pub fn write(&mut self, msg: &[u8]) -> Result<u32, LogError> {
        if let Format::Fixed(n) = self.format {
            if msg.len() != n as usize {
                return Err(LogError::FixedLenMismatch);
            }
        }
        if msg.len() > RecSize::MAX as usize {
            return Err(LogError::PayloadTooLarge);
        }
        let header_len = self.format.data_header_len();
        let total = u64::from(header_len) + msg.len() as u64;
        let total = u32::try_from(total).map_err(|_| LogError::PayloadTooLarge)?;

        if self.current.is_none() {
            self.start_session()?;
        }

        // Roll an exactly-full segment before the record begins. Rolling
        // afterwards would set the new file's `remaining` to the whole
        // record size, and a reader takes that to mean the record began
        // in an earlier file and skips it as unrecoverable.
        if self.room() == 0 {
            self.roll()?;
        }

        let (built, meta) = self.build_data_header(msg.len())?;
        self.record_bytes_left = u64::from(total);
        self.write_bytes_from_header(built)?;
        self.write_bytes(msg)?;
        self.last_meta = meta;

        let record_complete = self.callbacks.record_complete;
        if let Some(current) = self.current.as_mut() {
            record_complete(&mut current.file)?;
        }
        Ok(total)
    }

    /// Flushes buffered data for the current segment file to storage.
    ///
    /// Returns nothing on success, and nothing to do when there is no
    /// current segment file.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::IoError`] if the underlying flush fails.
    pub fn flush(&mut self) -> Result<(), LogError> {
        if let Some(current) = self.current.as_mut() {
            current.file.flush()?;
        }
        Ok(())
    }

    /// Removes every one of this log's segment files, including the one
    /// being written.
    ///
    /// The current segment file is closed before any name is unlinked.
    /// Not every supported platform permits removing an open file, and
    /// closing it is also what makes the clearing complete rather than
    /// partial: a log cleared down to its last segment file still reads
    /// back as a log, which is not what a caller reclaiming storage
    /// asked for.
    ///
    /// The records in those files are discarded, a record part way
    /// through being written included. [`WriteCallbacks::send`] is not
    /// invoked for any of them: that callback hands a segment file to
    /// user code, and these are being thrown away.
    ///
    /// The writer stays usable and is left with no current segment file.
    /// The next [`write`](Self::write) begins a new session, whose first
    /// segment file carries sequence zero and numbers its records from
    /// one. A new session is required rather than merely tidy:
    /// continuing the cleared session's numbering would leave a lone
    /// segment file claiming a position with nothing before it, which a
    /// reader must report as that many segment files lost -- a log
    /// emptied deliberately would come back as one damaged by a fault.
    ///
    /// Returns nothing on success.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::IoError`] if enumerating the directory or
    /// removing a file fails. The current segment file is closed before
    /// either is attempted, so it is closed even when the removal that
    /// follows fails.
    pub fn clear(&mut self) -> Result<(), LogError> {
        self.current = None;
        self.record_bytes_left = 0;
        // The highest identifier is of no use here: every file carrying
        // one is being removed, and this writer's clock is already past
        // them all.
        self.for_each_segment_file(|path| fs::remove_file(path))
            .map(|_| ())
    }

    /// Hands every pre-existing segment file of this log to the `send`
    /// callback.
    ///
    /// Returns the highest identifier those files carried, which is what
    /// [`Clock::advance_past`] needs, or `None` if there were none.
    fn send_existing(&mut self) -> Result<Option<SegId>, LogError> {
        let send = self.callbacks.send;
        self.for_each_segment_file(send)
    }

    /// Applies `action` to the path of every segment file of this log,
    /// oldest first.
    ///
    /// Enumerating a directory allocates whatever the platform needs to
    /// list it, so this is the one operation a caller can reach that
    /// does allocate. The steady-state paths -- `write` and everything
    /// under it -- do not.
    ///
    /// Returns the highest identifier found, or `None` for a directory
    /// holding no segment file of this log. It is taken from the scan
    /// before `action` runs, so it is still reported for files the action
    /// goes on to remove or rename.
    fn for_each_segment_file(
        &mut self,
        mut action: impl FnMut(&Path) -> std::io::Result<()>,
    ) -> Result<Option<SegId>, LogError> {
        let ids = scan_segment_ids(&self.dir, &self.prefix, &self.suffix)?;
        // The scan sorts, so the highest is the last.
        let highest = ids.last().copied();
        let mut path = self.placeholder_path();
        for id in ids {
            build_name(&mut self.name_buf, &self.prefix, id, &self.suffix);
            path.set_file_name(&self.name_buf);
            action(&path)?;
        }
        Ok(highest)
    }

    /// Starts a session: resets the record count and sequence, then
    /// creates the session's first segment file.
    ///
    /// The session identifier is that file's own segment ID, which is
    /// what lets a reader recognize the start of a session by comparing
    /// the two fields.
    fn start_session(&mut self) -> Result<(), LogError> {
        self.record_count = 0;
        self.sequence = SeqId::ZERO;
        self.record_bytes_left = 0;
        self.create_segment(true)
    }

    /// Creates the next segment file and writes its header.
    ///
    /// * `opens_session` -- true when this file begins a session, in
    ///   which case its own identifier becomes the session identifier.
    fn create_segment(&mut self, opens_session: bool) -> Result<(), LogError> {
        // The buffer the segment file just closed was named in, so that
        // a roll allocates nothing.
        let buf = self
            .spare_path
            .take()
            .unwrap_or_else(|| self.placeholder_path());
        let (id, path, file) = self.create_unique_file(buf)?;
        if opens_session {
            self.session_id = id;
        }
        let header = SegmentHeader {
            segment_id: id,
            session_id: self.session_id,
            max_size: self.seg_size_max,
            // What the previous segment file still owes the record it
            // was part way through, and zero when it ended on a record
            // boundary.
            remaining: self.record_bytes_left,
            format: self.format,
            sequence: self.sequence,
        };
        let mut current = Current {
            id,
            path,
            file,
            data_written: 0,
        };
        header.write_to(&mut current.file)?;
        self.current = Some(current);
        Ok(())
    }

    /// Creates a segment file whose name no file has.
    ///
    /// The segment ID is the current time, so a collision means two
    /// files were created within one tick of the clock. Sleeping for
    /// twice the clock's resolution guarantees that the next reading is
    /// larger, and [`Clock`] never going backwards guarantees it is
    /// larger than every earlier one of this writer, so the retry
    /// terminates.
    /// * `buf` -- a path buffer to build the name in, so that creating a
    ///   segment file need not allocate one.
    fn create_unique_file(&mut self, mut buf: PathBuf) -> Result<(SegId, PathBuf, File), LogError> {
        let nap = Duration::from_nanos(TIMER_RESOLUTION_NS.saturating_mul(2));
        loop {
            let id = self.clock.now_seg_id();
            build_name(&mut self.name_buf, &self.prefix, id, &self.suffix);
            buf.set_file_name(&self.name_buf);
            match OpenOptions::new().write(true).create_new(true).open(&buf) {
                Ok(file) => return Ok((id, buf, file)),
                Err(e) if e.kind() == ErrorKind::AlreadyExists => thread::sleep(nap),
                Err(e) => return Err(LogError::IoError(e)),
            }
        }
    }

    /// A path inside this log's directory whose last component is about
    /// to be replaced by a segment file name.
    ///
    /// The component has to be there for `set_file_name` to replace:
    /// given a path ending in a separator it would replace the directory
    /// instead.
    fn placeholder_path(&self) -> PathBuf {
        self.dir.join("placeholder")
    }

    /// The segment file being written.
    ///
    /// Every caller has just made sure one is open, by starting a
    /// session or by rolling, and both of those report their own
    /// failures. The error here is therefore for a state the writer
    /// cannot reach; it is returned rather than asserted so that no
    /// path through this crate can panic.
    fn current_mut(&mut self) -> Result<&mut Current, LogError> {
        self.current
            .as_mut()
            .ok_or_else(|| LogError::IoError(std::io::Error::other("no segment file is open")))
    }

    /// Bytes still available in the current segment file's data section.
    ///
    /// Returns zero when there is no current segment file, which makes
    /// the caller roll or start a session.
    fn room(&self) -> u32 {
        self.current.as_ref().map_or(0, |current| {
            self.seg_size_max - SEGMENT_FILE_HEADER_LEN - current.data_written
        })
    }

    /// Builds the data header for a record of `payload_len` bytes into
    /// the reusable buffer.
    ///
    /// Returns the number of header bytes built and the metadata the
    /// header carries.
    fn build_data_header(&mut self, payload_len: usize) -> Result<(usize, Meta), LogError> {
        let n = RecSize::try_from(payload_len).map_err(|_| LogError::PayloadTooLarge)?;
        match self.format {
            Format::Fixed(_) => Ok((0, Meta::Fixed)),
            Format::VariableSimple => {
                self.header_buf[0..4].copy_from_slice(&n.to_le_bytes());
                Ok((4, Meta::VariableSimple))
            }
            Format::VariableTsRc => {
                let ts = self.clock.now();
                self.record_count += 1;
                let rc = self.record_count;
                self.header_buf[0..4].copy_from_slice(&n.to_le_bytes());
                self.header_buf[4..12].copy_from_slice(&ts.to_le_bytes());
                self.header_buf[12..20].copy_from_slice(&rc.to_le_bytes());
                Ok((20, Meta::VariableTsRc(ts, rc)))
            }
        }
    }

    /// Writes the first `len` bytes of the data header buffer.
    ///
    /// The buffer is a field of `self`, which the byte writer also
    /// borrows, so the bytes are copied to the stack first. A data
    /// header is at most twenty bytes, so the copy is free.
    fn write_bytes_from_header(&mut self, len: usize) -> Result<(), LogError> {
        let mut scratch = [0u8; MAX_DATA_HEADER_LEN];
        scratch[..len].copy_from_slice(&self.header_buf[..len]);
        self.write_bytes(&scratch[..len])
    }

    /// Appends bytes to the data section, rolling to a new segment file
    /// each time the current one is exactly full.
    ///
    /// Nothing is padded, because nothing needs to be: the writer stops
    /// at `seg_size_max` exactly and continues the record in the next
    /// file.
    fn write_bytes(&mut self, data: &[u8]) -> Result<(), LogError> {
        let mut off = 0usize;
        while off < data.len() {
            if self.room() == 0 {
                self.roll()?;
            }
            let room = self.room() as usize;
            let take = room.min(data.len() - off);
            let chunk = &data[off..off + take];

            let outcome = self.current_mut()?.file.write_all(chunk);
            if let Err(e) = outcome {
                // The segment file is given up rather than retried: its
                // contents are no longer trustworthy. A fresh one is
                // opened so that the next write has somewhere to go, and
                // a failure to open that is what reaches the caller
                // instead.
                self.abandon_after_error()?;
                return Err(LogError::IoError(e));
            }

            let current = self.current_mut()?;
            current.data_written += u32::try_from(take).unwrap_or(u32::MAX);
            self.record_bytes_left = self.record_bytes_left.saturating_sub(take as u64);
            off += take;
        }
        Ok(())
    }

    /// Closes the current segment file, hands it to `send`, and creates
    /// its successor.
    fn roll(&mut self) -> Result<(), LogError> {
        self.close_and_send()?;
        self.sequence = self.sequence.next();
        self.create_segment(false)
    }

    /// Flushes and closes the current segment file, then hands its path
    /// to the `send` callback.
    ///
    /// The path buffer is kept for the next segment file however this
    /// turns out, so that a writer that has met an error still rolls
    /// without allocating.
    fn close_and_send(&mut self) -> Result<(), LogError> {
        let Some(current) = self.current.take() else {
            return Ok(());
        };
        let Current { path, mut file, .. } = current;
        let mut result = file.flush().map_err(LogError::IoError);
        drop(file);
        if result.is_ok() {
            let send = self.callbacks.send;
            result = send(&path).map_err(LogError::IoError);
        }
        self.spare_path = Some(path);
        result
    }

    /// Recovers from a failed write by giving up the current segment
    /// file and opening a fresh one.
    ///
    /// The record in progress is abandoned, so the new file's
    /// `remaining` field is zero: the next write starts a record at the
    /// beginning of its data section.
    fn abandon_after_error(&mut self) -> Result<(), LogError> {
        self.record_bytes_left = 0;
        // A close that also fails must not mask the write error, and the
        // path still needs handing over, so the result is dropped here.
        let _ = self.close_and_send();
        self.sequence = self.sequence.next();
        self.create_segment(false)
    }
}

impl Drop for LogWrite {
    /// Flushes the current segment file and hands it to `send` if it
    /// holds any data, so that the records written last are not stranded
    /// in a file the caller was never told about.
    ///
    /// A destructor cannot report a failure and must not panic, so the
    /// errors from the flush and from `send` are discarded. This is the
    /// one place in the library where an error is dropped rather than
    /// returned.
    fn drop(&mut self) {
        let Some(mut current) = self.current.take() else {
            return;
        };
        if current.data_written == 0 {
            return;
        }
        let _ = current.file.flush();
        drop(current.file);
        let send = self.callbacks.send;
        let _ = send(&current.path);
    }
}

/// The clock a writer takes its segment IDs and record timestamps from:
/// the real-time clock's epoch, advanced by the monotonic clock.
///
/// A segment ID is nanoseconds since the UNIX epoch, which rules out
/// using [`Instant`] as one. An `Instant` is opaque -- no epoch, no
/// accessor, and a zero point that differs from one boot to the next --
/// so it can be subtracted from another `Instant` and nothing else.
///
/// Reading [`SystemTime`] afresh for each ID, though, makes the IDs only
/// as ordered as the real-time clock, and that clock is not ordered at
/// all: NTP or a manual setting can step it backwards, after which a
/// file created later takes an ID below one created earlier. No data is
/// lost, but a reader sorts segment files by ID and validates them by
/// sequence, so the two disagree and it reports an intact log as one
/// missing segment files.
///
/// Pairing the two clocks once gives an ID that is both: `delta` is the
/// distance from the UNIX epoch to the monotonic reading `mono`, and
/// every later time is `delta` plus however far `mono` has advanced
/// since. The real-time clock is read exactly once per writer, which is
/// why it has to be set by then -- see [`Clock::new`].
///
/// On Linux an `Instant` is `CLOCK_MONOTONIC`, which NTP slews but never
/// steps, so a derived time follows real time's *rate* while staying
/// immune to its jumps. It does not advance while the system is
/// suspended, so a writer that outlives a suspend reports times short by
/// however long that lasted; nothing in the standard library exposes a
/// clock that counts suspended time.
struct Clock {
    /// Nanoseconds from the UNIX epoch to the moment `mono` was taken.
    delta: Timestamp,
    /// The monotonic reading `delta` was paired with.
    mono: Instant,
}

impl Clock {
    /// Pairs the real-time clock with the monotonic one, fixing the
    /// epoch every later reading is measured from.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::ClockError`] if the real-time clock does not
    /// read later than the UNIX epoch. That is what an unset clock reads
    /// on most systems: it sits at the epoch, or before it. Such a clock
    /// yields no usable time, and because the pairing is made here and
    /// never remade, a correction arriving later would not reach the
    /// times already minted -- so it is refused rather than reported
    /// once and worked around.
    fn new() -> Result<Self, LogError> {
        // The real-time reading is taken first, so that the gap between
        // the two biases every later reading early by that gap rather
        // than late: a segment ID never names a moment after the file it
        // identifies was created.
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| LogError::ClockError)?;
        Self::anchor(since_epoch, Instant::now())
    }

    /// Pairs a real-time reading already taken with a monotonic one.
    ///
    /// Split out of [`new`](Self::new) because a test cannot set the
    /// system clock, and the reading this takes is the whole of what
    /// `new` decides on.
    ///
    /// * `since_epoch` -- how far the real-time clock reads past the
    ///   UNIX epoch.
    /// * `mono` -- the monotonic reading taken alongside it.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::ClockError`] if `since_epoch` is zero, the
    /// real-time clock reading at the epoch itself.
    fn anchor(since_epoch: Duration, mono: Instant) -> Result<Self, LogError> {
        // A u64 of nanoseconds runs to the year 2554, so saturation here
        // is unreachable in practice; it is written out rather than
        // asserted so that no clock reading can panic.
        let delta = u64::try_from(since_epoch.as_nanos()).unwrap_or(u64::MAX);
        if delta == 0 {
            return Err(LogError::ClockError);
        }
        Ok(Self { delta, mono })
    }

    /// The current time as nanoseconds since the UNIX epoch.
    ///
    /// Never decreases, and increases between any two calls far enough
    /// apart for the monotonic clock to have ticked.
    fn now(&self) -> Timestamp {
        // Saturating for the same reason as in `new`: the sum cannot
        // reach the end of a u64 of nanoseconds within any mission, and
        // reading the clock must not be able to panic.
        let elapsed = u64::try_from(self.mono.elapsed().as_nanos()).unwrap_or(u64::MAX);
        self.delta.saturating_add(elapsed)
    }

    /// The current time as a segment identifier.
    fn now_seg_id(&self) -> SegId {
        SegId::from_u64(self.now())
    }

    /// Shifts the epoch forward, if it has to, so that this clock reads
    /// past `id`.
    ///
    /// Anchoring keeps one writer's identifiers in order, but each
    /// writer anchors on its own reading of the real-time clock, so a
    /// backward step between two of them leaves the later writer behind
    /// the identifiers the earlier one minted. The only record of the
    /// earlier clock is those identifiers themselves, so a writer starts
    /// by stepping its epoch past the highest it finds.
    ///
    /// The epoch moves once, rather than each identifier being clamped
    /// to `id` + 1. Clamping would mint that same value over and over
    /// until real time caught up, and since a file of that name exists
    /// already, [`LogWrite::create_unique_file`] would retry for as long
    /// as that took.
    ///
    /// The cost is accuracy: past a backward step, this clock reads ahead
    /// of real time by the size of the step for the life of the writer.
    /// Ordering is what a reader depends on and accuracy is not, so that
    /// is the direction to err in, but it is a trade and not a free win.
    ///
    /// * `id` -- the highest identifier the log's directory already
    ///   holds.
    fn advance_past(&mut self, id: SegId) {
        let now = self.now();
        if id.as_u64() >= now {
            // Saturating for the same reason as elsewhere here: a clock
            // this cannot move past is one no mission will see, and
            // nothing about reading a clock may panic.
            self.delta = self.delta.saturating_add(id.as_u64() - now + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_refuses_an_unset_real_time_clock() {
        let err = Clock::anchor(Duration::ZERO, Instant::now());
        assert!(
            matches!(err, Err(LogError::ClockError)),
            "a clock reading the epoch itself was accepted"
        );
    }

    #[test]
    fn clock_accepts_a_set_real_time_clock() {
        let clock = Clock::anchor(Duration::from_nanos(1), Instant::now())
            .expect("one nanosecond past the epoch is a set clock");
        assert!(clock.now() >= 1);
    }

    #[test]
    fn clock_measures_from_the_real_time_epoch() {
        // The anchor is the floor of every later reading, and the gap
        // above it is time this test actually took.
        let delta = 1_700_000_000_000_000_000;
        let clock =
            Clock::anchor(Duration::from_nanos(delta), Instant::now()).expect("a set clock");
        let now = clock.now();
        assert!(now >= delta, "{now} is below the anchor {delta}");
        assert!(
            u128::from(now - delta) < Duration::from_secs(60).as_nanos(),
            "{} ns of drift from the anchor",
            now - delta
        );
    }

    #[test]
    fn clock_never_goes_backwards() {
        let clock = Clock::anchor(Duration::from_nanos(1), Instant::now()).expect("a set clock");
        let mut last = clock.now();
        for _ in 0..10_000 {
            let now = clock.now();
            assert!(now >= last, "{now} follows {last}");
            last = now;
        }
    }

    #[test]
    fn clock_advances_across_a_sleep() {
        let clock = Clock::anchor(Duration::from_nanos(1), Instant::now()).expect("a set clock");
        let before = clock.now();
        thread::sleep(Duration::from_millis(2));
        assert!(clock.now() > before);
    }

    #[test]
    fn advance_past_steps_over_an_identifier_from_a_later_clock() {
        let mut clock =
            Clock::anchor(Duration::from_micros(1), Instant::now()).expect("a set clock");
        let ahead = SegId::from_u64(5_000_000_000);
        clock.advance_past(ahead);
        assert!(
            clock.now() > ahead.as_u64(),
            "{} did not step past {ahead}",
            clock.now()
        );
    }

    #[test]
    fn advance_past_leaves_a_clock_already_ahead_where_it_was() {
        let delta = 1_700_000_000_000_000_000;
        let mut clock =
            Clock::anchor(Duration::from_nanos(delta), Instant::now()).expect("a set clock");
        clock.advance_past(SegId::from_u64(delta - 1_000_000_000));
        assert_eq!(
            clock.delta, delta,
            "the epoch moved for an older identifier"
        );
    }

    #[test]
    fn a_seeded_clock_still_advances_rather_than_sticking() {
        // The point of moving the epoch instead of clamping each
        // identifier: a clamped clock would mint one value until real
        // time caught up, and `create_unique_file` would spin on it.
        let mut clock =
            Clock::anchor(Duration::from_micros(1), Instant::now()).expect("a set clock");
        clock.advance_past(SegId::from_u64(5_000_000_000));
        let first = clock.now_seg_id();
        thread::sleep(Duration::from_millis(2));
        assert!(clock.now_seg_id() > first);
    }

    #[test]
    fn segment_ids_follow_the_clock() {
        let clock = Clock::anchor(Duration::from_nanos(1), Instant::now()).expect("a set clock");
        let first = clock.now_seg_id();
        thread::sleep(Duration::from_millis(2));
        assert!(clock.now_seg_id() > first);
    }
}
