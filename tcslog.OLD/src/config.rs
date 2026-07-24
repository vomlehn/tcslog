/*
 * Values that are system dependent
 */

/// Block size in bytes (4KB).
pub const BLOCK_SIZE: usize = 4096;

// Microseconds to wait for time to advance enough
// that the log file name we generate is unique.
// This should be at least as larget as the minimum
// resolution of the system timer
pub const WAIT_FOR_NEW_NAME: u64 = 100;

// Maximum number of retries for the time to
// change enough that a new name is generated
pub const MAX_RETRIES: u32 = 10;

type ChainId = u8;
type SerialId = u8;