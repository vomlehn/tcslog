//! Time stamps. The tricky part is that I want to use a time stamp to
//! indicate a continuation to the next log file with a zero bits. This means
//! sacrificing a nanosecond from the over thirty million year range.

use std::cmp::Ordering;
use std::fmt;
use std::num::ParseIntError;
//use std::time::{SystemTime, UNIX_EPOCH};
use std::mem::size_of;

//pub const PACKLEN: usize = Uid::packlen();

/// Uid type (nanoseconds since UNIX epoch).
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct Uid {
    nanos: u32, // Must be 0 <= and < 1_000_000_000
    secs: u64,
}

impl Uid {
    pub const ZERO: Uid = Self::from_nanos_raw(Self::Uid_OFFSET as u128);
    // The largest representable Uid: maximum seconds with the maximum
    // valid nanoseconds (which must be < 1_000_000_000). This equals
    // `Uid::new(u64::MAX, 999_999_998)` once the +1 offset is applied.
    pub const MAX: Uid = Self::new_raw(0xffffffffffffffff, 999_999_999);
    // The continuation marker: an all-zero-bits Uid. Real Uids are
    // produced through `from_nanos`, which adds `Uid_OFFSET`, so they can
    // never be all zero — making this value safe as a sentinel that ends a file
    // and points at the next file in the chain.
    pub const CONT: Uid = Self::new_raw(0, 0);
    pub const PACKLEN: usize = size_of::<u32>() + size_of::<u64>();

    // Number of nanoseconds to add so that continuation can be all zeros without
    // interfering with the rest of the range. The range becomes smaller
    // by this many nanoseconds, but this is tiny.
    const Uid_OFFSET: u8 = 1;

    pub const fn new(mut secs: u64, nanos: u32) -> Uid {
        let mut nanos = nanos as u64 + Self::Uid_OFFSET as u64;
        if nanos >= 1_000_000_000 {
            nanos -= 1_000_000_000;
            secs += 1;
        }
        Uid {
            nanos: nanos as u32,
            secs,
        }
    }

    const fn new_raw(secs: u64, nanos: u32) -> Uid {
        Uid { nanos, secs }
    }

    // Length when packed
    pub const fn packlen(&self) -> usize {
        size_of_val(&self.nanos) + size_of_val(&self.secs)
    }

    pub fn from_nanos(mut nanos_arg: u128) -> Uid {
        nanos_arg += Self::Uid_OFFSET as u128;
        let nanos = (nanos_arg % 1_000_000_000) as u32;
        let secs = (nanos_arg / 1_000_000_000).try_into().unwrap();

        Uid { nanos, secs }
    }

    const fn from_nanos_raw(nanos_arg: u128) -> Uid {
        let nanos = (nanos_arg % 1_000_000_000) as u32;
        let secs = ((nanos_arg / 1_000_000_000) & 0xffff_ffff_ffff_ffff) as u64;

        Uid { nanos, secs }
    }

    pub const fn as_nanos(&self) -> u128 {
        self.as_nanos_raw() - Self::Uid_OFFSET as u128
    }

    const fn as_nanos_raw(&self) -> u128 {
        self.secs as u128 * 1_000_000_000 + self.nanos as u128
    }

    /// The Uid in microseconds, truncating any sub-microsecond part.
    pub const fn as_micros(&self) -> u128 {
        self.as_nanos() / 1_000
    }

    /// Builds a Uid from a microsecond count.
    pub fn from_micros(micros: u128) -> Uid {
        Uid::from_nanos(micros * 1_000)
    }

    pub fn to_le_bytes(&self) -> [u8; Self::PACKLEN] {
        let a_nanos = self.nanos.to_le_bytes();
        let a_secs = self.secs.to_le_bytes();

        let mut Uid = [0; Self::PACKLEN];
        Uid[..4].copy_from_slice(&a_nanos);
        Uid[4..].copy_from_slice(&a_secs);

        Uid
    }

    pub fn from_le_bytes(buf: [u8; Self::PACKLEN]) -> Uid {
        let mut a_secs: [u8; 8] = [0; 8];
        let mut a_nanos: [u8; 4] = [0; 4];

        a_nanos.copy_from_slice(&buf[..4]);
        a_secs.copy_from_slice(&buf[4..]);

        let nanos = u32::from_le_bytes(a_nanos);
        let secs = u64::from_le_bytes(a_secs);

        Uid { nanos, secs }
    }

    pub fn from_str_radix(src: &str, radix: u32) -> Result<Uid, ParseIntError> {
        let nanos = u128::from_str_radix(src, radix)?;
        Ok(Uid::from_nanos(nanos))
    }
}

impl PartialEq for Uid {
    fn eq(&self, r: &Uid) -> bool {
        let t = self.secs == r.secs && self.nanos == r.nanos;
        println!("eq self {:?} r {:?} t {:?}", *self, *r, t);
        t
    }
}

impl PartialOrd for Uid {
    fn partial_cmp(&self, r: &Uid) -> Option<Ordering> {
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

impl fmt::LowerHex for Uid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "{:08x}{:04x}", self.secs, self.nanos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bidir_u128() {
        let Uid_in = Uid::from_nanos(0);
        let Uid_out = Uid_in.as_nanos();
        assert_eq!(0, Uid_out);

        let Uid_in = Uid::from_nanos(1);
        let Uid_out = Uid_in.as_nanos();
        assert_eq!(1, Uid_out);
    }

    #[test]
    fn test_bidir_Uid() {
        let Uid_in = Uid::new(0, 0);
        let Uid_out = Uid_in.as_nanos();
        assert_eq!(0, Uid_out);

        let Uid_in = Uid::new(0, 999_999_999);
        let Uid_out = Uid_in.as_nanos();
        assert_eq!(999_999_999, Uid_out);

        let Uid_in = Uid::new(1, 999_999_999);
        let Uid_out = Uid_in.as_nanos();
        assert_eq!(1_999_999_999, Uid_out);
    }

    #[test]
    fn test_bidir_overflow() {
        let Uid_in = Uid::new(0xffff_ffff_ffff_ffff, 999_999_998);

        assert_eq!(Uid_in, Uid::MAX);

        // FIXME: I'd to verify that this overflows, but it's not clear now
        // let Uid_in = Uid::new(0xffff_ffff_ffff_ffff, 999_999_999);
    }
}
