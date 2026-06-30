//! Data block handling for log files.
//! FIXME: validate that no memory is allocated after the TcsLog is created

//use std::cmp::Ordering;

use crate::error::TcsLogError;
use crate::Timestamp;
use crate::Offset;
use crate::BLOCK_SIZE;
use crate::Filename;

/*
/// Block header indicating null pointer (does not reference a file).
pub const TCSLOG_NULL: u64 = 0x0000_0000_0000_0001;

/// Block header indicating next record starts at end of header.
pub const Offset::REC_START: u64 = 0x0000_0000_0000_0002;
*/

/// Size of block header in bytes.
pub const PACKLEN: usize = 8;

/// Size of a block header in bytes (same as [`BlockHeader::PACKLEN`]).
pub const BLOCK_HEADER_SIZE: usize = Offset::PACKLEN;

/// Size of record length field in bytes.
pub const RECORD_LENGTH_SIZE: usize = 8;

/// Size of timestamp field in bytes (the packed, on-disk size, which is
/// smaller than `size_of::<Timestamp>()` because of struct padding).
pub const RECORD_PACKLEN: usize = Timestamp::PACKLEN;

/// Size of record metadata (length + timestamp).
pub const RECORD_METADATA_SIZE: usize = RECORD_PACKLEN + RECORD_LENGTH_SIZE;

pub const CONT_SIZE: usize = Timestamp::PACKLEN + Filename::PACKLEN;

/// Maximum record size that can fit in a single data block.
pub const MAX_RECORD_SIZE: usize = BLOCK_SIZE - BlockHeader::PACKLEN - RECORD_METADATA_SIZE;

/// Represent a block header
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockHeader {
    offset: Offset
}

/*
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockHeader {
    /// Null pointer - does not reference a file.
    Null,
    /// Next record starts at end of header.
    NewRecord,
    /// Offset to end of current record (continuation from previous block).
    Continuation(u64),
}
*/

impl BlockHeader {
    /// Block header indicating null pointer (does not reference within a file).
    pub const NULL: BlockHeader = BlockHeader::new(Offset::NULL);

    /// Number of bytes in block header in the log file
    pub const PACKLEN: usize = Offset::PACKLEN;

    /// Block header indicating next record starts at end of header.
    pub const REC_START: BlockHeader = BlockHeader::new(Offset::REC_START);

    pub const fn new(offset: Offset) -> BlockHeader {
        BlockHeader { offset }
    }

/*
    pub const fn packlen(&self) -> usize {
        Offset::PACKLEN
        self.offset.packlen()
    }
*/

    /// Converts a block header into its on-disk format
    pub fn to_le_bytes(&self) -> [u8; Offset::PACKLEN] {
        self.offset.to_le_bytes()
    }

    pub fn from_le_bytes(buf: [u8; Offset::PACKLEN]) -> BlockHeader {
        BlockHeader { offset: Offset::from_le_bytes(buf) }
    }

    /// Converts the block header to its u64 representation.
    pub fn to_u64(&self) -> u64 {
        self.offset.into()
/*
        match self {
            BlockHeader::Null => Offset::NULL,
            BlockHeader::NewRecord => Offset::REC_START,
            BlockHeader::Continuation(offset) => *offset,
        }
*/
    }
}

#[derive(Debug, Clone, Copy)]
/*
 * This is the marker used to indicate the end of this log file. It has the
 * following fields:
 * 
 * timestamp    Time at which the EOF was written
 * next_file    Name of the next log file
 */
struct EofMarker {
    timestamp:  Timestamp,
    next_file:  Filename,
}

impl EofMarker {
    pub const PACKLEN: usize = Timestamp::PACKLEN + Filename::PACKLEN;

    pub const fn new(timestamp: Timestamp, next_file: Filename) -> EofMarker {
        EofMarker { timestamp, next_file }
    }

    // Convert an EOF marker to its packed, i.e. in-file, representation
    pub fn to_le_bytes(self) -> [u8; EofMarker::PACKLEN] {
        // Allocate a place to put the result
        let mut eof_marker = [0; Self::PACKLEN];

        // Copy in the timestamp bytes
        let mut i = 0;
        let a_timestamp = self.timestamp.to_le_bytes();
        eof_marker[i..Timestamp::PACKLEN].copy_from_slice(&a_timestamp);
        i += Timestamp::PACKLEN;

        // Copy in the file name
        let a_next_file = self.next_file.to_le_bytes();
        eof_marker[i..i + Filename::PACKLEN].copy_from_slice(&a_next_file);

        eof_marker
    }

    // Convert an EOF marker from the representation in the file to its
    // manipulatable in-memory representation
    pub fn from_le_bytes(buf: [u8; EofMarker::PACKLEN]) -> EofMarker {
        let mut i = 0;

        // First, pull out the timestamp
        let mut a_timestamp: [u8; Timestamp::PACKLEN] = [0; Timestamp::PACKLEN];
        a_timestamp.copy_from_slice(&buf[i..Timestamp::PACKLEN]);
        let timestamp = Timestamp::from_le_bytes(a_timestamp);

        // First, pull out the next_file
        let mut a_next_file: [u8; Filename::PACKLEN] = [0; Filename::PACKLEN];
        a_next_file.copy_from_slice(&buf[i..Filename::PACKLEN]);
        let next_file = Filename::from_le_bytes(a_next_file);

        EofMarker {
            timestamp,
            next_file,
        }
    }
}

/*
 * FIXME: needed?
impl From<EofMarker> for u64 {
    fn from(value: EofMarker) -> Self {
        value.timestamp
    }
}
*/

/*
 * FIXME: needed?
impl PartialEq for EofMarker {
    fn eq(&self, r: &EofMarker) -> bool {
        self.timestamp == r.timestamp
    }
}

impl PartialOrd for EofMarker {
    fn partial_cmp(&self, r: &EofMarker) -> Option<Ordering> {
        if self < r.eof_marker {
            Some(Ordering::Less)
        } else if self > r.eof_marker {
            Some(Ordering::Greater)
        } else {
            Some(Ordering::Equal)
        }
    }
}
*/

/// Represents a data record with its metadata.
#[derive(Debug, Clone)]
pub struct DataRecord {
    /// Timestamp when the record was written (nanoseconds since UNIX epoch).
    pub timestamp: Timestamp,
    /// The actual data payload.
    pub data: Vec<u8>,
}

impl DataRecord {
    /// Creates a new data record.
    pub fn new(timestamp: Timestamp, data: Vec<u8>) -> Self {
        DataRecord { timestamp, data }
    }

    /// Returns the total size of this record including metadata as it
    /// is stored in the file.
    pub fn packlen(&self) -> usize {
        RECORD_METADATA_SIZE + self.data.len()
    }

    /// Serializes the record metadata and data to bytes.
    pub fn to_le_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.packlen());
        bytes.extend_from_slice(&self.timestamp.to_le_bytes());
        bytes.extend_from_slice(&(self.data.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&self.data);
        bytes
    }

    /// Deserializes a record from bytes.
    pub fn from_le_bytes(bytes: &[u8]) -> Result<Self, TcsLogError<'_>> {
        if bytes.len() < RECORD_METADATA_SIZE {
            return Err(TcsLogError::InvalidFormat(
                "Record too short for metadata".to_string(),
            ));
        }

        let mut i = 0;

        // On-disk order matches `to_le_bytes` and `TcsLog::write`:
        // [timestamp][length][data].
        let timestamp = Timestamp::from_le_bytes(
            bytes[i..i + Timestamp::PACKLEN]
                .try_into()
                .map_err(|_| TcsLogError::InvalidFormat("Invalid timestamp".to_string()))?,
        );
        i += Timestamp::PACKLEN;

        let length = u64::from_le_bytes(
            bytes[i..i + RECORD_LENGTH_SIZE]
                .try_into()
                .map_err(|_| TcsLogError::InvalidFormat("Invalid record length".to_string()))?,
        ) as usize;
        i += RECORD_LENGTH_SIZE;
        debug_assert_eq!(i, RECORD_METADATA_SIZE);

        if bytes.len() < RECORD_METADATA_SIZE + length {
            return Err(TcsLogError::InvalidFormat(
                "Record data truncated".to_string(),
            ));
        }

        let data = bytes[RECORD_METADATA_SIZE..RECORD_METADATA_SIZE + length].to_vec();

        Ok(DataRecord { timestamp, data })
    }
}

/// Manages writing data records to blocks.
#[allow(unused)]
pub struct DataBlockWriter {
    /// Current block buffer.
    buffer: [u8; BLOCK_SIZE],
    /// Current write position within the block.
    position: usize,
    /// Whether a record is currently being written (spanning blocks).
    in_record: bool,
}

impl Default for DataBlockWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl DataBlockWriter {
    /// Creates a new data block writer.
    #[allow(unused)]
    pub fn new() -> Self {
        let mut buffer = [0u8; BLOCK_SIZE];
        // Initialize with Offset::REC_START header
        buffer[0..8].copy_from_slice(&Offset::REC_START.to_le_bytes());

        DataBlockWriter {
            buffer,
            position: BlockHeader::PACKLEN,
            in_record: false,
        }
    }

    /// Returns the remaining space in the current block.
    #[allow(unused)]
    pub fn remaining(&self) -> usize {
        BLOCK_SIZE - self.position
    }

    /// Returns true if the block is empty (only has header).
    #[allow(unused)]
    pub fn is_empty(&self) -> bool {
        self.position == BlockHeader::PACKLEN
    }

    /// Returns the current buffer.
    #[allow(unused)]
    pub fn buffer(&self) -> &[u8; BLOCK_SIZE] {
        &self.buffer
    }

    /// Resets the writer for a new block.
    #[allow(unused)]
    pub fn reset(&mut self) {
        self.buffer = [0u8; BLOCK_SIZE];
        self.buffer[0..8].copy_from_slice(&Offset::REC_START.to_le_bytes());
        self.position = BlockHeader::PACKLEN;
        self.in_record = false;
    }

    /// Writes data to the block, returning how many bytes were written.
    #[allow(unused)]
    pub fn write(&mut self, data: &[u8]) -> usize {
        let to_write = data.len().min(self.remaining());
        self.buffer[self.position..self.position + to_write].copy_from_slice(&data[..to_write]);
        self.position += to_write;
        to_write
    }

    /// Sets the continuation offset for when a record spans blocks.
    #[allow(unused)]
    pub fn set_continuation(&mut self, offset_in_block: usize) {
        let header_value = (offset_in_block as u64) & !0xFF;
        self.buffer[0..8].copy_from_slice(&header_value.to_le_bytes());
    }
}

/// Manages reading data records from blocks.
pub struct DataBlockReader {
    /// Current block buffer.
    buffer: [u8; BLOCK_SIZE],
    /// Current read position within the block.
    position: usize,
}

impl DataBlockReader {
    /// Creates a new data block reader from a buffer.
    #[allow(unused)]
    pub fn new(buffer: [u8; BLOCK_SIZE]) -> Self {
        DataBlockReader {
            buffer,
            position: BlockHeader::PACKLEN,
        }
    }

    /// Returns the block header.
    #[allow(unused)]
    pub fn header(&self) -> Result<BlockHeader, TcsLogError<'_>> {
        let offset = Offset::from_le_bytes(self.buffer[0..8].try_into().unwrap());
        Ok(BlockHeader::new(offset))
    }

    /// Returns the remaining bytes in the block.
    #[allow(unused)]
    pub fn remaining(&self) -> usize {
        BLOCK_SIZE - self.position
    }

    /// Reads bytes from the block.
    #[allow(unused)]
    pub fn read(&mut self, count: usize) -> &[u8] {
        let to_read = count.min(self.remaining());
        let start = self.position;
        self.position += to_read;
        &self.buffer[start..start + to_read]
    }

    /// Sets the read position.
    #[allow(unused)]
    pub fn set_position(&mut self, position: usize) {
        self.position = position.min(BLOCK_SIZE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_header_null() {
        let header = BlockHeader::from_le_bytes(Offset::NULL.to_le_bytes());
        assert_eq!(header, BlockHeader::NULL);
        assert_eq!(header.to_u64(), u64::from(Offset::NULL));
    }

    #[test]
    fn test_block_header_rec_start() {
        let header = BlockHeader::from_le_bytes(Offset::REC_START.to_le_bytes());
        assert_eq!(header, BlockHeader::REC_START);
        assert_eq!(header.to_u64(), u64::from(Offset::REC_START));
    }

    #[test]
    fn test_block_header_roundtrip() {
        let offset: u64 = 0x1234_5600;
        let header = BlockHeader::new(Offset::new_raw(offset));
        let restored = BlockHeader::from_le_bytes(header.to_le_bytes());
        assert_eq!(header, restored);
        assert_eq!(restored.to_u64(), offset);
    }

    #[test]
    fn test_data_record_roundtrip() {
        let record = DataRecord::new(Timestamp::from_nanos(12345678900), vec![1, 2, 3, 4, 5]);
        let bytes = record.to_le_bytes();
        let restored = DataRecord::from_le_bytes(&bytes).unwrap();

        assert_eq!(record.timestamp, restored.timestamp);
        assert_eq!(record.data, restored.data);
    }

    #[test]
    fn test_data_block_writer() {
        let mut writer = DataBlockWriter::new();
        assert!(writer.is_empty());
        assert_eq!(writer.remaining(), BLOCK_SIZE - BlockHeader::PACKLEN);

        let data = vec![1, 2, 3, 4, 5];
        let written = writer.write(&data);
        assert_eq!(written, 5);
        assert!(!writer.is_empty());
    }
}
