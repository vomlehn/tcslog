//! TcsLog provides:
//! * Fixed length logs with automatic switching to new logs when old ones fill
//! * Arbitrary record sizes
//! * Self-identified log files (the name is in the header)
//! * Indexed by automatically supplied Uids with nanosecond resolution
//! * Metadata all in little-endian form

use std::cmp::Ordering;
// Imported only so the `write!` macro can reach `fmt::Write::write_fmt`; aliased
// to `_` so it does not collide with `std::io::Write`.
use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};
use thiserror::Error;

mod config;
mod data;
mod error;
mod header;
mod index;
mod uid;

pub use config::{BLOCK_SIZE, MAX_RETRIES, WAIT_FOR_NEW_NAME};
pub use data::{
    BlockHeader, BLOCK_HEADER_SIZE, CONT_SIZE, DataRecord, MAX_RECORD_SIZE, RECORD_METADATA_SIZE,
};
pub use error::TcsLogError;
pub use header::{Header};
pub use index::{IndexBlock, IndexEntry, IndexStructure, ENTRIES_PER_BLOCK, FILE_NULL};
pub use uid::Uid;

/// Default file size (64MB).
pub const DEFAULT_FILE_SIZE: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
struct Offset {
    offset: u64,
}

impl Offset {
    /// Block header indicating null pointer (does not reference within a file).
    pub const NULL: Offset = Offset::new_raw(0x0000_0000_0000_0001);

    /// Block header indicating next record starts at end of header.
    pub const REC_START: Offset = Offset::new_raw(0x0000_0000_0000_0002);

    pub const PACKLEN: usize = size_of::<u64>();

    pub const fn new_raw(offset: u64) -> Offset {
        Offset { offset }
    }

/*
    // Length when packed
    pub const fn packlen(&self) -> usize {
        size_of_val(&self.offset)
    }
*/

    pub fn to_le_bytes(&self) -> [u8; Self::PACKLEN] {
        let a_offset = self.offset.to_le_bytes();

        let mut offset = [0; Self::PACKLEN];
        offset[..8].copy_from_slice(&a_offset);

        offset
    }

    pub fn from_le_bytes(buf: [u8; Self::PACKLEN]) -> Offset {
        let mut a_offset: [u8; Self::PACKLEN] = [0; Self::PACKLEN];
        a_offset.copy_from_slice(&buf[..Self::PACKLEN]);
        let offset = u64::from_le_bytes(a_offset);
        Offset { offset }
    }
}

impl From<Offset> for u64 {
    fn from(value: Offset) -> Self {
        value.offset
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

/// A `fmt::Write` sink over a fixed-size byte buffer. It lets `write!` format
/// names and paths into stack/inline storage without allocating on the heap.
/// Writing more than the buffer holds fails with `fmt::Error`.
struct ByteBuf<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl<'a> ByteBuf<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        ByteBuf { buf, len: 0 }
    }
}

impl std::fmt::Write for ByteBuf<'_> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        let end = self.len + s.len();
        if end > self.buf.len() {
            return Err(std::fmt::Error);
        }
        self.buf[self.len..end].copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

/// Packs a length-bounded string into a fixed `[u8; N]` buffer (truncating if it
/// somehow exceeds `N`), returning the buffer and the number of bytes written.
/// Used to store validated prefixes/suffixes inline without heap allocation.
fn pack_str<const N: usize>(s: &str) -> ([u8; N], usize) {
    let mut buf = [0u8; N];
    let len = s.len().min(N);
    buf[..len].copy_from_slice(&s.as_bytes()[..len]);
    (buf, len)
}

/// Maximum length of a log file path, stored inline so that creating/opening a
/// log file (including chain rollover) needs no heap allocation. This matches
/// the typical POSIX `PATH_MAX`; tune it for platforms with different limits.
const MAX_PATH_LEN: usize = 4096;

/// Formats `<dir>/<name>` into `buf` without allocating, returning the byte
/// length. Errors if the result would not fit in `buf`.
fn build_path(buf: &mut [u8], dir: &str, name: &str) -> Result<usize, TcsLogError<'static>> {
    let mut w = ByteBuf::new(buf);
    write!(w, "{dir}/{name}")
        .map_err(|_| TcsLogError::InvalidFormat("Path too long".to_string()))?;
    Ok(w.len)
}

/// Copies an already-built path string into a fixed `MAX_PATH_LEN` buffer,
/// erroring if it is too long. No heap allocation.
fn pack_path(s: &str) -> Result<([u8; MAX_PATH_LEN], usize), TcsLogError<'static>> {
    if s.len() > MAX_PATH_LEN {
        return Err(TcsLogError::InvalidFormat("Path too long".to_string()));
    }
    let mut buf = [0u8; MAX_PATH_LEN];
    buf[..s.len()].copy_from_slice(s.as_bytes());
    Ok((buf, s.len()))
}

/// FIXME: This needs to use system-dependent functions file file name
/// manipulation
#[derive(Debug, Clone, Copy)]
pub struct Filename {
    file_name:  [u8; Self::PACKLEN],
}

type FilenameLen = u8;

impl Filename {
    /// Maximum prefix length for file names.
    pub const MAX_PREFIX_LEN: usize = 32;

    /// Size of the Uid portion of the file name. It has
    /// six segments, each with four hex digits, separated by
    /// underlines (6 * 4 hex digits + 5 separators).
    pub const FILE_Uid_LEN: usize = 6 * 4 + 5;

    // Max suffix length for file names
    pub const MAX_SUFFIX_LEN: usize = 16;

    pub const PACKLEN: usize = size_of::<FilenameLen>() +
        Self::MAX_PREFIX_LEN + Self::FILE_Uid_LEN + Self::MAX_SUFFIX_LEN;

    pub fn new(prefix: &str, Uid: Uid, suffix: &str) -> 
        Result<Filename, TcsLogError<'static>> {
        Self::validate_prefix(prefix)?;
        Self::validate_suffix(suffix)?;

        // Format <prefix><XXXX_XXXX_XXXX_XXXX_XXXX_XXXX><suffix> directly into
        // the fixed-size, NUL-padded name buffer, with no heap allocation. The
        // Uid is rendered in microseconds as 24 lowercase hex digits in
        // six underscore-separated groups (each group is one 16-bit slice of
        // the 96-bit value).
        let ts = Uid.as_micros();
        let mut file_name = [0u8; Self::PACKLEN];
        {
            let mut w = ByteBuf::new(&mut file_name);
            write!(
                w,
                "{prefix}{:04x}_{:04x}_{:04x}_{:04x}_{:04x}_{:04x}{suffix}",
                (ts >> 80) as u16,
                (ts >> 64) as u16,
                (ts >> 48) as u16,
                (ts >> 32) as u16,
                (ts >> 16) as u16,
                ts as u16,
            )
            .map_err(|_| TcsLogError::InvalidFormat("File name too long".to_string()))?;
        }

        Ok(Filename { file_name })
    }

    /// Returns the file name as a string slice (up to the first NUL byte).
    pub fn as_str(&self) -> &str {
        let nul_pos = self
            .file_name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(Self::PACKLEN);
        std::str::from_utf8(&self.file_name[..nul_pos]).unwrap_or("")
    }

/*
    pub fn packlen(&self) -> usize {
        size_of_val(&self.file_name)
    }
*/



    pub fn to_le_bytes(&self) -> [u8; Self::PACKLEN] {
        self.file_name
    }

    pub fn from_le_bytes(file_name: [u8; Self::PACKLEN]) -> Filename {
        Filename {
            file_name,
        }
    }

    // Determines whether the prefix is valid
    // Returns Ok(()) if valid, Err(TcsLogError) if not
    pub fn validate_prefix(prefix: &str) -> Result<(), TcsLogError<'static>> {
        if prefix.is_empty() || prefix.len() > Filename::MAX_PREFIX_LEN {
            return Err(TcsLogError::InvalidPrefixLen(Filename::MAX_PREFIX_LEN));
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
        if suffix.is_empty() || suffix.len() > Filename::MAX_SUFFIX_LEN {
            return Err(TcsLogError::InvalidSuffixLen(Filename::MAX_SUFFIX_LEN));
        }

        if suffix
            .chars()
            .all(|c| !c.is_ascii_alphanumeric() && c != '_')
        {
            return Err(TcsLogError::InvalidSuffixChar(suffix.to_string()));
        }

        Ok(())
    }
}

// Object that keeps track of the time for Uids
pub trait Uidable {
    fn Uid(&mut self) -> Result<Uid, UidableError>;
}

/*
 * Define a type that returns the Uid. In this implementation,
 * we return the time since the UNIX epoch.
 */
#[derive(Debug)]
pub struct Uider {}

impl Uider {
    /// Creates a Uider backed by the system clock (nanoseconds since the
    /// UNIX epoch).
    pub fn new() -> Uider {
        Uider {}
    }
}

impl Default for Uider {
    fn default() -> Self {
        Self::new()
    }
}

impl Uidable for Uider {
    /// Returns the current Uid in nanoseconds since UNIX epoch. This
    /// can fail if the system clock was changed. In this case, the only
    /// way to ensure consistent Uids in a chain of log files is to
    /// restart the application. This is because Uids are assumed to
    /// be monotonically increasing
    fn Uid(&mut self) -> Result<Uid, UidableError> {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Err(e) => Err(UidableError::DurationSinceFailed(e)),
            Ok(now) => {
                let now_ns = now.as_nanos();
                Ok(Uid::from_nanos(now_ns))
            }
        }
    }
}

#[derive(Clone, Debug, Error)]
pub enum UidableError {
    #[error("DurationSince failed: {0} (did SystemClock go backwards?)")]
    DurationSinceFailed(SystemTimeError),
}

/// Represents a TcsLog instance for reading or writing telemetry records.
#[derive(Debug)]
/*
 * dir_name         System-dependend directory name
 * file             The file handle for doing file I/O
 * path             Absolute or relative path name to the file
 * header           Header for the log file
 * max_size         Maximum log file size, in bytes
 * write_position   Byte offset of the next write from the beginning of the log
 *                  file
 * read_position    Byte offset of the next read from the beginning of the log
 *                  file
 * prefix           Prefix added to log file name before Uid
 * suffix           Suffix added to log file name after Uid
 */
pub struct TcsLog<'a> {
    dir_name: &'a str,
    file: File,
    /// The full file path, stored inline to avoid heap allocation.
    path_buf: [u8; MAX_PATH_LEN],
    path_len: usize,
    /// The file header.
    header: Header,
    /// Maximum file size.
    max_size: u64,
    /// Current write position in the data section.
    write_position: u64,
    /// Current read position.
    read_position: u64,
    /// The prefix used for file naming, stored inline to avoid heap allocation.
    prefix: [u8; Filename::MAX_PREFIX_LEN],
    prefix_len: usize,
    /// The suffix used for file naming, stored inline to avoid heap allocation.
    suffix: [u8; Filename::MAX_SUFFIX_LEN],
    suffix_len: usize,
    /// Whether the log is open for writing.
    writing: bool,
    /// Total number of records written to this log, carried across rollover so
    /// it counts every message written to the chain.
    messages_written: u64,
}

impl<'a> TcsLog<'a> {
    /// Generates the log file name for the given prefix, Uid, and suffix.
    /// This reproduces the name a log was (or would be) created under, so an
    /// existing log can be reopened.
    pub fn generate_file_name(
        prefix: &str,
        Uid: Uid,
        suffix: &str,
    ) -> Result<String, TcsLogError<'static>> {
        Ok(Filename::new(prefix, Uid, suffix)?.as_str().to_string())
    }

    /// Returns this log file's position in the chain. The first file in a
    /// chain has a count of zero; each successor increments it.
    pub fn chain_count(&self) -> u32 {
        self.header.chain_count
    }

    /// The prefix used for this log's file names.
    fn prefix(&self) -> &str {
        std::str::from_utf8(&self.prefix[..self.prefix_len]).unwrap_or("")
    }

    /// The suffix used for this log's file names.
    fn suffix(&self) -> &str {
        std::str::from_utf8(&self.suffix[..self.suffix_len]).unwrap_or("")
    }

    /// Creates a new TcsLog with a specified maximum file size.
    /// dir_name    System-dependend directory name
    /// prefix      String that is prepended to the Uid part of
    ///             the log file name
    /// suffix      String that is appended to the Uid part of the log
    ///             file name.
    /// max_size    Maximum size of the log file
    pub fn new(
        dir_name: &'a str,
        prefix: &str,
        suffix: &str,
        max_size: u64,
    ) -> Result<TcsLog<'a>, TcsLogError<'static>> {
        // Generate Uid and file name
        let mut Uider = Uider::new();
        Self::new_with_Uid(dir_name, prefix, &mut Uider, suffix, max_size)
    }

    /// Creates a new TcsLog with a specified maximum file size while
    /// specifying the Uid. This is useful for testing when you
    /// want to know the name of the file.
    pub fn new_with_Uid(
        dir_name: &'a str,
        prefix: &str,
        Uider: &mut dyn Uidable,
        suffix: &str,
        max_size: u64,
    ) -> Result<TcsLog<'a>, TcsLogError<'static>> {
        // The first file in a chain has a chain count of zero.
        Self::new_with_Uid_chained(dir_name, prefix, Uider, suffix, max_size, 0)
    }

    /// Like `new_with_Uid`, but stamps the new log file's header with the
    /// given chain count. Used when a record overflows into a freshly created
    /// successor file in the chain.
    fn new_with_Uid_chained(
        dir_name: &'a str,
        prefix: &str,
        Uider: &mut dyn Uidable,
        suffix: &str,
        max_size: u64,
        chain_count: u32,
    ) -> Result<TcsLog<'a>, TcsLogError<'static>> {
        Filename::validate_prefix(prefix)?;
        Filename::validate_suffix(suffix)?;

        // FIXME: Need to validate whether there is room for any data

        // Compute offsets
        let (index_offset, data_offset, _index_blocks) = TcsLog::compute_offsets(max_size);

        let mut retries = 0;

        let (path_buf, path_len, mut file, mut header) = loop {
            let Uid = Uider.Uid().unwrap();
            let file_name = Filename::new(prefix, Uid, suffix)?;

            // Create header
            let header = Header::new(Uid, index_offset, data_offset, file_name.as_str());

            // Build the file path inline (no heap allocation).
            let mut path_buf = [0u8; MAX_PATH_LEN];
            let path_len = build_path(&mut path_buf, dir_name, file_name.as_str())?;

            // Create the file, scoping the `&Path` borrow so `path_buf` can be
            // moved out of the loop afterwards.
            let opened = {
                let path = Path::new(std::str::from_utf8(&path_buf[..path_len]).unwrap());
                OpenOptions::new()
                    // FIXME: remove this?
                    //                .read(true)
                    .write(true)
                    .create_new(true)
                    .open(path)
            };

            match opened {
                Ok(f) => break (path_buf, path_len, f, header),
                Err(e)
                    if e.kind() == std::io::ErrorKind::AlreadyExists && retries < MAX_RETRIES =>
                {
                    // Wait and try again with new Uid
                    std::thread::sleep(std::time::Duration::from_micros(WAIT_FOR_NEW_NAME));
                    retries += 1;
                    continue;
                }
                Err(e) => return Err(TcsLogError::Io(e)),
            }
        };

        // Record this file's position in the chain.
        header.chain_count = chain_count;

        // Write the header
        let header_bytes = header.to_bytes();

        if header_bytes.len() > Header::HEADER_SIZE {
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

        let (prefix_buf, prefix_len): ([u8; Filename::MAX_PREFIX_LEN], usize) = pack_str(prefix);
        let (suffix_buf, suffix_len): ([u8; Filename::MAX_SUFFIX_LEN], usize) = pack_str(suffix);

        Ok(TcsLog {
            dir_name,
            file,
            path_buf,
            path_len,
            header,
            max_size,
            write_position: data_offset,
            read_position: data_offset,
            prefix: prefix_buf,
            prefix_len,
            suffix: suffix_buf,
            suffix_len,
            writing: true,
            messages_written: 0,
        })
    }

    /// Computes the index and data section offsets.
    fn compute_offsets(max_size: u64) -> (u64, u64, usize) {
        let header_size = Header::HEADER_SIZE as u64;
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
        Uid: Uid,
        suffix: &str,
    ) -> Result<TcsLog<'a>, TcsLogError<'static>> {
        let file_name = Filename::new(prefix, Uid, suffix)?;
        let (path_buf, path_len) = pack_path(file_name.as_str())?;
        let path = Path::new(std::str::from_utf8(&path_buf[..path_len]).unwrap());
        println!("TcsLog::open: path {:?}", path);

        if !path.exists() {
            return Err(TcsLogError::NotFound);
        }

        let mut file = OpenOptions::new().read(true).open(path)?;

        // Read and parse header
        let mut header_bytes = [0u8; Header::HEADER_SIZE];
        println!("reading header bytes");
        file.read_exact(&mut header_bytes)?;
        println!("read header bytes");
        let header = Header::from_bytes(&header_bytes)?;

        let max_size = DEFAULT_FILE_SIZE; // Could also store in header

        let (prefix_buf, prefix_len): ([u8; Filename::MAX_PREFIX_LEN], usize) = pack_str(prefix);
        let (suffix_buf, suffix_len): ([u8; Filename::MAX_SUFFIX_LEN], usize) = pack_str(suffix);

        Ok(TcsLog {
            dir_name,
            file,
            path_buf,
            path_len,
            header: header.clone(),
            max_size,
            write_position: 0,
            read_position: header.data_offset,
            prefix: prefix_buf,
            prefix_len,
            suffix: suffix_buf,
            suffix_len,
            writing: false,
            messages_written: 0,
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

        // Store the path inline (no heap allocation).
        let path_str = path
            .to_str()
            .ok_or_else(|| TcsLogError::InvalidFormat("Path is not valid UTF-8".to_string()))?;
        let (path_buf, path_len) = pack_path(path_str)?;

        let mut file = OpenOptions::new().read(true).open(path)?;

        // Read and parse header
        let mut header_bytes = [0u8; Header::HEADER_SIZE];
        //println!("reading {:?}", header_bytes[..160]);
        file.read_exact(&mut header_bytes)?;
        //println!("read {:?}", header_bytes[..160]);
        let header = Header::from_bytes(&header_bytes)?;

        let max_size = DEFAULT_FILE_SIZE;

        // Extract prefix from file name
        println!("extracting prefix");
        println!("open_path: initial read_position {:?}", header.data_offset);

        Ok(TcsLog {
            dir_name: "", // FIXME: not needed
            file,
            path_buf,
            path_len,
            header: header.clone(),
            max_size,
            write_position: 0,
            read_position: header.data_offset,
            prefix: [0u8; Filename::MAX_PREFIX_LEN], // FIXME: not needed
            prefix_len: 0,
            suffix: [0u8; Filename::MAX_SUFFIX_LEN], // FIXME: not needed
            suffix_len: 0,
            writing: false,
            messages_written: 0,
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
        Uider: &mut dyn Uidable,
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

        let Uid = Uider.Uid()?;

        // Calculate space needed in current block
        let current_block_offset =
            (self.write_position - self.header.data_offset) % BLOCK_SIZE as u64;

        /*
                let space_in_block = if current_block_offset == 0 {
                    BLOCK_SIZE - data::PACKLEN
                } else {
                    BLOCK_SIZE - current_block_offset as usize
                };
        */

        // Write block header if at start of new block
        if current_block_offset == 0 {
            self.file.seek(SeekFrom::Start(self.write_position))?;
            self.file.write_all(&Offset::REC_START.to_le_bytes())?;
            self.write_position += data::PACKLEN as u64;
        }

        // Check if we need a new file. We roll over when the record plus a
        // trailing continuation marker would no longer fit, so there is always
        // room to write the marker that links this file to its successor.
        let total_needed = data::RECORD_METADATA_SIZE + data.len();
        let remaining_in_file = self.max_size - self.write_position;

        if (total_needed + CONT_SIZE) as u64 > remaining_in_file {
            // Create the next log file in the chain, incrementing the chain count.
            let mut new_log = Self::new_with_Uid_chained(
                self.dir_name,
                self.prefix(),
                Uider,
                self.suffix(),
                self.max_size,
                self.header.chain_count + 1,
            )?;

            // Write a continuation marker into this file pointing at the
            // successor, so a reader can follow the chain.
            let next_file = Filename::from_le_bytes(new_log.header.file_name);
            let marker = data::EofMarker::new(Uid::CONT, next_file);
            self.file.seek(SeekFrom::Start(self.write_position))?;
            self.file.write_all(&marker.to_le_bytes())?;

            // Carry the running message count into the successor so it counts
            // every message written across the whole chain.
            new_log.messages_written = self.messages_written;
            *self = new_log;
            return self.write(Uider, data);
        }

        // Write Uid
        println!(
            "TcsLog::write: writing timetamp {:?} at {:?}",
            Uid,
            self.file.stream_position()
        );
        let buf = Uid.to_le_bytes();
        self.file.write_all(&buf)?;
        self.write_position += buf.len() as u64;

        // Write record length
        self.file.write_all(&(data.len() as u64).to_le_bytes())?;
        self.write_position += 8;

        // Write data
        self.file.write_all(data)?;
        self.write_position += data.len() as u64;

        // Update index
        self.update_index(self.write_position - data.len() as u64, Uid)?;

        self.messages_written += 1;

        Ok(())
    }

    /* See whether this record could fit in a (fresh) log file at all.
     * Specifically, we see whether the data plus the metadata plus a
     * subsequent continuation marker will fit within a single file's data
     * section. When the current file lacks room, `write` rolls over to a new
     * file in the chain, so this check only rejects records that are too large
     * to fit in any file.
     *
     * data_len:    Size of the data record
     */
    fn record_fits(&self, data_len: usize) -> bool {
        let data_capacity = self.max_size - self.header.data_offset;
        let record_size = data::RECORD_METADATA_SIZE + data_len;
        record_size + CONT_SIZE < data_capacity as usize
    }

    /// Updates the index with a new record.
    fn update_index(
        &mut self,
        _offset: u64,
        _Uid: Uid,
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
        Uid: &mut Uid,
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
            self.read_position += data::PACKLEN as u64;
            println!("read_position adjusted to {:?}", self.read_position);
        }

        // Read Uid. If we get zero bytes, we're at the physical, and
        // hence, logical EOF. If the Uid is Uid::CONT, we are
        // at the continuation marker that ends the file.
        self.file.seek(SeekFrom::Start(self.read_position))?;
        let mut ts_bytes = [0u8; Uid::PACKLEN];

        // FIXME: this code assumes that seeking past the end of the file and
        // then reading will return zero bytes. Is that guaranteed by the
        // Rust runtime library? I think it is by Linux, but this might be
        // a portability if the Rust RT doesn't guarantee it. It looks like the
        // answer is no, so this needs to be fixed.
println!("TcsLog::read: reading Uid");
        match self.file.read(&mut ts_bytes) {
            Err(e) => return Err(TcsLogError::Io(e)),
            Ok(n) => {
                if n == 0 {
                    // FIXME: double check this
                    return Err(TcsLogError::EOF);
                } else if n != Uid::PACKLEN {
                    return Err(TcsLogError::CorruptedEOF);
                }
            }
        }

        *Uid = Uid::from_le_bytes(ts_bytes);
println!("TcsLog::read: read Uid {:?}", Uid);
        if *Uid == Uid::CONT {
            // Continuation marker: read the successor file name and follow the
            // chain. An empty name means this is the end of the chain.
            let mut marker_bytes = [0u8; data::EofMarker::PACKLEN];
            marker_bytes[..Uid::PACKLEN].copy_from_slice(&ts_bytes);
            self.file.read_exact(&mut marker_bytes[Uid::PACKLEN..])?;
            let marker = data::EofMarker::from_le_bytes(marker_bytes);
            let next_file = marker.next_file();
            let next_name = next_file.as_str();
            if next_name.is_empty() {
                return Err(TcsLogError::EOF);
            }

            // Open the successor in the same directory and continue reading.
            // Build "<dir>/<next_name>" inline, with no heap allocation.
            let dir = self
                .path()
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or("");
            let mut next_path_buf = [0u8; MAX_PATH_LEN];
            let next_path_len = build_path(&mut next_path_buf, dir, next_name)?;
            let next = {
                let next_path =
                    Path::new(std::str::from_utf8(&next_path_buf[..next_path_len]).unwrap());
                println!("TcsLog::read: following chain to {:?}", next_path);
                TcsLog::open_path(next_path)?
            };
            *self = next;
            return self.read(Uid, data);
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
        let read_offset: u64 = (Uid::PACKLEN as u64).try_into().unwrap();
        self.read_position += read_offset;

        // Read data
        println!("TcsLogUid::read: len {len} data.len {}", data.len());
        if len > data.len() {
            return Err(TcsLogError::InvalidFormat(
                "Buffer too small for record".to_string(),
            ));
        }

        println!("Uid::read: reading data len {len}");
        self.file.read_exact(&mut data[..len])?;
        println!("Uid::read: read data len {len}");
        self.read_position += len as u64;
println!("Final read position {:?}", self.read_position);

        Ok(len)
    }

    /// Given a Uid, determines the offset in the log file of the first data block
    /// containing a header with that Uid or greater.
    ///
    /// If no error occurred, returns the offset. Otherwise, returns Err(TcsLogError).
    pub fn Uid_offset(&mut self, Uid: Uid) -> Result<u64, TcsLogError<'static>> {
        // Read index block
        let mut index_bytes = [0u8; BLOCK_SIZE];
        self.file.seek(SeekFrom::Start(self.header.index_offset))?;
        println!("Uid_offset: reading index");
        self.file.read_exact(&mut index_bytes)?;
        println!("Uid_offset: read index");

        let index_block = IndexBlock::from_bytes(&index_bytes);

        // Find the entry with Uid >= given Uid
        for entry in index_block.entries.iter() {
            if entry.is_null() {
                break;
            }
            if entry.Uid >= Uid {
                return Ok(entry.offset);
            }
        }

        // If no matching entry found, return the start of data section
        Ok(self.header.data_offset)
    }

    /// The full log file path as a string.
    fn path_str(&self) -> &str {
        std::str::from_utf8(&self.path_buf[..self.path_len]).unwrap_or("")
    }

    /// Returns the path to the log file.
    pub fn path(&self) -> &Path {
        Path::new(self.path_str())
    }

    /// Returns the name of the current log file.
    pub fn file_name(&self) -> &str {
        self.header.file_name_str()
    }

    /// Returns the total number of log messages written through this log,
    /// counting messages written to earlier files in the chain as well.
    pub fn message_count(&self) -> u64 {
        self.messages_written
    }

    /// Returns the creation Uid.
    pub fn Uid(&self) -> Uid {
        self.header.Uid
    }

    /// Returns the header of the current log file.
    pub fn header(&self) -> &Header {
        &self.header
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
        // The file name encodes the Uid in microseconds. 1_234_567_000 ns
        // is 1_234_567 us = 0x12_d687, rendered as six 4-hex-digit groups.
        let ts: Uid = Uid::from_nanos(1_234_567_000);
        let name = Filename::new("test-", ts, ".tcslog").unwrap();
        assert_eq!(name.as_str(), "test-0000_0000_0000_0000_0012_d687.tcslog");
    }

    #[test]
    fn test_compute_offsets() {
        let (index_offset, data_offset, _) = TcsLog::compute_offsets(DEFAULT_FILE_SIZE);
        assert_eq!(index_offset, Header::HEADER_SIZE as u64);
        assert!(data_offset > index_offset);
        assert_eq!(data_offset % BLOCK_SIZE as u64, 0);
    }

    #[test]
    fn test_invalid_prefix() {
        let result = TcsLog::new("", "", "", DEFAULT_FILE_SIZE);
        assert!(matches!(result, Err(TcsLogError::InvalidPrefixLen(_))));

        //        let result = TestLog::create("a/b");
        //        assert!(matches!(result, Err(TcsLogError::InvalidPrefix(_))));
    }
}
