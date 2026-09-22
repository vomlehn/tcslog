//! Segment-file data formats and per-record metadata.

use std::fmt;

/// Type used for the size (in bytes) of the telemetry payload portion of
/// a data record.
pub type RecSize = u32;

/// Nanoseconds since the UNIX epoch, as stored with
/// [`Format::VariableTsRc`] records.
pub type Timestamp = u64;

/// Monotonically increasing record identifier stored with
/// [`Format::VariableTsRc`] records. Starts at one for the first record
/// of a session.
pub type RecordCount = u64;

/// Selects how records are laid out in the data section of a segment
/// file.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Format {
    /// Every record contains exactly `n` payload bytes. There is no
    /// per-record header. `n` must be at least one and at most
    /// [`RecSize::MAX`](RecSize).
    Fixed(RecSize),
    /// Records carry a length prefix and a variable-size payload from
    /// zero up to [`RecSize::MAX`](RecSize) bytes long. No other
    /// metadata is written per record.
    VariableSimple,
    /// Like [`Format::VariableSimple`], but each record also carries a
    /// [`Timestamp`] and a [`RecordCount`].
    VariableTsRc,
}

impl Format {
    /// The single-byte tag written in the segment header to identify this
    /// format.
    #[must_use]
    pub const fn tag(self) -> u8 {
        match self {
            Format::Fixed(_) => 0,
            Format::VariableSimple => 1,
            Format::VariableTsRc => 2,
        }
    }

    /// The `n` parameter for [`Format::Fixed`], or zero for other formats.
    #[must_use]
    pub const fn fixed_len(self) -> RecSize {
        match self {
            Format::Fixed(n) => n,
            _ => 0,
        }
    }

    /// Number of bytes the per-record data header consumes on disk.
    #[must_use]
    pub const fn data_header_len(self) -> u32 {
        match self {
            Format::Fixed(_) => 0,
            Format::VariableSimple => 4,
            Format::VariableTsRc => 4 + 8 + 8,
        }
    }
}

/// Per-record metadata returned to the reader alongside the payload.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Meta {
    /// The segment file uses [`Format::Fixed`].
    Fixed,
    /// The segment file uses [`Format::VariableSimple`]; no metadata is
    /// carried with the record.
    VariableSimple,
    /// The segment file uses [`Format::VariableTsRc`]. The first value
    /// is the [`Timestamp`]; the second is the [`RecordCount`].
    VariableTsRc(Timestamp, RecordCount),
}

/// Nanoseconds in one second.
const NANOS_PER_SEC: u64 = 1_000_000_000;

/// Seconds in one day.
const SECS_PER_DAY: u64 = 86_400;

/// Renders a [`Timestamp`] as an ISO 8601 UTC date and time with
/// nanosecond precision, for example
/// `2026-09-21T16:45:12.123456789Z`.
///
/// The conversion is done here rather than through a date-time crate so
/// that the library keeps its minimal dependency set.
#[must_use]
pub fn format_timestamp(ts: Timestamp) -> String {
    let secs = ts / NANOS_PER_SEC;
    let nanos = ts % NANOS_PER_SEC;
    let days = secs / SECS_PER_DAY;
    let secs_of_day = secs % SECS_PER_DAY;

    let (year, month, day) = civil_from_days(days);
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;

    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:\
         {second:02}.{nanos:09}Z"
    )
}

/// Converts a count of days since the UNIX epoch (1970-01-01) into a
/// proleptic Gregorian `(year, month, day)`.
///
/// This is Howard Hinnant's `civil_from_days` algorithm, specialised to
/// the non-negative range a [`Timestamp`] can express (the epoch through
/// roughly the year 2554).
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    // Shift the era origin from 1970-01-01 to 0000-03-01 so that the
    // leap day lands at the end of the (shifted) year.
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

impl fmt::Display for Meta {
    /// Formats the per-record metadata as comma-separated `field=value`
    /// pairs. Formats that carry no metadata render as the empty string.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fixed | Self::VariableSimple => Ok(()),
            Self::VariableTsRc(ts, rc) => {
                write!(f, "ts={}, rc={rc}", format_timestamp(*ts))
            }
        }
    }
}

/// Renders the parenthesised trailer that `tcslog-gen` and
/// `tcslog-dump` print after each record's payload: the payload length
/// followed by whatever metadata the record format carries.
///
/// Both tools share this so that generated and dumped output stay in
/// the same shape.
#[must_use]
pub fn record_trailer(payload_len: usize, meta: Meta) -> String {
    match meta {
        Meta::Fixed | Meta::VariableSimple => {
            format!("({payload_len} bytes)")
        }
        Meta::VariableTsRc(..) => {
            format!("({payload_len} bytes, {meta})")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whole-second reference points, including the century leap year
    /// 2000, the ordinary leap year 2024, and 2100 (a multiple of 100
    /// that is not a leap year).
    #[test]
    fn format_timestamp_matches_known_dates() {
        let cases = [
            (0, "1970-01-01T00:00:00.000000000Z"),
            (1_234_567_890, "2009-02-13T23:31:30.000000000Z"),
            (951_782_400, "2000-02-29T00:00:00.000000000Z"),
            (1_709_164_800, "2024-02-29T00:00:00.000000000Z"),
            (4_107_542_400, "2100-03-01T00:00:00.000000000Z"),
        ];
        for (secs, expected) in cases {
            let ts = secs * NANOS_PER_SEC;
            assert_eq!(format_timestamp(ts), expected, "secs={secs}");
        }
    }

    #[test]
    fn format_timestamp_keeps_sub_second_precision() {
        assert_eq!(
            format_timestamp(123_456_789),
            "1970-01-01T00:00:00.123456789Z"
        );
        assert_eq!(
            format_timestamp(NANOS_PER_SEC - 1),
            "1970-01-01T00:00:00.999999999Z"
        );
    }

    /// The largest value a `Timestamp` can hold must still render, since
    /// the reader will format whatever bytes it finds on disk.
    #[test]
    fn format_timestamp_handles_max() {
        assert_eq!(
            format_timestamp(Timestamp::MAX),
            "2554-07-21T23:34:33.709551615Z"
        );
    }

    #[test]
    fn trailer_omits_metadata_when_format_carries_none() {
        assert_eq!(record_trailer(7, Meta::Fixed), "(7 bytes)");
        assert_eq!(record_trailer(0, Meta::VariableSimple), "(0 bytes)");
    }

    #[test]
    fn trailer_includes_timestamp_and_record_count() {
        let meta = Meta::VariableTsRc(1_234_567_890 * NANOS_PER_SEC, 42);
        assert_eq!(
            record_trailer(9, meta),
            "(9 bytes, ts=2009-02-13T23:31:30.000000000Z, rc=42)"
        );
    }
}
