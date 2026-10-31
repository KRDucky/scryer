import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import test from "node:test";
import { buildSchema, parse, validate } from "graphql";
import en from "../i18n/locales/en.ts";
import * as mutations from "../graphql/mutations.ts";
import * as queries from "../graphql/queries.ts";
import {
  PROCESSED_FOLDER_ACTION_KEYS,
  PROCESSED_FOLDER_ENTRY_STATUSES,
  PROCESSED_FOLDER_REASON_KEYS,
  PROCESSED_FOLDER_REFUSAL_KEYS,
  PROCESSED_FOLDER_STATUS_KEYS,
  processedFolderErrorMessage,
  processedFolderReasonLabel,
  processedFolderRefusalKey,
} from "./processed-folder-error-message.ts";

const t = (key: string) => en[key] ?? key;

const refusal = (reason: string, code = "VALIDATION_ERROR") => ({
  graphQLErrors: [{ message: "changed server wording", extensions: { code, reason } }],
});

const REPO_ROOT = new URL("../../../../", import.meta.url);
const RUST_REFUSALS = new URL("crates/scryer-application/src/processed_folder_refusal.rs", REPO_ROOT);
const RUST_REFUSAL_TEMPLATE = new URL("crates/scryer-application/src/http_release_refusal.rs", REPO_ROOT);

// The refusal codes in the PR1 contract, section 4. Kept by hand so a code
// the map gains or loses is a visible change here as well.
const CONTRACT_REFUSAL_CODES = [
  "PROCESSED_FOLDER_NOT_FOUND",
  "PROCESSED_FOLDER_ENTRY_NOT_FOUND",
  "PROCESSED_FOLDER_NAME_REQUIRED",
  "PROCESSED_FOLDER_NAME_TAKEN",
  "PROCESSED_FOLDER_PATH_NOT_ABSOLUTE",
  "PROCESSED_FOLDER_PATH_MISSING",
  "PROCESSED_FOLDER_PATH_NOT_DIRECTORY",
  "PROCESSED_FOLDER_PATH_UNRESOLVABLE",
  "PROCESSED_FOLDER_OVERLAPS_LIBRARY_ROOT",
  "PROCESSED_FOLDER_OVERLAPS_RECYCLE_BIN",
  "PROCESSED_FOLDER_OVERLAPS_BACKUP_PATH",
  "PROCESSED_FOLDER_OVERLAPS_OTHER_FOLDER",
  "PROCESSED_FOLDER_LIBRARY_NOT_FOUND",
  "PROCESSED_FOLDER_INVALID_STABILITY",
  "PROCESSED_FOLDER_INVALID_MARKER",
  "PROCESSED_FOLDER_DISABLED",
  "PROCESSED_FOLDER_SCAN_IN_PROGRESS",
  "PROCESSED_FOLDER_ENTRY_NOT_ACTIONABLE",
  "PROCESSED_FOLDER_ENTRY_CHANGED",
  "PROCESSED_FOLDER_TITLE_OUTSIDE_LIBRARY",
  "PROCESSED_FOLDER_IMPORTS_IN_FLIGHT",
];

/**
 * The code strings in a refusal module's `ALL` list. `ALL` names the
 * constants (`pub const ALL: &[&str] = &[DISABLED, ...]`), so each name is
 * resolved through its `pub const NAME: &str = "CODE";` line; a string
 * literal written straight into `ALL` is taken as is.
 */
function rustRefusalCodes(source: string): string[] {
  const constants = new Map<string, string>();
  for (const match of source.matchAll(/pub\s+const\s+([A-Z0-9_]+)\s*:\s*&(?:'static\s+)?str\s*=\s*"([^"]+)"\s*;/g)) {
    constants.set(match[1], match[2]);
  }
  const all = source.match(/pub\s+const\s+ALL\s*:\s*&\[\s*&(?:'static\s+)?str\s*\]\s*=\s*&\[([\s\S]*?)\]\s*;/);
  assert.ok(all, "the refusal module has a `pub const ALL` list");
  const body = all[1].replace(/\/\/[^\n]*/g, "");
  return body
    .split(",")
    .map((item) => item.trim())
    .filter((item) => item.length > 0)
    .map((item) => {
      const literal = item.match(/^"([^"]+)"$/);
      if (literal) return literal[1];
      const code = constants.get(item);
      assert.ok(code, `ALL names ${item}, which is not a string constant in the module`);
      return code;
    });
}

test("each processed-folder refusal is translated whatever the server says", () => {
  for (const [reason, key] of Object.entries(PROCESSED_FOLDER_REFUSAL_KEYS)) {
    assert.ok(en[key], `${key} has English text`);
    assert.equal(processedFolderRefusalKey(refusal(reason)), key);
    assert.equal(processedFolderErrorMessage(refusal(reason), t, "failed"), en[key]);
  }
});

test("a processed-folder reason this client does not know shows the server sentence", () => {
  assert.equal(processedFolderRefusalKey(refusal("PROCESSED_FOLDER_FUTURE_CONDITION")), null);
  assert.equal(
    processedFolderErrorMessage(refusal("PROCESSED_FOLDER_FUTURE_CONDITION"), t, "failed"),
    "changed server wording",
  );
});

test("a reason on a non-validation error is not read as a processed-folder refusal", () => {
  assert.equal(
    processedFolderErrorMessage(refusal("PROCESSED_FOLDER_DISABLED", "REPOSITORY_ERROR"), t, "failed"),
    "changed server wording",
  );
});

test("an error with no usable sentence falls back to the caller's text", () => {
  assert.equal(processedFolderErrorMessage(null, t, "failed"), "failed");
});

test("the refusal map holds exactly the contract's codes", () => {
  assert.deepEqual(Object.keys(PROCESSED_FOLDER_REFUSAL_KEYS).sort(), [...CONTRACT_REFUSAL_CODES].sort());
  assert.equal(new Set(Object.values(PROCESSED_FOLDER_REFUSAL_KEYS)).size, CONTRACT_REFUSAL_CODES.length);
});

test("the refusal parser reads the http-release module it is modelled on", (context) => {
  if (!existsSync(RUST_REFUSAL_TEMPLATE)) {
    context.skip("crates/scryer-application/src/http_release_refusal.rs is not in this checkout");
    return;
  }
  const codes = rustRefusalCodes(readFileSync(RUST_REFUSAL_TEMPLATE, "utf8"));
  assert.ok(codes.length > 0);
  assert.ok(codes.every((code) => code.startsWith("HTTP_RELEASE_")), codes.join(", "));
  assert.equal(new Set(codes).size, codes.length);
});

test("the refusal map matches the server's ALL list", (context) => {
  if (!existsSync(RUST_REFUSALS)) {
    const message =
      "crates/scryer-application/src/processed_folder_refusal.rs is not in this checkout yet; " +
      "skipping the server ALL comparison (the contract list above still applies)";
    console.log(message);
    context.skip(message);
    return;
  }
  const codes = rustRefusalCodes(readFileSync(RUST_REFUSALS, "utf8"));
  assert.deepEqual([...new Set(codes)].sort(), Object.keys(PROCESSED_FOLDER_REFUSAL_KEYS).sort());
});

test("every status, action and reason has English text", () => {
  assert.deepEqual(Object.keys(PROCESSED_FOLDER_STATUS_KEYS), [...PROCESSED_FOLDER_ENTRY_STATUSES]);
  for (const key of [
    ...Object.values(PROCESSED_FOLDER_STATUS_KEYS),
    ...Object.values(PROCESSED_FOLDER_ACTION_KEYS),
    ...Object.values(PROCESSED_FOLDER_REASON_KEYS),
  ]) {
    assert.ok(en[key], `${key} has English text`);
  }
});

test("a known reason code is translated and an unknown one shows as sent", () => {
  assert.equal(processedFolderReasonLabel("unmatched", t), en["processedFolder.reason.unmatched"]);
  assert.equal(processedFolderReasonLabel("some_new_reason", t), "some_new_reason");
  assert.equal(processedFolderReasonLabel(null, t), null);
});

// The PR1 contract's SDL (section 4), used to check the documents until the
// API schema in this checkout carries the processed-folder types.
const CONTRACT_SDL = `
enum ProcessedFolderMarkerMode { NONE SIBLING_SUFFIX DIRECTORY_FILE }
enum ProcessedFolderEntryStatus { OBSERVING STABLE IMPORTING IMPORTED SKIPPED FAILED NEEDS_REVIEW IGNORED REJECTED SUPERSEDED GONE }
enum ProcessedFolderEntryAction { IMPORT_AS RETRY IGNORE }
type ProcessedFolder {
  id: ID! name: String! rootPath: String! libraryId: ID! libraryName: String
  enabled: Boolean! minStableScans: Int! minAgeSeconds: Int!
  markerMode: ProcessedFolderMarkerMode! markerValue: String
  lastScanStartedAt: DateTime lastScanFinishedAt: DateTime
  counts: ProcessedFolderEntryCounts!
}
type ProcessedFolderEntryCounts { observing: Int! stable: Int! importing: Int! imported: Int! skipped: Int! failed: Int! needsReview: Int! ignored: Int! rejected: Int! }
type ProcessedFolderEntry {
  id: ID! folderId: ID! relativePath: String! sizeBytes: Int! modifiedAt: DateTime!
  status: ProcessedFolderEntryStatus! reasonCode: String
  stableScanCount: Int! firstSeenAt: DateTime! lastSeenAt: DateTime! markerSeen: Boolean!
  titleId: ID importId: ID attempts: Int! lastError: String
  eligibleAt: DateTime
  allowedActions: [ProcessedFolderEntryAction!]!
}
type ProcessedFolderEntryConnection { items: [ProcessedFolderEntry!]! totalCount: Int! }
type ProcessedFolderScanPayload {
  folder: ProcessedFolder!
  discovered: Int! becameStable: Int! importsQueued: Int! needsReview: Int! rejected: Int! gone: Int! recovered: Int!
  nextEligibleAt: DateTime
}
type ProcessedFolderPreviewPayload { entries: [ProcessedFolderPreviewEntry!]! truncated: Boolean! }
type ProcessedFolderPreviewEntry { relativePath: String! sizeBytes: Int! modifiedAt: DateTime! wouldBe: ProcessedFolderEntryStatus! reasonCode: String matchedTitleId: ID }
input CreateProcessedFolderInput { name: String! rootPath: String! libraryId: ID! minStableScans: Int minAgeSeconds: Int markerMode: ProcessedFolderMarkerMode markerValue: String }
input UpdateProcessedFolderInput { id: ID! name: String rootPath: String libraryId: ID minStableScans: Int minAgeSeconds: Int markerMode: ProcessedFolderMarkerMode markerValue: String }
type ProcessedFolderPayload { folder: ProcessedFolder! }
type DeleteProcessedFolderPayload { id: ID! }
type ProcessedFolderEntryPayload { entry: ProcessedFolderEntry! }
extend type Query {
  processedFolders: [ProcessedFolder!]!
  processedFolder(id: ID!): ProcessedFolder
  processedFolderEntries(folderId: ID!, statuses: [ProcessedFolderEntryStatus!], limit: Int! = 50, offset: Int! = 0): ProcessedFolderEntryConnection!
  previewProcessedFolderScan(id: ID!, limit: Int! = 200): ProcessedFolderPreviewPayload!
}
extend type Mutation {
  createProcessedFolder(input: CreateProcessedFolderInput!): ProcessedFolderPayload!
  updateProcessedFolder(input: UpdateProcessedFolderInput!): ProcessedFolderPayload!
  deleteProcessedFolder(id: ID!): DeleteProcessedFolderPayload!
  setProcessedFolderEnabled(id: ID!, enabled: Boolean!): ProcessedFolderPayload!
  scanProcessedFolder(id: ID!): ProcessedFolderScanPayload!
  importProcessedFolderEntry(entryId: ID!, titleId: ID!): ProcessedFolderEntryPayload!
  retryProcessedFolderEntry(entryId: ID!): ProcessedFolderEntryPayload!
  ignoreProcessedFolderEntry(entryId: ID!): ProcessedFolderEntryPayload!
}
`;

test("the processed-folder documents validate against the API schema (or the contract SDL until it lands)", () => {
  const apiSchema = readFileSync(new URL("api/graphql/schema.graphql", REPO_ROOT), "utf8");
  const schemaHasFolders = /\btype\s+ProcessedFolder\b/.test(apiSchema);
  if (!schemaHasFolders) {
    console.log("api/graphql/schema.graphql has no ProcessedFolder type yet; validating against the contract SDL");
  }
  // The API names its root types QueryRoot and MutationRoot; the contract
  // shorthand writes Query and Mutation.
  const baseSchema = buildSchema(apiSchema);
  const contractSdl = CONTRACT_SDL.replace("extend type Query", `extend type ${baseSchema.getQueryType()?.name ?? "Query"}`)
    .replace("extend type Mutation", `extend type ${baseSchema.getMutationType()?.name ?? "Mutation"}`);
  const schema = schemaHasFolders ? baseSchema : buildSchema(`${apiSchema}\n${contractSdl}`);
  const documents = Object.entries({ ...queries, ...mutations }).filter(
    ([name, value]) => typeof value === "string" && /ProcessedFolder/i.test(name),
  );
  const operations = documents.filter(([name]) => /(Query|Mutation)$/.test(name));
  assert.equal(operations.length, 11, operations.map(([name]) => name).join(", "));
  const failures: string[] = [];
  for (const [name, source] of operations) {
    for (const error of validate(schema, parse(source as string))) {
      failures.push(`${name}: ${error.message}`);
    }
  }
  assert.deepEqual(failures, []);
});
