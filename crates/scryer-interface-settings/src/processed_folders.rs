//! Processed-folder settings. Every field is limited to operators who can
//! manage system settings, and the application checks that permission again.

use async_graphql::{
    Context, Enum, ID, InputObject, MaybeUndefined, Object, Result as GqlResult, SimpleObject,
};
use chrono::{DateTime, Utc};
use scryer_application::{
    AppError, CreateProcessedFolder, MarkerValueUpdate, ProcessedFolderEntryConnection,
    ProcessedFolderEntryView, ProcessedFolderPreview, ProcessedFolderScanReport,
    ProcessedFolderView, UpdateProcessedFolder,
};
use scryer_domain::AppPermission;
use scryer_domain::processed_folder::{
    ProcessedFolderEntry, ProcessedFolderEntryAction, ProcessedFolderEntryCounts,
    ProcessedFolderEntryStatus, ProcessedFolderMarkerMode, mtime_ns_to_utc,
};
use scryer_interface_core::{require_config_app_permission, to_gql_error};
use scryer_interface_media::types::Long;

/// How a processed folder decides a file is finished.
#[derive(Enum, Copy, Clone, Eq, PartialEq)]
#[graphql(
    name = "ProcessedFolderMarkerMode",
    rename_items = "SCREAMING_SNAKE_CASE"
)]
enum ProcessedFolderMarkerModeValue {
    /// No marker file. A file becomes stable after the scan and age gates.
    None,
    /// A sibling file named `<video><suffix>` marks that video finished.
    SiblingSuffix,
    /// A file of this exact name marks every video directly in its directory.
    DirectoryFile,
}

impl From<ProcessedFolderMarkerMode> for ProcessedFolderMarkerModeValue {
    fn from(value: ProcessedFolderMarkerMode) -> Self {
        match value {
            ProcessedFolderMarkerMode::None => Self::None,
            ProcessedFolderMarkerMode::SiblingSuffix => Self::SiblingSuffix,
            ProcessedFolderMarkerMode::DirectoryFile => Self::DirectoryFile,
        }
    }
}

impl From<ProcessedFolderMarkerModeValue> for ProcessedFolderMarkerMode {
    fn from(value: ProcessedFolderMarkerModeValue) -> Self {
        match value {
            ProcessedFolderMarkerModeValue::None => Self::None,
            ProcessedFolderMarkerModeValue::SiblingSuffix => Self::SiblingSuffix,
            ProcessedFolderMarkerModeValue::DirectoryFile => Self::DirectoryFile,
        }
    }
}

/// Lifecycle of one observed file version.
#[derive(Enum, Copy, Clone, Eq, PartialEq)]
#[graphql(
    name = "ProcessedFolderEntryStatus",
    rename_items = "SCREAMING_SNAKE_CASE"
)]
enum ProcessedFolderEntryStatusValue {
    /// Seen, and not yet proven finished.
    Observing,
    /// Finished and waiting for an import claim.
    Stable,
    /// Claimed by an import task.
    Importing,
    /// Imported, or the library already had identical bytes.
    Imported,
    /// The pipeline declined this version.
    Skipped,
    /// Execution failed.
    Failed,
    /// No safe title was found, so an operator must choose one.
    NeedsReview,
    /// An operator chose not to import this version.
    Ignored,
    /// Never importable, such as an empty file or a non-video.
    Rejected,
    /// The file changed, so a newer row exists or will be created.
    Superseded,
    /// The file vanished before import.
    Gone,
}

impl From<ProcessedFolderEntryStatus> for ProcessedFolderEntryStatusValue {
    fn from(value: ProcessedFolderEntryStatus) -> Self {
        match value {
            ProcessedFolderEntryStatus::Observing => Self::Observing,
            ProcessedFolderEntryStatus::Stable => Self::Stable,
            ProcessedFolderEntryStatus::Importing => Self::Importing,
            ProcessedFolderEntryStatus::Imported => Self::Imported,
            ProcessedFolderEntryStatus::Skipped => Self::Skipped,
            ProcessedFolderEntryStatus::Failed => Self::Failed,
            ProcessedFolderEntryStatus::NeedsReview => Self::NeedsReview,
            ProcessedFolderEntryStatus::Ignored => Self::Ignored,
            ProcessedFolderEntryStatus::Rejected => Self::Rejected,
            ProcessedFolderEntryStatus::Superseded => Self::Superseded,
            ProcessedFolderEntryStatus::Gone => Self::Gone,
        }
    }
}

impl From<ProcessedFolderEntryStatusValue> for ProcessedFolderEntryStatus {
    fn from(value: ProcessedFolderEntryStatusValue) -> Self {
        match value {
            ProcessedFolderEntryStatusValue::Observing => Self::Observing,
            ProcessedFolderEntryStatusValue::Stable => Self::Stable,
            ProcessedFolderEntryStatusValue::Importing => Self::Importing,
            ProcessedFolderEntryStatusValue::Imported => Self::Imported,
            ProcessedFolderEntryStatusValue::Skipped => Self::Skipped,
            ProcessedFolderEntryStatusValue::Failed => Self::Failed,
            ProcessedFolderEntryStatusValue::NeedsReview => Self::NeedsReview,
            ProcessedFolderEntryStatusValue::Ignored => Self::Ignored,
            ProcessedFolderEntryStatusValue::Rejected => Self::Rejected,
            ProcessedFolderEntryStatusValue::Superseded => Self::Superseded,
            ProcessedFolderEntryStatusValue::Gone => Self::Gone,
        }
    }
}

/// Operator action a ledger row accepts in its current state.
#[derive(Enum, Copy, Clone, Eq, PartialEq)]
#[graphql(
    name = "ProcessedFolderEntryAction",
    rename_items = "SCREAMING_SNAKE_CASE"
)]
enum ProcessedFolderEntryActionValue {
    /// Import the file as a chosen title in the folder's library.
    ImportAs,
    /// Return a failed or ignored row to stable so a later scan can queue it.
    Retry,
    /// Leave the file in place and do not import this version.
    Ignore,
}

impl From<ProcessedFolderEntryAction> for ProcessedFolderEntryActionValue {
    fn from(value: ProcessedFolderEntryAction) -> Self {
        match value {
            ProcessedFolderEntryAction::ImportAs => Self::ImportAs,
            ProcessedFolderEntryAction::Retry => Self::Retry,
            ProcessedFolderEntryAction::Ignore => Self::Ignore,
        }
    }
}

/// How many ledger rows are in each state.
#[derive(SimpleObject, Clone)]
#[graphql(name = "ProcessedFolderEntryCounts")]
struct ProcessedFolderEntryCountsPayload {
    /// Rows still being observed.
    observing: i32,
    /// Rows waiting for an import claim.
    stable: i32,
    /// Rows an import task currently owns.
    importing: i32,
    /// Rows the pipeline imported.
    imported: i32,
    /// Rows the pipeline declined.
    skipped: i32,
    /// Rows whose import failed.
    failed: i32,
    /// Rows that need an operator to choose a title.
    needs_review: i32,
    /// Rows an operator chose to skip.
    ignored: i32,
    /// Rows that can never be imported.
    rejected: i32,
    /// Rows replaced by a newer size or mtime.
    superseded: i32,
    /// Rows whose file disappeared before import.
    gone: i32,
}

impl From<ProcessedFolderEntryCounts> for ProcessedFolderEntryCountsPayload {
    fn from(value: ProcessedFolderEntryCounts) -> Self {
        Self {
            observing: as_int(value.observing),
            stable: as_int(value.stable),
            importing: as_int(value.importing),
            imported: as_int(value.imported),
            skipped: as_int(value.skipped),
            failed: as_int(value.failed),
            needs_review: as_int(value.needs_review),
            ignored: as_int(value.ignored),
            rejected: as_int(value.rejected),
            superseded: as_int(value.superseded),
            gone: as_int(value.gone),
        }
    }
}

/// A folder of finished media Scryer copies into one library.
///
/// Scryer only reads the folder. Imports always copy. Nothing in the folder
/// is moved, linked, renamed, or deleted.
#[derive(SimpleObject, Clone)]
#[graphql(name = "ProcessedFolder")]
struct ProcessedFolderPayload {
    /// Stable folder identity.
    id: ID,
    /// Operator-visible name. Unique across folders.
    name: String,
    /// Absolute directory Scryer reads.
    root_path: String,
    /// Library the files are copied into.
    library_id: ID,
    /// Library name, when the library still exists.
    library_name: Option<String>,
    /// Whether scans and new claims are allowed. New folders start disabled.
    enabled: bool,
    /// Consecutive unchanged scans required before a file is stable. From 2 to 10.
    min_stable_scans: i32,
    /// Minimum age, in seconds, of the file and of its mtime. From 60 to 86400.
    min_age_seconds: i32,
    /// How a finished file is recognized.
    marker_mode: ProcessedFolderMarkerModeValue,
    /// Marker suffix or file name. Null when the mode is none.
    marker_value: Option<String>,
    /// When the latest scan started, if one has started.
    last_scan_started_at: Option<DateTime<Utc>>,
    /// When the latest scan finished, if one has finished.
    last_scan_finished_at: Option<DateTime<Utc>>,
    /// Ledger rows grouped by state.
    counts: ProcessedFolderEntryCountsPayload,
}

impl From<ProcessedFolderView> for ProcessedFolderPayload {
    fn from(value: ProcessedFolderView) -> Self {
        let folder = value.folder;
        Self {
            id: folder.id.into(),
            name: folder.name,
            root_path: folder.root_path,
            library_id: folder.library_id.into(),
            library_name: value.library_name,
            enabled: folder.enabled,
            min_stable_scans: as_int(folder.min_stable_scans),
            min_age_seconds: as_int(folder.min_age_seconds),
            marker_mode: folder.marker_mode.into(),
            marker_value: folder.marker_value,
            last_scan_started_at: folder.last_scan_started_at,
            last_scan_finished_at: folder.last_scan_finished_at,
            counts: value.counts.into(),
        }
    }
}

/// One observed file version and the actions an operator can take on it.
#[derive(SimpleObject, Clone)]
#[graphql(name = "ProcessedFolderEntry")]
struct ProcessedFolderEntryPayload {
    /// Stable entry identity.
    id: ID,
    /// Folder that owns this version.
    folder_id: ID,
    /// Path relative to the folder root, using `/` separators.
    relative_path: String,
    /// File size in bytes.
    size_bytes: Long,
    /// File modification time.
    modified_at: DateTime<Utc>,
    /// Current lifecycle state.
    status: ProcessedFolderEntryStatusValue,
    /// Why the row is in this state, when a reason was recorded.
    reason_code: Option<String>,
    /// Unchanged scans counted toward stability.
    stable_scan_count: i32,
    /// When this version was first seen.
    first_seen_at: DateTime<Utc>,
    /// When a scan last saw this version.
    last_seen_at: DateTime<Utc>,
    /// Whether a marker file was present on the last scan.
    marker_seen: bool,
    /// Title the file was imported as, when one was chosen or matched.
    title_id: Option<ID>,
    /// Import record created for this version, when one exists.
    import_id: Option<ID>,
    /// How many times an import claim has been taken for this version.
    attempts: i32,
    /// Most recent error sentence, when the last attempt failed.
    last_error: Option<String>,
    /// Earliest time a further scan can mark this entry stable, when still observing.
    eligible_at: Option<DateTime<Utc>>,
    /// Operator actions this entry accepts now.
    allowed_actions: Vec<ProcessedFolderEntryActionValue>,
}

/// A page of ledger rows.
#[derive(SimpleObject, Clone)]
#[graphql(name = "ProcessedFolderEntryConnection")]
struct ProcessedFolderEntryConnectionPayload {
    /// Rows in this page, ordered by relative path.
    items: Vec<ProcessedFolderEntryPayload>,
    /// Rows matching the filter, including those outside this page.
    total_count: i32,
}

/// Counts from one scan.
#[derive(SimpleObject, Clone)]
struct ProcessedFolderScanPayload {
    /// Folder after the scan.
    folder: ProcessedFolderPayload,
    /// File versions this scan recorded or refreshed.
    discovered: i32,
    /// Rows that became stable on this scan.
    became_stable: i32,
    /// Stable rows queued for import.
    imports_queued: i32,
    /// Rows left for an operator because no safe title was found.
    needs_review: i32,
    /// Rows rejected as never importable.
    rejected: i32,
    /// Rows replaced because the file's size or mtime changed.
    superseded: i32,
    /// Rows whose file was absent.
    gone: i32,
    /// Importing rows recovered because their task was no longer running.
    recovered: i32,
    /// Earliest time a further scan can make another observed file stable.
    next_eligible_at: Option<DateTime<Utc>>,
}

/// One file a preview would record, without writing the ledger.
#[derive(SimpleObject, Clone)]
#[graphql(name = "ProcessedFolderPreviewEntry")]
struct ProcessedFolderPreviewEntryPayload {
    /// Path relative to the folder root, using `/` separators.
    relative_path: String,
    /// File size in bytes.
    size_bytes: Long,
    /// File modification time.
    modified_at: DateTime<Utc>,
    /// State the next scan would give this file.
    would_be: ProcessedFolderEntryStatusValue,
    /// Why that state would be chosen, when there is a reason.
    reason_code: Option<String>,
    /// Title that would be used when exactly one title in the library matches.
    matched_title_id: Option<ID>,
}

/// Files a scan would consider, without changing the ledger.
#[derive(SimpleObject, Clone)]
#[graphql(name = "ProcessedFolderPreviewPayload")]
struct ProcessedFolderPreviewPayload {
    /// Preview rows, capped by the requested limit.
    entries: Vec<ProcessedFolderPreviewEntryPayload>,
    /// Whether more files exist than this preview returned.
    truncated: bool,
}

/// Folder returned by a configuration change.
#[derive(SimpleObject, Clone)]
#[graphql(name = "ProcessedFolderPayload")]
struct ProcessedFolderPayloadResult {
    /// Folder after the change.
    folder: ProcessedFolderPayload,
}

/// Identity of a deleted folder. The ledger rows are removed with it.
#[derive(SimpleObject, Clone)]
struct DeleteProcessedFolderPayload {
    /// Identity of the folder that was deleted.
    id: ID,
}

/// Entry returned by an operator action.
#[derive(SimpleObject, Clone)]
#[graphql(name = "ProcessedFolderEntryPayload")]
struct ProcessedFolderEntryPayloadResult {
    /// Entry after the action.
    entry: ProcessedFolderEntryPayload,
}

/// Fields for a new processed folder. The folder is created disabled.
#[derive(InputObject)]
struct CreateProcessedFolderInput {
    /// Operator-visible name, 1 to 100 characters, without surrounding whitespace.
    name: String,
    /// Absolute directory Scryer will read. It must exist and must not overlap a library, the recycle bin, the backup directory, or another processed folder.
    root_path: String,
    /// Library the files will be copied into.
    library_id: ID,
    /// Consecutive unchanged scans required before a file is stable. Defaults to 2. Allowed range is 2 to 10.
    min_stable_scans: Option<i32>,
    /// Minimum age in seconds. Defaults to 300. Allowed range is 60 to 86400.
    min_age_seconds: Option<i32>,
    /// How a finished file is recognized. Defaults to none.
    marker_mode: Option<ProcessedFolderMarkerModeValue>,
    /// Marker suffix or file name. Omit it when the mode is none.
    marker_value: Option<String>,
}

/// Fields that change on an existing processed folder. Omitted fields stay as stored.
#[derive(InputObject)]
struct UpdateProcessedFolderInput {
    /// Folder to change.
    id: ID,
    /// Replacement name.
    name: Option<String>,
    /// Replacement absolute directory.
    root_path: Option<String>,
    /// Replacement library.
    library_id: Option<ID>,
    /// Replacement scan count gate.
    min_stable_scans: Option<i32>,
    /// Replacement age gate, in seconds.
    min_age_seconds: Option<i32>,
    /// Replacement marker mode.
    marker_mode: Option<ProcessedFolderMarkerModeValue>,
    /// Replacement marker value. Null clears it. Omit the field to keep the stored value.
    marker_value: MaybeUndefined<String>,
}

fn as_int(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}

fn modified_at(entry: &ProcessedFolderEntry) -> GqlResult<DateTime<Utc>> {
    mtime_ns_to_utc(entry.mtime_ns).ok_or_else(|| {
        to_gql_error(AppError::Repository(format!(
            "processed folder entry {} has an mtime outside the UTC range",
            entry.id
        )))
    })
}

fn entry_payload(view: ProcessedFolderEntryView) -> GqlResult<ProcessedFolderEntryPayload> {
    let modified_at = modified_at(&view.entry)?;
    let entry = view.entry;
    Ok(ProcessedFolderEntryPayload {
        id: entry.id.into(),
        folder_id: entry.folder_id.into(),
        relative_path: entry.relative_path,
        size_bytes: Long::from_u64_saturating(entry.size_bytes),
        modified_at,
        status: entry.status.into(),
        reason_code: entry.reason_code,
        stable_scan_count: as_int(entry.stable_scan_count),
        first_seen_at: entry.first_seen_at,
        last_seen_at: entry.last_seen_at,
        marker_seen: entry.marker_seen,
        title_id: entry.title_id.map(ID::from),
        import_id: entry.import_id.map(ID::from),
        attempts: as_int(entry.attempts),
        last_error: entry.last_error,
        eligible_at: view.eligible_at,
        allowed_actions: view.allowed_actions.into_iter().map(Into::into).collect(),
    })
}

fn connection_payload(
    page: ProcessedFolderEntryConnection,
) -> GqlResult<ProcessedFolderEntryConnectionPayload> {
    let items = page
        .items
        .into_iter()
        .map(entry_payload)
        .collect::<GqlResult<Vec<_>>>()?;
    Ok(ProcessedFolderEntryConnectionPayload {
        items,
        total_count: as_int(page.total_count),
    })
}

fn preview_payload(preview: ProcessedFolderPreview) -> ProcessedFolderPreviewPayload {
    ProcessedFolderPreviewPayload {
        truncated: preview.truncated,
        entries: preview
            .entries
            .into_iter()
            .map(|entry| ProcessedFolderPreviewEntryPayload {
                relative_path: entry.relative_path,
                size_bytes: Long::from_u64_saturating(entry.size_bytes),
                modified_at: entry.modified_at,
                would_be: entry.would_be.into(),
                reason_code: entry.reason_code,
                matched_title_id: entry.matched_title_id.map(ID::from),
            })
            .collect(),
    }
}

fn scan_payload(
    report: ProcessedFolderScanReport,
    folder: ProcessedFolderView,
) -> ProcessedFolderScanPayload {
    ProcessedFolderScanPayload {
        folder: folder.into(),
        discovered: as_int(report.discovered),
        became_stable: as_int(report.became_stable),
        imports_queued: as_int(report.imports_queued),
        needs_review: as_int(report.needs_review),
        rejected: as_int(report.rejected),
        superseded: as_int(report.superseded),
        gone: as_int(report.gone),
        recovered: as_int(report.recovered),
        next_eligible_at: report.next_eligible_at,
    }
}

fn create_request(input: CreateProcessedFolderInput) -> CreateProcessedFolder {
    CreateProcessedFolder {
        name: input.name,
        root_path: input.root_path,
        library_id: input.library_id.to_string(),
        min_stable_scans: input.min_stable_scans.map(i64::from),
        min_age_seconds: input.min_age_seconds.map(i64::from),
        marker_mode: input.marker_mode.map(Into::into),
        marker_value: input.marker_value,
    }
}

fn update_request(input: UpdateProcessedFolderInput) -> UpdateProcessedFolder {
    UpdateProcessedFolder {
        id: input.id.to_string(),
        name: input.name,
        root_path: input.root_path,
        library_id: input.library_id.map(|id| id.to_string()),
        min_stable_scans: input.min_stable_scans.map(i64::from),
        min_age_seconds: input.min_age_seconds.map(i64::from),
        marker_mode: input.marker_mode.map(Into::into),
        marker_value: match input.marker_value {
            MaybeUndefined::Undefined => MarkerValueUpdate::Keep,
            MaybeUndefined::Null => MarkerValueUpdate::Clear,
            MaybeUndefined::Value(value) => MarkerValueUpdate::Set(value),
        },
    }
}

#[derive(Default)]
pub struct ProcessedFolderQueries;

#[Object]
impl ProcessedFolderQueries {
    /// List processed folders and how many ledger rows each state holds.
    async fn processed_folders(&self, ctx: &Context<'_>) -> GqlResult<Vec<ProcessedFolderPayload>> {
        let actor = require_config_app_permission(ctx, AppPermission::ManageSystemSettings).await?;
        let app = scryer_interface_core::app_from_ctx(ctx)?;
        Ok(app
            .list_processed_folders(&actor)
            .await
            .map_err(to_gql_error)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Load one processed folder, or null when the id does not exist.
    async fn processed_folder(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Processed folder identity.")] id: ID,
    ) -> GqlResult<Option<ProcessedFolderPayload>> {
        let actor = require_config_app_permission(ctx, AppPermission::ManageSystemSettings).await?;
        let app = scryer_interface_core::app_from_ctx(ctx)?;
        Ok(app
            .get_processed_folder(&actor, id.as_str())
            .await
            .map_err(to_gql_error)?
            .map(Into::into))
    }

    /// List ledger rows for one folder.
    async fn processed_folder_entries(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Folder whose rows to list.")] folder_id: ID,
        #[graphql(desc = "States to include. Omit to include every state.")] statuses: Option<
            Vec<ProcessedFolderEntryStatusValue>,
        >,
        #[graphql(default = 50, desc = "Maximum rows to return. Defaults to 50.")] limit: i32,
        #[graphql(default = 0, desc = "Rows to skip. Defaults to 0.")] offset: i32,
    ) -> GqlResult<ProcessedFolderEntryConnectionPayload> {
        let actor = require_config_app_permission(ctx, AppPermission::ManageSystemSettings).await?;
        let app = scryer_interface_core::app_from_ctx(ctx)?;
        let page = app
            .list_processed_folder_entries(
                &actor,
                folder_id.as_str(),
                statuses.map(|statuses| statuses.into_iter().map(Into::into).collect()),
                i64::from(limit),
                i64::from(offset),
            )
            .await
            .map_err(to_gql_error)?;
        connection_payload(page)
    }

    /// Describe what a scan would record, without writing the ledger. Works while the folder is disabled.
    async fn preview_processed_folder_scan(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Folder to preview.")] id: ID,
        #[graphql(default = 200, desc = "Maximum preview rows. Defaults to 200.")] limit: i32,
    ) -> GqlResult<ProcessedFolderPreviewPayload> {
        let actor = require_config_app_permission(ctx, AppPermission::ManageSystemSettings).await?;
        let app = scryer_interface_core::app_from_ctx(ctx)?;
        let preview = app
            .preview_processed_folder_scan(&actor, id.as_str(), i64::from(limit))
            .await
            .map_err(to_gql_error)?;
        Ok(preview_payload(preview))
    }
}

#[derive(Default)]
pub struct ProcessedFolderMutations;

#[Object]
impl ProcessedFolderMutations {
    /// Create a processed folder. It is always created disabled.
    async fn create_processed_folder(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Name, directory, library, and optional stability and marker settings.")]
        input: CreateProcessedFolderInput,
    ) -> GqlResult<ProcessedFolderPayloadResult> {
        let actor = require_config_app_permission(ctx, AppPermission::ManageSystemSettings).await?;
        let app = scryer_interface_core::app_from_ctx(ctx)?;
        let folder = app
            .create_processed_folder(&actor, create_request(input))
            .await
            .map_err(to_gql_error)?;
        Ok(ProcessedFolderPayloadResult {
            folder: folder.into(),
        })
    }

    /// Change a processed folder's name, directory, library, stability, or marker.
    async fn update_processed_folder(
        &self,
        ctx: &Context<'_>,
        #[graphql(
            desc = "Identity and the fields to replace. Omit a field to keep its stored value."
        )]
        input: UpdateProcessedFolderInput,
    ) -> GqlResult<ProcessedFolderPayloadResult> {
        let actor = require_config_app_permission(ctx, AppPermission::ManageSystemSettings).await?;
        let app = scryer_interface_core::app_from_ctx(ctx)?;
        let folder = app
            .update_processed_folder(&actor, update_request(input))
            .await
            .map_err(to_gql_error)?;
        Ok(ProcessedFolderPayloadResult {
            folder: folder.into(),
        })
    }

    /// Delete a processed folder and its ledger. Files on disk are left untouched. Refused while an entry is still importing.
    async fn delete_processed_folder(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Folder to delete.")] id: ID,
    ) -> GqlResult<DeleteProcessedFolderPayload> {
        let actor = require_config_app_permission(ctx, AppPermission::ManageSystemSettings).await?;
        let app = scryer_interface_core::app_from_ctx(ctx)?;
        let id = app
            .delete_processed_folder(&actor, id.as_str())
            .await
            .map_err(to_gql_error)?;
        Ok(DeleteProcessedFolderPayload { id: id.into() })
    }

    /// Allow or stop scans and new import claims. An in-flight import is left to finish.
    async fn set_processed_folder_enabled(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Folder to enable or disable.")] id: ID,
        #[graphql(desc = "True allows scans and new claims. False stops them.")] enabled: bool,
    ) -> GqlResult<ProcessedFolderPayloadResult> {
        let actor = require_config_app_permission(ctx, AppPermission::ManageSystemSettings).await?;
        let app = scryer_interface_core::app_from_ctx(ctx)?;
        let folder = app
            .set_processed_folder_enabled(&actor, id.as_str(), enabled)
            .await
            .map_err(to_gql_error)?;
        Ok(ProcessedFolderPayloadResult {
            folder: folder.into(),
        })
    }

    /// Recover orphaned claims, scan the folder, and queue imports for stable files.
    async fn scan_processed_folder(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Folder to scan.")] id: ID,
    ) -> GqlResult<ProcessedFolderScanPayload> {
        let actor = require_config_app_permission(ctx, AppPermission::ManageSystemSettings).await?;
        let app = scryer_interface_core::app_from_ctx(ctx)?;
        let report = app
            .scan_processed_folder(&actor, id.as_str())
            .await
            .map_err(to_gql_error)?;
        let folder = app
            .get_processed_folder(&actor, &report.folder_id)
            .await
            .map_err(to_gql_error)?
            .ok_or_else(|| {
                to_gql_error(AppError::validation_refused(
                    scryer_application::processed_folder_refusal::NOT_FOUND,
                    "processed folder was not found",
                ))
            })?;
        Ok(scan_payload(report, folder))
    }

    /// Import one needs-review, failed, or skipped file as a title in the folder's library.
    async fn import_processed_folder_entry(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Ledger row to import.")] entry_id: ID,
        #[graphql(desc = "Title in the folder's library that should receive the file.")]
        title_id: ID,
    ) -> GqlResult<ProcessedFolderEntryPayloadResult> {
        let actor = require_config_app_permission(ctx, AppPermission::ManageSystemSettings).await?;
        let app = scryer_interface_core::app_from_ctx(ctx)?;
        let entry = app
            .import_processed_folder_entry(&actor, entry_id.as_str(), title_id.as_str())
            .await
            .map_err(to_gql_error)?;
        Ok(ProcessedFolderEntryPayloadResult {
            entry: entry_payload(entry)?,
        })
    }

    /// Return a failed or ignored row to stable.
    async fn retry_processed_folder_entry(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Ledger row to retry.")] entry_id: ID,
    ) -> GqlResult<ProcessedFolderEntryPayloadResult> {
        let actor = require_config_app_permission(ctx, AppPermission::ManageSystemSettings).await?;
        let app = scryer_interface_core::app_from_ctx(ctx)?;
        let entry = app
            .retry_processed_folder_entry(&actor, entry_id.as_str())
            .await
            .map_err(to_gql_error)?;
        Ok(ProcessedFolderEntryPayloadResult {
            entry: entry_payload(entry)?,
        })
    }

    /// Mark a row ignored. Allowed while the folder is disabled.
    async fn ignore_processed_folder_entry(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Ledger row to ignore.")] entry_id: ID,
    ) -> GqlResult<ProcessedFolderEntryPayloadResult> {
        let actor = require_config_app_permission(ctx, AppPermission::ManageSystemSettings).await?;
        let app = scryer_interface_core::app_from_ctx(ctx)?;
        let entry = app
            .ignore_processed_folder_entry(&actor, entry_id.as_str())
            .await
            .map_err(to_gql_error)?;
        Ok(ProcessedFolderEntryPayloadResult {
            entry: entry_payload(entry)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processed_folder_refusal_reaches_graphql_extensions() {
        let error = to_gql_error(AppError::validation_refused(
            scryer_application::processed_folder_refusal::NAME_REQUIRED,
            "processed folder name is required",
        ));
        let extensions = error.extensions.as_ref().expect("extensions");
        let code = extensions.get("code").and_then(|value| match value {
            async_graphql::Value::String(value) => Some(value.as_str()),
            _ => None,
        });
        let reason = extensions.get("reason").and_then(|value| match value {
            async_graphql::Value::String(value) => Some(value.as_str()),
            _ => None,
        });
        assert_eq!(code, Some("VALIDATION_ERROR"));
        assert_eq!(
            reason,
            Some(scryer_application::processed_folder_refusal::NAME_REQUIRED)
        );
    }
}
