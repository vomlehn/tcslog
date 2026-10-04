//! Tcslog stores telemetry on board a vehicle that cannot send it home
//! as it is produced. Spacecraft, autonomous underwater vehicles, buoys,
//! and balloons all spend long stretches out of contact, and all of them
//! have a storage budget fixed long before launch. Tcslog is built for
//! that situation: it keeps telemetry in a form that can be handed down
//! in small pieces whenever a link appears, and it keeps it in a form
//! that still yields most of its contents after part of the storage has
//! gone bad.
//!
//! A Tcslog log is a directory of *segment files*. Each one is at most a
//! size the caller chooses, and its name is a caller-chosen prefix and
//! suffix wrapped around a [`SegId`]. Records are appended to the file
//! being written; when it is full, Tcslog hands it to a function the
//! caller supplied and opens the next one.
//!
//! Records are written with [`LogWrite`] and read with [`LogRead`].
//!
//! # Advantages in normal operation
//!
//! **The storage a log occupies is known in advance.** The writer fills a
//! segment file to exactly the maximum and continues the record in the
//! next one, so nothing is padded and no file is left short of its own
//! accord. Two kinds are short because the writer stopped rather than
//! because it rolled: the file currently being written, and the file
//! that ended each earlier session. Every other file is exactly the
//! maximum size, so the number of files times that size is the space a
//! log occupies, and is an upper bound on it in every case. A mission can
//! commit a storage budget to telemetry and know it will be honoured.
//!
//! **Data leaves in small pieces.** Telemetry is handed over a file at a
//! time, as each one fills, rather than as one growing file that must be
//! sent whole. A pass over a ground station that is too short for the
//! whole log is still long enough for some of it, and what has already
//! gone down is a set of files that can simply be skipped.
//!
//! **No allocation once running.** Writing a record and reading a record
//! allocate nothing, so a Tcslog log can be written and read on a system
//! with a fixed memory budget and no heap to grow. Records are read into
//! a buffer the caller owns, and the internal buffers a segment file name
//! and path need are reserved once and reused, so even rolling from one
//! segment file to the next allocates nothing.
//!
//! The three exceptions are all operations a caller asks for explicitly,
//! and each says so where it is described: enumerating the directory,
//! which [`LogRead::new`], [`LogWrite::new`], and [`LogWrite::clear`]
//! must do and which no platform offers without allocating;
//! [`LogRead::iter`], whose records own their payloads; and
//! [`LogRead::take_opened_headers`].
//!
//! **Metadata only where it is wanted.** Three record layouts are
//! offered, from one that stores payload bytes and nothing else to one
//! that stamps every record with the time it was written and its
//! position in the run. The cost of the metadata is paid only by the logs
//! that use it. See [Record formats](#record-formats).
//!
//! **The caller decides what durability costs.** Two callbacks decide
//! how hard Tcslog works to get bytes onto stable storage: one runs
//! after every record, and one runs when a segment file fills. A log
//! that must survive an unplanned reset can flush at every record; one
//! that must keep up with a fast sensor need not.
//!
//! # Advantages when things go wrong
//!
//! Stored telemetry gets damaged. A sector goes bad, a file is lost in a
//! reset part way through a write, a copy off the vehicle drops a file.
//! What distinguishes Tcslog is what a reader can still do with the
//! remains.
//!
//! **Damage is contained.** Splitting a log into segment files puts a
//! bound on what any single failure costs: the records in the damaged
//! file, and at most one more that happened to straddle its boundary.
//! Everything else in the log still reads. The bound is a choice the
//! caller makes, because the segment file size is a parameter: smaller
//! files mean less telemetry lost per failure.
//!
//! **The reader recovers by itself.** On finding a file missing,
//! unreadable, or cut short, the reader gives up the record it was in
//! the middle of, finds the start of the next whole record in the first
//! file that opens cleanly, and carries on. It does not stop, and it
//! does not need to be reset. The only thing that ends a read is running
//! out of files.
//!
//! **Losses are reported, not hidden.** A reader that quietly returned a
//! smaller log would be worse than one that failed, because nothing
//! downstream would know that the gap in the telemetry was a gap rather
//! than a quiet spell. Tcslog reports every loss it finds, and reports
//! how many files' worth went missing. Losses that fall exactly on a
//! record boundary are caught too, which is the case a naive reader
//! misses entirely: both sides of such a gap look perfectly ordinary.
//!
//! **Nothing is invented.** The reader will not stitch bytes from either
//! side of a gap into a record and hand it over. Where the surviving
//! bytes cannot be shown to be one record, the record is reported lost.
//! A plausible-looking record that was never written is a worse outcome
//! than a missing one, because it cannot be told from real telemetry.
//!
//! **Partial records are still telemetry.** Bytes that reached the
//! caller before a gap are real measurements, and are handed over rather
//! than discarded, marked so that a record cut short cannot be mistaken
//! for a whole one.
//!
//! **A renamed or copied file is still readable.** Each segment file
//! carries its own identity, so a file that has been renamed, or copied
//! out of its directory, can still be identified and its contents
//! examined.
//!
//! # Setup
//!
//! Two things have to be in place before a log can be written. Neither
//! is needed to read one, so a program that only reads can take the
//! crate with `default-features = false` and skip this section.
//!
//! **The real-time clock must hold the correct time.** A writer reads it
//! once, when it is constructed, pairs it with the monotonic clock, and
//! measures every segment identifier and record timestamp from that
//! pairing -- which is what keeps identifiers in creation order when NTP
//! or an operator steps the real-time clock backwards. Because it is
//! read once, a correction arriving later does not reach the identifiers
//! already minted, so a clock that does not read later than the UNIX
//! epoch, which is what an unset clock reads on most systems, is refused
//! with [`LogError::ClockError`] rather than used.
//!
//! **`TIMER_RESOLUTION` must be set when building with the `write`
//! feature.** It is how finely this machine's clock advances, in
//! nanoseconds, and nothing guesses it: left unset the crate still
//! builds, but opening a log reports
//! [`LogError::TimerResolutionZero`]. Set it in `.cargo/config.toml`:
//!
//! ```toml
//! [env]
//! TIMER_RESOLUTION = "1"
//! ```
//!
//! or on the command line, as `TIMER_RESOLUTION=1 cargo build`.
//!
//! It need not be exact. A writer that finds the value too small doubles
//! it, goes on doubling until a segment file name is free, and keeps
//! what it arrived at; the figure in force is reported back, and is the
//! one to build with next time. So starting at `1` and reading it back
//! is a fine way to find it.
//!
//! Both requirements, and the reasoning behind them, are set out at
//! length in the user manual, `docs/tcslog.rst` in [the
//! repository](https://github.com/vomlehn/tcslog).
//!
//! # Writing
//!
//! ```no_run
//! use tcslog::{Format, LogWrite, WriteCallbacks, SEGMENT_FILE_HEADER_LEN};
//!
//! # fn main() -> Result<(), tcslog::LogError> {
//! let mut log = LogWrite::new(
//!     "/var/telemetry",                 // an existing directory
//!     "seg-",                           // file name prefix
//!     ".tcslog",                        // file name suffix
//!     SEGMENT_FILE_HEADER_LEN + 65_536, // bytes per segment file
//!     Format::VariableTsRc,
//!     WriteCallbacks::default(),
//! )?;
//!
//! log.write_str("attitude nominal")?;
//! # Ok(())
//! # }
//! ```
//!
//! [`WriteCallbacks`] is where a filled segment file leaves this
//! library's care: `send` is called with its path, and must leave no
//! file of that name behind -- compress it, downlink it, or rename it
//! out of the log's naming pattern. The default `send` does nothing,
//! which suits development and lets segment files accumulate.
//!
//! # Reading
//!
//! Reading needs no timer resolution, so a program that only reads can
//! take the crate without its default features:
//!
//! ```toml
//! [dependencies]
//! tcslog = { version = "0.2", default-features = false }
//! ```
//!
//! ```no_run
//! use tcslog::{LogError, LogRead, Meta, RecSize};
//!
//! # fn handle(_payload: &[u8], _meta: Meta) {}
//! # fn note_loss(_lost: u64, _n: RecSize) {}
//! # fn main() -> Result<(), LogError> {
//! let mut log = LogRead::new("/var/telemetry", "seg-", ".tcslog")?;
//! let mut buf = [0u8; 4096];
//!
//! loop {
//!     match log.read(&mut buf) {
//!         Ok(result) => handle(&buf[..result.n as usize], result.meta),
//!         Err(LogError::Eof) => break,
//!         // Writing was interrupted here; record numbering restarts.
//!         Err(LogError::SessionEnd) => continue,
//!         // Telemetry was lost. `lost` files are missing, and the
//!         // first `n` bytes are real telemetry from a record that was
//!         // cut short.
//!         Err(LogError::ReadTruncated { lost, n }) => note_loss(lost, n),
//!         Err(e) => return Err(e),
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! The rule is: read again until [`LogError::Eof`]. Every other outcome
//! is news about the telemetry, not a failure of the reader.
//!
//! # Record formats
//!
//! | `Format` | Per-record overhead | What it stores |
//! | --- | --- | --- |
//! | `Fixed(n)` | none | payloads of exactly `n` bytes |
//! | `VariableSimple` | 4 bytes | a length |
//! | `VariableTsRc` | 20 bytes | a length, a timestamp, a record number |
//!
//! # Tools
//!
//! [`tcslog-tools`](https://github.com/vomlehn/tcslog-tools) provides `tcslog-dump`,
//! which prints a log, and `tcslog-dumphdr`, which prints one segment file's
//! header without consulting its name -- for files renamed or copied out of their
//! directory.
//!
//! # Documentation
//!
//! The full manual, including the theory of operation, is `docs/tcslog.rst` in
//! [the repository](https://github.com/vomlehn/tcslog).
//!
//! # License
//!
//! Licensed under either of Apache License, Version 2.0 or the MIT license, at
//! your option. Unless you explicitly state otherwise, any contribution
//! intentionally submitted for inclusion in the work by you, as defined in the
//! Apache-2.0 license, shall be dual licensed as above, without any additional
//! terms or conditions.
//!
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
