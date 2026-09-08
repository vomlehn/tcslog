//! Time stamps. The tricky part is that I want to use a time stamp to
//! indicate a continuation to the next log file with a zero bits. This means
//! sacrificing a nanosecond from the over thirty million year range.

use std::cmp::Ordering;
use std::fmt;
use std::intrinsics::atomic_and;
use std::num::ParseIntError;
//use std::time::{SystemTime, UNIX_EPOCH};
use std::mem::size_of;

#include!("config.rs")
//pub const PACKLEN: usize = Uid::packlen();

/// UID Type  Each log file consist of a chain of files, with a chain ID running from
///  zero to 255. Each chain has segments, with a segmenent ID running from zero to 255.
/// Each time tcslog::new() is invoked, it will find the next unused chain ID, then
/// create segments from there. The segment size is that given when the TcsLog is created.
/// Thus, each log file can be up to segment size times 255.
/// 
/// Since logging will stop when 
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct Uid {
    seg_id:  SerialId;
    chain_id:   ChainId;
}

impl Uid {

    pub const PACKLEN: usize = size_of::<ChainId>() + size_of::<SerialId>();

    // Number of nanoseconds to add so that continuation can be all zeros without
    // interfering with the rest of the range. The range becomes smaller
    // by this many nanoseconds, but this is tiny.
    const Uid_OFFSET: u8 = 1;

    pub const fn new() -> Uid {
        Uid {0, 0}
    }


    /*
     * Increment to the next log segment
     */
    pub fn next_segment(&self) -> {
        if self.seg_id == UID::MAX {
            return TcsLogError::TooManySegments
        }
    }
    pub fn to_le_bytes(&self) -> [u8; Self::PACKLEN] {
        let a_seg_id = self.seg_id.to_le_bytes();
        let a_chain_id = self.chain_id.to_le_bytes();

        let mut Uid = [0; Self::PACKLEN];
        Uid[..4].copy_from_slice(&a_seg_id);
        Uid[4..].copy_from_slice(&a_chain_id);

        Uid
    }

    pub fn from_le_bytes(buf: [u8; Self::PACKLEN]) -> Uid {
        let mut a_chain_id: [u8; 8] = [0; 8];
        let mut a_seg_id: [u8; 4] = [0; 4];

        a_seg_id.copy_from_slice(&buf[..4]);
        a_chain_id.copy_from_slice(&buf[4..]);

        let seg_id = u32::from_le_bytes(a_seg_id);
        let chain_id = u64::from_le_bytes(a_chain_id);

        Uid { seg_id, chain_id }
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

/*
 * Convert the UID into a contiguous string of hex digits of a constant length
 */
impl fmt::LowerHex for Uid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "{:02x}{:02x}", self.chain_id, self.seg_id)
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
