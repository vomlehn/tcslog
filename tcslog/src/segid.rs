//! Segment identifier: an unsigned 64-bit value formatted as
//! `xxxx-xxxx-xxxx-xxxx` (16 lowercase hex characters, dashes every four).

use std::fmt;
use std::str::FromStr;

/// Identifier of a segment file.
///
/// A `SegId` is a `u64`, but its `Display` and `FromStr` implementations use
/// the on-disk canonical form: sixteen lowercase hexadecimal characters with
/// a `-` separator every four characters, e.g. `1234-abcd-5678-efab`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SegId(u64);

/// Number of characters in a formatted segment ID (16 hex + 3 dashes).
pub const SEG_ID_STR_LEN: usize = 19;

impl SegId {
    /// The maximum representable segment ID (u64::MAX).
    pub const MAX: SegId = SegId(u64::MAX);

    /// The zero segment ID; useful as a sentinel in tests.
    pub const ZERO: SegId = SegId(0);

    /// Create a segment ID from its raw `u64` value.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Return the raw `u64` value.
    pub const fn as_u64(&self) -> u64 {
        self.0
    }
}

impl From<u64> for SegId {
    fn from(v: u64) -> Self {
        SegId(v)
    }
}

impl From<SegId> for u64 {
    fn from(v: SegId) -> Self {
        v.0
    }
}

impl fmt::Display for SegId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hex = format!("{:016x}", self.0);
        write!(
            f,
            "{}-{}-{}-{}",
            &hex[0..4],
            &hex[4..8],
            &hex[8..12],
            &hex[12..16],
        )
    }
}

/// Error returned by `SegId::from_str` for malformed input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseSegIdError;

impl fmt::Display for ParseSegIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid segment id: expected xxxx-xxxx-xxxx-xxxx")
    }
}

impl std::error::Error for ParseSegIdError {}

impl FromStr for SegId {
    type Err = ParseSegIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != SEG_ID_STR_LEN {
            return Err(ParseSegIdError);
        }
        let bytes = s.as_bytes();
        if bytes[4] != b'-' || bytes[9] != b'-' || bytes[14] != b'-' {
            return Err(ParseSegIdError);
        }
        let mut hex = String::with_capacity(16);
        for chunk in [&s[0..4], &s[5..9], &s[10..14], &s[15..19]] {
            for c in chunk.chars() {
                if !matches!(c, '0'..='9' | 'a'..='f') {
                    return Err(ParseSegIdError);
                }
                hex.push(c);
            }
        }
        u64::from_str_radix(&hex, 16)
            .map(SegId)
            .map_err(|_| ParseSegIdError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_zero() {
        assert_eq!(SegId::new(0).to_string(), "0000-0000-0000-0000");
    }

    #[test]
    fn display_example() {
        assert_eq!(
            SegId::new(0x1234_abcd_5678_efab).to_string(),
            "1234-abcd-5678-efab",
        );
    }

    #[test]
    fn roundtrip() {
        let s = "1234-abcd-5678-efab";
        let id: SegId = s.parse().unwrap();
        assert_eq!(id.as_u64(), 0x1234_abcd_5678_efab);
        assert_eq!(id.to_string(), s);
    }

    #[test]
    fn reject_wrong_length() {
        assert!("1234-abcd-5678-efa".parse::<SegId>().is_err());
        assert!("1234-abcd-5678-efabc".parse::<SegId>().is_err());
    }

    #[test]
    fn reject_bad_dashes() {
        assert!("1234abcd-5678-efab-1111".parse::<SegId>().is_err());
    }

    #[test]
    fn reject_non_hex() {
        assert!("1234-abcd-5678-efaG".parse::<SegId>().is_err());
        assert!("1234-abcd-5678-EFAB".parse::<SegId>().is_err());
    }
}
