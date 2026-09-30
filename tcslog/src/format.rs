//! Data record layouts and the per-record metadata they carry.

use crate::error::LogError;
use crate::{RecSize, RecordCount, Timestamp};

/// Tag byte stored in the segment header for [`Format::Fixed`].
pub(crate) const TAG_FIXED: u8 = 0;
/// Tag byte stored in the segment header for [`Format::VariableSimple`].
pub(crate) const TAG_VARIABLE_SIMPLE: u8 = 1;
/// Tag byte stored in the segment header for [`Format::VariableTsRc`].
pub(crate) const TAG_VARIABLE_TSRC: u8 = 2;

/// On-disk size of a [`Format::VariableSimple`] data header: one
/// `RecSize` payload length.
pub(crate) const VARIABLE_SIMPLE_HEADER_LEN: u32 = 4;
/// On-disk size of a [`Format::VariableTsRc`] data header: a `RecSize`
/// payload length, a `Timestamp`, and a `RecordCount`.
pub(crate) const VARIABLE_TSRC_HEADER_LEN: u32 = 4 + 8 + 8;

/// How data records are laid out in a segment file's data section.
///
/// The choice is made once, when the log is created, and is recorded in
/// every segment file's header. It trades storage against metadata: a
/// fixed-length record costs nothing per record and carries nothing,
/// while a timestamped one costs twenty bytes and dates itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Every record holds exactly this many payload bytes, and no data
    /// header is written at all. The most compact format, at the price
    /// of a length fixed for the life of the log. The length must be at
    /// least one.
    Fixed(RecSize),

    /// Records hold from zero to [`RecSize::MAX`](crate::RecSize) bytes,
    /// with a four-byte data header giving the length. Suited to
    /// telemetry that already carries its own timestamp.
    VariableSimple,

    /// As [`VariableSimple`](Self::VariableSimple), and the data header
    /// also carries the time the record was written and its position
    /// within the session.
    VariableTsRc,
}

impl Format {
    /// The single byte stored in the segment file header's data format
    /// field.
    ///
    /// Returns 0 for `Fixed`, 1 for `VariableSimple`, and 2 for
    /// `VariableTsRc`.
    #[must_use]
    pub const fn tag(self) -> u8 {
        match self {
            Self::Fixed(_) => TAG_FIXED,
            Self::VariableSimple => TAG_VARIABLE_SIMPLE,
            Self::VariableTsRc => TAG_VARIABLE_TSRC,
        }
    }

    /// The `n` of `Fixed(n)`, stored in the header's data format
    /// argument field.
    ///
    /// Returns the fixed record length, or zero for the variable
    /// formats, which do not use the field.
    #[must_use]
    pub const fn fixed_len(self) -> RecSize {
        match self {
            Self::Fixed(n) => n,
            Self::VariableSimple | Self::VariableTsRc => 0,
        }
    }

    /// On-disk size of one data record's data header.
    ///
    /// Returns zero for `Fixed`, four for `VariableSimple`, and twenty
    /// for `VariableTsRc`.
    #[must_use]
    pub const fn data_header_len(self) -> u32 {
        match self {
            Self::Fixed(_) => 0,
            Self::VariableSimple => VARIABLE_SIMPLE_HEADER_LEN,
            Self::VariableTsRc => VARIABLE_TSRC_HEADER_LEN,
        }
    }

    /// Rebuilds a `Format` from the two header fields that store it.
    ///
    /// * `tag` -- the data format tag byte.
    /// * `arg` -- the data format argument, the `n` of `Fixed(n)`.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::InvalidHeader`] if the tag is not one of the
    /// defined values, or if it is that of `Fixed` and the argument is
    /// zero. Neither can come from a segment file this library wrote, so
    /// the header is not one.
    pub(crate) fn from_tag(tag: u8, arg: RecSize) -> Result<Self, LogError> {
        match tag {
            TAG_FIXED if arg > 0 => Ok(Self::Fixed(arg)),
            TAG_VARIABLE_SIMPLE => Ok(Self::VariableSimple),
            TAG_VARIABLE_TSRC => Ok(Self::VariableTsRc),
            _ => Err(LogError::InvalidHeader),
        }
    }
}

/// The metadata one data record carries, which depends on the log's
/// [`Format`].
///
/// A writer mints this as it builds a record's data header, and a reader
/// recovers it from that header, so the same values come back out as
/// went in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Meta {
    /// The log uses fixed-length records, which carry no metadata.
    Fixed,

    /// The record is variable-length and carries no further metadata.
    VariableSimple,

    /// The record carries the time it was written, in nanoseconds since
    /// the UNIX epoch, and its position within the session, counting
    /// from one.
    VariableTsRc(Timestamp, RecordCount),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_are_zero_one_two() {
        assert_eq!(Format::Fixed(1).tag(), 0);
        assert_eq!(Format::VariableSimple.tag(), 1);
        assert_eq!(Format::VariableTsRc.tag(), 2);
    }

    #[test]
    fn data_header_lengths() {
        assert_eq!(Format::Fixed(7).data_header_len(), 0);
        assert_eq!(Format::VariableSimple.data_header_len(), 4);
        assert_eq!(Format::VariableTsRc.data_header_len(), 20);
    }

    #[test]
    fn fixed_len_is_zero_for_variable_formats() {
        assert_eq!(Format::Fixed(7).fixed_len(), 7);
        assert_eq!(Format::VariableSimple.fixed_len(), 0);
        assert_eq!(Format::VariableTsRc.fixed_len(), 0);
    }

    #[test]
    fn round_trips_through_the_header_fields() {
        for f in [
            Format::Fixed(9),
            Format::VariableSimple,
            Format::VariableTsRc,
        ] {
            assert_eq!(Format::from_tag(f.tag(), f.fixed_len()).unwrap(), f);
        }
    }

    #[test]
    fn rejects_unknown_tag_and_zero_length_fixed() {
        assert!(Format::from_tag(3, 0).is_err());
        assert!(Format::from_tag(TAG_FIXED, 0).is_err());
    }
}
