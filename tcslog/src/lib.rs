//! TcsLog provides:
//! * Fixed length logs with automatic switching to new logs when old ones fill
//! * Arbitrary record sizes
//! * Self-identified log files (the name is in the header)
//! * Indexed by automatically supplied timestamps with nanosecond resolution
//! * Metadata all in little-endian form

use std::cmp::Ordering;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};
use thiserror::Error;

mod config;
mod data;
mod error;
mod header;
mod index;
mod timestamp;

struct Offset {
    offset: u64,
}

impl Offset {
    const OFFSET_SIZE: usize = size_of::<u64>();

    fn new(offset: u64) -> Offset {
        Offset { offset }
    }

    // Length when packed
    pub const fn len() -> usize {
        size_of::<u64>()
    }

    pub fn to_le_bytes(&self) -> [u8; Self::OFFSET_SIZE] {
        let a_offset = self.offset.to_le_bytes();

        let mut offset = [0; Self::OFFSET_SIZE];
        offset[..8].copy_from_slice(&a_offset);

        offset
    }

    pub fn from_le_bytes(buf: [u8; Self::OFFSET_SIZE]) -> Offset {
        let mut a_offset: [u8; Self::OFFSET_SIZE] = [0; Self::OFFSET_SIZE];
        a_offset.copy_from_slice(&buf[..Self::OFFSET_SIZE]);
        let offset = u64::from_le_bytes(a_offset);
        Offset { offset }
    }
}

impl PartialEq for Offset {
    fn eq(&self, r: &Offset) -> bool {
        self.offset == r.offset
    }
}

impl PartialOrd for Offset {
    fn partial_cmp(&self, r: &Offset) -> Option<Ordering> {
        if self.offset < r.offset {
            Some(Ordering::Less)
        } else if self.offset > r.offset {
            Some(Ordering::Greater)
        } else {
            Some(Ordering::Equal)
        }
    }
}

pub use config::{BLOCK_SIZE, MAX_RETRIES, WAIT_FOR_NEW_NAME};
pub use data::{
    BlockHeader, CONT_SIZE, DataRecord, BLOCK_HEADER_SIZE, MAX_RECORD_SIZE, RECORD_METADATA_SIZE, TCSLOG_NULL, TCSLOG_REC,
};
pub use error::TcsLogError;
pub use header::{Header, HEADER_SIZE};
pub use index::{IndexBlock, IndexEntry, IndexStructure, ENTRIES_PER_BLOCK, FILE_NULL};
pub use timestamp::Timestamp;

/// Default file size (64MB).
pub const DEFAULT_FILE_SIZE: u64 = 64 * 1024 * 1024;

/// Maximum prefix length for file names.
pub const MAX_PREFIX_LEN: usize = 32;

/// Size of everything after the prefix
pub const FILE_TIMESTAMP_LEN: usize = 6 * 5;

// Max suffix length for file names
pub const MAX_SUFFIX_LEN: usize = 8;

pub const MAX_FILENAME_LEN: usize = MAX_PREFIX_LEN + FILE_TIMESTAMP_LEN +
    MAX_SUFFIX_LEN;

/// File type identifier.
pub const FILE_TYPE: &[u8; 8] = b"tcslog  ";

/// Version string (major.minor.patch).
pub const VERSION_00_01_00: &[u8; 8] = b"00.01.00";

// Object that keeps track of the time for timestamps
pub trait Timestampable {
    fn timestamp(&mut self) -> Result<Timestamp, TimestampableError>;
}

/*
 * Define a type that returns the timestamp. In this implementation,
 * we return the time since the UNIX epoch.
 */
#[derive(Debug)]
pub struct Timestamper {}

impl Timestamper {
    fn new() -> Timestamper {
        Timestamper {}
    }
}

impl Timestampable for Timestamper {
    /// Returns the current timestamp in nanoseconds since UNIX epoch. This
    /// can fail if the system clock was changed. In this case, the only
    /// way to ensure consistent timestamps in a chain of log files is to
    /// restart the application. This is because timestamps are assumed to
    /// be monotonically increasing
    fn timestamp(&mut self) -> Result<Timestamp, TimestampableError> {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Err(e) => Err(TimestampableError::DurationSinceFailed(e)),
            Ok(now) => {
                let now_ns = now.as_nanos();
                Ok(Timestamp::from_nanos(now_ns))
            }
        }
    }
}

#[derive(Clone, Debug, Error)]
pub enum TimestampableError {
    #[error("DurationSince failed: {0} (did SystemClock go backwards?)")]
    DurationSinceFailed(SystemTimeError),
}

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
    /// The suffix used for file naming.
    suffix: String,
    /// Whether the log is open for writing.
    writing: bool,
}

impl<'a> TcsLog<'a> {
    /// Creates a new TcsLog with a specified maximum file size.
    pub fn new(
        dir_name: &'a str,
        prefix: &str,
        suffix: &str,
        max_size: u64,
    ) -> Result<TcsLog<'a>, TcsLogError<'static>> {
        // Generate timestamp and file name
        let mut timestamper = Timestamper::new();
        Self::new_with_timestamp(dir_name, prefix, &mut timestamper, suffix, max_size)
    }

    /// Creates a new TcsLog with a specified maximum file size while
    /// specifying the timestamp. This is useful for testing when you
    /// want to know the name of the file.
    pub fn new_with_timestamp(
        dir_name: &'a str,
        prefix: &str,
        timestamper: &mut dyn Timestampable,
        suffix: &str,
        max_size: u64,
    ) -> Result<TcsLog<'a>, TcsLogError<'static>> {
        Self::validate_prefix(prefix)?;
        Self::validate_suffix(suffix)?;

        // FIXME: Need to validate whether there is room for any data

        // Compute offsets
        let (index_offset, data_offset, _index_blocks) = TcsLog::compute_offsets(max_size);

        let mut retries = 0;

        let (path, mut file, header) = loop {
            let timestamp = timestamper.timestamp().unwrap();
            let file_name = TcsLog::generate_file_name(prefix, timestamp, suffix)?;

            // Create header
            let header = Header::new(timestamp, index_offset, data_offset, &file_name);

            // Create the file
            let path = PathBuf::from(dir_name).join(&file_name);

            match OpenOptions::new()
                // FIXME: remove this?
                //                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(f) => break (path, f, header),
                Err(e)
                    if e.kind() == std::io::ErrorKind::AlreadyExists && retries < MAX_RETRIES =>
                {
                    // Wait and try again with new timestamp
                    std::thread::sleep(std::time::Duration::from_micros(WAIT_FOR_NEW_NAME));
                    retries += 1;
                    continue;
                }
                Err(e) => return Err(TcsLogError::Io(e)),
            }
        };

        // Write the header
        let header_bytes = header.to_bytes();

        if header_bytes.len() > HEADER_SIZE {
            return Err(TcsLogError::BlockSizeTooSmall);
        }

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
            suffix: suffix.to_string(),
            writing: true,
        })
    }

    /// Generates a file name from prefix and timestamp.
    pub fn generate_file_name(
        prefix: &str,
        timestamp: Timestamp,
        suffix: &str,
    ) -> Result<String, TcsLogError<'static>> {
        Self::validate_prefix(prefix)?;
        Self::validate_suffix(suffix)?;

        // Format: <prefix><XXXX_XXXX_XXXX_XXXX_XXXX_XXXX><suffix> (where X is hex digit)
        let timestamp_u128 = timestamp.as_nanos();
        let hex = format!("{:024}", timestamp_u128);
        let filename = format!(
            "{}{}_{}_{}_{}_{}_{}{}",
            prefix,
            &hex[0..4],
            &hex[4..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..24],
            suffix,
        );

        Ok(filename)
    }

    // Determines whether the prefix is valid
    // Returns Ok(()) if valid, Err(TcsLogError) if not
    pub fn validate_prefix(prefix: &str) -> Result<(), TcsLogError<'static>> {
        if prefix.is_empty() || prefix.len() > MAX_PREFIX_LEN {
            return Err(TcsLogError::InvalidPrefixLen(MAX_PREFIX_LEN));
        }

        if prefix
            .chars()
            .all(|c| !c.is_ascii_alphanumeric() && c != '_')
        {
            return Err(TcsLogError::InvalidPrefixChar(prefix.to_string()));
        }

        Ok(())
    }

    // Determines whether the suffix is valid
    // Returns Ok(()) if valid, Err(TcsLogError) if not
    pub fn validate_suffix(suffix: &str) -> Result<(), TcsLogError<'static>> {
        if suffix.len() == 0 || suffix.len() > MAX_SUFFIX_LEN {
            return Err(TcsLogError::InvalidSuffixLen(MAX_SUFFIX_LEN));
        }

        if suffix
            .chars()
            .all(|c| !c.is_ascii_alphanumeric() && c != '_')
        {
            return Err(TcsLogError::InvalidSuffixChar(suffix.to_string()));
        }

        Ok(())
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
    pub fn open(
        dir_name: &'a str,
        prefix: &str,
        timestamp: Timestamp,
        suffix: &str,
    ) -> Result<TcsLog<'a>, TcsLogError<'static>> {
        let file_name = TcsLog::generate_file_name(prefix, timestamp, suffix)?;
        let path = PathBuf::from(&file_name);
        println!("TcsLog::open: path {:?}", path);

        if !path.exists() {
            return Err(TcsLogError::NotFound);
        }

        let mut file = OpenOptions::new().read(true).open(&path)?;

        // Read and parse header
        let mut header_bytes = [0u8; HEADER_SIZE];
        println!("reading header bytes");
        file.read_exact(&mut header_bytes)?;
        println!("read header bytes");
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
            suffix: suffix.to_string(),
            writing: false,
        })
    }

    /// Opens an existing TcsLog by path. This specifically does not validate
    /// the format of the path name so that, if used for recovery efforts,
    /// there is more flexibility in its use.
    pub fn open_path<P: AsRef<Path>>(path: P) -> Result<TcsLog<'a>, TcsLogError<'static>> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(TcsLogError::NotFound);
        }

        let mut file = OpenOptions::new().read(true).open(path)?;

        // Read and parse header
        let mut header_bytes = [0u8; HEADER_SIZE];
        //println!("reading {:?}", header_bytes[..160]);
        file.read_exact(&mut header_bytes)?;
        //println!("read {:?}", header_bytes[..160]);
        let header = Header::from_bytes(&header_bytes)?;

        let max_size = DEFAULT_FILE_SIZE;

        // Extract prefix from file name
        println!("extracting prefix");
        let file_name = header.file_name_str();
        println!("open_path: initial read_position {:?}", header.data_offset);

        Ok(TcsLog {
            dir_name: "", // FIXME: not needed
            file,
            path: path.to_path_buf(),
            header: header.clone(),
            max_size,
            write_position: 0,
            read_position: header.data_offset,
            prefix: "".to_string(), // FIXME: not needed
            suffix: "".to_string(), // FIXME: not needed
            writing: false,
        })
    }

    /// Writes the telemetry data to the TcsLog.
    ///
    /// If there is not enough room in the current log file, another will be created.
    /// It is an error to write more data than will fit in a newly created log file.
    ///
    /// Returns () if the data was written, otherwise Err(TcsLogError).
    pub fn write(
        &mut self,
        timestamper: &mut dyn Timestampable,
        data: &[u8],
    ) -> Result<(), TcsLogError<'static>> {
        if !self.writing {
            return Err(TcsLogError::InvalidFormat(
                "Log not opened for writing".to_string(),
            ));
        }

        if !self.record_fits(data.len()) {
            return Err(TcsLogError::RecordTooLarge);
        }

        let timestamp = timestamper.timestamp()?;

        // Calculate space needed in current block
        let current_block_offset =
            (self.write_position - self.header.data_offset) % BLOCK_SIZE as u64;

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
        let total_needed = data.len();
        let remaining_in_file = self.max_size - self.write_position;

        if total_needed as u64 > remaining_in_file {
            // Create a new log file
            let new_log =
                Self::new_with_timestamp(&self.dir_name, &self.prefix, timestamper, &self.suffix, self.max_size)?;
            *self = new_log;
            return self.write(timestamper, data);
        }

        // Write timestamp
        println!(
            "TcsLog::write: writing timetamp {:?} at {:?}",
            timestamp,
            self.file.stream_position()
        );
        let buf = timestamp.to_le_bytes();
        self.file.write_all(&buf)?;
        self.write_position += buf.len() as u64;

        // Write record length
        self.file.write_all(&(data.len() as u64).to_le_bytes())?;
        self.write_position += 8;

        // Write data
        self.file.write_all(data)?;
        self.write_position += data.len() as u64;

        // Update index
        self.update_index(self.write_position - data.len() as u64, timestamp)?;

        Ok(())
    }

    /* See whether this record fits in the current file. Specifically,
     * we see whether the data plus the metadata plus a subsequent
     * continuation marker will fit. This ensures that, if we write
     * this record, we can still write a continuation marker.
     *
     * data_len:    Size of the data record
     */
    fn record_fits(&self, data_len: usize) -> bool {
        let unused_space = self.max_size - self.write_position;
        let record_size = data::RECORD_METADATA_SIZE + data_len;
        record_size + CONT_SIZE < unused_space as usize
    }

    /// Updates the index with a new record.
    fn update_index(
        &mut self,
        _offset: u64,
        _timestamp: Timestamp,
    ) -> Result<(), TcsLogError<'static>> {
        // Index update implementation
        // For simplicity, we update the first index block entry
        // A full implementation would maintain a proper B-tree structure
        Ok(())
    }

    /// Reads the next telemetry record from the TcsLog.
    ///
    /// Returns the number of bytes placed in data on success,
    /// Err(TcsLogError) otherwise.
    pub fn read(
        &mut self,
        timestamp: &mut Timestamp,
        data: &mut [u8],
    ) -> Result<usize, TcsLogError<'static>> {
        if self.writing {
            return Err(TcsLogError::InvalidFormat(
                "Log not opened for reading".to_string(),
            ));
        }
        println!("Reading...");

        // Check if at start of new block
        println!("read_position {:?}", self.read_position);
        let block_offset = (self.read_position - self.header.data_offset) % BLOCK_SIZE as u64;
        if block_offset == 0 {
            // Skip block header
            self.read_position += data::BLOCK_HEADER_SIZE as u64;
            println!("read_position adjusted to {:?}", self.read_position);
        }

        // Read timestamp. If we get zero bytes, we're at the physical, and
        // hence, logical EOF. If the timestamp is Timestamp::CONT, we are
        // at the continuation marker that ends the file.
        self.file.seek(SeekFrom::Start(self.read_position))?;
        let mut ts_bytes = [0u8; Timestamp::TIMESTAMP_SIZE];

        // FIXME: this code assumes that seeking past the end of the file and
        // then reading will return zero bytes. Is that guaranteed by the
        // Rust runtime library? I think it is by Linux, but this might be
        // a portability if the Rust RT doesn't guarantee it. It looks like the
        // answer is no, so this needs to be fixed.
println!("TcsLog::read: reading timestamp");
        match self.file.read(&mut ts_bytes) {
            Err(e) => return Err(TcsLogError::Io(e)),
            Ok(n) => {
                if n == 0 {
                    // FIXME: double check this
                    return Err(TcsLogError::EOF);
                } else if n != Timestamp::TIMESTAMP_SIZE {
                    return Err(TcsLogError::CorruptedEOF);
                }
            }
        }

        *timestamp = Timestamp::from_le_bytes(ts_bytes);
println!("TcsLog::read: read timestamp {:?}", timestamp);
        if *timestamp == Timestamp::CONT {
            return Err(TcsLogError::EOF);
        }

        // Read record length
        let mut len_bytes = [0u8; 8];

        println!("TcsLog::read: position {:?}", self.file.stream_position());
        println!(
            "TcsLog::read: reading record length {} from {}",
            len_bytes.len(),
            self.read_position
        );
        if let Err(e) = self.file.read_exact(&mut len_bytes) {
            println!("TcsLog::read: record length read failed");
            return Err(TcsLogError::Io(e));
        }
        let len = u64::from_le_bytes(len_bytes) as usize;
        self.read_position += 8;

        println!("TcsLog::read: Reading record data");
        let read_offset: u64 = (Timestamp::TIMESTAMP_SIZE as u64).try_into().unwrap();
        self.read_position += read_offset;

        // Read data
        println!("TcsLogTimestamp::read: len {len} data.len {}", data.len());
        if len > data.len() {
            return Err(TcsLogError::InvalidFormat(
                "Buffer too small for record".to_string(),
            ));
        }

        println!("Timestamp::read: reading data len {len}");
        self.file.read_exact(&mut data[..len])?;
        println!("Timestamp::read: read data len {len}");
        self.read_position += len as u64;
println!("Final read position {:?}", self.read_position);

        Ok(len)
    }

    /// Given a timestamp, determines the offset in the log file of the first data block
    /// containing a header with that timestamp or greater.
    ///
    /// If no error occurred, returns the offset. Otherwise, returns Err(TcsLogError).
    pub fn timestamp_offset(&mut self, timestamp: Timestamp) -> Result<u64, TcsLogError<'static>> {
        // Read index block
        let mut index_bytes = [0u8; BLOCK_SIZE];
        self.file.seek(SeekFrom::Start(self.header.index_offset))?;
        println!("timestamp_offset: reading index");
        self.file.read_exact(&mut index_bytes)?;
        println!("timestamp_offset: read index");

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
    pub fn flush(&mut self) -> Result<(), TcsLogError<'static>> {
        self.file.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_file_name() {
        let ts: Timestamp = Timestamp::from_nanos(0x0001_2345_6789_ABCD_EF01_2345_6789_ABCD);
        let name = TcsLog::generate_file_name("test-", ts, ".tcslog").unwrap();
        assert_eq!(name, "test-0001_2345_6789_abcd_ef01_2345_6789_abcd.tcslog");
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

        //        let result = TestLog::create("a/b");
        //        assert!(matches!(result, Err(TcsLogError::InvalidPrefix(_))));
    }
}
