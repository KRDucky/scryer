-- Folders whose finished media files Scryer imports into one library.
-- Scryer only reads these folders: imports always copy; nothing here is moved, linked or deleted.
CREATE TABLE processed_folders (
    id text PRIMARY KEY NOT NULL,
    name text NOT NULL,
    root_path text NOT NULL,
    normalized_root_path text NOT NULL,
    scope_kind text NOT NULL DEFAULT 'library',
    library_id text NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    enabled boolean NOT NULL DEFAULT FALSE,
    min_stable_scans bigint NOT NULL DEFAULT 2,
    min_age_seconds bigint NOT NULL DEFAULT 300,
    marker_mode text NOT NULL DEFAULT 'none',
    marker_value text,
    last_scan_started_at timestamptz,
    last_scan_finished_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT NOW(),
    updated_at timestamptz NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX idx_processed_folders_normalized_root ON processed_folders(normalized_root_path);
CREATE UNIQUE INDEX idx_processed_folders_name ON processed_folders(name);
CREATE INDEX idx_processed_folders_library_id ON processed_folders(library_id);

-- One row per observed file version. A changed size or mtime is a new row.
CREATE TABLE processed_folder_entries (
    id text PRIMARY KEY NOT NULL,
    folder_id text NOT NULL REFERENCES processed_folders(id) ON DELETE CASCADE,
    relative_path text NOT NULL,
    size_bytes bigint NOT NULL,
    mtime_ns bigint NOT NULL,
    fingerprint text NOT NULL,
    status text NOT NULL,
    reason_code text,
    stable_scan_count bigint NOT NULL DEFAULT 1,
    first_seen_at timestamptz NOT NULL,
    last_seen_at timestamptz NOT NULL,
    marker_seen boolean NOT NULL DEFAULT FALSE,
    title_id text,
    import_id text,
    attempts bigint NOT NULL DEFAULT 0,
    claimed_at timestamptz,
    last_error text,
    created_at timestamptz NOT NULL DEFAULT NOW(),
    updated_at timestamptz NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX idx_processed_folder_entries_version
    ON processed_folder_entries(folder_id, relative_path, size_bytes, mtime_ns);
CREATE UNIQUE INDEX idx_processed_folder_entries_fingerprint ON processed_folder_entries(fingerprint);
CREATE INDEX idx_processed_folder_entries_status ON processed_folder_entries(folder_id, status);
