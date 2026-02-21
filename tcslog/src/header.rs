//! Log file header block handling.

use crate::{BLOCK_SIZE, FILE_TIMESTAMP_LEN, FILE_TYPE, MAX_PREFIX_LEN, VERSION_00_01_00};
use crate::error::TcsLogError;
use crate::timestamp::Timestamp;

/// Size of the file type field in bytes.
pub const FILE_TYPE_SIZE: usize = 8;

/// Size of the version field in bytes.
pub const VERSION_SIZE: usize = 8;

/// Maximum length of the file name (excluding NUL terminator).
pub const FILE_NAME_MAX_LEN: usize = MAX_PREFIX_LEN + FILE_TIMESTAMP_LEN;

/// Size of the file name field including NUL terminator.
pub const FILE_NAME_SIZE: usize = FILE_NAME_MAX_LEN + 1;

/// Size of the index offset field in bytes.
pub const INDEX_OFFSET_SIZE: usize = 8;

/// Size of the data offset field in bytes.
pub const DATA_OFFSET_SIZE: usize = 8;

/// Header block size (same as BLOCK_SIZE).
pub const HEADER_SIZE: usize = BLOCK_SIZE;

/// Represents the header block of a log file.
#[derive(Debug, Clone)]
pub struct Header {
    /// File type identifier ("tcslog  ").
    pub file_type: [u8; FILE_TYPE_SIZE],
    /// Version string (e.g., "00.01.00").
    pub version: [u8; VERSION_SIZE],
    /// Timestamp in nanoseconds since UNIX epoch.
    pub timestamp: Timestamp,
    /// File name (up to 52 characters plus NUL).
    pub file_name: [u8; FILE_NAME_SIZE],
    /// Offset to the beginning of the index section.
    pub index_offset: u64,
    /// Offset to the beginning of the data section.
    pub data_offset: u64,
}

impl Header {
    /// Creates a new header with the given parameters.
    pub fn new(timestamp: Timestamp, index_offset: u64, data_offset: u64, file_name: &str) -> Self {
        let mut name_bytes = [0u8; FILE_NAME_SIZE];
        let name_len = file_name.len().min(FILE_NAME_MAX_LEN);
        name_bytes[..name_len].copy_from_slice(&file_name.as_bytes()[..name_len]);

        Header {
            file_type: *FILE_TYPE,
            version: *VERSION_00_01_00,
            timestamp,
            index_offset,
            data_offset,
            file_name: name_bytes,
        }
    }

    /// Serializes the header to a byte buffer.
    pub fn to_bytes(&self) -> [u8; HEADER_SIZE] {
        let mut buffer = [0u8; HEADER_SIZE];
        let mut offset = 0;

        // File type
        buffer[offset..offset + FILE_TYPE_SIZE].copy_from_slice(&self.file_type);
        offset += FILE_TYPE_SIZE;

        // Version
        buffer[offset..offset + VERSION_SIZE].copy_from_slice(&self.version);
        offset += VERSION_SIZE;

        // Timestamp (little-endian)
        buffer[offset..offset + Timestamp::TIMESTAMP_SIZE].copy_from_slice(&self.timestamp.to_le_bytes());
        offset += Timestamp::TIMESTAMP_SIZE;

        // Index offset (little-endian)
        buffer[offset..offset + INDEX_OFFSET_SIZE]
            .copy_from_slice(&self.index_offset.to_le_bytes());
        offset += INDEX_OFFSET_SIZE;

        // Data offset (little-endian)
        buffer[offset..offset + DATA_OFFSET_SIZE].copy_from_slice(&self.data_offset.to_le_bytes());
        offset += DATA_OFFSET_SIZE;

        // File name
        buffer[offset..offset + FILE_NAME_SIZE].copy_from_slice(&self.file_name);
//        offset += FILE_NAME_SIZE;

        buffer
    }

    /// Deserializes a header from a byte buffer.
    pub fn from_bytes(buffer: &[u8; HEADER_SIZE]) -> Result<Self, TcsLogError> {
        let mut offset = 0;

        // File type
        let mut file_type = [0u8; FILE_TYPE_SIZE];
        file_type.copy_from_slice(&buffer[offset..offset + FILE_TYPE_SIZE]);
        if &file_type != FILE_TYPE {
            return Err(TcsLogError::InvalidFormat(
                "Invalid file type identifier".to_string(),
            ));
        }
        offset += FILE_TYPE_SIZE;

        // Version
        let mut version = [0u8; VERSION_SIZE];
        version.copy_from_slice(&buffer[offset..offset + VERSION_SIZE]);
        offset += VERSION_SIZE;
println!("Header::from_bytes: version {version:?}, offset {offset}");

        // Timestamp
        let timestamp = Timestamp::from_le_bytes(
            buffer[offset..offset + Timestamp::TIMESTAMP_SIZE]
                .try_into()
                .map_err(|_| TcsLogError::InvalidFormat("Invalid timestamp".to_string()))?,
        );
        offset += Timestamp::TIMESTAMP_SIZE;

        // Index offset
        let index_offset = u64::from_le_bytes(
            buffer[offset..offset + INDEX_OFFSET_SIZE]
                .try_into()
                .map_err(|_| TcsLogError::InvalidFormat("Invalid index offset".to_string()))?,
        );
        offset += INDEX_OFFSET_SIZE;

        // Data offset
        let data_offset = u64::from_le_bytes(
            buffer[offset..offset + DATA_OFFSET_SIZE]
                .try_into()
                .map_err(|_| TcsLogError::InvalidFormat("Invalid data offset".to_string()))?,
        );
        offset += DATA_OFFSET_SIZE;

        // File name
        let mut file_name = [0u8; FILE_NAME_SIZE];
        file_name.copy_from_slice(&buffer[offset..offset + FILE_NAME_SIZE]);
//        offset += FILE_NAME_SIZE;

        Ok(Header {
            file_type,
            version,
            timestamp,
            index_offset,
            data_offset,
            file_name,
        })
    }

    /// Returns the file name as a string.
    pub fn file_name_str(&self) -> &str {
        let nul_pos = self
            .file_name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(FILE_NAME_MAX_LEN);
        std::str::from_utf8(&self.file_name[..nul_pos]).unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_roundtrip() {
        let header = Header::new(
            Timestamp::from_nanos(1234567890_000_000_000),
            HEADER_SIZE as u64,
            HEADER_SIZE as u64 + BLOCK_SIZE as u64,
            "test-0001_2345_6789_0abc",
        );

        let bytes = header.to_bytes();
        let restored = Header::from_bytes(&bytes).unwrap();

        assert_eq!(header.file_type, restored.file_type);
        assert_eq!(header.version, restored.version);
        assert_eq!(header.timestamp, restored.timestamp);
        assert_eq!(header.file_name, restored.file_name);
        assert_eq!(header.index_offset, restored.index_offset);
        assert_eq!(header.data_offset, restored.data_offset);
    }

    #[test]
    fn test_file_name_str() {
        let header = Header::new(Timestamp::ZERO, HEADER_SIZE as u64, HEADER_SIZE as u64, "test-file");
        assert_eq!(header.file_name_str(), "test-file");
    }
}
