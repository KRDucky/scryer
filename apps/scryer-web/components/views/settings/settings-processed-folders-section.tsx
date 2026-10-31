import { useEffect, useState } from "react";
import {
  Ban,
  CheckCircle2,
  Eye,
  FolderInput,
  FolderOpen,
  Pencil,
  Plus,
  RotateCcw,
  ScanSearch,
  Search,
  ShieldCheck,
  Trash2,
} from "lucide-react";
import { FolderBrowserDialog } from "@/components/setup/folder-browser-dialog";
import { SettingsToggleSwitch } from "@/components/common/settings-toggle-switch";
import { LoadingMark } from "@/components/common/loading-mark";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { IconButton } from "@/components/ui/icon-button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useTranslate } from "@/lib/context/translate-context";
import { useUiDateTimeFormat } from "@/lib/context/ui-settings-context";
import type { LibraryRecord } from "@/lib/types";
import type { UiDateTimeFormat } from "@/lib/types/settings";
import { formatUiDateTime } from "@/lib/utils/date-format";
import { selectorId } from "@/lib/utils/dom-ids";
import {
  PROCESSED_FOLDER_ACTION_KEYS,
  PROCESSED_FOLDER_ENTRY_STATUSES,
  PROCESSED_FOLDER_STATUS_KEYS,
  processedFolderErrorMessage,
  processedFolderReasonLabel,
  type ProcessedFolderEntryAction,
  type ProcessedFolderEntryStatus,
} from "@/lib/utils/processed-folder-error-message";

// Shapes of the GraphQL types in contract section 4 (ProcessedFolder*).

export type ProcessedFolderMarkerMode =
  "NONE" | "SIBLING_SUFFIX" | "DIRECTORY_FILE";

export type ProcessedFolderEntryCounts = {
  observing: number;
  stable: number;
  importing: number;
  imported: number;
  skipped: number;
  failed: number;
  needsReview: number;
  ignored: number;
  rejected: number;
  superseded: number;
  gone: number;
};

export type ProcessedFolder = {
  id: string;
  name: string;
  rootPath: string;
  libraryId: string;
  libraryName: string | null;
  enabled: boolean;
  minStableScans: number;
  minAgeSeconds: number;
  markerMode: ProcessedFolderMarkerMode;
  markerValue: string | null;
  lastScanStartedAt: string | null;
  lastScanFinishedAt: string | null;
  counts: ProcessedFolderEntryCounts;
};

export type ProcessedFolderEntry = {
  id: string;
  folderId: string;
  relativePath: string;
  sizeBytes: number;
  modifiedAt: string;
  status: ProcessedFolderEntryStatus;
  reasonCode: string | null;
  stableScanCount: number;
  firstSeenAt: string;
  lastSeenAt: string;
  markerSeen: boolean;
  titleId: string | null;
  importId: string | null;
  attempts: number;
  lastError: string | null;
  eligibleAt: string | null;
  allowedActions: ProcessedFolderEntryAction[];
};

export type ProcessedFolderEntryConnection = {
  items: ProcessedFolderEntry[];
  totalCount: number;
};

export type ProcessedFolderScanResult = {
  folder: ProcessedFolder;
  discovered: number;
  becameStable: number;
  importsQueued: number;
  needsReview: number;
  rejected: number;
  superseded: number;
  gone: number;
  recovered: number;
  nextEligibleAt: string | null;
};

export type ProcessedFolderPreviewEntry = {
  relativePath: string;
  sizeBytes: number;
  modifiedAt: string;
  wouldBe: ProcessedFolderEntryStatus;
  reasonCode: string | null;
  matchedTitleId: string | null;
};

export type ProcessedFolderPreview = {
  entries: ProcessedFolderPreviewEntry[];
  truncated: boolean;
};

export type ProcessedFolderFormValues = {
  name: string;
  rootPath: string;
  libraryId: string;
  minStableScans: number;
  minAgeSeconds: number;
  markerMode: ProcessedFolderMarkerMode;
  markerValue: string | null;
};

export type ProcessedFolderTitleOption = {
  id: string;
  name: string;
  facet: string;
  year?: number | null;
  libraryId: string;
};

export const PROCESSED_FOLDER_MIN_STABLE_SCANS = 2;
export const PROCESSED_FOLDER_MAX_STABLE_SCANS = 10;
export const PROCESSED_FOLDER_MIN_AGE_SECONDS = 60;
export const PROCESSED_FOLDER_MAX_AGE_SECONDS = 86400;
const DEFAULT_MIN_STABLE_SCANS = 2;
const DEFAULT_MIN_AGE_SECONDS = 300;
const STATUS_FILTER_ALL = "ALL";
const MARKER_MODES: ProcessedFolderMarkerMode[] = [
  "NONE",
  "SIBLING_SUFFIX",
  "DIRECTORY_FILE",
];

export type ProcessedFolderStatusFilter =
  ProcessedFolderEntryStatus | typeof STATUS_FILTER_ALL;

const STATUS_TONES: Record<
  ProcessedFolderEntryStatus,
  "neutral" | "positive" | "warning" | "negative" | "info" | "accent"
> = {
  OBSERVING: "info",
  STABLE: "info",
  IMPORTING: "accent",
  IMPORTED: "positive",
  SKIPPED: "neutral",
  FAILED: "negative",
  NEEDS_REVIEW: "warning",
  IGNORED: "neutral",
  REJECTED: "negative",
  SUPERSEDED: "neutral",
  GONE: "neutral",
};

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024)
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function formatDate(
  iso: string | null | undefined,
  dateTimeFormat: UiDateTimeFormat,
): string {
  return formatUiDateTime(iso, dateTimeFormat, { fallback: iso ?? "" });
}

function parseIntegerInRange(
  value: string,
  min: number,
  max: number,
): number | null {
  if (!/^\d+$/.test(value.trim())) return null;
  const parsed = Number(value.trim());
  return Number.isSafeInteger(parsed) && parsed >= min && parsed <= max
    ? parsed
    : null;
}

/** Longest folder name the server accepts, counted in Unicode scalar values. */
const PROCESSED_FOLDER_NAME_MAX_CHARS = 100;

/** The server refuses leading or trailing whitespace instead of trimming it. */
function hasOuterWhitespace(value: string): boolean {
  return value !== value.trim();
}

/** Mirrors the server's marker rule so the form can say what is wrong before saving. */
function markerValueValid(
  mode: ProcessedFolderMarkerMode,
  value: string,
): boolean {
  if (mode === "NONE") return true;
  if (value.includes("/") || value.includes("\\") || value.includes("\u0000"))
    return false;
  if (mode === "SIBLING_SUFFIX") {
    const length = [...value].length;
    return (
      value.startsWith(".") &&
      length >= 2 &&
      length <= 32 &&
      !value.includes("..")
    );
  }
  const bytes = new TextEncoder().encode(value).length;
  return bytes >= 1 && bytes <= 255 && value !== "." && value !== "..";
}

function StatusBadge({ status }: { status: ProcessedFolderEntryStatus }) {
  const t = useTranslate();
  return (
    <Badge tone={STATUS_TONES[status]}>
      {t(PROCESSED_FOLDER_STATUS_KEYS[status])}
    </Badge>
  );
}

function CopyOnlyNotice() {
  const t = useTranslate();
  return (
    <div
      id="settings-processed-folders-copy-notice"
      className="flex items-start gap-2 rounded-md border border-[var(--scry-info-border)] bg-[var(--scry-info-bg)] p-3 text-sm text-[var(--scry-info-text)]"
    >
      <ShieldCheck className="mt-0.5 h-4 w-4 shrink-0" />
      <span>{t("settings.processedFolders.copyOnlyNotice")}</span>
    </div>
  );
}

type FolderFormDialogProps = {
  open: boolean;
  folder: ProcessedFolder | null;
  libraries: LibraryRecord[];
  saving: boolean;
  error: string | null;
  onOpenChange: (open: boolean) => void;
  onSubmit: (values: ProcessedFolderFormValues) => void;
};

function FolderFormDialog({
  open,
  folder,
  libraries,
  saving,
  error,
  onOpenChange,
  onSubmit,
}: FolderFormDialogProps) {
  const t = useTranslate();
  const [name, setName] = useState(folder?.name ?? "");
  const [rootPath, setRootPath] = useState(folder?.rootPath ?? "");
  const [libraryId, setLibraryId] = useState(
    folder?.libraryId ?? libraries[0]?.id ?? "",
  );
  const [stableScansDraft, setStableScansDraft] = useState(
    String(folder?.minStableScans ?? DEFAULT_MIN_STABLE_SCANS),
  );
  const [minAgeDraft, setMinAgeDraft] = useState(
    String(folder?.minAgeSeconds ?? DEFAULT_MIN_AGE_SECONDS),
  );
  const [markerMode, setMarkerMode] = useState<ProcessedFolderMarkerMode>(
    folder?.markerMode ?? "NONE",
  );
  const [markerValue, setMarkerValue] = useState(folder?.markerValue ?? "");
  const [folderBrowserOpen, setFolderBrowserOpen] = useState(false);

  const stableScans = parseIntegerInRange(
    stableScansDraft,
    PROCESSED_FOLDER_MIN_STABLE_SCANS,
    PROCESSED_FOLDER_MAX_STABLE_SCANS,
  );
  const minAge = parseIntegerInRange(
    minAgeDraft,
    PROCESSED_FOLDER_MIN_AGE_SECONDS,
    PROCESSED_FOLDER_MAX_AGE_SECONDS,
  );
  const markerValid = markerValueValid(markerMode, markerValue);
  const nameTooLong = [...name].length > PROCESSED_FOLDER_NAME_MAX_CHARS;
  const nameUntrimmed = hasOuterWhitespace(name);
  const rootPathUntrimmed = hasOuterWhitespace(rootPath);
  const markerUntrimmed =
    markerMode !== "NONE" && hasOuterWhitespace(markerValue);
  const notTrimmedMessage = t("processedFolder.refusal.fieldNotTrimmed");
  const canSubmit =
    !saving &&
    name.trim().length > 0 &&
    !nameTooLong &&
    !nameUntrimmed &&
    rootPath.trim().length > 0 &&
    !rootPathUntrimmed &&
    !markerUntrimmed &&
    libraryId !== "" &&
    stableScans !== null &&
    minAge !== null &&
    markerValid;

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => (saving ? undefined : onOpenChange(nextOpen))}
    >
      <DialogContent
        id="settings-processed-folder-form-dialog"
        className="max-h-[90vh] overflow-y-auto sm:max-w-xl"
      >
        <DialogHeader>
          <DialogTitle>
            {folder
              ? t("settings.processedFolders.editTitle")
              : t("settings.processedFolders.addTitle")}
          </DialogTitle>
          <DialogDescription>
            {folder
              ? t("settings.processedFolders.editDescription")
              : t("settings.processedFolders.addDescription")}
          </DialogDescription>
        </DialogHeader>
        <form
          id="settings-processed-folder-form"
          className="space-y-4"
          onSubmit={(event) => {
            event.preventDefault();
            if (!canSubmit || stableScans === null || minAge === null) return;
            onSubmit({
              name,
              rootPath,
              libraryId,
              minStableScans: stableScans,
              minAgeSeconds: minAge,
              markerMode,
              markerValue: markerMode === "NONE" ? null : markerValue,
            });
          }}
        >
          <div className="space-y-1">
            <Label htmlFor="settings-processed-folder-name">
              {t("settings.processedFolders.name")}
            </Label>
            <Input
              id="settings-processed-folder-name"
              value={name}
              disabled={saving}
              aria-invalid={nameTooLong || nameUntrimmed}
              aria-describedby={
                nameTooLong || nameUntrimmed
                  ? "settings-processed-folder-name-error"
                  : undefined
              }
              onChange={(event) => setName(event.target.value)}
            />
            {nameTooLong || nameUntrimmed ? (
              <p
                id="settings-processed-folder-name-error"
                className="text-xs text-[var(--scry-danger-text)]"
              >
                {nameTooLong
                  ? t("processedFolder.refusal.nameTooLong")
                  : notTrimmedMessage}
              </p>
            ) : null}
          </div>
          <div className="space-y-1">
            <Label htmlFor="settings-processed-folder-path">
              {t("settings.processedFolders.rootPath")}
            </Label>
            <div className="flex items-center gap-2">
              <Input
                id="settings-processed-folder-path"
                value={rootPath}
                disabled={saving}
                className="font-mono"
                placeholder="/srv/media/processed"
                aria-invalid={rootPathUntrimmed}
                aria-describedby={
                  rootPathUntrimmed
                    ? "settings-processed-folder-path-error settings-processed-folder-path-help"
                    : "settings-processed-folder-path-help"
                }
                onChange={(event) => setRootPath(event.target.value)}
              />
              <Button
                type="button"
                variant="outline"
                disabled={saving}
                aria-haspopup="dialog"
                aria-label={t("settings.processedFolders.browse")}
                onClick={() => setFolderBrowserOpen(true)}
              >
                <FolderOpen className="h-4 w-4" />
              </Button>
            </div>
            {folderBrowserOpen ? (
              <FolderBrowserDialog
                open={folderBrowserOpen}
                onOpenChange={setFolderBrowserOpen}
                onSelect={(selectedPath) => setRootPath(selectedPath)}
                selectionTypes={["folder"]}
                initialPath={rootPath || "/"}
                title={t("settings.processedFolders.rootPath")}
              />
            ) : null}
            {rootPathUntrimmed ? (
              <p
                id="settings-processed-folder-path-error"
                className="text-xs text-[var(--scry-danger-text)]"
              >
                {notTrimmedMessage}
              </p>
            ) : null}
            <p
              id="settings-processed-folder-path-help"
              className="text-xs text-muted-foreground"
            >
              {t("settings.processedFolders.rootPathHelp")}
            </p>
          </div>
          <div className="space-y-1">
            <Label htmlFor="settings-processed-folder-library">
              {t("settings.processedFolders.library")}
            </Label>
            <Select
              value={libraryId}
              disabled={saving}
              onValueChange={setLibraryId}
            >
              <SelectTrigger
                id="settings-processed-folder-library"
                className="w-full"
                aria-describedby="settings-processed-folder-library-help"
              >
                <SelectValue
                  placeholder={t(
                    "settings.processedFolders.libraryPlaceholder",
                  )}
                />
              </SelectTrigger>
              <SelectContent>
                {libraries.map((library) => (
                  <SelectItem key={library.id} value={library.id}>
                    {library.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p
              id="settings-processed-folder-library-help"
              className="text-xs text-muted-foreground"
            >
              {t("settings.processedFolders.libraryHelp")}
            </p>
          </div>
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="space-y-1">
              <Label htmlFor="settings-processed-folder-min-stable-scans">
                {t("settings.processedFolders.minStableScans")}
              </Label>
              <Input
                id="settings-processed-folder-min-stable-scans"
                type="number"
                inputMode="numeric"
                min={PROCESSED_FOLDER_MIN_STABLE_SCANS}
                max={PROCESSED_FOLDER_MAX_STABLE_SCANS}
                step={1}
                value={stableScansDraft}
                disabled={saving}
                aria-invalid={stableScans === null}
                aria-describedby="settings-processed-folder-min-stable-scans-help"
                onChange={(event) => setStableScansDraft(event.target.value)}
              />
              <p
                id="settings-processed-folder-min-stable-scans-help"
                className={`text-xs ${stableScans === null ? "text-[var(--scry-danger-text)]" : "text-muted-foreground"}`}
              >
                {t("settings.processedFolders.minStableScansHelp", {
                  min: PROCESSED_FOLDER_MIN_STABLE_SCANS,
                  max: PROCESSED_FOLDER_MAX_STABLE_SCANS,
                })}
              </p>
            </div>
            <div className="space-y-1">
              <Label htmlFor="settings-processed-folder-min-age">
                {t("settings.processedFolders.minAgeSeconds")}
              </Label>
              <Input
                id="settings-processed-folder-min-age"
                type="number"
                inputMode="numeric"
                min={PROCESSED_FOLDER_MIN_AGE_SECONDS}
                max={PROCESSED_FOLDER_MAX_AGE_SECONDS}
                step={1}
                value={minAgeDraft}
                disabled={saving}
                aria-invalid={minAge === null}
                aria-describedby="settings-processed-folder-min-age-help"
                onChange={(event) => setMinAgeDraft(event.target.value)}
              />
              <p
                id="settings-processed-folder-min-age-help"
                className={`text-xs ${minAge === null ? "text-[var(--scry-danger-text)]" : "text-muted-foreground"}`}
              >
                {t("settings.processedFolders.minAgeSecondsHelp", {
                  min: PROCESSED_FOLDER_MIN_AGE_SECONDS,
                  max: PROCESSED_FOLDER_MAX_AGE_SECONDS,
                })}
              </p>
            </div>
          </div>
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="space-y-1">
              <Label htmlFor="settings-processed-folder-marker-mode">
                {t("settings.processedFolders.markerMode")}
              </Label>
              <Select
                value={markerMode}
                disabled={saving}
                onValueChange={(value) =>
                  setMarkerMode(value as ProcessedFolderMarkerMode)
                }
              >
                <SelectTrigger
                  id="settings-processed-folder-marker-mode"
                  className="w-full"
                  aria-describedby="settings-processed-folder-marker-help"
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {MARKER_MODES.map((mode) => (
                    <SelectItem key={mode} value={mode}>
                      {mode === "NONE"
                        ? t("settings.processedFolders.markerModeNone")
                        : mode === "SIBLING_SUFFIX"
                          ? t(
                              "settings.processedFolders.markerModeSiblingSuffix",
                            )
                          : t(
                              "settings.processedFolders.markerModeDirectoryFile",
                            )}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            {markerMode !== "NONE" ? (
              <div className="space-y-1">
                <Label htmlFor="settings-processed-folder-marker-value">
                  {t("settings.processedFolders.markerValue")}
                </Label>
                <Input
                  id="settings-processed-folder-marker-value"
                  value={markerValue}
                  disabled={saving}
                  className="font-mono"
                  placeholder={
                    markerMode === "SIBLING_SUFFIX" ? ".done" : ".processed"
                  }
                  aria-invalid={!markerValid || markerUntrimmed}
                  aria-describedby={
                    markerUntrimmed
                      ? "settings-processed-folder-marker-error settings-processed-folder-marker-help"
                      : "settings-processed-folder-marker-help"
                  }
                  onChange={(event) => setMarkerValue(event.target.value)}
                />
                {markerUntrimmed ? (
                  <p
                    id="settings-processed-folder-marker-error"
                    className="text-xs text-[var(--scry-danger-text)]"
                  >
                    {notTrimmedMessage}
                  </p>
                ) : null}
              </div>
            ) : null}
          </div>
          <p
            id="settings-processed-folder-marker-help"
            className={`text-xs ${markerValid ? "text-muted-foreground" : "text-[var(--scry-danger-text)]"}`}
          >
            {markerMode === "SIBLING_SUFFIX"
              ? t("settings.processedFolders.markerSiblingSuffixHelp")
              : markerMode === "DIRECTORY_FILE"
                ? t("settings.processedFolders.markerDirectoryFileHelp")
                : t("settings.processedFolders.markerNoneHelp")}
          </p>
          {folder ? null : (
            <p
              id="settings-processed-folder-created-disabled"
              className="text-xs text-muted-foreground"
            >
              {t("settings.processedFolders.createdDisabledNote")}
            </p>
          )}
          {error ? (
            <p
              id="settings-processed-folder-form-error"
              role="alert"
              className="text-sm text-[var(--scry-danger-text)]"
            >
              {error}
            </p>
          ) : null}
          <DialogFooter>
            <Button
              type="button"
              variant="secondary"
              disabled={saving}
              onClick={() => onOpenChange(false)}
            >
              {t("label.cancel")}
            </Button>
            <Button
              id="settings-processed-folder-form-save"
              type="submit"
              disabled={!canSubmit}
            >
              {saving ? <LoadingMark className="mr-2 h-4 w-4" /> : null}
              {saving
                ? t("label.saving")
                : folder
                  ? t("label.save")
                  : t("settings.processedFolders.addAction")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

const TITLE_SEARCH_DEBOUNCE_MS = 250;

type ImportAsDialogProps = {
  entry: ProcessedFolderEntry | null;
  libraryName: string;
  importing: boolean;
  onSearchTitles: (query: string) => Promise<ProcessedFolderTitleOption[]>;
  onImport: (entry: ProcessedFolderEntry, titleId: string) => void;
  onClose: () => void;
};

/**
 * Title search for "Import as...". The container's search only returns titles
 * in the folder's library; the server refuses any other title
 * (PROCESSED_FOLDER_TITLE_OUTSIDE_LIBRARY).
 */
function ImportAsDialog({
  entry,
  libraryName,
  importing,
  onSearchTitles,
  onImport,
  onClose,
}: ImportAsDialogProps) {
  const t = useTranslate();
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<ProcessedFolderTitleOption[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const trimmed = query.trim();
    if (!entry || !trimmed) {
      setResults([]);
      setError(null);
      setLoading(false);
      return undefined;
    }
    let cancelled = false;
    setLoading(true);
    setError(null);
    const timer = window.setTimeout(() => {
      onSearchTitles(trimmed)
        .then((titles) => {
          if (!cancelled) setResults(titles);
        })
        .catch((searchError: unknown) => {
          if (cancelled) return;
          setResults([]);
          setError(
            processedFolderErrorMessage(
              searchError,
              t,
              t("status.failedToLoad"),
            ),
          );
        })
        .finally(() => {
          if (!cancelled) setLoading(false);
        });
    }, TITLE_SEARCH_DEBOUNCE_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [entry, onSearchTitles, query, t]);

  return (
    <CommandDialog
      open={entry !== null}
      onOpenChange={(open) => {
        if (!open && !importing) onClose();
      }}
      title={t("settings.processedFolders.importAsTitle")}
      description={t("settings.processedFolders.importAsDescription", {
        library: libraryName,
      })}
      className="sm:max-w-2xl"
      contentId="settings-processed-folder-import-as-dialog"
      shouldFilter={false}
    >
      <div className="border-b border-border px-4 py-3 text-sm">
        <p className="font-medium">
          {t("settings.processedFolders.importAsTitle")}
        </p>
        <p className="break-all font-mono text-xs text-muted-foreground">
          {entry?.relativePath}
        </p>
        <p className="text-xs text-muted-foreground">
          {t("settings.processedFolders.importAsDescription", {
            library: libraryName,
          })}
        </p>
      </div>
      <CommandInput
        wrapperId="settings-processed-folder-import-as-search"
        value={query}
        onValueChange={setQuery}
        placeholder={t("settings.processedFolders.importAsPlaceholder")}
      />
      <CommandList>
        {error ? (
          <div className="px-4 py-3 text-sm text-[var(--scry-danger-text-soft)]">
            {error}
          </div>
        ) : null}
        {loading ? (
          <div className="flex items-center gap-2 px-4 py-3 text-sm text-muted-foreground">
            <LoadingMark className="h-4 w-4" />
            <span>{t("label.loading")}</span>
          </div>
        ) : null}
        {query.trim() ? (
          <CommandEmpty>
            {t("settings.processedFolders.importAsEmpty")}
          </CommandEmpty>
        ) : null}
        <CommandGroup>
          {results.map((title) => (
            <CommandItem
              key={title.id}
              value={title.id}
              disabled={importing}
              onSelect={() => {
                if (entry) onImport(entry, title.id);
              }}
            >
              <Search className="h-4 w-4 text-muted-foreground" />
              <div
                id={selectorId(
                  "settings-processed-folder-import-as-item",
                  title.id,
                )}
                className="flex min-w-0 flex-1 flex-col gap-0.5"
              >
                <span className="truncate font-medium text-foreground">
                  {title.name}
                </span>
                <span className="text-xs text-muted-foreground">
                  {libraryName}
                  {title.year ? ` • ${title.year}` : ""}
                </span>
              </div>
              {importing ? (
                <LoadingMark className="ml-auto h-4 w-4" />
              ) : (
                <CheckCircle2 className="ml-auto h-4 w-4 text-muted-foreground/50" />
              )}
            </CommandItem>
          ))}
        </CommandGroup>
      </CommandList>
    </CommandDialog>
  );
}

const COUNT_FIELDS: Record<
  ProcessedFolderEntryStatus,
  keyof ProcessedFolderEntryCounts
> = {
  OBSERVING: "observing",
  STABLE: "stable",
  IMPORTING: "importing",
  IMPORTED: "imported",
  SKIPPED: "skipped",
  FAILED: "failed",
  NEEDS_REVIEW: "needsReview",
  IGNORED: "ignored",
  REJECTED: "rejected",
  SUPERSEDED: "superseded",
  GONE: "gone",
};

/** One count per ledger state; choosing one filters the table to it. */
function StatusCounts({
  counts,
  statusFilter,
  onStatusFilterChange,
}: {
  counts: ProcessedFolderEntryCounts;
  statusFilter: ProcessedFolderStatusFilter;
  onStatusFilterChange: (filter: ProcessedFolderStatusFilter) => void;
}) {
  const t = useTranslate();
  return (
    <div
      id="settings-processed-folder-status-counts"
      role="group"
      aria-label={t("settings.processedFolders.countsByStatus")}
      className="space-y-1"
    >
      <p className="text-xs text-muted-foreground">
        {t("settings.processedFolders.countsByStatus")}
      </p>
      <div className="flex flex-wrap gap-1.5">
        {PROCESSED_FOLDER_ENTRY_STATUSES.map((status) => {
          const active = statusFilter === status;
          return (
            <Button
              key={status}
              id={selectorId("settings-processed-folder-count", status)}
              type="button"
              size="sm"
              variant={active ? "default" : "outline"}
              className="h-7 gap-1.5 px-2 text-xs"
              aria-pressed={active}
              onClick={() =>
                onStatusFilterChange(active ? STATUS_FILTER_ALL : status)
              }
            >
              {t(PROCESSED_FOLDER_STATUS_KEYS[status])}
              <span
                className={
                  active
                    ? "font-[var(--font-code)] opacity-80"
                    : "font-[var(--font-code)] text-muted-foreground"
                }
              >
                {counts[COUNT_FIELDS[status]]}
              </span>
            </Button>
          );
        })}
      </div>
    </div>
  );
}

function ScanResultSummary({ result }: { result: ProcessedFolderScanResult }) {
  const t = useTranslate();
  const dateTimeFormat = useUiDateTimeFormat();
  const counts: Array<[string, number]> = [
    [t("settings.processedFolders.scanDiscovered"), result.discovered],
    [t("settings.processedFolders.scanBecameStable"), result.becameStable],
    [t("settings.processedFolders.scanImportsQueued"), result.importsQueued],
    [t("settings.processedFolders.scanNeedsReview"), result.needsReview],
    [t("settings.processedFolders.scanRejected"), result.rejected],
    [t("settings.processedFolders.scanSuperseded"), result.superseded],
    [t("settings.processedFolders.scanGone"), result.gone],
    [t("settings.processedFolders.scanRecovered"), result.recovered],
  ];
  return (
    <div
      id="settings-processed-folder-scan-result"
      className="space-y-2 rounded-md border border-border bg-muted/30 p-3 text-sm"
    >
      <p className="font-medium">
        {t("settings.processedFolders.scanResultTitle")}
      </p>
      <dl className="grid grid-cols-2 gap-x-6 gap-y-1 sm:grid-cols-4">
        {counts.map(([label, value]) => (
          <div
            key={label}
            className="flex items-baseline justify-between gap-2"
          >
            <dt className="text-muted-foreground">{label}</dt>
            <dd className="font-[var(--font-code)]">{value}</dd>
          </div>
        ))}
      </dl>
      {result.nextEligibleAt ? (
        <p className="text-xs text-muted-foreground">
          {t("settings.processedFolders.nextEligibleAt", {
            time: formatDate(result.nextEligibleAt, dateTimeFormat),
          })}
        </p>
      ) : null}
    </div>
  );
}

function PreviewResults({
  preview,
  onClose,
}: {
  preview: ProcessedFolderPreview;
  onClose: () => void;
}) {
  const t = useTranslate();
  const dateTimeFormat = useUiDateTimeFormat();
  return (
    <div id="settings-processed-folder-preview" className="space-y-2">
      <div className="flex items-center justify-between gap-2">
        <div>
          <p className="text-sm font-medium">
            {t("settings.processedFolders.previewTitle")}
          </p>
          <p className="text-xs text-muted-foreground">
            {t("settings.processedFolders.previewHelp")}
          </p>
        </div>
        <Button size="sm" variant="ghost" onClick={onClose}>
          {t("label.close")}
        </Button>
      </div>
      {preview.truncated ? (
        <p
          id="settings-processed-folder-preview-truncated"
          className="text-xs text-[var(--scry-warning-text)]"
        >
          {t("settings.processedFolders.previewTruncated", {
            count: preview.entries.length,
          })}
        </p>
      ) : null}
      {preview.entries.length === 0 ? (
        <p className="py-2 text-sm text-muted-foreground">
          {t("settings.processedFolders.previewEmpty")}
        </p>
      ) : (
        <Table
          className="min-w-[44rem]"
          wrapperClassName="rounded-[14px] border border-[var(--scry-border2)] bg-[var(--scry-surfC)]"
        >
          <TableHeader>
            <TableRow>
              <TableHead>{t("settings.processedFolders.file")}</TableHead>
              <TableHead>{t("settings.processedFolders.size")}</TableHead>
              <TableHead>{t("settings.processedFolders.modified")}</TableHead>
              <TableHead>{t("settings.processedFolders.wouldBe")}</TableHead>
              <TableHead>{t("settings.processedFolders.reason")}</TableHead>
              <TableHead>
                {t("settings.processedFolders.matchedTitle")}
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {preview.entries.map((entry) => (
              <TableRow key={entry.relativePath} data-ui="settings-table-row">
                <TableCell className="max-w-[360px] break-all font-[var(--font-code)] text-xs">
                  {entry.relativePath}
                </TableCell>
                <TableCell className="whitespace-nowrap font-[var(--font-code)] text-sm text-muted-foreground">
                  {formatSize(entry.sizeBytes)}
                </TableCell>
                <TableCell className="whitespace-nowrap text-sm text-muted-foreground">
                  {formatDate(entry.modifiedAt, dateTimeFormat)}
                </TableCell>
                <TableCell>
                  <StatusBadge status={entry.wouldBe} />
                </TableCell>
                <TableCell className="text-sm text-muted-foreground">
                  {processedFolderReasonLabel(entry.reasonCode, t) ?? ""}
                </TableCell>
                <TableCell className="font-[var(--font-code)] text-xs text-muted-foreground">
                  {entry.matchedTitleId ?? ""}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </div>
  );
}

type EntryActionsProps = {
  entry: ProcessedFolderEntry;
  folderEnabled: boolean;
  busy: boolean;
  onImportAs: (entry: ProcessedFolderEntry) => void;
  onRetry: (entry: ProcessedFolderEntry) => void;
  onIgnore: (entry: ProcessedFolderEntry) => void;
};

/** Buttons come only from the server's `allowedActions`; this never decides which apply. */
function EntryActions({
  entry,
  folderEnabled,
  busy,
  onImportAs,
  onRetry,
  onIgnore,
}: EntryActionsProps) {
  const t = useTranslate();
  const disabledReason = t("settings.processedFolders.actionNeedsEnabled");
  return (
    <div className="flex flex-wrap items-center justify-end gap-1">
      {entry.allowedActions.map((action) => {
        const needsEnabled = action === "IMPORT_AS" || action === "RETRY";
        const blocked = needsEnabled && !folderEnabled;
        const label = blocked
          ? `${t(PROCESSED_FOLDER_ACTION_KEYS[action])}: ${disabledReason}`
          : t(PROCESSED_FOLDER_ACTION_KEYS[action]);
        const Icon =
          action === "IMPORT_AS"
            ? FolderInput
            : action === "RETRY"
              ? RotateCcw
              : Ban;
        return (
          <Button
            key={action}
            id={selectorId(
              "settings-processed-folder-entry",
              action.toLowerCase(),
              entry.id,
            )}
            type="button"
            size="sm"
            variant="outline"
            className="h-7 gap-1 px-2 text-xs"
            aria-label={label}
            title={label}
            disabled={busy || blocked}
            onClick={() => {
              if (action === "IMPORT_AS") onImportAs(entry);
              else if (action === "RETRY") onRetry(entry);
              else onIgnore(entry);
            }}
          >
            <Icon className="h-3.5 w-3.5" />
            {t(PROCESSED_FOLDER_ACTION_KEYS[action])}
          </Button>
        );
      })}
    </div>
  );
}

type Props = {
  canManage: boolean;
  loading: boolean;
  folders: ProcessedFolder[];
  libraries: LibraryRecord[];
  selectedFolderId: string | null;
  entries: ProcessedFolderEntryConnection | null;
  entriesLoading: boolean;
  statusFilter: ProcessedFolderStatusFilter;
  pageSize: number;
  offset: number;
  scanResult: ProcessedFolderScanResult | null;
  preview: ProcessedFolderPreview | null;
  busyKey: string | null;
  error: string | null;
  formOpen: boolean;
  formFolder: ProcessedFolder | null;
  formSaving: boolean;
  formError: string | null;
  importEntry: ProcessedFolderEntry | null;
  onSelectFolder: (folderId: string) => void;
  onAddFolder: () => void;
  onEditFolder: (folder: ProcessedFolder) => void;
  onDeleteFolder: (folder: ProcessedFolder) => void;
  onFormOpenChange: (open: boolean) => void;
  onFormSubmit: (values: ProcessedFolderFormValues) => void;
  onEnabledChange: (folder: ProcessedFolder, enabled: boolean) => void;
  onScan: (folder: ProcessedFolder) => void;
  onPreview: (folder: ProcessedFolder) => void;
  onClosePreview: () => void;
  onStatusFilterChange: (filter: ProcessedFolderStatusFilter) => void;
  onOffsetChange: (offset: number) => void;
  onImportAs: (entry: ProcessedFolderEntry) => void;
  onCloseImportAs: () => void;
  onConfirmImport: (entry: ProcessedFolderEntry, titleId: string) => void;
  onSearchTitles: (query: string) => Promise<ProcessedFolderTitleOption[]>;
  onRetry: (entry: ProcessedFolderEntry) => void;
  onIgnore: (entry: ProcessedFolderEntry) => void;
};

export function SettingsProcessedFoldersSection({
  canManage,
  loading,
  folders,
  libraries,
  selectedFolderId,
  entries,
  entriesLoading,
  statusFilter,
  pageSize,
  offset,
  scanResult,
  preview,
  busyKey,
  error,
  formOpen,
  formFolder,
  formSaving,
  formError,
  importEntry,
  onSelectFolder,
  onAddFolder,
  onEditFolder,
  onDeleteFolder,
  onFormOpenChange,
  onFormSubmit,
  onEnabledChange,
  onScan,
  onPreview,
  onClosePreview,
  onStatusFilterChange,
  onOffsetChange,
  onImportAs,
  onCloseImportAs,
  onConfirmImport,
  onSearchTitles,
  onRetry,
  onIgnore,
}: Props) {
  const t = useTranslate();
  const dateTimeFormat = useUiDateTimeFormat();
  const selectedFolder =
    folders.find((folder) => folder.id === selectedFolderId) ?? null;
  const isBusy = busyKey !== null;
  const totalCount = entries?.totalCount ?? 0;
  const pageStart = totalCount === 0 ? 0 : offset + 1;
  const pageEnd = Math.min(offset + pageSize, totalCount);

  return (
    <section id="settings-processed-folders-section" className="space-y-4">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div className="space-y-1">
          <h2 className="text-lg font-semibold text-[var(--scry-ink2)]">
            {t("settings.processedFolders.title")}
          </h2>
          <p className="text-sm text-muted-foreground">
            {t("settings.processedFolders.description")}
          </p>
        </div>
        {canManage ? (
          <Button
            id="settings-processed-folders-add"
            size="sm"
            disabled={loading || isBusy}
            onClick={onAddFolder}
          >
            <Plus className="mr-2 h-4 w-4" />
            {t("settings.processedFolders.addAction")}
          </Button>
        ) : null}
      </div>

      <CopyOnlyNotice />

      {error ? (
        <p
          id="settings-processed-folders-error"
          role="alert"
          className="rounded-md border border-[var(--scry-danger-border)] bg-[var(--scry-danger-bg)] p-3 text-sm text-[var(--scry-danger-text)]"
        >
          {error}
        </p>
      ) : null}

      {!canManage ? (
        <p className="text-sm text-muted-foreground">
          {t("settings.processedFolders.readonly")}
        </p>
      ) : loading ? (
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <LoadingMark className="h-4 w-4" />
          {t("label.loading")}
        </div>
      ) : folders.length === 0 ? (
        <p
          id="settings-processed-folders-empty"
          className="py-2 text-sm text-muted-foreground"
        >
          {t("settings.processedFolders.empty")}
        </p>
      ) : (
        <Table
          id="settings-processed-folders-table"
          className="min-w-[56rem]"
          wrapperClassName="rounded-[14px] border border-[var(--scry-border2)] bg-[var(--scry-surfC)]"
        >
          <TableHeader>
            <TableRow>
              <TableHead>{t("label.name")}</TableHead>
              <TableHead>{t("settings.processedFolders.library")}</TableHead>
              <TableHead>{t("settings.processedFolders.stability")}</TableHead>
              <TableHead>{t("settings.processedFolders.counts")}</TableHead>
              <TableHead>{t("settings.processedFolders.lastScan")}</TableHead>
              <TableHead>{t("settings.processedFolders.enabled")}</TableHead>
              <TableHead className="text-right">{t("label.actions")}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {folders.map((folder) => {
              const selected = folder.id === selectedFolderId;
              return (
                <TableRow
                  key={folder.id}
                  id={selectorId("settings-processed-folder-row", folder.id)}
                  data-ui="settings-table-row"
                  data-state={selected ? "selected" : undefined}
                  className={
                    selected
                      ? "bg-[rgba(var(--scry-accent-rgb),0.08)]"
                      : undefined
                  }
                >
                  <TableCell>
                    <button
                      type="button"
                      className="text-left"
                      onClick={() => onSelectFolder(folder.id)}
                    >
                      <div className="font-medium">{folder.name}</div>
                      <div
                        className="max-w-[320px] truncate font-[var(--font-code)] text-xs text-muted-foreground"
                        title={folder.rootPath}
                      >
                        {folder.rootPath}
                      </div>
                    </button>
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {folder.libraryName ?? folder.libraryId}
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground">
                    <div>
                      {t("settings.processedFolders.stabilitySummary", {
                        scans: folder.minStableScans,
                        seconds: folder.minAgeSeconds,
                      })}
                    </div>
                    {folder.markerMode !== "NONE" && folder.markerValue ? (
                      <div className="font-[var(--font-code)]">
                        {t("settings.processedFolders.markerSummary", {
                          marker: folder.markerValue,
                        })}
                      </div>
                    ) : null}
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground">
                    <div>
                      {t("settings.processedFolders.countsSummary", {
                        imported: folder.counts.imported,
                        needsReview: folder.counts.needsReview,
                        failed: folder.counts.failed,
                        observing: folder.counts.observing,
                      })}
                    </div>
                    {folder.counts.superseded + folder.counts.gone > 0 ? (
                      <div
                        id={selectorId(
                          "settings-processed-folder-counts-retired",
                          folder.id,
                        )}
                      >
                        {t("settings.processedFolders.countsRetired", {
                          superseded: folder.counts.superseded,
                          gone: folder.counts.gone,
                        })}
                      </div>
                    ) : null}
                  </TableCell>
                  <TableCell className="whitespace-nowrap text-xs text-muted-foreground">
                    {folder.lastScanFinishedAt
                      ? formatDate(folder.lastScanFinishedAt, dateTimeFormat)
                      : t("settings.processedFolders.neverScanned")}
                  </TableCell>
                  <TableCell>
                    <SettingsToggleSwitch
                      id={selectorId(
                        "settings-processed-folder-enabled",
                        folder.id,
                      )}
                      checked={folder.enabled}
                      disabled={isBusy}
                      ariaLabel={t("settings.processedFolders.enabledAria", {
                        name: folder.name,
                      })}
                      onChange={(enabled) => onEnabledChange(folder, enabled)}
                    />
                  </TableCell>
                  <TableCell className="text-right">
                    <div className="flex items-center justify-end gap-1">
                      <IconButton
                        id={selectorId(
                          "settings-processed-folder-open",
                          folder.id,
                        )}
                        label={t("settings.processedFolders.showEntries")}
                        tone="accent"
                        disabled={isBusy}
                        onClick={() => onSelectFolder(folder.id)}
                      >
                        <Eye className="h-4 w-4" />
                      </IconButton>
                      <IconButton
                        id={selectorId(
                          "settings-processed-folder-edit",
                          folder.id,
                        )}
                        label={t("label.edit")}
                        tone="edit"
                        disabled={isBusy}
                        onClick={() => onEditFolder(folder)}
                      >
                        <Pencil className="h-4 w-4" />
                      </IconButton>
                      <IconButton
                        id={selectorId(
                          "settings-processed-folder-delete",
                          folder.id,
                        )}
                        label={t("label.delete")}
                        tone="delete"
                        disabled={isBusy}
                        onClick={() => onDeleteFolder(folder)}
                      >
                        <Trash2 className="h-4 w-4" />
                      </IconButton>
                    </div>
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      )}

      {canManage && selectedFolder ? (
        <div
          id="settings-processed-folder-detail"
          className="space-y-4 rounded-[14px] border border-[var(--scry-border2)] p-4"
        >
          <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <div className="min-w-0">
              <p className="font-medium">{selectedFolder.name}</p>
              <p className="truncate font-[var(--font-code)] text-xs text-muted-foreground">
                {selectedFolder.rootPath}
              </p>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <Button
                id="settings-processed-folder-preview-action"
                size="sm"
                variant="outline"
                disabled={isBusy}
                onClick={() => onPreview(selectedFolder)}
              >
                {busyKey === `preview:${selectedFolder.id}` ? (
                  <LoadingMark className="mr-2 h-4 w-4" />
                ) : (
                  <Eye className="mr-2 h-4 w-4" />
                )}
                {t("settings.processedFolders.preview")}
              </Button>
              <Button
                id="settings-processed-folder-scan-action"
                size="sm"
                disabled={isBusy || !selectedFolder.enabled}
                title={
                  selectedFolder.enabled
                    ? undefined
                    : t("settings.processedFolders.actionNeedsEnabled")
                }
                onClick={() => onScan(selectedFolder)}
              >
                {busyKey === `scan:${selectedFolder.id}` ? (
                  <LoadingMark className="mr-2 h-4 w-4" />
                ) : (
                  <ScanSearch className="mr-2 h-4 w-4" />
                )}
                {t("settings.processedFolders.scanNow")}
              </Button>
            </div>
          </div>

          {selectedFolder.enabled ? null : (
            <p
              id="settings-processed-folder-disabled-notice"
              className="rounded-md border border-[var(--scry-warning-border)] bg-[var(--scry-warning-bg)] p-3 text-sm text-[var(--scry-warning-text)]"
            >
              {t("settings.processedFolders.disabledNotice")}
            </p>
          )}

          <p className="text-xs text-muted-foreground">
            {t("settings.processedFolders.manualScanHelp")}
          </p>

          {scanResult && scanResult.folder.id === selectedFolder.id ? (
            <ScanResultSummary result={scanResult} />
          ) : null}
          {preview ? (
            <PreviewResults preview={preview} onClose={onClosePreview} />
          ) : null}

          <StatusCounts
            counts={selectedFolder.counts}
            statusFilter={statusFilter}
            onStatusFilterChange={onStatusFilterChange}
          />

          <div className="space-y-2">
            <div className="flex flex-col gap-2 sm:flex-row sm:items-end sm:justify-between">
              <p className="text-sm font-medium">
                {t("settings.processedFolders.entriesTitle")}
              </p>
              <div className="space-y-1">
                <Label htmlFor="settings-processed-folder-status-filter">
                  {t("settings.processedFolders.statusFilter")}
                </Label>
                <Select
                  value={statusFilter}
                  onValueChange={(value) =>
                    onStatusFilterChange(value as ProcessedFolderStatusFilter)
                  }
                >
                  <SelectTrigger
                    id="settings-processed-folder-status-filter"
                    className="w-56"
                  >
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value={STATUS_FILTER_ALL}>
                      {t("settings.processedFolders.statusAll")}
                    </SelectItem>
                    {PROCESSED_FOLDER_ENTRY_STATUSES.map((status) => (
                      <SelectItem key={status} value={status}>
                        {t(PROCESSED_FOLDER_STATUS_KEYS[status])}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </div>

            {entriesLoading ? (
              <div className="flex items-center gap-2 py-4 text-sm text-muted-foreground">
                <LoadingMark className="h-4 w-4" />
                {t("label.loading")}
              </div>
            ) : !entries || entries.items.length === 0 ? (
              <p
                id="settings-processed-folder-entries-empty"
                className="py-4 text-sm text-muted-foreground"
              >
                {t("settings.processedFolders.entriesEmpty")}
              </p>
            ) : (
              <Table
                id="settings-processed-folder-entries"
                className="min-w-[60rem]"
                wrapperClassName="rounded-[14px] border border-[var(--scry-border2)] bg-[var(--scry-surfC)]"
              >
                <TableHeader>
                  <TableRow>
                    <TableHead>{t("settings.processedFolders.file")}</TableHead>
                    <TableHead>{t("label.status")}</TableHead>
                    <TableHead>
                      {t("settings.processedFolders.reason")}
                    </TableHead>
                    <TableHead>
                      {t("settings.processedFolders.progress")}
                    </TableHead>
                    <TableHead>
                      {t("settings.processedFolders.attempts")}
                    </TableHead>
                    <TableHead className="text-right">
                      {t("label.actions")}
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {entries.items.map((entry) => (
                    <TableRow
                      key={entry.id}
                      id={selectorId(
                        "settings-processed-folder-entry-row",
                        entry.id,
                      )}
                      data-ui="settings-table-row"
                    >
                      <TableCell>
                        <div className="max-w-[360px] break-all font-[var(--font-code)] text-xs">
                          {entry.relativePath}
                        </div>
                        <div className="text-xs text-muted-foreground">
                          {formatSize(entry.sizeBytes)} ·{" "}
                          {formatDate(entry.modifiedAt, dateTimeFormat)}
                        </div>
                      </TableCell>
                      <TableCell>
                        <StatusBadge status={entry.status} />
                      </TableCell>
                      <TableCell className="max-w-[280px] text-sm text-muted-foreground">
                        <div>
                          {processedFolderReasonLabel(entry.reasonCode, t) ??
                            ""}
                        </div>
                        {entry.lastError ? (
                          <div
                            className="break-words text-xs text-[var(--scry-danger-text-soft)]"
                            title={entry.lastError}
                          >
                            {entry.lastError}
                          </div>
                        ) : null}
                      </TableCell>
                      <TableCell className="text-xs text-muted-foreground">
                        <div>
                          {t("settings.processedFolders.stableScanCount", {
                            count: entry.stableScanCount,
                          })}
                        </div>
                        {entry.status === "OBSERVING" && entry.eligibleAt ? (
                          <div>
                            {t("settings.processedFolders.eligibleAt", {
                              time: formatDate(
                                entry.eligibleAt,
                                dateTimeFormat,
                              ),
                            })}
                          </div>
                        ) : null}
                      </TableCell>
                      <TableCell className="font-[var(--font-code)] text-sm text-muted-foreground">
                        {entry.attempts}
                      </TableCell>
                      <TableCell className="text-right">
                        <EntryActions
                          entry={entry}
                          folderEnabled={selectedFolder.enabled}
                          busy={isBusy}
                          onImportAs={onImportAs}
                          onRetry={onRetry}
                          onIgnore={onIgnore}
                        />
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            )}

            {totalCount > 0 ? (
              <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
                <span id="settings-processed-folder-entries-range">
                  {t("settings.processedFolders.pageRange", {
                    start: pageStart,
                    end: pageEnd,
                    total: totalCount,
                  })}
                </span>
                <div className="flex gap-2">
                  <Button
                    id="settings-processed-folder-entries-prev"
                    size="sm"
                    variant="outline"
                    disabled={entriesLoading || offset === 0}
                    onClick={() =>
                      onOffsetChange(Math.max(0, offset - pageSize))
                    }
                  >
                    {t("settings.processedFolders.previousPage")}
                  </Button>
                  <Button
                    id="settings-processed-folder-entries-next"
                    size="sm"
                    variant="outline"
                    disabled={entriesLoading || offset + pageSize >= totalCount}
                    onClick={() => onOffsetChange(offset + pageSize)}
                  >
                    {t("settings.processedFolders.nextPage")}
                  </Button>
                </div>
              </div>
            ) : null}
          </div>
        </div>
      ) : null}

      {formOpen ? (
        <FolderFormDialog
          key={formFolder?.id ?? "new"}
          open={formOpen}
          folder={formFolder}
          libraries={libraries}
          saving={formSaving}
          error={formError}
          onOpenChange={onFormOpenChange}
          onSubmit={onFormSubmit}
        />
      ) : null}

      <ImportAsDialog
        key={importEntry?.id ?? "none"}
        entry={importEntry}
        libraryName={
          selectedFolder?.libraryName ?? selectedFolder?.libraryId ?? ""
        }
        importing={
          importEntry !== null && busyKey === `import:${importEntry.id}`
        }
        onSearchTitles={onSearchTitles}
        onImport={onConfirmImport}
        onClose={onCloseImportAs}
      />
    </section>
  );
}
