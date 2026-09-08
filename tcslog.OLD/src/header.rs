//! Log file header block handling.

use std::mem::size_of;

use crate::error::TcsLogError;
use crate::Offset;
use crate::uid::Uid;
use crate::{BLOCK_SIZE, Filename};

/// Represents the header block of a log file.
#[derive(Debug, Clone)]
pub struct Header {
    /// File type identifier ("tcslog  ").
    pub file_type: [u8; Self::FILE_TYPE_SIZE],
    /// Version string (e.g., "00.01.00").
    pub version: [u8; Self::VERSION_SIZE],
    /// Uid in nanoseconds since UNIX epoch.
    pub Uid: Uid,
    /// Offset to the beginning of the index section.
    pub index_offset: u64,
    /// Offset to the beginning of the data section.
    pub data_offset: u64,
    /// File name (up to 52 characters plus NUL).
    pub file_name: [u8; Filename::PACKLEN],
    /// Number of files in the log file chain. Starts at zero.
    pub chain_count: u32,
}

impl Header {
    /// File type identifier.
    pub const FILE_TYPE: &[u8; 8] = b"tcslog  ";

    /// Version string (major.minor.patch).
    pub const VERSION_00_01_00: &[u8; 8] = b"00.01.00";

    /// Size of the file type field in bytes.
    pub const FILE_TYPE_SIZE: usize = 8;

    /// Size of the version field in bytes.
    pub const VERSION_SIZE: usize = 8;

    /// Size of the file name field including NUL terminator.
    //pub const MAX_FILENAME_SIZE: usize = MAX_FILENAME_SIZE + 1;

    /// Size of the index offset field in bytes.
    pub const INDEX_OFFSET_PACKLEN: usize = Offset::PACKLEN;

    /// Size of the data offset field in bytes.
    pub const DATA_OFFSET_PACKLEN: usize = Offset::PACKLEN;

    /// Size of the chain count field in bytes.
    pub const CHAIN_COUNT_SIZE: usize = size_of::<u32>();

    /// Header block size (same as BLOCK_SIZE).
    pub const HEADER_SIZE: usize = BLOCK_SIZE;

    /// Creates a new header with the given parameters.
    pub fn new(Uid: Uid, index_offset: u64, data_offset: u64, file_name: &str) -> Self {
        let mut name_bytes = [0u8; Filename::PACKLEN];
        let name_len = file_name.len().min(Filename::PACKLEN);
        name_bytes[..name_len].copy_from_slice(&file_name.as_bytes()[..name_len]);

        Header {
            file_type: *Self::FILE_TYPE,
            version: *Self::VERSION_00_01_00,
            Uid,
            index_offset,
            data_offset,
            file_name: name_bytes,
            chain_count: 0,
        }
    }

    /// Serializes the header to a byte buffer.
    pub fn to_bytes(&self) -> [u8; Self::HEADER_SIZE] {
        let mut buffer = [0u8; Self::HEADER_SIZE];
        let mut offset = 0;

        // File type
        buffer[offset..offset + Self::FILE_TYPE_SIZE].copy_from_slice(&self.file_type);
        offset += Self::FILE_TYPE_SIZE;

        // Version
        buffer[offset..offset + Self::VERSION_SIZE].copy_from_slice(&self.version);
        offset += Self::VERSION_SIZE;

        // Uid (little-endian)
        buffer[offset..offset + Uid::PACKLEN]
            .copy_from_slice(&self.Uid.to_le_bytes());
        offset += Uid::PACKLEN;

        // Index offset (little-endian)
        buffer[offset..offset + Self::INDEX_OFFSET_PACKLEN]
            .copy_from_slice(&self.index_offset.to_le_bytes());
        offset += Self::INDEX_OFFSET_PACKLEN;

        // Data offset (little-endian)
        buffer[offset..offset + Self::DATA_OFFSET_PACKLEN].copy_from_slice(&self.data_offset.to_le_bytes());
        offset += Self::DATA_OFFSET_PACKLEN;

        // File name
        buffer[offset..offset + Filename::PACKLEN].copy_from_slice(&self.file_name);
        offset += Filename::PACKLEN;

        // Chain count (little-endian)
        buffer[offset..offset + Self::CHAIN_COUNT_SIZE]
            .copy_from_slice(&self.chain_count.to_le_bytes());

        buffer
    }

    /// Deserializes a header from a byte buffer.
    pub fn from_bytes(buffer: &[u8; Self::HEADER_SIZE]) -> Result<Self, TcsLogError<'static>> {
        let mut offset = 0;

        // File type
        let mut file_type = [0u8; Self::FILE_TYPE_SIZE];
        file_type.copy_from_slice(&buffer[offset..offset + Self::FILE_TYPE_SIZE]);
        if &file_type != Self::FILE_TYPE {
            return Err(TcsLogError::InvalidFormat(
                "Invalid file type identifier".to_string(),
            ));
        }
        offset += Self::FILE_TYPE_SIZE;

        // Version
        let mut version = [0u8; Self::VERSION_SIZE];
        version.copy_from_slice(&buffer[offset..offset + Self::VERSION_SIZE]);
        offset += Self::VERSION_SIZE;

        // Uid
        let Uid = Uid::from_le_bytes(
            buffer[offset..offset + Uid::PACKLEN]
                .try_into()
                .map_err(|_| TcsLogError::InvalidFormat("Invalid Uid".to_string()))?,
        );
        offset += Uid::PACKLEN;

        // Index offset
        let index_offset = u64::from_le_bytes(
            buffer[offset..offset + Self::INDEX_OFFSET_PACKLEN]
                .try_into()
                .map_err(|_| TcsLogError::InvalidFormat("Invalid index offset".to_string()))?,
        );
        offset += Self::INDEX_OFFSET_PACKLEN;

        // Data offset
        let data_offset = u64::from_le_bytes(
            buffer[offset..offset + Self::DATA_OFFSET_PACKLEN]
                .try_into()
                .map_err(|_| TcsLogError::InvalidFormat("Invalid data offset".to_string()))?,
        );
        offset += Self::DATA_OFFSET_PACKLEN;
        println!("header::frombytes: index offset {index_offset} data_offset {data_offset}");

        // File name
        let mut file_name = [0u8; Filename::PACKLEN];
        file_name.copy_from_slice(&buffer[offset..offset + Filename::PACKLEN]);
        offset += Filename::PACKLEN;

        // Chain count
        let chain_count = u32::from_le_bytes(
            buffer[offset..offset + Self::CHAIN_COUNT_SIZE]
                .try_into()
                .map_err(|_| TcsLogError::InvalidFormat("Invalid chain count".to_string()))?,
        );

        Ok(Header {
            file_type,
            version,
            Uid,
            index_offset,
            data_offset,
            file_name,
            chain_count,
        })
    }

    /// Returns the file name as a string.
    pub fn file_name_str(&self) -> &str {
        let nul_pos = self
            .file_name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(Filename::PACKLEN);
        std::str::from_utf8(&self.file_name[..nul_pos]).unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_roundtrip() {
        let header = Header::new(
            Uid::from_nanos(1234567890_000_000_000),
            Header::HEADER_SIZE as u64,
            Header::HEADER_SIZE as u64 + BLOCK_SIZE as u64,
            "test-0001_2345_6789_0abc",
        );

        let bytes = header.to_bytes();
        let restored = Header::from_bytes(&bytes).unwrap();

        assert_eq!(header.file_type, restored.file_type);
        assert_eq!(header.version, restored.version);
        assert_eq!(header.Uid, restored.Uid);
        assert_eq!(header.file_name, restored.file_name);
        assert_eq!(header.index_offset, restored.index_offset);
        assert_eq!(header.data_offset, restored.data_offset);
        assert_eq!(header.chain_count, restored.chain_count);
    }

    #[test]
    fn test_chain_count_default_zero() {
        let header = Header::new(Uid::ZERO, 0, 0, "test");
        assert_eq!(header.chain_count, 0);
    }

    #[test]
    fn test_file_name_str() {
        let header = Header::new(
            Uid::ZERO,
            Header::HEADER_SIZE as u64,
            Header::HEADER_SIZE as u64,
            "test-file",
        );
        assert_eq!(header.file_name_str(), "test-file");
    }
}
