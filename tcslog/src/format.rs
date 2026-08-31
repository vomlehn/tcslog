//! Data record and metadata format definitions.
//!
//! A tcslog segment file's on-disk layout is:
//!
//! ```text
//! +-------------------+
//! |  segment header   |  SEGMENT_HEADER_SIZE bytes (see header.rs)
//! +-------------------+
//! |  data section     |  data records back-to-back
//! +-------------------+
//! ```
//!
//! Every data record consists of a fixed-shape *data header* followed by
//! *telemetry payload* bytes. The `Format` chosen for the log determines the
//! shape of the data header and whether the payload length is fixed or
//! variable. `Meta` is the format-specific metadata returned to the reader
//! alongside each record's payload.

/// Type used to hold the payload byte count of a single data record.
pub type RecSize = u32;

/// Type used to hold the monotonically-increasing record count carried by
/// [`Format::VariableTsRc`] records.
pub type RecordCount = u64;

/// Nanoseconds since the UNIX epoch. Carried by [`Format::VariableTsRc`]
/// records and used as the source of unique segment identifiers.
pub type Timestamp = u64;

/// Log data format. Selected at [`crate::LogWrite`] construction and stored in
/// every segment header, so a reader can decode records without out-of-band
/// information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Every record has the same fixed payload length `n`. The data header
    /// is zero bytes.
    Fixed(RecSize),
    /// Variable payload length prefixed by a 4-byte length field. Nothing
    /// else is stored per-record.
    VariableSimple,
    /// Variable payload length prefixed by a 4-byte length field, an 8-byte
    /// nanosecond timestamp, and an 8-byte record count.
    VariableTsRc,
}

impl Format {
    /// Number of bytes occupied by this format's data header (i.e. the
    /// per-record overhead before the payload).
    pub const fn header_size(self) -> usize {
        match self {
            Format::Fixed(_) => 0,
            Format::VariableSimple => 4,
            Format::VariableTsRc => 4 + 8 + 8,
        }
    }

    /// Discriminant byte stored in the segment header.
    pub(crate) const fn tag(self) -> u8 {
        match self {
            Format::Fixed(_) => 0,
            Format::VariableSimple => 1,
            Format::VariableTsRc => 2,
        }
    }

    /// `n` value stored in the segment header alongside the tag. Meaningful
    /// only for [`Format::Fixed`]; zero otherwise.
    pub(crate) const fn fixed_n(self) -> u32 {
        match self {
            Format::Fixed(n) => n,
            _ => 0,
        }
    }

    /// Reconstruct a `Format` from the byte pair written into the segment
    /// header. Returns `None` if the tag is unknown.
    pub(crate) fn from_tag_and_n(tag: u8, n: u32) -> Option<Format> {
        match tag {
            0 => Some(Format::Fixed(n)),
            1 => Some(Format::VariableSimple),
            2 => Some(Format::VariableTsRc),
            _ => None,
        }
    }
}

/// Format-specific metadata returned by a read.
///
/// The variants line up with [`Format`]. Only the fields carried in the data
/// header appear here; the payload bytes are written into the caller's buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Meta {
    /// Companion to [`Format::Fixed`]. Records carry no metadata.
    Fixed,
    /// Companion to [`Format::VariableSimple`]. Records carry no metadata.
    VariableSimple,
    /// Companion to [`Format::VariableTsRc`]. The tuple is
    /// `(timestamp, record_count)`.
    VariableTsRc(Timestamp, RecordCount),
}
