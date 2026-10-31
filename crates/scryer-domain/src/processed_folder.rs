//! Processed-folder import ledger types.
//!
//! A processed folder is a directory of finished media that Scryer copies into
//! one library. Scryer only reads that directory: nothing inside it is moved,
//! linked, renamed, or deleted.

use chrono::{DateTime, Utc};

use crate::CompletedDownload;

/// `CompletedDownload.client_type` for a file taken from a processed folder.
///
/// Not a download client. `client_type_is_torrent` is false for this value.
pub const PROCESSED_FOLDER_CLIENT_TYPE: &str = "processed_folder";

impl CompletedDownload {
    /// Whether this download was synthesized from a processed-folder file.
    pub fn is_processed_folder_source(&self) -> bool {
        self.client_type == PROCESSED_FOLDER_CLIENT_TYPE
    }
}

/// Where a processed folder's files are imported. PR1 stores `library` only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProcessedFolderScopeKind {
    Library,
}

impl ProcessedFolderScopeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Library => "library",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "library" => Some(Self::Library),
            _ => None,
        }
    }
}

/// How a folder decides that a file is finished.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProcessedFolderMarkerMode {
    None,
    SiblingSuffix,
    DirectoryFile,
}

impl ProcessedFolderMarkerMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::SiblingSuffix => "sibling_suffix",
            Self::DirectoryFile => "directory_file",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "sibling_suffix" => Some(Self::SiblingSuffix),
            "directory_file" => Some(Self::DirectoryFile),
            _ => None,
        }
    }
}

/// Operator action on one ledger row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProcessedFolderEntryAction {
    ImportAs,
    Retry,
    Ignore,
}

impl ProcessedFolderEntryAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ImportAs => "import_as",
            Self::Retry => "retry",
            Self::Ignore => "ignore",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "import_as" => Some(Self::ImportAs),
            "retry" => Some(Self::Retry),
            "ignore" => Some(Self::Ignore),
            _ => None,
        }
    }
}

/// Lifecycle of one observed file version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProcessedFolderEntryStatus {
    Observing,
    Stable,
    Importing,
    Imported,
    Skipped,
    Failed,
    NeedsReview,
    Ignored,
    Rejected,
    Superseded,
    Gone,
}

impl ProcessedFolderEntryStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Observing => "observing",
            Self::Stable => "stable",
            Self::Importing => "importing",
            Self::Imported => "imported",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
            Self::NeedsReview => "needs_review",
            Self::Ignored => "ignored",
            Self::Rejected => "rejected",
            Self::Superseded => "superseded",
            Self::Gone => "gone",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "observing" => Some(Self::Observing),
            "stable" => Some(Self::Stable),
            "importing" => Some(Self::Importing),
            "imported" => Some(Self::Imported),
            "skipped" => Some(Self::Skipped),
            "failed" => Some(Self::Failed),
            "needs_review" => Some(Self::NeedsReview),
            "ignored" => Some(Self::Ignored),
            "rejected" => Some(Self::Rejected),
            "superseded" => Some(Self::Superseded),
            "gone" => Some(Self::Gone),
            _ => None,
        }
    }

    /// Every status, in the order the counts object reports them.
    pub const ALL: [Self; 11] = [
        Self::Observing,
        Self::Stable,
        Self::Importing,
        Self::Imported,
        Self::Skipped,
        Self::Failed,
        Self::NeedsReview,
        Self::Ignored,
        Self::Rejected,
        Self::Superseded,
        Self::Gone,
    ];
}

/// Operator actions the matrix allows for `status`. Anything else is refused.
pub fn allowed_entry_actions(
    status: ProcessedFolderEntryStatus,
) -> &'static [ProcessedFolderEntryAction] {
    use ProcessedFolderEntryAction::{Ignore, ImportAs, Retry};
    match status {
        ProcessedFolderEntryStatus::Observing | ProcessedFolderEntryStatus::Stable => &[Ignore],
        ProcessedFolderEntryStatus::Skipped | ProcessedFolderEntryStatus::NeedsReview => {
            &[ImportAs, Ignore]
        }
        ProcessedFolderEntryStatus::Failed => &[ImportAs, Retry, Ignore],
        ProcessedFolderEntryStatus::Ignored => &[Retry],
        ProcessedFolderEntryStatus::Importing
        | ProcessedFolderEntryStatus::Imported
        | ProcessedFolderEntryStatus::Rejected
        | ProcessedFolderEntryStatus::Superseded
        | ProcessedFolderEntryStatus::Gone => &[],
    }
}

pub fn entry_action_allowed(
    status: ProcessedFolderEntryStatus,
    action: ProcessedFolderEntryAction,
) -> bool {
    allowed_entry_actions(status).contains(&action)
}

/// Unix mtime in nanoseconds as UTC, or `None` when the instant is out of range.
pub fn mtime_ns_to_utc(mtime_ns: i64) -> Option<DateTime<Utc>> {
    let seconds = mtime_ns.div_euclid(1_000_000_000);
    let nanos = u32::try_from(mtime_ns.rem_euclid(1_000_000_000)).ok()?;
    DateTime::from_timestamp(seconds, nanos)
}

/// Earliest time a further scan can mark an observing entry stable.
///
/// The scan count is a separate gate. This is only the age deadline: the later
/// of `first_seen_at + min_age` and `mtime + min_age`. Other statuses have no
/// deadline.
pub fn entry_eligible_at(
    status: ProcessedFolderEntryStatus,
    first_seen_at: DateTime<Utc>,
    mtime_ns: i64,
    min_age_seconds: i64,
) -> Option<DateTime<Utc>> {
    if status != ProcessedFolderEntryStatus::Observing {
        return None;
    }
    let age = chrono::Duration::seconds(min_age_seconds);
    let from_seen = first_seen_at + age;
    let from_mtime = mtime_ns_to_utc(mtime_ns).map(|mtime| mtime + age);
    Some(match from_mtime {
        Some(instant) => from_seen.max(instant),
        None => from_seen,
    })
}

/// One configured processed folder, as stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessedFolder {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub normalized_root_path: String,
    pub scope_kind: ProcessedFolderScopeKind,
    pub library_id: String,
    pub enabled: bool,
    pub min_stable_scans: i64,
    pub min_age_seconds: i64,
    pub marker_mode: ProcessedFolderMarkerMode,
    pub marker_value: Option<String>,
    pub last_scan_started_at: Option<DateTime<Utc>>,
    pub last_scan_finished_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One observed file version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessedFolderEntry {
    pub id: String,
    pub folder_id: String,
    pub relative_path: String,
    pub size_bytes: u64,
    pub mtime_ns: i64,
    pub fingerprint: String,
    pub status: ProcessedFolderEntryStatus,
    pub reason_code: Option<String>,
    pub stable_scan_count: i64,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub marker_seen: bool,
    pub title_id: Option<String>,
    pub import_id: Option<String>,
    pub attempts: i64,
    pub claimed_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One count per entry status, in [`ProcessedFolderEntryStatus::ALL`] order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProcessedFolderEntryCounts {
    pub observing: i64,
    pub stable: i64,
    pub importing: i64,
    pub imported: i64,
    pub skipped: i64,
    pub failed: i64,
    pub needs_review: i64,
    pub ignored: i64,
    pub rejected: i64,
    pub superseded: i64,
    pub gone: i64,
}

impl ProcessedFolderEntryCounts {
    pub fn add(&mut self, status: ProcessedFolderEntryStatus, count: i64) {
        let slot = match status {
            ProcessedFolderEntryStatus::Observing => &mut self.observing,
            ProcessedFolderEntryStatus::Stable => &mut self.stable,
            ProcessedFolderEntryStatus::Importing => &mut self.importing,
            ProcessedFolderEntryStatus::Imported => &mut self.imported,
            ProcessedFolderEntryStatus::Skipped => &mut self.skipped,
            ProcessedFolderEntryStatus::Failed => &mut self.failed,
            ProcessedFolderEntryStatus::NeedsReview => &mut self.needs_review,
            ProcessedFolderEntryStatus::Ignored => &mut self.ignored,
            ProcessedFolderEntryStatus::Rejected => &mut self.rejected,
            ProcessedFolderEntryStatus::Superseded => &mut self.superseded,
            ProcessedFolderEntryStatus::Gone => &mut self.gone,
        };
        *slot = slot.saturating_add(count);
    }

    pub fn importing_count(self) -> i64 {
        self.importing
    }
}

/// Page of ledger rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessedFolderEntryQuery {
    pub folder_id: String,
    pub statuses: Option<Vec<ProcessedFolderEntryStatus>>,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessedFolderEntryPage {
    pub items: Vec<ProcessedFolderEntry>,
    pub total_count: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn download(client_type: &str) -> CompletedDownload {
        CompletedDownload {
            client_type: client_type.to_string(),
            client_id: "folder".to_string(),
            download_client_item_id: "item".to_string(),
            download_id: None,
            name: "Show".to_string(),
            release_name: None,
            dest_dir: "/library/Show.mkv".to_string(),
            category: None,
            size_bytes: Some(1),
            completed_at: None,
            parameters: Vec::new(),
            posted_file_names: Vec::new(),
            subtitle_paths: Vec::new(),
        }
    }

    #[test]
    fn processed_folder_client_type_identifies_the_source() {
        assert!(download(PROCESSED_FOLDER_CLIENT_TYPE).is_processed_folder_source());
        assert!(!download("qbittorrent").is_processed_folder_source());
        assert!(!download("processed_folder ").is_processed_folder_source());
    }

    #[test]
    fn processed_folder_enums_round_trip_their_stored_spellings() {
        for kind in [ProcessedFolderScopeKind::Library] {
            assert_eq!(ProcessedFolderScopeKind::parse(kind.as_str()), Some(kind));
        }
        for mode in [
            ProcessedFolderMarkerMode::None,
            ProcessedFolderMarkerMode::SiblingSuffix,
            ProcessedFolderMarkerMode::DirectoryFile,
        ] {
            assert_eq!(ProcessedFolderMarkerMode::parse(mode.as_str()), Some(mode));
        }
        for action in [
            ProcessedFolderEntryAction::ImportAs,
            ProcessedFolderEntryAction::Retry,
            ProcessedFolderEntryAction::Ignore,
        ] {
            assert_eq!(
                ProcessedFolderEntryAction::parse(action.as_str()),
                Some(action)
            );
        }
        assert_eq!(ProcessedFolderEntryStatus::ALL.len(), 11);
        for status in ProcessedFolderEntryStatus::ALL {
            assert_eq!(
                ProcessedFolderEntryStatus::parse(status.as_str()),
                Some(status)
            );
        }
        assert_eq!(
            ProcessedFolderScopeKind::parse("download_client_category"),
            None
        );
        assert_eq!(ProcessedFolderEntryStatus::parse("pending"), None);
    }

    #[test]
    fn allowed_actions_match_the_operator_matrix() {
        use ProcessedFolderEntryAction::{Ignore, ImportAs, Retry};
        use ProcessedFolderEntryStatus::*;

        let cases = [
            (Observing, &[Ignore][..]),
            (Stable, &[Ignore][..]),
            (Importing, &[][..]),
            (Imported, &[][..]),
            (Skipped, &[ImportAs, Ignore][..]),
            (Failed, &[ImportAs, Retry, Ignore][..]),
            (NeedsReview, &[ImportAs, Ignore][..]),
            (Ignored, &[Retry][..]),
            (Rejected, &[][..]),
            (Superseded, &[][..]),
            (Gone, &[][..]),
        ];
        assert_eq!(cases.len(), ProcessedFolderEntryStatus::ALL.len());
        for (status, expected) in cases {
            assert_eq!(allowed_entry_actions(status), expected, "{status:?}");
            for action in [
                ProcessedFolderEntryAction::ImportAs,
                ProcessedFolderEntryAction::Retry,
                ProcessedFolderEntryAction::Ignore,
            ] {
                assert_eq!(
                    entry_action_allowed(status, action),
                    expected.contains(&action),
                    "{status:?} {action:?}"
                );
            }
        }
    }

    #[test]
    fn eligible_at_is_the_later_age_deadline_while_observing() {
        let first = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let mtime_ns = 1_700_000_100_i64 * 1_000_000_000;
        let eligible =
            entry_eligible_at(ProcessedFolderEntryStatus::Observing, first, mtime_ns, 300).unwrap();
        assert_eq!(
            eligible,
            DateTime::from_timestamp(1_700_000_100 + 300, 0).unwrap()
        );
        assert!(
            entry_eligible_at(ProcessedFolderEntryStatus::Stable, first, mtime_ns, 300).is_none()
        );
    }

    #[test]
    fn entry_counts_add_by_status() {
        let mut counts = ProcessedFolderEntryCounts::default();
        counts.add(ProcessedFolderEntryStatus::Gone, 2);
        counts.add(ProcessedFolderEntryStatus::Superseded, 1);
        counts.add(ProcessedFolderEntryStatus::Importing, 4);
        assert_eq!(counts.gone, 2);
        assert_eq!(counts.superseded, 1);
        assert_eq!(counts.importing_count(), 4);
        assert_eq!(counts.observing, 0);
    }
}
