//! Migration 0312 (PR1 contract section 3): the processed-folder tables on
//! SQLite and Postgres, with the same columns, nullability, defaults and
//! indexes on both engines.
//!
//! The migration does not exist on baddf1f, so the engine tests are ignored
//! until `pf/pr1-base` lands. The Postgres test reads the SQL file at run time
//! (not `include_str!`) so this file compiles before the file exists, and it is
//! opt-in through `SCRYER_TEST_POSTGRES_URL` like
//! `delay_profile_tables_upgrade_tests.rs`. The store itself lives in
//! `scryer-infrastructure-acquisition`, which this crate cannot reach, so store
//! behaviour parity is tested there; this file covers schema parity only.

use sqlx::SqlitePool;

/// Version the catalog is replayed to before 0312 is applied.
const PRE_UPGRADE_VERSION: i64 = 311;

/// Postgres 0312 SQL, relative to this crate.
const POSTGRES_0312: &str =
    "../scryer/src/db/postgres/migrations/0312_processed_folder_imports.sql";

/// Expected type of a column on each engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// SQLite `TEXT`, Postgres `text`.
    Text,
    /// SQLite `INTEGER`, Postgres `bigint` (sizes, mtimes, counts).
    Integer,
    /// SQLite `INTEGER` 0/1, Postgres `boolean`.
    Flag,
    /// SQLite `TEXT`, Postgres `timestamp with time zone`.
    Timestamp,
}

impl Kind {
    fn sqlite(self) -> &'static str {
        match self {
            Self::Text | Self::Timestamp => "TEXT",
            Self::Integer | Self::Flag => "INTEGER",
        }
    }

    fn postgres(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Integer => "bigint",
            Self::Flag => "boolean",
            Self::Timestamp => "timestamp with time zone",
        }
    }
}

/// `(column, kind, not_null)` per table, in contract order.
const PROCESSED_FOLDERS: &[(&str, Kind, bool)] = &[
    ("id", Kind::Text, true),
    ("name", Kind::Text, true),
    ("root_path", Kind::Text, true),
    ("normalized_root_path", Kind::Text, true),
    ("scope_kind", Kind::Text, true),
    ("library_id", Kind::Text, true),
    ("enabled", Kind::Flag, true),
    ("min_stable_scans", Kind::Integer, true),
    ("min_age_seconds", Kind::Integer, true),
    ("marker_mode", Kind::Text, true),
    ("marker_value", Kind::Text, false),
    ("last_scan_started_at", Kind::Timestamp, false),
    ("last_scan_finished_at", Kind::Timestamp, false),
    ("created_at", Kind::Timestamp, true),
    ("updated_at", Kind::Timestamp, true),
];

const PROCESSED_FOLDER_ENTRIES: &[(&str, Kind, bool)] = &[
    ("id", Kind::Text, true),
    ("folder_id", Kind::Text, true),
    ("relative_path", Kind::Text, true),
    ("size_bytes", Kind::Integer, true),
    ("mtime_ns", Kind::Integer, true),
    ("fingerprint", Kind::Text, true),
    ("status", Kind::Text, true),
    ("reason_code", Kind::Text, false),
    ("stable_scan_count", Kind::Integer, true),
    ("first_seen_at", Kind::Timestamp, true),
    ("last_seen_at", Kind::Timestamp, true),
    ("marker_seen", Kind::Flag, true),
    ("title_id", Kind::Text, false),
    ("import_id", Kind::Text, false),
    ("attempts", Kind::Integer, true),
    ("claimed_at", Kind::Timestamp, false),
    ("last_error", Kind::Text, false),
    ("created_at", Kind::Timestamp, true),
    ("updated_at", Kind::Timestamp, true),
];

/// `(index, table, unique, columns)`.
const INDEXES: &[(&str, &str, bool, &[&str])] = &[
    (
        "idx_processed_folders_normalized_root",
        "processed_folders",
        true,
        &["normalized_root_path"],
    ),
    (
        "idx_processed_folders_name",
        "processed_folders",
        true,
        &["name"],
    ),
    (
        "idx_processed_folders_library_id",
        "processed_folders",
        false,
        &["library_id"],
    ),
    (
        "idx_processed_folder_entries_version",
        "processed_folder_entries",
        true,
        &["folder_id", "relative_path", "size_bytes", "mtime_ns"],
    ),
    (
        "idx_processed_folder_entries_fingerprint",
        "processed_folder_entries",
        true,
        &["fingerprint"],
    ),
    (
        "idx_processed_folder_entries_status",
        "processed_folder_entries",
        false,
        &["folder_id", "status"],
    ),
];

fn table_spec(table: &str) -> &'static [(&'static str, Kind, bool)] {
    match table {
        "processed_folders" => PROCESSED_FOLDERS,
        "processed_folder_entries" => PROCESSED_FOLDER_ENTRIES,
        other => panic!("no spec for {other}"),
    }
}

async fn pre_upgrade_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory SQLite should open");
    crate::migrations::replay_source_catalog_for_fresh_install(
        &pool,
        Some(PRE_UPGRADE_VERSION),
        true,
    )
    .await
    .expect("pre-0312 migration fixture should apply");
    pool
}

async fn upgraded_pool() -> SqlitePool {
    let pool = pre_upgrade_pool().await;
    crate::migrations::run_migrations(&pool, crate::MigrationMode::Apply)
        .await
        .expect("0312 upgrade should apply");
    pool
}

/// Runs today: the spec tables above are internally consistent and match the
/// contract's column and index counts.
#[test]
fn the_expected_schema_spec_is_consistent() {
    assert_eq!(PROCESSED_FOLDERS.len(), 15);
    assert_eq!(PROCESSED_FOLDER_ENTRIES.len(), 19);
    for (index, table, _, columns) in INDEXES {
        let spec = table_spec(table);
        for column in *columns {
            assert!(
                spec.iter().any(|(name, ..)| name == column),
                "{index} names unknown column {table}.{column}"
            );
        }
    }
    for spec in [PROCESSED_FOLDERS, PROCESSED_FOLDER_ENTRIES] {
        let mut names: Vec<_> = spec.iter().map(|(name, ..)| *name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), spec.len(), "duplicate column in spec");
    }
    assert_eq!(Kind::Flag.postgres(), "boolean");
    assert_eq!(Kind::Timestamp.sqlite(), "TEXT");
}

#[tokio::test]
#[ignore = "pending pf/pr1-base: migration 0312 (SQLite) and manifest entry"]
async fn sqlite_upgrade_creates_both_tables_with_the_contract_columns() {
    let pool = upgraded_pool().await;
    for table in ["processed_folders", "processed_folder_entries"] {
        let columns: Vec<(String, String, i64)> = sqlx::query_as(
            "SELECT name, type, \"notnull\" FROM pragma_table_info(?1) ORDER BY cid",
        )
        .bind(table)
        .fetch_all(&pool)
        .await
        .expect("table info");
        let expected: Vec<(String, String, i64)> = table_spec(table)
            .iter()
            .map(|(name, kind, not_null)| {
                (
                    (*name).to_string(),
                    kind.sqlite().to_string(),
                    i64::from(*not_null),
                )
            })
            .collect();
        assert_eq!(columns, expected, "{table}");
    }
}

#[tokio::test]
#[ignore = "pending pf/pr1-base: migration 0312 (SQLite) and manifest entry"]
async fn sqlite_upgrade_creates_the_contract_indexes() {
    let pool = upgraded_pool().await;
    for (index, table, unique, columns) in INDEXES {
        let (on_table, is_unique): (String, i64) = sqlx::query_as(
            "SELECT m.name, l.\"unique\" FROM sqlite_master m, pragma_index_list(m.name) l
             WHERE m.type = 'table' AND l.name = ?1",
        )
        .bind(index)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|error| panic!("{index} missing: {error}"));
        assert_eq!(on_table, *table, "{index}");
        assert_eq!(is_unique == 1, *unique, "{index}");
        let actual: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")
                .bind(index)
                .fetch_all(&pool)
                .await
                .expect("index info");
        assert_eq!(actual, *columns, "{index}");
    }
}

#[tokio::test]
#[ignore = "pending pf/pr1-base: migration 0312 (SQLite) and manifest entry"]
async fn sqlite_defaults_uniqueness_and_cascade_follow_the_contract() {
    let pool = upgraded_pool().await;
    sqlx::query(
        "INSERT INTO libraries (id, facet, name, slug, created_at, updated_at)
         VALUES ('lib-pf', 'movie', 'PF', 'pf', '2026-10-01T00:00:00Z', '2026-10-01T00:00:00Z')",
    )
    .execute(&pool)
    .await
    .expect("seed library");
    sqlx::query(
        "INSERT INTO processed_folders
                 (id, name, root_path, normalized_root_path, library_id, created_at, updated_at)
         VALUES ('pf-1', 'Synthetic', '/synthetic/root', '/synthetic/root', 'lib-pf',
                 '2026-10-01T00:00:00Z', '2026-10-01T00:00:00Z')",
    )
    .execute(&pool)
    .await
    .expect("insert folder with defaults");
    let defaults: (String, i64, i64, i64, String, Option<String>) = sqlx::query_as(
        "SELECT scope_kind, enabled, min_stable_scans, min_age_seconds, marker_mode, marker_value
         FROM processed_folders WHERE id = 'pf-1'",
    )
    .fetch_one(&pool)
    .await
    .expect("read defaults");
    assert_eq!(
        defaults,
        ("library".to_string(), 0, 2, 300, "none".to_string(), None),
        "created disabled with the section 3 defaults"
    );

    let insert_entry = |id: &'static str, fingerprint: &'static str, mtime_ns: i64| {
        sqlx::query(
            "INSERT INTO processed_folder_entries
                 (id, folder_id, relative_path, size_bytes, mtime_ns, fingerprint, status,
                  first_seen_at, last_seen_at, created_at, updated_at)
             VALUES (?1, 'pf-1', 'a/Synthetic.mkv', 10, ?3, ?2, 'observing',
                     '2026-10-01T00:00:00Z', '2026-10-01T00:00:00Z',
                     '2026-10-01T00:00:00Z', '2026-10-01T00:00:00Z')",
        )
        .bind(id)
        .bind(fingerprint)
        .bind(mtime_ns)
    };
    insert_entry("e-1", "pf1:aa", 1)
        .execute(&pool)
        .await
        .expect("first version");
    insert_entry("e-2", "pf1:bb", 1)
        .execute(&pool)
        .await
        .expect_err("same (folder, path, size, mtime) is one version");
    insert_entry("e-3", "pf1:aa", 2)
        .execute(&pool)
        .await
        .expect_err("fingerprints are unique");
    insert_entry("e-4", "pf1:cc", 2)
        .execute(&pool)
        .await
        .expect("a new mtime is a new version");
    let entry_defaults: (i64, i64, i64) = sqlx::query_as(
        "SELECT stable_scan_count, marker_seen, attempts FROM processed_folder_entries
         WHERE id = 'e-1'",
    )
    .fetch_one(&pool)
    .await
    .expect("entry defaults");
    assert_eq!(entry_defaults, (1, 0, 0));

    sqlx::query("DELETE FROM libraries WHERE id = 'lib-pf'")
        .execute(&pool)
        .await
        .expect("delete library");
    let left: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM processed_folders) + (SELECT COUNT(*) FROM processed_folder_entries)",
    )
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(left, 0, "library delete cascades to folders and entries");
}

#[tokio::test]
#[ignore = "pending pf/pr1-base: migration 0312 (Postgres)"]
async fn postgres_0312_matches_the_sqlite_schema() {
    let Ok(url) = std::env::var("SCRYER_TEST_POSTGRES_URL") else {
        eprintln!("skipped: SCRYER_TEST_POSTGRES_URL must name an isolated test database");
        return;
    };
    let sql_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(POSTGRES_0312);
    let sql = std::fs::read_to_string(&sql_path)
        .unwrap_or_else(|error| panic!("{}: {error}", sql_path.display()));
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .expect("connect to test postgres");
    let mut tx = pool.begin().await.expect("begin fixture");
    let schema = format!("upgrade_test_{}", uuid::Uuid::new_v4().simple());
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE SCHEMA {schema}; SET LOCAL search_path TO {schema};"
    )))
    .execute(&mut *tx)
    .await
    .expect("isolate fixture");
    sqlx::raw_sql("CREATE TABLE libraries (id text PRIMARY KEY NOT NULL);")
        .execute(&mut *tx)
        .await
        .expect("seed libraries");
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(&mut *tx)
        .await
        .expect("apply 0312");

    for table in ["processed_folders", "processed_folder_entries"] {
        let columns: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT column_name::text, data_type::text, is_nullable::text
             FROM information_schema.columns
             WHERE table_schema = $1 AND table_name = $2 ORDER BY ordinal_position",
        )
        .bind(&schema)
        .bind(table)
        .fetch_all(&mut *tx)
        .await
        .expect("columns");
        let expected: Vec<(String, String, String)> = table_spec(table)
            .iter()
            .map(|(name, kind, not_null)| {
                (
                    (*name).to_string(),
                    kind.postgres().to_string(),
                    if *not_null { "NO" } else { "YES" }.to_string(),
                )
            })
            .collect();
        assert_eq!(columns, expected, "{table}");
    }

    for (index, table, unique, columns) in INDEXES {
        let (on_table, is_unique, actual): (String, bool, Vec<String>) = sqlx::query_as(
            "SELECT t.relname::text, ix.indisunique,
                    ARRAY(SELECT a.attname::text FROM unnest(ix.indkey) WITH ORDINALITY k(attnum, n)
                          JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = k.attnum
                          ORDER BY k.n)
             FROM pg_index ix
             JOIN pg_class i ON i.oid = ix.indexrelid
             JOIN pg_class t ON t.oid = ix.indrelid
             JOIN pg_namespace n ON n.oid = i.relnamespace
             WHERE n.nspname = $1 AND i.relname = $2",
        )
        .bind(&schema)
        .bind(index)
        .fetch_one(&mut *tx)
        .await
        .unwrap_or_else(|error| panic!("{index} missing: {error}"));
        assert_eq!(on_table, *table, "{index}");
        assert_eq!(is_unique, *unique, "{index}");
        assert_eq!(actual, *columns, "{index}");
    }

    let defaults: (String, bool, i64, i64, String) = {
        sqlx::raw_sql(
            "INSERT INTO libraries VALUES ('lib-pf');
             INSERT INTO processed_folders
                 (id, name, root_path, normalized_root_path, library_id, created_at, updated_at)
             VALUES ('pf-1', 'Synthetic', '/synthetic/root', '/synthetic/root', 'lib-pf',
                 '2026-10-01T00:00:00Z', '2026-10-01T00:00:00Z');",
        )
        .execute(&mut *tx)
        .await
        .expect("insert folder with defaults");
        sqlx::query_as(
            "SELECT scope_kind, enabled, min_stable_scans, min_age_seconds, marker_mode
             FROM processed_folders WHERE id = 'pf-1'",
        )
        .fetch_one(&mut *tx)
        .await
        .expect("read defaults")
    };
    assert_eq!(
        defaults,
        ("library".to_string(), false, 2, 300, "none".to_string())
    );
    tx.rollback().await.expect("roll back synthetic fixture");
}
