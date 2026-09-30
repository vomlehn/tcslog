//! Onboard logging of telemetry for vehicles that must store data until
//! there is an opportunity to send it: spacecraft, autonomous
//! underwater vehicles, and anything else out of contact for long
//! stretches.
//!
//! A log is a directory of *segment files*, each at most
//! `seg_size_max` bytes, whose names are a caller-chosen prefix and
//! suffix around a [`SegId`]. Bounding the file size bounds the storage
//! a log occupies, which is what lets a mission commit storage to
//! telemetry with confidence; splitting the log into files is what lets
//! it be sent down in small batches, and what limits the damage a bad
//! sector can do.
//!
//! Records are written with [`LogWrite`] and read with [`LogRead`].
//! Neither allocates once it has been constructed, so both suit
//! embedded systems with a fixed memory budget. The two exceptions are
//! documented where they appear: [`LogRead::iter`], which yields owned
//! [`Record`] values, and [`LogRead::take_opened_headers`].
//!
//! # Recovery
//!
//! The reason for the segment header's `remaining` and `sequence`
//! fields is that stored telemetry gets damaged. A reader that finds a
//! segment file missing, unreadable, or cut short discards the record
//! that was in progress, resynchronizes on the next segment file that
//! opens cleanly, and carries on with the records after it. It reports
//! what it lost rather than quietly returning a smaller log, and it
//! never splices bytes from either side of a gap into a record that was
//! never written.
//!
//! # Example
//!
//! ```no_run
//! use tcslog::{Format, LogRead, LogWrite, WriteCallbacks, SEGMENT_FILE_HEADER_LEN};
//!
//! # fn main() -> Result<(), tcslog::LogError> {
//! let mut log = LogWrite::new(
//!     "/var/telemetry",
//!     "tlm-",
//!     ".seg",
//!     SEGMENT_FILE_HEADER_LEN + 4096,
//!     Format::VariableTsRc,
//!     WriteCallbacks::default(),
//! )?;
//! log.write_str("attitude nominal")?;
//! log.flush()?;
//! drop(log);
//!
//! let mut log = LogRead::new("/var/telemetry", "tlm-", ".seg")?;
//! let mut buf = [0u8; 256];
//! while let Ok(rec) = log.read(&mut buf) {
//!     println!("{}", String::from_utf8_lossy(&buf[..rec.n as usize]));
//! }
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]

mod error;
mod format;
mod header;
mod read;
mod segid;
mod seq_id;
mod util;
#[cfg(feature = "write")]
mod write;

pub use error::LogError;
pub use format::{Format, Meta};
pub use header::{
    SegmentHeader, SEGMENT_FILE_HEADER_LEN, VERSION_MAJOR, VERSION_MINOR, VERSION_PATCH,
};
pub use read::{LogRead, LogReadIter, ReadResult, Record};
pub use segid::SegId;
pub use seq_id::SeqId;
pub use util::{format_timestamp, record_trailer};
#[cfg(feature = "write")]
pub use write::{LogWrite, WriteCallbacks};

/// The type that holds the size of a data record's telemetry payload.
///
/// For on-disk format version 0.1.0 this is `u32`, so
/// `RecSize::MAX` is the largest payload a record may hold.
pub type RecSize = u32;

/// Nanoseconds since the UNIX epoch, the form in which
/// [`Format::VariableTsRc`] stamps each record.
pub type Timestamp = u64;

/// Position of a record within its session: one for the first record,
/// one more for each record after it.
///
/// The count belongs to the session rather than to the log, so it
/// restarts at one whenever a new session begins -- which is why a
/// reader must report a session boundary before handing back the records
/// that follow it.
pub type RecordCount = u64;
