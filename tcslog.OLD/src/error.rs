//! Error types for TcsLog operations.

use std::io;
use thiserror::Error;

use crate::UidableError;

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
    #[error("Invalid Uid")]
    InvalidUid,
    #[error("Log file already exists")]
    AlreadyExists,
    #[error("Invalid prefix character: {0}")]
    InvalidPrefixChar(String),
    #[error("Prefix lengths must by 1 < len <= {0}")]
    InvalidPrefixLen(usize),
    #[error("Invalid suffix character: {0}")]
    InvalidSuffixChar(String),
    #[error("Suffix lengths must by 1 < len <= {0}")]
    InvalidSuffixLen(usize),
    #[error("Index corruption detected")]
    IndexCorrupted,
    #[error("Value too large")]
    ValueTooLarge,
    #[error("Uid error: {0}")]
    UidableError(UidableError),
    #[error("Log does not have an EOF marker at the end")]
    MissingEOF,
}

impl From<std::io::Error> for TcsLogError<'_> {
    fn from(value: std::io::Error) -> Self {
        TcsLogError::Io(value)
    }
}

impl From<UidableError> for TcsLogError<'_> {
    fn from(value: UidableError) -> Self {
        TcsLogError::UidableError(value)
    }
}
