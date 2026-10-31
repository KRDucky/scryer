//! Stable names for the ways a processed-folder change is refused.
//!
//! Each travels beside the English sentence as `extensions.reason` on a
//! `VALIDATION_ERROR`, so a client can tell them apart and say them in the
//! operator's language. A name states the condition, never the wording, and
//! is API once released.

/// The folder id does not exist.
pub const NOT_FOUND: &str = "PROCESSED_FOLDER_NOT_FOUND";
/// The entry id does not exist.
pub const ENTRY_NOT_FOUND: &str = "PROCESSED_FOLDER_ENTRY_NOT_FOUND";
/// `name` is empty.
pub const NAME_REQUIRED: &str = "PROCESSED_FOLDER_NAME_REQUIRED";
/// `name` is longer than 100 Unicode scalar values.
pub const NAME_TOO_LONG: &str = "PROCESSED_FOLDER_NAME_TOO_LONG";
/// Another folder already uses this name.
pub const NAME_TAKEN: &str = "PROCESSED_FOLDER_NAME_TAKEN";
/// `name`, `rootPath`, or `markerValue` has leading or trailing whitespace.
pub const FIELD_NOT_TRIMMED: &str = "PROCESSED_FOLDER_FIELD_NOT_TRIMMED";
/// `rootPath` is not absolute.
pub const PATH_NOT_ABSOLUTE: &str = "PROCESSED_FOLDER_PATH_NOT_ABSOLUTE";
/// `rootPath` does not exist.
pub const PATH_MISSING: &str = "PROCESSED_FOLDER_PATH_MISSING";
/// `rootPath` is not a directory.
pub const PATH_NOT_DIRECTORY: &str = "PROCESSED_FOLDER_PATH_NOT_DIRECTORY";
/// `rootPath` cannot be canonicalized.
pub const PATH_UNRESOLVABLE: &str = "PROCESSED_FOLDER_PATH_UNRESOLVABLE";
/// The path equals, contains, or sits inside a library root.
pub const OVERLAPS_LIBRARY_ROOT: &str = "PROCESSED_FOLDER_OVERLAPS_LIBRARY_ROOT";
/// The path overlaps the enabled custom recycle bin.
pub const OVERLAPS_RECYCLE_BIN: &str = "PROCESSED_FOLDER_OVERLAPS_RECYCLE_BIN";
/// The path overlaps the configured backup directory.
pub const OVERLAPS_BACKUP_PATH: &str = "PROCESSED_FOLDER_OVERLAPS_BACKUP_PATH";
/// The path overlaps another processed folder, or the normalized root is taken.
pub const OVERLAPS_OTHER_FOLDER: &str = "PROCESSED_FOLDER_OVERLAPS_OTHER_FOLDER";
/// `libraryId` does not exist.
pub const LIBRARY_NOT_FOUND: &str = "PROCESSED_FOLDER_LIBRARY_NOT_FOUND";
/// `scope_kind` is not `library`.
pub const SCOPE_KIND_INVALID: &str = "PROCESSED_FOLDER_SCOPE_KIND_INVALID";
/// `minStableScans` or `minAgeSeconds` is outside its allowed range.
pub const INVALID_STABILITY: &str = "PROCESSED_FOLDER_INVALID_STABILITY";
/// A marker mode is missing its value, or the value is malformed.
pub const INVALID_MARKER: &str = "PROCESSED_FOLDER_INVALID_MARKER";
/// `markerValue` was given while the mode is `none`.
pub const MARKER_VALUE_NOT_ALLOWED: &str = "PROCESSED_FOLDER_MARKER_VALUE_NOT_ALLOWED";
/// Scan, import, or retry was asked of a disabled folder, or a claim lost that race.
pub const DISABLED: &str = "PROCESSED_FOLDER_DISABLED";
/// Another scan of the same folder already holds the scan lock.
pub const SCAN_IN_PROGRESS: &str = "PROCESSED_FOLDER_SCAN_IN_PROGRESS";
/// The entry's state does not allow this action, or the transition lost a race.
pub const ENTRY_NOT_ACTIONABLE: &str = "PROCESSED_FOLDER_ENTRY_NOT_ACTIONABLE";
/// The file's size, mtime, or canonical path changed before import.
pub const ENTRY_CHANGED: &str = "PROCESSED_FOLDER_ENTRY_CHANGED";
/// Import-as named a title that belongs to another library.
pub const TITLE_OUTSIDE_LIBRARY: &str = "PROCESSED_FOLDER_TITLE_OUTSIDE_LIBRARY";
/// Import-as named a title that does not exist.
pub const TITLE_NOT_FOUND: &str = "PROCESSED_FOLDER_TITLE_NOT_FOUND";
/// Delete was asked while an entry is still importing.
pub const IMPORTS_IN_FLIGHT: &str = "PROCESSED_FOLDER_IMPORTS_IN_FLIGHT";

/// Every reason a processed-folder refusal can name. Clients key translations
/// on these, so adding, renaming, or removing one must be deliberate.
pub const ALL: &[&str] = &[
    NOT_FOUND,
    ENTRY_NOT_FOUND,
    NAME_REQUIRED,
    NAME_TOO_LONG,
    NAME_TAKEN,
    FIELD_NOT_TRIMMED,
    PATH_NOT_ABSOLUTE,
    PATH_MISSING,
    PATH_NOT_DIRECTORY,
    PATH_UNRESOLVABLE,
    OVERLAPS_LIBRARY_ROOT,
    OVERLAPS_RECYCLE_BIN,
    OVERLAPS_BACKUP_PATH,
    OVERLAPS_OTHER_FOLDER,
    LIBRARY_NOT_FOUND,
    SCOPE_KIND_INVALID,
    INVALID_STABILITY,
    INVALID_MARKER,
    MARKER_VALUE_NOT_ALLOWED,
    DISABLED,
    SCAN_IN_PROGRESS,
    ENTRY_NOT_ACTIONABLE,
    ENTRY_CHANGED,
    TITLE_OUTSIDE_LIBRARY,
    TITLE_NOT_FOUND,
    IMPORTS_IN_FLIGHT,
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_reason_is_a_distinct_processed_folder_name_in_screaming_snake() {
        assert_eq!(ALL.len(), 26, "the contract table has 26 codes");
        let distinct = ALL.iter().collect::<BTreeSet<_>>();
        assert_eq!(distinct.len(), ALL.len(), "a reason is listed twice");
        for reason in ALL {
            assert!(reason.starts_with("PROCESSED_FOLDER_"), "{reason}");
            assert!(
                reason
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
                "{reason}"
            );
            assert!(!reason.ends_with('_') && !reason.contains("__"), "{reason}");
        }
    }
}
