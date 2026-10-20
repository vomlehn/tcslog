//! A C ABI over [`LogWrite`] and [`LogRead`].
//!
//! Every function here returns a [`TcslogStatus`], and every value a
//! caller wants back is written through an out-parameter. That is the
//! shape a C caller can check: there is no value a status code could be
//! confused with, and nothing is reported by a sentinel that also looks
//! like data.
//!
//! The status codes are ABI. Once a C program has been compiled against
//! this header its numbers are fixed, so a code's value never changes
//! and a new one is only ever added at the end.
//!
//! # Panics do not cross the boundary
//!
//! Unwinding out of an `extern "C"` function is undefined behaviour, so
//! every entry point catches a panic and reports
//! [`TcslogStatus::Panic`] instead. A caller that sees it should treat
//! the handle as unusable: the panic happened part way through an
//! operation and nothing here can say how far.
//!
//! # Callbacks carry a context
//!
//! [`TcslogCallbacks`] is passed to [`tcslog_write_open`] and holds the
//! three function pointers along with a `void *ctx` handed back to each
//! of them. The writer keeps its own copy, so two logs in one process
//! can have different callbacks and different contexts -- which is
//! what a C caller needs and what a process-wide set of function
//! pointers could not express.
//!
//! The structure is copied, so it need not outlive the call. The `ctx`
//! it holds is used until the writer is closed, and the library does
//! not own it, so that must outlive the writer.
//!
//! # Allocation
//!
//! Writing, reading, flushing, and the queries allocate nothing, as in
//! the library: a record is read into the caller's buffer, and the path
//! handed to `send` is built in a buffer the writer reserved when it
//! was opened. What allocates is confined to the calls a caller makes
//! once, or asks for explicitly:
//!
//! * [`tcslog_write_open`] and [`tcslog_read_open`] allocate the handle
//!   itself, and the library allocates the buffers it then reuses and
//!   the directory listing it opens from. The handle is opaque, and
//!   its size depends on the Rust standard library it was built with,
//!   so a C/C++ caller could not reserve storage for it; it is boxed
//!   here and freed by the matching close.
//! * [`tcslog_write_clear`] enumerates the directory, which no platform
//!   offers without allocating.
//! * A writer's I/O error from the operating system may carry an
//!   allocated description. A callback's failure does not.

use std::ffi::{c_char, c_int, c_void, CStr};
use std::fmt::{self, Write as _};
use std::fs::File;
use std::io;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

use tcslog::{
    write_timestamp, Format, LogError, LogRead, LogWrite, Meta, RecSize, SegId, SegmentHeader,
    WriteCallbacks, SEGMENT_FILE_HEADER_LEN, TIMESTAMP_STR_LEN, VERSION_MAJOR, VERSION_MINOR,
    VERSION_PATCH,
};

/// What a call reported.
///
/// `Ok` is zero and every failure is positive, so `if (status)` reads
/// as "something happened". The values are ABI: see the module
/// documentation.
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TcslogStatus {
    /// The call did what was asked.
    Ok = 0,

    /// The log has no more records. Not a failure: it is how a read
    /// loop ends.
    Eof = 1,

    /// Writing stopped and started again here, so record numbering
    /// restarts. Read again to continue with the next session.
    SessionEnd = 2,

    /// Telemetry was lost. `n` in the result holds the bytes of a
    /// cut-short record that were recovered, and `lost` the number of
    /// segment files missing. Read again to continue.
    ReadTruncated = 3,

    /// The record was larger than the buffer offered. `n` in the result
    /// holds how many bytes reached the front of the buffer, and they
    /// are real telemetry. The rest of the record was skipped, so the
    /// next read starts at the record after it rather than at this one:
    /// a buffer this read overflowed is too small for this log, and
    /// reading again does not recover what was dropped.
    ReadOverflow = 4,

    /// The real-time clock does not read later than the UNIX epoch,
    /// which is what an unset clock reads on most systems.
    ClockError = 5,

    /// A `Fixed` log was given a payload that is not exactly its record
    /// length, or a record length of zero.
    FixedLenMismatch = 6,

    /// A segment file's header could not be read.
    InvalidHeader = 7,

    /// The directory, prefix, or suffix cannot name a log.
    InvalidPathname = 8,

    /// The operating system reported an error.
    IoError = 9,

    /// The directory holds no segment file of this log.
    NoSegmentFiles = 10,

    /// A prefix or suffix contains the path separator.
    PathDelimiterNotAllowed = 11,

    /// The payload is larger than a record can hold.
    PayloadTooLarge = 12,

    /// `seg_size_max` leaves no room for a data record.
    SegSizeTooSmall = 13,

    /// The library was built without `TIMER_RESOLUTION`, so a writer
    /// cannot name segment files. See the library's Setup documentation.
    TimerResolutionZero = 14,

    /// A segment file was written by a version this build cannot read.
    VersionMismatch = 15,

    /// A pointer argument that must not be null was null.
    NullArgument = 16,

    /// A string argument is not valid UTF-8, which a log's directory,
    /// prefix, and suffix must be.
    NotUtf8 = 17,

    /// A panic was caught at the boundary rather than let unwind into C.
    /// The handle it happened on is no longer usable.
    Panic = 18,

    /// `format_tag` is not one of the three formats.
    InvalidFormat = 19,

    /// A buffer offered for text is too short for it and its NUL.
    /// Nothing was written to it.
    BufferTooSmall = 20,
}

/// The record layouts a log may use, as `format_tag` values for
/// [`tcslog_write_open`].
///
/// These are the library's own tags, so a log written by a Rust caller
/// and one written through this binding agree on them. The argument is
/// a plain `uint32_t` rather than this type: a value outside the three
/// would be an invalid enum, and refusing it as
/// [`TcslogStatus::InvalidFormat`] is better than the undefined
/// behaviour of constructing one.
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TcslogFormat {
    /// Every record holds exactly `fixed_len` payload bytes and carries
    /// no data header at all. The most compact, at the price of a
    /// length fixed for the life of the log.
    Fixed = 0,

    /// Records vary in length, with a four-byte data header giving it.
    VariableSimple = 1,

    /// As `VariableSimple`, and the data header also carries the time
    /// the record was written and its position in the session.
    VariableTsRc = 2,
}

/// Which metadata a record carried, mirroring [`Meta`].
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TcslogMeta {
    /// Fixed-length records, which carry no metadata.
    Fixed = 0,

    /// Variable-length, carrying no metadata beyond the length.
    VariableSimple = 1,

    /// Variable-length, carrying the time written and the position in
    /// the session. `timestamp` and `record_count` in the result hold
    /// them; with any other value those two are zero.
    VariableTsRc = 2,
}

/// What a read produced.
///
/// Which fields mean anything depends on the status the read returned,
/// and each field says which.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TcslogReadResult {
    /// Payload bytes placed in the caller's buffer, which are real
    /// telemetry whichever status came with them: the whole record on
    /// [`TcslogStatus::Ok`], the bytes recovered of a record cut short
    /// on [`TcslogStatus::ReadTruncated`], and the front of a record too
    /// large for the buffer on [`TcslogStatus::ReadOverflow`].
    pub n: u32,

    /// Which of the three metadata shapes the record had. Filled
    /// whenever `n` is, so the front of a record that came with
    /// [`TcslogStatus::ReadTruncated`] or [`TcslogStatus::ReadOverflow`]
    /// is described as a whole one is. A record whose data header was
    /// lost reports [`TcslogMeta::VariableSimple`].
    pub meta: TcslogMeta,

    /// Nanoseconds since the UNIX epoch, when `meta` is
    /// [`TcslogMeta::VariableTsRc`]. Zero otherwise.
    pub timestamp: u64,

    /// Position of the record within its session, counting from one,
    /// when `meta` is [`TcslogMeta::VariableTsRc`]. Zero otherwise.
    pub record_count: u64,

    /// Segment files found missing, on [`TcslogStatus::ReadTruncated`].
    /// Zero otherwise -- including where a record was cut short with no
    /// file missing at all, which is a truncation the sequence cannot
    /// explain.
    pub lost: u64,
}

impl TcslogReadResult {
    /// A result holding nothing, which every call fills from.
    fn empty() -> Self {
        Self {
            n: 0,
            meta: TcslogMeta::Fixed,
            timestamp: 0,
            record_count: 0,
            lost: 0,
        }
    }
}

/// A record's metadata, mirroring [`Meta`]: what
/// [`tcslog_write_last_meta`] and [`tcslog_read_last_meta`] report.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TcslogRecordMeta {
    /// Which of the three metadata shapes the record had.
    pub meta: TcslogMeta,

    /// Nanoseconds since the UNIX epoch, when `meta` is
    /// [`TcslogMeta::VariableTsRc`]. Zero otherwise.
    pub timestamp: u64,

    /// Position of the record within its session, counting from one,
    /// when `meta` is [`TcslogMeta::VariableTsRc`]. Zero otherwise.
    pub record_count: u64,
}

impl From<Meta> for TcslogRecordMeta {
    fn from(meta: Meta) -> Self {
        let mut out = TcslogReadResult::empty();
        set_meta(&mut out, meta);
        Self {
            meta: out.meta,
            timestamp: out.timestamp,
            record_count: out.record_count,
        }
    }
}

/// The header that opens a segment file, mirroring [`SegmentHeader`]:
/// what [`tcslog_read_current_header`] and
/// [`tcslog_read_take_opened_headers`] report.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TcslogSegmentHeader {
    /// Identifier of this segment file, which [`tcslog_seg_id_format`]
    /// renders as it appears in the file's name.
    pub segment_id: u64,

    /// Identifier of the first segment file of this session.
    pub session_id: u64,

    /// Bytes at the start of the data section that are the tail of a
    /// record begun in an earlier segment file, its data header counted.
    pub remaining: u64,

    /// Zero-based position of this segment file within its session.
    pub sequence: u64,

    /// The maximum size, in bytes, of a segment file of this log.
    pub max_size: u32,

    /// How records in this file are laid out, as a [`TcslogFormat`]
    /// value.
    pub format_tag: u32,

    /// The record length when `format_tag` is
    /// [`TcslogFormat::Fixed`]. Zero otherwise.
    pub fixed_len: u32,
}

impl From<&SegmentHeader> for TcslogSegmentHeader {
    fn from(h: &SegmentHeader) -> Self {
        Self {
            segment_id: h.segment_id.as_u64(),
            session_id: h.session_id.as_u64(),
            remaining: h.remaining,
            sequence: h.sequence.as_u64(),
            max_size: h.max_size,
            format_tag: u32::from(h.format.tag()),
            fixed_len: h.format.fixed_len(),
        }
    }
}

/// The characters a segment identifier renders as, without the NUL.
pub const TCSLOG_SEG_ID_STR_LEN: usize = 19;

/// The characters a timestamp renders as, without the NUL.
pub const TCSLOG_TIMESTAMP_STR_LEN: usize = 30;

// Literals above because cbindgen can only emit a literal into the
// header; these keep them from drifting from the library's own.
const _: () = assert!(TCSLOG_SEG_ID_STR_LEN == SegId::STR_LEN);
const _: () = assert!(TCSLOG_TIMESTAMP_STR_LEN == TIMESTAMP_STR_LEN);

/// A writer. Opaque to C: made by [`tcslog_write_open`] and released by
/// [`tcslog_write_close`].
pub struct TcslogWrite {
    inner: LogWrite<CHandler>,
}

/// A reader. Opaque to C: made by [`tcslog_read_open`] and released by
/// [`tcslog_read_close`].
pub struct TcslogRead {
    inner: LogRead,
}

/// Called with the path of a segment file the library is handing over.
/// Returning non-zero makes the write that triggered it report
/// [`TcslogStatus::IoError`].
pub type TcslogSendFn = extern "C" fn(path: *const c_char) -> c_int;

/// Called with the file descriptor of the segment file a record ended
/// in. Returning non-zero makes the write report
/// [`TcslogStatus::IoError`].
pub type TcslogRecordCompleteFn = extern "C" fn(fd: c_int) -> c_int;

/// Called with the widened timer resolution, in nanoseconds, when the
/// build-time value turned out to be too small for this machine. The
/// value passed is the figure to build with next time.
pub type TcslogTimerResolutionAdjustedFn = extern "C" fn(resolution_ns: u64);

/// The callbacks a writer is given, and the context it hands back to
/// each of them.
///
/// Any of the three function pointers may be null, which is that
/// callback unset. `ctx` is passed to each one and is never examined
/// here: it may be null, a pointer to anything, or an integer cast to a
/// pointer. The library does not own it, so it must outlive the writer.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TcslogCallbacks {
    /// Called with the path of a segment file being handed over.
    /// Returning non-zero makes the write that triggered it report
    /// [`TcslogStatus::IoError`].
    pub send: Option<extern "C" fn(ctx: *mut c_void, path: *const c_char) -> c_int>,

    /// Called with the descriptor of the segment file a record ended
    /// in, which the library still owns: do not close it. On a platform
    /// other than Unix, where a file is not a descriptor, it is -1.
    /// Returning non-zero makes the write report
    /// [`TcslogStatus::IoError`].
    pub record_complete: Option<extern "C" fn(ctx: *mut c_void, fd: c_int) -> c_int>,

    /// Called with the widened timer resolution, in nanoseconds, when
    /// the build-time value turned out too small for this machine.
    pub timer_resolution_adjusted: Option<extern "C" fn(ctx: *mut c_void, resolution_ns: u64)>,

    /// Handed to each callback above, untouched.
    pub ctx: *mut c_void,
}

impl TcslogCallbacks {
    /// No callbacks at all, which is what a null `cb` argument means.
    fn none() -> Self {
        Self {
            send: None,
            record_complete: None,
            timer_resolution_adjusted: None,
            ctx: std::ptr::null_mut(),
        }
    }
}

/// Routes the library's calls to the C function pointers a writer was
/// given, with that writer's own context.
///
/// Each writer owns one of these, so two logs in one process can have
/// different callbacks and different contexts -- which is what the
/// context is for, and what three process-wide function pointers could
/// not express.
struct CHandler {
    cb: TcslogCallbacks,
    /// The NUL-terminated path handed to `send`, rebuilt in place for
    /// each segment file. Reserved at open for the longest path the log
    /// can name, so handing a file over allocates nothing.
    path_buf: Vec<u8>,
}

impl CHandler {
    fn new(cb: TcslogCallbacks, dir: &str, prefix: &str, suffix: &str) -> Self {
        // The directory, a separator, the prefix, a segment identifier,
        // the suffix, and the NUL.
        let longest = dir.len() + 1 + prefix.len() + SegId::STR_LEN + suffix.len() + 1;
        Self {
            cb,
            path_buf: Vec::with_capacity(longest),
        }
    }
}

/// The descriptor `record_complete` is handed for `file`.
///
/// On Unix that is the file's own descriptor, which is what C code there
/// works with. Elsewhere a file is a handle that does not fit the `int`
/// the ABI promises, so the callback is handed -1, and learns only that
/// a record ended; widening the argument would break every caller built
/// against the header.
#[cfg(unix)]
fn raw_fd(file: &File) -> c_int {
    use std::os::fd::AsRawFd;
    file.as_raw_fd()
}

/// See the Unix version above.
#[cfg(not(unix))]
fn raw_fd(_file: &File) -> c_int {
    -1
}

/// The error a callback's non-zero return becomes. A bare kind, rather
/// than one carrying a message, so that it allocates nothing: the C
/// caller sees only [`TcslogStatus::IoError`] either way.
fn callback_failed() -> io::Error {
    io::ErrorKind::Other.into()
}

impl WriteCallbacks for CHandler {
    fn record_complete(&mut self, file: &mut File) -> io::Result<()> {
        let Some(f) = self.cb.record_complete else {
            return Ok(());
        };
        if f(self.cb.ctx, raw_fd(file)) == 0 {
            Ok(())
        } else {
            Err(callback_failed())
        }
    }

    /// A path that is not valid UTF-8, or that holds a NUL, cannot be
    /// made into a C string without inventing bytes, so it is refused
    /// rather than passed on. Neither is reachable through this
    /// binding: a log's directory, prefix and suffix all arrived as C
    /// strings and were checked to be UTF-8, and the identifier between
    /// them is hexadecimal.
    ///
    /// The path is copied into a buffer reserved at open, so this
    /// allocates only if the library ever hands over a longer path than
    /// the log can name.
    fn send(&mut self, path: &Path) -> io::Result<()> {
        let Some(f) = self.cb.send else {
            return Ok(());
        };
        let Some(text) = path.to_str() else {
            return Err(io::ErrorKind::InvalidData.into());
        };
        let bytes = text.as_bytes();
        if bytes.contains(&0) {
            return Err(io::ErrorKind::InvalidData.into());
        }
        self.path_buf.clear();
        self.path_buf.extend_from_slice(bytes);
        self.path_buf.push(0);
        if f(self.cb.ctx, self.path_buf.as_ptr().cast::<c_char>()) == 0 {
            Ok(())
        } else {
            Err(callback_failed())
        }
    }

    fn timer_resolution_adjusted(&mut self, resolution_ns: u64) {
        if let Some(f) = self.cb.timer_resolution_adjusted {
            f(self.cb.ctx, resolution_ns);
        }
    }
}

/// Maps a library error onto a status, filling in what the error
/// carried.
fn status_of(e: &LogError, out: &mut TcslogReadResult) -> TcslogStatus {
    match e {
        LogError::ClockError => TcslogStatus::ClockError,
        LogError::Eof => TcslogStatus::Eof,
        LogError::FixedLenMismatch => TcslogStatus::FixedLenMismatch,
        LogError::InvalidHeader => TcslogStatus::InvalidHeader,
        LogError::InvalidPathname => TcslogStatus::InvalidPathname,
        LogError::IoError(_) => TcslogStatus::IoError,
        LogError::NoSegmentFiles => TcslogStatus::NoSegmentFiles,
        LogError::PathDelimiterNotAllowed => TcslogStatus::PathDelimiterNotAllowed,
        LogError::PayloadTooLarge => TcslogStatus::PayloadTooLarge,
        LogError::ReadOverflow(need) => {
            out.n = *need;
            TcslogStatus::ReadOverflow
        }
        LogError::ReadTruncated { lost, n } => {
            out.lost = *lost;
            out.n = *n;
            TcslogStatus::ReadTruncated
        }
        LogError::SegSizeTooSmall => TcslogStatus::SegSizeTooSmall,
        LogError::SessionEnd => TcslogStatus::SessionEnd,
        LogError::TimerResolutionZero => TcslogStatus::TimerResolutionZero,
        LogError::VersionMismatch => TcslogStatus::VersionMismatch,
    }
}

/// Fills the metadata fields of a read result.
fn set_meta(out: &mut TcslogReadResult, meta: Meta) {
    match meta {
        Meta::Fixed => out.meta = TcslogMeta::Fixed,
        Meta::VariableSimple => out.meta = TcslogMeta::VariableSimple,
        Meta::VariableTsRc(ts, rc) => {
            out.meta = TcslogMeta::VariableTsRc;
            out.timestamp = ts;
            out.record_count = rc;
        }
    }
}

/// A library error where there is no result to fill, which is every
/// call but a read.
fn status_only(e: &LogError) -> TcslogStatus {
    status_of(e, &mut TcslogReadResult::empty())
}

/// Borrows a C string as `&str`.
///
/// # Safety
///
/// `p` must be null or a pointer to a NUL-terminated string that stays
/// put for the length of the call.
unsafe fn as_str<'a>(p: *const c_char) -> Result<&'a str, TcslogStatus> {
    if p.is_null() {
        return Err(TcslogStatus::NullArgument);
    }
    // SAFETY: the caller guarantees a NUL-terminated string.
    unsafe { CStr::from_ptr(p) }
        .to_str()
        .map_err(|_| TcslogStatus::NotUtf8)
}

/// Runs `f`, turning a panic into [`TcslogStatus::Panic`] rather than
/// letting it unwind into C.
fn guard(f: impl FnOnce() -> TcslogStatus) -> TcslogStatus {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(TcslogStatus::Panic)
}

/// Builds a [`Format`] from the tag and length a C caller passed.
///
/// The tags are the library's own: 0 fixed, 1 variable-simple, 2
/// variable-tsrc. `fixed_len` is read only for tag 0.
fn format_of(tag: u32, fixed_len: u32) -> Result<Format, TcslogStatus> {
    match tag {
        0 => Ok(Format::Fixed(fixed_len)),
        1 => Ok(Format::VariableSimple),
        2 => Ok(Format::VariableTsRc),
        _ => Err(TcslogStatus::InvalidFormat),
    }
}

/// A [`fmt::Write`] over a caller's buffer, which refuses rather than
/// truncates when the text does not fit, and allocates nothing.
struct SliceWriter<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl fmt::Write for SliceWriter<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.len + s.len();
        let dst = self.buf.get_mut(self.len..end).ok_or(fmt::Error)?;
        dst.copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

/// Renders `render`'s text into the caller's `cap` bytes at `buf` with
/// a NUL after it.
///
/// `len` is the length the text always has, so a buffer too short for
/// it and the NUL is refused before anything is written to it.
///
/// # Safety
///
/// `buf` must be null or point to `cap` writable bytes.
unsafe fn render_into(
    buf: *mut c_char,
    cap: usize,
    len: usize,
    render: impl FnOnce(&mut SliceWriter<'_>) -> fmt::Result,
) -> TcslogStatus {
    if buf.is_null() {
        return TcslogStatus::NullArgument;
    }
    if cap <= len {
        return TcslogStatus::BufferTooSmall;
    }
    // SAFETY: the caller guarantees `cap` writable bytes.
    let dst = unsafe { std::slice::from_raw_parts_mut(buf.cast::<u8>(), cap) };
    let mut w = SliceWriter { buf: dst, len: 0 };
    if render(&mut w).is_err() {
        return TcslogStatus::BufferTooSmall;
    }
    let end = w.len;
    dst[end] = 0;
    TcslogStatus::Ok
}

/// Writes the stored format version this build reads and writes.
///
/// A build reads a segment file whose major version matches this one
/// and whose minor version is no greater. Any of the three pointers may
/// be null.
#[no_mangle]
pub extern "C" fn tcslog_format_version(major: *mut u32, minor: *mut u32, patch: *mut u32) {
    for (p, v) in [
        (major, VERSION_MAJOR),
        (minor, VERSION_MINOR),
        (patch, VERSION_PATCH),
    ] {
        if !p.is_null() {
            // SAFETY: checked non-null, and the caller owns the
            // pointee for the length of the call.
            unsafe { *p = v };
        }
    }
}

/// The bytes a segment file's own header occupies.
///
/// `seg_size_max` must leave room for this and for a data header, so
/// this is the floor a caller computes from.
#[no_mangle]
pub extern "C" fn tcslog_segment_file_header_len() -> u32 {
    SEGMENT_FILE_HEADER_LEN
}

/// Borrows a NUL-terminated byte literal as a C string.
///
/// `c"..."` would say this in one token, but it needs Rust 1.77 and the
/// workspace declares 1.75. The validation is not wasted either: it
/// rejects a literal whose NUL was left off or that holds an interior
/// one, which is the mistake this form invites.
fn cstr(bytes: &'static [u8]) -> &'static CStr {
    CStr::from_bytes_with_nul(bytes).expect("a NUL-terminated literal with no interior NUL")
}

/// A short description of a status, as a static NUL-terminated string.
///
/// Never null, and never needs freeing. An unrecognized value gives
/// "unknown status" rather than nothing, so a caller that was compiled
/// against an older header still prints something.
#[no_mangle]
pub extern "C" fn tcslog_status_str(status: TcslogStatus) -> *const c_char {
    let s: &CStr = match status {
        TcslogStatus::Ok => cstr(b"ok\0"),
        TcslogStatus::Eof => cstr(b"end of log\0"),
        TcslogStatus::SessionEnd => cstr(b"end of session\0"),
        TcslogStatus::ReadTruncated => cstr(b"telemetry lost; record cut short\0"),
        TcslogStatus::ReadOverflow => {
            cstr(b"record larger than the buffer; its front was captured\0")
        }
        TcslogStatus::ClockError => cstr(b"real-time clock is not set\0"),
        TcslogStatus::FixedLenMismatch => cstr(b"payload is not the fixed record length\0"),
        TcslogStatus::InvalidHeader => cstr(b"segment file header could not be read\0"),
        TcslogStatus::InvalidPathname => cstr(b"directory, prefix, or suffix cannot name a log\0"),
        TcslogStatus::IoError => cstr(b"I/O error\0"),
        TcslogStatus::NoSegmentFiles => cstr(b"no segment files\0"),
        TcslogStatus::PathDelimiterNotAllowed => cstr(b"prefix or suffix holds a path separator\0"),
        TcslogStatus::PayloadTooLarge => cstr(b"payload too large for a record\0"),
        TcslogStatus::SegSizeTooSmall => cstr(b"segment size leaves no room for a record\0"),
        TcslogStatus::TimerResolutionZero => cstr(b"built without TIMER_RESOLUTION\0"),
        TcslogStatus::VersionMismatch => {
            cstr(b"segment file version is not readable by this build\0")
        }
        TcslogStatus::NullArgument => cstr(b"null argument\0"),
        TcslogStatus::NotUtf8 => cstr(b"argument is not valid UTF-8\0"),
        TcslogStatus::Panic => cstr(b"panic caught at the boundary\0"),
        TcslogStatus::InvalidFormat => cstr(b"format tag is not one of the three formats\0"),
        TcslogStatus::BufferTooSmall => cstr(b"buffer too small for the text\0"),
    };
    s.as_ptr()
}

/// Renders a segment identifier as it appears in a segment file's name:
/// four dash-separated groups of four lower-case hexadecimal digits.
///
/// `buf` must hold at least [`TCSLOG_SEG_ID_STR_LEN`] + 1 bytes, for the
/// text and its NUL; a shorter one is refused as
/// [`TcslogStatus::BufferTooSmall`] and left untouched.
///
/// # Safety
///
/// `buf` must point to `cap` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn tcslog_seg_id_format(
    id: u64,
    buf: *mut c_char,
    cap: usize,
) -> TcslogStatus {
    guard(|| {
        // SAFETY: the caller guarantees `cap` writable bytes.
        unsafe {
            render_into(buf, cap, TCSLOG_SEG_ID_STR_LEN, |w| {
                write!(w, "{}", SegId::from_u64(id))
            })
        }
    })
}

/// Parses a segment identifier in the form [`tcslog_seg_id_format`]
/// renders.
///
/// Text in any other form is refused as
/// [`TcslogStatus::InvalidPathname`], as the library refuses it, since
/// it names no segment file.
///
/// # Safety
///
/// `text` must be a NUL-terminated string, and `out` must point to
/// writable storage for one `uint64_t`.
#[no_mangle]
pub unsafe extern "C" fn tcslog_seg_id_parse(text: *const c_char, out: *mut u64) -> TcslogStatus {
    guard(|| {
        if out.is_null() {
            return TcslogStatus::NullArgument;
        }
        // SAFETY: the caller guarantees a NUL-terminated string.
        let text = match unsafe { as_str(text) } {
            Ok(t) => t,
            Err(e) => return e,
        };
        match SegId::parse(text) {
            Ok(id) => {
                // SAFETY: checked non-null above.
                unsafe { *out = id.as_u64() };
                TcslogStatus::Ok
            }
            Err(e) => status_only(&e),
        }
    })
}

/// Renders a timestamp as an ISO 8601 UTC date and time with nanosecond
/// precision, for example `2026-09-21T16:45:12.123456789Z`.
///
/// `buf` must hold at least [`TCSLOG_TIMESTAMP_STR_LEN`] + 1 bytes, for
/// the text and its NUL; a shorter one is refused as
/// [`TcslogStatus::BufferTooSmall`] and left untouched.
///
/// # Safety
///
/// `buf` must point to `cap` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn tcslog_format_timestamp(
    ts: u64,
    buf: *mut c_char,
    cap: usize,
) -> TcslogStatus {
    guard(|| {
        // SAFETY: the caller guarantees `cap` writable bytes.
        unsafe {
            render_into(buf, cap, TCSLOG_TIMESTAMP_STR_LEN, |w| {
                write_timestamp(w, ts)
            })
        }
    })
}

/// Opens a writer on the log in `dir` whose segment files are named
/// `prefix` + identifier + `suffix`.
///
/// `seg_size_max` is the most bytes a segment file may occupy, its own
/// header included. `format_tag` is a [`TcslogFormat`] value, and
/// `fixed_len` is the record length, read only for
/// [`TcslogFormat::Fixed`]. A tag outside the three is refused as
/// [`TcslogStatus::InvalidFormat`].
///
/// `cb` is the callbacks this writer is to use and the context to hand
/// them, and may be null for none. It is copied, so the structure
/// itself need not outlive the call -- but the `ctx` it holds is used
/// until the writer is closed, so that must outlive the writer. Each
/// writer has its own, so two logs in one process can have different
/// callbacks and different contexts.
///
/// On success `*out` holds a writer to pass to
/// [`tcslog_write_close`]. On failure `*out` is left null.
///
/// # Safety
///
/// The three strings must be NUL-terminated, `cb` must be null or point
/// to a readable [`TcslogCallbacks`], and `out` must point to writable
/// storage for one pointer.
#[no_mangle]
pub unsafe extern "C" fn tcslog_write_open(
    dir: *const c_char,
    prefix: *const c_char,
    suffix: *const c_char,
    seg_size_max: u32,
    format_tag: u32,
    fixed_len: u32,
    cb: *const TcslogCallbacks,
    out: *mut *mut TcslogWrite,
) -> TcslogStatus {
    guard(|| {
        if out.is_null() {
            return TcslogStatus::NullArgument;
        }
        // SAFETY: checked non-null just above.
        unsafe { *out = std::ptr::null_mut() };

        // SAFETY: the caller guarantees NUL-terminated strings.
        let (dir, prefix, suffix) = unsafe {
            match (as_str(dir), as_str(prefix), as_str(suffix)) {
                (Ok(d), Ok(p), Ok(s)) => (d, p, s),
                (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => return e,
            }
        };

        let format = match format_of(format_tag, fixed_len) {
            Ok(f) => f,
            Err(e) => return e,
        };

        // Copied rather than borrowed: the caller's structure is
        // theirs to reuse or discard the moment this returns, where the
        // ctx inside it has to last as long as the writer.
        let cb = if cb.is_null() {
            TcslogCallbacks::none()
        } else {
            // SAFETY: the caller guarantees a readable structure.
            unsafe { *cb }
        };

        match LogWrite::new(
            dir,
            prefix,
            suffix,
            seg_size_max,
            format,
            CHandler::new(cb, dir, prefix, suffix),
        ) {
            Ok(inner) => {
                let handle = Box::new(TcslogWrite { inner });
                // SAFETY: `out` was checked non-null above.
                unsafe { *out = Box::into_raw(handle) };
                TcslogStatus::Ok
            }
            Err(e) => status_only(&e),
        }
    })
}

/// Writes one record, `len` bytes from `data`.
///
/// `*written`, when `written` is not null, is left holding the bytes
/// the record occupied in the log, its data header included.
///
/// # Safety
///
/// `h` must come from [`tcslog_write_open`] and not yet be closed, and
/// `data` must point to `len` readable bytes. `len` of zero is allowed
/// and `data` may then be null.
#[no_mangle]
pub unsafe extern "C" fn tcslog_write_record(
    h: *mut TcslogWrite,
    data: *const u8,
    len: usize,
    written: *mut u32,
) -> TcslogStatus {
    guard(|| {
        if h.is_null() || (data.is_null() && len != 0) {
            return TcslogStatus::NullArgument;
        }
        if RecSize::try_from(len).is_err() {
            return TcslogStatus::PayloadTooLarge;
        }
        // SAFETY: the caller guarantees a live handle and `len`
        // readable bytes; a zero length needs no pointer, and
        // `from_raw_parts` will not accept null, so it is given a
        // dangling-but-aligned pointer instead.
        let (log, msg) = unsafe {
            (
                &mut (*h).inner,
                if len == 0 {
                    &[][..]
                } else {
                    std::slice::from_raw_parts(data, len)
                },
            )
        };
        match log.write(msg) {
            Ok(n) => {
                if !written.is_null() {
                    // SAFETY: checked non-null.
                    unsafe { *written = n };
                }
                TcslogStatus::Ok
            }
            Err(e) => status_only(&e),
        }
    })
}

/// Flushes the segment file now being written.
///
/// # Safety
///
/// `h` must come from [`tcslog_write_open`] and not yet be closed.
#[no_mangle]
pub unsafe extern "C" fn tcslog_write_flush(h: *mut TcslogWrite) -> TcslogStatus {
    guard(|| {
        if h.is_null() {
            return TcslogStatus::NullArgument;
        }
        // SAFETY: the caller guarantees a live handle.
        match unsafe { (*h).inner.flush() } {
            Ok(()) => TcslogStatus::Ok,
            Err(e) => status_only(&e),
        }
    })
}

/// Removes every segment file of this log, leaving it empty.
///
/// # Safety
///
/// `h` must come from [`tcslog_write_open`] and not yet be closed.
#[no_mangle]
pub unsafe extern "C" fn tcslog_write_clear(h: *mut TcslogWrite) -> TcslogStatus {
    guard(|| {
        if h.is_null() {
            return TcslogStatus::NullArgument;
        }
        // SAFETY: the caller guarantees a live handle.
        match unsafe { (*h).inner.clear() } {
            Ok(()) => TcslogStatus::Ok,
            Err(e) => status_only(&e),
        }
    })
}

/// Writes the timer resolution this writer is working with, in
/// nanoseconds.
///
/// This is the build-time `TIMER_RESOLUTION` unless the writer found it
/// too small and widened it, in which case it is the figure to build
/// with next time.
///
/// # Safety
///
/// `h` must come from [`tcslog_write_open`] and not yet be closed, and
/// `out` must point to writable storage for one `uint64_t`.
#[no_mangle]
pub unsafe extern "C" fn tcslog_write_timer_resolution(
    h: *mut TcslogWrite,
    out: *mut u64,
) -> TcslogStatus {
    guard(|| {
        if h.is_null() || out.is_null() {
            return TcslogStatus::NullArgument;
        }
        // SAFETY: the caller guarantees a live handle and writable
        // storage.
        unsafe { *out = (*h).inner.timer_resolution() };
        TcslogStatus::Ok
    })
}

/// Writes the identifier of the session being written, which is the
/// identifier of its first segment file.
///
/// After [`tcslog_write_clear`] this still names the cleared session
/// until the next write starts one.
///
/// # Safety
///
/// `h` must come from [`tcslog_write_open`] and not yet be closed, and
/// `out` must point to writable storage for one `uint64_t`.
#[no_mangle]
pub unsafe extern "C" fn tcslog_write_session_id(
    h: *mut TcslogWrite,
    out: *mut u64,
) -> TcslogStatus {
    guard(|| {
        if h.is_null() || out.is_null() {
            return TcslogStatus::NullArgument;
        }
        // SAFETY: the caller guarantees a live handle and writable
        // storage.
        unsafe { *out = (*h).inner.session_id().as_u64() };
        TcslogStatus::Ok
    })
}

/// Writes the identifier of the segment file being written.
///
/// # Safety
///
/// `h` must come from [`tcslog_write_open`] and not yet be closed, and
/// `out` must point to writable storage for one `uint64_t`.
#[no_mangle]
pub unsafe extern "C" fn tcslog_write_current_segment_id(
    h: *mut TcslogWrite,
    out: *mut u64,
) -> TcslogStatus {
    guard(|| {
        if h.is_null() || out.is_null() {
            return TcslogStatus::NullArgument;
        }
        // SAFETY: the caller guarantees a live handle and writable
        // storage.
        unsafe { *out = (*h).inner.current_segment_id().as_u64() };
        TcslogStatus::Ok
    })
}

/// Writes the metadata minted for the most recently written record,
/// which is how a caller learns the timestamp and record count a
/// variable-tsrc record was stamped with.
///
/// # Safety
///
/// `h` must come from [`tcslog_write_open`] and not yet be closed, and
/// `out` must point to writable storage for one [`TcslogRecordMeta`].
#[no_mangle]
pub unsafe extern "C" fn tcslog_write_last_meta(
    h: *mut TcslogWrite,
    out: *mut TcslogRecordMeta,
) -> TcslogStatus {
    guard(|| {
        if h.is_null() || out.is_null() {
            return TcslogStatus::NullArgument;
        }
        // SAFETY: the caller guarantees a live handle and writable
        // storage.
        unsafe { *out = (*h).inner.last_meta().into() };
        TcslogStatus::Ok
    })
}

/// Closes a writer and releases it. Null is accepted and does nothing,
/// as `free` does.
///
/// The segment file being written is flushed and, if it holds any
/// records at all, handed to `send` -- so the records written last are
/// not stranded in a file the caller was never told about. That file is
/// short, unlike every other file `send` is given, which is the one
/// place a `send` that cares about size will see one.
///
/// A close cannot report a failure, so an error from that flush or from
/// `send` is discarded. A caller that needs to know the last records
/// reached storage calls [`tcslog_write_flush`] first, which does
/// report.
///
/// # Safety
///
/// `h` must come from [`tcslog_write_open`] and must not be used again
/// after this returns.
#[no_mangle]
pub unsafe extern "C" fn tcslog_write_close(h: *mut TcslogWrite) {
    if h.is_null() {
        return;
    }
    // The drop is what flushes and closes the open segment file, and a
    // panic in it must not unwind into C.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the caller guarantees a handle from
        // `tcslog_write_open` that has not been closed, so this is the
        // one box made for it.
        drop(unsafe { Box::from_raw(h) });
    }));
}

/// Opens a reader on the log in `dir` whose segment files are named
/// `prefix` + identifier + `suffix`.
///
/// On success `*out` holds a reader to pass to [`tcslog_read_close`].
/// On failure `*out` is left null.
///
/// # Safety
///
/// The three strings must be NUL-terminated, and `out` must point to
/// writable storage for one pointer.
#[no_mangle]
pub unsafe extern "C" fn tcslog_read_open(
    dir: *const c_char,
    prefix: *const c_char,
    suffix: *const c_char,
    out: *mut *mut TcslogRead,
) -> TcslogStatus {
    guard(|| {
        if out.is_null() {
            return TcslogStatus::NullArgument;
        }
        // SAFETY: checked non-null just above.
        unsafe { *out = std::ptr::null_mut() };

        // SAFETY: the caller guarantees NUL-terminated strings.
        let (dir, prefix, suffix) = unsafe {
            match (as_str(dir), as_str(prefix), as_str(suffix)) {
                (Ok(d), Ok(p), Ok(s)) => (d, p, s),
                (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => return e,
            }
        };

        match LogRead::new(dir, prefix, suffix) {
            Ok(inner) => {
                let handle = Box::new(TcslogRead { inner });
                // SAFETY: `out` was checked non-null above.
                unsafe { *out = Box::into_raw(handle) };
                TcslogStatus::Ok
            }
            Err(e) => status_only(&e),
        }
    })
}

/// Reads the next record into `buf`, which holds `cap` bytes.
///
/// `*result` is filled whatever the status, so the fields a status
/// describes can be read without checking for null first. The statuses
/// that are news about the telemetry rather than a failure --
/// [`TcslogStatus::SessionEnd`], [`TcslogStatus::ReadTruncated`] and
/// [`TcslogStatus::ReadOverflow`] -- are followed by reading again; the
/// loop ends at [`TcslogStatus::Eof`].
///
/// # Safety
///
/// `h` must come from [`tcslog_read_open`] and not yet be closed, `buf`
/// must point to `cap` writable bytes, and `result` must point to
/// writable storage for one [`TcslogReadResult`].
#[no_mangle]
pub unsafe extern "C" fn tcslog_read_record(
    h: *mut TcslogRead,
    buf: *mut u8,
    cap: usize,
    result: *mut TcslogReadResult,
) -> TcslogStatus {
    guard(|| {
        if h.is_null() || result.is_null() || (buf.is_null() && cap != 0) {
            return TcslogStatus::NullArgument;
        }
        let mut out = TcslogReadResult::empty();
        // SAFETY: the caller guarantees a live handle and `cap`
        // writable bytes; as in `tcslog_write_record`, a zero capacity
        // needs no pointer.
        let status = unsafe {
            let log = &mut (*h).inner;
            let dst = if cap == 0 {
                &mut [][..]
            } else {
                std::slice::from_raw_parts_mut(buf, cap)
            };
            match log.read(dst) {
                Ok(r) => {
                    out.n = r.n;
                    set_meta(&mut out, r.meta);
                    TcslogStatus::Ok
                }
                Err(e) => {
                    let status = status_of(&e, &mut out);
                    // The front of a record reached the buffer, so the
                    // caller is owed its metadata as on success.
                    if out.n > 0 {
                        set_meta(&mut out, log.last_meta());
                    }
                    status
                }
            }
        };
        // SAFETY: `result` was checked non-null above.
        unsafe { *result = out };
        status
    })
}

/// Writes the number of segment files this reader has opened.
///
/// # Safety
///
/// `h` must come from [`tcslog_read_open`] and not yet be closed, and
/// `out` must point to writable storage for one `uint64_t`.
#[no_mangle]
pub unsafe extern "C" fn tcslog_read_segments_opened(
    h: *mut TcslogRead,
    out: *mut u64,
) -> TcslogStatus {
    guard(|| {
        if h.is_null() || out.is_null() {
            return TcslogStatus::NullArgument;
        }
        // SAFETY: the caller guarantees a live handle and writable
        // storage.
        unsafe { *out = (*h).inner.segments_opened() };
        TcslogStatus::Ok
    })
}

/// Writes the metadata of the record most recently decoded, which is
/// what describes the bytes a [`TcslogStatus::ReadOverflow`] or a
/// [`TcslogStatus::ReadTruncated`] left in the buffer.
///
/// # Safety
///
/// `h` must come from [`tcslog_read_open`] and not yet be closed, and
/// `out` must point to writable storage for one [`TcslogRecordMeta`].
#[no_mangle]
pub unsafe extern "C" fn tcslog_read_last_meta(
    h: *mut TcslogRead,
    out: *mut TcslogRecordMeta,
) -> TcslogStatus {
    guard(|| {
        if h.is_null() || out.is_null() {
            return TcslogStatus::NullArgument;
        }
        // SAFETY: the caller guarantees a live handle and writable
        // storage.
        unsafe { *out = (*h).inner.last_meta().into() };
        TcslogStatus::Ok
    })
}

/// Writes the header of the segment file now open.
///
/// `*present` is set to 1 and `*out` filled when a segment file is
/// open, and `*present` set to 0 and `*out` left alone when none is:
/// before the first read, and after a read that returned
/// [`TcslogStatus::Eof`], [`TcslogStatus::SessionEnd`] or
/// [`TcslogStatus::ReadTruncated`].
///
/// # Safety
///
/// `h` must come from [`tcslog_read_open`] and not yet be closed, `out`
/// must point to writable storage for one [`TcslogSegmentHeader`], and
/// `present` to writable storage for one `int`.
#[no_mangle]
pub unsafe extern "C" fn tcslog_read_current_header(
    h: *mut TcslogRead,
    out: *mut TcslogSegmentHeader,
    present: *mut c_int,
) -> TcslogStatus {
    guard(|| {
        if h.is_null() || out.is_null() || present.is_null() {
            return TcslogStatus::NullArgument;
        }
        // SAFETY: the caller guarantees a live handle and writable
        // storage for both.
        unsafe {
            match (*h).inner.current_header() {
                Some(header) => {
                    *out = header.into();
                    *present = 1;
                }
                None => *present = 0,
            }
        }
        TcslogStatus::Ok
    })
}

/// Asks the reader to retain the header of every segment file it opens,
/// when `enable` is non-zero, or to stop.
///
/// Retention is off by default and must be: the headers accumulate,
/// allocating as they do, until taken by
/// [`tcslog_read_take_opened_headers`].
///
/// # Safety
///
/// `h` must come from [`tcslog_read_open`] and not yet be closed.
#[no_mangle]
pub unsafe extern "C" fn tcslog_read_collect_opened_headers(
    h: *mut TcslogRead,
    enable: c_int,
) -> TcslogStatus {
    guard(|| {
        if h.is_null() {
            return TcslogStatus::NullArgument;
        }
        // SAFETY: the caller guarantees a live handle.
        unsafe { (*h).inner.collect_opened_headers(enable != 0) };
        TcslogStatus::Ok
    })
}

/// Moves the oldest of the headers retained since the last take into
/// the `cap` entries at `out`, and writes how many to `*n`.
///
/// Those that do not fit stay retained, in order, so a caller drains
/// them by calling until `*n` comes back less than `cap`.
///
/// # Safety
///
/// `h` must come from [`tcslog_read_open`] and not yet be closed, `out`
/// must point to `cap` writable [`TcslogSegmentHeader`] entries, and
/// `n` to writable storage for one `size_t`. `out` may be null when
/// `cap` is zero.
#[no_mangle]
pub unsafe extern "C" fn tcslog_read_take_opened_headers(
    h: *mut TcslogRead,
    out: *mut TcslogSegmentHeader,
    cap: usize,
    n: *mut usize,
) -> TcslogStatus {
    guard(|| {
        if h.is_null() || n.is_null() || (out.is_null() && cap != 0) {
            return TcslogStatus::NullArgument;
        }
        // SAFETY: the caller guarantees a live handle.
        let log = unsafe { &mut (*h).inner };
        // Drained through a small array on the stack and converted, so
        // that taking them allocates nothing whatever `cap` is.
        let mut chunk = [SegmentHeader {
            segment_id: SegId::from_u64(0),
            session_id: SegId::from_u64(0),
            max_size: 0,
            remaining: 0,
            format: Format::VariableSimple,
            sequence: tcslog::SeqId::ZERO,
        }; 16];
        let mut done = 0;
        while done < cap {
            let want = (cap - done).min(chunk.len());
            let got = log.take_opened_headers_into(&mut chunk[..want]);
            for (i, header) in chunk[..got].iter().enumerate() {
                // SAFETY: `done + i` is below `cap`, and the caller
                // guarantees `cap` writable entries.
                unsafe { *out.add(done + i) = header.into() };
            }
            done += got;
            if got < want {
                break;
            }
        }
        // SAFETY: checked non-null above.
        unsafe { *n = done };
        TcslogStatus::Ok
    })
}

/// Closes a reader and releases it. Null is accepted and does nothing,
/// as `free` does.
///
/// # Safety
///
/// `h` must come from [`tcslog_read_open`] and must not be used again
/// after this returns.
#[no_mangle]
pub unsafe extern "C" fn tcslog_read_close(h: *mut TcslogRead) {
    if h.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the caller guarantees a handle from
        // `tcslog_read_open` that has not been closed.
        drop(unsafe { Box::from_raw(h) });
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    /// Counts into the context it is handed, which is a `u32` the test
    /// owns on its own stack.
    extern "C" fn count_send(ctx: *mut c_void, _path: *const c_char) -> c_int {
        if !ctx.is_null() {
            // SAFETY: the test passes a pointer to its own live `u32`
            // and the writer is closed before that goes out of scope.
            unsafe { *ctx.cast::<u32>() += 1 };
        }
        0
    }

    /// A directory that removes itself, so a test that opens a log
    /// leaves nothing behind.
    struct TmpDir(std::path::PathBuf);

    impl TmpDir {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!(
                "tcslog-c-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            std::fs::create_dir_all(&p).expect("a temporary directory");
            Self(p)
        }

        fn c_string(&self) -> CString {
            CString::new(self.0.to_str().expect("a UTF-8 path")).expect("a path without NUL")
        }
    }

    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_record_written_through_the_boundary_reads_back_through_it() {
        let dir = TmpDir::new("roundtrip");
        let d = dir.c_string();
        let prefix = CString::new("seg-").unwrap();
        let suffix = CString::new(".log").unwrap();

        // A context the callback counts into, which is the whole
        // point of carrying one: nothing here is a static.
        let mut sends = 0u32;
        let cb = TcslogCallbacks {
            send: Some(count_send),
            record_complete: None,
            timer_resolution_adjusted: None,
            ctx: std::ptr::addr_of_mut!(sends).cast::<c_void>(),
        };

        let mut w: *mut TcslogWrite = std::ptr::null_mut();
        let status = unsafe {
            tcslog_write_open(
                d.as_ptr(),
                prefix.as_ptr(),
                suffix.as_ptr(),
                SEGMENT_FILE_HEADER_LEN + 4096,
                1,
                0,
                &cb,
                &mut w,
            )
        };
        assert_eq!(status, TcslogStatus::Ok, "open: {status:?}");
        assert!(!w.is_null());

        let msg = b"attitude nominal";
        let mut written = 0u32;
        let status = unsafe { tcslog_write_record(w, msg.as_ptr(), msg.len(), &mut written) };
        assert_eq!(status, TcslogStatus::Ok);
        // The payload plus the four-byte data header a VariableSimple
        // record carries. The conversion is the test's own arithmetic
        // rather than a cast of the value under test: a message this
        // short cannot overflow it, and `try_from` says so without an
        // allow.
        let payload = u32::try_from(msg.len()).expect("a short message");
        assert_eq!(written, payload + 4);
        assert_eq!(unsafe { tcslog_write_flush(w) }, TcslogStatus::Ok);
        unsafe { tcslog_write_close(w) };
        // The close handed the open segment file over, through the
        // context rather than through any process-wide state.
        assert_eq!(sends, 1, "the context did not reach the send callback");

        let mut r: *mut TcslogRead = std::ptr::null_mut();
        let status =
            unsafe { tcslog_read_open(d.as_ptr(), prefix.as_ptr(), suffix.as_ptr(), &mut r) };
        assert_eq!(status, TcslogStatus::Ok, "read open: {status:?}");

        let mut buf = [0u8; 64];
        let mut result = TcslogReadResult::empty();
        let status = unsafe { tcslog_read_record(r, buf.as_mut_ptr(), buf.len(), &mut result) };
        assert_eq!(status, TcslogStatus::Ok);
        assert_eq!(result.meta, TcslogMeta::VariableSimple);
        assert_eq!(&buf[..result.n as usize], msg);

        // The log held one record, so the next read is the end of it.
        let status = unsafe { tcslog_read_record(r, buf.as_mut_ptr(), buf.len(), &mut result) };
        assert_eq!(status, TcslogStatus::Eof);
        unsafe { tcslog_read_close(r) };
    }

    #[test]
    fn a_null_handle_is_reported_rather_than_dereferenced() {
        let mut result = TcslogReadResult::empty();
        let mut buf = [0u8; 8];
        assert_eq!(
            unsafe { tcslog_read_record(std::ptr::null_mut(), buf.as_mut_ptr(), 8, &mut result) },
            TcslogStatus::NullArgument
        );
        assert_eq!(
            unsafe { tcslog_write_flush(std::ptr::null_mut()) },
            TcslogStatus::NullArgument
        );
        // Closing null is allowed, as freeing null is.
        unsafe { tcslog_write_close(std::ptr::null_mut()) };
        unsafe { tcslog_read_close(std::ptr::null_mut()) };
    }

    #[test]
    fn a_string_that_is_not_utf8_is_refused() {
        let dir = TmpDir::new("utf8");
        let d = dir.c_string();
        // 0xFF is not valid UTF-8 anywhere.
        let bad = CString::new(vec![0xFFu8, 0xFE]).unwrap();
        let suffix = CString::new(".log").unwrap();
        let mut w: *mut TcslogWrite = std::ptr::null_mut();
        let status = unsafe {
            tcslog_write_open(
                d.as_ptr(),
                bad.as_ptr(),
                suffix.as_ptr(),
                SEGMENT_FILE_HEADER_LEN + 4096,
                1,
                0,
                std::ptr::null(),
                &mut w,
            )
        };
        assert_eq!(status, TcslogStatus::NotUtf8);
        assert!(w.is_null(), "a failed open must leave the handle null");
    }

    #[test]
    fn every_status_has_a_description() {
        // A missing arm would be a null-free string on the C side, so
        // the one thing worth asserting is that none of them is empty.
        for s in [
            TcslogStatus::Ok,
            TcslogStatus::Eof,
            TcslogStatus::SessionEnd,
            TcslogStatus::ReadTruncated,
            TcslogStatus::ReadOverflow,
            TcslogStatus::ClockError,
            TcslogStatus::FixedLenMismatch,
            TcslogStatus::InvalidHeader,
            TcslogStatus::InvalidPathname,
            TcslogStatus::IoError,
            TcslogStatus::NoSegmentFiles,
            TcslogStatus::PathDelimiterNotAllowed,
            TcslogStatus::PayloadTooLarge,
            TcslogStatus::SegSizeTooSmall,
            TcslogStatus::TimerResolutionZero,
            TcslogStatus::VersionMismatch,
            TcslogStatus::NullArgument,
            TcslogStatus::NotUtf8,
            TcslogStatus::Panic,
            TcslogStatus::InvalidFormat,
        ] {
            let p = tcslog_status_str(s);
            assert!(!p.is_null());
            let text = unsafe { CStr::from_ptr(p) }.to_str().expect("UTF-8");
            assert!(!text.is_empty(), "{s:?} has no description");
        }
    }

    #[test]
    fn the_format_version_is_reported() {
        let (mut major, mut minor, mut patch) = (99u32, 99u32, 99u32);
        tcslog_format_version(&mut major, &mut minor, &mut patch);
        assert_eq!(
            (major, minor, patch),
            (VERSION_MAJOR, VERSION_MINOR, VERSION_PATCH)
        );
        // Null is allowed for any of the three.
        tcslog_format_version(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    #[test]
    fn a_header_length_is_reported() {
        assert_eq!(tcslog_segment_file_header_len(), SEGMENT_FILE_HEADER_LEN);
    }
}
