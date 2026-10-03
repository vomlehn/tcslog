//! The error type every fallible operation in this crate returns.

use std::io;

use crate::RecSize;

/// Everything that can go wrong reading or writing a log.
///
/// Several variants are outcomes rather than faults. [`Eof`](Self::Eof)
/// ends a read, [`SessionEnd`](Self::SessionEnd) separates one session's
/// records from the next, and
/// [`ReadOverflow`](Self::ReadOverflow) says the caller's buffer was
/// smaller than the record. They travel as errors because each one
/// means the caller did not get the record it asked for.
///
/// No variant exists that nothing constructs. A segment file the reader
/// cannot use, in particular, raises no error of its own: it is skipped,
/// and the loss shows up through the sequence gap its neighbours reveal,
/// exactly as a deleted file's would.
#[derive(Debug, thiserror::Error)]
pub enum LogError {
    /// The real-time clock did not read later than the UNIX epoch when
    /// a writer was being constructed, which is what an unset clock
    /// reads on most systems. No segment ID or record timestamp can be
    /// minted from such a clock, and a writer reads it only once, so a
    /// correction arriving later would not mend the times already
    /// minted.
    #[error("the real-time clock is not set: it does not read later than the UNIX epoch")]
    ClockError,

    /// No more data records are available: the reader has exhausted its
    /// list of segment files.
    #[error("end of log")]
    Eof,

    /// The format is `Fixed(n)` and the payload length is not `n`. A
    /// zero-length payload under `Fixed(n)` is this error, since zero is
    /// a wrong length like any other. `Format::Fixed(0)`, which no log
    /// may use, is reported the same way.
    #[error("payload length does not match the fixed record length")]
    FixedLenMismatch,

    /// The bytes read do not form a segment file header: the type field
    /// is not `tcslogsf`, or the data format tag is not one of the
    /// defined values, or the tag is that of `Fixed` with a length of
    /// zero.
    #[error("not a tcslog segment file header")]
    InvalidHeader,

    /// A directory name, prefix, and suffix could not be combined with a
    /// segment ID into a usable path, or the named directory is not a
    /// directory.
    #[error("invalid path name")]
    InvalidPathname,

    /// An underlying I/O operation failed.
    #[error("I/O error: {0}")]
    IoError(#[from] io::Error),

    /// No file in the directory matches the prefix and suffix, so there
    /// is no log there to read.
    #[error("no segment files match the given prefix and suffix")]
    NoSegmentFiles,

    /// The prefix or suffix contains a path separator, which would let a
    /// segment file escape the directory it was meant for.
    #[error("prefix and suffix must not contain a path delimiter")]
    PathDelimiterNotAllowed,

    /// A payload larger than [`RecSize::MAX`](crate::RecSize) was
    /// offered to a write, or one whose length plus its data header
    /// would exceed the byte count `write` returns.
    #[error("payload is too large to store in one data record")]
    PayloadTooLarge,

    /// The record was longer than the supplied buffer. The value is how
    /// many bytes reached the front of the buffer; they are real
    /// telemetry. The rest of the record was skipped, so the next read
    /// starts at the following record.
    #[error("record is larger than the supplied buffer; {0} byte(s) captured")]
    ReadOverflow(RecSize),

    /// A crossing from one segment file into the next found a
    /// continuation that cannot follow it: a gap in the sequence, or a
    /// `remaining` field disagreeing with the bytes still owed to the
    /// record in progress. The record is lost and the next read
    /// resynchronizes.
    #[error("read truncated: {lost} segment file(s) lost, {n} payload byte(s) recovered")]
    ReadTruncated {
        /// How many segment files the sequence numbers show are missing
        /// at the crossing. Zero when the sequence is intact and the
        /// crossing was refused because a surviving segment is itself
        /// corrupt or short.
        lost: u64,
        /// How many payload bytes of the cut-short record reached the
        /// front of the caller's buffer. Those bytes are real telemetry.
        /// Zero when the record was cut short before any payload was
        /// reached, which is the case while a data header was being
        /// decoded and ahead of a session's first surviving segment.
        n: RecSize,
    },

    /// The requested maximum segment size is not strictly greater than
    /// the segment file header plus one data header for the format in
    /// use, so a segment file could not hold even an empty record.
    #[error("maximum segment size leaves no room for a data record")]
    SegSizeTooSmall,

    /// Every record of the session just being read has been returned,
    /// and the next segment file belongs to a different session. The
    /// read after this one returns that session's first record.
    #[error("end of session")]
    SessionEnd,

    /// The build-time timer resolution is zero, so the writer could not
    /// guarantee that a fresh segment ID differs from the last one.
    #[error("the build-time timer resolution is zero")]
    TimerResolutionZero,

    /// The segment file was written by a version of the on-disk format
    /// this build cannot read.
    #[error("segment file version is not readable by this build")]
    VersionMismatch,
}
