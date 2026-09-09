//! Helper library for the `tcslog-gen` binary: creates a chain of sample
//! segment files whose data records match a caller-specified
//! [`RecordFormatSpec`].

use std::fmt;
use std::fs::File;
use std::path::Path;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use tcslog::{Format, LogError, LogWrite, RecSize, SEGMENT_FILE_HEADER_LEN,
    WriteCallbacks};

/// User-facing record-format specification parsed from the `--format`
/// command-line option. Combines the on-disk [`Format`] variant with the
/// payload-size range the generator should produce.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RecordFormatSpec {
    /// Every generated record is exactly `n` bytes long.
    Fixed(RecSize),
    /// [`Format::VariableSimple`] records whose payload length cycles
    /// through `[min, max]`.
    VariableSimple { min: RecSize, max: RecSize },
    /// [`Format::VariableTsRc`] records whose payload length cycles
    /// through `[min, max]`.
    VariableTsRc { min: RecSize, max: RecSize },
}

impl RecordFormatSpec {
    /// On-disk [`Format`] variant.
    #[must_use]
    pub fn format(self) -> Format {
        match self {
            Self::Fixed(n) => Format::Fixed(n),
            Self::VariableSimple { .. } => Format::VariableSimple,
            Self::VariableTsRc { .. } => Format::VariableTsRc,
        }
    }

    /// Minimum payload size the generator will produce.
    #[must_use]
    pub fn min_size(self) -> RecSize {
        match self {
            Self::Fixed(n) => n,
            Self::VariableSimple { min, .. } | Self::VariableTsRc { min, .. } => min,
        }
    }

    /// Maximum payload size the generator will produce.
    #[must_use]
    pub fn max_size(self) -> RecSize {
        match self {
            Self::Fixed(n) => n,
            Self::VariableSimple { max, .. } | Self::VariableTsRc { max, .. } => max,
        }
    }
}

impl fmt::Display for RecordFormatSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fixed(n) => write!(f, "fixed:{n}"),
            Self::VariableSimple { min, max } if min == max => {
                write!(f, "variable-simple:{min}")
            }
            Self::VariableSimple { min, max } => {
                write!(f, "variable-simple:{min}..{max}")
            }
            Self::VariableTsRc { min, max } if min == max => {
                write!(f, "variable-ts-rc:{min}")
            }
            Self::VariableTsRc { min, max } => {
                write!(f, "variable-ts-rc:{min}..{max}")
            }
        }
    }
}

/// Error returned when a `--format` string fails to parse or validate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordFormatSpecError(String);

impl RecordFormatSpecError {
    fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

impl fmt::Display for RecordFormatSpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RecordFormatSpecError {}

impl FromStr for RecordFormatSpec {
    type Err = RecordFormatSpecError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (kind, spec) = s.split_once(':').ok_or_else(|| {
            RecordFormatSpecError::new(format!(
                "invalid --format value {s:?}: expected 'KIND:LEN' or \
                 'KIND:MIN..MAX' where KIND is one of 'fixed', \
                 'variable-simple', 'variable-ts-rc'"
            ))
        })?;

        match kind {
            "fixed" => {
                let n = parse_len(spec, "fixed")?;
                if n == 0 {
                    return Err(RecordFormatSpecError::new(
                        "invalid --format value: 'fixed' length must be at \
                         least 1"
                            .to_string(),
                    ));
                }
                if spec.contains("..") {
                    return Err(RecordFormatSpecError::new(
                        "invalid --format value: 'fixed' takes a single \
                         length, not a range"
                            .to_string(),
                    ));
                }
                Ok(Self::Fixed(n))
            }
            "variable-simple" => {
                let (min, max) = parse_range(spec, "variable-simple")?;
                Ok(Self::VariableSimple { min, max })
            }
            "variable-ts-rc" => {
                let (min, max) = parse_range(spec, "variable-ts-rc")?;
                Ok(Self::VariableTsRc { min, max })
            }
            other => Err(RecordFormatSpecError::new(format!(
                "invalid --format value {s:?}: unknown kind {other:?}; \
                 expected 'fixed', 'variable-simple', or 'variable-ts-rc'"
            ))),
        }
    }
}

fn parse_range(spec: &str, kind: &str) -> Result<(RecSize, RecSize), RecordFormatSpecError> {
    if let Some((min_s, max_s)) = spec.split_once("..") {
        let min = parse_len(min_s, kind)?;
        let max = parse_len(max_s, kind)?;
        if min > max {
            return Err(RecordFormatSpecError::new(format!(
                "invalid --format value: {kind} min ({min}) must be less \
                 than or equal to max ({max})"
            )));
        }
        Ok((min, max))
    } else {
        let n = parse_len(spec, kind)?;
        Ok((n, n))
    }
}

fn parse_len(s: &str, kind: &str) -> Result<RecSize, RecordFormatSpecError> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(RecordFormatSpecError::new(format!(
            "invalid --format value: {kind} length is missing"
        )));
    }
    trimmed.parse::<RecSize>().map_err(|_| {
        RecordFormatSpecError::new(format!(
            "invalid --format value: {trimmed:?} is not a valid {kind} \
             length (expected a non-negative integer up to {})",
            RecSize::MAX
        ))
    })
}

/// Summary of a created log chain.
pub struct LogInfo {
    /// Base name of the first (root) segment file in the chain.
    pub root_file: String,
    /// Total number of records written across the chain.
    pub message_count: u64,
}

fn noop_record_complete(_f: &mut File) -> std::io::Result<()> {
    Ok(())
}

fn send(p: &Path) -> std::io::Result<()> {
    println!("--> Send file {}", p.display());
    Ok(())
}

const SAMPLE_WRITE_CALLBACKS: WriteCallbacks = WriteCallbacks {
    record_complete: noop_record_complete,
    send,
};

/// Creates a log chain in `dir_name` using the given file-name `prefix`
/// and `suffix`, with each data record shaped according to `spec`.
///
/// # Errors
///
/// Returns any [`LogError`] produced while creating segment files or
/// writing records.
pub fn create_log(
    dir_name: &str,
    prefix: &str,
    suffix: &str,
    spec: RecordFormatSpec,
    data_size: u32,
    count: u64,
    verbose: bool,
) -> Result<LogInfo, LogError> {
    let format = spec.format();
    let seg_size_max = SEGMENT_FILE_HEADER_LEN.saturating_add(data_size);

    if verbose {
        println!("Record format:      {spec}");
        println!("Segment file size:  {seg_size_max}");
        println!("Segment header:     {SEGMENT_FILE_HEADER_LEN}");
        println!(
            "Data section:       {}",
            seg_size_max - SEGMENT_FILE_HEADER_LEN,
        );
        println!();
    }

    let mut log = LogWrite::new(
        dir_name,
        prefix,
        suffix,
        seg_size_max,
        format,
        SAMPLE_WRITE_CALLBACKS,
    )?;

    let session_id = log.session_id();
    let root_file = format!("{prefix}{session_id}{suffix}");

    let mut message_count: u64 = 0;
    while message_count < count {
        let payload = build_payload(spec, message_count);
        if verbose {
            println!("record {} ({} bytes)", message_count + 1, payload.len());
        }
        log.write(&payload)?;
        message_count += 1;
    }

    Ok(LogInfo {
        root_file,
        message_count,
    })
}

fn build_payload(spec: RecordFormatSpec, index: u64) -> Vec<u8> {
    let min = spec.min_size();
    let max = spec.max_size();
    let target = if min == max {
        min
    } else {
        let span = u64::from(max - min) + 1;
        min + RecSize::try_from(index % span).unwrap_or(0)
    };

    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock is before 1970");
    let base = format!(
        "time={}.{:09}s message={}",
        dur.as_secs(),
        dur.subsec_nanos(),
        index + 1,
    );

    let target_usize = target as usize;
    let mut buf = base.into_bytes();
    if buf.len() > target_usize {
        buf.truncate(target_usize);
    } else {
        buf.resize(target_usize, b'.');
    }
    buf
}
