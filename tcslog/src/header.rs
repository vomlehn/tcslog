//! On-disk segment file header.
//!
//! Every segment file begins with a fixed-size header whose layout is:
//!
//! | offset | size | field                                       |
//! |-------:|-----:|---------------------------------------------|
//! |      0 |    8 | ASCII magic: `tcslogsf`                     |
//! |      8 |    4 | ASCII version: e.g. `0010` for `0.1.0`      |
//! |     12 |    8 | `segment_id` (u64, little-endian)           |
//! |     20 |    8 | `session_id` (u64, little-endian)           |
//! |     28 |    4 | `max_size`   (u32, little-endian)           |
//! |     32 |    4 | `remaining`  (u32, little-endian)           |
//! |     36 |    1 | format tag: 0=Fixed, 1=VariableSimple, 2=VariableTsRc |
//! |     37 |    4 | `Fixed(n)` payload size (u32, LE); zero otherwise |
//!
//! All integers are packed little-endian; there is no padding.

use std::io::{Read, Write};

use crate::format::Format;
use crate::segid::SegId;
use crate::LogError;

/// Number of bytes occupied by a segment file header.
pub const SEGMENT_HEADER_SIZE: usize = 41;

/// ASCII magic that must appear at offset 0 of every segment file.
pub const MAGIC: &[u8; 8] = b"tcslogsf";

/// ASCII version string written into the segment header. `0010` corresponds
/// to semver `0.1.0`.
pub const VERSION: &[u8; 4] = b"0010";

/// Decoded segment file header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentHeader {
    /// Unique identifier of this segment file. Matches the ID encoded in the
    /// file name.
    pub segment_id: SegId,
    /// Identifier of the first segment file created for the writing session
    /// that produced this segment. Constant across every segment of the
    /// session, so readers can detect session boundaries.
    pub session_id: SegId,
    /// Maximum size in bytes that this segment file may grow to. Used by
    /// the reader to know the intended data-section length.
    pub max_size: u32,
    /// Number of bytes at the start of this segment's data section that are
    /// the continuation of a data record started in a previous segment.
    /// Zero when this segment begins cleanly at a record boundary.
    pub remaining: u32,
    /// Data format used by every record in this segment.
    pub format: Format,
}

impl SegmentHeader {
    /// Serialize this header into `w`.
    pub(crate) fn write_to<W: Write>(&self, mut w: W) -> Result<(), LogError> {
        let mut buf = [0u8; SEGMENT_HEADER_SIZE];
        let mut off = 0usize;

        buf[off..off + 8].copy_from_slice(MAGIC);
        off += 8;
        buf[off..off + 4].copy_from_slice(VERSION);
        off += 4;
        buf[off..off + 8].copy_from_slice(&self.segment_id.as_u64().to_le_bytes());
        off += 8;
        buf[off..off + 8].copy_from_slice(&self.session_id.as_u64().to_le_bytes());
        off += 8;
        buf[off..off + 4].copy_from_slice(&self.max_size.to_le_bytes());
        off += 4;
        buf[off..off + 4].copy_from_slice(&self.remaining.to_le_bytes());
        off += 4;
        buf[off] = self.format.tag();
        off += 1;
        buf[off..off + 4].copy_from_slice(&self.format.fixed_n().to_le_bytes());

        w.write_all(&buf)?;
        Ok(())
    }

    /// Deserialize a header from `r`, validating the magic and version.
    pub(crate) fn read_from<R: Read>(mut r: R) -> Result<Self, LogError> {
        let mut buf = [0u8; SEGMENT_HEADER_SIZE];
        r.read_exact(&mut buf)?;

        if &buf[0..8] != MAGIC {
            return Err(LogError::BadMagic);
        }
        let ver: [u8; 4] = buf[8..12].try_into().unwrap();
        if &ver != VERSION {
            return Err(LogError::IncompatibleVersion(ver));
        }

        let segment_id = SegId::new(u64::from_le_bytes(buf[12..20].try_into().unwrap()));
        let session_id = SegId::new(u64::from_le_bytes(buf[20..28].try_into().unwrap()));
        let max_size = u32::from_le_bytes(buf[28..32].try_into().unwrap());
        let remaining = u32::from_le_bytes(buf[32..36].try_into().unwrap());
        let tag = buf[36];
        let n = u32::from_le_bytes(buf[37..41].try_into().unwrap());
        let format = Format::from_tag_and_n(tag, n)
            .ok_or(LogError::InconsistentHeader("unknown format tag"))?;

        Ok(SegmentHeader {
            segment_id,
            session_id,
            max_size,
            remaining,
            format,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use std::io::Cursor;

    #[test]
    fn roundtrip_fixed() {
        let h = SegmentHeader {
            segment_id: SegId::new(0x1234_5678_9abc_def0),
            session_id: SegId::new(0x1234_5678_9abc_de00),
            max_size: 4096,
            remaining: 17,
            format: Format::Fixed(42),
        };
        let mut buf = Vec::new();
        h.write_to(&mut buf).unwrap();
        assert_eq!(buf.len(), SEGMENT_HEADER_SIZE);

        let mut cur = Cursor::new(buf);
        let got = SegmentHeader::read_from(&mut cur).unwrap();
        assert_eq!(got, h);
    }

    #[test]
    fn roundtrip_variable() {
        for fmt in [Format::VariableSimple, Format::VariableTsRc] {
            let h = SegmentHeader {
                segment_id: SegId::new(1),
                session_id: SegId::new(1),
                max_size: 128,
                remaining: 0,
                format: fmt,
            };
            let mut buf = Vec::new();
            h.write_to(&mut buf).unwrap();
            let got = SegmentHeader::read_from(Cursor::new(buf)).unwrap();
            assert_eq!(got, h);
        }
    }

    #[test]
    fn rejects_bad_magic() {
        let mut buf = vec![0u8; SEGMENT_HEADER_SIZE];
        buf[0..8].copy_from_slice(b"badmagic");
        buf[8..12].copy_from_slice(VERSION);
        let err = SegmentHeader::read_from(Cursor::new(buf)).unwrap_err();
        assert!(matches!(err, LogError::BadMagic));
    }

    #[test]
    fn rejects_bad_version() {
        let mut buf = vec![0u8; SEGMENT_HEADER_SIZE];
        buf[0..8].copy_from_slice(MAGIC);
        buf[8..12].copy_from_slice(b"9999");
        let err = SegmentHeader::read_from(Cursor::new(buf)).unwrap_err();
        assert!(matches!(err, LogError::IncompatibleVersion(_)));
    }
}
