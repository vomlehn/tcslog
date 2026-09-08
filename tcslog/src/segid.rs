//! Segment file identifiers and their fixed-width string encoding.

use std::fmt;

/// Identifier of a segment file. A `SegId` is a 64-bit value that is
/// rendered on disk as four groups of four lowercase hex digits separated
/// by dashes (for example, `1234-abcd-5678-efab`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SegId(u64);

impl SegId {
    /// The number of ASCII characters produced by the [`SegId`] string
    /// form. Sixteen hexadecimal digits plus three dashes.
    pub const STR_LEN: usize = 19;

    /// The largest representable [`SegId`] value.
    pub const MAX: SegId = SegId(u64::MAX);

    /// Wraps a raw `u64` value as a [`SegId`].
    pub const fn from_u64(v: u64) -> SegId {
        SegId(v)
    }

    /// Returns the underlying `u64`.
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Parses a segment identifier from its dashed hex string form.
    ///
    /// The input must be exactly [`STR_LEN`](Self::STR_LEN) bytes long and
    /// match the pattern `xxxx-xxxx-xxxx-xxxx` where each `x` is a
    /// lowercase hexadecimal digit.
    pub fn parse(s: &str) -> Option<SegId> {
        let bytes = s.as_bytes();
        if bytes.len() != Self::STR_LEN {
            return None;
        }
        if bytes[4] != b'-' || bytes[9] != b'-' || bytes[14] != b'-' {
            return None;
        }
        let mut v: u64 = 0;
        for &b in bytes.iter() {
            if b == b'-' {
                continue;
            }
            let digit = match b {
                b'0'..=b'9' => (b - b'0') as u64,
                b'a'..=b'f' => (b - b'a' + 10) as u64,
                _ => return None,
            };
            v = (v << 4) | digit;
        }
        Some(SegId(v))
    }

    /// Little-endian byte encoding of the underlying `u64`.
    pub fn to_le_bytes(self) -> [u8; 8] {
        self.0.to_le_bytes()
    }

    /// Constructs a [`SegId`] from its little-endian byte encoding.
    pub fn from_le_bytes(bytes: [u8; 8]) -> SegId {
        SegId(u64::from_le_bytes(bytes))
    }
}

impl fmt::Display for SegId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let v = self.0;
        write!(
            f,
            "{:04x}-{:04x}-{:04x}-{:04x}",
            ((v >> 48) & 0xFFFF) as u16,
            ((v >> 32) & 0xFFFF) as u16,
            ((v >> 16) & 0xFFFF) as u16,
            (v & 0xFFFF) as u16,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_roundtrip() {
        let id = SegId::from_u64(0x1234_abcd_5678_efabu64);
        let s = format!("{id}");
        assert_eq!(s, "1234-abcd-5678-efab");
        assert_eq!(s.len(), SegId::STR_LEN);
        assert_eq!(SegId::parse(&s), Some(id));
    }

    #[test]
    fn parse_rejects_uppercase() {
        assert!(SegId::parse("1234-ABCD-5678-EFAB").is_none());
    }

    #[test]
    fn parse_rejects_bad_length() {
        assert!(SegId::parse("1234-abcd-5678").is_none());
        assert!(SegId::parse("1234-abcd-5678-efab-0000").is_none());
    }

    #[test]
    fn parse_rejects_missing_dashes() {
        assert!(SegId::parse("1234_abcd_5678_efab").is_none());
    }

    #[test]
    fn zero() {
        assert_eq!(format!("{}", SegId::from_u64(0)), "0000-0000-0000-0000");
    }
}
