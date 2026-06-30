//! Time stamps. The tricky part is that I want to use a time stamp to
//! indicate a continuation to the next log file with a zero bits. This means
//! sacrificing a nanosecond from the over thirty million year range.

use std::cmp::Ordering;
use std::fmt;
use std::num::ParseIntError;
//use std::time::{SystemTime, UNIX_EPOCH};
use std::mem::size_of;

//pub const PACKLEN: usize = Timestamp::packlen();

/// Timestamp type (nanoseconds since UNIX epoch).
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct Timestamp {
    nanos: u32, // Must be 0 <= and < 1_000_000_000
    secs: u64,
}

impl Timestamp {
    pub const ZERO: Timestamp = Self::from_nanos_raw(Self::TIMESTAMP_OFFSET as u128);
    pub const MAX: Timestamp = Self::new_raw(0xffffffffffffffff, 999_999_998);
    pub const CONT: Timestamp = Self::from_nanos_raw(Self::TIMESTAMP_OFFSET as u128);
    pub const PACKLEN: usize = size_of::<u32>() + size_of::<u64>();

    // Number of nanoseconds to add so that continuation can be all zeros without
    // interfering with the rest of the range. The range becomes smaller
    // by this many nanoseconds, but this is tiny.
    const TIMESTAMP_OFFSET: u8 = 1;

    pub const fn new(mut secs: u64, nanos: u32) -> Timestamp {
        let mut nanos = nanos as u64 + Self::TIMESTAMP_OFFSET as u64;
        if nanos >= 1_000_000_000 {
            nanos -= 1_000_000_000;
            secs += 1;
        }
        Timestamp {
            nanos: nanos as u32,
            secs,
        }
    }

    const fn new_raw(secs: u64, nanos: u32) -> Timestamp {
        Timestamp { nanos, secs }
    }

    // Length when packed
    pub const fn packlen(&self) -> usize {
        size_of_val(&self.nanos) + size_of_val(&self.secs)
    }

    pub fn from_nanos(mut nanos_arg: u128) -> Timestamp {
        nanos_arg += Self::TIMESTAMP_OFFSET as u128;
        let nanos = (nanos_arg % 1_000_000_000) as u32;
        let secs = (nanos_arg / 1_000_000_000).try_into().unwrap();

        Timestamp { nanos, secs }
    }

    const fn from_nanos_raw(nanos_arg: u128) -> Timestamp {
        let nanos = (nanos_arg % 1_000_000_000) as u32;
        let secs = ((nanos_arg / 1_000_000_000) & 0xffff_ffff_ffff_ffff) as u64;

        Timestamp { nanos, secs }
    }

    pub const fn as_nanos(&self) -> u128 {
        self.as_nanos_raw() - Self::TIMESTAMP_OFFSET as u128
    }

    const fn as_nanos_raw(&self) -> u128 {
        self.secs as u128 * 1_000_000_000 + self.nanos as u128
    }

    pub fn to_le_bytes(&self) -> [u8; Self::PACKLEN] {
        let a_nanos = self.nanos.to_le_bytes();
        let a_secs = self.secs.to_le_bytes();

        let mut timestamp = [0; Self::PACKLEN];
        timestamp[..4].copy_from_slice(&a_nanos);
        timestamp[4..].copy_from_slice(&a_secs);

        timestamp
    }

    pub fn from_le_bytes(buf: [u8; Self::PACKLEN]) -> Timestamp {
        let mut a_secs: [u8; 8] = [0; 8];
        let mut a_nanos: [u8; 4] = [0; 4];

        a_nanos.copy_from_slice(&buf[..4]);
        a_secs.copy_from_slice(&buf[4..]);

        let nanos = u32::from_le_bytes(a_nanos);
        let secs = u64::from_le_bytes(a_secs);

        Timestamp { nanos, secs }
    }

    pub fn from_str_radix(src: &str, radix: u32) -> Result<Timestamp, ParseIntError> {
        let nanos = u128::from_str_radix(src, radix)?;
        Ok(Timestamp::from_nanos(nanos))
    }
}

impl PartialEq for Timestamp {
    fn eq(&self, r: &Timestamp) -> bool {
        let t = self.secs == r.secs && self.nanos == r.nanos;
        println!("eq self {:?} r {:?} t {:?}", *self, *r, t);
        t
    }
}

impl PartialOrd for Timestamp {
    fn partial_cmp(&self, r: &Timestamp) -> Option<Ordering> {
        let t = if self.secs < r.secs {
            Some(Ordering::Less)
        } else if self.secs > r.secs {
            Some(Ordering::Greater)
        } else if self.nanos < r.nanos {
            Some(Ordering::Less)
        } else if self.nanos > r.nanos {
            Some(Ordering::Greater)
        } else {
            Some(Ordering::Equal)
        };
        println!("partial_cmd self {:?} r {:?} t {:?}", *self, *r, t);
        t
    }
}

impl fmt::LowerHex for Timestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "{:08x}{:04x}", self.secs, self.nanos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bidir_u128() {
        let timestamp_in = Timestamp::from_nanos(0);
        let timestamp_out = timestamp_in.as_nanos();
        assert_eq!(0, timestamp_out);

        let timestamp_in = Timestamp::from_nanos(1);
        let timestamp_out = timestamp_in.as_nanos();
        assert_eq!(1, timestamp_out);
    }

    #[test]
    fn test_bidir_timestamp() {
        let timestamp_in = Timestamp::new(0, 0);
        let timestamp_out = timestamp_in.as_nanos();
        assert_eq!(0, timestamp_out);

        let timestamp_in = Timestamp::new(0, 999_999_999);
        let timestamp_out = timestamp_in.as_nanos();
        assert_eq!(999_999_999, timestamp_out);

        let timestamp_in = Timestamp::new(1, 999_999_999);
        let timestamp_out = timestamp_in.as_nanos();
        assert_eq!(1_999_999_999, timestamp_out);
    }

    #[test]
    fn test_bidir_overflow() {
        let timestamp_in = Timestamp::new(0xffff_ffff_ffff_ffff, 999_999_998);

        assert_eq!(timestamp_in, Timestamp::MAX);

        // FIXME: I'd to verify that this overflows, but it's not clear now
        // let timestamp_in = Timestamp::new(0xffff_ffff_ffff_ffff, 999_999_999);
    }
}
