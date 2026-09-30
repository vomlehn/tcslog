//! The segment header's sequence field.

use std::fmt;

/// Zero-based position of a segment file within its session.
///
/// Segment IDs are wall-clock timestamps and so cannot be counted; this
/// is the dense counter that can. A reader checks that each segment it
/// crosses into carries one more than the last, and that the first
/// segment it opens for a session carries zero. Those two checks are
/// what make a lost segment file visible even when it was lost on a
/// record boundary, where the `remaining` field of both neighbours
/// agrees and nothing else would show the gap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SeqId(u64);

impl SeqId {
    /// The value carried by the first segment file of a session.
    pub const ZERO: Self = Self(0);

    /// Wraps a raw counter value.
    ///
    /// * `value` -- the position within the session.
    ///
    /// Returns the corresponding `SeqId`.
    #[must_use]
    pub const fn from_u64(value: u64) -> Self {
        Self(value)
    }

    /// Unwraps the counter to its raw value.
    ///
    /// Returns the position within the session.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// The value for the segment file after this one.
    ///
    /// Saturates rather than wrapping: a counter that wrapped to zero
    /// would announce itself as the first segment of a session, which
    /// is the one thing a reader must be able to trust. A `u64` counter
    /// cannot reach the saturation point in any real session.
    ///
    /// Returns the next `SeqId`.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    /// Encodes the counter for the segment file header.
    ///
    /// Returns the eight little-endian bytes stored on disk.
    #[must_use]
    pub const fn to_le_bytes(self) -> [u8; 8] {
        self.0.to_le_bytes()
    }

    /// Decodes a counter from its stored form.
    ///
    /// * `bytes` -- the eight little-endian bytes read from a header.
    ///
    /// Returns the decoded `SeqId`.
    #[must_use]
    pub const fn from_le_bytes(bytes: [u8; 8]) -> Self {
        Self(u64::from_le_bytes(bytes))
    }
}

impl fmt::Display for SeqId {
    /// Renders the counter as a decimal number.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_opens_a_session() {
        assert_eq!(SeqId::ZERO.as_u64(), 0);
    }

    #[test]
    fn next_steps_by_one() {
        assert_eq!(SeqId::ZERO.next(), SeqId::from_u64(1));
    }

    #[test]
    fn next_saturates_rather_than_wrapping_to_zero() {
        let last = SeqId::from_u64(u64::MAX);
        assert_eq!(last.next(), last);
        assert_ne!(last.next(), SeqId::ZERO);
    }

    #[test]
    fn little_endian_round_trip() {
        let s = SeqId::from_u64(0x0102_0304_0506_0708);
        assert_eq!(SeqId::from_le_bytes(s.to_le_bytes()), s);
    }

    #[test]
    fn displays_as_decimal() {
        assert_eq!(SeqId::from_u64(17).to_string(), "17");
    }
}
