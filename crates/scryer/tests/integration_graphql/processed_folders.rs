//! PR1 processed-folder importer at the GraphQL boundary, with the real
//! `FsFileImporter` (contract sections 1.4, 2, 4, 7 and 8).
//!
//! Helper code here runs today in `fixture_self_tests`. Every test that needs
//! the PR1 resolvers is ignored with the branch that unblocks it; its body is
//! either the real assertions (they fail on baddf1f for the stated reason) or a
//! `todo!` naming the exact steps. Source trees are synthetic bytes only.

use super::*;
use scryer_domain::AppPermission;
use scryer_infrastructure_workflow::workflow::file_importer::FsFileImporter;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(super) mod fixtures {
    use super::*;

    /// Query or mutation root.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Root {
        Query,
        Mutation,
    }

    /// One contract section 4 operation: root field, root type, a minimal
    /// document. Ids are synthetic; resolvers must refuse on permission before
    /// they look anything up.
    pub struct Operation {
        pub field: &'static str,
        pub root: Root,
        pub document: &'static str,
    }

    pub const OPERATIONS: &[Operation] = &[
        Operation {
            field: "processedFolders",
            root: Root::Query,
            document: "query { processedFolders { id name enabled } }",
        },
        Operation {
            field: "processedFolder",
            root: Root::Query,
            document: r#"query { processedFolder(id: "pf-missing") { id } }"#,
        },
        Operation {
            field: "processedFolderEntries",
            root: Root::Query,
            document: r#"query { processedFolderEntries(folderId: "pf-missing") { totalCount items { id status allowedActions } } }"#,
        },
        Operation {
            field: "previewProcessedFolderScan",
            root: Root::Query,
            document: r#"query { previewProcessedFolderScan(id: "pf-missing") { truncated entries { relativePath wouldBe } } }"#,
        },
        Operation {
            field: "createProcessedFolder",
            root: Root::Mutation,
            document: r#"mutation { createProcessedFolder(input: { name: "Synthetic", rootPath: "/synthetic/processed", libraryId: "lib-missing" }) { folder { id enabled } } }"#,
        },
        Operation {
            field: "updateProcessedFolder",
            root: Root::Mutation,
            document: r#"mutation { updateProcessedFolder(input: { id: "pf-missing", name: "Renamed" }) { folder { id } } }"#,
        },
        Operation {
            field: "deleteProcessedFolder",
            root: Root::Mutation,
            document: r#"mutation { deleteProcessedFolder(id: "pf-missing") { id } }"#,
        },
        Operation {
            field: "setProcessedFolderEnabled",
            root: Root::Mutation,
            document: r#"mutation { setProcessedFolderEnabled(id: "pf-missing", enabled: true) { folder { id enabled } } }"#,
        },
        Operation {
            field: "scanProcessedFolder",
            root: Root::Mutation,
            document: r#"mutation { scanProcessedFolder(id: "pf-missing") { discovered recovered folder { id } } }"#,
        },
        Operation {
            field: "importProcessedFolderEntry",
            root: Root::Mutation,
            document: r#"mutation { importProcessedFolderEntry(entryId: "pfe-missing", titleId: "title-missing") { entry { id status } } }"#,
        },
        Operation {
            field: "retryProcessedFolderEntry",
            root: Root::Mutation,
            document: r#"mutation { retryProcessedFolderEntry(entryId: "pfe-missing") { entry { id status } } }"#,
        },
        Operation {
            field: "ignoreProcessedFolderEntry",
            root: Root::Mutation,
            document: r#"mutation { ignoreProcessedFolderEntry(entryId: "pfe-missing") { entry { id status } } }"#,
        },
    ];

    /// Section 4 type names the SDL must declare.
    pub const SDL_TYPES: &[&str] = &[
        "ProcessedFolderMarkerMode",
        "ProcessedFolderEntryStatus",
        "ProcessedFolder",
        "ProcessedFolderEntryCounts",
        "ProcessedFolderEntry",
        "ProcessedFolderEntryAction",
        "ProcessedFolderEntryConnection",
        "ProcessedFolderScanPayload",
        "ProcessedFolderPreviewPayload",
        "ProcessedFolderPreviewEntry",
        "CreateProcessedFolderInput",
        "UpdateProcessedFolderInput",
        "ProcessedFolderPayload",
        "DeleteProcessedFolderPayload",
        "ProcessedFolderEntryPayload",
    ];

    /// An actor with exactly these app permissions and no library rights.
    pub fn actor_with(name: &str, permissions: &[AppPermission]) -> User {
        let mut user = User::new_admin(name);
        user.id = Id::new().0;
        user.authorization.app = AppPermissionMask::from_permissions(permissions.iter().copied());
        user
    }

    /// Holds ManageSystemSettings only (the gate on every resolver).
    pub fn config_actor() -> User {
        actor_with("pf-config", &[AppPermission::ManageSystemSettings])
    }

    /// Every app permission except ManageSystemSettings.
    pub fn non_config_actor() -> User {
        actor_with(
            "pf-not-config",
            &[
                AppPermission::ManageUsers,
                AppPermission::ManagePermissions,
                AppPermission::ManageCatalogSettings,
                AppPermission::ManageLists,
            ],
        )
    }

    /// The refusal code a `validation_refused` error carries.
    pub fn refusal_reason(body: &Value) -> Option<&str> {
        body["errors"][0]["extensions"]["reason"].as_str()
    }

    /// The `extensions.code` of the first error, if any.
    pub fn error_code(body: &Value) -> Option<&str> {
        body["errors"][0]["extensions"]["code"].as_str()
    }

    /// App wired with the real filesystem importer, as integration_upgrade.rs.
    pub fn app_with_real_fs(ctx: &TestContext) -> scryer_application::AppUseCase {
        ctx.app.with_test_overrides(|builder| {
            builder
                .with_media_files(Arc::new(ctx.media_files.clone()))
                .with_file_importer(Arc::new(FsFileImporter::new()))
        })
    }

    /// Schema over `app_with_real_fs`.
    pub fn real_fs_schema(ctx: &TestContext) -> scryer_interface::ApiSchema {
        scryer_interface::build_schema(app_with_real_fs(ctx), ctx.auth_runtime.clone())
    }

    /// Runs `document` as `user` against `schema`.
    pub async fn exec(
        schema: &scryer_interface::ApiSchema,
        document: &str,
        user: Option<User>,
    ) -> Value {
        let mut request = async_graphql::Request::new(document);
        if let Some(user) = user {
            request = request.data(user);
        }
        serde_json::to_value(schema.execute(request).await).expect("serialize response")
    }

    /// Deterministic non-media bytes of `len` for a synthetic video name.
    pub fn synthetic_bytes(name: &str, len: usize) -> Vec<u8> {
        let seed = name.bytes().fold(0x9e37_79b9_u32, |acc, byte| {
            acc.rotate_left(5) ^ u32::from(byte)
        });
        (0..len)
            .map(|index| (seed.wrapping_mul(index as u32 + 1) >> 7) as u8)
            .collect()
    }

    /// A processed tree in a temp dir: `relative -> bytes`.
    pub struct SyntheticTree {
        pub dir: tempfile::TempDir,
    }

    impl SyntheticTree {
        pub fn new(files: &[(&str, usize)]) -> Self {
            let dir = tempfile::tempdir().expect("temp dir");
            for (relative, len) in files {
                let path = dir.path().join(relative);
                std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
                std::fs::write(&path, synthetic_bytes(relative, *len)).expect("write");
            }
            Self { dir }
        }

        pub fn root(&self) -> &Path {
            self.dir.path()
        }

        pub fn snapshot(&self) -> TreeState {
            TreeState::capture(self.root())
        }
    }

    /// Per file: bytes, mtime and (unix) inode and link count.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct FileState {
        pub bytes: Vec<u8>,
        pub modified: std::time::SystemTime,
        pub inode: u64,
        pub links: u64,
    }

    /// Every file under a root, keyed by relative path.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct TreeState(pub BTreeMap<PathBuf, FileState>);

    impl TreeState {
        pub fn capture(root: &Path) -> Self {
            let mut files = BTreeMap::new();
            let mut pending = vec![root.to_path_buf()];
            while let Some(dir) = pending.pop() {
                for entry in std::fs::read_dir(&dir).expect("read dir") {
                    let entry = entry.expect("dir entry");
                    let path = entry.path();
                    let meta = std::fs::symlink_metadata(&path).expect("metadata");
                    if meta.is_dir() {
                        pending.push(path);
                    } else if meta.is_file() {
                        let relative = path.strip_prefix(root).expect("under root").to_path_buf();
                        files.insert(
                            relative,
                            FileState {
                                bytes: std::fs::read(&path).expect("read"),
                                modified: meta.modified().expect("mtime"),
                                inode: inode(&meta),
                                links: links(&meta),
                            },
                        );
                    }
                }
            }
            Self(files)
        }

        /// The copy-only invariant: nothing under the source changed.
        pub fn assert_unchanged(&self, root: &Path) {
            assert_eq!(
                &Self::capture(root),
                self,
                "the processed source was modified"
            );
        }
    }

    #[cfg(unix)]
    fn inode(meta: &std::fs::Metadata) -> u64 {
        std::os::unix::fs::MetadataExt::ino(meta)
    }

    #[cfg(unix)]
    fn links(meta: &std::fs::Metadata) -> u64 {
        std::os::unix::fs::MetadataExt::nlink(meta)
    }

    #[cfg(not(unix))]
    fn inode(_meta: &std::fs::Metadata) -> u64 {
        0
    }

    #[cfg(not(unix))]
    fn links(_meta: &std::fs::Metadata) -> u64 {
        1
    }
}

use fixtures::*;

mod fixture_self_tests {
    use super::*;

    #[test]
    fn every_operation_document_parses_and_names_its_root_field() {
        let mut fields = std::collections::BTreeSet::new();
        for operation in OPERATIONS {
            async_graphql::parser::parse_query(operation.document)
                .unwrap_or_else(|error| panic!("{}: {error}", operation.field));
            let prefix = match operation.root {
                Root::Query => "query {",
                Root::Mutation => "mutation {",
            };
            assert!(
                operation.document.starts_with(prefix),
                "{}",
                operation.field
            );
            assert!(
                operation.document[prefix.len()..]
                    .trim_start()
                    .starts_with(operation.field),
                "{} is the root field",
                operation.field
            );
            assert!(
                fields.insert(operation.field),
                "duplicate {}",
                operation.field
            );
        }
        let queries = OPERATIONS
            .iter()
            .filter(|op| op.root == Root::Query)
            .count();
        assert_eq!((queries, OPERATIONS.len() - queries), (4, 8));
        assert_eq!(SDL_TYPES.len(), 15);
    }

    #[test]
    fn the_actors_split_on_manage_system_settings() {
        assert!(
            config_actor()
                .authorization
                .app
                .contains(AppPermissionMask::from_permission(
                    AppPermission::ManageSystemSettings
                ))
        );
        let other = non_config_actor();
        assert!(
            !other
                .authorization
                .app
                .contains(AppPermissionMask::from_permission(
                    AppPermission::ManageSystemSettings
                ))
        );
        assert!(
            other
                .authorization
                .app
                .contains(AppPermissionMask::from_permission(
                    AppPermission::ManageCatalogSettings
                ))
        );
    }

    #[test]
    fn refusal_reason_reads_extensions_reason() {
        let body = json!({"errors": [{"message": "validation: x",
            "extensions": {"code": "VALIDATION_ERROR", "reason": "PROCESSED_FOLDER_DISABLED"}}]});
        assert_eq!(refusal_reason(&body), Some("PROCESSED_FOLDER_DISABLED"));
        assert_eq!(error_code(&body), Some("VALIDATION_ERROR"));
        assert_eq!(refusal_reason(&json!({"data": {}})), None);
    }

    #[test]
    fn the_tree_snapshot_sees_content_and_link_changes() {
        let tree = SyntheticTree::new(&[("a/Synthetic.Movie.2020.mkv", 4096), ("b/x.mkv", 10)]);
        let before = tree.snapshot();
        assert_eq!(before.0.len(), 2);
        before.assert_unchanged(tree.root());
        assert!(
            before
                .0
                .values()
                .all(|file| file.links == 1 || cfg!(not(unix)))
        );

        let movie = tree.root().join("a/Synthetic.Movie.2020.mkv");
        std::fs::hard_link(&movie, tree.dir.path().join("linked.mkv")).expect("hard link");
        assert_ne!(tree.snapshot(), before, "a new link is a change");
        std::fs::remove_file(tree.root().join("linked.mkv")).expect("unlink");
        before.assert_unchanged(tree.root());

        std::fs::write(&movie, synthetic_bytes("other", 4096)).expect("rewrite");
        assert_ne!(
            tree.snapshot().0,
            before.0,
            "same size new bytes is a change"
        );
    }

    #[test]
    fn synthetic_bytes_are_deterministic_and_name_dependent() {
        assert_eq!(synthetic_bytes("a", 64), synthetic_bytes("a", 64));
        assert_ne!(synthetic_bytes("a", 64), synthetic_bytes("b", 64));
        assert_eq!(synthetic_bytes("a", 0).len(), 0);
    }

    #[tokio::test]
    async fn the_real_fs_schema_builds_and_answers() {
        let ctx = TestContext::new().await;
        let schema = real_fs_schema(&ctx);
        let body = exec(&schema, "{ __typename }", Some(config_actor())).await;
        assert_no_errors(&body);
        assert!(body["data"]["__typename"].is_string(), "{body}");
    }
}

/// Section 4: the SDL declares every contract type and operation. Description
/// and em dash rules are enforced schema-wide by schema_contract.rs:63.
#[tokio::test]
#[ignore = "pending pf/pr1-base: section 4 SDL"]
async fn the_sdl_declares_every_contract_type_and_operation() {
    let ctx = TestContext::new().await;
    let sdl = schema_sdl(&ctx);
    for name in SDL_TYPES {
        let declared = ["type", "enum", "input"].iter().any(|kind| {
            sdl.contains(&format!("{kind} {name} ")) || sdl.contains(&format!("{kind} {name}{{"))
        });
        assert!(declared, "SDL lacks {name}");
    }
    for operation in OPERATIONS {
        assert!(
            sdl.contains(&format!("{}(", operation.field))
                || sdl.contains(&format!("{}:", operation.field)),
            "SDL lacks {}",
            operation.field
        );
    }
    assert!(!sdl.contains('\u{2014}'), "no em dash anywhere in the SDL");
}

/// Section 4 auth and section 8: every resolver refuses an actor without
/// ManageSystemSettings, and an anonymous request, before any lookup.
#[tokio::test]
#[ignore = "pending pf/pr1-base: section 4 resolvers"]
async fn every_operation_refuses_an_actor_without_manage_system_settings() {
    let ctx = TestContext::new().await;
    let sdl = schema_sdl(&ctx);
    for operation in OPERATIONS {
        assert!(
            sdl.contains(operation.field),
            "{} must exist, or the refusal below is only an unknown field",
            operation.field
        );
        for user in [Some(non_config_actor()), None] {
            let anonymous = user.is_none();
            let body = schema_exec(&ctx, operation.document, user).await;
            assert_graphql_field_denied(&body, operation.field);
            assert!(
                !matches!(refusal_reason(&body), Some(reason) if reason.starts_with("PROCESSED_FOLDER_")),
                "{} (anonymous {anonymous}) leaked a lookup result before the permission check: {body}",
                operation.field
            );
        }
    }
}

/// Section 4 auth: a ManageSystemSettings actor gets past the gate; the
/// synthetic ids then fail only with a processed-folder refusal.
#[tokio::test]
#[ignore = "pending pf/pr1-base: section 4 resolvers"]
async fn every_operation_admits_a_config_actor() {
    let ctx = TestContext::new().await;
    for operation in OPERATIONS {
        let body = schema_exec(&ctx, operation.document, Some(config_actor())).await;
        if body.get("errors").is_some() {
            let reason = refusal_reason(&body).unwrap_or_default();
            assert!(
                reason.starts_with("PROCESSED_FOLDER_"),
                "{} refused a config actor for a reason other than a contract refusal: {body}",
                operation.field
            );
        }
    }
}

/// Section 4: importProcessedFolderEntry re-checks ResolveImports per title.
#[tokio::test]
#[ignore = "pending pf/pr1-core: trigger_manual_import wiring"]
async fn import_as_rechecks_resolve_imports_on_the_target_title() {
    todo!(
        "actor_with(ManageSystemSettings) and library perms without ResolveImports; create + enable a folder \
         over a SyntheticTree on the default library; scan until an entry is needs_review; \
         importProcessedFolderEntry(entry, title) must be denied (no import row, entry stays needs_review), \
         and the same call by config_actor() with ResolveImports succeeds"
    )
}

/// Section 3 and 8: a created folder is disabled and a disabled folder
/// refuses scan, import and retry, but allows preview and ignore.
#[tokio::test]
#[ignore = "pending pf/pr1-base: createProcessedFolder"]
async fn a_new_folder_is_disabled_and_refuses_scan_until_enabled() {
    todo!(
        "configure_default_library_root; SyntheticTree with one video; createProcessedFolder(rootPath = tree root) \
         -> folder.enabled == false; scanProcessedFolder -> reason PROCESSED_FOLDER_DISABLED; \
         previewProcessedFolderScan works and lists the file; tree.snapshot().assert_unchanged"
    )
}

/// Q1 (section 1.4), real importer: with the library configured Move, a
/// processed-folder import copies; the source keeps bytes, mtime, inode and
/// link count 1.
#[tokio::test]
#[ignore = "pending pf/pr1-core: scan + import pipeline"]
async fn q1_real_import_copies_and_leaves_the_source_untouched() {
    todo!(
        "app_with_real_fs; set the default library import mode to Move; default library root on the same filesystem as the SyntheticTree; folder enabled with \
         min_stable_scans 2 and an injected clock past min_age; scan twice; wait on the import (gql_until); \
         assert destination exists with equal bytes and a different inode, source links == 1, \
         before.assert_unchanged(tree root), import row strategy is copy and source_cleanup is none"
    )
}

/// Q2 (section 1.4), real importer: library on HardlinkOrCopy, same
/// filesystem, still a copy; an in-place source rewrite afterwards leaves the
/// library bytes unchanged.
#[tokio::test]
#[ignore = "pending pf/pr1-core: scan + import pipeline"]
async fn q2_same_filesystem_still_copies_not_hardlinks() {
    todo!(
        "as q1 with the library root and processed root under one tempdir (hard_link known to work: prove it with \
         std::fs::hard_link in the fixture first); library mode HardlinkOrCopy; after import the source links == 1 and \
         destination inode != source; truncate+write the source in place; library file bytes unchanged"
    )
}

/// Q3 (section 1.4, C5), real importer: an upgrade over an existing library
/// file copies and never deletes or recycles the processed source. Run once
/// with the library on Move and once on HardlinkOrCopy.
#[tokio::test]
#[ignore = "pending pf/pr1-core: upgrade path via processed folder"]
async fn q3_upgrade_import_copies_and_keeps_the_source() {
    todo!(
        "for mode in [Move, HardlinkOrCopy]: seed a title with a lower quality library file; processed tree holds a higher quality synthetic name; \
         import; old library file goes to the recycle bin as today, the processed source is byte-identical \
         (assert_unchanged) and finalize_upgrade_source_cleanup ran without removing it"
    )
}

/// Q4c (section 1.4): a cancelled or failed copy leaves no partial file and
/// the source unchanged; the entry ends failed.
#[tokio::test]
#[ignore = "pending pf/pr1-core: FsFileImporter fault hook (private seam, see test plan)"]
async fn q4c_failed_copy_leaves_source_and_no_partial_destination() {
    todo!(
        "use the core-provided fault seam to fail the copy after N bytes; entry -> failed with last_error; \
         no file at the reserved destination or its temp name; assert_unchanged on the source; retry -> stable"
    )
}

/// Q4d (Ian's decision): an empty reserved destination after a crash is not
/// auto-deleted; recovery ends the entry failed with
/// ManualReconciliationRequired.
#[tokio::test]
#[ignore = "pending pf/pr1-core: section 2.4 recovery"]
async fn q4d_empty_reserved_destination_after_crash_needs_manual_reconciliation() {
    todo!(
        "seed an importing entry with claimed_at older than the claim timeout and an import row whose reserved \
         destination exists as an empty file; scanProcessedFolder -> recovered == 1; entry status failed, \
         reason_code/last_error names ManualReconciliationRequired; the empty destination still exists; \
         assert_unchanged on the source"
    )
}

/// Q5 third case (section 1.4), real importer: a destination already present
/// with the same bytes ends imported (AlreadyPresent) with no second media
/// file.
#[tokio::test]
#[ignore = "pending pf/pr1-core: scan + import pipeline"]
async fn q5c_an_identical_destination_already_present_ends_imported() {
    todo!(
        "place a byte-identical copy of the synthetic source at the title's expected destination and register \
         no media file for it (then repeat with one registered); scan to stable and import; entry imported, \
         import result AlreadyPresent, exactly one media file row for the title, source assert_unchanged"
    )
}

/// Section 2.2 row 10, real importer: the source changes between stable and
/// the import-time re-stat; nothing is placed.
#[tokio::test]
#[ignore = "pending pf/pr1-core: version check before placement"]
async fn row_10_a_source_changed_before_placement_is_superseded() {
    todo!(
        "reach stable, then rewrite the file (new size or mtime) before the queued import runs (block the spawned \
         task via the core seam); entry -> superseded, a new observing row exists for the new version, \
         no destination file, importProcessedFolderEntry on the old row -> PROCESSED_FOLDER_ENTRY_CHANGED"
    )
}

/// Section 4: a second concurrent scan of one folder is refused.
#[tokio::test]
#[ignore = "pending pf/pr1-base: scan lock"]
async fn a_concurrent_scan_is_refused() {
    todo!(
        "hold the folder scan lock through the base-provided seam; scanProcessedFolder -> reason \
         PROCESSED_FOLDER_SCAN_IN_PROGRESS; release; scan succeeds"
    )
}

/// Section 4 and 8: delete removes config and ledger only, never files, and
/// is refused while an import is in flight.
#[tokio::test]
#[ignore = "pending pf/pr1-base: deleteProcessedFolder"]
async fn delete_keeps_files_and_is_refused_while_importing() {
    todo!(
        "folder with entries; one entry importing and in flight -> deleteProcessedFolder reason \
         PROCESSED_FOLDER_IMPORTS_IN_FLIGHT; finish import; delete succeeds; processedFolder(id) is null, \
         entries gone, source tree assert_unchanged, imported library file still present"
    )
}

/// Section 7: validation refusals reach the client as extensions.reason with
/// code VALIDATION_ERROR.
#[tokio::test]
#[ignore = "pending pf/pr1-base: section 7 validation"]
async fn validation_refusals_surface_as_extensions_reason() {
    let ctx = TestContext::new().await;
    let body = schema_exec(
        &ctx,
        r#"mutation { createProcessedFolder(input: { name: "Synthetic", rootPath: "relative/path", libraryId: "lib-missing" }) { folder { id } } }"#,
        Some(config_actor()),
    )
    .await;
    assert_eq!(error_code(&body), Some("VALIDATION_ERROR"), "{body}");
    let reason = refusal_reason(&body).unwrap_or_default();
    assert!(
        matches!(
            reason,
            "PROCESSED_FOLDER_PATH_NOT_ABSOLUTE" | "PROCESSED_FOLDER_LIBRARY_NOT_FOUND"
        ),
        "section 7 does not fix the check order; either refusal is acceptable: {body}"
    );
}
