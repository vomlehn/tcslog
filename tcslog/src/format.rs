//! Segment-file data formats and per-record metadata.

/// Type used for the size (in bytes) of the telemetry payload portion of
/// a data record.
pub type RecSize = u32;

/// Selects how records are laid out in the data section of a segment
/// file.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Format {
    /// Every record contains exactly `n` payload bytes. There is no
    /// per-record header. `n` must be at least one and at most
    /// [`RecSize::MAX`](RecSize).
    Fixed(RecSize),
    /// Records carry a length prefix and a variable-size payload from
    /// zero up to [`RecSize::MAX`](RecSize) bytes long. No other
    /// metadata is written per record.
    VariableSimple,
    /// Like [`Format::VariableSimple`], but each record also carries a
    /// timestamp (nanoseconds since the UNIX epoch) and a monotonically
    /// increasing record counter.
    VariableTsRc,
}

impl Format {
    /// The single-byte tag written in the segment header to identify this
    /// format.
    pub const fn tag(self) -> u8 {
        match self {
            Format::Fixed(_) => 0,
            Format::VariableSimple => 1,
            Format::VariableTsRc => 2,
        }
    }

    /// The `n` parameter for [`Format::Fixed`], or zero for other formats.
    pub const fn fixed_len(self) -> RecSize {
        match self {
            Format::Fixed(n) => n,
            _ => 0,
        }
    }

    /// Number of bytes the per-record data header consumes on disk.
    pub const fn data_header_len(self) -> u32 {
        match self {
            Format::Fixed(_) => 0,
            Format::VariableSimple => 4,
            Format::VariableTsRc => 4 + 8 + 8,
        }
    }
}

/// Per-record metadata returned to the reader alongside the payload.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Meta {
    /// The segment file uses [`Format::Fixed`].
    Fixed,
    /// The segment file uses [`Format::VariableSimple`]; no metadata is
    /// carried with the record.
    VariableSimple,
    /// The segment file uses [`Format::VariableTsRc`]. The first value is
    /// the nanosecond UNIX timestamp; the second is the record count.
    VariableTsRc(u64, u64),
}
