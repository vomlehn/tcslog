//! Shared helpers: segment file naming, directory scanning, and the two
//! rendering functions the support binaries print through.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::LogError;
use crate::format::Meta;
use crate::segid::SegId;
use crate::Timestamp;

/// Nanoseconds in one second.
const NANOS_PER_SEC: u64 = 1_000_000_000;
/// Seconds in one day.
const SECS_PER_DAY: u64 = 86_400;

/// Reports whether a prefix or suffix holds a path separator.
///
/// Both separators are rejected on every platform rather than only the
/// local one, so that a prefix accepted on one target is accepted on all
/// of them. A prefix that could hold a separator could steer a segment
/// file out of the directory the caller named.
///
/// * `text` -- the prefix or suffix to examine.
///
/// Returns true if `text` holds a path separator.
pub(crate) fn has_path_delimiter(text: &str) -> bool {
    text.chars()
        .any(|c| c == '/' || c == '\\' || std::path::is_separator(c))
}

/// Checks the arguments common to opening a log for reading and for
/// writing.
///
/// * `dir` -- directory that holds the segment files. It must already
///   exist; neither entry point creates it.
/// * `prefix` -- first part of every segment file name.
/// * `suffix` -- last part of every segment file name.
///
/// Returns the directory as a [`PathBuf`].
///
/// # Errors
///
/// Returns [`LogError::PathDelimiterNotAllowed`] if either the prefix or
/// the suffix holds a path separator, and
/// [`LogError::InvalidPathname`] if `dir` does not name a directory.
pub(crate) fn check_log_location(
    dir: &str,
    prefix: &str,
    suffix: &str,
) -> Result<PathBuf, LogError> {
    if has_path_delimiter(prefix) || has_path_delimiter(suffix) {
        return Err(LogError::PathDelimiterNotAllowed);
    }
    let dir = PathBuf::from(dir);
    if !dir.is_dir() {
        return Err(LogError::InvalidPathname);
    }
    Ok(dir)
}

/// Builds a segment file name into a reusable buffer.
///
/// The buffer is reused so that opening one segment file after another
/// does not allocate, which is what lets a reader honour the crate's
/// no-allocation guarantee while walking a log.
///
/// * `buf` -- the name buffer, overwritten.
/// * `prefix` -- first part of the name.
/// * `id` -- segment ID, rendered between the prefix and the suffix.
/// * `suffix` -- last part of the name.
pub(crate) fn build_name(buf: &mut String, prefix: &str, id: SegId, suffix: &str) {
    use std::fmt::Write as _;

    buf.clear();
    buf.push_str(prefix);
    // Writing into a String cannot fail, so there is no error to report.
    let _ = write!(buf, "{id}");
    buf.push_str(suffix);
}

/// Extracts the segment ID from a file name built by [`build_name`].
///
/// * `name` -- the file name to examine.
/// * `prefix` -- the prefix the log's names begin with.
/// * `suffix` -- the suffix the log's names end with.
///
/// Returns the identifier, or `None` if the name does not belong to this
/// log. A name that does not match is not an error: a directory may hold
/// anything at all beside a log's segment files.
pub(crate) fn id_from_name(name: &str, prefix: &str, suffix: &str) -> Option<SegId> {
    if name.len() != prefix.len() + SegId::STR_LEN + suffix.len() {
        return None;
    }
    let body = name.strip_prefix(prefix)?.strip_suffix(suffix)?;
    SegId::parse(body).ok()
}

/// Lists a log's segment files, oldest first.
///
/// The identifiers are parsed out of the names and sorted, which is the
/// order actually meant: oldest to newest, because an identifier is the
/// time the file was created. Sorting the names as text would give the
/// same order, the identifier being fixed-width zero-filled hexadecimal,
/// but on the parsed values the intent is visible.
///
/// * `dir` -- directory to scan.
/// * `prefix` -- first part of the names to accept.
/// * `suffix` -- last part of the names to accept.
///
/// Returns the sorted identifiers, which is empty when the directory
/// holds no segment file of this log.
///
/// # Errors
///
/// Returns [`LogError::IoError`] if the directory cannot be enumerated.
pub(crate) fn scan_segment_ids(
    dir: &Path,
    prefix: &str,
    suffix: &str,
) -> Result<Vec<SegId>, LogError> {
    let mut ids = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if let Some(id) = id_from_name(name, prefix, suffix) {
            ids.push(id);
        }
    }
    ids.sort_unstable();
    Ok(ids)
}

/// Renders a timestamp as an ISO 8601 UTC date and time with nanosecond
/// precision, for example `2026-09-21T16:45:12.123456789Z`.
///
/// The conversion is done here rather than through a date-and-time crate
/// so that the dependency set stays small enough to audit, which matters
/// for software that flies.
///
/// * `ts` -- nanoseconds since the UNIX epoch.
///
/// Returns the rendered string.
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
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{nanos:09}Z")
}

/// Converts a count of days since 1970-01-01 into a calendar date.
///
/// This is Howard Hinnant's `civil_from_days`, restricted to dates at or
/// after the epoch, which is the only range a `u64` nanosecond timestamp
/// can reach.
///
/// * `days` -- days since 1970-01-01.
///
/// Returns the year, month, and day.
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    // Shift the epoch to 0000-03-01, which puts the leap day at the end
    // of a 400-year era and makes the arithmetic uniform.
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// Renders a record's length and its format-specific metadata as the
/// parenthesised trailer the support binaries print after a payload.
///
/// Both `tcslog-gen` and `tcslog-dump` print through this, so a record
/// reads the same coming out of a log as it did going in.
///
/// * `payload_len` -- number of payload bytes the record holds.
/// * `meta` -- the metadata the record carried.
///
/// Returns the trailer, parentheses included.
#[must_use]
pub fn record_trailer(payload_len: usize, meta: Meta) -> String {
    match meta {
        Meta::Fixed | Meta::VariableSimple => format!("({payload_len} bytes)"),
        Meta::VariableTsRc(ts, rc) => {
            format!(
                "({payload_len} bytes, ts={}, rc={rc})",
                format_timestamp(ts)
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_renders_as_nineteen_seventy() {
        assert_eq!(format_timestamp(0), "1970-01-01T00:00:00.000000000Z");
    }

    #[test]
    fn known_instant_renders_correctly() {
        assert_eq!(
            format_timestamp(1_234_567_890_000_000_000),
            "2009-02-13T23:31:30.000000000Z"
        );
    }

    #[test]
    fn nanoseconds_are_zero_padded_to_nine_digits() {
        assert_eq!(
            format_timestamp(1_000_000_000 + 42),
            "1970-01-01T00:00:01.000000042Z"
        );
    }

    #[test]
    fn leap_day_is_handled() {
        // 2000-02-29T00:00:00Z
        assert_eq!(
            format_timestamp(951_782_400 * NANOS_PER_SEC),
            "2000-02-29T00:00:00.000000000Z"
        );
    }

    #[test]
    fn trailer_reports_length_alone_without_metadata() {
        assert_eq!(record_trailer(7, Meta::Fixed), "(7 bytes)");
        assert_eq!(record_trailer(0, Meta::VariableSimple), "(0 bytes)");
    }

    #[test]
    fn trailer_reports_timestamp_and_record_count() {
        assert_eq!(
            record_trailer(10, Meta::VariableTsRc(1_234_567_890_000_000_000, 5)),
            "(10 bytes, ts=2009-02-13T23:31:30.000000000Z, rc=5)"
        );
    }

    #[test]
    fn path_delimiters_are_rejected_on_every_platform() {
        assert!(has_path_delimiter("a/b"));
        assert!(has_path_delimiter("a\\b"));
        assert!(!has_path_delimiter("pfx-"));
        assert!(!has_path_delimiter(".log"));
    }

    #[test]
    fn names_round_trip_through_the_identifier() {
        let mut buf = String::new();
        let id = SegId::from_u64(0x1234_abcd_5678_efab);
        build_name(&mut buf, "pfx-", id, ".log");
        assert_eq!(buf, "pfx-1234-abcd-5678-efab.log");
        assert_eq!(id_from_name(&buf, "pfx-", ".log"), Some(id));
    }

    #[test]
    fn foreign_names_are_not_segment_files() {
        for name in [
            "pfx-1234-abcd-5678-efab.txt",
            "other-1234-abcd-5678-efab.log",
            "pfx-.log",
            "pfx-1234-abcd-5678-efabX.log",
            "README",
        ] {
            assert_eq!(id_from_name(name, "pfx-", ".log"), None, "accepted {name}");
        }
    }

    #[test]
    fn a_reused_name_buffer_does_not_keep_the_old_name() {
        let mut buf = String::new();
        build_name(&mut buf, "long-prefix-", SegId::from_u64(1), ".suffix");
        build_name(&mut buf, "p", SegId::from_u64(2), "");
        assert_eq!(buf, "p0000-0000-0000-0002");
    }
}
