//! Error type shared by the read and write halves of tcslog.

use std::io;
use thiserror::Error;

/// Errors returned by any tcslog operation.
#[derive(Debug, Error)]
pub enum LogError {
    /// The underlying operation returned an I/O error.
    #[error("I/O error: {0}")]
    IoError(#[from] io::Error),

    /// The requested path could not be represented as a valid pathname on
    /// this platform (for example, it contained a NUL byte, or its length
    /// exceeded `PATH_MAX`).
    #[error("invalid pathname")]
    InvalidPathname,

    /// The combination of prefix, suffix, and segment identifier does not
    /// form a valid file name.
    #[error("invalid file name")]
    InvalidFileName,

    /// The prefix or suffix contained a path separator.
    #[error("path delimiter not allowed in prefix or suffix")]
    PathDelimiterNotAllowed,

    /// Too much telemetry data in the current data record to fit in the
    /// user-supplied buffer. The wrapped value is the number of bytes
    /// actually placed in the buffer.
    #[error("read overflow (buffer filled with {0} bytes; remainder discarded)")]
    ReadOverflow(u32),

    /// The requested `seg_size_max` is not larger than
    /// `SEGMENT_FILE_HEADER_LEN` plus a single data-record header.
    #[error("seg_size_max is smaller than a segment header plus one data header")]
    SegSizeTooSmall,

    /// The most recently opened segment file belongs to a different
    /// session than the previous one. The next read will return the
    /// first record of the new session.
    #[error("session boundary reached")]
    SessionEnd,

    /// No more data records are available.
    #[error("end of log")]
    Eof,

    /// The segment file header did not match the tcslog on-disk layout.
    #[error("invalid or corrupt segment file header")]
    InvalidHeader,

    /// The segment file was written by an incompatible version of tcslog.
    #[error("incompatible segment file version")]
    VersionMismatch,

    /// The segment file's stored `segment_id` did not match the value
    /// encoded in its file name.
    #[error("segment id does not match file name")]
    SegIdMismatch,

    /// The current `Format::Fixed(n)` requires records of exactly `n`
    /// bytes; the caller passed a differently sized payload.
    #[error("payload length does not match Format::Fixed record size")]
    FixedLenMismatch,

    /// A payload larger than `RecSize::MAX` bytes was supplied to a
    /// write.
    #[error("payload larger than RecSize::MAX")]
    PayloadTooLarge,

    /// No segment files matching the given prefix and suffix were found
    /// in the log directory.
    #[error("no segment files found")]
    NoSegmentFiles,

    /// The system clock returned a value before the UNIX epoch.
    #[error("system clock returned a value before the UNIX epoch")]
    ClockError,
}
