//! The segment file header: its in-memory form and on-disk encoding.

use std::io::{Read, Write};

use crate::error::LogError;
use crate::format::Format;
use crate::segid::SegId;
use crate::seq_id::SeqId;
use crate::RecSize;

/// Length of a segment file header, in bytes. The same for every format,
/// so that a reader can obtain the header before it knows which format
/// the file uses.
pub const SEGMENT_FILE_HEADER_LEN: u32 = 53;

/// Major number of the on-disk format this build writes. A file whose
/// major differs is refused: a different major means the layout itself
/// differs.
pub const VERSION_MAJOR: u32 = 0;

/// Minor number of the on-disk format this build writes. A file whose
/// minor is greater is refused, since it may use something this build
/// does not know about; a lesser or equal one is readable.
pub const VERSION_MINOR: u32 = 1;

/// Patch number of the on-disk format this build writes. It takes no
/// part in the compatibility decision.
pub const VERSION_PATCH: u32 = 0;

/// The ASCII string that identifies a tcslog segment file, and the first
/// bytes of every one.
const TYPE_MAGIC: &[u8; 8] = b"tcslogsf";

// Field offsets within the header. The fields are packed with no
// padding, and every numeric value is little-endian.
const OFF_TYPE: usize = 0;
const OFF_VERSION: usize = 8;
const OFF_SEGMENT_ID: usize = 12;
const OFF_SESSION_ID: usize = 20;
const OFF_MAX_SIZE: usize = 28;
const OFF_REMAINING: usize = 32;
const OFF_FORMAT_TAG: usize = 40;
const OFF_FORMAT_ARG: usize = 41;
const OFF_SEQUENCE: usize = 45;

/// Length of the header as a `usize`, for slicing.
const HEADER_LEN: usize = SEGMENT_FILE_HEADER_LEN as usize;

/// The header that opens every segment file.
///
/// The type and version fields are not represented here: they are
/// checked when a header is decoded and written from this build's own
/// constants when one is encoded, so there is no state a caller could
/// set to something the format does not allow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentHeader {
    /// Identifier of this segment file, which must match the identifier
    /// in its name.
    pub segment_id: SegId,

    /// Identifier of the first segment file of this session, carried by
    /// every segment file of it. A change in this field is how a reader
    /// sees one session end and the next begin.
    pub session_id: SegId,

    /// The maximum size, in bytes, of a segment file of this log. No
    /// segment file may be larger.
    pub max_size: u32,

    /// How many bytes at the start of this data section are the tail of
    /// a data record that began in an earlier segment file, counting
    /// that record's data header as well as its payload. Zero when the
    /// data section starts a fresh record. May exceed the data section,
    /// in which case the record continues past this file and no fresh
    /// record begins here.
    pub remaining: u64,

    /// How data records in this file are laid out.
    pub format: Format,

    /// Zero-based position of this segment file within its session.
    pub sequence: SeqId,
}

impl SegmentHeader {
    /// The number of bytes of a segment file that hold data records.
    ///
    /// * `max_size` -- the maximum segment file size, as stored in the
    ///   header's max size field.
    ///
    /// Returns `max_size` less the header length, saturating at zero so
    /// that a nonsensical stored value cannot underflow.
    #[must_use]
    pub const fn data_section_len(max_size: u32) -> u32 {
        max_size.saturating_sub(SEGMENT_FILE_HEADER_LEN)
    }

    /// The data section length of the file this header came from.
    ///
    /// Returns the length implied by this header's max size field.
    #[must_use]
    pub const fn data_len(&self) -> u32 {
        Self::data_section_len(self.max_size)
    }

    /// Encodes the header in its on-disk form.
    ///
    /// Returns the [`SEGMENT_FILE_HEADER_LEN`] bytes that open a segment
    /// file, with the type and version fields set from this build's
    /// constants.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; HEADER_LEN] {
        let mut out = [0u8; HEADER_LEN];
        out[OFF_TYPE..OFF_TYPE + 8].copy_from_slice(TYPE_MAGIC);
        out[OFF_VERSION..OFF_VERSION + 4].copy_from_slice(&version_bytes());
        out[OFF_SEGMENT_ID..OFF_SEGMENT_ID + 8].copy_from_slice(&self.segment_id.to_le_bytes());
        out[OFF_SESSION_ID..OFF_SESSION_ID + 8].copy_from_slice(&self.session_id.to_le_bytes());
        out[OFF_MAX_SIZE..OFF_MAX_SIZE + 4].copy_from_slice(&self.max_size.to_le_bytes());
        out[OFF_REMAINING..OFF_REMAINING + 8].copy_from_slice(&self.remaining.to_le_bytes());
        out[OFF_FORMAT_TAG] = self.format.tag();
        out[OFF_FORMAT_ARG..OFF_FORMAT_ARG + 4]
            .copy_from_slice(&self.format.fixed_len().to_le_bytes());
        out[OFF_SEQUENCE..OFF_SEQUENCE + 8].copy_from_slice(&self.sequence.to_le_bytes());
        out
    }

    /// Decodes a header from its on-disk form.
    ///
    /// * `bytes` -- at least [`SEGMENT_FILE_HEADER_LEN`] bytes read from
    ///   the start of a segment file.
    ///
    /// Returns the decoded header.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::InvalidHeader`] if `bytes` is too short, if
    /// the type field is not `tcslogsf`, or if the data format fields do
    /// not name a format. Returns [`LogError::VersionMismatch`] if the
    /// version field does not parse or names a version this build cannot
    /// read: the major must match and the minor must be no greater.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, LogError> {
        if bytes.len() < HEADER_LEN {
            return Err(LogError::InvalidHeader);
        }
        if &bytes[OFF_TYPE..OFF_TYPE + 8] != TYPE_MAGIC {
            return Err(LogError::InvalidHeader);
        }
        check_version(&bytes[OFF_VERSION..OFF_VERSION + 4])?;

        let tag = bytes[OFF_FORMAT_TAG];
        let arg = RecSize::from_le_bytes(take4(bytes, OFF_FORMAT_ARG));
        let format = Format::from_tag(tag, arg)?;

        Ok(Self {
            segment_id: SegId::from_le_bytes(take8(bytes, OFF_SEGMENT_ID)),
            session_id: SegId::from_le_bytes(take8(bytes, OFF_SESSION_ID)),
            max_size: u32::from_le_bytes(take4(bytes, OFF_MAX_SIZE)),
            remaining: u64::from_le_bytes(take8(bytes, OFF_REMAINING)),
            format,
            sequence: SeqId::from_le_bytes(take8(bytes, OFF_SEQUENCE)),
        })
    }

    /// Reads and decodes a header from a stream.
    ///
    /// Exactly [`SEGMENT_FILE_HEADER_LEN`] bytes are consumed, leaving
    /// the stream positioned at the first byte of the data section. No
    /// more than that is read, so this works on a pipe whose writer is
    /// still running.
    ///
    /// * `src` -- the stream to read from, positioned at the start of a
    ///   segment file.
    ///
    /// Returns the decoded header.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::IoError`] if the stream cannot supply that
    /// many bytes, and otherwise whatever
    /// [`from_bytes`](Self::from_bytes) reports.
    pub fn read_from<R: Read + ?Sized>(src: &mut R) -> Result<Self, LogError> {
        let mut buf = [0u8; HEADER_LEN];
        src.read_exact(&mut buf)?;
        Self::from_bytes(&buf)
    }

    /// Encodes the header and writes it to a stream.
    ///
    /// * `dst` -- the stream to write to, positioned at the start of a
    ///   segment file.
    ///
    /// Returns nothing on success.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::IoError`] if the write fails.
    pub fn write_to<W: Write + ?Sized>(&self, dst: &mut W) -> Result<(), LogError> {
        dst.write_all(&self.to_bytes())?;
        Ok(())
    }
}

/// The four version characters this build writes: two digits of major,
/// one of minor, one of patch.
fn version_bytes() -> [u8; 4] {
    [
        b'0' + u8::try_from(VERSION_MAJOR / 10).unwrap_or(0),
        b'0' + u8::try_from(VERSION_MAJOR % 10).unwrap_or(0),
        b'0' + u8::try_from(VERSION_MINOR % 10).unwrap_or(0),
        b'0' + u8::try_from(VERSION_PATCH % 10).unwrap_or(0),
    ]
}

/// Decides whether a stored version field names a format this build can
/// read.
///
/// The major must match exactly, because a different major means the
/// layout differs. The minor must be no greater than this build's, since
/// a greater one may use something unknown here. The patch takes no
/// part. A character outside `'0'` to `'9'` anywhere in the field is
/// refused as well: a field that does not parse cannot be compared.
fn check_version(field: &[u8]) -> Result<(), LogError> {
    if field.len() != 4 || field.iter().any(|b| !b.is_ascii_digit()) {
        return Err(LogError::VersionMismatch);
    }
    let digit = |b: u8| u32::from(b - b'0');
    let major = digit(field[0]) * 10 + digit(field[1]);
    let minor = digit(field[2]);
    if major == VERSION_MAJOR && minor <= VERSION_MINOR {
        Ok(())
    } else {
        Err(LogError::VersionMismatch)
    }
}

/// Copies four bytes out of a header buffer at `off`.
fn take4(bytes: &[u8], off: usize) -> [u8; 4] {
    let mut out = [0u8; 4];
    out.copy_from_slice(&bytes[off..off + 4]);
    out
}

/// Copies eight bytes out of a header buffer at `off`.
fn take8(bytes: &[u8], off: usize) -> [u8; 8] {
    let mut out = [0u8; 8];
    out.copy_from_slice(&bytes[off..off + 8]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SegmentHeader {
        SegmentHeader {
            segment_id: SegId::from_u64(0x1122_3344_5566_7788),
            session_id: SegId::from_u64(0x0011_2233_4455_6677),
            max_size: 4096,
            remaining: 19,
            format: Format::Fixed(64),
            sequence: SeqId::from_u64(5),
        }
    }

    #[test]
    fn header_is_fifty_three_bytes() {
        assert_eq!(SEGMENT_FILE_HEADER_LEN, 53);
        assert_eq!(sample().to_bytes().len(), 53);
    }

    #[test]
    fn fields_land_at_the_specified_offsets() {
        let h = sample();
        let b = h.to_bytes();
        assert_eq!(&b[0..8], b"tcslogsf");
        assert_eq!(&b[8..12], b"0010");
        assert_eq!(&b[12..20], &h.segment_id.to_le_bytes());
        assert_eq!(&b[20..28], &h.session_id.to_le_bytes());
        assert_eq!(&b[28..32], &h.max_size.to_le_bytes());
        assert_eq!(&b[32..40], &h.remaining.to_le_bytes());
        assert_eq!(b[40], 0);
        assert_eq!(&b[41..45], &64u32.to_le_bytes());
        assert_eq!(&b[45..53], &5u64.to_le_bytes());
    }

    #[test]
    fn round_trips_every_format() {
        for format in [
            Format::Fixed(1),
            Format::Fixed(RecSize::MAX),
            Format::VariableSimple,
            Format::VariableTsRc,
        ] {
            let mut h = sample();
            h.format = format;
            assert_eq!(SegmentHeader::from_bytes(&h.to_bytes()).unwrap(), h);
        }
    }

    #[test]
    fn stream_round_trip() {
        let h = sample();
        let mut buf = Vec::new();
        h.write_to(&mut buf).unwrap();
        // A trailing byte stands in for the data section: read_from must
        // stop at the end of the header and leave it alone.
        buf.push(0xAA);
        let mut src = buf.as_slice();
        assert_eq!(SegmentHeader::read_from(&mut src).unwrap(), h);
        assert_eq!(src, &[0xAA]);
    }

    #[test]
    fn rejects_wrong_magic() {
        let mut b = sample().to_bytes();
        b[0] = 0;
        assert!(matches!(
            SegmentHeader::from_bytes(&b),
            Err(LogError::InvalidHeader)
        ));
    }

    #[test]
    fn rejects_short_buffer() {
        let b = sample().to_bytes();
        assert!(matches!(
            SegmentHeader::from_bytes(&b[..HEADER_LEN - 1]),
            Err(LogError::InvalidHeader)
        ));
    }

    #[test]
    fn rejects_unknown_format_tag() {
        let mut b = sample().to_bytes();
        b[OFF_FORMAT_TAG] = 7;
        assert!(matches!(
            SegmentHeader::from_bytes(&b),
            Err(LogError::InvalidHeader)
        ));
    }

    #[test]
    fn rejects_fixed_with_zero_length() {
        let mut b = sample().to_bytes();
        b[OFF_FORMAT_TAG] = 0;
        b[OFF_FORMAT_ARG..OFF_FORMAT_ARG + 4].copy_from_slice(&0u32.to_le_bytes());
        assert!(matches!(
            SegmentHeader::from_bytes(&b),
            Err(LogError::InvalidHeader)
        ));
    }

    #[test]
    fn accepts_an_equal_or_lesser_minor_and_any_patch() {
        // The rule is a comparison of parsed numbers, not a match
        // against the literal string this build writes.
        for field in [b"0010", b"0000", b"0019"] {
            let mut b = sample().to_bytes();
            b[OFF_VERSION..OFF_VERSION + 4].copy_from_slice(field);
            assert!(
                SegmentHeader::from_bytes(&b).is_ok(),
                "refused readable version {:?}",
                std::str::from_utf8(field).unwrap()
            );
        }
    }

    #[test]
    fn refuses_greater_minor_other_major_and_non_digits() {
        for field in [b"0020", b"0110", b"9900", b"00a0", b"    "] {
            let mut b = sample().to_bytes();
            b[OFF_VERSION..OFF_VERSION + 4].copy_from_slice(field);
            assert!(
                matches!(
                    SegmentHeader::from_bytes(&b),
                    Err(LogError::VersionMismatch)
                ),
                "accepted unreadable version {:?}",
                std::str::from_utf8(field).unwrap()
            );
        }
    }

    #[test]
    fn data_section_length_subtracts_the_header() {
        assert_eq!(SegmentHeader::data_section_len(100), 47);
        // A stored max size smaller than a header cannot underflow.
        assert_eq!(SegmentHeader::data_section_len(0), 0);
    }
}
