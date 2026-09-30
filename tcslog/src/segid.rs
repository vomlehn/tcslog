//! Segment file identifiers.

use std::fmt;

use crate::error::LogError;

/// Identifier of one segment file: nanoseconds since the UNIX epoch,
/// taken when the file was created.
///
/// A `SegId` increases with time but is not dense, so it cannot be used
/// to count segment files or to notice a gap between two of them. That
/// is what [`SeqId`](crate::SeqId), the segment header's sequence field,
/// is for. Nanoseconds that fit in a `u64` run to the year 2554, so the
/// identifiers of one log cannot collide within any plausible mission.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SegId(u64);

impl SegId {
    /// Number of characters in the string form: sixteen hexadecimal
    /// digits and the three dashes that group them.
    pub const STR_LEN: usize = 19;

    /// Wraps a raw nanosecond count as a segment ID.
    ///
    /// * `value` -- nanoseconds since the UNIX epoch.
    ///
    /// Returns the corresponding `SegId`.
    #[must_use]
    pub const fn from_u64(value: u64) -> Self {
        Self(value)
    }

    /// Unwraps the segment ID to its raw nanosecond count.
    ///
    /// Returns nanoseconds since the UNIX epoch.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Encodes the segment ID for the segment file header.
    ///
    /// Returns the eight little-endian bytes stored on disk.
    #[must_use]
    pub const fn to_le_bytes(self) -> [u8; 8] {
        self.0.to_le_bytes()
    }

    /// Decodes a segment ID from its stored form.
    ///
    /// * `bytes` -- the eight little-endian bytes read from a header.
    ///
    /// Returns the decoded `SegId`.
    #[must_use]
    pub const fn from_le_bytes(bytes: [u8; 8]) -> Self {
        Self(u64::from_le_bytes(bytes))
    }

    /// Parses the dashed hexadecimal form that appears in a segment file
    /// name, as written by [`Display`](fmt::Display).
    ///
    /// * `text` -- exactly [`STR_LEN`](Self::STR_LEN) characters:
    ///   four groups of four lower-case hexadecimal digits separated by
    ///   dashes.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::InvalidPathname`] if `text` is not in that
    /// form, since the text came from a file name that therefore names
    /// no segment file of this log.
    pub fn parse(text: &str) -> Result<Self, LogError> {
        let bytes = text.as_bytes();
        if bytes.len() != Self::STR_LEN {
            return Err(LogError::InvalidPathname);
        }
        let mut value: u64 = 0;
        for (i, &b) in bytes.iter().enumerate() {
            // Dashes fall after each group of four digits.
            if i % 5 == 4 {
                if b != b'-' {
                    return Err(LogError::InvalidPathname);
                }
                continue;
            }
            let digit = match b {
                b'0'..=b'9' => u64::from(b - b'0'),
                b'a'..=b'f' => u64::from(b - b'a') + 10,
                _ => return Err(LogError::InvalidPathname),
            };
            value = (value << 4) | digit;
        }
        Ok(Self(value))
    }
}

impl fmt::Display for SegId {
    /// Renders the identifier as four dash-separated groups of four
    /// lower-case hexadecimal digits, which is the form that appears
    /// between a segment file's prefix and suffix.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04x}-{:04x}-{:04x}-{:04x}",
            (self.0 >> 48) & 0xffff,
            (self.0 >> 32) & 0xffff,
            (self.0 >> 16) & 0xffff,
            self.0 & 0xffff,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_as_dashed_lowercase_hex() {
        let id = SegId::from_u64(0x1234_abcd_5678_efab);
        assert_eq!(id.to_string(), "1234-abcd-5678-efab");
        assert_eq!(id.to_string().len(), SegId::STR_LEN);
    }

    #[test]
    fn zero_is_fully_padded() {
        assert_eq!(SegId::from_u64(0).to_string(), "0000-0000-0000-0000");
    }

    #[test]
    fn parse_round_trips_display() {
        for raw in [0, 1, u64::MAX, 0x0fed_cba9_8765_4321] {
            let id = SegId::from_u64(raw);
            assert_eq!(SegId::parse(&id.to_string()).unwrap(), id);
        }
    }

    #[test]
    fn parse_rejects_malformed_text() {
        for bad in [
            "",
            "1234-abcd-5678-efa",
            "1234-abcd-5678-efabc",
            "1234_abcd_5678_efab",
            "1234-ABCD-5678-efab",
            "1234-abcd-5678-efag",
        ] {
            assert!(SegId::parse(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn little_endian_round_trip() {
        let id = SegId::from_u64(0x0123_4567_89ab_cdef);
        assert_eq!(SegId::from_le_bytes(id.to_le_bytes()), id);
    }

    #[test]
    fn ordering_follows_time() {
        assert!(SegId::from_u64(10) < SegId::from_u64(11));
    }
}
