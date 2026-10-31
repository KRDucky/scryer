import type { Translate } from "@/components/root/types";
import {
  graphQlErrorExtension,
  userFacingGraphQlErrorMessage,
} from "../graphql/error-message.ts";

/**
 * Why the server refused a processed-folder request. The server sends these
 * as `extensions.reason` on a `VALIDATION_ERROR`; each is said in the viewer's
 * language, and a reason this client does not know shows the server's
 * sentence. The server list lives in
 * crates/scryer-application/src/processed_folder_refusal.rs (`ALL`).
 */
export const PROCESSED_FOLDER_REFUSAL_KEYS: Readonly<Record<string, string>> = {
  PROCESSED_FOLDER_NOT_FOUND: "processedFolder.refusal.notFound",
  PROCESSED_FOLDER_ENTRY_NOT_FOUND: "processedFolder.refusal.entryNotFound",
  PROCESSED_FOLDER_NAME_REQUIRED: "processedFolder.refusal.nameRequired",
  PROCESSED_FOLDER_NAME_TOO_LONG: "processedFolder.refusal.nameTooLong",
  PROCESSED_FOLDER_NAME_TAKEN: "processedFolder.refusal.nameTaken",
  PROCESSED_FOLDER_FIELD_NOT_TRIMMED: "processedFolder.refusal.fieldNotTrimmed",
  PROCESSED_FOLDER_PATH_NOT_ABSOLUTE: "processedFolder.refusal.pathNotAbsolute",
  PROCESSED_FOLDER_PATH_MISSING: "processedFolder.refusal.pathMissing",
  PROCESSED_FOLDER_PATH_NOT_DIRECTORY:
    "processedFolder.refusal.pathNotDirectory",
  PROCESSED_FOLDER_PATH_UNRESOLVABLE:
    "processedFolder.refusal.pathUnresolvable",
  PROCESSED_FOLDER_OVERLAPS_LIBRARY_ROOT:
    "processedFolder.refusal.overlapsLibraryRoot",
  PROCESSED_FOLDER_OVERLAPS_RECYCLE_BIN:
    "processedFolder.refusal.overlapsRecycleBin",
  PROCESSED_FOLDER_OVERLAPS_BACKUP_PATH:
    "processedFolder.refusal.overlapsBackupPath",
  PROCESSED_FOLDER_OVERLAPS_OTHER_FOLDER:
    "processedFolder.refusal.overlapsOtherFolder",
  PROCESSED_FOLDER_LIBRARY_NOT_FOUND: "processedFolder.refusal.libraryNotFound",
  PROCESSED_FOLDER_SCOPE_KIND_INVALID:
    "processedFolder.refusal.scopeKindInvalid",
  PROCESSED_FOLDER_INVALID_STABILITY:
    "processedFolder.refusal.invalidStability",
  PROCESSED_FOLDER_INVALID_MARKER: "processedFolder.refusal.invalidMarker",
  PROCESSED_FOLDER_MARKER_VALUE_NOT_ALLOWED:
    "processedFolder.refusal.markerValueNotAllowed",
  PROCESSED_FOLDER_DISABLED: "processedFolder.refusal.disabled",
  PROCESSED_FOLDER_SCAN_IN_PROGRESS: "processedFolder.refusal.scanInProgress",
  PROCESSED_FOLDER_ENTRY_NOT_ACTIONABLE:
    "processedFolder.refusal.entryNotActionable",
  PROCESSED_FOLDER_ENTRY_CHANGED: "processedFolder.refusal.entryChanged",
  PROCESSED_FOLDER_TITLE_OUTSIDE_LIBRARY:
    "processedFolder.refusal.titleOutsideLibrary",
  PROCESSED_FOLDER_TITLE_NOT_FOUND: "processedFolder.refusal.titleNotFound",
  PROCESSED_FOLDER_IMPORTS_IN_FLIGHT: "processedFolder.refusal.importsInFlight",
};

/** The translation key for a processed-folder refusal, if the error is one this client knows. */
export function processedFolderRefusalKey(error: unknown): string | null {
  const reason = graphQlErrorExtension(error, "VALIDATION_ERROR", "reason");
  return reason !== null && Object.hasOwn(PROCESSED_FOLDER_REFUSAL_KEYS, reason)
    ? PROCESSED_FOLDER_REFUSAL_KEYS[reason]
    : null;
}

/**
 * What to show for a failed processed-folder request: the translated reason
 * when the server named one this client knows, otherwise the server's own
 * sentence, and `fallback` when the failure carries no usable sentence.
 */
export function processedFolderErrorMessage(
  error: unknown,
  t: Translate,
  fallback: string,
): string {
  const key = processedFolderRefusalKey(error);
  return key ? t(key) : userFacingGraphQlErrorMessage(error, fallback, t);
}

/** Ledger states, as the GraphQL `ProcessedFolderEntryStatus` enum names them. */
export const PROCESSED_FOLDER_ENTRY_STATUSES = [
  "OBSERVING",
  "STABLE",
  "IMPORTING",
  "IMPORTED",
  "SKIPPED",
  "FAILED",
  "NEEDS_REVIEW",
  "IGNORED",
  "REJECTED",
  "SUPERSEDED",
  "GONE",
] as const;

export type ProcessedFolderEntryStatus =
  (typeof PROCESSED_FOLDER_ENTRY_STATUSES)[number];

export const PROCESSED_FOLDER_STATUS_KEYS: Readonly<
  Record<ProcessedFolderEntryStatus, string>
> = {
  OBSERVING: "processedFolder.status.observing",
  STABLE: "processedFolder.status.stable",
  IMPORTING: "processedFolder.status.importing",
  IMPORTED: "processedFolder.status.imported",
  SKIPPED: "processedFolder.status.skipped",
  FAILED: "processedFolder.status.failed",
  NEEDS_REVIEW: "processedFolder.status.needsReview",
  IGNORED: "processedFolder.status.ignored",
  REJECTED: "processedFolder.status.rejected",
  SUPERSEDED: "processedFolder.status.superseded",
  GONE: "processedFolder.status.gone",
};

/** Operator actions, as the GraphQL `ProcessedFolderEntryAction` enum names them. */
export type ProcessedFolderEntryAction = "IMPORT_AS" | "RETRY" | "IGNORE";

export const PROCESSED_FOLDER_ACTION_KEYS: Readonly<
  Record<ProcessedFolderEntryAction, string>
> = {
  IMPORT_AS: "processedFolder.action.importAs",
  RETRY: "processedFolder.action.retry",
  IGNORE: "processedFolder.action.ignore",
};

/**
 * The fixed `reasonCode` strings a ledger entry can carry: the folder's own
 * (contract section 2, V0.2.7 and 2.4) and every `ImportSkipReason::as_str`.
 */
export const PROCESSED_FOLDER_REASON_KEYS: Readonly<Record<string, string>> = {
  unmatched: "processedFolder.reason.unmatched",
  ambiguous: "processedFolder.reason.ambiguous",
  outside_library: "processedFolder.reason.outsideLibrary",
  conflict: "processedFolder.reason.conflict",
  symlink_escapes_root: "processedFolder.reason.symlinkEscapesRoot",
  symlink_dir: "processedFolder.reason.symlinkDir",
  not_a_video: "processedFolder.reason.notAVideo",
  empty: "processedFolder.reason.empty",
  entry_changed: "processedFolder.reason.entryChanged",
  already_imported: "processedFolder.reason.alreadyImported",
  rejected: "processedFolder.reason.rejected",
  error: "processedFolder.reason.error",
  interrupted: "processedFolder.reason.interrupted",
  duplicate_file: "processedFolder.reason.duplicateFile",
  post_download_rule_blocked: "processedFolder.reason.postDownloadRuleBlocked",
  policy_mismatch: "processedFolder.reason.policyMismatch",
  unresolved_identity: "processedFolder.reason.unresolvedIdentity",
  unparseable_episode: "processedFolder.reason.unparseableEpisode",
  no_video_files: "processedFolder.reason.noVideoFiles",
  unwanted_executables: "processedFolder.reason.unwantedExecutables",
  download_in_progress: "processedFolder.reason.downloadInProgress",
  disk_full: "processedFolder.reason.diskFull",
  permission_denied: "processedFolder.reason.permissionDenied",
  password_required: "processedFolder.reason.passwordRequired",
  archive_extraction_plugin_required:
    "processedFolder.reason.archiveExtractionPluginRequired",
  archive_extraction_timed_out:
    "processedFolder.reason.archiveExtractionTimedOut",
  source_changed: "processedFolder.reason.sourceChanged",
  source_unavailable: "processedFolder.reason.sourceUnavailable",
  scene_subtitle_pending: "processedFolder.reason.sceneSubtitlePending",
  // Ledger-only reasons (contract V0.2.7 and the restart recovery in 2.4).
  skipped: "processedFolder.reason.skipped",
  failed: "processedFolder.reason.failed",
  cancelled: "processedFolder.reason.cancelled",
  file_not_found: "processedFolder.reason.fileNotFound",
  manual_reconciliation_required:
    "processedFolder.reason.manualReconciliationRequired",
};

/** The entry's reason in the viewer's language; an unknown code shows as sent. */
export function processedFolderReasonLabel(
  reasonCode: string | null | undefined,
  t: Translate,
): string | null {
  if (!reasonCode) return null;
  return Object.hasOwn(PROCESSED_FOLDER_REASON_KEYS, reasonCode)
    ? t(PROCESSED_FOLDER_REASON_KEYS[reasonCode])
    : reasonCode;
}
