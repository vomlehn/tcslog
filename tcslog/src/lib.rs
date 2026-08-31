//! Tcslog: onboard telemetry logging in rolling segment files.
//!
//! See `docs/tcslog-prompt.rst` for the design specification and
//! `docs/tcslog.rst` for the user guide.

#![deny(rust_2018_idioms)]
#![warn(missing_docs)]

mod format;
mod header;
mod io_util;
mod read;
mod segid;
mod write;

pub use format::{Format, Meta, RecSize, RecordCount, Timestamp};
pub use header::{SegmentHeader, SEGMENT_HEADER_SIZE};
pub use read::{LogRead, ReadResult};
pub use segid::SegId;
pub use write::{LogWrite, WriteCallbacks};

use std::io;
use thiserror::Error;

/// Errors returned by Tcslog operations.
#[derive(Debug, Error)]
pub enum LogError {
    /// Wraps an underlying I/O error.
    #[error("I/O error: {0}")]
    IoError(#[from] io::Error),

    /// The user-supplied buffer was too small to hold the data record. The
    /// value is the number of bytes actually available for this record.
    #[error("read buffer too small; record has {0} bytes")]
    ReadOverflow(u32),

    /// A supplied prefix or suffix contained a filesystem path delimiter.
    #[error("prefix or suffix contains a filesystem delimiter")]
    HasDelimiter,

    /// `seg_size_max` was too small to hold a segment header plus at least
    /// one byte of data.
    #[error("seg_size_max ({0}) is smaller than the minimum segment size")]
    SegSizeTooSmall(u32),

    /// A configuration value was invalid (e.g. `Fixed(0)`).
    #[error("invalid configuration: {0}")]
    InvalidConfig(&'static str),

    /// A segment file did not contain the expected `tcslogsf` magic.
    #[error("segment file is not a tcslog segment file")]
    BadMagic,

    /// A segment file's version was incompatible with this Tcslog.
    #[error("segment file version {0:?} is incompatible")]
    IncompatibleVersion([u8; 4]),

    /// A segment file's header contents were inconsistent (e.g. a segment ID
    /// that does not match the segment ID encoded in the file name).
    #[error("segment header is inconsistent: {0}")]
    InconsistentHeader(&'static str),

    /// The end of the log has been reached; no more data is available.
    #[error("end of log")]
    Eof,

    /// No segment files matching the log's prefix and suffix were found.
    #[error("no matching segment files were found")]
    NoSegments,
}
