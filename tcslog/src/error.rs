//! Error types for TcsLog operations.

use std::fmt;
use std::io;

/// Error type for TcsLog operations.
#[derive(Debug)]
pub enum TcsLogError {
    /// I/O error occurred.
    Io(io::Error),
    /// Invalid file format or corrupted data.
    InvalidFormat(String),
    /// Record too large to fit in a log file.
    RecordTooLarge,
    /// No more records to read.
    EndOfLog,
    /// File not found.
    NotFound,
    /// Invalid timestamp.
    InvalidTimestamp,
    /// File already exists.
    AlreadyExists,
    /// Invalid prefix (too long or contains invalid characters).
    InvalidPrefix(String),
    /// Index corruption detected.
    IndexCorrupted,
}

impl fmt::Display for TcsLogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TcsLogError::Io(e) => write!(f, "I/O error: {}", e),
            TcsLogError::InvalidFormat(msg) => write!(f, "Invalid format: {}", msg),
            TcsLogError::RecordTooLarge => write!(f, "Record too large to fit in log file"),
            TcsLogError::EndOfLog => write!(f, "End of log reached"),
            TcsLogError::NotFound => write!(f, "Log file not found"),
            TcsLogError::InvalidTimestamp => write!(f, "Invalid timestamp"),
            TcsLogError::AlreadyExists => write!(f, "Log file already exists"),
            TcsLogError::InvalidPrefix(msg) => write!(f, "Invalid prefix: {}", msg),
            TcsLogError::IndexCorrupted => write!(f, "Index corruption detected"),
        }
    }
}

impl std::error::Error for TcsLogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TcsLogError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for TcsLogError {
    fn from(err: io::Error) -> Self {
        TcsLogError::Io(err)
    }
}
