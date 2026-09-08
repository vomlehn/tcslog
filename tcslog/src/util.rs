//! Shared helpers that are not part of the public API.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::LogError;
use crate::segid::SegId;

/// Native path separator characters that must not appear inside a
/// prefix or suffix.
const PATH_SEPARATORS: &[char] = &['/', '\\'];

/// Rejects prefixes and suffixes that contain path separators.
pub(crate) fn check_no_path_delim(s: &str) -> Result<(), LogError> {
    if s.contains(PATH_SEPARATORS) {
        Err(LogError::PathDelimiterNotAllowed)
    } else {
        Ok(())
    }
}

/// Assembles the on-disk name of a segment file.
pub(crate) fn segment_file_name(prefix: &str, seg_id: SegId, suffix: &str) -> String {
    format!("{prefix}{seg_id}{suffix}")
}

/// Assembles the on-disk path of a segment file.
pub(crate) fn segment_path(
    dir: &Path,
    prefix: &str,
    seg_id: SegId,
    suffix: &str,
) -> PathBuf {
    dir.join(segment_file_name(prefix, seg_id, suffix))
}

/// Extracts the [`SegId`] embedded in a file name, or returns `None` if
/// the name does not match `<prefix><SegId::STR_LEN chars><suffix>`.
pub(crate) fn parse_segment_file_name(
    name: &str,
    prefix: &str,
    suffix: &str,
) -> Option<SegId> {
    if !name.starts_with(prefix) || !name.ends_with(suffix) {
        return None;
    }
    let inner = &name[prefix.len()..name.len() - suffix.len()];
    SegId::parse(inner)
}

/// Enumerates the segment files in `dir` whose names match the given
/// prefix and suffix, returning them sorted by segment identifier in
/// ascending order.
pub(crate) fn enumerate_segments(
    dir: &Path,
    prefix: &str,
    suffix: &str,
) -> Result<Vec<SegId>, LogError> {
    let entries = fs::read_dir(dir).map_err(LogError::IoError)?;
    let mut ids = Vec::new();
    for entry in entries {
        let entry = entry.map_err(LogError::IoError)?;
        let name = entry.file_name();
        let name = match name.to_str() {
            Some(s) => s.to_owned(),
            None => continue,
        };
        if let Some(id) = parse_segment_file_name(&name, prefix, suffix) {
            ids.push(id);
        }
    }
    ids.sort();
    Ok(ids)
}
