//! Data block handling for log files.

use crate::error::TcsLogError;
use crate::BLOCK_SIZE;

/// Block header indicating null pointer (does not reference a file).
pub const TCSLOG_NULL: u64 = 0x0000_0000_0000_0000;

/// Block header indicating next record starts at end of header.
pub const TCSLOG_REC: u64 = 0x0000_0000_0000_0001;

/// Size of block header in bytes.
pub const BLOCK_HEADER_SIZE: usize = 8;

/// Size of record length field in bytes.
pub const RECORD_LENGTH_SIZE: usize = 8;

/// Size of timestamp field in bytes.
pub const RECORD_TIMESTAMP_SIZE: usize = 8;

/// Size of record metadata (length + timestamp).
pub const RECORD_METADATA_SIZE: usize = RECORD_LENGTH_SIZE + RECORD_TIMESTAMP_SIZE;

/// Maximum record size that can fit in a single data block.
pub const MAX_RECORD_SIZE: usize = BLOCK_SIZE - BLOCK_HEADER_SIZE - RECORD_METADATA_SIZE;

/// Parses a block header value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockHeader {
    /// Null pointer - does not reference a file.
    Null,
    /// Next record starts at end of header.
    NewRecord,
    /// Offset to end of current record (continuation from previous block).
    Continuation(u64),
}

impl BlockHeader {
    /// Parses a block header from its u64 representation.
    pub fn from_u64(value: u64) -> Result<Self, TcsLogError> {
        let lower_byte = value & 0xFF;
        let upper_bytes = value >> 8;

        match (upper_bytes, lower_byte) {
            (0, 0) => Ok(BlockHeader::Null),
            (0, 1) => Ok(BlockHeader::NewRecord),
            (offset, 0) if offset > 0 => Ok(BlockHeader::Continuation(offset << 8)),
            _ => Err(TcsLogError::InvalidFormat(format!(
                "Invalid block header: 0x{:016X}",
                value
            ))),
        }
    }

    /// Converts the block header to its u64 representation.
    pub fn to_u64(&self) -> u64 {
        match self {
            BlockHeader::Null => TCSLOG_NULL,
            BlockHeader::NewRecord => TCSLOG_REC,
            BlockHeader::Continuation(offset) => *offset,
        }
    }
}

/// Represents a data record with its metadata.
#[derive(Debug, Clone)]
pub struct DataRecord {
    /// Timestamp when the record was written (nanoseconds since UNIX epoch).
    pub timestamp: u64,
    /// The actual data payload.
    pub data: Vec<u8>,
}

impl DataRecord {
    /// Creates a new data record.
    pub fn new(timestamp: u64, data: Vec<u8>) -> Self {
        DataRecord { timestamp, data }
    }

    /// Returns the total size of this record including metadata.
    pub fn total_size(&self) -> usize {
        RECORD_METADATA_SIZE + self.data.len()
    }

    /// Serializes the record metadata and data to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.total_size());
        bytes.extend_from_slice(&(self.data.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&self.timestamp.to_le_bytes());
        bytes.extend_from_slice(&self.data);
        bytes
    }

    /// Deserializes a record from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, TcsLogError> {
        if bytes.len() < RECORD_METADATA_SIZE {
            return Err(TcsLogError::InvalidFormat(
                "Record too short for metadata".to_string(),
            ));
        }

        let length =
            u64::from_le_bytes(bytes[0..8].try_into().map_err(|_| {
                TcsLogError::InvalidFormat("Invalid record length".to_string())
            })?) as usize;

        let timestamp = u64::from_le_bytes(
            bytes[8..16]
                .try_into()
                .map_err(|_| TcsLogError::InvalidFormat("Invalid timestamp".to_string()))?,
        );

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
    pub fn new() -> Self {
        let mut buffer = [0u8; BLOCK_SIZE];
        // Initialize with TCSLOG_REC header
        buffer[0..8].copy_from_slice(&TCSLOG_REC.to_le_bytes());

        DataBlockWriter {
            buffer,
            position: BLOCK_HEADER_SIZE,
            in_record: false,
        }
    }

    /// Returns the remaining space in the current block.
    pub fn remaining(&self) -> usize {
        BLOCK_SIZE - self.position
    }

    /// Returns true if the block is empty (only has header).
    pub fn is_empty(&self) -> bool {
        self.position == BLOCK_HEADER_SIZE
    }

    /// Returns the current buffer.
    pub fn buffer(&self) -> &[u8; BLOCK_SIZE] {
        &self.buffer
    }

    /// Resets the writer for a new block.
    pub fn reset(&mut self) {
        self.buffer = [0u8; BLOCK_SIZE];
        self.buffer[0..8].copy_from_slice(&TCSLOG_REC.to_le_bytes());
        self.position = BLOCK_HEADER_SIZE;
        self.in_record = false;
    }

    /// Writes data to the block, returning how many bytes were written.
    pub fn write(&mut self, data: &[u8]) -> usize {
        let to_write = data.len().min(self.remaining());
        self.buffer[self.position..self.position + to_write].copy_from_slice(&data[..to_write]);
        self.position += to_write;
        to_write
    }

    /// Sets the continuation offset for when a record spans blocks.
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
    pub fn new(buffer: [u8; BLOCK_SIZE]) -> Self {
        DataBlockReader {
            buffer,
            position: BLOCK_HEADER_SIZE,
        }
    }

    /// Returns the block header.
    pub fn header(&self) -> Result<BlockHeader, TcsLogError> {
        let value = u64::from_le_bytes(self.buffer[0..8].try_into().unwrap());
        BlockHeader::from_u64(value)
    }

    /// Returns the remaining bytes in the block.
    pub fn remaining(&self) -> usize {
        BLOCK_SIZE - self.position
    }

    /// Reads bytes from the block.
    pub fn read(&mut self, count: usize) -> &[u8] {
        let to_read = count.min(self.remaining());
        let start = self.position;
        self.position += to_read;
        &self.buffer[start..start + to_read]
    }

    /// Sets the read position.
    pub fn set_position(&mut self, position: usize) {
        self.position = position.min(BLOCK_SIZE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_header_null() {
        let header = BlockHeader::from_u64(TCSLOG_NULL).unwrap();
        assert_eq!(header, BlockHeader::Null);
        assert_eq!(header.to_u64(), TCSLOG_NULL);
    }

    #[test]
    fn test_block_header_new_record() {
        let header = BlockHeader::from_u64(TCSLOG_REC).unwrap();
        assert_eq!(header, BlockHeader::NewRecord);
        assert_eq!(header.to_u64(), TCSLOG_REC);
    }

    #[test]
    fn test_block_header_continuation() {
        let offset: u64 = 0x1234_5600;
        let header = BlockHeader::from_u64(offset).unwrap();
        assert_eq!(header, BlockHeader::Continuation(offset));
    }

    #[test]
    fn test_data_record_roundtrip() {
        let record = DataRecord::new(12345678900, vec![1, 2, 3, 4, 5]);
        let bytes = record.to_bytes();
        let restored = DataRecord::from_bytes(&bytes).unwrap();

        assert_eq!(record.timestamp, restored.timestamp);
        assert_eq!(record.data, restored.data);
    }

    #[test]
    fn test_data_block_writer() {
        let mut writer = DataBlockWriter::new();
        assert!(writer.is_empty());
        assert_eq!(writer.remaining(), BLOCK_SIZE - BLOCK_HEADER_SIZE);

        let data = vec![1, 2, 3, 4, 5];
        let written = writer.write(&data);
        assert_eq!(written, 5);
        assert!(!writer.is_empty());
    }
}
