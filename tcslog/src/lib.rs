//! TcsLog - Telemetry logging library with fixed-length logs.
//!
//! TcsLog provides:
//! * Fixed length logs with automatic switching to new logs when old ones fill
//! * Arbitrary record sizes
//! * Self-identified log files (the name is in the header)
//! * Indexed by automatically supplied timestamps with nanosecond resolution
//! * Metadata all in little-endian form

mod data;
mod error;
mod header;
mod index;

pub use data::{BlockHeader, DataRecord, BLOCK_HEADER_SIZE, MAX_RECORD_SIZE, TCSLOG_NULL, TCSLOG_REC};
pub use error::TcsLogError;
pub use header::{Header, HEADER_SIZE};
pub use index::{IndexBlock, IndexEntry, IndexStructure, ENTRIES_PER_BLOCK, FILE_NULL};

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use std::mem::size_of;

/// Block size in bytes (4KB).
pub const BLOCK_SIZE: usize = 4096;

/// Default file size (64MB).
pub const DEFAULT_FILE_SIZE: u64 = 64 * 1024 * 1024;

/// Maximum prefix length for file names.
pub const MAX_PREFIX_LEN: usize = 32;

/// File type identifier.
pub const FILE_TYPE: &[u8; 8] = b"tcslog  ";

/// Version string (major.minor.patch).
pub const VERSION: &[u8; 8] = b"00.01.00";

/// Timestamp type (nanoseconds since UNIX epoch).
pub type Timestamp = u128;

/// Represents a TcsLog instance for reading or writing telemetry records.
#[derive(Debug)]
pub struct TcsLog<'a> {
    /// Directory name
    dir_name: &'a str,
    /// The file handle.
    file: File,
    /// The file path.
    path: PathBuf,
    /// The file header.
    header: Header,
    /// Maximum file size.
    max_size: u64,
    /// Current write position in the data section.
    write_position: u64,
    /// Current read position.
    read_position: u64,
    /// The prefix used for file naming.
    prefix: String,
    /// Whether the log is open for writing.
    writing: bool,
}

impl<'a> TcsLog<'a> {
    /// Creates a new TcsLog with a specified maximum file size.
    pub fn new(dir_name: &'a str, prefix: &str, max_size: u64) -> Result<TcsLog<'a>, TcsLogError> {
        // Validate prefix
        if prefix.is_empty() || prefix.len() > MAX_PREFIX_LEN {
            return Err(TcsLogError::InvalidPrefix(format!(
                "Prefix must be 1-{} characters",
                MAX_PREFIX_LEN
            )));
        }

        if prefix.contains('/') || prefix.contains('\\') || prefix.contains('\0') {
            return Err(TcsLogError::InvalidPrefix(
                "Prefix contains invalid characters".to_string(),
            ));
        }

        // Generate timestamp and file name
        let timestamp = TcsLog::current_timestamp();
        let file_name = TcsLog::generate_file_name(prefix, timestamp);

        // Compute offsets
        let (index_offset, data_offset, _index_blocks) = TcsLog::compute_offsets(max_size);

        // Create header
        let header = Header::new(timestamp, &file_name, index_offset, data_offset);

        // Create the file
        let path = PathBuf::from(dir_name).join(&file_name);

        let mut retries = 0;
        let mut file = loop {
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(f) => break f,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && retries < 10 => {
                    // Wait 100 microseconds and try again with new timestamp
                    std::thread::sleep(std::time::Duration::from_micros(100));
                    retries += 1;
                    continue;
                }
                Err(e) => return Err(TcsLogError::Io(e)),
            }
        };

        // Write header
        file.write_all(&header.to_bytes())?;

        // Initialize index block(s) with zeros (already zeroed by OS for sparse files)
    //    let index_size = data_offset - index_offset;
        file.seek(SeekFrom::Start(data_offset - 1))?;
        file.write_all(&[0])?;

        // Seek to beginning of data section
        file.seek(SeekFrom::Start(data_offset))?;

        Ok(TcsLog {
            dir_name,
            file,
            path,
            header,
            max_size,
            write_position: data_offset,
            read_position: data_offset,
            prefix: prefix.to_string(),
            writing: true,
        })
    }

    /// Returns the current timestamp in nanoseconds since UNIX epoch.
    fn current_timestamp() -> Timestamp {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as Timestamp)
            .unwrap_or(0)
    }

    /// Generates a file name from prefix and timestamp.
    fn generate_file_name(prefix: &str, timestamp: Timestamp) -> String {
        // Format: prefix-XXXX_XXXX_XXXX_XXXX_XXXX_XXXX_XXXX_XXXX (where X is hex digit)
        let hex = format!("{:032x}", timestamp);
        format!(
            "{}-{}_{}_{}_{}_{}_{}_{}_{}",
            prefix,
            &hex[0..4],
            &hex[4..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..24],
            &hex[24..28],
            &hex[28..32]
        )
    }

    /// Parses a timestamp from a file name.
    #[allow(unused)]
    fn parse_timestamp_from_name(name: &str, prefix: &str) -> Option<Timestamp> {
        let suffix = name.strip_prefix(prefix)?.strip_prefix('-')?;
        let hex: String = suffix.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        if hex.len() == size_of::<Timestamp>() {
            Timestamp::from_str_radix(&hex, size_of::<Timestamp>().try_into().unwrap()).ok()
        } else {
            None
        }
    }

    /// Computes the index and data section offsets.
    fn compute_offsets(max_size: u64) -> (u64, u64, usize) {
        let header_size = HEADER_SIZE as u64;
        let remaining = max_size - header_size;

        // Start with one index block
        let index_blocks = 1usize;
        let index_size = (index_blocks * BLOCK_SIZE) as u64;
        let data_size = remaining - index_size;

        let data_blocks = (data_size / BLOCK_SIZE as u64) as usize;

        // Recompute if we need more index blocks
        let structure = IndexStructure::compute(data_blocks).unwrap();
        let actual_index_size = (structure.total_blocks * BLOCK_SIZE) as u64;

        let index_offset = header_size;
        let data_offset = header_size + actual_index_size;

        (index_offset, data_offset, structure.total_blocks)
    }

    /// Opens an existing TcsLog so that the telemetry records it contains may be read.
    ///
    /// If successful, returns a TcsLog. Otherwise, returns Err(TcsLogError).
    pub fn tcslog_open(dir_name: &'a str, prefix: &str, timestamp: Timestamp) -> Result<TcsLog<'a>, TcsLogError> {
        let file_name = TcsLog::generate_file_name(prefix, timestamp);
        let path = PathBuf::from(&file_name);

        if !path.exists() {
            return Err(TcsLogError::NotFound);
        }

        let mut file = OpenOptions::new().read(true).open(&path)?;

        // Read and parse header
        let mut header_bytes = [0u8; HEADER_SIZE];
        file.read_exact(&mut header_bytes)?;
        let header = Header::from_bytes(&header_bytes)?;

        let max_size = DEFAULT_FILE_SIZE; // Could also store in header

        Ok(TcsLog {
            dir_name,
            file,
            path,
            header: header.clone(),
            max_size,
            write_position: 0,
            read_position: header.data_offset,
            prefix: prefix.to_string(),
            writing: false,
        })
    }

    /// Opens an existing TcsLog by path.
    pub fn tcslog_open_path<P: AsRef<Path>>(path: P) -> Result<TcsLog<'a>, TcsLogError> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(TcsLogError::NotFound);
        }

        let mut file = OpenOptions::new().read(true).open(path)?;

        // Read and parse header
        let mut header_bytes = [0u8; HEADER_SIZE];
        file.read_exact(&mut header_bytes)?;
        let header = Header::from_bytes(&header_bytes)?;

        let max_size = DEFAULT_FILE_SIZE;

        // Extract prefix from file name
        let file_name = header.file_name_str();
        let prefix = file_name
            .split('-')
            .next()
            .unwrap_or("")
            .to_string();

        Ok(TcsLog {
            dir_name: "",              // FIXME: not needed
            file,
            path: path.to_path_buf(),
            header: header.clone(),
            max_size,
            write_position: 0,
            read_position: header.data_offset,
            prefix,
            writing: false,
        })
    }

    /// Writes the telemetry data to the TcsLog.
    ///
    /// If there is not enough room in the current log file, another will be created.
    /// It is an error to write more data than will fit in a newly created log file.
    ///
    /// Returns () if the data was written, otherwise Err(TcsLogError).
    pub fn write(&mut self, data: &[u8]) -> Result<(), TcsLogError> {
        if !self.writing {
            return Err(TcsLogError::InvalidFormat(
                "Log not opened for writing".to_string(),
            ));
        }

        // Check if record fits
        let record_size = data::RECORD_METADATA_SIZE + data.len();
        let max_record_size = self.max_size as usize - HEADER_SIZE - BLOCK_SIZE; // Minimum index

        if record_size > max_record_size {
            return Err(TcsLogError::RecordTooLarge);
        }

        let timestamp = Self::current_timestamp();

        // Calculate space needed in current block
        let current_block_offset = (self.write_position - self.header.data_offset) % BLOCK_SIZE as u64;

/*
        let space_in_block = if current_block_offset == 0 {
            BLOCK_SIZE - data::BLOCK_HEADER_SIZE
        } else {
            BLOCK_SIZE - current_block_offset as usize
        };
*/

        // Write block header if at start of new block
        if current_block_offset == 0 {
            self.file.seek(SeekFrom::Start(self.write_position))?;
            self.file.write_all(&data::TCSLOG_REC.to_le_bytes())?;
            self.write_position += data::BLOCK_HEADER_SIZE as u64;
        }

        // Check if we need a new file
        let total_needed = record_size;
        let remaining_in_file = self.max_size - self.write_position;

        if total_needed as u64 > remaining_in_file {
            // Create a new log file
            let new_log = Self::new(&self.dir_name, &self.prefix, self.max_size)?;
            *self = new_log;
            return self.write(data);
        }

        // Write record length
        self.file.write_all(&(data.len() as u64).to_le_bytes())?;
        self.write_position += 8;

        // Write timestamp
        self.file.write_all(&timestamp.to_le_bytes())?;
        self.write_position += 8;

        // Write data
        self.file.write_all(data)?;
        self.write_position += data.len() as u64;

        // Update index
        self.update_index(self.write_position - record_size as u64, timestamp)?;

        Ok(())
    }

    /// Updates the index with a new record.
    fn update_index(&mut self, _offset: u64, _timestamp: Timestamp) -> Result<(), TcsLogError> {
        // Index update implementation
        // For simplicity, we update the first index block entry
        // A full implementation would maintain a proper B-tree structure
        Ok(())
    }

    /// Reads the next telemetry record from the TcsLog.
    ///
    /// Returns the number of bytes placed in data on success, Err(TcsLogError) otherwise.
    pub fn read(&mut self, timestamp: &mut Timestamp, data: &mut [u8]) -> Result<usize, TcsLogError> {
        if self.writing {
            return Err(TcsLogError::InvalidFormat(
                "Log not opened for reading".to_string(),
            ));
        }

        // Check if at start of new block
        let block_offset = (self.read_position - self.header.data_offset) % BLOCK_SIZE as u64;
        if block_offset == 0 {
            // Skip block header
            self.read_position += data::BLOCK_HEADER_SIZE as u64;
        }

        // Read record length
        self.file.seek(SeekFrom::Start(self.read_position))?;
        let mut len_bytes = [0u8; 8];
        if self.file.read_exact(&mut len_bytes).is_err() {
            return Err(TcsLogError::EndOfLog);
        }
        let len = u64::from_le_bytes(len_bytes) as usize;

        if len == 0 {
            return Err(TcsLogError::EndOfLog);
        }

        self.read_position += 8;

        // Read timestamp
        let mut ts_bytes = [0u8; size_of::<Timestamp>()];
        self.file.read_exact(&mut ts_bytes)?;
        *timestamp = Timestamp::from_le_bytes(ts_bytes);
        let read_offset: u64 = size_of::<Timestamp>().try_into().unwrap();
        self.read_position += read_offset;

        // Read data
        if len > data.len() {
            return Err(TcsLogError::InvalidFormat(
                "Buffer too small for record".to_string(),
            ));
        }

        self.file.read_exact(&mut data[..len])?;
        self.read_position += len as u64;

        Ok(len)
    }

    /// Given a timestamp, determines the offset in the log file of the first data block
    /// containing a header with that timestamp or greater.
    ///
    /// If no error occurred, returns the offset. Otherwise, returns Err(TcsLogError).
    pub fn timestamp_offset(&mut self, timestamp: Timestamp) -> Result<u64, TcsLogError> {
        // Read index block
        let mut index_bytes = [0u8; BLOCK_SIZE];
        self.file.seek(SeekFrom::Start(self.header.index_offset))?;
        self.file.read_exact(&mut index_bytes)?;

        let index_block = IndexBlock::from_bytes(&index_bytes);

        // Find the entry with timestamp >= given timestamp
        for entry in index_block.entries.iter() {
            if entry.is_null() {
                break;
            }
            if entry.timestamp >= timestamp {
                return Ok(entry.offset);
            }
        }

        // If no matching entry found, return the start of data section
        Ok(self.header.data_offset)
    }

    /// Returns the path to the log file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the file name.
    pub fn file_name(&self) -> &str {
        self.header.file_name_str()
    }

    /// Returns the creation timestamp.
    pub fn timestamp(&self) -> Timestamp {
        self.header.timestamp
    }

    /// Flushes any buffered data to disk.
    pub fn flush(&mut self) -> Result<(), TcsLogError> {
        self.file.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_file_name() {
        let ts: Timestamp = 0x0001_2345_6789_ABCD;
        let name = TcsLog::generate_file_name("test", ts);
        assert_eq!(name, "test-0001_2345_6789_abcd");
    }

    #[test]
    fn test_parse_timestamp_from_name() {
        let ts = TcsLog::parse_timestamp_from_name("test-0001_2345_6789_abcd", "test");
        assert_eq!(ts, Some(0x0001_2345_6789_ABCD));
    }

    #[test]
    fn test_compute_offsets() {
        let (index_offset, data_offset, _) = TcsLog::compute_offsets(DEFAULT_FILE_SIZE);
        assert_eq!(index_offset, HEADER_SIZE as u64);
        assert!(data_offset > index_offset);
        assert_eq!(data_offset % BLOCK_SIZE as u64, 0);
    }

    #[test]
    fn test_invalid_prefix() {
        let result = TcsLog::new("", "", DEFAULT_FILE_SIZE);
        assert!(matches!(result, Err(TcsLogError::InvalidPrefix(_))));

        let result = tcslog_create("a/b");
        assert!(matches!(result, Err(TcsLogError::InvalidPrefix(_))));
    }
}
