//! Segment-within-session sequence numbers.

use std::fmt;

/// Zero-based index of a segment file within its session. A [`SeqId`]
/// is a 64-bit counter that starts at zero for the first segment of a
/// session and increments by one on each roll.
///
/// The `u64` width is chosen so that the counter cannot realistically
/// overflow during a single session; at one roll per microsecond it
/// would take more than half a million years to wrap.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SeqId(u64);

impl SeqId {
    /// The first sequence number in a session.
    pub const ZERO: SeqId = SeqId(0);

    /// Wraps a raw `u64` value as a [`SeqId`].
    #[must_use]
    pub const fn from_u64(v: u64) -> SeqId {
        SeqId(v)
    }

    /// Returns the underlying `u64`.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Returns the next sequence number, saturating at [`u64::MAX`].
    #[must_use]
    pub const fn saturating_next(self) -> SeqId {
        SeqId(self.0.saturating_add(1))
    }

    /// Little-endian byte encoding of the underlying `u64`.
    #[must_use]
    pub fn to_le_bytes(self) -> [u8; 8] {
        self.0.to_le_bytes()
    }

    /// Constructs a [`SeqId`] from its little-endian byte encoding.
    #[must_use]
    pub fn from_le_bytes(bytes: [u8; 8]) -> SeqId {
        SeqId(u64::from_le_bytes(bytes))
    }
}

impl fmt::Display for SeqId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_and_next() {
        assert_eq!(SeqId::ZERO.as_u64(), 0);
        assert_eq!(SeqId::ZERO.saturating_next(), SeqId::from_u64(1));
    }

    #[test]
    fn saturates_at_max() {
        let max = SeqId::from_u64(u64::MAX);
        assert_eq!(max.saturating_next(), max);
    }

    #[test]
    fn le_bytes_roundtrip() {
        let s = SeqId::from_u64(0x0102_0304_0506_0708);
        assert_eq!(SeqId::from_le_bytes(s.to_le_bytes()), s);
    }

    #[test]
    fn display_is_decimal() {
        assert_eq!(format!("{}", SeqId::from_u64(42)), "42");
    }
}
