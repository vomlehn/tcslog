//! Small helpers shared by the read and write halves of tcslog: file-name
//! validation and pattern matching against the log's `<prefix><id><suffix>`
//! convention.

use crate::segid::{SegId, SEG_ID_STR_LEN};
use crate::LogError;

/// Reject prefixes and suffixes that contain a path delimiter. The check
/// covers both the platform-native separator and `/`, since Windows also
/// treats `/` as a separator.
pub(crate) fn validate_prefix_suffix(prefix: &str, suffix: &str) -> Result<(), LogError> {
    for s in [prefix, suffix] {
        if s.contains('/') || s.contains(std::path::MAIN_SEPARATOR) {
            return Err(LogError::HasDelimiter);
        }
    }
    Ok(())
}

/// True when `name` matches the log's `<prefix><id><suffix>` shape and the
/// middle portion parses as a [`SegId`].
pub(crate) fn is_segment_name(name: &str, prefix: &str, suffix: &str) -> bool {
    if name.len() != prefix.len() + SEG_ID_STR_LEN + suffix.len() {
        return false;
    }
    if !name.starts_with(prefix) || !name.ends_with(suffix) {
        return false;
    }
    let mid = &name[prefix.len()..name.len() - suffix.len()];
    mid.parse::<SegId>().is_ok()
}

/// Extract the segment ID from a name known to match [`is_segment_name`].
pub(crate) fn parse_segment_name(name: &str, prefix: &str, suffix: &str) -> Option<SegId> {
    if !is_segment_name(name, prefix, suffix) {
        return None;
    }
    name[prefix.len()..name.len() - suffix.len()].parse::<SegId>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_name() {
        assert!(is_segment_name(
            "pre_0000-0000-0000-0001_suf",
            "pre_",
            "_suf",
        ));
    }

    #[test]
    fn rejects_bad_id() {
        assert!(!is_segment_name(
            "pre_zzzz-zzzz-zzzz-zzzz_suf",
            "pre_",
            "_suf",
        ));
    }

    #[test]
    fn rejects_wrong_shape() {
        assert!(!is_segment_name("something_else", "pre_", "_suf"));
    }

    #[test]
    fn parse_returns_id() {
        let id = parse_segment_name("pre_0000-0000-0000-000f_suf", "pre_", "_suf").unwrap();
        assert_eq!(id.as_u64(), 0x0f);
    }

    #[test]
    fn delimiter_rejected() {
        assert!(matches!(
            validate_prefix_suffix("bad/pre", "suf"),
            Err(LogError::HasDelimiter),
        ));
        assert!(matches!(
            validate_prefix_suffix("pre", "bad/suf"),
            Err(LogError::HasDelimiter),
        ));
        assert!(validate_prefix_suffix("pre_", "_suf").is_ok());
    }
}
