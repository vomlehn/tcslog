//! Error types for TcsLog operations.

use std::io;
use thiserror::Error;

use crate::TimestampableError;

#[derive(Debug, Error)]
pub enum TcsLogError {
	#[error("Header block too small")]
    BlockSizeTooSmall,
	#[error("I/O error: {0}")]
    Io(io::Error),
	#[error("Invalid format: {0}")]
    InvalidFormat(String),
	#[error("Record too large to fit in log file")]
    RecordTooLarge,
	#[error("End of log reached")]
    EndOfLog,
	#[error("Log file not found")]
    NotFound,
	#[error("Invalid timestamp")]
    InvalidTimestamp,
	#[error("Log file already exists")]
    AlreadyExists,
	#[error("Invalid prefix: {0}")]
    InvalidPrefix(String),
	#[error("Index corruption detected")]
    IndexCorrupted,
	#[error("Value too large")]
    ValueTooLarge,
    #[error("Timestamp error: {0}")]
    TimestampableError(TimestampableError),
    #[error("Invalid prefix in file path: {0}")]
    InvalidPrefixLen(usize),
}

impl From<std::io::Error> for TcsLogError {
    fn from(value: std::io::Error) -> Self {
        TcsLogError::Io(value)
    }
}

impl From<TimestampableError> for TcsLogError {
    fn from(value: TimestampableError) -> Self {
        TcsLogError::TimestampableError(value)
    }
}
