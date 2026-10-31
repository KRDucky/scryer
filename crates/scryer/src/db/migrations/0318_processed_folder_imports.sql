-- Folders whose finished media files Scryer imports into one library.
-- Scryer only reads these folders: imports always copy; nothing here is moved, linked or deleted.
CREATE TABLE processed_folders (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    root_path TEXT NOT NULL,
    normalized_root_path TEXT NOT NULL,
    -- PR1 supports 'library' only. PR2 adds 'download_client_category'
    -- (nullable download_client_id + category columns) without a table rebuild.
    scope_kind TEXT NOT NULL DEFAULT 'library',
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    enabled INTEGER NOT NULL DEFAULT 0,
    min_stable_scans INTEGER NOT NULL DEFAULT 2,
    min_age_seconds INTEGER NOT NULL DEFAULT 300,
    marker_mode TEXT NOT NULL DEFAULT 'none',      -- none | sibling_suffix | directory_file
    marker_value TEXT,
    last_scan_started_at TEXT,
    last_scan_finished_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
CREATE UNIQUE INDEX idx_processed_folders_normalized_root ON processed_folders(normalized_root_path);
CREATE UNIQUE INDEX idx_processed_folders_name ON processed_folders(name);
CREATE INDEX idx_processed_folders_library_id ON processed_folders(library_id);

-- One row per observed file version. A changed size or mtime is a new row.
CREATE TABLE processed_folder_entries (
    id TEXT PRIMARY KEY NOT NULL,
    folder_id TEXT NOT NULL REFERENCES processed_folders(id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    mtime_ns INTEGER NOT NULL,
    fingerprint TEXT NOT NULL,
    status TEXT NOT NULL,
    reason_code TEXT,
    stable_scan_count INTEGER NOT NULL DEFAULT 1,
    first_seen_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    marker_seen INTEGER NOT NULL DEFAULT 0,
    title_id TEXT,
    import_id TEXT,
    attempts INTEGER NOT NULL DEFAULT 0,
    claimed_at TEXT,
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
CREATE UNIQUE INDEX idx_processed_folder_entries_version
    ON processed_folder_entries(folder_id, relative_path, size_bytes, mtime_ns);
CREATE UNIQUE INDEX idx_processed_folder_entries_fingerprint ON processed_folder_entries(fingerprint);
CREATE INDEX idx_processed_folder_entries_status ON processed_folder_entries(folder_id, status);
