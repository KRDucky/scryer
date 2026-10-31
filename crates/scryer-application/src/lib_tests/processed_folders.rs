//! Processed-media folder importer (PR1, copy-only): application-level tests.
//!
//! The PR1 contract v2.2 is the source of truth (V0-V7 override the v2.1 body):
//! section 1.4 (Q1 to Q7), section 2 (state machine, action matrix, restart
//! recovery), section 7 (validation) and section 8 (other cases). The contract
//! lives outside the repository.
//!
//! Layout of this file:
//! - `fixtures`: helpers that do not depend on PR1 types. They compile today and
//!   `fixture_self_tests` proves them.
//! - every other module holds skeletons for behaviour that needs the base branch
//!   (`pf/pr1-base`: domain types, `ProcessedFolderRepository`, the `AppUseCase`
//!   processed-folder API) or the core branch (`pf/pr1-core`: copy-only pipeline,
//!   scan, stability, importer, actions). Each skeleton is ignored with the branch
//!   it waits for and its body is a `todo!` that lists the exact steps and
//!   assertions, so the ignored list doubles as the open-work list.
//!
//! Tests here use `FileImporter` doubles. `FsFileImporter` lives in
//! `scryer-infrastructure-workflow`, which depends on this crate, so the variants
//! that need the real importer are in
//! `crates/scryer/tests/integration_graphql/processed_folders.rs`.

use super::*;

pub(super) mod fixtures {
    //! Processed-folder fixtures. Synthetic bytes only; nothing here reads or
    //! writes outside the temp directories it creates.
    #![allow(
        dead_code,
        reason = "fixtures are shared with skeletons that start using them once pf/pr1-base lands"
    )]

    use super::super::support_imports::test_import_source_snapshot;
    use crate::{AppError, AppResult, FileImporter};
    use std::collections::{BTreeMap, VecDeque};
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    // Contract constants mirrored for fixtures.

    /// Contract decision 2: the synthetic client type. The base branch adds the
    /// real `PROCESSED_FOLDER_CLIENT_TYPE`; a skeleton asserts they are equal.
    pub(crate) const EXPECTED_CLIENT_TYPE: &str = "processed_folder";

    /// Contract section 7.5: suffixes a scan ignores.
    pub(crate) const IGNORED_SUFFIXES: &[&str] = &[
        ".part",
        ".partial",
        ".tmp",
        ".tmp_import",
        ".!qB",
        ".crdownload",
        ".filepart",
    ];

    /// Contract section 7.5: name prefixes a scan ignores.
    pub(crate) const IGNORED_PREFIXES: &[&str] = &[".", "~$"];

    /// Contract V4: every refusal code, in table order. The base branch's
    /// `processed_folder_refusal::ALL` must equal this set (26 codes).
    pub(crate) const EXPECTED_REFUSAL_CODES: &[&str] = &[
        "PROCESSED_FOLDER_NOT_FOUND",
        "PROCESSED_FOLDER_ENTRY_NOT_FOUND",
        "PROCESSED_FOLDER_NAME_REQUIRED",
        "PROCESSED_FOLDER_NAME_TOO_LONG",
        "PROCESSED_FOLDER_NAME_TAKEN",
        "PROCESSED_FOLDER_FIELD_NOT_TRIMMED",
        "PROCESSED_FOLDER_PATH_NOT_ABSOLUTE",
        "PROCESSED_FOLDER_PATH_MISSING",
        "PROCESSED_FOLDER_PATH_NOT_DIRECTORY",
        "PROCESSED_FOLDER_PATH_UNRESOLVABLE",
        "PROCESSED_FOLDER_OVERLAPS_LIBRARY_ROOT",
        "PROCESSED_FOLDER_OVERLAPS_RECYCLE_BIN",
        "PROCESSED_FOLDER_OVERLAPS_BACKUP_PATH",
        "PROCESSED_FOLDER_OVERLAPS_OTHER_FOLDER",
        "PROCESSED_FOLDER_LIBRARY_NOT_FOUND",
        "PROCESSED_FOLDER_SCOPE_KIND_INVALID",
        "PROCESSED_FOLDER_INVALID_STABILITY",
        "PROCESSED_FOLDER_INVALID_MARKER",
        "PROCESSED_FOLDER_MARKER_VALUE_NOT_ALLOWED",
        "PROCESSED_FOLDER_DISABLED",
        "PROCESSED_FOLDER_SCAN_IN_PROGRESS",
        "PROCESSED_FOLDER_ENTRY_NOT_ACTIONABLE",
        "PROCESSED_FOLDER_ENTRY_CHANGED",
        "PROCESSED_FOLDER_TITLE_OUTSIDE_LIBRARY",
        "PROCESSED_FOLDER_TITLE_NOT_FOUND",
        "PROCESSED_FOLDER_IMPORTS_IN_FLIGHT",
    ];

    /// Exact V4 trigger for each code, same order as `EXPECTED_REFUSAL_CODES`.
    pub(crate) const REFUSAL_TRIGGERS: &[(&str, &str)] = &[
        (
            "PROCESSED_FOLDER_NOT_FOUND",
            "folder id does not exist, on every folder query and mutation",
        ),
        (
            "PROCESSED_FOLDER_ENTRY_NOT_FOUND",
            "entry id does not exist, on entry actions",
        ),
        (
            "PROCESSED_FOLDER_NAME_REQUIRED",
            "create and update with name empty (after the trim check; whitespace is FIELD_NOT_TRIMMED, never trimmed)",
        ),
        (
            "PROCESSED_FOLDER_NAME_TOO_LONG",
            "create and update: name of 101 Unicode scalar values is refused; a name of 100 scalar values is accepted",
        ),
        (
            "PROCESSED_FOLDER_NAME_TAKEN",
            "create and update: the store reports RepositoryErrorKind::UniqueViolation on idx_processed_folders_name",
        ),
        (
            "PROCESSED_FOLDER_FIELD_NOT_TRIMMED",
            "create and update: name, rootPath, or markerValue has leading or trailing whitespace; refuse, never trim",
        ),
        (
            "PROCESSED_FOLDER_PATH_NOT_ABSOLUTE",
            "create, update, and scan start: rootPath is not absolute",
        ),
        (
            "PROCESSED_FOLDER_PATH_MISSING",
            "create, update, and scan start: rootPath does not exist",
        ),
        (
            "PROCESSED_FOLDER_PATH_NOT_DIRECTORY",
            "create, update, and scan start: rootPath is not a directory",
        ),
        (
            "PROCESSED_FOLDER_PATH_UNRESOLVABLE",
            "create, update, and scan start: canonicalize or resolve_existing_component fails",
        ),
        (
            "PROCESSED_FOLDER_OVERLAPS_LIBRARY_ROOT",
            "create, update, and scan start: root is equal to, an ancestor of, or a descendant of any library root, lexically or canonically; also the reverse library-root change",
        ),
        (
            "PROCESSED_FOLDER_OVERLAPS_RECYCLE_BIN",
            "create, update, and scan start: root overlaps the enabled custom recycle bin; also the reverse recycle-bin change",
        ),
        (
            "PROCESSED_FOLDER_OVERLAPS_BACKUP_PATH",
            "create, update, and scan start: root overlaps BACKUP_PATH_KEY",
        ),
        (
            "PROCESSED_FOLDER_OVERLAPS_OTHER_FOLDER",
            "create and update: root overlaps another processed folder, or the store reports UniqueViolation on idx_processed_folders_normalized_root",
        ),
        (
            "PROCESSED_FOLDER_LIBRARY_NOT_FOUND",
            "create and update: libraryId does not exist",
        ),
        (
            "PROCESSED_FOLDER_SCOPE_KIND_INVALID",
            "store read, create, and update: scope_kind is not library",
        ),
        (
            "PROCESSED_FOLDER_INVALID_STABILITY",
            "create and update: minStableScans is outside 2..=10 or minAgeSeconds is outside 60..=86400",
        ),
        (
            "PROCESSED_FOLDER_INVALID_MARKER",
            "create and update: mode SIBLING_SUFFIX or DIRECTORY_FILE with a missing or malformed marker value (V1.2)",
        ),
        (
            "PROCESSED_FOLDER_MARKER_VALUE_NOT_ALLOWED",
            "create and update: markerValue is set while the resulting mode is NONE",
        ),
        (
            "PROCESSED_FOLDER_DISABLED",
            "scan, import-as, or retry on a disabled folder, and a claim refused because the folder is disabled (V2.1)",
        ),
        (
            "PROCESSED_FOLDER_SCAN_IN_PROGRESS",
            "scan: another scan of the same folder holds the scan lock",
        ),
        (
            "PROCESSED_FOLDER_ENTRY_NOT_ACTIONABLE",
            "entry action not allowed in the row state, or the transition UPDATE changed 0 rows",
        ),
        (
            "PROCESSED_FOLDER_ENTRY_CHANGED",
            "import-as: import-time re-stat or re-canonicalize differs, and the row becomes superseded",
        ),
        (
            "PROCESSED_FOLDER_TITLE_OUTSIDE_LIBRARY",
            "import-as: titleId belongs to another library",
        ),
        (
            "PROCESSED_FOLDER_TITLE_NOT_FOUND",
            "import-as: titleId does not exist",
        ),
        (
            "PROCESSED_FOLDER_IMPORTS_IN_FLIGHT",
            "delete: an entry is still importing after orphan recovery (V2.3)",
        ),
    ];

    /// V4 create/update check order. Later phases run only after earlier ones
    /// pass. `OVERLAPS_OTHER_FOLDER` is both the overlap phase and the
    /// normalized-root unique violation at store write.
    pub(crate) const CREATE_UPDATE_CHECK_ORDER: &[&[&str]] = &[
        &["PROCESSED_FOLDER_FIELD_NOT_TRIMMED"],
        &[
            "PROCESSED_FOLDER_NAME_REQUIRED",
            "PROCESSED_FOLDER_NAME_TOO_LONG",
        ],
        &[
            "PROCESSED_FOLDER_PATH_NOT_ABSOLUTE",
            "PROCESSED_FOLDER_PATH_MISSING",
            "PROCESSED_FOLDER_PATH_NOT_DIRECTORY",
            "PROCESSED_FOLDER_PATH_UNRESOLVABLE",
        ],
        &["PROCESSED_FOLDER_LIBRARY_NOT_FOUND"],
        &["PROCESSED_FOLDER_SCOPE_KIND_INVALID"],
        &["PROCESSED_FOLDER_INVALID_STABILITY"],
        &[
            "PROCESSED_FOLDER_INVALID_MARKER",
            "PROCESSED_FOLDER_MARKER_VALUE_NOT_ALLOWED",
        ],
        &[
            "PROCESSED_FOLDER_OVERLAPS_LIBRARY_ROOT",
            "PROCESSED_FOLDER_OVERLAPS_RECYCLE_BIN",
            "PROCESSED_FOLDER_OVERLAPS_BACKUP_PATH",
            "PROCESSED_FOLDER_OVERLAPS_OTHER_FOLDER",
        ],
        &[
            "PROCESSED_FOLDER_NAME_TAKEN",
            "PROCESSED_FOLDER_OVERLAPS_OTHER_FOLDER",
        ],
    ];

    /// Contract section 2.1: ledger states, in SDL order.
    pub(crate) const STATUSES: &[&str] = &[
        "observing",
        "stable",
        "importing",
        "imported",
        "skipped",
        "failed",
        "needs_review",
        "ignored",
        "rejected",
        "superseded",
        "gone",
    ];

    /// An operator action from contract section 2.3.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub(crate) enum EntryAction {
        ImportAs,
        Retry,
        Ignore,
    }

    pub(crate) const ACTIONS: &[EntryAction] = &[
        EntryAction::ImportAs,
        EntryAction::Retry,
        EntryAction::Ignore,
    ];

    /// Contract section 2.3, cell by cell: (status, importAs, retry, ignore).
    pub(crate) const ACTION_MATRIX: &[(&str, bool, bool, bool)] = &[
        ("observing", false, false, true),
        ("stable", false, false, true),
        ("importing", false, false, false),
        ("imported", false, false, false),
        ("skipped", true, false, true),
        ("failed", true, true, true),
        ("needs_review", true, false, true),
        ("ignored", false, true, false),
        ("rejected", false, false, false),
        ("superseded", false, false, false),
        ("gone", false, false, false),
    ];

    /// Whether the contract allows `action` on an entry in `status`.
    pub(crate) fn action_allowed(status: &str, action: EntryAction) -> bool {
        let (_, import_as, retry, ignore) = ACTION_MATRIX
            .iter()
            .copied()
            .find(|(candidate, ..)| *candidate == status)
            .unwrap_or_else(|| panic!("unknown ledger status {status}"));
        match action {
            EntryAction::ImportAs => import_as,
            EntryAction::Retry => retry,
            EntryAction::Ignore => ignore,
        }
    }

    /// The actor that drives a transition (contract section 2.2).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Actor {
        Scan,
        Worker,
        Operator,
        Recovery,
    }

    /// One row of contract section 2.2. `from` is empty for a new row; `to` is
    /// `"per_2_4"` for recovery.
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct TransitionRow {
        pub(crate) row: usize,
        pub(crate) from: &'static [&'static str],
        pub(crate) to: &'static str,
        pub(crate) actor: Actor,
    }

    const fn transition(
        row: usize,
        from: &'static [&'static str],
        to: &'static str,
        actor: Actor,
    ) -> TransitionRow {
        TransitionRow {
            row,
            from,
            to,
            actor,
        }
    }

    pub(crate) const TRANSITIONS: &[TransitionRow] = &[
        transition(1, &[], "observing", Actor::Scan),
        transition(2, &[], "rejected", Actor::Scan),
        transition(3, &["observing"], "observing", Actor::Scan),
        transition(4, &["observing"], "stable", Actor::Scan),
        transition(5, &["observing", "stable"], "superseded", Actor::Scan),
        transition(
            6,
            &[
                "observing",
                "stable",
                "needs_review",
                "failed",
                "skipped",
                "ignored",
            ],
            "gone",
            Actor::Scan,
        ),
        transition(7, &["gone"], "observing", Actor::Scan),
        transition(8, &["stable"], "importing", Actor::Worker),
        transition(9, &["stable"], "needs_review", Actor::Worker),
        transition(10, &["importing"], "superseded", Actor::Worker),
        transition(11, &["importing"], "imported", Actor::Worker),
        transition(12, &["importing"], "needs_review", Actor::Worker),
        transition(13, &["importing"], "skipped", Actor::Worker),
        transition(14, &["importing"], "failed", Actor::Worker),
        transition(15, &["importing"], "per_2_4", Actor::Recovery),
        transition(
            16,
            &["needs_review", "failed", "skipped"],
            "importing",
            Actor::Operator,
        ),
        transition(17, &["failed", "ignored"], "stable", Actor::Operator),
        transition(
            18,
            &["observing", "stable", "needs_review", "failed", "skipped"],
            "ignored",
            Actor::Operator,
        ),
    ];

    /// One row of the V0.2.7 result mapping.
    ///
    /// `skip_reason` is `*` for any skip reason, `-` when the skip reason is
    /// absent, or `ImportSkipReason::as_str`. An empty `reason_code` means the
    /// ledger stores none. `error_else_failed` and `import_io_else_error` are
    /// sentinels the mapping skeleton spells out. `attempt_cap` 3 means
    /// `attempts >= 3` stores `failed` with the same reason.
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct ResultMapping {
        pub decision: &'static str,
        pub skip_reason: &'static str,
        pub ledger_status: &'static str,
        pub reason_code: &'static str,
        pub attempt_cap: u8,
        pub executor_proposed: bool,
    }

    const fn result_row(
        decision: &'static str,
        skip_reason: &'static str,
        ledger_status: &'static str,
        reason_code: &'static str,
        attempt_cap: u8,
        executor_proposed: bool,
    ) -> ResultMapping {
        ResultMapping {
            decision,
            skip_reason,
            ledger_status,
            reason_code,
            attempt_cap,
            executor_proposed,
        }
    }

    pub(crate) const RESULT_MAPPINGS: &[ResultMapping] = &[
        result_row("Imported", "*", "imported", "", 0, false),
        result_row(
            "Imported",
            "scene_subtitle_pending",
            "imported",
            "",
            0,
            false,
        ),
        result_row(
            "Skipped",
            "already_imported",
            "imported",
            "already_imported",
            0,
            false,
        ),
        result_row(
            "Skipped",
            "duplicate_file",
            "skipped",
            "duplicate_file",
            0,
            false,
        ),
        result_row(
            "Skipped",
            "post_download_rule_blocked",
            "skipped",
            "post_download_rule_blocked",
            0,
            false,
        ),
        result_row(
            "Skipped",
            "policy_mismatch",
            "skipped",
            "policy_mismatch",
            0,
            false,
        ),
        result_row(
            "Skipped",
            "unresolved_identity",
            "needs_review",
            "unresolved_identity",
            0,
            true,
        ),
        result_row(
            "Skipped",
            "unparseable_episode",
            "needs_review",
            "unparseable_episode",
            0,
            true,
        ),
        result_row(
            "Skipped",
            "no_video_files",
            "skipped",
            "no_video_files",
            0,
            false,
        ),
        result_row(
            "Skipped",
            "unwanted_executables",
            "skipped",
            "unwanted_executables",
            0,
            false,
        ),
        result_row(
            "Skipped",
            "download_in_progress",
            "stable",
            "download_in_progress",
            3,
            true,
        ),
        result_row(
            "Skipped",
            "source_changed",
            "stable",
            "source_changed",
            3,
            false,
        ),
        result_row(
            "Skipped",
            "source_unavailable",
            "stable",
            "source_unavailable",
            3,
            false,
        ),
        result_row(
            "Skipped",
            "scene_subtitle_pending",
            "skipped",
            "scene_subtitle_pending",
            0,
            false,
        ),
        result_row("Skipped", "disk_full", "failed", "disk_full", 0, false),
        result_row(
            "Skipped",
            "permission_denied",
            "failed",
            "permission_denied",
            0,
            false,
        ),
        result_row(
            "Skipped",
            "password_required",
            "skipped",
            "password_required",
            0,
            false,
        ),
        result_row(
            "Skipped",
            "archive_extraction_plugin_required",
            "skipped",
            "archive_extraction_plugin_required",
            0,
            false,
        ),
        result_row(
            "Skipped",
            "archive_extraction_timed_out",
            "skipped",
            "archive_extraction_timed_out",
            0,
            false,
        ),
        result_row("Skipped", "-", "skipped", "skipped", 0, false),
        result_row("Rejected", "*", "skipped", "rejected", 0, false),
        result_row("Conflict", "*", "needs_review", "conflict", 0, false),
        result_row("Unmatched", "*", "needs_review", "unmatched", 0, false),
        result_row("Failed", "*", "failed", "error_else_failed", 0, false),
        result_row("ErrCanceled", "-", "stable", "cancelled", 3, true),
        result_row(
            "ErrManualReconciliationRequired",
            "-",
            "failed",
            "manual_reconciliation_required",
            0,
            false,
        ),
        result_row("ErrOther", "-", "failed", "import_io_else_error", 0, true),
    ];

    /// Contract decision 2 fingerprint: `"pf1:" + hex(blake3(folder_id \0
    /// rel_path \0 size \0 mtime_ns))`. The contract does not say how size and
    /// mtime are encoded; this assumes decimal ASCII. A skeleton compares it with
    /// the base implementation and fails loudly if the encodings differ.
    pub(crate) fn expected_fingerprint(
        folder_id: &str,
        relative_path: &str,
        size_bytes: u64,
        mtime_ns: i128,
    ) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(folder_id.as_bytes());
        hasher.update(b"\0");
        hasher.update(relative_path.as_bytes());
        hasher.update(b"\0");
        hasher.update(size_bytes.to_string().as_bytes());
        hasher.update(b"\0");
        hasher.update(mtime_ns.to_string().as_bytes());
        format!("pf1:{}", hasher.finalize().to_hex())
    }

    /// The synthetic `CompletedDownload` contract decision 2 describes for one
    /// stable file, before `trigger_manual_import` adds `*scryer_title_id`.
    pub(crate) fn expected_completed_download(
        folder_id: &str,
        relative_path: &str,
        canonical_path: &Path,
        size_bytes: u64,
        mtime: SystemTime,
        facet: &scryer_domain::MediaFacet,
    ) -> scryer_domain::CompletedDownload {
        let stem = canonical_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_string();
        scryer_domain::CompletedDownload {
            client_type: EXPECTED_CLIENT_TYPE.to_string(),
            client_id: folder_id.to_string(),
            download_client_item_id: expected_fingerprint(
                folder_id,
                relative_path,
                size_bytes,
                mtime_ns(mtime),
            ),
            download_id: None,
            name: stem.clone(),
            release_name: Some(stem),
            dest_dir: canonical_path.to_string_lossy().into_owned(),
            category: None,
            size_bytes: Some(i64::try_from(size_bytes).unwrap_or(i64::MAX)),
            completed_at: Some(chrono::DateTime::<chrono::Utc>::from(mtime)),
            parameters: vec![("*scryer_facet".to_string(), facet.as_str().to_string())],
            posted_file_names: Vec::new(),
            subtitle_paths: Vec::new(),
        }
    }

    // Injected clock.

    /// Fixed start of every fixture clock: 2026-10-01T00:00:00Z.
    pub(crate) const CLOCK_EPOCH_SECS: i64 = 1_790_812_800;

    /// Default `min_age_seconds` from section 3.
    pub(crate) const DEFAULT_MIN_AGE: Duration = Duration::from_secs(300);

    /// A manually advanced clock for stability tests (contract section 8.1:
    /// injected clock, no sleeps). Clones share one instant. File mtimes are
    /// written from the same clock so `now - mtime` is exact.
    #[derive(Clone, Debug)]
    pub(crate) struct TestClock {
        now: Arc<Mutex<chrono::DateTime<chrono::Utc>>>,
    }

    impl TestClock {
        pub(crate) fn new() -> Self {
            Self::at(
                chrono::DateTime::from_timestamp(CLOCK_EPOCH_SECS, 0)
                    .expect("clock epoch is a valid timestamp"),
            )
        }

        pub(crate) fn at(start: chrono::DateTime<chrono::Utc>) -> Self {
            Self {
                now: Arc::new(Mutex::new(start)),
            }
        }

        pub(crate) fn now(&self) -> chrono::DateTime<chrono::Utc> {
            *self.now.lock().expect("clock lock")
        }

        pub(crate) fn advance(&self, by: chrono::Duration) -> chrono::DateTime<chrono::Utc> {
            let mut now = self.now.lock().expect("clock lock");
            *now += by;
            *now
        }

        pub(crate) fn advance_secs(&self, secs: i64) -> chrono::DateTime<chrono::Utc> {
            self.advance(chrono::Duration::seconds(secs))
        }

        pub(crate) fn system_time(&self) -> SystemTime {
            SystemTime::from(self.now())
        }

        /// A `now` source for whatever clock seam the base branch exposes.
        pub(crate) fn source(
            &self,
        ) -> impl Fn() -> chrono::DateTime<chrono::Utc> + Send + Sync + 'static {
            let clock = self.clone();
            move || clock.now()
        }
    }

    impl Default for TestClock {
        fn default() -> Self {
            Self::new()
        }
    }

    pub(crate) fn mtime_ns(time: SystemTime) -> i128 {
        match time.duration_since(UNIX_EPOCH) {
            Ok(after) => after.as_nanos() as i128,
            Err(before) => -(before.duration().as_nanos() as i128),
        }
    }

    // Processed-folder tree builder.

    /// A temp processed-folder tree. Relative paths use `/`.
    pub(crate) struct ProcessedTree {
        dir: tempfile::TempDir,
    }

    impl ProcessedTree {
        pub(crate) fn new() -> Self {
            Self {
                dir: tempfile::tempdir().expect("processed folder tempdir"),
            }
        }

        pub(crate) fn root(&self) -> &Path {
            self.dir.path()
        }

        /// The symlink-resolved root, as validation and the importer see it.
        pub(crate) fn canonical_root(&self) -> PathBuf {
            std::fs::canonicalize(self.root()).expect("canonical processed root")
        }

        pub(crate) fn path(&self, relative: &str) -> PathBuf {
            self.root().join(relative)
        }

        pub(crate) fn mkdir(&self, relative: &str) -> PathBuf {
            let path = self.path(relative);
            std::fs::create_dir_all(&path).expect("create fixture dir");
            path
        }

        /// Writes `bytes` at `relative` (parents created) and sets its mtime.
        pub(crate) fn write_file(
            &self,
            relative: &str,
            bytes: &[u8],
            mtime: SystemTime,
        ) -> PathBuf {
            let path = self.path(relative);
            write_file_at(&path, bytes, mtime);
            path
        }

        /// Writes a synthetic video of exactly `size` bytes with `mtime`.
        pub(crate) fn write_video(&self, relative: &str, size: u64, mtime: SystemTime) -> PathBuf {
            let path = self.path(relative);
            write_synthetic_video(&path, size, mtime);
            path
        }

        pub(crate) fn set_mtime(&self, relative: &str, mtime: SystemTime) {
            set_mtime(&self.path(relative), mtime);
        }

        /// Appends `extra` bytes and sets a new mtime, as a processor still
        /// writing its output would.
        pub(crate) fn grow(&self, relative: &str, extra: u64, mtime: SystemTime) {
            let path = self.path(relative);
            let file = std::fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .expect("open growing fixture");
            let len = file.metadata().expect("stat growing fixture").len();
            file.set_len(len + extra).expect("grow fixture");
            file.set_modified(mtime).expect("set grown mtime");
        }

        pub(crate) fn snapshot(&self) -> TreeSnapshot {
            TreeSnapshot::capture(self.root())
        }
    }

    impl Default for ProcessedTree {
        fn default() -> Self {
            Self::new()
        }
    }

    pub(crate) fn write_file_at(path: &Path, bytes: &[u8], mtime: SystemTime) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create fixture parent");
        }
        let mut file = std::fs::File::create(path).expect("create fixture file");
        file.write_all(bytes).expect("write fixture file");
        file.set_modified(mtime).expect("set fixture mtime");
    }

    pub(crate) fn set_mtime(path: &Path, mtime: SystemTime) {
        std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("open fixture for mtime")
            .set_modified(mtime)
            .expect("set fixture mtime");
    }

    /// Prefix of every synthetic video: an EBML-like id, then a label. Not a
    /// playable file; media probing is overridden in import tests.
    pub(crate) const SYNTHETIC_VIDEO_HEADER: &[u8] =
        b"\x1a\x45\xdf\xa3scryer-pr1-synthetic-video\n";

    /// Bytes past this are left sparse so large sizes cost nothing.
    const SYNTHETIC_DENSE_BYTES: u64 = 64 * 1024;

    /// Deterministic content for a file name, so two different fixtures never
    /// share bytes by accident and one fixture always has the same bytes.
    pub(crate) fn synthetic_video_bytes(path: &Path, size: u64) -> Vec<u8> {
        let dense = size.min(SYNTHETIC_DENSE_BYTES) as usize;
        let mut bytes = Vec::with_capacity(dense);
        bytes.extend_from_slice(&SYNTHETIC_VIDEO_HEADER[..SYNTHETIC_VIDEO_HEADER.len().min(dense)]);
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut stream = blake3::Hasher::new_derive_key("scryer pr1 synthetic video")
            .update(name.as_bytes())
            .finalize_xof();
        let mut rest = vec![0u8; dense - bytes.len()];
        stream.fill(&mut rest);
        bytes.extend_from_slice(&rest);
        bytes
    }

    pub(crate) fn write_synthetic_video(path: &Path, size: u64, mtime: SystemTime) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create video parent");
        }
        let mut file = std::fs::File::create(path).expect("create synthetic video");
        file.write_all(&synthetic_video_bytes(path, size))
            .expect("write synthetic video");
        file.set_len(size).expect("size synthetic video");
        file.set_modified(mtime).expect("set synthetic video mtime");
    }

    /// Truncates and rewrites a file through the same inode, as a processor
    /// that rewrites its output in place does (contract Q2).
    pub(crate) fn rewrite_in_place(path: &Path, bytes: &[u8]) {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(path)
            .expect("open for in-place rewrite");
        file.write_all(bytes).expect("rewrite in place");
        file.sync_all().expect("sync in-place rewrite");
    }

    pub(crate) fn file_blake3(path: &Path) -> String {
        let mut hasher = blake3::Hasher::new();
        let mut file = std::fs::File::open(path).expect("open file to hash");
        std::io::copy(&mut file, &mut hasher).expect("hash file");
        hasher.finalize().to_hex().to_string()
    }

    // Tree snapshot: "byte-identical before and after".

    /// One path in a [`TreeSnapshot`]. Symlinks are recorded, never followed.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(crate) enum TreeNode {
        Dir {
            mode: u32,
        },
        File {
            len: u64,
            mtime_ns: i128,
            blake3: String,
            mode: u32,
            /// Inode and link count on Unix: a new hard link to a source raises
            /// `nlink`, a rename-and-replace changes `ino`.
            ino: u64,
            nlink: u64,
        },
        Symlink {
            target: PathBuf,
        },
    }

    /// Every path under a root with kind, content hash, size, mtime, mode and
    /// (Unix) inode and link count. Equal snapshots mean the tree was not
    /// written, renamed, linked, chmod'ed or deleted in any observable way.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(crate) struct TreeSnapshot {
        pub(crate) nodes: BTreeMap<String, TreeNode>,
    }

    impl TreeSnapshot {
        pub(crate) fn capture(root: &Path) -> Self {
            let mut nodes = BTreeMap::new();
            let mut pending = vec![root.to_path_buf()];
            while let Some(dir) = pending.pop() {
                let mut entries: Vec<_> = std::fs::read_dir(&dir)
                    .expect("read snapshot dir")
                    .map(|entry| entry.expect("snapshot dir entry").path())
                    .collect();
                entries.sort();
                for path in entries {
                    let relative = path
                        .strip_prefix(root)
                        .expect("snapshot path under root")
                        .components()
                        .map(|part| part.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/");
                    let metadata = std::fs::symlink_metadata(&path).expect("snapshot lstat");
                    let node = if metadata.file_type().is_symlink() {
                        TreeNode::Symlink {
                            target: std::fs::read_link(&path).expect("read snapshot link"),
                        }
                    } else if metadata.is_dir() {
                        pending.push(path.clone());
                        TreeNode::Dir {
                            mode: unix_mode(&metadata),
                        }
                    } else {
                        let (ino, nlink) = unix_ino_nlink(&metadata);
                        TreeNode::File {
                            len: metadata.len(),
                            mtime_ns: mtime_ns(metadata.modified().expect("snapshot mtime")),
                            blake3: file_blake3(&path),
                            mode: unix_mode(&metadata),
                            ino,
                            nlink,
                        }
                    };
                    nodes.insert(relative, node);
                }
            }
            Self { nodes }
        }

        /// One combined hash of the snapshot, for compact logging.
        pub(crate) fn digest(&self) -> String {
            let mut hasher = blake3::Hasher::new();
            for (path, node) in &self.nodes {
                hasher.update(format!("{path}\0{node:?}\n").as_bytes());
            }
            hasher.finalize().to_hex().to_string()
        }

        /// Paths whose node differs, appeared or disappeared.
        pub(crate) fn diff(&self, after: &Self) -> Vec<String> {
            let mut changed = Vec::new();
            for (path, node) in &self.nodes {
                match after.nodes.get(path) {
                    None => changed.push(format!("removed {path}")),
                    Some(other) if other != node => {
                        changed.push(format!("changed {path}: {node:?} -> {other:?}"))
                    }
                    Some(_) => {}
                }
            }
            for path in after.nodes.keys() {
                if !self.nodes.contains_key(path) {
                    changed.push(format!("added {path}"));
                }
            }
            changed
        }

        #[track_caller]
        pub(crate) fn assert_unchanged(&self, after: &Self, what: &str) {
            let changed = self.diff(after);
            assert!(changed.is_empty(), "{what}: tree changed: {changed:#?}");
        }
    }

    #[cfg(unix)]
    fn unix_mode(metadata: &std::fs::Metadata) -> u32 {
        use std::os::unix::fs::PermissionsExt as _;
        metadata.permissions().mode()
    }

    #[cfg(not(unix))]
    fn unix_mode(metadata: &std::fs::Metadata) -> u32 {
        u32::from(metadata.permissions().readonly())
    }

    #[cfg(unix)]
    fn unix_ino_nlink(metadata: &std::fs::Metadata) -> (u64, u64) {
        use std::os::unix::fs::MetadataExt as _;
        (metadata.ino(), metadata.nlink())
    }

    #[cfg(not(unix))]
    fn unix_ino_nlink(_metadata: &std::fs::Metadata) -> (u64, u64) {
        (0, 0)
    }

    /// `(dev, ino)` of a path, following symlinks.
    #[cfg(unix)]
    pub(crate) fn inode(path: &Path) -> (u64, u64) {
        use std::os::unix::fs::MetadataExt as _;
        let metadata = std::fs::metadata(path).expect("stat for inode");
        (metadata.dev(), metadata.ino())
    }

    #[cfg(unix)]
    pub(crate) fn nlink(path: &Path) -> u64 {
        use std::os::unix::fs::MetadataExt as _;
        std::fs::metadata(path).expect("stat for nlink").nlink()
    }

    // Symlink fixtures (Unix).

    /// `link` (inside a processed tree) pointing at `target`.
    #[cfg(unix)]
    pub(crate) fn symlink(target: &Path, link: &Path) {
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent).expect("create symlink parent");
        }
        std::os::unix::fs::symlink(target, link).expect("create fixture symlink");
    }

    /// The section 7.4 and 8.5 symlink shapes inside a tree.
    #[cfg(unix)]
    pub(crate) struct SymlinkLayout {
        /// File symlink whose target is outside the root:
        /// `rejected/symlink_escapes_root`.
        pub(crate) escaping_file: String,
        /// Symlinked directory: `rejected/symlink_dir`, contents never scanned.
        pub(crate) symlinked_dir: String,
        /// A video only reachable through `symlinked_dir`.
        pub(crate) behind_symlinked_dir: String,
        /// File symlink to a video inside the root: imported by copying the
        /// resolved target.
        pub(crate) in_root_link: String,
        /// The in-root target of `in_root_link`.
        pub(crate) in_root_target: String,
    }

    /// Builds [`SymlinkLayout`] in `tree`, with outside targets in `outside`.
    #[cfg(unix)]
    pub(crate) fn build_symlink_layout(
        tree: &ProcessedTree,
        outside: &Path,
        mtime: SystemTime,
    ) -> SymlinkLayout {
        let outside_video = outside.join("Outside.Movie.2020.1080p.mkv");
        write_synthetic_video(&outside_video, 8192, mtime);
        let outside_dir = outside.join("outside-dir");
        write_synthetic_video(
            &outside_dir.join("Hidden.Behind.Link.2019.mkv"),
            8192,
            mtime,
        );

        let layout = SymlinkLayout {
            escaping_file: "links/Escaping.Movie.2020.1080p.mkv".to_string(),
            symlinked_dir: "linked-dir".to_string(),
            behind_symlinked_dir: "linked-dir/Hidden.Behind.Link.2019.mkv".to_string(),
            in_root_link: "links/Inside.Link.2021.1080p.mkv".to_string(),
            in_root_target: "real/Inside.Target.2021.1080p.mkv".to_string(),
        };
        symlink(&outside_video, &tree.path(&layout.escaping_file));
        symlink(&outside_dir, &tree.path(&layout.symlinked_dir));
        tree.write_video(&layout.in_root_target, 8192, mtime);
        symlink(
            &tree.path(&layout.in_root_target),
            &tree.path(&layout.in_root_link),
        );
        layout
    }

    // Processor output layouts.

    /// What contract sections 2.2 and 7.5 say a scan does with a file.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Expected {
        /// A finished video: observed, then stable, then imported.
        Importable,
        /// Ends in a video extension but is still being written; only stability
        /// (scan count plus age, or a missing marker) keeps it out.
        InProgressVideo,
        /// Temp suffix from section 7.5: ignored, no ledger row.
        IgnoredSuffix,
        /// Name starting with `.` or `~$`, or inside such a directory: ignored.
        IgnoredHidden,
        /// A marker file: never a row, never touched.
        Marker,
        /// Not a video. Section 7.5 says ignored; sections 2.1 and 2.2 say
        /// `rejected/not_a_video`. Tests assert "never imported" until resolved.
        NotVideo,
        /// Zero bytes. Same open question as `NotVideo` (`rejected/empty`).
        Empty,
    }

    /// Plain generated files shaped loosely like a processor's output folder.
    /// They are not produced by those tools and make no claim about their exact
    /// naming; they only exercise the section 7.5 rules on realistic shapes.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum ProcessorLayout {
        FileFlows,
        Tdarr,
        Unmanic,
        CustomFfmpeg,
    }

    pub(crate) const ALL_LAYOUTS: &[ProcessorLayout] = &[
        ProcessorLayout::FileFlows,
        ProcessorLayout::Tdarr,
        ProcessorLayout::Unmanic,
        ProcessorLayout::CustomFfmpeg,
    ];

    /// Marker settings a layout is meant to be scanned with (section 3
    /// `marker_mode` and `marker_value`).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum MarkerFixture {
        None,
        /// `<file name><suffix>` next to the video. The contract does not say
        /// whether the suffix follows the full name or the stem; this assumes
        /// the full name.
        SiblingSuffix(&'static str),
        DirectoryFile(&'static str),
    }

    #[derive(Clone, Copy, Debug)]
    pub(crate) struct LayoutFile {
        pub(crate) relative: &'static str,
        pub(crate) size: u64,
        pub(crate) expected: Expected,
    }

    const KIB: u64 = 1024;

    const fn file(relative: &'static str, size: u64, expected: Expected) -> LayoutFile {
        LayoutFile {
            relative,
            size,
            expected,
        }
    }

    const FILEFLOWS: &[LayoutFile] = &[
        file(
            "Movies/Synthetic Movie (2024)/Synthetic Movie (2024).mkv",
            48 * KIB,
            Expected::Importable,
        ),
        file(
            "Movies/Synthetic Movie (2024)/Synthetic Movie (2024).nfo",
            KIB,
            Expected::NotVideo,
        ),
        file(
            "Movies/Second Movie (2023)/Second Movie (2023).mkv.tmp",
            16 * KIB,
            Expected::IgnoredSuffix,
        ),
        file(
            "Movies/Third Movie (2022)/Third Movie (2022).mp4.part",
            16 * KIB,
            Expected::IgnoredSuffix,
        ),
    ];

    const TDARR: &[LayoutFile] = &[
        file(
            "Synthetic Show/Season 01/Synthetic.Show.S01E01.1080p.WEB-DL.mkv",
            40 * KIB,
            Expected::Importable,
        ),
        file(
            "Synthetic Show/Season 01/Synthetic.Show.S01E02.1080p.WEB-DL-TdarrCacheFile-a1b2c3.mkv",
            12 * KIB,
            Expected::InProgressVideo,
        ),
        file(
            "Synthetic Show/Season 01/Synthetic.Show.S01E03.1080p.WEB-DL.mkv.partial",
            12 * KIB,
            Expected::IgnoredSuffix,
        ),
        file(
            "Synthetic Show/Season 01/Synthetic.Show.S01E01.en.srt",
            KIB,
            Expected::NotVideo,
        ),
    ];

    const UNMANIC: &[LayoutFile] = &[
        file(
            "Synthetic.Film.2021.1080p.BluRay.mkv",
            40 * KIB,
            Expected::Importable,
        ),
        file(
            "Synthetic.Film.2021.1080p.BluRay.mkv.done",
            0,
            Expected::Marker,
        ),
        file(
            "Synthetic.Film.2020.720p.WEB.mkv",
            24 * KIB,
            Expected::InProgressVideo,
        ),
        file("~$Synthetic.Film.lock", KIB, Expected::IgnoredHidden),
        file(
            ".unmanic-cache/Synthetic.Film.2021.1080p.BluRay.mkv",
            8 * KIB,
            Expected::IgnoredHidden,
        ),
        file(
            "Synthetic.Film.2019.mkv.!qB",
            8 * KIB,
            Expected::IgnoredSuffix,
        ),
    ];

    const CUSTOM_FFMPEG: &[LayoutFile] = &[
        file(
            "out/batch-01/Synthetic.Feature.2019.2160p.mkv",
            56 * KIB,
            Expected::Importable,
        ),
        file("out/batch-01/_DONE", 0, Expected::Marker),
        file(
            "out/batch-02/Synthetic.Feature.2018.1080p.mkv",
            20 * KIB,
            Expected::InProgressVideo,
        ),
        file(
            "out/batch-02/Synthetic.Feature.2018.1080p.mkv.filepart",
            8 * KIB,
            Expected::IgnoredSuffix,
        ),
        file(
            "out/batch-02/Synthetic.Feature.2017.mkv.crdownload",
            8 * KIB,
            Expected::IgnoredSuffix,
        ),
        file(
            "out/batch-02/Synthetic.Feature.2016.mkv.tmp_import",
            8 * KIB,
            Expected::IgnoredSuffix,
        ),
        file(
            "out/batch-02/Synthetic.Feature.2015.mkv",
            0,
            Expected::Empty,
        ),
        file("out/batch-02/ffmpeg2pass-0.log", KIB, Expected::NotVideo),
        file(
            "out/batch-02/.Synthetic.Feature.2014.mkv",
            8 * KIB,
            Expected::IgnoredHidden,
        ),
    ];

    impl ProcessorLayout {
        pub(crate) fn marker(self) -> MarkerFixture {
            match self {
                Self::FileFlows | Self::Tdarr => MarkerFixture::None,
                Self::Unmanic => MarkerFixture::SiblingSuffix(".done"),
                Self::CustomFfmpeg => MarkerFixture::DirectoryFile("_DONE"),
            }
        }

        pub(crate) fn files(self) -> &'static [LayoutFile] {
            match self {
                Self::FileFlows => FILEFLOWS,
                Self::Tdarr => TDARR,
                Self::Unmanic => UNMANIC,
                Self::CustomFfmpeg => CUSTOM_FFMPEG,
            }
        }

        /// Writes the layout into `tree`, every file with `mtime`.
        pub(crate) fn materialize(self, tree: &ProcessedTree, mtime: SystemTime) {
            for file in self.files() {
                if file.size == 0 {
                    tree.write_file(file.relative, b"", mtime);
                } else if has_video_extension(file.relative) {
                    tree.write_video(file.relative, file.size, mtime);
                } else {
                    tree.write_file(file.relative, &vec![b'x'; file.size as usize], mtime);
                }
            }
        }

        pub(crate) fn relatives(self, expected: Expected) -> Vec<&'static str> {
            self.files()
                .iter()
                .filter(|file| file.expected == expected)
                .map(|file| file.relative)
                .collect()
        }
    }

    pub(crate) fn has_video_extension(relative: &str) -> bool {
        Path::new(relative)
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                scryer_domain::VIDEO_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
            })
    }

    pub(crate) fn has_ignored_suffix(relative: &str) -> bool {
        IGNORED_SUFFIXES
            .iter()
            .any(|suffix| relative.ends_with(suffix))
    }

    pub(crate) fn has_ignored_component(relative: &str) -> bool {
        relative.split('/').any(|part| {
            IGNORED_PREFIXES
                .iter()
                .any(|prefix| part.starts_with(prefix))
        })
    }

    // FileImporter doubles.

    fn import_result(
        strategy: scryer_domain::ImportStrategy,
        source: &Path,
        dest: &Path,
        source_cleanup: Option<scryer_domain::ImportSourceCleanupGuard>,
    ) -> scryer_domain::ImportFileResult {
        scryer_domain::ImportFileResult {
            strategy,
            source_path: source.to_path_buf(),
            dest_path: dest.to_path_buf(),
            size_bytes: std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0),
            destination_disposition: scryer_domain::ImportDestinationDisposition::Created,
            source_disposition: scryer_domain::ImportSourceDisposition::Retained,
            source_cleanup,
            verification: None,
        }
    }

    fn ensure_parent(dest: &Path) -> AppResult<()> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                AppError::Repository(format!(
                    "failed to create fixture destination {}: {error}",
                    parent.display()
                ))
            })?;
        }
        Ok(())
    }

    fn copy_bytes(source: &Path, dest: &Path) -> AppResult<()> {
        ensure_parent(dest)?;
        std::fs::copy(source, dest).map(|_| ()).map_err(|error| {
            AppError::Repository(format!(
                "fixture copy {} to {} failed: {error}",
                source.display(),
                dest.display()
            ))
        })
    }

    /// Call counters every double exposes.
    #[derive(Debug, Default)]
    pub(crate) struct ImporterCalls {
        imports: AtomicUsize,
        source_removals: AtomicUsize,
        modes: Mutex<Vec<scryer_domain::ImportMode>>,
    }

    impl ImporterCalls {
        fn record(&self, mode: scryer_domain::ImportMode) {
            self.imports.fetch_add(1, Ordering::SeqCst);
            self.modes.lock().expect("modes lock").push(mode);
        }

        pub(crate) fn imports(&self) -> usize {
            self.imports.load(Ordering::SeqCst)
        }

        /// Calls of `remove_import_source_after_verified_import`. No double
        /// deletes anything; a non-zero count is itself the failure signal for
        /// the copy-only belts (C5).
        pub(crate) fn source_removals(&self) -> usize {
            self.source_removals.load(Ordering::SeqCst)
        }

        /// The `ImportMode` of every `import_file` call, for the C1 assertion
        /// that a processed-folder import never asks for `Move` (C1 forces
        /// `HardlinkOrCopy`; copy-only itself travels in the execution context,
        /// which the `FileImporter` default methods drop before reaching a
        /// double).
        pub(crate) fn modes(&self) -> Vec<scryer_domain::ImportMode> {
            self.modes.lock().expect("modes lock").clone()
        }
    }

    /// Ignores the copy-only policy and always hard links (contract Q6: C4 must
    /// turn its result into `ManualReconciliationRequired`).
    #[derive(Debug, Default)]
    pub(crate) struct AlwaysHardLinkImporter {
        pub(crate) calls: ImporterCalls,
    }

    #[async_trait::async_trait]
    impl FileImporter for AlwaysHardLinkImporter {
        async fn snapshot_import_source(
            &self,
            source: &Path,
        ) -> AppResult<scryer_domain::ImportSourceSnapshot> {
            test_import_source_snapshot(source)
        }

        async fn import_file(
            &self,
            source: &Path,
            dest: &Path,
            mode: scryer_domain::ImportMode,
            _expected_source: Option<&scryer_domain::ImportSourceSnapshot>,
        ) -> AppResult<scryer_domain::ImportFileResult> {
            self.calls.record(mode);
            ensure_parent(dest)?;
            std::fs::hard_link(source, dest).map_err(|error| {
                AppError::Repository(format!("fixture hard link failed: {error}"))
            })?;
            Ok(import_result(
                scryer_domain::ImportStrategy::HardLink,
                source,
                dest,
                None,
            ))
        }

        async fn remove_import_source_after_verified_import(
            &self,
            _guard: scryer_domain::ImportSourceCleanupGuard,
            _final_dest_path: &Path,
        ) -> AppResult<()> {
            self.calls.source_removals.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    /// Copies but hands back a source-cleanup guard (contract C4 second leg:
    /// `source_cleanup.is_some()` must be refused even with strategy `Copy`).
    #[derive(Debug, Default)]
    pub(crate) struct CopyWithCleanupGuardImporter {
        pub(crate) calls: ImporterCalls,
    }

    #[async_trait::async_trait]
    impl FileImporter for CopyWithCleanupGuardImporter {
        async fn snapshot_import_source(
            &self,
            source: &Path,
        ) -> AppResult<scryer_domain::ImportSourceSnapshot> {
            test_import_source_snapshot(source)
        }

        async fn import_file(
            &self,
            source: &Path,
            dest: &Path,
            mode: scryer_domain::ImportMode,
            _expected_source: Option<&scryer_domain::ImportSourceSnapshot>,
        ) -> AppResult<scryer_domain::ImportFileResult> {
            self.calls.record(mode);
            copy_bytes(source, dest)?;
            let guard = fabricated_cleanup_guard(source, dest)?;
            Ok(import_result(
                scryer_domain::ImportStrategy::Copy,
                source,
                dest,
                Some(guard),
            ))
        }

        async fn remove_import_source_after_verified_import(
            &self,
            _guard: scryer_domain::ImportSourceCleanupGuard,
            _final_dest_path: &Path,
        ) -> AppResult<()> {
            self.calls.source_removals.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    /// A cleanup guard for `source` and `dest` built from their current bytes,
    /// as a buggy importer would hand back (contract Q7 belts).
    pub(crate) fn fabricated_cleanup_guard(
        source: &Path,
        dest: &Path,
    ) -> AppResult<scryer_domain::ImportSourceCleanupGuard> {
        let source_snapshot = test_import_source_snapshot(source)?;
        let dest_snapshot = test_import_source_snapshot(dest)?;
        Ok(scryer_domain::ImportSourceCleanupGuard {
            source_path: source.to_path_buf(),
            dest_path: dest.to_path_buf(),
            size_bytes: source_snapshot.proof.size_bytes,
            source_identity: source_snapshot.identity,
            source_proof: source_snapshot.proof,
            dest_proof: dest_snapshot.proof,
        })
    }

    /// One scripted outcome of [`ScriptedImporter`].
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(crate) enum ScriptedOutcome {
        /// Copy the bytes and report `Copy`.
        Copy,
        /// Fail without writing anything; the next call may succeed
        /// (ledger-level retry, contract Q4a at app level).
        TransientFailure,
        /// Fail without writing anything (contract Q4b at app level).
        PermanentFailure,
        /// Change the source after the pipeline snapshotted it, then refuse the
        /// import as the real importer does (contract Q4e at app level).
        SourceChangedMidCopy,
    }

    /// Plays a script of outcomes, one per `import_file` call, then copies.
    /// Copy-lane internals (temp files, fsync, verify, fault hooks) belong to
    /// `FsFileImporter` and are tested there and in the integration file.
    #[derive(Debug, Default)]
    pub(crate) struct ScriptedImporter {
        script: Mutex<VecDeque<ScriptedOutcome>>,
        pub(crate) calls: ImporterCalls,
    }

    impl ScriptedImporter {
        pub(crate) fn new(script: impl IntoIterator<Item = ScriptedOutcome>) -> Self {
            Self {
                script: Mutex::new(script.into_iter().collect()),
                calls: ImporterCalls::default(),
            }
        }

        pub(crate) fn remaining(&self) -> usize {
            self.script.lock().expect("script lock").len()
        }
    }

    #[async_trait::async_trait]
    impl FileImporter for ScriptedImporter {
        async fn snapshot_import_source(
            &self,
            source: &Path,
        ) -> AppResult<scryer_domain::ImportSourceSnapshot> {
            test_import_source_snapshot(source)
        }

        async fn import_file(
            &self,
            source: &Path,
            dest: &Path,
            mode: scryer_domain::ImportMode,
            expected_source: Option<&scryer_domain::ImportSourceSnapshot>,
        ) -> AppResult<scryer_domain::ImportFileResult> {
            self.calls.record(mode);
            let outcome = self
                .script
                .lock()
                .expect("script lock")
                .pop_front()
                .unwrap_or(ScriptedOutcome::Copy);
            match outcome {
                ScriptedOutcome::Copy => {
                    if let Some(expected) = expected_source
                        && &test_import_source_snapshot(source)? != expected
                    {
                        return Err(AppError::Repository(format!(
                            "import source changed after validation: {}",
                            source.display()
                        )));
                    }
                    copy_bytes(source, dest)?;
                    Ok(import_result(
                        scryer_domain::ImportStrategy::Copy,
                        source,
                        dest,
                        None,
                    ))
                }
                ScriptedOutcome::TransientFailure => Err(AppError::Repository(
                    "synthetic transient copy failure".to_string(),
                )),
                ScriptedOutcome::PermanentFailure => Err(AppError::Repository(
                    "synthetic permanent copy failure".to_string(),
                )),
                ScriptedOutcome::SourceChangedMidCopy => {
                    let mut file = std::fs::OpenOptions::new()
                        .append(true)
                        .open(source)
                        .map_err(|error| AppError::Repository(error.to_string()))?;
                    file.write_all(b"grown")
                        .map_err(|error| AppError::Repository(error.to_string()))?;
                    Err(AppError::Repository(format!(
                        "import source changed after validation: {}",
                        source.display()
                    )))
                }
            }
        }

        async fn remove_import_source_after_verified_import(
            &self,
            _guard: scryer_domain::ImportSourceCleanupGuard,
            _final_dest_path: &Path,
        ) -> AppResult<()> {
            self.calls.source_removals.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    // Library and title seeding.

    /// Points the default library of `facet` at `root` as its only root and
    /// returns the library. Mirrors `register_import_fixture_root` in
    /// `downloads.rs`, which is private to that module.
    pub(crate) async fn point_default_library_at(
        app: &crate::AppUseCase,
        facet: scryer_domain::MediaFacet,
        root: &Path,
    ) -> scryer_domain::Library {
        let library = app
            .services
            .catalog
            .libraries
            .default_for_facet(facet)
            .await
            .expect("library lookup")
            .expect("fixture library exists");
        app.services
            .catalog
            .libraries
            .update(
                &library.id,
                library.name.clone(),
                library.slug.clone(),
                vec![crate::LibraryRootDraft {
                    id: None,
                    path: root.to_string_lossy().into_owned(),
                    is_default: true,
                }],
            )
            .await
            .expect("point fixture library at root")
    }

    /// Adds a title through the public use case.
    pub(crate) async fn add_title(
        app: &crate::AppUseCase,
        actor: &scryer_domain::User,
        name: &str,
        facet: scryer_domain::MediaFacet,
        monitored: bool,
    ) -> scryer_domain::Title {
        app.add_title(
            actor,
            scryer_domain::NewTitle {
                name: name.to_string(),
                facet,
                monitored,
                tags: vec![],
                external_ids: vec![],
                min_availability: None,
                ..Default::default()
            },
        )
        .await
        .unwrap_or_else(|error| panic!("add fixture title {name}: {error}"))
    }

    /// A user holding exactly `permissions` at app level and full library
    /// permissions, so a denial can only come from the app-level check.
    pub(crate) fn user_with(permissions: &[scryer_domain::AppPermission]) -> scryer_domain::User {
        let mut user = scryer_domain::User::new_admin("processed-folder-fixture");
        user.authorization.app =
            scryer_domain::AppPermissionMask::from_permissions(permissions.iter().copied());
        user
    }

    /// Reads a source file under this crate, for hygiene checks.
    pub(crate) fn crate_source(relative: &str) -> Option<String> {
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)).ok()
    }
}

use fixtures::*;

/// Proves the fixtures. These run today.
mod fixture_self_tests {
    use super::*;
    use std::time::Duration as StdDuration;

    #[test]
    fn the_clock_only_moves_when_told_and_clones_share_it() {
        let clock = TestClock::new();
        let start = clock.now();
        let shared = clock.clone();
        assert_eq!(clock.now(), start, "no wall clock involved");
        shared.advance_secs(299);
        assert_eq!(clock.now() - start, chrono::Duration::seconds(299));
        let source = clock.source();
        clock.advance_secs(1);
        assert_eq!(source() - start, chrono::Duration::seconds(300));
        assert_eq!(
            mtime_ns(clock.system_time()),
            i128::from(clock.now().timestamp()) * 1_000_000_000
        );
        assert_eq!(DEFAULT_MIN_AGE.as_secs(), 300);
    }

    #[test]
    fn a_synthetic_video_has_the_exact_size_mtime_and_stable_bytes() {
        let clock = TestClock::new();
        let tree = ProcessedTree::new();
        let big = tree.write_video(
            "a/Big.Synthetic.2024.mkv",
            3 * 1024 * 1024,
            clock.system_time(),
        );
        let small = tree.write_video("Small.Synthetic.2024.mkv", 10, clock.system_time());
        let metadata = std::fs::metadata(&big).unwrap();
        assert_eq!(metadata.len(), 3 * 1024 * 1024);
        assert_eq!(metadata.modified().unwrap(), clock.system_time());
        assert_eq!(std::fs::metadata(&small).unwrap().len(), 10);

        let again = ProcessedTree::new();
        let twin = again.write_video(
            "a/Big.Synthetic.2024.mkv",
            3 * 1024 * 1024,
            clock.system_time(),
        );
        assert_eq!(
            file_blake3(&big),
            file_blake3(&twin),
            "same name, same bytes"
        );
        let other = again.write_video(
            "a/Other.Synthetic.2024.mkv",
            3 * 1024 * 1024,
            clock.system_time(),
        );
        assert_ne!(
            file_blake3(&big),
            file_blake3(&other),
            "different name, different bytes"
        );
        assert!(
            std::fs::read(&big)
                .unwrap()
                .starts_with(SYNTHETIC_VIDEO_HEADER)
        );
    }

    #[test]
    fn growing_a_file_changes_size_and_mtime() {
        let clock = TestClock::new();
        let tree = ProcessedTree::new();
        let path = tree.write_video("Growing.2024.mkv", 4096, clock.system_time());
        clock.advance_secs(30);
        tree.grow("Growing.2024.mkv", 1024, clock.system_time());
        let metadata = std::fs::metadata(&path).unwrap();
        assert_eq!(metadata.len(), 5120);
        assert_eq!(metadata.modified().unwrap(), clock.system_time());
    }

    #[test]
    fn an_untouched_tree_snapshots_equal() {
        let clock = TestClock::new();
        let tree = ProcessedTree::new();
        ProcessorLayout::CustomFfmpeg.materialize(&tree, clock.system_time());
        let before = tree.snapshot();
        let after = tree.snapshot();
        before.assert_unchanged(&after, "two reads of the same tree");
        assert_eq!(before.digest(), after.digest());
        assert!(before.nodes.contains_key("out/batch-01/_DONE"));
        assert!(matches!(
            before.nodes.get("out"),
            Some(TreeNode::Dir { .. })
        ));
    }

    #[test]
    fn a_same_size_rewrite_with_the_mtime_put_back_is_still_detected() {
        let clock = TestClock::new();
        let tree = ProcessedTree::new();
        let path = tree.write_file("Same.Size.mkv", b"aaaa", clock.system_time());
        let before = tree.snapshot();
        rewrite_in_place(&path, b"bbbb");
        set_mtime(&path, clock.system_time());
        let changed = before.diff(&tree.snapshot());
        assert_eq!(changed.len(), 1, "{changed:#?}");
        assert!(
            changed[0].starts_with("changed Same.Size.mkv"),
            "{changed:#?}"
        );
    }

    #[test]
    fn added_and_removed_paths_and_mtime_changes_are_detected() {
        let clock = TestClock::new();
        let tree = ProcessedTree::new();
        tree.write_file("keep.mkv", b"k", clock.system_time());
        tree.write_file("gone.mkv", b"g", clock.system_time());
        let before = tree.snapshot();
        std::fs::remove_file(tree.path("gone.mkv")).unwrap();
        tree.write_file("new.mkv", b"n", clock.system_time());
        tree.set_mtime("keep.mkv", clock.system_time() + StdDuration::from_secs(1));
        let mut changed = before.diff(&tree.snapshot());
        changed.sort();
        assert_eq!(changed.len(), 3, "{changed:#?}");
        assert!(changed[0].starts_with("added new.mkv"));
        assert!(changed[1].starts_with("changed keep.mkv"));
        assert!(changed[2].starts_with("removed gone.mkv"));
    }

    #[cfg(unix)]
    #[test]
    fn a_new_hard_link_to_a_source_changes_its_snapshot() {
        let clock = TestClock::new();
        let tree = ProcessedTree::new();
        let library = tempfile::tempdir().unwrap();
        let source = tree.write_video("Linked.2024.mkv", 4096, clock.system_time());
        let before = tree.snapshot();
        std::fs::hard_link(&source, library.path().join("Linked.2024.mkv")).unwrap();
        let changed = before.diff(&tree.snapshot());
        assert_eq!(changed.len(), 1, "{changed:#?}");
        assert!(changed[0].contains("nlink: 2"), "{changed:#?}");
    }

    #[cfg(unix)]
    #[test]
    fn an_in_place_rewrite_keeps_the_inode_and_reaches_every_hard_link() {
        let clock = TestClock::new();
        let tree = ProcessedTree::new();
        let library = tempfile::tempdir().unwrap();
        let source = tree.write_video("Rewritten.2024.mkv", 4096, clock.system_time());
        let linked = library.path().join("Rewritten.2024.mkv");
        let copied = library.path().join("Copied.2024.mkv");
        std::fs::hard_link(&source, &linked).unwrap();
        std::fs::copy(&source, &copied).unwrap();
        let copied_hash = file_blake3(&copied);
        let inode_before = inode(&source);

        rewrite_in_place(&source, b"processor rewrote this output");

        assert_eq!(inode(&source), inode_before, "same inode after rewrite");
        assert_eq!(
            std::fs::read(&linked).unwrap(),
            b"processor rewrote this output",
            "a hard-linked library file follows the rewrite: why Q2 needs a copy"
        );
        assert_eq!(file_blake3(&copied), copied_hash, "a copy does not");
    }

    #[cfg(unix)]
    #[test]
    fn the_symlink_layout_has_the_shapes_section_7_4_names() {
        let clock = TestClock::new();
        let tree = ProcessedTree::new();
        let outside = tempfile::tempdir().unwrap();
        let layout = build_symlink_layout(&tree, outside.path(), clock.system_time());
        let root = tree.canonical_root();

        let escaping = std::fs::canonicalize(tree.path(&layout.escaping_file)).unwrap();
        assert!(
            !escaping.starts_with(&root),
            "escaping link resolves outside root"
        );
        assert!(
            std::fs::symlink_metadata(tree.path(&layout.symlinked_dir))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(
            tree.path(&layout.behind_symlinked_dir).exists(),
            "reachable only by following"
        );
        let inside = std::fs::canonicalize(tree.path(&layout.in_root_link)).unwrap();
        assert!(inside.starts_with(&root));
        assert_eq!(
            inside,
            std::fs::canonicalize(tree.path(&layout.in_root_target)).unwrap()
        );

        let snapshot = tree.snapshot();
        assert!(matches!(
            snapshot.nodes.get(&layout.symlinked_dir),
            Some(TreeNode::Symlink { .. })
        ));
        assert!(
            !snapshot.nodes.contains_key(&layout.behind_symlinked_dir),
            "the snapshot never follows a symlinked dir"
        );
    }

    #[test]
    fn every_layout_file_obeys_the_section_7_5_rules_it_is_meant_to_test() {
        let clock = TestClock::new();
        for layout in ALL_LAYOUTS {
            let tree = ProcessedTree::new();
            layout.materialize(&tree, clock.system_time());
            for file in layout.files() {
                let path = tree.path(file.relative);
                let metadata = std::fs::metadata(&path)
                    .unwrap_or_else(|error| panic!("{layout:?} {}: {error}", file.relative));
                assert_eq!(metadata.len(), file.size, "{layout:?} {}", file.relative);
                assert_eq!(metadata.modified().unwrap(), clock.system_time());
                let video = has_video_extension(file.relative);
                let suffix = has_ignored_suffix(file.relative);
                let hidden = has_ignored_component(file.relative);
                let name = file.relative;
                match file.expected {
                    Expected::Importable | Expected::InProgressVideo => {
                        assert!(video && !suffix && !hidden && file.size > 0, "{name}")
                    }
                    Expected::IgnoredSuffix => assert!(suffix && !hidden, "{name}"),
                    Expected::IgnoredHidden => assert!(hidden, "{name}"),
                    Expected::NotVideo => assert!(!video && !suffix && !hidden, "{name}"),
                    Expected::Empty => assert!(video && file.size == 0, "{name}"),
                    Expected::Marker => match layout.marker() {
                        MarkerFixture::SiblingSuffix(marker) => {
                            let target = name.strip_suffix(marker).expect("sibling marker");
                            assert!(tree.path(target).exists(), "{name}");
                        }
                        MarkerFixture::DirectoryFile(marker) => {
                            assert!(name.ends_with(&format!("/{marker}")), "{name}");
                        }
                        MarkerFixture::None => {
                            panic!("{layout:?} has a marker but no marker mode")
                        }
                    },
                }
            }
            assert!(
                !layout.relatives(Expected::Importable).is_empty(),
                "{layout:?}"
            );
        }
    }

    #[test]
    fn the_layouts_together_cover_every_ignored_suffix_and_prefix() {
        let all: Vec<&str> = ALL_LAYOUTS
            .iter()
            .flat_map(|layout| layout.files().iter().map(|file| file.relative))
            .collect();
        for suffix in IGNORED_SUFFIXES {
            assert!(
                all.iter().any(|path| path.ends_with(suffix)),
                "no fixture for {suffix}"
            );
        }
        for prefix in IGNORED_PREFIXES {
            assert!(
                all.iter()
                    .any(|path| path.split('/').any(|part| part.starts_with(prefix))),
                "no fixture for prefix {prefix}"
            );
        }
        for expected in [
            Expected::Empty,
            Expected::NotVideo,
            Expected::Marker,
            Expected::InProgressVideo,
        ] {
            assert!(
                ALL_LAYOUTS
                    .iter()
                    .any(|layout| !layout.relatives(expected).is_empty()),
                "no fixture for {expected:?}"
            );
        }
    }

    #[test]
    fn the_fingerprint_is_versioned_hex_and_moves_with_every_input() {
        let base = expected_fingerprint("folder-1", "a/b.mkv", 10, 20);
        let hex = base.strip_prefix("pf1:").expect("pf1 prefix");
        assert_eq!(hex.len(), 64);
        assert!(
            hex.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        for other in [
            expected_fingerprint("folder-2", "a/b.mkv", 10, 20),
            expected_fingerprint("folder-1", "a/c.mkv", 10, 20),
            expected_fingerprint("folder-1", "a/b.mkv", 11, 20),
            expected_fingerprint("folder-1", "a/b.mkv", 10, 21),
            expected_fingerprint("folder-1", "a/b.mkv1", 0, 20),
        ] {
            assert_ne!(other, base);
        }
        assert_eq!(base, expected_fingerprint("folder-1", "a/b.mkv", 10, 20));
    }

    #[test]
    fn the_expected_completed_download_matches_decision_2() {
        let clock = TestClock::new();
        let tree = ProcessedTree::new();
        let relative = "Movies/Synthetic.Movie.2024.1080p.mkv";
        tree.write_video(relative, 4096, clock.system_time());
        let canonical = std::fs::canonicalize(tree.path(relative)).unwrap();
        let completed = expected_completed_download(
            "folder-1",
            relative,
            &canonical,
            4096,
            clock.system_time(),
            &scryer_domain::MediaFacet::Movie,
        );
        assert_eq!(completed.client_type, EXPECTED_CLIENT_TYPE);
        assert_eq!(completed.client_id, "folder-1");
        assert_eq!(
            completed.download_client_item_id,
            expected_fingerprint("folder-1", relative, 4096, mtime_ns(clock.system_time()))
        );
        assert_eq!(completed.download_id, None);
        assert_eq!(completed.name, "Synthetic.Movie.2024.1080p");
        assert_eq!(
            completed.release_name.as_deref(),
            Some("Synthetic.Movie.2024.1080p")
        );
        assert!(Path::new(&completed.dest_dir).is_absolute());
        assert_eq!(Path::new(&completed.dest_dir), canonical);
        assert_eq!(completed.category, None);
        assert_eq!(completed.size_bytes, Some(4096));
        assert_eq!(completed.completed_at, Some(clock.now()));
        assert_eq!(
            completed.parameters,
            vec![("*scryer_facet".to_string(), "movie".to_string())]
        );
        assert!(completed.posted_file_names.is_empty() && completed.subtitle_paths.is_empty());
    }

    #[test]
    fn the_action_matrix_and_transition_table_match_the_contract_shape() {
        assert_eq!(ACTION_MATRIX.len(), STATUSES.len());
        for (row, status) in ACTION_MATRIX.iter().zip(STATUSES) {
            assert_eq!(row.0, *status, "matrix rows follow section 2.1 order");
        }
        let allowed: usize = STATUSES
            .iter()
            .map(|status| {
                ACTIONS
                    .iter()
                    .filter(|action| action_allowed(status, **action))
                    .count()
            })
            .sum();
        assert_eq!(allowed, 10, "section 2.3 has ten allowed cells");
        assert!(action_allowed("failed", EntryAction::Retry));
        assert!(action_allowed("ignored", EntryAction::Retry));
        assert!(!action_allowed("ignored", EntryAction::Ignore));
        assert!(!action_allowed("importing", EntryAction::Ignore));

        assert_eq!(TRANSITIONS.len(), 18, "section 2.2 has eighteen rows");
        for (index, row) in TRANSITIONS.iter().enumerate() {
            assert_eq!(row.row, index + 1);
            for from in row.from {
                assert!(STATUSES.contains(from), "row {} from {from}", row.row);
            }
            assert!(
                row.to == "per_2_4" || STATUSES.contains(&row.to),
                "row {}",
                row.row
            );
        }
        // The operator rows of 2.2 agree with the 2.3 matrix.
        for (row, action) in [
            (16, EntryAction::ImportAs),
            (17, EntryAction::Retry),
            (18, EntryAction::Ignore),
        ] {
            let transition = TRANSITIONS[row - 1];
            assert_eq!(transition.actor, Actor::Operator);
            let mut from_matrix: Vec<&str> = STATUSES
                .iter()
                .copied()
                .filter(|status| action_allowed(status, action))
                .collect();
            from_matrix.sort_unstable();
            let mut from_table = transition.from.to_vec();
            from_table.sort_unstable();
            assert_eq!(from_table, from_matrix, "row {row} vs matrix {action:?}");
        }
    }

    #[test]
    fn the_refusal_code_list_is_distinct_and_area_prefixed() {
        assert_eq!(EXPECTED_REFUSAL_CODES.len(), 26);
        assert_eq!(REFUSAL_TRIGGERS.len(), EXPECTED_REFUSAL_CODES.len());
        let distinct: HashSet<_> = EXPECTED_REFUSAL_CODES.iter().collect();
        assert_eq!(distinct.len(), EXPECTED_REFUSAL_CODES.len());
        for (code, (trigger_code, trigger)) in EXPECTED_REFUSAL_CODES.iter().zip(REFUSAL_TRIGGERS) {
            assert_eq!(code, trigger_code);
            assert!(!trigger.is_empty(), "{code}");
            assert!(code.starts_with("PROCESSED_FOLDER_"), "{code}");
            assert!(
                code.chars().all(|c| c.is_ascii_uppercase() || c == '_'),
                "{code}"
            );
        }
        for code in [
            "PROCESSED_FOLDER_NAME_TOO_LONG",
            "PROCESSED_FOLDER_FIELD_NOT_TRIMMED",
            "PROCESSED_FOLDER_SCOPE_KIND_INVALID",
            "PROCESSED_FOLDER_MARKER_VALUE_NOT_ALLOWED",
            "PROCESSED_FOLDER_TITLE_NOT_FOUND",
        ] {
            assert!(EXPECTED_REFUSAL_CODES.contains(&code), "{code}");
        }

        let flat: Vec<&str> = CREATE_UPDATE_CHECK_ORDER
            .iter()
            .copied()
            .flatten()
            .copied()
            .collect();
        assert_eq!(
            flat,
            vec![
                "PROCESSED_FOLDER_FIELD_NOT_TRIMMED",
                "PROCESSED_FOLDER_NAME_REQUIRED",
                "PROCESSED_FOLDER_NAME_TOO_LONG",
                "PROCESSED_FOLDER_PATH_NOT_ABSOLUTE",
                "PROCESSED_FOLDER_PATH_MISSING",
                "PROCESSED_FOLDER_PATH_NOT_DIRECTORY",
                "PROCESSED_FOLDER_PATH_UNRESOLVABLE",
                "PROCESSED_FOLDER_LIBRARY_NOT_FOUND",
                "PROCESSED_FOLDER_SCOPE_KIND_INVALID",
                "PROCESSED_FOLDER_INVALID_STABILITY",
                "PROCESSED_FOLDER_INVALID_MARKER",
                "PROCESSED_FOLDER_MARKER_VALUE_NOT_ALLOWED",
                "PROCESSED_FOLDER_OVERLAPS_LIBRARY_ROOT",
                "PROCESSED_FOLDER_OVERLAPS_RECYCLE_BIN",
                "PROCESSED_FOLDER_OVERLAPS_BACKUP_PATH",
                "PROCESSED_FOLDER_OVERLAPS_OTHER_FOLDER",
                "PROCESSED_FOLDER_NAME_TAKEN",
                "PROCESSED_FOLDER_OVERLAPS_OTHER_FOLDER",
            ]
        );
        assert!(
            flat.iter()
                .all(|code| EXPECTED_REFUSAL_CODES.contains(code))
        );
    }

    #[test]
    fn name_length_counts_unicode_scalar_values() {
        let accepted = "a".repeat(100);
        let refused = "a".repeat(101);
        assert_eq!(accepted.chars().count(), 100);
        assert_eq!(refused.chars().count(), 101);
        let accented = "é".repeat(100);
        assert_eq!(accented.chars().count(), 100);
        assert!(
            accented.len() > 100,
            "the boundary is scalar values, not bytes"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_hard_link_double_links_and_never_deletes() {
        let clock = TestClock::new();
        let tree = ProcessedTree::new();
        let library = tempfile::tempdir().unwrap();
        let source = tree.write_video("Hard.Link.2024.mkv", 4096, clock.system_time());
        let dest = library.path().join("Title/Hard.Link.2024.mkv");
        let importer = AlwaysHardLinkImporter::default();
        let result = importer
            .import_file(
                &source,
                &dest,
                scryer_domain::ImportMode::HardlinkOrCopy,
                None,
            )
            .await
            .unwrap();
        assert_eq!(result.strategy, scryer_domain::ImportStrategy::HardLink);
        assert_eq!(inode(&source), inode(&dest));
        assert_eq!(nlink(&source), 2);
        let guard = fabricated_cleanup_guard(&source, &dest).unwrap();
        importer
            .remove_import_source_after_verified_import(guard, &dest)
            .await
            .unwrap();
        assert!(source.exists(), "doubles never delete");
        assert_eq!(importer.calls.imports(), 1);
        assert_eq!(importer.calls.source_removals(), 1);
        assert_eq!(
            importer.calls.modes(),
            vec![scryer_domain::ImportMode::HardlinkOrCopy]
        );
    }

    #[tokio::test]
    async fn the_cleanup_guard_double_copies_and_returns_a_guard() {
        let clock = TestClock::new();
        let tree = ProcessedTree::new();
        let library = tempfile::tempdir().unwrap();
        let source = tree.write_video("Guarded.2024.mkv", 4096, clock.system_time());
        let dest = library.path().join("Guarded.2024.mkv");
        let importer = CopyWithCleanupGuardImporter::default();
        let result = importer
            .import_file(
                &source,
                &dest,
                scryer_domain::ImportMode::HardlinkOrCopy,
                None,
            )
            .await
            .unwrap();
        assert_eq!(result.strategy, scryer_domain::ImportStrategy::Copy);
        let guard = result.source_cleanup.expect("guard");
        assert_eq!(guard.source_path, source);
        assert_eq!(guard.dest_path, dest);
        assert_eq!(guard.source_proof, guard.dest_proof);
        assert_eq!(file_blake3(&source), file_blake3(&dest));
    }

    #[tokio::test]
    async fn the_scripted_double_plays_its_script_then_copies() {
        let clock = TestClock::new();
        let tree = ProcessedTree::new();
        let library = tempfile::tempdir().unwrap();
        let source = tree.write_video("Scripted.2024.mkv", 4096, clock.system_time());
        let dest = library.path().join("Scripted.2024.mkv");
        let importer = ScriptedImporter::new([
            ScriptedOutcome::TransientFailure,
            ScriptedOutcome::PermanentFailure,
        ]);
        let before = tree.snapshot();
        for _ in 0..2 {
            importer
                .import_file(
                    &source,
                    &dest,
                    scryer_domain::ImportMode::HardlinkOrCopy,
                    None,
                )
                .await
                .expect_err("scripted failure");
            assert!(!dest.exists(), "a scripted failure writes nothing");
        }
        before.assert_unchanged(&tree.snapshot(), "failures leave the source alone");
        let snapshot = importer.snapshot_import_source(&source).await.unwrap();
        let result = importer
            .import_file(
                &source,
                &dest,
                scryer_domain::ImportMode::HardlinkOrCopy,
                Some(&snapshot),
            )
            .await
            .unwrap();
        assert_eq!(result.strategy, scryer_domain::ImportStrategy::Copy);
        assert_eq!(file_blake3(&source), file_blake3(&dest));
        assert_eq!(importer.remaining(), 0);
        assert_eq!(importer.calls.imports(), 3);

        let changing = ScriptedImporter::new([ScriptedOutcome::SourceChangedMidCopy]);
        let other_dest = library.path().join("Changed.2024.mkv");
        changing
            .import_file(
                &source,
                &other_dest,
                scryer_domain::ImportMode::HardlinkOrCopy,
                Some(&snapshot),
            )
            .await
            .expect_err("source changed");
        assert!(!other_dest.exists());
        assert_eq!(std::fs::metadata(&source).unwrap().len(), 4096 + 5);

        let stale = ScriptedImporter::new([]);
        stale
            .import_file(
                &source,
                &other_dest,
                scryer_domain::ImportMode::HardlinkOrCopy,
                Some(&snapshot),
            )
            .await
            .expect_err("a stale snapshot is refused");
        assert!(!other_dest.exists());
    }

    #[tokio::test]
    async fn the_seeding_helpers_point_a_library_at_a_root_and_add_titles() {
        let (app, user) = bootstrap();
        let root = tempfile::tempdir().unwrap();
        let library =
            point_default_library_at(&app, scryer_domain::MediaFacet::Movie, root.path()).await;
        assert!(
            library
                .roots
                .iter()
                .any(|entry| Path::new(&entry.path) == root.path()),
            "{library:?}"
        );
        let monitored = add_title(
            &app,
            &user,
            "Synthetic Monitored",
            scryer_domain::MediaFacet::Movie,
            true,
        )
        .await;
        let unmonitored = add_title(
            &app,
            &user,
            "Synthetic Unmonitored",
            scryer_domain::MediaFacet::Movie,
            false,
        )
        .await;
        assert!(monitored.monitored);
        assert!(!unmonitored.monitored);
        assert_eq!(monitored.library_id, library.id);
    }

    #[test]
    fn a_fixture_user_holds_exactly_the_given_app_permissions() {
        let user = user_with(&[scryer_domain::AppPermission::ManageCatalogSettings]);
        assert!(
            user.authorization
                .has_app_permission(scryer_domain::AppPermission::ManageCatalogSettings)
        );
        assert!(
            !user
                .authorization
                .has_app_permission(scryer_domain::AppPermission::ManageSystemSettings)
        );
    }

    #[test]
    fn crate_source_reads_files_under_this_crate() {
        assert!(crate_source("src/lib.rs").is_some());
        assert!(crate_source("src/no/such/file.rs").is_none());
    }

    #[test]
    fn the_result_mapping_covers_every_import_decision_and_skip_reason() {
        use scryer_domain::{ImportDecision, ImportSkipReason};

        fn skip_label(reason: ImportSkipReason) -> &'static str {
            match reason {
                ImportSkipReason::AlreadyImported
                | ImportSkipReason::DuplicateFile
                | ImportSkipReason::PostDownloadRuleBlocked
                | ImportSkipReason::PolicyMismatch
                | ImportSkipReason::UnresolvedIdentity
                | ImportSkipReason::UnparseableEpisode
                | ImportSkipReason::NoVideoFiles
                | ImportSkipReason::UnwantedExecutables
                | ImportSkipReason::DownloadInProgress
                | ImportSkipReason::SourceChanged
                | ImportSkipReason::SourceUnavailable
                | ImportSkipReason::SceneSubtitlePending
                | ImportSkipReason::DiskFull
                | ImportSkipReason::PermissionDenied
                | ImportSkipReason::PasswordRequired
                | ImportSkipReason::ArchiveExtractionPluginRequired
                | ImportSkipReason::ArchiveExtractionTimedOut => reason.as_str(),
            }
        }

        fn decision_label(decision: ImportDecision) -> &'static str {
            match decision {
                ImportDecision::Imported => "Imported",
                ImportDecision::Rejected => "Rejected",
                ImportDecision::Skipped => "Skipped",
                ImportDecision::Conflict => "Conflict",
                ImportDecision::Unmatched => "Unmatched",
                ImportDecision::Failed => "Failed",
            }
        }

        assert_eq!(RESULT_MAPPINGS.len(), 27);
        let mut pairs = HashSet::new();
        for row in RESULT_MAPPINGS {
            assert!(
                pairs.insert((row.decision, row.skip_reason)),
                "duplicate {} {}",
                row.decision,
                row.skip_reason
            );
            assert!(
                STATUSES.contains(&row.ledger_status),
                "{}",
                row.ledger_status
            );
            assert!(
                row.attempt_cap == 0 || row.attempt_cap == 3,
                "{}",
                row.decision
            );
        }

        for decision in [
            ImportDecision::Imported,
            ImportDecision::Rejected,
            ImportDecision::Skipped,
            ImportDecision::Conflict,
            ImportDecision::Unmatched,
            ImportDecision::Failed,
        ] {
            let label = decision_label(decision);
            assert!(
                RESULT_MAPPINGS.iter().any(|row| row.decision == label),
                "{label}"
            );
        }
        for reason in [
            ImportSkipReason::AlreadyImported,
            ImportSkipReason::DuplicateFile,
            ImportSkipReason::PostDownloadRuleBlocked,
            ImportSkipReason::PolicyMismatch,
            ImportSkipReason::UnresolvedIdentity,
            ImportSkipReason::UnparseableEpisode,
            ImportSkipReason::NoVideoFiles,
            ImportSkipReason::UnwantedExecutables,
            ImportSkipReason::DownloadInProgress,
            ImportSkipReason::SourceChanged,
            ImportSkipReason::SourceUnavailable,
            ImportSkipReason::SceneSubtitlePending,
            ImportSkipReason::DiskFull,
            ImportSkipReason::PermissionDenied,
            ImportSkipReason::PasswordRequired,
            ImportSkipReason::ArchiveExtractionPluginRequired,
            ImportSkipReason::ArchiveExtractionTimedOut,
        ] {
            let label = skip_label(reason);
            assert!(
                RESULT_MAPPINGS
                    .iter()
                    .any(|row| { row.decision == "Skipped" && row.skip_reason == label }),
                "{label}"
            );
        }

        let imported_pending = RESULT_MAPPINGS
            .iter()
            .find(|row| row.decision == "Imported" && row.skip_reason == "scene_subtitle_pending");
        let imported_pending = imported_pending.expect("Imported + SceneSubtitlePending");
        assert_eq!(imported_pending.ledger_status, "imported");
        assert_eq!(imported_pending.reason_code, "");

        for reason in [
            "source_changed",
            "source_unavailable",
            "download_in_progress",
        ] {
            let row = RESULT_MAPPINGS
                .iter()
                .find(|row| row.decision == "Skipped" && row.skip_reason == reason)
                .unwrap_or_else(|| panic!("{reason}"));
            assert_eq!(row.ledger_status, "stable", "{reason}");
            assert_eq!(row.attempt_cap, 3, "{reason}");
        }
        let cancelled = RESULT_MAPPINGS
            .iter()
            .find(|row| row.decision == "ErrCanceled")
            .expect("cancelled");
        assert_eq!(cancelled.ledger_status, "stable");
        assert_eq!(cancelled.reason_code, "cancelled");
        assert_eq!(cancelled.attempt_cap, 3);
        assert!(cancelled.executor_proposed);
        assert!(
            RESULT_MAPPINGS
                .iter()
                .any(|row| { row.executor_proposed && row.skip_reason == "unresolved_identity" })
        );
        assert!(
            RESULT_MAPPINGS
                .iter()
                .any(|row| { row.executor_proposed && row.skip_reason == "unparseable_episode" })
        );
    }
}

// Skeletons. Each body lists the exact steps and assertions to make real once
// the named branch lands. Shared setup for every skeleton below, written once:
//
// SETUP(mode, importer):
//   1. `let (app, user) = bootstrap();` then
//      `app.with_test_overrides(|b| b.with_file_importer(Arc::new(importer)))`
//      (crate::testing::AppUseCaseTestExt) when a double is named.
//   2. `let library_root = tempfile::tempdir()`; `point_default_library_at(&app,
//      MediaFacet::Movie, library_root.path())`.
//   3. Set `import.mode` (settings key `IMPORT_MODE_KEY`) for that library to
//      `mode` (`Move` or `HardlinkOrCopy`).
//   4. `let tree = ProcessedTree::new()`; `let clock = TestClock::new()`; wire
//      `clock.source()` into the processed-folder clock seam the base adds.
//   5. `app.create_processed_folder(&user, { name: "Synthetic", root_path:
//      tree.root(), library_id })`, then `set_processed_folder_enabled(true)`.
//   6. `crate::post_download_gate::probe_override::install(...)` so the
//      synthetic bytes probe as the release name says (no real media).
//   7. `add_title(&app, &user, "Synthetic Movie", Movie, true)`.
//   8. Write `Synthetic.Movie.2024.1080p.mkv` (64 KiB) with `clock` mtime,
//      advance the clock past min_age, scan twice so the entry is `stable`,
//      take `let before = tree.snapshot()`.
// RUN: `scan_processed_folder` (queues the import) and wait for the spawned
// import with `test_wait::wait_until` on the entry status (no sleeps).

/// Contract section 1.4, Q1 to Q7: copy-only placement at app level. The
/// variants that need the real `FsFileImporter` (Q1, Q2, Q3, Q4c, Q4d, Q5c on
/// disk) are in `crates/scryer/tests/integration_graphql/processed_folders.rs`.
mod copy_only {
    // Add `use super::*;` when the first body here stops being a todo.

    /// Q1 (double variant), C1: a library configured `Move` still never asks
    /// the importer for `Move` on a processed-folder source.
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: C1 mode force plus scan and importer"]
    async fn q1_a_move_library_never_asks_the_importer_to_move_a_processed_file() {
        todo!(
            "SETUP(Move, ScriptedImporter::new([])); RUN. Assert: entry status imported; \
             importer.calls.modes() == [HardlinkOrCopy] (never Move); \
             importer.calls.source_removals() == 0; before.assert_unchanged(&tree.snapshot()); \
             one media file registered for the title; import row result strategy/link_type Copy."
        )
    }

    /// C1 unit: `resolve_seeding_safe_import_mode` returns `HardlinkOrCopy` for
    /// a processed-folder `CompletedDownload` whatever the configured mode.
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: C1 in seeding_gate.rs"]
    async fn c1_seeding_gate_forces_hardlink_or_copy_for_processed_folder_sources() {
        todo!(
            "For configured in [Move, HardlinkOrCopy]: set import.mode, build \
             expected_completed_download(folder_id, rel, path, size, mtime, Movie), call \
             resolve_seeding_safe_import_mode(..., Some(&completed)); assert HardlinkOrCopy. \
             Control: a non-processed completed (client_type \"qbittorrent\") with Move keeps Move."
        )
    }

    /// C2: `import_file_with_record_progress` sets `copy_only` on the context
    /// exactly for processed-folder sources.
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: C2 setter in import/workflow/mod.rs"]
    async fn c2_the_execution_context_is_copy_only_exactly_for_processed_folder_sources() {
        todo!(
            "Install a FileImporter double that overrides import_file_verified_with_execution_context \
             and records context.copy_only(). Import a processed-folder completed: recorded true. \
             Import a regular completed_download_fixture_item: recorded false."
        )
    }

    /// Q4a (ledger level): a transient importer failure leaves the entry
    /// retryable and a later attempt succeeds.
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: importer and actions"]
    async fn q4a_a_transient_failure_is_retried_and_succeeds() {
        todo!(
            "SETUP(HardlinkOrCopy, ScriptedImporter::new([TransientFailure])); RUN. Assert entry \
             failed, reason error, last_error contains 'synthetic transient copy failure', attempts 1, \
             no library file. retry_processed_folder_entry -> stable; scan -> imported, attempts 2, \
             library bytes == source bytes, before.assert_unchanged. Copy-level retry inside \
             FsFileImporter needs its private fault hooks: core file_importer tests own that part."
        )
    }

    /// Q4b: a non-transient failure gives `failed`, no destination, no temp,
    /// and an untouched source.
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: importer and actions"]
    async fn q4b_a_permanent_failure_ends_failed_and_leaves_nothing_behind() {
        todo!(
            "SETUP(HardlinkOrCopy, ScriptedImporter::new([PermanentFailure])); RUN. Assert entry \
             failed with reason error; no file under library_root (walk it); no *.tmp_import \
             anywhere under library_root or tree; before.assert_unchanged(&tree.snapshot()); \
             importer.calls.source_removals() == 0."
        )
    }

    /// Q4e: a source changed mid-copy gives `failed` (or `superseded` when the
    /// re-stat catches it first) and nothing is imported.
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: importer and actions"]
    async fn q4e_a_source_changed_mid_copy_ends_failed() {
        todo!(
            "SETUP(HardlinkOrCopy, ScriptedImporter::new([SourceChangedMidCopy])); RUN. Assert entry \
             failed (contract Q4e) with last_error naming the source; no media file; no library \
             file. Next scan, V0.2.3: the failed row becomes superseded and a new observing row \
             exists for the grown version, in one transaction. Source bytes stay the grown file."
        )
    }

    /// Q5a: a fresh `AppUseCase` on the same database plus a re-scan imports
    /// nothing new for `imported` rows.
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: scan and importer"]
    async fn q5a_a_fresh_app_on_the_same_database_reimports_nothing() {
        todo!(
            "Use bootstrap_with_cleanup_tracking or a file-backed sqlite path so two AppUseCase \
             values share one DB. SETUP + RUN on app1 until imported. Build app2 on the same DB, \
             scan twice. Assert: importer double on app2 has imports() == 0; entry still imported; \
             one media file; scan payload importsQueued == 0 and recovered == 0."
        )
    }

    /// Q5b and section 2.4: a row forced to `importing` with nothing in flight
    /// is recovered by the next scan, for each imports-row outcome.
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: recover_orphaned_entries (needs a test seam to force importing)"]
    async fn q5b_an_orphaned_importing_row_is_recovered_per_section_2_4() {
        todo!(
            "Drive through state_machine::recovery_* cases; this test is the end-to-end one: force \
             a stable row to importing with attempts 1 through the repository (no in_flight entry), \
             no imports row. scan_processed_folder. Assert status stable, reason interrupted, \
             payload recovered == 1, then the same scan or the next queues it and it imports."
        )
    }

    /// Q6, C4: an importer that hard links anyway is caught after placement.
    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: C4 post-placement assertion"]
    async fn q6_a_hard_link_result_becomes_manual_reconciliation_required() {
        todo!(
            "SETUP(HardlinkOrCopy, AlwaysHardLinkImporter::default()); RUN. Assert entry failed; \
             last_error mentions ManualReconciliationRequired and names source and destination; \
             source still exists (tree.path(rel).exists()); importer.calls.source_removals() == 0; \
             the hard link the double made is left in place (nothing deleted on this path, C4) \
             so nlink(source) == 2 and that is the only tree change: the snapshot diff is exactly \
             one 'changed' line for rel with nlink 1 -> 2."
        )
    }

    /// C4 second leg: a `Copy` result that still carries a cleanup guard is
    /// refused too.
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: C4 post-placement assertion"]
    async fn c4_a_copy_result_with_a_cleanup_guard_is_refused() {
        todo!(
            "SETUP(Move, CopyWithCleanupGuardImporter::default()); RUN. Assert entry failed with \
             ManualReconciliationRequired; source_removals() == 0; before.assert_unchanged."
        )
    }

    /// Q7, C5: the three cleanup consumers refuse a guard for a
    /// processed-folder source without calling the importer's delete.
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: C5 belts in results.rs and upgrade.rs"]
    async fn q7_cleanup_consumers_delete_nothing_for_a_processed_folder_source() {
        todo!(
            "For each of finalize_import_source_cleanup, finalize_deferred_import_source_cleanup, \
             finalize_upgrade_source_cleanup (needs pub(crate) or a #[cfg(test)] hook from core): \
             importer = CopyWithCleanupGuardImporter; guard = fabricated_cleanup_guard(source, dest); \
             completed = expected_completed_download(...). Call with Some(&completed), mode Move. \
             Assert Err(ManualReconciliationRequired), importer.calls.source_removals() == 0, \
             source bytes unchanged."
        )
    }

    /// Q7, C6: the burned-data belt skips processed-folder sources.
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: C6 in burned_source.rs"]
    async fn q7_burned_data_cleanup_skips_a_processed_folder_source() {
        todo!(
            "Call delete_burned_download_data with completed_local = expected_completed_download \
             whose dest_dir is the source file. Assert Skipped(\"processed_folder_source\") and \
             before.assert_unchanged(&tree.snapshot())."
        )
    }

    /// Ian's decision (contract risk 2, Q4d): an empty reserved destination
    /// left by a crash is never deleted automatically. Double variant: the
    /// real-importer variant is in the integration file.
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: importer and C3 existing-destination rule"]
    async fn q4d_an_empty_reserved_destination_is_not_deleted_and_the_entry_fails() {
        todo!(
            "SETUP(HardlinkOrCopy, real-like double that returns ManualReconciliationRequired when \
             dest exists and is empty, as FsFileImporter does). Pre-create the resolved destination \
             path under library_root as a zero-byte file with no media-file row. RUN. Assert entry \
             failed, last_error mentions ManualReconciliationRequired; the zero-byte destination \
             still exists with len 0 (NOT auto-deleted); before.assert_unchanged(&tree.snapshot()); \
             no media file row."
        )
    }
}

/// Contract section 2.2 (one test per row) and section 2.4 (one test per
/// recovery outcome). Rows are driven through the public API where an actor
/// can reach them; rows that need a precondition the API cannot produce use the
/// repository seam the base adds (`ProcessedFolderRepository`).
mod state_machine {
    use super::*;

    /// Shared driver: prints the row so the `todo!` names exactly what to
    /// build. Becomes the real table-driven driver once the base lands.
    async fn drive_row(row: usize, steps: &str) {
        let transition = TRANSITIONS[row - 1];
        assert_eq!(transition.row, row);
        todo!(
            "section 2.2 row {row}: {:?} -> {} by {:?}. Steps: {steps}. Always also assert \
             before.assert_unchanged(&tree.snapshot()) (no actor ever writes the processed tree).",
            transition.from,
            transition.to,
            transition.actor
        )
    }

    macro_rules! row_test {
        ($name:ident, $row:literal, $reason:literal, $steps:literal) => {
            #[tokio::test]
            #[ignore = $reason]
            async fn $name() {
                drive_row($row, $steps).await;
            }
        };
    }

    row_test!(
        row_01_new_version_is_observing_with_count_one,
        1,
        "pending pf/pr1-core: scan",
        "write a video; scan once; entry observing, stable_scan_count 1, first_seen_at == \
         last_seen_at == clock.now(), fingerprint == expected_fingerprint(...)"
    );
    row_test!(
        row_02_unimportable_new_file_is_rejected,
        2,
        "pending pf/pr1-core: scan",
        "V0.2.1: a non-video file (not_a_video) and a zero-byte file (empty) each get a rejected \
         row. A name starting with '.' or '~$', a temp suffix, a marker file, and anything under \
         a hidden directory get no row. An escaping symlink and a symlinked directory stay rejected"
    );
    row_test!(
        row_03_unchanged_ineligible_file_stays_observing_and_counts_up,
        3,
        "pending pf/pr1-core: scan and stability",
        "min_stable_scans 3; scan, advance 10s, scan; observing, count 2, last_seen_at moved"
    );
    row_test!(
        row_04_observing_becomes_stable_on_scans_plus_age_or_marker,
        4,
        "pending pf/pr1-core: scan and stability",
        "covered in detail by the stability module; here: defaults, scan, advance 300s, scan -> \
         stable"
    );
    row_test!(
        row_05_a_changed_version_supersedes_and_inserts_a_new_observing_row,
        5,
        "pending pf/pr1-core: scan",
        "from observing, stable, failed, skipped, needs_review, ignored, and rejected: \
         tree.grow(rel); scan; old row superseded, new observing row count 1 with the new size \
         and mtime, both visible in one transaction. The four non-observing/stable sources are \
         the V0.2.3 [X] rows"
    );
    row_test!(
        row_06_an_absent_path_goes_gone_from_every_listed_state,
        6,
        "pending pf/pr1-core: scan",
        "for each of observing, stable, needs_review, failed, skipped, ignored: put a row there \
         (via API or repository seam), remove the file, scan; status gone. Control: imported, \
         rejected, superseded rows with the file removed stay as they are"
    );
    row_test!(
        row_07_gone_identical_version_reappears_as_observing_on_the_same_row,
        7,
        "pending pf/pr1-core: scan",
        "observing row, remove file, scan (gone), write the identical bytes and mtime back, scan; \
         same entry id, observing, stable_scan_count reset to 1, no second row"
    );
    row_test!(
        row_08_the_worker_claims_stable_to_importing_once,
        8,
        "pending pf/pr1-core: importer",
        "stable row; claim with a blocking importer double (gate on a tokio Notify); observe \
         importing, claimed_at set, attempts 1; a second concurrent claim attempt changes 0 rows"
    );
    row_test!(
        row_09_pre_resolve_failure_goes_to_needs_review,
        9,
        "pending pf/pr1-core: importer",
        "see needs_review module: unmatched, ambiguous, outside_library; here just one: no title \
         -> needs_review/unmatched with attempts 0 (no claim made)"
    );
    row_test!(
        row_10_import_time_restat_mismatch_supersedes_without_importing,
        10,
        "pending pf/pr1-core: importer",
        "stable row; between claim and import change the file (hook in the importer double's \
         snapshot_import_source) or move the canonical path out of root; superseded, reason \
         entry_changed, importer.calls.imports() == 0"
    );
    row_test!(
        row_11_imported_and_already_imported_both_end_imported,
        11,
        "pending pf/pr1-core: importer",
        "case Imported: import_id and title_id stored. case Skipped+AlreadyImported: prime the \
         library with identical bytes; status imported, reason already_imported"
    );
    row_test!(
        row_12_unmatched_or_conflict_from_the_pipeline_goes_to_needs_review,
        12,
        "pending pf/pr1-core: importer",
        "force ImportDecision::Unmatched (title deleted between pre-resolve and import) -> \
         unmatched; Conflict -> conflict. No guessed title_id"
    );
    row_test!(
        row_13_rejected_or_other_skips_go_to_skipped,
        13,
        "pending pf/pr1-core: importer",
        "Rejected -> skipped/rejected. Other Skipped reasons follow RESULT_MAPPINGS, including \
         the [X] rows, not a single skipped bucket"
    );
    row_test!(
        row_14_failures_and_environmental_skips_go_to_failed,
        14,
        "pending pf/pr1-core: importer",
        "ScriptedImporter PermanentFailure -> failed with last_error and reason failed or error, \
         per RESULT_MAPPINGS. Skipped+DiskFull -> failed/disk_full. Skipped+PermissionDenied -> \
         failed/permission_denied. SourceChanged, SourceUnavailable, DownloadInProgress, and \
         Err(Canceled) return to stable unless attempts >= 3, then failed with the same reason"
    );
    row_test!(
        row_15_an_orphaned_claim_is_recovered,
        15,
        "pending pf/pr1-core: recover_orphaned_entries",
        "see recovery_* tests below for each section 2.4 outcome"
    );
    row_test!(
        row_16_import_as_moves_needs_review_failed_skipped_to_importing,
        16,
        "pending pf/pr1-core: actions",
        "for each from-state: import_processed_folder_entry(entry, title in folder library); \
         returns entry importing then imported; same re-stat check (changed file -> \
         PROCESSED_FOLDER_ENTRY_CHANGED)"
    );
    row_test!(
        row_17_retry_moves_failed_and_ignored_to_stable,
        17,
        "pending pf/pr1-core: actions",
        "for failed and ignored: retry; status stable, reason_code None, last_error None, and \
         attempts reset to 0 (V0.2.8 [X]). With the folder enabled the mutation queues the import"
    );
    row_test!(
        row_18_ignore_moves_the_listed_states_to_ignored,
        18,
        "pending pf/pr1-core: actions",
        "for observing, stable, needs_review, failed, skipped: ignore; status ignored; a later \
         scan never touches it while the file is unchanged"
    );

    /// Section 2.4: recovery outcome for an orphaned `importing` row, keyed by
    /// the imports-row status found via `list_imports_for_identities`.
    async fn drive_recovery(imports_row: Option<&str>, attempts: i64, expected: &str) {
        todo!(
            "Force a row to importing with attempts {attempts} and no in_flight id; seed an \
             imports row with status {imports_row:?} for ClientJobLocator{{client_id: folder_id, \
             client_type: \"processed_folder\", item_id: fingerprint}} (None = no row). Run \
             scan_processed_folder. Assert entry {expected}; payload recovered == 1; for \
             completed also import_id set. Also run the same via an entry action on that folder \
             (recovery runs there too, under the scan lock)."
        )
    }

    macro_rules! recovery_test {
        ($name:ident, $row:expr, $attempts:literal, $expected:literal) => {
            #[tokio::test]
            #[ignore = "pending pf/pr1-core: recover_orphaned_entries"]
            async fn $name() {
                drive_recovery($row, $attempts, $expected).await;
            }
        };
    }

    recovery_test!(
        recovery_completed_imports_row_ends_imported,
        Some("completed"),
        1,
        "imported"
    );
    recovery_test!(
        recovery_skipped_imports_row_ends_skipped,
        Some("skipped"),
        1,
        "skipped"
    );
    recovery_test!(
        recovery_failed_imports_row_under_three_attempts_is_stable_interrupted,
        Some("failed"),
        2,
        "stable/interrupted"
    );
    recovery_test!(
        recovery_processing_imports_row_is_stable_interrupted,
        Some("processing"),
        1,
        "stable/interrupted"
    );
    recovery_test!(
        recovery_running_imports_row_is_stable_interrupted,
        Some("running"),
        1,
        "stable/interrupted"
    );
    recovery_test!(
        recovery_pending_imports_row_is_stable_interrupted,
        Some("pending"),
        1,
        "stable/interrupted"
    );
    recovery_test!(
        recovery_no_imports_row_is_stable_interrupted,
        None,
        1,
        "stable/interrupted"
    );
    recovery_test!(
        recovery_third_attempt_ends_failed_interrupted,
        None,
        3,
        "failed/interrupted"
    );

    /// Section 2.4: a claim whose task is alive is never stolen, however long
    /// it runs (no wall-clock timeout).
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: recover_orphaned_entries"]
    async fn a_live_claim_is_not_recovered_however_old() {
        todo!(
            "Gate a ScriptedImporter-like double on a tokio Notify so the import stays in flight; \
             advance the TestClock by 24h; scan. Assert entry still importing, payload \
             recovered == 0; release the gate; entry imported with attempts 1."
        )
    }

    /// V0.2.7, every decision and skip reason, including the [X] rows.
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: exhaustive result mapping V0.2.7"]
    async fn every_import_result_maps_without_a_catchall_arm() {
        let rows = RESULT_MAPPINGS.len();
        todo!(
            "Drive the ledger mapper for each of the {rows} RESULT_MAPPINGS rows. A specific \
             skip_reason wins over '*'. '-' means the skip reason is absent. An empty reason_code \
             stores null. error_else_failed: Failed uses the mapped error reason, else reason_code \
             failed. import_io_else_error: Err(other) maps AppError::reason() equal to \
             error_reasons::import::DISK_FULL, PERMISSION_DENIED, or FILE_NOT_FOUND (values \
             IMPORT_DISK_FULL, IMPORT_PERMISSION_DENIED, IMPORT_FILE_NOT_FOUND) to disk_full, \
             permission_denied, or file_not_found; anything else is error; last_error is the \
             message. Never match message text. attempt_cap 3: attempts >= 3 stores failed with \
             the same reason, otherwise ledger_status. executor_proposed rows are the [X] rows \
             and are still asserted. The match has no '_' arm. Imported plus SceneSubtitlePending \
             is imported."
        )
    }

    /// V0.2.1 named beside row 2.
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: scan qualification"]
    async fn v0_2_1_non_video_and_empty_are_rejected_and_hidden_names_have_no_row() {
        todo!(
            "Rejected rows: .nfo -> rejected/not_a_video, zero-byte .mkv -> rejected/empty. No \
             row: '.hidden.mkv', '~$lock.mkv', 'file.mkv.part' and the other IGNORED_SUFFIXES, \
             the marker file, and 'subdir/.secret/clip.mkv'. Counts do not include the no-row paths."
        )
    }

    /// V0.2.3 [X]: the same size or mtime change supersedes terminal rows too.
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: scan version rows"]
    async fn v0_2_3_terminal_rows_supersede_when_size_or_mtime_changes() {
        todo!(
            "For each of failed, skipped, needs_review, ignored, and rejected: seed that status \
             at rel, change size or mtime, scan. Old row superseded, new observing row for the \
             new version, same transaction. imported and superseded and gone do not follow this \
             rule (row 6 controls which statuses can go gone)."
        )
    }

    /// V0.2.9 [X].
    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: scan"]
    async fn v0_2_9_a_non_utf8_file_name_leaves_no_row() {
        todo!(
            "Create a directory entry whose name is not UTF-8 (a byte 0xFF in the name) inside \
             the processed root. Scan. No ledger row, a warning log carries the lossy path, and \
             discovered, rejected, and gone stay unchanged for that name. The file is untouched."
        )
    }

    /// V0.2.10 [X]: copy-only refuses a symlink at the destination.
    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: C3 existing-destination rule"]
    async fn v0_2_10_copy_only_refuses_a_symlink_destination() {
        todo!(
            "Copy-only import whose destination is an existing symlink to a regular file, and a \
             second case whose destination is the same inode as the source. Both refuse with \
             ManualReconciliationRequired. The symlink is not replaced, the source is untouched, \
             and no media-file row is written."
        )
    }

    /// V2.1: the claim statement requires the folder to be enabled.
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: claim SQL"]
    async fn v2_1_a_disabled_folder_blocks_the_claim() {
        todo!(
            "Stable row, then disable the folder before the worker claims. The claim UPDATE \
             (status stable AND folder enabled) changes 0 rows. The worker reports \
             PROCESSED_FOLDER_DISABLED. The row stays stable with attempts unchanged. Re-enable \
             and the same row can be claimed."
        )
    }

    /// V2.4 [U]: record what an in-flight import does when its library disappears.
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: library delete during import"]
    async fn v2_4_library_delete_while_importing_is_an_observation() {
        todo!(
            "One entry importing, gated on a Notify. Delete the library. The FK cascade removes \
             the folder and its ledger (risk 15) and is not blocked. Release the import. Record \
             the in-flight outcome (error, destination, source). Do not assert a specific ledger \
             status: the row is already gone. This is an observation, not a pass/fail contract."
        )
    }
}

/// Contract section 2.3: one test per cell. Allowed cells assert the 2.2
/// target state; refused cells assert `PROCESSED_FOLDER_ENTRY_NOT_ACTIONABLE`
/// and an unchanged row (every column, compared as a whole).
mod action_matrix {
    use super::*;

    async fn drive_cell(status: &str, action: EntryAction) {
        let allowed = action_allowed(status, action);
        todo!(
            "Put an entry in {status} (scan, worker or repository seam). Snapshot the row. Call \
             the {action:?} mutation on the use case (importAs with a monitored title in the \
             folder library). allowed={allowed}: if true assert the section 2.2 target (importAs \
             -> importing then the worker outcome; retry -> stable, reason and last_error \
             cleared; ignore -> ignored); if false assert AppError validation_refused with code \
             PROCESSED_FOLDER_ENTRY_NOT_ACTIONABLE and the re-read row equals the snapshot. Also \
             assert the GraphQL allowedActions of the entry before the call lists exactly the \
             allowed actions for {status}."
        )
    }

    macro_rules! cell {
        ($name:ident, $status:literal, $action:ident) => {
            #[tokio::test]
            #[ignore = "pending pf/pr1-core: actions"]
            async fn $name() {
                drive_cell($status, EntryAction::$action).await;
            }
        };
    }

    cell!(observing_then_import_as, "observing", ImportAs);
    cell!(observing_then_retry, "observing", Retry);
    cell!(observing_then_ignore, "observing", Ignore);
    cell!(stable_then_import_as, "stable", ImportAs);
    cell!(stable_then_retry, "stable", Retry);
    cell!(stable_then_ignore, "stable", Ignore);
    cell!(importing_then_import_as, "importing", ImportAs);
    cell!(importing_then_retry, "importing", Retry);
    cell!(importing_then_ignore, "importing", Ignore);
    cell!(imported_then_import_as, "imported", ImportAs);
    cell!(imported_then_retry, "imported", Retry);
    cell!(imported_then_ignore, "imported", Ignore);
    cell!(skipped_then_import_as, "skipped", ImportAs);
    cell!(skipped_then_retry, "skipped", Retry);
    cell!(skipped_then_ignore, "skipped", Ignore);
    cell!(failed_then_import_as, "failed", ImportAs);
    cell!(failed_then_retry, "failed", Retry);
    cell!(failed_then_ignore, "failed", Ignore);
    cell!(needs_review_then_import_as, "needs_review", ImportAs);
    cell!(needs_review_then_retry, "needs_review", Retry);
    cell!(needs_review_then_ignore, "needs_review", Ignore);
    cell!(ignored_then_import_as, "ignored", ImportAs);
    cell!(ignored_then_retry, "ignored", Retry);
    cell!(ignored_then_ignore, "ignored", Ignore);
    cell!(rejected_then_import_as, "rejected", ImportAs);
    cell!(rejected_then_retry, "rejected", Retry);
    cell!(rejected_then_ignore, "rejected", Ignore);
    cell!(superseded_then_import_as, "superseded", ImportAs);
    cell!(superseded_then_retry, "superseded", Retry);
    cell!(superseded_then_ignore, "superseded", Ignore);
    cell!(gone_then_import_as, "gone", ImportAs);
    cell!(gone_then_retry, "gone", Retry);
    cell!(gone_then_ignore, "gone", Ignore);

    /// Section 2.3: a lost race (status changed between read and UPDATE)
    /// refuses with `ENTRY_NOT_ACTIONABLE` and changes nothing.
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: actions"]
    async fn a_lost_race_is_not_actionable() {
        todo!(
            "failed row; start retry and ignore concurrently (tokio::join!). Exactly one \
             succeeds; the other returns PROCESSED_FOLDER_ENTRY_NOT_ACTIONABLE; the row ends in \
             the winner's state."
        )
    }

    /// Section 2.3: `importAs` and `retry` on a disabled folder are refused
    /// with `PROCESSED_FOLDER_DISABLED`; `ignore` is allowed.
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: actions"]
    async fn a_disabled_folder_refuses_import_as_and_retry_but_allows_ignore() {
        todo!(
            "failed row; set_processed_folder_enabled(false). importAs -> PROCESSED_FOLDER_DISABLED, \
             retry -> PROCESSED_FOLDER_DISABLED, row unchanged after both; ignore -> ignored."
        )
    }
}

/// Contract section 8.1: stability needs `min_stable_scans` and `min_age`,
/// driven only by the injected clock.
mod stability {
    // Add `use super::*;` when the first body here stops being a todo.

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: stability.rs and a clock seam on scans"]
    async fn scans_alone_are_not_enough_without_age() {
        todo!(
            "min_stable_scans 2, min_age 300. Write file with mtime = clock.now(). Scan 5 times \
             advancing 10s each (50s total). Entry observing, count 5, eligibleAt == \
             first_seen_at + 300s. Advance to first_seen_at + 300s; scan; stable."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: stability.rs and a clock seam on scans"]
    async fn age_alone_is_not_enough_without_scans() {
        todo!(
            "min_stable_scans 3. Write file with mtime = now - 1 day. Scan once; advance 1 day; \
             scan: count 2, still observing. Scan again: stable."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: stability.rs and a clock seam on scans"]
    async fn a_recent_mtime_holds_an_old_entry_back() {
        todo!(
            "Both clauses of section 2.2 row 4: now - first_seen_at >= min_age AND now - mtime >= \
             min_age. Write file with mtime = now + 0, scan, advance 299s, scan: observing; \
             advance 1s, scan: stable. Exact boundary: >= (299 no, 300 yes)."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: stability.rs and scan"]
    async fn a_growing_file_never_imports_and_leaves_superseded_rows() {
        todo!(
            "Each scan: tree.grow(rel, 1024, clock.system_time()) then advance 400s and scan, 5 \
             times. Assert: 5 rows for rel, 4 superseded, 1 observing; importer.calls.imports() == 0. \
             Stop growing; two more scans past min_age; the newest row becomes stable and imports."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: stability.rs and scan"]
    async fn a_marker_makes_a_file_stable_on_the_first_scan() {
        todo!(
            "V0.2.2: for ProcessorLayout::Unmanic (SiblingSuffix '.done') and CustomFfmpeg \
             (DirectoryFile '_DONE'): materialize with mtime = now, configure the layout marker, \
             scan once. Every Expected::Importable file goes (none) -> stable directly, \
             marker_seen = 1, stable_scan_count = 1. Marker files have no row. InProgressVideo \
             files stay observing."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: stability.rs and scan"]
    async fn with_marker_mode_on_and_no_marker_the_file_falls_back_to_scans_plus_age() {
        todo!(
            "Unmanic layout with marker mode SiblingSuffix('.done'): the InProgressVideo file has \
             no marker. Scan, advance 300s, scan: stable (fallback), marker_seen false."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: scan"]
    async fn every_processor_layout_ignores_temp_hidden_and_marker_files() {
        todo!(
            "V0.2.1: for each ALL_LAYOUTS, materialize and scan twice past min_age. NotVideo rows \
             are rejected/not_a_video and Empty rows are rejected/empty. Importable and \
             InProgressVideo have rows. A name starting with '.' or '~$', a temp suffix, a marker \
             file, and anything under a hidden directory have no row. Tree snapshot unchanged."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-base: preview_processed_folder_scan"]
    async fn preview_reports_would_be_states_without_writing_the_ledger() {
        todo!(
            "Materialize CustomFfmpeg; preview. wouldBe matches what a scan would do; entry count \
             in the ledger stays 0; limit 1 sets truncated true; tree snapshot unchanged."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: scan"]
    async fn a_second_concurrent_scan_is_refused() {
        todo!(
            "Hold the folder scan lock (start a scan whose importer or a scan hook waits on a \
             Notify); a second scan_processed_folder returns PROCESSED_FOLDER_SCAN_IN_PROGRESS."
        )
    }
}

/// Contract section 8.3: `needs_review` never guesses a title.
mod needs_review {
    // Add `use super::*;` when the first body here stops being a todo.

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: importer pre-resolve"]
    async fn no_matching_title_is_unmatched() {
        todo!(
            "No titles. Stable 'Synthetic.Movie.2024.1080p.mkv'. Scan. needs_review/unmatched, \
             title_id None; list imports is empty; no media file; no library file."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: importer pre-resolve"]
    async fn two_monitored_titles_are_ambiguous() {
        todo!(
            "add_title twice with names that both match the stem (two monitored 'Synthetic Movie' \
             with different years absent from the name). needs_review/ambiguous; no import row; \
             no media file."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: importer pre-resolve"]
    async fn a_title_in_another_library_is_outside_library() {
        todo!(
            "Create a second movie library (libraries.create) and add the matching title there. \
             needs_review/outside_library; no import row; no media file."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: importer pre-resolve (contract risk 6)"]
    async fn an_unmonitored_title_is_not_matched() {
        todo!(
            "Only an unmonitored matching title. needs_review/unmatched (MonitoredTitleMatcher \
             matches monitored titles only)."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: actions"]
    async fn import_as_a_title_from_another_library_is_refused() {
        todo!(
            "needs_review entry; import_processed_folder_entry with a title from another library \
             -> PROCESSED_FOLDER_TITLE_OUTSIDE_LIBRARY; row unchanged; importer not called."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: actions"]
    async fn import_as_a_good_title_imports_by_copy() {
        todo!(
            "needs_review/ambiguous; import_processed_folder_entry(entry, title_a). Entry imported \
             with title_id == title_a.id; media file on title_a only; source unchanged."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: actions"]
    async fn import_as_an_unknown_entry_or_title_is_not_found() {
        todo!(
            "Unknown entry id -> PROCESSED_FOLDER_ENTRY_NOT_FOUND. Unknown title id: record the \
             code core returns (contract does not name one; likely a generic not-found)."
        )
    }
}

/// Contract section 8.4: re-scan after `imported` and `gone` revival.
mod rescan_and_gone {
    // Add `use super::*;` when the first body here stops being a todo.

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: scan"]
    async fn a_rescan_after_imported_imports_nothing() {
        todo!(
            "Import one file. Scan 3 more times past min_age. importer.calls.imports() == 1; one \
             media file; entry imported; payload importsQueued == 0 each time."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: scan"]
    async fn gone_then_reappearing_is_observing_on_the_same_row() {
        todo!(
            "Same as state_machine row 7 but end to end through import: observing row, remove, \
             scan (gone, payload gone == 1), restore identical bytes and mtime, scan: same id, \
             observing, count 1; two scans later imports once."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: scan"]
    async fn an_imported_file_removed_from_the_folder_stays_imported() {
        todo!(
            "Import; remove the source; scan. Entry still imported (2.2 row 6 excludes imported); \
             the library copy still exists with the same bytes."
        )
    }
}

/// Contract sections 7.4 and 8.5: symlinks.
#[cfg(unix)]
mod symlinks {
    // Add `use super::*;` when the first body here stops being a todo.

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: scan"]
    async fn an_escaping_file_symlink_is_rejected() {
        todo!(
            "build_symlink_layout(&tree, outside.path(), mtime); scan. escaping_file row rejected, \
             reason symlink_escapes_root; outside target untouched (TreeSnapshot of outside equal)."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: scan"]
    async fn a_symlinked_dir_is_rejected_and_not_followed() {
        todo!("Scan. symlinked_dir row rejected/symlink_dir; no row for behind_symlinked_dir.")
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: scan and importer"]
    async fn an_in_root_symlinked_file_imports_by_copy_and_the_library_file_is_not_a_symlink() {
        todo!(
            "Stable and import in_root_link. Library file: symlink_metadata is a regular file, \
             inode != inode(in_root_target), bytes equal; importer received the canonical target \
             path (record it in the double); entry keyed by the link's relative path."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: importer re-canonicalize"]
    async fn a_symlink_retargeted_outside_after_stable_is_superseded() {
        todo!(
            "in_root_link stable; retarget the link to an outside file before the claim; RUN. \
             Entry superseded/entry_changed; importer not called (section 7.4 last bullet)."
        )
    }
}

/// Contract section 7 and 8.6: create and update validation, one test per
/// refusal case, lexical and canonical (symlinked alias) overlap, and the
/// reverse checks. Every refusal also asserts that no folder row was written
/// (create) or the row is unchanged (update).
mod validation {
    use super::*;

    async fn drive_refusal(code: &str, steps: &str) {
        assert!(EXPECTED_REFUSAL_CODES.contains(&code), "{code}");
        todo!(
            "{steps}. Assert Err(AppError) whose validation_refused code == {code} (same value \
             GraphQL puts in extensions.reason); list_processed_folders unchanged."
        )
    }

    macro_rules! refusal {
        ($name:ident, $reason:literal, $code:literal, $steps:literal) => {
            #[tokio::test]
            #[ignore = $reason]
            async fn $name() {
                drive_refusal($code, $steps).await;
            }
        };
    }

    refusal!(
        empty_name_is_required,
        "pending pf/pr1-base: validation.rs",
        "PROCESSED_FOLDER_NAME_REQUIRED",
        "create and update with name '' only; a whitespace name is FIELD_NOT_TRIMMED, checked first, and is never trimmed into empty"
    );
    refusal!(
        duplicate_name_is_taken,
        "pending pf/pr1-base: validation.rs",
        "PROCESSED_FOLDER_NAME_TAKEN",
        "create 'A' on tree1, then 'A' on tree2; update B's name to 'A'. The store must surface RepositoryErrorKind::UniqueViolation on idx_processed_folders_name (AppError::repository_kind); map that kind to this code. Never match the driver message"
    );
    refusal!(
        relative_path_is_not_absolute,
        "pending pf/pr1-base: validation.rs",
        "PROCESSED_FOLDER_PATH_NOT_ABSOLUTE",
        "create with root_path 'relative/dir'"
    );
    refusal!(
        missing_path_is_missing,
        "pending pf/pr1-base: validation.rs",
        "PROCESSED_FOLDER_PATH_MISSING",
        "create with tree.path('does-not-exist')"
    );
    refusal!(
        file_path_is_not_a_directory,
        "pending pf/pr1-base: validation.rs",
        "PROCESSED_FOLDER_PATH_NOT_DIRECTORY",
        "write tree file 'f.mkv'; create with that file as root_path"
    );
    refusal!(
        dangling_symlink_path_is_unresolvable,
        "pending pf/pr1-base: validation.rs",
        "PROCESSED_FOLDER_PATH_UNRESOLVABLE",
        "unix: symlink tree.path('dangling') -> tree.path('nowhere'); create with it"
    );
    refusal!(
        equal_to_a_library_root_overlaps,
        "pending pf/pr1-base: validation.rs",
        "PROCESSED_FOLDER_OVERLAPS_LIBRARY_ROOT",
        "point_default_library_at(root); create with root itself"
    );
    refusal!(
        inside_a_library_root_overlaps,
        "pending pf/pr1-base: validation.rs",
        "PROCESSED_FOLDER_OVERLAPS_LIBRARY_ROOT",
        "create with root/sub (descendant)"
    );
    refusal!(
        containing_a_library_root_overlaps,
        "pending pf/pr1-base: validation.rs",
        "PROCESSED_FOLDER_OVERLAPS_LIBRARY_ROOT",
        "library root = parent/lib; create with parent (ancestor)"
    );
    refusal!(
        a_symlinked_alias_of_a_library_root_overlaps,
        "pending pf/pr1-base: validation.rs",
        "PROCESSED_FOLDER_OVERLAPS_LIBRARY_ROOT",
        "unix: alias = symlink to the library root (and to its parent, and to a child); create with alias, alias/., and a trailing-slash spelling; all refused canonically"
    );
    refusal!(
        a_library_root_reached_through_a_symlinked_parent_overlaps,
        "pending pf/pr1-base: validation.rs",
        "PROCESSED_FOLDER_OVERLAPS_LIBRARY_ROOT",
        "unix: library root configured via a symlinked path; processed folder given by its real path; refused"
    );
    refusal!(
        the_custom_recycle_bin_overlaps,
        "pending pf/pr1-base: validation.rs",
        "PROCESSED_FOLDER_OVERLAPS_RECYCLE_BIN",
        "enable custom recycle bin at tree2; create at tree2, tree2/sub, and a symlinked alias of tree2"
    );
    refusal!(
        the_backup_path_overlaps,
        "pending pf/pr1-base: validation.rs",
        "PROCESSED_FOLDER_OVERLAPS_BACKUP_PATH",
        "set BACKUP_PATH_KEY to tree2; create at tree2, its child, its parent, and a symlinked alias"
    );
    refusal!(
        another_processed_folder_overlaps,
        "pending pf/pr1-base: validation.rs",
        "PROCESSED_FOLDER_OVERLAPS_OTHER_FOLDER",
        "create A at tree; create B at tree, tree/sub, the parent of tree, and a symlinked alias of tree; update A onto B's path is refused too, updating A onto its own path is allowed. A same normalized root with a different name is UniqueViolation on idx_processed_folders_normalized_root, mapped to this code"
    );
    refusal!(
        an_unknown_library_is_not_found,
        "pending pf/pr1-base: validation.rs",
        "PROCESSED_FOLDER_LIBRARY_NOT_FOUND",
        "create with library_id 'no-such-library'"
    );
    refusal!(
        scans_out_of_range_are_invalid_stability,
        "pending pf/pr1-base: validation.rs",
        "PROCESSED_FOLDER_INVALID_STABILITY",
        "min_stable_scans 1 and 11 refused; 2 and 10 accepted (boundaries)"
    );
    refusal!(
        age_out_of_range_is_invalid_stability,
        "pending pf/pr1-base: validation.rs",
        "PROCESSED_FOLDER_INVALID_STABILITY",
        "min_age_seconds 59 and 86401 refused; 60 and 86400 accepted"
    );
    refusal!(
        bad_markers_are_invalid,
        "pending pf/pr1-base: validation.rs",
        "PROCESSED_FOLDER_INVALID_MARKER",
        "marker_value '', 'a/b', 'a\\\\b', '..', and SIBLING_SUFFIX 'done' (no leading dot) refused; SIBLING_SUFFIX '.done' and DIRECTORY_FILE '_DONE' accepted. A value while the mode is NONE is MARKER_VALUE_NOT_ALLOWED, not this code"
    );
    refusal!(
        updating_an_unknown_folder_is_not_found,
        "pending pf/pr1-base: config.rs",
        "PROCESSED_FOLDER_NOT_FOUND",
        "update, delete, set_enabled, scan, preview with id 'no-such-folder'"
    );

    #[tokio::test]
    #[ignore = "pending pf/pr1-base: validation.rs"]
    async fn untrimmed_long_names_and_untrimmed_paths_are_refused() {
        todo!(
            "Combined case of the named code skeletons: FIELD_NOT_TRIMMED for name, rootPath and \
             markerValue (leading or trailing whitespace, never trimmed) and NAME_TOO_LONG at 101 \
             Unicode scalar values, with 100 scalar values accepted. Run it after the single-code \
             cases, on both create and update."
        )
    }

    macro_rules! refusal_by_code {
        ($name:ident, $code:literal) => {
            #[tokio::test]
            #[ignore = "pending pf/pr1-base: processed_folder_refusal and validation.rs"]
            async fn $name() {
                let trigger = REFUSAL_TRIGGERS
                    .iter()
                    .find(|(code, _)| *code == $code)
                    .map(|(_, trigger)| *trigger)
                    .expect($code);
                drive_refusal($code, trigger).await;
            }
        };
    }

    refusal_by_code!(processed_folder_not_found, "PROCESSED_FOLDER_NOT_FOUND");
    refusal_by_code!(
        processed_folder_entry_not_found,
        "PROCESSED_FOLDER_ENTRY_NOT_FOUND"
    );
    refusal_by_code!(
        processed_folder_name_required,
        "PROCESSED_FOLDER_NAME_REQUIRED"
    );
    refusal_by_code!(
        processed_folder_name_too_long,
        "PROCESSED_FOLDER_NAME_TOO_LONG"
    );
    refusal_by_code!(processed_folder_name_taken, "PROCESSED_FOLDER_NAME_TAKEN");
    refusal_by_code!(
        processed_folder_field_not_trimmed,
        "PROCESSED_FOLDER_FIELD_NOT_TRIMMED"
    );
    refusal_by_code!(
        processed_folder_path_not_absolute,
        "PROCESSED_FOLDER_PATH_NOT_ABSOLUTE"
    );
    refusal_by_code!(
        processed_folder_path_missing,
        "PROCESSED_FOLDER_PATH_MISSING"
    );
    refusal_by_code!(
        processed_folder_path_not_directory,
        "PROCESSED_FOLDER_PATH_NOT_DIRECTORY"
    );
    refusal_by_code!(
        processed_folder_path_unresolvable,
        "PROCESSED_FOLDER_PATH_UNRESOLVABLE"
    );
    refusal_by_code!(
        processed_folder_overlaps_library_root,
        "PROCESSED_FOLDER_OVERLAPS_LIBRARY_ROOT"
    );
    refusal_by_code!(
        processed_folder_overlaps_recycle_bin,
        "PROCESSED_FOLDER_OVERLAPS_RECYCLE_BIN"
    );
    refusal_by_code!(
        processed_folder_overlaps_backup_path,
        "PROCESSED_FOLDER_OVERLAPS_BACKUP_PATH"
    );
    refusal_by_code!(
        processed_folder_overlaps_other_folder,
        "PROCESSED_FOLDER_OVERLAPS_OTHER_FOLDER"
    );
    refusal_by_code!(
        processed_folder_library_not_found,
        "PROCESSED_FOLDER_LIBRARY_NOT_FOUND"
    );
    refusal_by_code!(
        processed_folder_scope_kind_invalid,
        "PROCESSED_FOLDER_SCOPE_KIND_INVALID"
    );
    refusal_by_code!(
        processed_folder_invalid_stability,
        "PROCESSED_FOLDER_INVALID_STABILITY"
    );
    refusal_by_code!(
        processed_folder_invalid_marker,
        "PROCESSED_FOLDER_INVALID_MARKER"
    );
    refusal_by_code!(
        processed_folder_marker_value_not_allowed,
        "PROCESSED_FOLDER_MARKER_VALUE_NOT_ALLOWED"
    );
    refusal_by_code!(processed_folder_disabled, "PROCESSED_FOLDER_DISABLED");
    refusal_by_code!(
        processed_folder_scan_in_progress,
        "PROCESSED_FOLDER_SCAN_IN_PROGRESS"
    );
    refusal_by_code!(
        processed_folder_entry_not_actionable,
        "PROCESSED_FOLDER_ENTRY_NOT_ACTIONABLE"
    );
    refusal_by_code!(
        processed_folder_entry_changed,
        "PROCESSED_FOLDER_ENTRY_CHANGED"
    );
    refusal_by_code!(
        processed_folder_title_outside_library,
        "PROCESSED_FOLDER_TITLE_OUTSIDE_LIBRARY"
    );
    refusal_by_code!(
        processed_folder_title_not_found,
        "PROCESSED_FOLDER_TITLE_NOT_FOUND"
    );
    refusal_by_code!(
        processed_folder_imports_in_flight,
        "PROCESSED_FOLDER_IMPORTS_IN_FLIGHT"
    );

    #[tokio::test]
    #[ignore = "pending pf/pr1-base: validation.rs check order"]
    async fn create_and_update_follow_the_v4_check_order() {
        let order = CREATE_UPDATE_CHECK_ORDER
            .iter()
            .map(|phase| phase.join(", "))
            .collect::<Vec<_>>()
            .join(" -> ");
        todo!(
            "On create, then on update, feed one input that is also wrong in every later phase. \
             The first refusal is the earliest phase, and repairing that phase reveals the next: \
             {order}. Trim is FIELD_NOT_TRIMMED and must not trim. Store write maps \
             UniqueViolation on idx_processed_folders_name to NAME_TAKEN and on \
             idx_processed_folders_normalized_root to OVERLAPS_OTHER_FOLDER. Never match error text."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-base: store unique indexes"]
    async fn unique_violation_forces_both_unique_indexes() {
        todo!(
            "On SQLite and, when SCRYER_TEST_POSTGRES_URL is set, on Postgres: two folders that \
             share a name and differ in normalized_root_path. The insert or update fails as \
             RepositoryErrorKind::UniqueViolation on idx_processed_folders_name, and the use case \
             raises PROCESSED_FOLDER_NAME_TAKEN. Two folders that share normalized_root_path and \
             differ in name: UniqueViolation on idx_processed_folders_normalized_root maps to \
             PROCESSED_FOLDER_OVERLAPS_OTHER_FOLDER. Read the kind with AppError::repository_kind \
             before the mapping, or validation_reason after it. Never match the driver message."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-base: config.rs"]
    async fn a_valid_create_stores_a_normalized_disabled_folder() {
        todo!(
            "create with defaults. enabled false; min_stable_scans 2; min_age_seconds 300; \
             marker_mode none; scope_kind library; normalized_root_path == \
             normalize_recycle_config_path(root); tree snapshot unchanged."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: reverse check in library/library.rs"]
    async fn reverse_a_library_root_onto_a_processed_folder_is_refused() {
        todo!(
            "Processed folder at tree. libraries.update(root = tree), root = tree/sub, root = \
             parent of tree, root = symlinked alias of tree: each refused with \
             PROCESSED_FOLDER_OVERLAPS_LIBRARY_ROOT (or the code core picks; record it); library \
             roots unchanged."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: reverse check in location/root_scope_execution.rs"]
    async fn reverse_a_root_relocation_onto_a_processed_folder_is_refused() {
        todo!(
            "Relocate an existing library root (root_scope_execution path) onto tree and onto a \
             symlinked alias of tree; refused; root unchanged."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: reverse check in settings/runtime/recycle_bin.rs"]
    async fn reverse_a_recycle_bin_onto_a_processed_folder_is_refused() {
        todo!(
            "update_recycle_bin_settings with custom path tree, tree/sub, and a symlinked alias of \
             tree; refused; settings unchanged."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: scan-start re-check"]
    async fn a_rule_broken_after_create_is_caught_at_scan_start() {
        todo!(
            "Create folder at tree; then (bypassing reverse checks via the repository) make the \
             library root equal to tree, or delete the directory. scan_processed_folder refuses \
             with the overlap or PATH_MISSING code and writes no entries."
        )
    }
}

/// Contract section 8.7: disabled by default and what a disabled folder
/// allows.
mod disabled {
    // Add `use super::*;` when the first body here stops being a todo.

    #[tokio::test]
    #[ignore = "pending pf/pr1-base: config.rs"]
    async fn a_new_folder_is_created_disabled() {
        todo!("create; folder.enabled == false; scan refused with PROCESSED_FOLDER_DISABLED.")
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: actions"]
    async fn a_disabled_folder_refuses_scan_import_and_retry_and_allows_preview_and_ignore() {
        todo!(
            "Enabled folder with a failed entry; disable. scan -> PROCESSED_FOLDER_DISABLED; \
             importAs -> PROCESSED_FOLDER_DISABLED; retry -> PROCESSED_FOLDER_DISABLED; preview ok; \
             ignore ok; importer.calls.imports() unchanged."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: scan and importer"]
    async fn disabling_does_not_cancel_or_steal_an_import_in_flight() {
        todo!(
            "V2.2: gate an import on a Notify so the entry is importing, then disable the folder. \
             Disabling is allowed. Release the gate: the in-flight import finishes and writes its \
             result. A new claim after disable changes 0 rows (V2.1). Scan, import-as, and retry \
             return PROCESSED_FOLDER_DISABLED; preview and ignore still work."
        )
    }
}

/// Contract section 8.8 (app layer): every use-case entry point re-checks
/// `ManageSystemSettings`. The GraphQL side is in the integration file.
mod authorization {
    use super::*;

    #[tokio::test]
    #[ignore = "pending pf/pr1-base: AppUseCase processed-folder API"]
    async fn every_use_case_entry_point_refuses_a_user_without_manage_system_settings() {
        let denied = user_with(&[
            scryer_domain::AppPermission::ManageCatalogSettings,
            scryer_domain::AppPermission::ManageUsers,
        ]);
        assert!(
            !denied
                .authorization
                .has_app_permission(scryer_domain::AppPermission::ManageSystemSettings)
        );
        todo!(
            "With `denied`, call each of list_processed_folders, get_processed_folder, \
             list_processed_folder_entries, preview_processed_folder_scan, \
             create_processed_folder, update_processed_folder, delete_processed_folder, \
             set_processed_folder_enabled, scan_processed_folder, import_processed_folder_entry, \
             retry_processed_folder_entry, ignore_processed_folder_entry. Each returns \
             AppError::Unauthorized (or the variant require_app_permission yields) and changes \
             nothing (folder list and entry rows equal before and after; tree unchanged)."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: actions"]
    async fn import_as_rechecks_resolve_imports_on_the_title() {
        todo!(
            "User with ManageSystemSettings but library permissions without ResolveImports for \
             the folder library. importAs refused by trigger_manual_import's ResolveImports check; \
             entry unchanged (or back to its prior state)."
        )
    }
}

/// Contract section 8.10: delete touches configuration and ledger only.
mod delete_folder {
    // Add `use super::*;` when the first body here stops being a todo.

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: scan and importer (base config.rs for delete itself)"]
    async fn delete_leaves_every_file_byte_identical() {
        todo!(
            "Materialize every ALL_LAYOUTS plus build_symlink_layout into one tree; import one \
             file; let before = tree.snapshot() and let lib_before = TreeSnapshot::capture(library \
             root). delete_processed_folder. Assert before.assert_unchanged(&tree.snapshot()) and \
             lib_before unchanged; get_processed_folder -> None; its entries are gone (cascade, \
             risk 15); the media file and the import row remain."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: importer in_flight set"]
    async fn delete_is_refused_while_an_entry_is_in_flight() {
        todo!(
            "V2.3: under the scan lock, delete runs orphan recovery first. An entry still importing \
             after recovery refuses with PROCESSED_FOLDER_IMPORTS_IN_FLIGHT and the folder stays. \
             Release, wait for imported, delete again: one transaction runs DELETE FROM \
             processed_folder_entries WHERE folder_id = ? and then deletes the folder. Entries are \
             gone even when SQLite foreign_keys is off. Nothing on disk is touched."
        )
    }
}

/// Contract risks 13 and 14.
mod risks {
    // Add `use super::*;` when the first body here stops being a todo.

    /// Risk 13: a `Rejected` result with a blocklist disposition must not
    /// blocklist a processed-folder file stem or reopen search.
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: importer (risk 13 disposition for processed_folder)"]
    async fn a_rejected_processed_file_adds_no_blocklist_entry_and_reopens_no_search() {
        todo!(
            "Monitored title; probe override that makes post_download_gate reject the file with \
             a blocklist disposition (post_download_gate.rs:2190, 2222). RUN. Entry skipped/rejected. \
             Assert: no blocklist row whose release name equals the file stem; no failed release \
             attempt recorded for the title; no search reopened (no wanted/search queue item \
             added); tree snapshot unchanged. If any appear, core must skip the disposition for \
             processed_folder sources."
        )
    }

    /// Risk 14: copy-only needs the full source size free on the library
    /// side. Evidence from reading `import/checks.rs:81-103` at baddf1f:
    /// `disk_space_verdict` requires `source_size + 500 MiB` whatever the mode
    /// (it never assumed a free hard link), and an unmeasurable volume is
    /// accepted. `disk_space_verdict` is private, so this test goes through
    /// the pipeline.
    #[tokio::test]
    #[ignore = "pending pf/pr1-core: importer (needs a free-space seam to fake available bytes)"]
    async fn the_free_space_check_requires_a_full_copy_for_processed_files() {
        todo!(
            "Fake available bytes on the library volume to source_size + 500 MiB - 1 (seam on \
             the free-space measurement). RUN. Entry failed (Skipped+DiskFull maps to failed, \
             2.2 row 14) with InsufficientDiskSpace in last_error; no library file. Raise to \
             source_size + 500 MiB: imports."
        )
    }
}

/// Contract constants that the base branch must match, and repo hygiene.
mod contract_and_hygiene {
    use super::*;
    use crate::security::backup_bundle::BackupTableClassification;

    fn classification_of(table: &str) -> Option<BackupTableClassification> {
        crate::BACKUP_TABLE_CATALOG
            .iter()
            .find(|entry| entry.table == table)
            .map(|entry| entry.classification)
    }

    /// Contract section 3 backup catalog. Real body: fails on ab526b42 by
    /// design, passes once api adds the two entries.
    #[test]
    #[ignore = "pending pf/pr1-base: backup catalog entries in security/backup_bundle.rs"]
    fn the_backup_catalog_classifies_both_processed_folder_tables() {
        assert_eq!(
            classification_of("processed_folders"),
            Some(BackupTableClassification::Export)
        );
        assert_eq!(
            classification_of("processed_folder_entries"),
            Some(BackupTableClassification::ResetOnRestore)
        );
        let count = crate::BACKUP_TABLE_CATALOG
            .iter()
            .filter(|entry| entry.table.starts_with("processed_folder"))
            .count();
        assert_eq!(count, 2, "exactly two processed-folder catalog rows");
    }

    /// Runs today on ab526b42: the two tables are unclassified and migration
    /// 0318 (manifest version 318) is not in the catalog yet. Once pf/pr1-base
    /// lands either, this returns and the ignored test above is the contract.
    #[test]
    fn the_baseline_catalog_does_not_yet_know_the_processed_folder_tables() {
        let manifest = crate_source("../scryer/src/db/migration_manifest.toml")
            .expect("migration manifest is readable");
        let base_landed = classification_of("processed_folders").is_some()
            || manifest.contains("0318_processed_folder_imports.sql");
        if base_landed {
            return;
        }
        assert!(
            manifest.contains("0317_user_ui_drop_glow.sql"),
            "baseline is the 0317 catalog"
        );
        assert!(
            !manifest.contains("version = 318"),
            "0318 must not be in the manifest yet"
        );
        assert_eq!(classification_of("processed_folder_entries"), None);
        assert!(
            classification_of("imports").is_some(),
            "catalog lookup works"
        );
    }

    /// Contract section 6.2: the base ships core's files as stubs returning
    /// `"not implemented"`. None may remain at final review. Real body.
    #[test]
    #[ignore = "pending pf/pr1-core: scan, stability, importer, actions replace the base stubs"]
    fn no_stub_text_remains_in_the_processed_folder_modules() {
        for file in ["actions.rs", "scan.rs", "stability.rs", "importer.rs"] {
            let relative = format!("src/import/processed_folders/{file}");
            let source = crate_source(&relative)
                .unwrap_or_else(|| panic!("{relative} is missing; the base adds it"));
            assert!(
                !source.to_ascii_lowercase().contains("not implemented"),
                "{relative} still contains stub text"
            );
            assert!(!source.contains("todo!("), "{relative} contains todo!");
            assert!(
                !source.contains("unimplemented!("),
                "{relative} contains unimplemented!"
            );
        }
    }

    #[test]
    #[ignore = "pending pf/pr1-base: processed_folder_refusal::ALL"]
    fn the_refusal_codes_match_the_contract_exactly() {
        todo!(
            "let actual: Vec<&str> = crate::processed_folder_refusal::ALL.to_vec(); \
             assert_eq!(actual, EXPECTED_REFUSAL_CODES); 26 codes, V4 order, no duplicates. \
             Each REFUSAL_TRIGGERS row has a skeleton named by that code."
        )
    }

    #[test]
    #[ignore = "pending pf/pr1-base: scryer_domain::processed_folder"]
    fn the_domain_constants_match_the_contract() {
        todo!(
            "assert_eq!(scryer_domain::PROCESSED_FOLDER_CLIENT_TYPE, EXPECTED_CLIENT_TYPE); \
             ProcessedFolderEntryStatus: as_str of every variant in SDL order == STATUSES and \
             parse(as_str(x)) == x; ProcessedFolderEntryAction allowed set per status == \
             action_allowed for every STATUSES x ACTIONS cell; \
             expected_completed_download(...).is_processed_folder_source() is true and false for \
             client_type 'qbittorrent'."
        )
    }

    #[test]
    #[ignore = "pending pf/pr1-core: fingerprint in scan.rs"]
    fn the_fingerprint_encoding_matches_the_fixture() {
        todo!(
            "Compare core's fingerprint fn for (folder_id, rel, size, mtime_ns) with \
             expected_fingerprint. If they differ, the contract never said how size and mtime \
             are encoded (decimal ASCII assumed here); record the chosen encoding in the \
             contract and update expected_fingerprint, never the other way silently."
        )
    }

    #[tokio::test]
    #[ignore = "pending pf/pr1-core: importer"]
    async fn the_synthetic_completed_download_matches_decision_2() {
        todo!(
            "Capture the CompletedDownload the importer hands to trigger_manual_import (double \
             importer sees the path; capture via a recording hook or the imports row). Assert \
             equal to expected_completed_download(...) plus ('*scryer_title_id', title.id)."
        )
    }
}
