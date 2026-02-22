//! Error types for TcsLog operations.

use std::io;
use thiserror::Error;

use crate::TimestampableError;

#[derive(Debug, Error)]
pub enum TcsLogError<'a> {
    #[error("Test error: {0}")]
    TestError(&'a str),
	#[error("Header block too small")]
    BlockSizeTooSmall,
	#[error("I/O error: {0}")]
    Io(io::Error),
	#[error("Invalid format: {0}")]
    InvalidFormat(String),
	#[error("Record too large to fit in log file")]
    RecordTooLarge,
	#[error("End of log reached")]
    EOF,
    #[error("Corrupted EOF")]
    CorruptedEOF,
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
    #[error("Log does not have an EOF marker at the end")]
    MissingEOF,
}

impl From<std::io::Error> for TcsLogError<'_> {
    fn from(value: std::io::Error) -> Self {
        TcsLogError::Io(value)
    }
}

impl From<TimestampableError> for TcsLogError<'_> {
    fn from(value: TimestampableError) -> Self {
        TcsLogError::TimestampableError(value)
    }
}
