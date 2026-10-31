//! Persistence for processed folders and their ledger rows.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use scryer_application::{
    AppError, AppResult, ProcessedFolderRepository, RepositoryErrorKind,
    processed_folder_refusal::{self, NAME_TAKEN, OVERLAPS_OTHER_FOLDER, SCOPE_KIND_INVALID},
};
use scryer_domain::processed_folder::{
    ProcessedFolder, ProcessedFolderEntry, ProcessedFolderEntryCounts, ProcessedFolderEntryPage,
    ProcessedFolderEntryQuery, ProcessedFolderEntryStatus, ProcessedFolderMarkerMode,
    ProcessedFolderScopeKind, mtime_ns_to_utc,
};

use crate::queries::sql_runtime::{SqlArg, SqlRow, SqlRuntime, SqlTx, StoreDatastore};

const FOLDER_COLUMNS: &str = "id, name, root_path, normalized_root_path, scope_kind, library_id, \
     enabled, min_stable_scans, min_age_seconds, marker_mode, marker_value, last_scan_started_at, \
     last_scan_finished_at, created_at, updated_at";

const ENTRY_COLUMNS: &str = "id, folder_id, relative_path, size_bytes, mtime_ns, fingerprint, \
     status, reason_code, stable_scan_count, first_seen_at, last_seen_at, marker_seen, title_id, \
     import_id, attempts, claimed_at, last_error, created_at, updated_at";

const CLAIM_STABLE_ENTRY_SQL: &str = "UPDATE processed_folder_entries
    SET status = 'importing', claimed_at = {}, attempts = attempts + 1, updated_at = {}
  WHERE id = {}
    AND status = 'stable'
    AND EXISTS (
        SELECT 1 FROM processed_folders f
         WHERE f.id = processed_folder_entries.folder_id
           AND f.enabled = {}
    )";

#[derive(Clone)]
pub struct ProcessedFolderStore {
    datastore: StoreDatastore,
}

impl ProcessedFolderStore {
    pub fn new(datastore: StoreDatastore) -> Self {
        Self { datastore }
    }

    async fn map_folder_write_error(
        &self,
        error: AppError,
        exclude_id: Option<&str>,
        name: &str,
        normalized_root_path: &str,
    ) -> AppError {
        match error.repository_kind() {
            Some(RepositoryErrorKind::UniqueViolation) => self
                .unique_folder_refusal(exclude_id, name, normalized_root_path)
                .await
                .unwrap_or(error),
            Some(RepositoryErrorKind::ForeignKeyViolation) => AppError::validation_refused(
                processed_folder_refusal::LIBRARY_NOT_FOUND,
                "library was not found",
            ),
            _ => error,
        }
    }

    /// Which unique index lost, by reading the row that is already stored.
    /// The driver kind says a unique index lost; the columns say which one.
    async fn unique_folder_refusal(
        &self,
        exclude_id: Option<&str>,
        name: &str,
        normalized_root_path: &str,
    ) -> Result<AppError, AppError> {
        let name_owner = self.folder_id_with_name(name).await?;
        if name_owner
            .as_deref()
            .is_some_and(|id| Some(id) != exclude_id)
        {
            return Ok(AppError::validation_refused(
                NAME_TAKEN,
                "a processed folder with this name already exists",
            ));
        }
        let root_owner = self
            .folder_id_with_normalized_root(normalized_root_path)
            .await?;
        if root_owner
            .as_deref()
            .is_some_and(|id| Some(id) != exclude_id)
        {
            return Ok(AppError::validation_refused(
                OVERLAPS_OTHER_FOLDER,
                "processed folder path overlaps another processed folder",
            ));
        }
        Err(AppError::repository_failure(
            RepositoryErrorKind::UniqueViolation,
            "processed folder unique constraint could not be attributed",
        ))
    }

    async fn folder_id_with_name(&self, name: &str) -> AppResult<Option<String>> {
        let row = SqlRuntime::fetch_optional(
            self.datastore.read_exec(),
            "SELECT id FROM processed_folders WHERE name = {}",
            &[SqlArg::Text(name.to_string())],
        )
        .await?;
        row.map(|row| row.text("id")).transpose()
    }

    async fn folder_id_with_normalized_root(&self, normalized: &str) -> AppResult<Option<String>> {
        let row = SqlRuntime::fetch_optional(
            self.datastore.read_exec(),
            "SELECT id FROM processed_folders WHERE normalized_root_path = {}",
            &[SqlArg::Text(normalized.to_string())],
        )
        .await?;
        row.map(|row| row.text("id")).transpose()
    }
}

#[async_trait]
impl ProcessedFolderRepository for ProcessedFolderStore {
    async fn list_folders(&self) -> AppResult<Vec<ProcessedFolder>> {
        let rows = SqlRuntime::fetch_all(
            self.datastore.read_exec(),
            &format!("SELECT {FOLDER_COLUMNS} FROM processed_folders ORDER BY name, id"),
            &[],
        )
        .await?;
        rows.iter().map(folder_from_row).collect()
    }

    async fn get_folder(&self, id: &str) -> AppResult<Option<ProcessedFolder>> {
        let row = SqlRuntime::fetch_optional(
            self.datastore.read_exec(),
            &format!("SELECT {FOLDER_COLUMNS} FROM processed_folders WHERE id = {{}}"),
            &[SqlArg::Text(id.to_string())],
        )
        .await?;
        row.as_ref().map(folder_from_row).transpose()
    }

    async fn insert_folder(&self, folder: ProcessedFolder) -> AppResult<ProcessedFolder> {
        let stored = folder.clone();
        let result =
            SqlRuntime::run_in_transaction(&self.datastore, "insert_processed_folder", move |tx| {
                let folder = folder.clone();
                Box::pin(async move {
                    tx.execute(
                        "INSERT INTO processed_folders (
                            id, name, root_path, normalized_root_path, scope_kind, library_id,
                            enabled, min_stable_scans, min_age_seconds, marker_mode, marker_value,
                            last_scan_started_at, last_scan_finished_at, created_at, updated_at
                         ) VALUES ({}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {})",
                        &folder_args(&folder),
                    )
                    .await?;
                    Ok(())
                })
            })
            .await;
        match result {
            Ok(()) => Ok(stored),
            Err(error) => Err(self
                .map_folder_write_error(error, None, &stored.name, &stored.normalized_root_path)
                .await),
        }
    }

    async fn update_folder(&self, folder: ProcessedFolder) -> AppResult<bool> {
        let stored = folder.clone();
        let result =
            SqlRuntime::run_in_transaction(&self.datastore, "update_processed_folder", move |tx| {
                let folder = folder.clone();
                Box::pin(async move {
                    let rows = tx
                        .execute(
                            "UPDATE processed_folders SET
                                name = {}, root_path = {}, normalized_root_path = {},
                                scope_kind = {}, library_id = {}, enabled = {},
                                min_stable_scans = {}, min_age_seconds = {}, marker_mode = {},
                                marker_value = {}, last_scan_started_at = {},
                                last_scan_finished_at = {}, updated_at = {}
                              WHERE id = {}",
                            &[
                                SqlArg::Text(folder.name.clone()),
                                SqlArg::Text(folder.root_path.clone()),
                                SqlArg::Text(folder.normalized_root_path.clone()),
                                SqlArg::Text(folder.scope_kind.as_str().to_string()),
                                SqlArg::Text(folder.library_id.clone()),
                                SqlArg::Bool(folder.enabled),
                                SqlArg::I64(folder.min_stable_scans),
                                SqlArg::I64(folder.min_age_seconds),
                                SqlArg::Text(folder.marker_mode.as_str().to_string()),
                                SqlArg::OptText(folder.marker_value.clone()),
                                SqlArg::OptTimestamp(folder.last_scan_started_at),
                                SqlArg::OptTimestamp(folder.last_scan_finished_at),
                                SqlArg::Timestamp(folder.updated_at),
                                SqlArg::Text(folder.id.clone()),
                            ],
                        )
                        .await?;
                    Ok(rows > 0)
                })
            })
            .await;
        match result {
            Ok(updated) => Ok(updated),
            Err(error) => Err(self
                .map_folder_write_error(
                    error,
                    Some(stored.id.as_str()),
                    &stored.name,
                    &stored.normalized_root_path,
                )
                .await),
        }
    }

    async fn set_folder_enabled(
        &self,
        id: &str,
        enabled: bool,
        updated_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        let rows = SqlRuntime::execute_write(
            &self.datastore,
            "set_processed_folder_enabled",
            "UPDATE processed_folders SET enabled = {}, updated_at = {} WHERE id = {}",
            vec![
                SqlArg::Bool(enabled),
                SqlArg::Timestamp(updated_at),
                SqlArg::Text(id.to_string()),
            ],
        )
        .await?;
        Ok(rows > 0)
    }

    async fn delete_folder(&self, id: &str) -> AppResult<bool> {
        let id = id.to_string();
        SqlRuntime::run_in_transaction(&self.datastore, "delete_processed_folder", move |tx| {
            let id = id.clone();
            Box::pin(async move {
                tx.execute(
                    "DELETE FROM processed_folder_entries WHERE folder_id = {}",
                    &[SqlArg::Text(id.clone())],
                )
                .await?;
                let rows = tx
                    .execute(
                        "DELETE FROM processed_folders WHERE id = {}",
                        &[SqlArg::Text(id)],
                    )
                    .await?;
                Ok(rows > 0)
            })
        })
        .await
    }

    async fn record_scan_timestamps(
        &self,
        id: &str,
        started_at: Option<DateTime<Utc>>,
        finished_at: Option<DateTime<Utc>>,
        updated_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        let rows = SqlRuntime::execute_write(
            &self.datastore,
            "record_processed_folder_scan",
            "UPDATE processed_folders
                SET last_scan_started_at = {}, last_scan_finished_at = {}, updated_at = {}
              WHERE id = {}",
            vec![
                SqlArg::OptTimestamp(started_at),
                SqlArg::OptTimestamp(finished_at),
                SqlArg::Timestamp(updated_at),
                SqlArg::Text(id.to_string()),
            ],
        )
        .await?;
        Ok(rows > 0)
    }

    async fn list_entries(
        &self,
        query: ProcessedFolderEntryQuery,
    ) -> AppResult<ProcessedFolderEntryPage> {
        let (filter_sql, filter_args) = entry_filter(&query.folder_id, query.statuses.as_deref());
        // `filter_sql` already contains `{}` placeholders. Concatenate it so
        // `format!` does not treat those braces as its own arguments.
        let mut count_sql = String::from("SELECT COUNT(*) AS n FROM processed_folder_entries ");
        count_sql.push_str(&filter_sql);
        let count_row =
            SqlRuntime::fetch_optional(self.datastore.read_exec(), &count_sql, &filter_args)
                .await?
                .ok_or_else(|| {
                    AppError::Repository("processed folder entry count returned no row".into())
                })?;
        let total_count = count_row.i64("n")?;
        let mut args = filter_args;
        args.push(SqlArg::I64(query.limit));
        args.push(SqlArg::I64(query.offset));
        let mut page_sql = format!("SELECT {ENTRY_COLUMNS} FROM processed_folder_entries ");
        page_sql.push_str(&filter_sql);
        page_sql.push_str(" ORDER BY relative_path, id LIMIT {} OFFSET {}");
        let rows = SqlRuntime::fetch_all(self.datastore.read_exec(), &page_sql, &args).await?;
        let items = rows
            .iter()
            .map(entry_from_row)
            .collect::<AppResult<Vec<_>>>()?;
        Ok(ProcessedFolderEntryPage { items, total_count })
    }

    async fn list_all_entries(&self, folder_id: &str) -> AppResult<Vec<ProcessedFolderEntry>> {
        let rows = SqlRuntime::fetch_all(
            self.datastore.read_exec(),
            &format!(
                "SELECT {ENTRY_COLUMNS} FROM processed_folder_entries
                  WHERE folder_id = {{}} ORDER BY relative_path, id"
            ),
            &[SqlArg::Text(folder_id.to_string())],
        )
        .await?;
        rows.iter().map(entry_from_row).collect()
    }

    async fn get_entry(&self, id: &str) -> AppResult<Option<ProcessedFolderEntry>> {
        let row = SqlRuntime::fetch_optional(
            self.datastore.read_exec(),
            &format!("SELECT {ENTRY_COLUMNS} FROM processed_folder_entries WHERE id = {{}}"),
            &[SqlArg::Text(id.to_string())],
        )
        .await?;
        row.as_ref().map(entry_from_row).transpose()
    }

    async fn entry_counts(&self, folder_id: &str) -> AppResult<ProcessedFolderEntryCounts> {
        let rows = SqlRuntime::fetch_all(
            self.datastore.read_exec(),
            "SELECT status, COUNT(*) AS n FROM processed_folder_entries
              WHERE folder_id = {} GROUP BY status",
            &[SqlArg::Text(folder_id.to_string())],
        )
        .await?;
        let mut counts = ProcessedFolderEntryCounts::default();
        for row in &rows {
            let status = row.text("status")?;
            let status = ProcessedFolderEntryStatus::parse(&status).ok_or_else(|| {
                AppError::Repository(format!(
                    "processed folder entry has unknown status '{status}'"
                ))
            })?;
            counts.add(status, row.i64("n")?);
        }
        Ok(counts)
    }

    async fn count_importing_entries(&self, folder_id: &str) -> AppResult<i64> {
        let row = SqlRuntime::fetch_optional(
            self.datastore.read_exec(),
            "SELECT COUNT(*) AS n FROM processed_folder_entries
              WHERE folder_id = {} AND status = 'importing'",
            &[SqlArg::Text(folder_id.to_string())],
        )
        .await?
        .ok_or_else(|| {
            AppError::Repository("processed folder importing count returned no row".into())
        })?;
        row.i64("n")
    }

    async fn insert_entry(&self, entry: ProcessedFolderEntry) -> AppResult<ProcessedFolderEntry> {
        let stored = entry.clone();
        SqlRuntime::run_in_transaction(
            &self.datastore,
            "insert_processed_folder_entry",
            move |tx| {
                let entry = entry.clone();
                Box::pin(async move {
                    insert_entry_tx(tx, &entry).await?;
                    Ok(())
                })
            },
        )
        .await?;
        Ok(stored)
    }

    async fn claim_stable_entry(&self, entry_id: &str, now: DateTime<Utc>) -> AppResult<bool> {
        let rows = SqlRuntime::execute_write(
            &self.datastore,
            "claim_processed_folder_entry",
            CLAIM_STABLE_ENTRY_SQL,
            vec![
                SqlArg::Timestamp(now),
                SqlArg::Timestamp(now),
                SqlArg::Text(entry_id.to_string()),
                SqlArg::Bool(true),
            ],
        )
        .await?;
        Ok(rows == 1)
    }

    async fn force_entry_state_for_tests(
        &self,
        entry_id: &str,
        status: ProcessedFolderEntryStatus,
        attempts: i64,
        claimed_at: Option<DateTime<Utc>>,
        updated_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        let rows = SqlRuntime::execute_write(
            &self.datastore,
            "force_processed_folder_entry_state",
            "UPDATE processed_folder_entries
                SET status = {}, attempts = {}, claimed_at = {}, updated_at = {}
              WHERE id = {}",
            vec![
                SqlArg::Text(status.as_str().to_string()),
                SqlArg::I64(attempts),
                SqlArg::OptTimestamp(claimed_at),
                SqlArg::Timestamp(updated_at),
                SqlArg::Text(entry_id.to_string()),
            ],
        )
        .await?;
        Ok(rows > 0)
    }
}

fn folder_args(folder: &ProcessedFolder) -> Vec<SqlArg> {
    vec![
        SqlArg::Text(folder.id.clone()),
        SqlArg::Text(folder.name.clone()),
        SqlArg::Text(folder.root_path.clone()),
        SqlArg::Text(folder.normalized_root_path.clone()),
        SqlArg::Text(folder.scope_kind.as_str().to_string()),
        SqlArg::Text(folder.library_id.clone()),
        SqlArg::Bool(folder.enabled),
        SqlArg::I64(folder.min_stable_scans),
        SqlArg::I64(folder.min_age_seconds),
        SqlArg::Text(folder.marker_mode.as_str().to_string()),
        SqlArg::OptText(folder.marker_value.clone()),
        SqlArg::OptTimestamp(folder.last_scan_started_at),
        SqlArg::OptTimestamp(folder.last_scan_finished_at),
        SqlArg::Timestamp(folder.created_at),
        SqlArg::Timestamp(folder.updated_at),
    ]
}

async fn insert_entry_tx(tx: &mut SqlTx<'_>, entry: &ProcessedFolderEntry) -> AppResult<()> {
    let size_bytes = i64::try_from(entry.size_bytes).map_err(|_| {
        AppError::Repository(format!(
            "processed folder entry {} size does not fit a signed 64-bit integer",
            entry.id
        ))
    })?;
    tx.execute(
        "INSERT INTO processed_folder_entries (
            id, folder_id, relative_path, size_bytes, mtime_ns, fingerprint, status, reason_code,
            stable_scan_count, first_seen_at, last_seen_at, marker_seen, title_id, import_id,
            attempts, claimed_at, last_error, created_at, updated_at
         ) VALUES ({}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {})",
        &[
            SqlArg::Text(entry.id.clone()),
            SqlArg::Text(entry.folder_id.clone()),
            SqlArg::Text(entry.relative_path.clone()),
            SqlArg::I64(size_bytes),
            SqlArg::I64(entry.mtime_ns),
            SqlArg::Text(entry.fingerprint.clone()),
            SqlArg::Text(entry.status.as_str().to_string()),
            SqlArg::OptText(entry.reason_code.clone()),
            SqlArg::I64(entry.stable_scan_count),
            SqlArg::Timestamp(entry.first_seen_at),
            SqlArg::Timestamp(entry.last_seen_at),
            SqlArg::Bool(entry.marker_seen),
            SqlArg::OptText(entry.title_id.clone()),
            SqlArg::OptText(entry.import_id.clone()),
            SqlArg::I64(entry.attempts),
            SqlArg::OptTimestamp(entry.claimed_at),
            SqlArg::OptText(entry.last_error.clone()),
            SqlArg::Timestamp(entry.created_at),
            SqlArg::Timestamp(entry.updated_at),
        ],
    )
    .await?;
    Ok(())
}

fn folder_from_row(row: &SqlRow) -> AppResult<ProcessedFolder> {
    let id = row.text("id")?;
    let scope_kind = row.text("scope_kind")?;
    let scope_kind = ProcessedFolderScopeKind::parse(&scope_kind).ok_or_else(|| {
        AppError::validation_refused(
            SCOPE_KIND_INVALID,
            format!("processed folder {id} has scope '{scope_kind}'"),
        )
    })?;
    let marker_mode = row.text("marker_mode")?;
    let marker_mode = ProcessedFolderMarkerMode::parse(&marker_mode).ok_or_else(|| {
        AppError::Repository(format!(
            "processed folder {id} has unknown marker mode '{marker_mode}'"
        ))
    })?;
    Ok(ProcessedFolder {
        id,
        name: row.text("name")?,
        root_path: row.text("root_path")?,
        normalized_root_path: row.text("normalized_root_path")?,
        scope_kind,
        library_id: row.text("library_id")?,
        enabled: row.bool("enabled")?,
        min_stable_scans: row.i64("min_stable_scans")?,
        min_age_seconds: row.i64("min_age_seconds")?,
        marker_mode,
        marker_value: row.opt_text("marker_value")?,
        last_scan_started_at: row.opt_timestamp("last_scan_started_at")?,
        last_scan_finished_at: row.opt_timestamp("last_scan_finished_at")?,
        created_at: row.timestamp("created_at")?,
        updated_at: row.timestamp("updated_at")?,
    })
}

fn entry_from_row(row: &SqlRow) -> AppResult<ProcessedFolderEntry> {
    let id = row.text("id")?;
    let status = row.text("status")?;
    let status = ProcessedFolderEntryStatus::parse(&status).ok_or_else(|| {
        AppError::Repository(format!(
            "processed folder entry {id} has unknown status '{status}'"
        ))
    })?;
    let size_bytes = row.i64("size_bytes")?;
    let size_bytes = u64::try_from(size_bytes).map_err(|_| {
        AppError::Repository(format!("processed folder entry {id} has a negative size"))
    })?;
    let mtime_ns = row.i64("mtime_ns")?;
    if mtime_ns_to_utc(mtime_ns).is_none() {
        return Err(AppError::Repository(format!(
            "processed folder entry {id} has an mtime outside the UTC range"
        )));
    }
    Ok(ProcessedFolderEntry {
        id,
        folder_id: row.text("folder_id")?,
        relative_path: row.text("relative_path")?,
        size_bytes,
        mtime_ns,
        fingerprint: row.text("fingerprint")?,
        status,
        reason_code: row.opt_text("reason_code")?,
        stable_scan_count: row.i64("stable_scan_count")?,
        first_seen_at: row.timestamp("first_seen_at")?,
        last_seen_at: row.timestamp("last_seen_at")?,
        marker_seen: row.bool("marker_seen")?,
        title_id: row.opt_text("title_id")?,
        import_id: row.opt_text("import_id")?,
        attempts: row.i64("attempts")?,
        claimed_at: row.opt_timestamp("claimed_at")?,
        last_error: row.opt_text("last_error")?,
        created_at: row.timestamp("created_at")?,
        updated_at: row.timestamp("updated_at")?,
    })
}

fn entry_filter(
    folder_id: &str,
    statuses: Option<&[ProcessedFolderEntryStatus]>,
) -> (String, Vec<SqlArg>) {
    let mut sql = "WHERE folder_id = {}".to_string();
    let mut args = vec![SqlArg::Text(folder_id.to_string())];
    if let Some(statuses) = statuses {
        if statuses.is_empty() {
            sql.push_str(" AND 0 = 1");
        } else {
            sql.push_str(" AND status IN (");
            for (index, status) in statuses.iter().enumerate() {
                if index > 0 {
                    sql.push_str(", ");
                }
                sql.push_str("{}");
                args.push(SqlArg::Text(status.as_str().to_string()));
            }
            sql.push(')');
        }
    }
    (sql, args)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::TimeZone;
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 31, 12, 0, 0).unwrap()
    }

    fn folder(id: &str, name: &str, root: &str, enabled: bool) -> ProcessedFolder {
        ProcessedFolder {
            id: id.to_string(),
            name: name.to_string(),
            root_path: root.to_string(),
            normalized_root_path: root.to_string(),
            scope_kind: ProcessedFolderScopeKind::Library,
            library_id: "library-a".to_string(),
            enabled,
            min_stable_scans: 2,
            min_age_seconds: 300,
            marker_mode: ProcessedFolderMarkerMode::None,
            marker_value: None,
            last_scan_started_at: None,
            last_scan_finished_at: None,
            created_at: now(),
            updated_at: now(),
        }
    }

    fn entry(
        id: &str,
        folder_id: &str,
        status: ProcessedFolderEntryStatus,
    ) -> ProcessedFolderEntry {
        ProcessedFolderEntry {
            id: id.to_string(),
            folder_id: folder_id.to_string(),
            relative_path: format!("Show/{id}.mkv"),
            size_bytes: 4096,
            mtime_ns: 1_700_000_000_000_000_000,
            fingerprint: format!("fp-{id}"),
            status,
            reason_code: None,
            stable_scan_count: 1,
            first_seen_at: now(),
            last_seen_at: now(),
            marker_seen: false,
            title_id: None,
            import_id: None,
            attempts: 0,
            claimed_at: None,
            last_error: None,
            created_at: now(),
            updated_at: now(),
        }
    }

    async fn sqlite_store() -> ProcessedFolderStore {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite should open");
        sqlx::raw_sql(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE libraries (id TEXT PRIMARY KEY NOT NULL);
             INSERT INTO libraries (id) VALUES ('library-a');",
        )
        .execute(&pool)
        .await
        .expect("library fixture should apply");
        sqlx::raw_sql(include_str!(
            "../../scryer/src/db/migrations/0318_processed_folder_imports.sql"
        ))
        .execute(&pool)
        .await
        .expect("processed folder schema should apply");
        ProcessedFolderStore::new(StoreDatastore::sqlite(
            pool,
            Arc::new(tokio::sync::Mutex::new(())),
        ))
    }

    async fn exercise_processed_folder_port_methods(store: &ProcessedFolderStore) {
        let inserted = store
            .insert_folder(folder("folder-a", "Alpha", "/processed/alpha", false))
            .await
            .expect("insert folder");
        assert!(!inserted.enabled);
        assert_eq!(
            store.list_folders().await.expect("list").len(),
            store
                .list_folders()
                .await
                .expect("list")
                .iter()
                .filter(|folder| folder.id == "folder-a")
                .count()
        );
        let loaded = store
            .get_folder("folder-a")
            .await
            .expect("get")
            .expect("row");
        assert_eq!(loaded.name, "Alpha");
        assert_eq!(loaded.normalized_root_path, "/processed/alpha");

        let mut renamed = loaded.clone();
        renamed.name = "Alpha Two".to_string();
        renamed.marker_mode = ProcessedFolderMarkerMode::SiblingSuffix;
        renamed.marker_value = Some(".done".to_string());
        renamed.updated_at = now() + chrono::Duration::seconds(5);
        assert!(store.update_folder(renamed).await.expect("update"));
        let loaded = store.get_folder("folder-a").await.expect("get").unwrap();
        assert_eq!(loaded.name, "Alpha Two");
        assert_eq!(loaded.marker_value.as_deref(), Some(".done"));

        assert!(
            store
                .set_folder_enabled("folder-a", true, now())
                .await
                .expect("enable")
        );
        assert!(store.get_folder("folder-a").await.unwrap().unwrap().enabled);
        assert!(
            !store
                .set_folder_enabled("missing", true, now())
                .await
                .unwrap()
        );

        let started = now();
        let finished = now() + chrono::Duration::seconds(2);
        assert!(
            store
                .record_scan_timestamps("folder-a", Some(started), Some(finished), finished)
                .await
                .unwrap()
        );
        let loaded = store.get_folder("folder-a").await.unwrap().unwrap();
        assert_eq!(loaded.last_scan_started_at, Some(started));
        assert_eq!(loaded.last_scan_finished_at, Some(finished));
        assert!(
            !store
                .record_scan_timestamps("missing", None, None, now())
                .await
                .unwrap()
        );

        let stable = entry(
            "entry-stable",
            "folder-a",
            ProcessedFolderEntryStatus::Stable,
        );
        let observing = entry(
            "entry-observing",
            "folder-a",
            ProcessedFolderEntryStatus::Observing,
        );
        store.insert_entry(stable).await.expect("insert stable");
        store
            .insert_entry(observing)
            .await
            .expect("insert observing");
        let page = store
            .list_entries(ProcessedFolderEntryQuery {
                folder_id: "folder-a".to_string(),
                statuses: Some(vec![ProcessedFolderEntryStatus::Stable]),
                limit: 10,
                offset: 0,
            })
            .await
            .unwrap();
        assert_eq!(page.total_count, 1);
        assert_eq!(page.items[0].id, "entry-stable");
        assert_eq!(page.items[0].size_bytes, 4096);
        let all = store.list_all_entries("folder-a").await.unwrap();
        assert_eq!(all.len(), 2);
        let counts = store.entry_counts("folder-a").await.unwrap();
        assert_eq!(counts.stable, 1);
        assert_eq!(counts.observing, 1);
        assert_eq!(counts.importing_count(), 0);
        assert_eq!(store.count_importing_entries("folder-a").await.unwrap(), 0);

        assert!(
            store
                .claim_stable_entry("entry-stable", now())
                .await
                .unwrap()
        );
        let claimed = store.get_entry("entry-stable").await.unwrap().unwrap();
        assert_eq!(claimed.status, ProcessedFolderEntryStatus::Importing);
        assert_eq!(claimed.attempts, 1);
        assert_eq!(claimed.claimed_at, Some(now()));
        assert!(
            !store
                .claim_stable_entry("entry-stable", now())
                .await
                .unwrap()
        );
        assert_eq!(store.count_importing_entries("folder-a").await.unwrap(), 1);

        let forced_at = now() + chrono::Duration::seconds(9);
        assert!(
            store
                .force_entry_state_for_tests(
                    "entry-stable",
                    ProcessedFolderEntryStatus::Failed,
                    3,
                    None,
                    forced_at,
                )
                .await
                .unwrap()
        );
        let forced = store.get_entry("entry-stable").await.unwrap().unwrap();
        assert_eq!(forced.status, ProcessedFolderEntryStatus::Failed);
        assert_eq!(forced.attempts, 3);
        assert_eq!(forced.claimed_at, None);
        assert!(
            !store
                .force_entry_state_for_tests(
                    "missing-entry",
                    ProcessedFolderEntryStatus::Stable,
                    0,
                    None,
                    now(),
                )
                .await
                .unwrap()
        );
        assert!(store.get_entry("missing-entry").await.unwrap().is_none());
        assert!(store.get_folder("missing-folder").await.unwrap().is_none());
        assert!(
            !store
                .update_folder(folder("missing-folder", "No", "/processed/no", false))
                .await
                .unwrap()
        );
    }

    async fn exercise_processed_folder_unique_name(store: &ProcessedFolderStore) {
        store
            .insert_folder(folder("name-a", "Taken Name", "/processed/name-a", false))
            .await
            .unwrap();
        let error = store
            .insert_folder(folder("name-b", "Taken Name", "/processed/name-b", false))
            .await
            .expect_err("duplicate name");
        assert_eq!(error.validation_reason(), Some(NAME_TAKEN));
        assert_eq!(error.repository_kind(), None);
        let mut other = folder("name-c", "Other Name", "/processed/name-c", false);
        store.insert_folder(other.clone()).await.unwrap();
        other.name = "Taken Name".to_string();
        let error = store
            .update_folder(other)
            .await
            .expect_err("rename collision");
        assert_eq!(error.validation_reason(), Some(NAME_TAKEN));
    }

    async fn exercise_processed_folder_unique_root(store: &ProcessedFolderStore) {
        store
            .insert_folder(folder("root-a", "Root A", "/processed/same-root", false))
            .await
            .unwrap();
        let error = store
            .insert_folder(folder("root-b", "Root B", "/processed/same-root", false))
            .await
            .expect_err("duplicate root");
        assert_eq!(error.validation_reason(), Some(OVERLAPS_OTHER_FOLDER));
        let mut other = folder("root-c", "Root C", "/processed/other-root", false);
        store.insert_folder(other.clone()).await.unwrap();
        other.normalized_root_path = "/processed/same-root".to_string();
        other.root_path = "/processed/same-root".to_string();
        let error = store
            .update_folder(other)
            .await
            .expect_err("root collision");
        assert_eq!(error.validation_reason(), Some(OVERLAPS_OTHER_FOLDER));
    }

    async fn exercise_processed_folder_claim_when_disabled(store: &ProcessedFolderStore) {
        store
            .insert_folder(folder("disabled", "Disabled", "/processed/disabled", false))
            .await
            .unwrap();
        store
            .insert_entry(entry(
                "disabled-entry",
                "disabled",
                ProcessedFolderEntryStatus::Stable,
            ))
            .await
            .unwrap();
        assert!(
            !store
                .claim_stable_entry("disabled-entry", now())
                .await
                .unwrap()
        );
        let row = store.get_entry("disabled-entry").await.unwrap().unwrap();
        assert_eq!(row.status, ProcessedFolderEntryStatus::Stable);
        assert_eq!(row.attempts, 0);
        assert!(row.claimed_at.is_none());
    }

    async fn exercise_processed_folder_delete_cascade(store: &ProcessedFolderStore) {
        store
            .insert_folder(folder("doomed", "Doomed", "/processed/doomed", true))
            .await
            .unwrap();
        store
            .insert_entry(entry(
                "doomed-entry",
                "doomed",
                ProcessedFolderEntryStatus::Observing,
            ))
            .await
            .unwrap();
        assert!(store.delete_folder("doomed").await.unwrap());
        assert!(!store.delete_folder("doomed").await.unwrap());
        assert!(store.get_folder("doomed").await.unwrap().is_none());
        assert!(store.get_entry("doomed-entry").await.unwrap().is_none());

        store
            .insert_folder(folder("cascaded", "Cascaded", "/processed/cascaded", true))
            .await
            .unwrap();
        store
            .insert_entry(entry(
                "cascaded-entry",
                "cascaded",
                ProcessedFolderEntryStatus::Stable,
            ))
            .await
            .unwrap();
        SqlRuntime::execute_write(
            &store.datastore,
            "delete_processed_folder_library",
            "DELETE FROM libraries WHERE id = {}",
            vec![SqlArg::Text("library-a".to_string())],
        )
        .await
        .expect("library delete");
        assert!(store.get_folder("cascaded").await.unwrap().is_none());
        assert!(store.get_entry("cascaded-entry").await.unwrap().is_none());
    }

    async fn exercise_processed_folder_invalid_scope(store: &ProcessedFolderStore) {
        store
            .insert_folder(folder("scoped", "Scoped", "/processed/scoped", false))
            .await
            .unwrap();
        SqlRuntime::execute_write(
            &store.datastore,
            "corrupt_processed_folder_scope",
            "UPDATE processed_folders SET scope_kind = {} WHERE id = {}",
            vec![
                SqlArg::Text("download_client_category".to_string()),
                SqlArg::Text("scoped".to_string()),
            ],
        )
        .await
        .unwrap();
        let error = store.get_folder("scoped").await.expect_err("bad scope");
        assert_eq!(error.validation_reason(), Some(SCOPE_KIND_INVALID));
        assert!(store.delete_folder("scoped").await.unwrap());
    }

    async fn exercise_processed_folder_unknown_library(store: &ProcessedFolderStore) {
        let mut stray = folder("stray", "Stray", "/processed/stray", false);
        stray.library_id = "library-missing".to_string();
        let error = store
            .insert_folder(stray)
            .await
            .expect_err("missing library");
        assert_eq!(
            error.validation_reason(),
            Some(processed_folder_refusal::LIBRARY_NOT_FOUND)
        );
    }

    #[tokio::test]
    async fn processed_folder_sqlite_store_covers_every_port_method() {
        let store = sqlite_store().await;
        exercise_processed_folder_port_methods(&store).await;
        exercise_processed_folder_invalid_scope(&store).await;
        exercise_processed_folder_unknown_library(&store).await;
    }

    #[tokio::test]
    async fn processed_folder_sqlite_unique_name_is_name_taken() {
        exercise_processed_folder_unique_name(&sqlite_store().await).await;
    }

    #[tokio::test]
    async fn processed_folder_sqlite_unique_root_is_overlap() {
        exercise_processed_folder_unique_root(&sqlite_store().await).await;
    }

    #[tokio::test]
    async fn processed_folder_sqlite_claim_when_disabled_changes_nothing() {
        exercise_processed_folder_claim_when_disabled(&sqlite_store().await).await;
    }

    #[tokio::test]
    async fn processed_folder_sqlite_delete_cascades_entries_and_library() {
        exercise_processed_folder_delete_cascade(&sqlite_store().await).await;
    }

    #[tokio::test]
    async fn processed_folder_postgres_store_tests_run_when_configured() {
        let Ok(url) = std::env::var("SCRYER_TEST_POSTGRES_URL") else {
            eprintln!(
                "processed_folder postgres store tests skipped: SCRYER_TEST_POSTGRES_URL is unset"
            );
            return;
        };
        let host = postgres_host(&url);
        let admin = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect to test postgres");
        let schema = format!("pf_test_{}", uuid::Uuid::new_v4().simple());
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&admin)
            .await
            .expect("create schema");
        let schema_for_connect = schema.clone();
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .after_connect(move |conn, _meta| {
                let schema = schema_for_connect.clone();
                Box::pin(async move {
                    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("SET search_path TO {schema}")))
                        .execute(&mut *conn)
                        .await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await
            .expect("connect with search_path");
        sqlx::raw_sql(
            "CREATE TABLE libraries (id text PRIMARY KEY NOT NULL);
             INSERT INTO libraries (id) VALUES ('library-a');",
        )
        .execute(&pool)
        .await
        .expect("library fixture");
        sqlx::raw_sql(include_str!(
            "../../scryer/src/db/postgres/migrations/0318_processed_folder_imports.sql"
        ))
        .execute(&pool)
        .await
        .expect("processed folder schema");
        let store = ProcessedFolderStore::new(StoreDatastore::Postgres { pool: pool.clone() });
        let outcome = async {
            exercise_processed_folder_port_methods(&store).await;
            exercise_processed_folder_unique_name(&store).await;
            exercise_processed_folder_unique_root(&store).await;
            exercise_processed_folder_claim_when_disabled(&store).await;
            exercise_processed_folder_invalid_scope(&store).await;
            exercise_processed_folder_unknown_library(&store).await;
            exercise_processed_folder_delete_cascade(&store).await;
        };
        outcome.await;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE"
        )))
        .execute(&admin)
        .await
        .expect("drop schema");
        println!("processed_folder postgres store tests ran against {host}");
    }

    fn postgres_host(url: &str) -> String {
        url::Url::parse(url)
            .ok()
            .and_then(|parsed| {
                parsed.host_str().map(|host| match parsed.port() {
                    Some(port) => format!("{host}:{port}"),
                    None => host.to_string(),
                })
            })
            .unwrap_or_else(|| "unknown".to_string())
    }
}
