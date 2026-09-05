//! On-disk segment file header layout.

use std::io::{Read, Write};

use crate::error::LogError;
use crate::format::Format;
use crate::segid::SegId;

/// ASCII magic tag stored at the start of every segment file.
pub const FILE_TYPE: &[u8; 8] = b"tcslogsf";

/// Segment-file format version corresponding to tcslog `0.1.0`.
/// The four ASCII digits encode `MMmp`: two-digit major, one-digit
/// minor, one-digit patch.
pub const VERSION: &[u8; 4] = b"0010";

/// Major version number of the on-disk format understood by this crate.
pub const VERSION_MAJOR: u8 = 0;
/// Minor version number of the on-disk format understood by this crate.
pub const VERSION_MINOR: u8 = 1;

/// Number of bytes the segment file header consumes on disk.
///
/// Layout (little-endian, tightly packed):
///
/// | offset | length | field       |
/// |-------:|-------:|:------------|
/// |      0 |      8 | file type   |
/// |      8 |      4 | version     |
/// |     12 |      8 | segment_id  |
/// |     20 |      8 | session_id  |
/// |     28 |      4 | max_size    |
/// |     32 |      8 | remaining   |
/// |     40 |      1 | format tag  |
/// |     41 |      4 | format arg  |
pub const SEGMENT_FILE_HEADER_LEN: u32 = 45;

/// In-memory representation of a segment file header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentHeader {
    /// Identifier of this segment file. Matches the segment portion of
    /// the file name.
    pub segment_id: SegId,
    /// Segment identifier of the first segment file created for the
    /// session this file belongs to.
    pub session_id: SegId,
    /// Maximum size in bytes that a segment file in this log may reach.
    pub max_size: u32,
    /// Number of bytes remaining in the data record whose first byte is
    /// the first byte of this segment file's data section. May exceed
    /// this segment's data section, in which case that record is
    /// continued in later segment files.
    pub remaining: u64,
    /// Layout used for records in the data section.
    pub format: Format,
}

impl SegmentHeader {
    /// Number of bytes usable for the data section of a segment file
    /// that is at most `max_size` bytes long.
    pub fn data_section_len(max_size: u32) -> u32 {
        max_size.saturating_sub(SEGMENT_FILE_HEADER_LEN)
    }

    /// Number of bytes available for records in this segment's data
    /// section.
    pub fn data_capacity(&self) -> u32 {
        Self::data_section_len(self.max_size)
    }

    /// Serializes the header into `SEGMENT_FILE_HEADER_LEN` bytes.
    pub fn to_bytes(&self) -> [u8; SEGMENT_FILE_HEADER_LEN as usize] {
        let mut buf = [0u8; SEGMENT_FILE_HEADER_LEN as usize];
        buf[0..8].copy_from_slice(FILE_TYPE);
        buf[8..12].copy_from_slice(VERSION);
        buf[12..20].copy_from_slice(&self.segment_id.to_le_bytes());
        buf[20..28].copy_from_slice(&self.session_id.to_le_bytes());
        buf[28..32].copy_from_slice(&self.max_size.to_le_bytes());
        buf[32..40].copy_from_slice(&self.remaining.to_le_bytes());
        buf[40] = self.format.tag();
        buf[41..45].copy_from_slice(&self.format.fixed_len().to_le_bytes());
        buf
    }

    /// Writes the header to `w` in on-disk form.
    pub fn write_to<W: Write>(&self, w: &mut W) -> Result<(), LogError> {
        let buf = self.to_bytes();
        w.write_all(&buf).map_err(LogError::IoError)
    }

    /// Deserializes a header from its on-disk byte encoding.
    pub fn from_bytes(buf: &[u8; SEGMENT_FILE_HEADER_LEN as usize]) -> Result<Self, LogError> {
        if &buf[0..8] != FILE_TYPE {
            return Err(LogError::InvalidHeader);
        }
        let version: [u8; 4] = buf[8..12].try_into().unwrap();
        if !version_is_compatible(&version) {
            return Err(LogError::VersionMismatch);
        }
        let segment_id = SegId::from_le_bytes(buf[12..20].try_into().unwrap());
        let session_id = SegId::from_le_bytes(buf[20..28].try_into().unwrap());
        let max_size = u32::from_le_bytes(buf[28..32].try_into().unwrap());
        let remaining = u64::from_le_bytes(buf[32..40].try_into().unwrap());
        let tag = buf[40];
        let arg = u32::from_le_bytes(buf[41..45].try_into().unwrap());
        let format = match tag {
            0 => {
                if arg == 0 {
                    return Err(LogError::InvalidHeader);
                }
                Format::Fixed(arg)
            }
            1 => Format::VariableSimple,
            2 => Format::VariableTsRc,
            _ => return Err(LogError::InvalidHeader),
        };
        Ok(SegmentHeader {
            segment_id,
            session_id,
            max_size,
            remaining,
            format,
        })
    }

    /// Reads a header from `r`.
    pub fn read_from<R: Read>(r: &mut R) -> Result<Self, LogError> {
        let mut buf = [0u8; SEGMENT_FILE_HEADER_LEN as usize];
        r.read_exact(&mut buf).map_err(LogError::IoError)?;
        Self::from_bytes(&buf)
    }
}

/// Returns `true` if a segment file with the given four-byte version
/// string can be read by this build of tcslog. The rule is: major must
/// match exactly; the file's minor must be less than or equal to the
/// crate's minor.
fn version_is_compatible(v: &[u8; 4]) -> bool {
    fn hex(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            _ => None,
        }
    }
    let (Some(h), Some(t), Some(m), Some(_p)) =
        (hex(v[0]), hex(v[1]), hex(v[2]), hex(v[3]))
    else {
        return false;
    };
    let major = (h << 4) | t;
    let minor = m;
    major == VERSION_MAJOR && minor <= VERSION_MINOR
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_variable_ts_rc() {
        let h = SegmentHeader {
            segment_id: SegId::from_u64(0x1111_2222_3333_4444),
            session_id: SegId::from_u64(0x1111_2222_3333_4444),
            max_size: 4096,
            remaining: 0,
            format: Format::VariableTsRc,
        };
        let bytes = h.to_bytes();
        let back = SegmentHeader::from_bytes(&bytes).unwrap();
        assert_eq!(h, back);
    }

    #[test]
    fn roundtrip_fixed() {
        let h = SegmentHeader {
            segment_id: SegId::from_u64(1),
            session_id: SegId::from_u64(1),
            max_size: 256,
            remaining: 17,
            format: Format::Fixed(64),
        };
        let bytes = h.to_bytes();
        let back = SegmentHeader::from_bytes(&bytes).unwrap();
        assert_eq!(h, back);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut bytes = SegmentHeader {
            segment_id: SegId::from_u64(1),
            session_id: SegId::from_u64(1),
            max_size: 256,
            remaining: 0,
            format: Format::VariableSimple,
        }
        .to_bytes();
        bytes[0] = b'X';
        assert!(matches!(
            SegmentHeader::from_bytes(&bytes),
            Err(LogError::InvalidHeader)
        ));
    }

    #[test]
    fn accepts_minor_zero() {
        assert!(version_is_compatible(b"0000"));
        assert!(version_is_compatible(b"0010"));
    }

    #[test]
    fn rejects_higher_minor() {
        assert!(!version_is_compatible(b"0020"));
    }

    #[test]
    fn rejects_higher_major() {
        assert!(!version_is_compatible(b"0100"));
    }
}
