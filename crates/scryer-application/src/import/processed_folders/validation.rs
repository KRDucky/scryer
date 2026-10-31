//! Field and overlap rules for creating or updating a processed folder.
//!
//! Check order matches the refusal table: trim, then name, then path, then
//! library, then scope, then stability, then marker, then overlaps. The store
//! maps the two unique indexes after that.

use std::path::{Path, PathBuf};

use scryer_domain::processed_folder::{ProcessedFolderMarkerMode, ProcessedFolderScopeKind};

use crate::processed_folder_refusal::{
    FIELD_NOT_TRIMMED, INVALID_MARKER, INVALID_STABILITY, MARKER_VALUE_NOT_ALLOWED, NAME_REQUIRED,
    NAME_TOO_LONG, OVERLAPS_BACKUP_PATH, OVERLAPS_LIBRARY_ROOT, OVERLAPS_OTHER_FOLDER,
    OVERLAPS_RECYCLE_BIN, PATH_MISSING, PATH_NOT_ABSOLUTE, PATH_NOT_DIRECTORY, PATH_UNRESOLVABLE,
    SCOPE_KIND_INVALID,
};
use crate::{AppError, AppResult, AppUseCase};

pub(crate) const DEFAULT_MIN_STABLE_SCANS: i64 = 2;
pub(crate) const DEFAULT_MIN_AGE_SECONDS: i64 = 300;
const MIN_STABLE_SCANS: i64 = 2;
const MAX_STABLE_SCANS: i64 = 10;
const MIN_AGE_SECONDS: i64 = 60;
const MAX_AGE_SECONDS: i64 = 86_400;
const MAX_NAME_CHARS: usize = 100;

/// What an update does with `markerValue`. Null clears; an omitted field keeps
/// the stored value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkerValueUpdate {
    Keep,
    Clear,
    Set(String),
}

pub(crate) struct ProcessedFolderWrite {
    pub name: String,
    pub root_path: String,
    pub library_id: String,
    pub scope_kind: ProcessedFolderScopeKind,
    pub min_stable_scans: i64,
    pub min_age_seconds: i64,
    pub marker_mode: ProcessedFolderMarkerMode,
    pub marker_value: Option<String>,
}

pub(crate) struct CheckedProcessedFolder {
    pub name: String,
    pub root_path: String,
    pub normalized_root_path: String,
    pub library_id: String,
    pub library_name: String,
    pub scope_kind: ProcessedFolderScopeKind,
    pub min_stable_scans: i64,
    pub min_age_seconds: i64,
    pub marker_mode: ProcessedFolderMarkerMode,
    pub marker_value: Option<String>,
}

fn refused(reason: &'static str, message: impl Into<String>) -> AppError {
    AppError::validation_refused(reason, message)
}

fn has_outer_whitespace(value: &str) -> bool {
    value != value.trim()
}

/// Same component walk as `normalize_recycle_config_path`. That helper is
/// private to the settings module, so the processed-folder path uses this copy.
pub(crate) fn normalize_processed_folder_root(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir => normalized.push(component.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::Normal(segment) => normalized.push(segment),
        }
    }
    normalized
}

/// Equal, ancestor, or descendant. Component-wise, so a shared name prefix is
/// not an overlap.
pub(crate) fn processed_folder_paths_overlap(left: &Path, right: &Path) -> bool {
    if left.as_os_str().is_empty() || right.as_os_str().is_empty() {
        return false;
    }
    left == right || left.starts_with(right) || right.starts_with(left)
}

fn unresolvable(path: &Path, error: &std::io::Error) -> AppError {
    refused(
        PATH_UNRESOLVABLE,
        format!(
            "processed folder path {} cannot be resolved: {error}",
            path.display()
        ),
    )
}

/// Lexical and canonical overlap of a resolved candidate with `other`.
///
/// An existing `other` that cannot be canonicalized is [`PATH_UNRESOLVABLE`],
/// so a broken link cannot hide an overlap. A missing `other` is compared
/// lexically only.
pub(crate) fn processed_folder_path_conflicts(
    candidate_normalized: &Path,
    candidate_canonical: &Path,
    other: &Path,
) -> AppResult<bool> {
    let other_normalized = normalize_processed_folder_root(other);
    if processed_folder_paths_overlap(candidate_normalized, &other_normalized)
        || processed_folder_paths_overlap(candidate_canonical, &other_normalized)
    {
        return Ok(true);
    }
    match std::fs::symlink_metadata(&other_normalized) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(unresolvable(&other_normalized, &error)),
        Ok(_) => match std::fs::canonicalize(&other_normalized) {
            Ok(canonical) => Ok(
                processed_folder_paths_overlap(candidate_normalized, &canonical)
                    || processed_folder_paths_overlap(candidate_canonical, &canonical),
            ),
            Err(error) => Err(unresolvable(&other_normalized, &error)),
        },
    }
}

pub(crate) fn validate_processed_folder_name(name: &str) -> AppResult<()> {
    if name.is_empty() {
        return Err(refused(NAME_REQUIRED, "processed folder name is required"));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(refused(
            NAME_TOO_LONG,
            "processed folder name must be at most 100 characters",
        ));
    }
    Ok(())
}

pub(crate) fn resolve_processed_folder_root(
    root_path: &str,
) -> AppResult<(String, PathBuf, PathBuf)> {
    let path = Path::new(root_path);
    if !path.is_absolute() {
        return Err(refused(
            PATH_NOT_ABSOLUTE,
            "processed folder path must be absolute",
        ));
    }
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(refused(
                PATH_MISSING,
                "processed folder path does not exist",
            ));
        }
        Err(error) => return Err(unresolvable(path, &error)),
        Ok(_) => {}
    }
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            return Err(refused(
                PATH_NOT_DIRECTORY,
                "processed folder path is not a directory",
            ));
        }
        Err(error) => return Err(unresolvable(path, &error)),
    }
    let canonical = std::fs::canonicalize(path).map_err(|error| unresolvable(path, &error))?;
    let normalized = normalize_processed_folder_root(path);
    Ok((root_path.to_string(), normalized, canonical))
}

pub(crate) fn validate_processed_folder_stability(
    min_stable_scans: i64,
    min_age_seconds: i64,
) -> AppResult<()> {
    if !(MIN_STABLE_SCANS..=MAX_STABLE_SCANS).contains(&min_stable_scans)
        || !(MIN_AGE_SECONDS..=MAX_AGE_SECONDS).contains(&min_age_seconds)
    {
        return Err(refused(
            INVALID_STABILITY,
            "min stable scans must be from 2 to 10 and min age must be from 60 to 86400 seconds",
        ));
    }
    Ok(())
}

pub(crate) fn validate_processed_folder_marker(
    mode: ProcessedFolderMarkerMode,
    value: Option<&str>,
) -> AppResult<Option<String>> {
    match mode {
        ProcessedFolderMarkerMode::SiblingSuffix => {
            let Some(value) = value.filter(|value| !value.is_empty()) else {
                return Err(refused(
                    INVALID_MARKER,
                    "a sibling marker needs a suffix such as .done",
                ));
            };
            let length = value.chars().count();
            let malformed = !value.starts_with('.')
                || !(2..=32).contains(&length)
                || value.chars().any(|c| c == '/' || c == '\\' || c == '\0')
                || value.contains("..");
            if malformed {
                return Err(refused(
                    INVALID_MARKER,
                    "a sibling marker suffix must start with a dot, be 2 to 32 characters, and contain no slash, backslash, NUL, or '..'",
                ));
            }
            Ok(Some(value.to_string()))
        }
        ProcessedFolderMarkerMode::DirectoryFile => {
            let Some(value) = value.filter(|value| !value.is_empty()) else {
                return Err(refused(
                    INVALID_MARKER,
                    "a directory marker needs a file name",
                ));
            };
            let malformed = !(1..=255).contains(&value.len())
                || value == "."
                || value == ".."
                || value.chars().any(|c| c == '/' || c == '\\' || c == '\0');
            if malformed {
                return Err(refused(
                    INVALID_MARKER,
                    "a directory marker must be a single file name of 1 to 255 bytes, not '.' or '..', and contain no slash, backslash, or NUL",
                ));
            }
            Ok(Some(value.to_string()))
        }
        ProcessedFolderMarkerMode::None => {
            if value.is_some() {
                return Err(refused(
                    MARKER_VALUE_NOT_ALLOWED,
                    "marker value is only allowed when a marker mode is set",
                ));
            }
            Ok(None)
        }
    }
}

fn refuse_untrimmed(name: &str, root_path: &str, marker_value: Option<&str>) -> AppResult<()> {
    if has_outer_whitespace(name)
        || has_outer_whitespace(root_path)
        || marker_value.is_some_and(has_outer_whitespace)
    {
        return Err(refused(
            FIELD_NOT_TRIMMED,
            "name, root path, and marker value must not have leading or trailing whitespace",
        ));
    }
    Ok(())
}

impl AppUseCase {
    /// The processed folder whose root equals, contains, or sits inside
    /// `candidate`, if any. Reverse checks (a new library root or recycle bin)
    /// call this before saving.
    #[allow(dead_code)] // Core's reverse checks call this. The base branch has no caller yet.
    pub(crate) async fn overlapping_processed_folder_id(
        &self,
        candidate: &Path,
        ignore_folder_id: Option<&str>,
    ) -> AppResult<Option<String>> {
        let normalized = normalize_processed_folder_root(candidate);
        let canonical = match std::fs::symlink_metadata(&normalized) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => normalized.clone(),
            Err(error) => return Err(unresolvable(&normalized, &error)),
            Ok(_) => std::fs::canonicalize(&normalized)
                .map_err(|error| unresolvable(&normalized, &error))?,
        };
        self.processed_folder_overlap_code(&normalized, &canonical, ignore_folder_id)
            .await
            .map(|overlap| overlap.map(|(_code, id)| id))
    }

    pub(crate) async fn validate_processed_folder_write(
        &self,
        write: &ProcessedFolderWrite,
        ignore_folder_id: Option<&str>,
    ) -> AppResult<CheckedProcessedFolder> {
        refuse_untrimmed(&write.name, &write.root_path, write.marker_value.as_deref())?;
        validate_processed_folder_name(&write.name)?;
        let (root_path, normalized, canonical) = resolve_processed_folder_root(&write.root_path)?;
        let library = self
            .services
            .catalog
            .libraries
            .get_by_id(&write.library_id)
            .await?
            .ok_or_else(|| {
                refused(
                    crate::processed_folder_refusal::LIBRARY_NOT_FOUND,
                    "library was not found",
                )
            })?;
        if write.scope_kind != ProcessedFolderScopeKind::Library {
            return Err(refused(
                SCOPE_KIND_INVALID,
                "processed folder scope must be library",
            ));
        }
        validate_processed_folder_stability(write.min_stable_scans, write.min_age_seconds)?;
        let marker_value =
            validate_processed_folder_marker(write.marker_mode, write.marker_value.as_deref())?;
        if let Some(code) = self
            .processed_folder_overlap_code(&normalized, &canonical, ignore_folder_id)
            .await?
            .map(|(code, _id)| code)
        {
            return Err(refused(code, overlap_message(code)));
        }
        Ok(CheckedProcessedFolder {
            name: write.name.clone(),
            root_path,
            normalized_root_path: normalized.to_string_lossy().into_owned(),
            library_id: library.id,
            library_name: library.name,
            scope_kind: write.scope_kind,
            min_stable_scans: write.min_stable_scans,
            min_age_seconds: write.min_age_seconds,
            marker_mode: write.marker_mode,
            marker_value,
        })
    }

    async fn processed_folder_overlap_code(
        &self,
        normalized: &Path,
        canonical: &Path,
        ignore_folder_id: Option<&str>,
    ) -> AppResult<Option<(&'static str, String)>> {
        let roots = self.all_library_root_folders().await?;
        for root in &roots {
            if processed_folder_path_conflicts(normalized, canonical, Path::new(root.path.trim()))?
            {
                return Ok(Some((OVERLAPS_LIBRARY_ROOT, root.library_id.clone())));
            }
        }

        let recycle_enabled = self
            .read_setting_string_value_for_scope(
                crate::SETTINGS_SCOPE_MEDIA,
                crate::RECYCLE_BIN_ENABLED_KEY,
                None,
            )
            .await?
            .is_none_or(|value| value != "false");
        if recycle_enabled {
            if let Some(path) = self
                .read_setting_string_value_for_scope(
                    crate::SETTINGS_SCOPE_MEDIA,
                    crate::RECYCLE_BIN_PATH_KEY,
                    None,
                )
                .await?
                .filter(|path| !path.is_empty())
            {
                if processed_folder_path_conflicts(normalized, canonical, Path::new(&path))? {
                    return Ok(Some((OVERLAPS_RECYCLE_BIN, path)));
                }
            }
        }

        let backup = self.effective_backup_dir().await?;
        if processed_folder_path_conflicts(normalized, canonical, &backup)? {
            return Ok(Some((OVERLAPS_BACKUP_PATH, backup.display().to_string())));
        }

        let folders = self
            .services
            .config
            .processed_folders
            .list_folders()
            .await?;
        for folder in folders {
            if ignore_folder_id == Some(folder.id.as_str()) {
                continue;
            }
            let normalized_root = Path::new(&folder.normalized_root_path);
            let root = Path::new(&folder.root_path);
            if processed_folder_path_conflicts(normalized, canonical, normalized_root)?
                || processed_folder_path_conflicts(normalized, canonical, root)?
            {
                return Ok(Some((OVERLAPS_OTHER_FOLDER, folder.id)));
            }
        }
        Ok(None)
    }
}

fn overlap_message(code: &str) -> &'static str {
    match code {
        OVERLAPS_LIBRARY_ROOT => "processed folder path overlaps a library root",
        OVERLAPS_RECYCLE_BIN => "processed folder path overlaps the recycle bin",
        OVERLAPS_BACKUP_PATH => "processed folder path overlaps the backup directory",
        _ => "processed folder path overlaps another processed folder",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processed_folder_refusal::{
        FIELD_NOT_TRIMMED, INVALID_MARKER, INVALID_STABILITY, MARKER_VALUE_NOT_ALLOWED,
        NAME_REQUIRED, NAME_TOO_LONG, PATH_MISSING, PATH_NOT_ABSOLUTE, PATH_NOT_DIRECTORY,
    };

    fn reason(error: AppError) -> &'static str {
        error.validation_reason().expect("named refusal")
    }

    #[test]
    fn processed_folder_trim_precedes_an_empty_name() {
        let error = refuse_untrimmed("  ", "/tmp", None).unwrap_err();
        assert_eq!(reason(error), FIELD_NOT_TRIMMED);
        assert!(refuse_untrimmed("name", "/tmp", Some(" .done")).is_err());
        assert!(refuse_untrimmed("name", "/tmp", None).is_ok());
    }

    #[test]
    fn processed_folder_name_length_counts_scalar_values() {
        assert_eq!(
            reason(validate_processed_folder_name("").unwrap_err()),
            NAME_REQUIRED
        );
        assert!(validate_processed_folder_name(&"n".repeat(100)).is_ok());
        assert_eq!(
            reason(validate_processed_folder_name(&"n".repeat(101)).unwrap_err()),
            NAME_TOO_LONG
        );
        assert!(validate_processed_folder_name(&"é".repeat(100)).is_ok());
    }

    #[test]
    fn processed_folder_path_rules_reject_relative_missing_and_files() {
        assert_eq!(
            reason(resolve_processed_folder_root("relative").unwrap_err()),
            PATH_NOT_ABSOLUTE
        );
        assert_eq!(
            reason(resolve_processed_folder_root("/no/such/processed/folder/root").unwrap_err()),
            PATH_MISSING
        );
        let file =
            std::env::temp_dir().join(format!("processed-folder-file-{}", uuid::Uuid::new_v4()));
        std::fs::write(&file, b"x").unwrap();
        let error = resolve_processed_folder_root(&file.to_string_lossy()).unwrap_err();
        assert_eq!(reason(error), PATH_NOT_DIRECTORY);
        std::fs::remove_file(&file).unwrap();
    }

    #[test]
    fn processed_folder_path_overlap_is_component_wise_and_follows_links() {
        let root =
            std::env::temp_dir().join(format!("processed-folder-overlap-{}", uuid::Uuid::new_v4()));
        let real = root.join("real");
        let nested = real.join("nested");
        let sibling = root.join("real-sibling");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        let link = root.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let (_path, normalized, canonical) =
            resolve_processed_folder_root(&link.to_string_lossy()).unwrap();
        assert!(processed_folder_path_conflicts(&normalized, &canonical, &real).unwrap());
        assert!(processed_folder_path_conflicts(&normalized, &canonical, &nested).unwrap());
        assert!(!processed_folder_path_conflicts(&normalized, &canonical, &sibling).unwrap());
        assert!(!processed_folder_paths_overlap(
            Path::new("/processed/foo"),
            Path::new("/processed/foobar")
        ));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn processed_folder_stability_and_marker_rules() {
        assert!(validate_processed_folder_stability(2, 60).is_ok());
        assert!(validate_processed_folder_stability(10, 86_400).is_ok());
        assert_eq!(
            reason(validate_processed_folder_stability(1, 300).unwrap_err()),
            INVALID_STABILITY
        );
        assert_eq!(
            reason(validate_processed_folder_stability(2, 59).unwrap_err()),
            INVALID_STABILITY
        );
        assert!(validate_processed_folder_marker(ProcessedFolderMarkerMode::None, None).is_ok());
        assert_eq!(
            reason(
                validate_processed_folder_marker(ProcessedFolderMarkerMode::None, Some(".done"))
                    .unwrap_err()
            ),
            MARKER_VALUE_NOT_ALLOWED
        );
        assert!(
            validate_processed_folder_marker(
                ProcessedFolderMarkerMode::SiblingSuffix,
                Some(".done")
            )
            .is_ok()
        );
        assert_eq!(
            reason(
                validate_processed_folder_marker(
                    ProcessedFolderMarkerMode::SiblingSuffix,
                    Some("done")
                )
                .unwrap_err()
            ),
            INVALID_MARKER
        );
        assert_eq!(
            reason(
                validate_processed_folder_marker(
                    ProcessedFolderMarkerMode::SiblingSuffix,
                    Some(".a/b"),
                )
                .unwrap_err()
            ),
            INVALID_MARKER
        );
        assert!(
            validate_processed_folder_marker(
                ProcessedFolderMarkerMode::DirectoryFile,
                Some("ready"),
            )
            .is_ok()
        );
        assert_eq!(
            reason(
                validate_processed_folder_marker(
                    ProcessedFolderMarkerMode::DirectoryFile,
                    Some("..")
                )
                .unwrap_err()
            ),
            INVALID_MARKER
        );
    }
}
