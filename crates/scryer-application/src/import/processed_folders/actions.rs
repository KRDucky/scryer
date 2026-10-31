//! Scan and entry actions. The bodies stay stubs until core owns this file.
//!
//! Each public method returns `AppError::Repository("not implemented")` after
//! the cheap existence and disabled-folder checks. QA greps for that sentence
//! before the final review.

use chrono::{DateTime, Utc};
use scryer_domain::processed_folder::{ProcessedFolder, ProcessedFolderEntryStatus};
use scryer_domain::{AppPermission, User};

use super::config::ProcessedFolderEntryView;
use crate::processed_folder_refusal::{DISABLED, ENTRY_NOT_FOUND, NOT_FOUND, SCAN_IN_PROGRESS};
use crate::{AppError, AppResult, AppUseCase};

/// Result of `scanProcessedFolder` once core fills the stub.
#[derive(Clone, Debug)]
pub struct ProcessedFolderScanReport {
    pub folder_id: String,
    pub discovered: i64,
    pub became_stable: i64,
    pub imports_queued: i64,
    pub needs_review: i64,
    pub rejected: i64,
    pub superseded: i64,
    pub gone: i64,
    pub recovered: i64,
    pub next_eligible_at: Option<DateTime<Utc>>,
}

/// One file a preview would record, without writing the ledger.
#[derive(Clone, Debug)]
pub struct ProcessedFolderPreviewEntry {
    pub relative_path: String,
    pub size_bytes: u64,
    pub modified_at: DateTime<Utc>,
    pub would_be: ProcessedFolderEntryStatus,
    pub reason_code: Option<String>,
    pub matched_title_id: Option<String>,
}

/// Result of `previewProcessedFolderScan`.
#[derive(Clone, Debug)]
pub struct ProcessedFolderPreview {
    pub entries: Vec<ProcessedFolderPreviewEntry>,
    pub truncated: bool,
}

fn refused(reason: &'static str, message: impl Into<String>) -> AppError {
    AppError::validation_refused(reason, message)
}

fn not_implemented() -> AppError {
    AppError::Repository("not implemented".into())
}

impl AppUseCase {
    pub async fn scan_processed_folder(
        &self,
        actor: &User,
        id: &str,
    ) -> AppResult<ProcessedFolderScanReport> {
        self.require_enabled_processed_folder(actor, id).await?;
        let _guard = self
            .runtime
            .imports
            .processed_folders
            .try_lock_folder(id)
            .await
            .map_err(|_| refused(SCAN_IN_PROGRESS, "a scan of this folder is already running"))?;
        let _ = _guard;
        Err(not_implemented())
    }

    pub async fn preview_processed_folder_scan(
        &self,
        actor: &User,
        id: &str,
        limit: i64,
    ) -> AppResult<ProcessedFolderPreview> {
        self.require_app_permission(actor, AppPermission::ManageSystemSettings)
            .await?;
        let _ = limit;
        if self
            .services
            .config
            .processed_folders
            .get_folder(id)
            .await?
            .is_none()
        {
            return Err(refused(NOT_FOUND, "processed folder was not found"));
        }
        Err(not_implemented())
    }

    pub async fn import_processed_folder_entry(
        &self,
        actor: &User,
        entry_id: &str,
        title_id: &str,
    ) -> AppResult<ProcessedFolderEntryView> {
        let entry = self.require_processed_folder_entry(actor, entry_id).await?;
        self.refuse_processed_folder_disabled(&entry.folder_id)
            .await?;
        let _ = title_id;
        Err(not_implemented())
    }

    pub async fn retry_processed_folder_entry(
        &self,
        actor: &User,
        entry_id: &str,
    ) -> AppResult<ProcessedFolderEntryView> {
        let entry = self.require_processed_folder_entry(actor, entry_id).await?;
        self.refuse_processed_folder_disabled(&entry.folder_id)
            .await?;
        Err(not_implemented())
    }

    pub async fn ignore_processed_folder_entry(
        &self,
        actor: &User,
        entry_id: &str,
    ) -> AppResult<ProcessedFolderEntryView> {
        let _entry = self.require_processed_folder_entry(actor, entry_id).await?;
        Err(not_implemented())
    }

    pub async fn recover_orphaned_entries(&self, actor: &User, folder_id: &str) -> AppResult<i64> {
        self.require_app_permission(actor, AppPermission::ManageSystemSettings)
            .await?;
        let _guard = self
            .runtime
            .imports
            .processed_folders
            .lock_folder(folder_id)
            .await;
        self.recover_orphaned_entries_locked(actor, folder_id).await
    }

    /// Recovery while the caller already holds the folder scan lock.
    pub(crate) async fn recover_orphaned_entries_locked(
        &self,
        actor: &User,
        folder_id: &str,
    ) -> AppResult<i64> {
        let _ = (actor, folder_id);
        Err(not_implemented())
    }

    async fn require_enabled_processed_folder(
        &self,
        actor: &User,
        id: &str,
    ) -> AppResult<ProcessedFolder> {
        self.require_app_permission(actor, AppPermission::ManageSystemSettings)
            .await?;
        self.refuse_processed_folder_disabled(id).await
    }

    async fn refuse_processed_folder_disabled(&self, id: &str) -> AppResult<ProcessedFolder> {
        let Some(folder) = self
            .services
            .config
            .processed_folders
            .get_folder(id)
            .await?
        else {
            return Err(refused(NOT_FOUND, "processed folder was not found"));
        };
        if !folder.enabled {
            return Err(refused(DISABLED, "processed folder is disabled"));
        }
        Ok(folder)
    }

    async fn require_processed_folder_entry(
        &self,
        actor: &User,
        entry_id: &str,
    ) -> AppResult<scryer_domain::processed_folder::ProcessedFolderEntry> {
        self.require_app_permission(actor, AppPermission::ManageSystemSettings)
            .await?;
        self.services
            .config
            .processed_folders
            .get_entry(entry_id)
            .await?
            .ok_or_else(|| refused(ENTRY_NOT_FOUND, "processed folder entry was not found"))
    }
}
