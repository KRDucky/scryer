//! List, get, create, update, delete, and enable a processed folder.

use chrono::{DateTime, Utc};
use scryer_domain::processed_folder::{
    ProcessedFolder, ProcessedFolderEntry, ProcessedFolderEntryAction, ProcessedFolderEntryCounts,
    ProcessedFolderEntryQuery, ProcessedFolderMarkerMode, ProcessedFolderScopeKind,
    allowed_entry_actions, entry_eligible_at,
};
use scryer_domain::{AppPermission, User};

use super::validation::{
    DEFAULT_MIN_AGE_SECONDS, DEFAULT_MIN_STABLE_SCANS, MarkerValueUpdate, ProcessedFolderWrite,
};
use crate::processed_folder_refusal::{IMPORTS_IN_FLIGHT, NOT_FOUND};
use crate::{AppError, AppResult, AppUseCase};

/// Fields of `createProcessedFolder`. The folder is always stored disabled.
#[derive(Clone, Debug)]
pub struct CreateProcessedFolder {
    pub name: String,
    pub root_path: String,
    pub library_id: String,
    pub min_stable_scans: Option<i64>,
    pub min_age_seconds: Option<i64>,
    pub marker_mode: Option<ProcessedFolderMarkerMode>,
    pub marker_value: Option<String>,
}

/// Fields of `updateProcessedFolder`. Omitted options keep the stored value.
#[derive(Clone, Debug)]
pub struct UpdateProcessedFolder {
    pub id: String,
    pub name: Option<String>,
    pub root_path: Option<String>,
    pub library_id: Option<String>,
    pub min_stable_scans: Option<i64>,
    pub min_age_seconds: Option<i64>,
    pub marker_mode: Option<ProcessedFolderMarkerMode>,
    pub marker_value: MarkerValueUpdate,
}

/// A folder plus the counts and library name the settings list shows.
#[derive(Clone, Debug)]
pub struct ProcessedFolderView {
    pub folder: ProcessedFolder,
    pub counts: ProcessedFolderEntryCounts,
    pub library_name: Option<String>,
}

/// One ledger row plus the operator actions and age deadline derived from it.
#[derive(Clone, Debug)]
pub struct ProcessedFolderEntryView {
    pub entry: ProcessedFolderEntry,
    pub allowed_actions: Vec<ProcessedFolderEntryAction>,
    pub eligible_at: Option<DateTime<Utc>>,
}

/// One page of ledger rows.
#[derive(Clone, Debug)]
pub struct ProcessedFolderEntryConnection {
    pub items: Vec<ProcessedFolderEntryView>,
    pub total_count: i64,
}

fn refused(reason: &'static str, message: impl Into<String>) -> AppError {
    AppError::validation_refused(reason, message)
}

impl AppUseCase {
    pub async fn list_processed_folders(
        &self,
        actor: &User,
    ) -> AppResult<Vec<ProcessedFolderView>> {
        self.require_app_permission(actor, AppPermission::ManageSystemSettings)
            .await?;
        let folders = self
            .services
            .config
            .processed_folders
            .list_folders()
            .await?;
        let mut views = Vec::with_capacity(folders.len());
        for folder in folders {
            views.push(self.processed_folder_view(folder).await?);
        }
        Ok(views)
    }

    pub async fn get_processed_folder(
        &self,
        actor: &User,
        id: &str,
    ) -> AppResult<Option<ProcessedFolderView>> {
        self.require_app_permission(actor, AppPermission::ManageSystemSettings)
            .await?;
        let Some(folder) = self
            .services
            .config
            .processed_folders
            .get_folder(id)
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(self.processed_folder_view(folder).await?))
    }

    pub async fn create_processed_folder(
        &self,
        actor: &User,
        input: CreateProcessedFolder,
    ) -> AppResult<ProcessedFolderView> {
        self.require_app_permission(actor, AppPermission::ManageSystemSettings)
            .await?;
        let write = ProcessedFolderWrite {
            name: input.name,
            root_path: input.root_path,
            library_id: input.library_id,
            scope_kind: ProcessedFolderScopeKind::Library,
            min_stable_scans: input.min_stable_scans.unwrap_or(DEFAULT_MIN_STABLE_SCANS),
            min_age_seconds: input.min_age_seconds.unwrap_or(DEFAULT_MIN_AGE_SECONDS),
            marker_mode: input.marker_mode.unwrap_or(ProcessedFolderMarkerMode::None),
            marker_value: input.marker_value,
        };
        let checked = self.validate_processed_folder_write(&write, None).await?;
        let now = self.runtime.imports.processed_folders.now();
        let folder = ProcessedFolder {
            id: uuid::Uuid::new_v4().to_string(),
            name: checked.name,
            root_path: checked.root_path,
            normalized_root_path: checked.normalized_root_path,
            scope_kind: checked.scope_kind,
            library_id: checked.library_id.clone(),
            enabled: false,
            min_stable_scans: checked.min_stable_scans,
            min_age_seconds: checked.min_age_seconds,
            marker_mode: checked.marker_mode,
            marker_value: checked.marker_value,
            last_scan_started_at: None,
            last_scan_finished_at: None,
            created_at: now,
            updated_at: now,
        };
        let folder = self
            .services
            .config
            .processed_folders
            .insert_folder(folder)
            .await?;
        Ok(ProcessedFolderView {
            folder,
            counts: ProcessedFolderEntryCounts::default(),
            library_name: Some(checked.library_name),
        })
    }

    pub async fn update_processed_folder(
        &self,
        actor: &User,
        input: UpdateProcessedFolder,
    ) -> AppResult<ProcessedFolderView> {
        self.require_app_permission(actor, AppPermission::ManageSystemSettings)
            .await?;
        let Some(existing) = self
            .services
            .config
            .processed_folders
            .get_folder(&input.id)
            .await?
        else {
            return Err(refused(NOT_FOUND, "processed folder was not found"));
        };
        let marker_value = match &input.marker_value {
            MarkerValueUpdate::Keep => existing.marker_value.clone(),
            MarkerValueUpdate::Clear => None,
            MarkerValueUpdate::Set(value) => Some(value.clone()),
        };
        let write = ProcessedFolderWrite {
            name: input.name.unwrap_or(existing.name),
            root_path: input.root_path.unwrap_or(existing.root_path),
            library_id: input.library_id.unwrap_or(existing.library_id),
            scope_kind: existing.scope_kind,
            min_stable_scans: input.min_stable_scans.unwrap_or(existing.min_stable_scans),
            min_age_seconds: input.min_age_seconds.unwrap_or(existing.min_age_seconds),
            marker_mode: input.marker_mode.unwrap_or(existing.marker_mode),
            marker_value,
        };
        let checked = self
            .validate_processed_folder_write(&write, Some(existing.id.as_str()))
            .await?;
        let now = self.runtime.imports.processed_folders.now();
        let folder = ProcessedFolder {
            id: existing.id,
            name: checked.name,
            root_path: checked.root_path,
            normalized_root_path: checked.normalized_root_path,
            scope_kind: checked.scope_kind,
            library_id: checked.library_id.clone(),
            enabled: existing.enabled,
            min_stable_scans: checked.min_stable_scans,
            min_age_seconds: checked.min_age_seconds,
            marker_mode: checked.marker_mode,
            marker_value: checked.marker_value,
            last_scan_started_at: existing.last_scan_started_at,
            last_scan_finished_at: existing.last_scan_finished_at,
            created_at: existing.created_at,
            updated_at: now,
        };
        let id = folder.id.clone();
        if !self
            .services
            .config
            .processed_folders
            .update_folder(folder)
            .await?
        {
            return Err(refused(NOT_FOUND, "processed folder was not found"));
        }
        self.processed_folder_by_id(&id).await
    }

    pub async fn set_processed_folder_enabled(
        &self,
        actor: &User,
        id: &str,
        enabled: bool,
    ) -> AppResult<ProcessedFolderView> {
        self.require_app_permission(actor, AppPermission::ManageSystemSettings)
            .await?;
        let now = self.runtime.imports.processed_folders.now();
        if !self
            .services
            .config
            .processed_folders
            .set_folder_enabled(id, enabled, now)
            .await?
        {
            return Err(refused(NOT_FOUND, "processed folder was not found"));
        }
        self.processed_folder_by_id(id).await
    }

    /// Recover orphans under the folder scan lock, refuse while a row is still
    /// `importing`, then delete the ledger. Nothing on disk is touched.
    ///
    /// Recovery itself is still the core stub, so this returns that stub's
    /// error until the scan owner replaces it.
    pub async fn delete_processed_folder(&self, actor: &User, id: &str) -> AppResult<String> {
        self.require_app_permission(actor, AppPermission::ManageSystemSettings)
            .await?;
        let Some(folder) = self
            .services
            .config
            .processed_folders
            .get_folder(id)
            .await?
        else {
            return Err(refused(NOT_FOUND, "processed folder was not found"));
        };
        let _guard = self
            .runtime
            .imports
            .processed_folders
            .lock_folder(&folder.id)
            .await;
        self.recover_orphaned_entries_locked(actor, &folder.id)
            .await?;
        let importing = self
            .services
            .config
            .processed_folders
            .count_importing_entries(&folder.id)
            .await?;
        if importing > 0 {
            return Err(refused(
                IMPORTS_IN_FLIGHT,
                "this folder still has an import in progress",
            ));
        }
        self.services
            .config
            .processed_folders
            .delete_folder(&folder.id)
            .await?;
        Ok(folder.id)
    }

    pub async fn list_processed_folder_entries(
        &self,
        actor: &User,
        folder_id: &str,
        statuses: Option<Vec<scryer_domain::processed_folder::ProcessedFolderEntryStatus>>,
        limit: i64,
        offset: i64,
    ) -> AppResult<ProcessedFolderEntryConnection> {
        self.require_app_permission(actor, AppPermission::ManageSystemSettings)
            .await?;
        let Some(folder) = self
            .services
            .config
            .processed_folders
            .get_folder(folder_id)
            .await?
        else {
            return Err(refused(NOT_FOUND, "processed folder was not found"));
        };
        let page = self
            .services
            .config
            .processed_folders
            .list_entries(ProcessedFolderEntryQuery {
                folder_id: folder.id.clone(),
                statuses,
                limit: limit.clamp(1, 500),
                offset: offset.max(0),
            })
            .await?;
        let items = page
            .items
            .into_iter()
            .map(|entry| entry_view(entry, folder.min_age_seconds))
            .collect();
        Ok(ProcessedFolderEntryConnection {
            items,
            total_count: page.total_count,
        })
    }

    async fn processed_folder_by_id(&self, id: &str) -> AppResult<ProcessedFolderView> {
        let folder = self
            .services
            .config
            .processed_folders
            .get_folder(id)
            .await?
            .ok_or_else(|| refused(NOT_FOUND, "processed folder was not found"))?;
        self.processed_folder_view(folder).await
    }

    async fn processed_folder_view(
        &self,
        folder: ProcessedFolder,
    ) -> AppResult<ProcessedFolderView> {
        let counts = self
            .services
            .config
            .processed_folders
            .entry_counts(&folder.id)
            .await?;
        let library_name = self
            .services
            .catalog
            .libraries
            .get_by_id(&folder.library_id)
            .await?
            .map(|library| library.name);
        Ok(ProcessedFolderView {
            folder,
            counts,
            library_name,
        })
    }
}

pub(crate) fn entry_view(
    entry: ProcessedFolderEntry,
    min_age_seconds: i64,
) -> ProcessedFolderEntryView {
    let eligible_at = entry_eligible_at(
        entry.status,
        entry.first_seen_at,
        entry.mtime_ns,
        min_age_seconds,
    );
    let allowed_actions = allowed_entry_actions(entry.status).to_vec();
    ProcessedFolderEntryView {
        entry,
        allowed_actions,
        eligible_at,
    }
}
