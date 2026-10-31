import { useCallback, useEffect, useState } from "react";
import { useClient } from "urql";
import { ConfirmDialog } from "@/components/common/confirm-dialog";
import {
  SettingsProcessedFoldersSection,
  type ProcessedFolder,
  type ProcessedFolderEntry,
  type ProcessedFolderEntryConnection,
  type ProcessedFolderFormValues,
  type ProcessedFolderPreview,
  type ProcessedFolderScanResult,
  type ProcessedFolderStatusFilter,
  type ProcessedFolderTitleOption,
} from "@/components/views/settings/settings-processed-folders-section";
import { useTranslate } from "@/lib/context/translate-context";
import { useGlobalStatus } from "@/lib/context/global-status-context";
import {
  catalogSearchTitlesQuery,
  librariesQuery,
  previewProcessedFolderScanQuery,
  processedFolderEntriesQuery,
  processedFoldersQuery,
} from "@/lib/graphql/queries";
import {
  createProcessedFolderMutation,
  deleteProcessedFolderMutation,
  ignoreProcessedFolderEntryMutation,
  importProcessedFolderEntryMutation,
  retryProcessedFolderEntryMutation,
  scanProcessedFolderMutation,
  setProcessedFolderEnabledMutation,
  updateProcessedFolderMutation,
} from "@/lib/graphql/mutations";
import { useAuth } from "@/lib/hooks/use-auth";
import type { LibraryRecord } from "@/lib/types";
import { APP_PERMISSIONS, hasAnyAppPermission } from "@/lib/utils/permissions";
import { processedFolderErrorMessage } from "@/lib/utils/processed-folder-error-message";

const ENTRIES_PAGE_SIZE = 50;
const PREVIEW_LIMIT = 200;
const TITLE_SEARCH_LIMIT = 25;

type ProcessedFoldersQueryResult = {
  processedFolders?: ProcessedFolder[] | null;
};
type ProcessedFolderEntriesQueryResult = {
  processedFolderEntries?: ProcessedFolderEntryConnection | null;
};
type PreviewQueryResult = {
  previewProcessedFolderScan?: ProcessedFolderPreview | null;
};
type LibrariesQueryResult = { libraries?: LibraryRecord[] | null };
type TitleSearchQueryResult = {
  titles?: { items?: ProcessedFolderTitleOption[] | null } | null;
};
type FolderPayload = { folder: ProcessedFolder } | null | undefined;
type EntryPayload = { entry: ProcessedFolderEntry } | null | undefined;

export function SettingsProcessedFoldersContainer() {
  const setGlobalStatus = useGlobalStatus();
  const t = useTranslate();
  const client = useClient();
  const { user } = useAuth();
  const canManage = hasAnyAppPermission(user, [
    APP_PERMISSIONS.manageSystemSettings,
  ]);

  const [folders, setFolders] = useState<ProcessedFolder[]>([]);
  const [foldersLoading, setFoldersLoading] = useState(true);
  const [libraries, setLibraries] = useState<LibraryRecord[]>([]);
  const [selectedFolderId, setSelectedFolderId] = useState<string | null>(null);
  const [entries, setEntries] = useState<ProcessedFolderEntryConnection | null>(
    null,
  );
  const [entriesLoading, setEntriesLoading] = useState(false);
  const [entriesRevision, setEntriesRevision] = useState(0);
  const [statusFilter, setStatusFilter] =
    useState<ProcessedFolderStatusFilter>("ALL");
  const [offset, setOffset] = useState(0);
  const [scanResult, setScanResult] =
    useState<ProcessedFolderScanResult | null>(null);
  const [preview, setPreview] = useState<ProcessedFolderPreview | null>(null);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [formFolder, setFormFolder] = useState<ProcessedFolder | null>(null);
  const [formSaving, setFormSaving] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [importEntry, setImportEntry] = useState<ProcessedFolderEntry | null>(
    null,
  );
  const [pendingDelete, setPendingDelete] = useState<ProcessedFolder | null>(
    null,
  );

  const reportError = useCallback(
    (failure: unknown, fallbackKey: string) => {
      const message = processedFolderErrorMessage(failure, t, t(fallbackKey));
      setError(message);
      setGlobalStatus(message, { level: "ERROR" });
    },
    [setGlobalStatus, t],
  );

  const replaceFolder = useCallback((folder: ProcessedFolder) => {
    setFolders((current) =>
      current.map((existing) =>
        existing.id === folder.id ? folder : existing,
      ),
    );
  }, []);

  const fetchFolders = useCallback(async () => {
    if (!canManage) {
      setFolders([]);
      setFoldersLoading(false);
      return;
    }
    setFoldersLoading(true);
    try {
      const { data, error: queryError } = await client
        .query<ProcessedFoldersQueryResult>(
          processedFoldersQuery,
          {},
          { requestPolicy: "network-only" },
        )
        .toPromise();
      if (queryError) throw queryError;
      const nextFolders = data?.processedFolders ?? [];
      setFolders(nextFolders);
      setSelectedFolderId((current) =>
        current && nextFolders.some((folder) => folder.id === current)
          ? current
          : (nextFolders[0]?.id ?? null),
      );
    } catch (failure) {
      reportError(failure, "status.failedToLoad");
    } finally {
      setFoldersLoading(false);
    }
  }, [canManage, client, reportError]);

  useEffect(() => {
    void fetchFolders();
  }, [fetchFolders]);

  useEffect(() => {
    if (!canManage) return;
    let cancelled = false;
    void client
      .query<LibrariesQueryResult>(
        librariesQuery,
        { facet: null },
        { requestPolicy: "network-only" },
      )
      .toPromise()
      .then(({ data, error: queryError }) => {
        if (cancelled) return;
        if (queryError) throw queryError;
        setLibraries(data?.libraries ?? []);
      })
      .catch((failure: unknown) => {
        if (!cancelled) reportError(failure, "status.failedToLoad");
      });
    return () => {
      cancelled = true;
    };
  }, [canManage, client, reportError]);

  useEffect(() => {
    if (!canManage || !selectedFolderId) {
      setEntries(null);
      return;
    }
    let cancelled = false;
    setEntriesLoading(true);
    void client
      .query<ProcessedFolderEntriesQueryResult>(
        processedFolderEntriesQuery,
        {
          folderId: selectedFolderId,
          statuses: statusFilter === "ALL" ? null : [statusFilter],
          limit: ENTRIES_PAGE_SIZE,
          offset,
        },
        { requestPolicy: "network-only" },
      )
      .toPromise()
      .then(({ data, error: queryError }) => {
        if (cancelled) return;
        if (queryError) throw queryError;
        setEntries(data?.processedFolderEntries ?? null);
      })
      .catch((failure: unknown) => {
        if (!cancelled) reportError(failure, "status.failedToLoad");
      })
      .finally(() => {
        if (!cancelled) setEntriesLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [
    canManage,
    client,
    entriesRevision,
    offset,
    reportError,
    selectedFolderId,
    statusFilter,
  ]);

  const refreshEntries = () => setEntriesRevision((current) => current + 1);

  const selectFolder = (folderId: string) => {
    if (folderId === selectedFolderId) return;
    setSelectedFolderId(folderId);
    setOffset(0);
    setStatusFilter("ALL");
    setPreview(null);
    setScanResult(null);
    setError(null);
  };

  const openForm = (folder: ProcessedFolder | null) => {
    setFormFolder(folder);
    setFormError(null);
    setFormOpen(true);
  };

  const submitForm = async (values: ProcessedFolderFormValues) => {
    setFormSaving(true);
    setFormError(null);
    try {
      if (formFolder) {
        const { data, error: mutationError } = await client
          .mutation<{ updateProcessedFolder?: FolderPayload }>(
            updateProcessedFolderMutation,
            {
              input: { id: formFolder.id, ...values },
            },
          )
          .toPromise();
        if (mutationError) throw mutationError;
        const saved = data?.updateProcessedFolder?.folder;
        if (!saved) throw new Error(t("status.apiError"));
        replaceFolder(saved);
        refreshEntries();
        setGlobalStatus(
          t("settings.processedFolders.saved", { name: saved.name }),
          { level: "SUCCESS" },
        );
      } else {
        // The server always creates a folder disabled; the input has no enabled field.
        const { data, error: mutationError } = await client
          .mutation<{ createProcessedFolder?: FolderPayload }>(
            createProcessedFolderMutation,
            { input: values },
          )
          .toPromise();
        if (mutationError) throw mutationError;
        const created = data?.createProcessedFolder?.folder;
        if (!created) throw new Error(t("status.apiError"));
        setFolders((current) => [...current, created]);
        selectFolder(created.id);
        setGlobalStatus(
          t("settings.processedFolders.created", { name: created.name }),
          { level: "SUCCESS" },
        );
      }
      setFormOpen(false);
    } catch (failure) {
      setFormError(
        processedFolderErrorMessage(failure, t, t("status.failedToUpdate")),
      );
    } finally {
      setFormSaving(false);
    }
  };

  const runBusy = async (
    key: string,
    action: () => Promise<void>,
    fallbackKey: string,
  ) => {
    setBusyKey(key);
    setError(null);
    try {
      await action();
    } catch (failure) {
      reportError(failure, fallbackKey);
    } finally {
      setBusyKey(null);
    }
  };

  const setEnabled = (folder: ProcessedFolder, enabled: boolean) =>
    runBusy(
      `enabled:${folder.id}`,
      async () => {
        const { data, error: mutationError } = await client
          .mutation<{ setProcessedFolderEnabled?: FolderPayload }>(
            setProcessedFolderEnabledMutation,
            {
              id: folder.id,
              enabled,
            },
          )
          .toPromise();
        if (mutationError) throw mutationError;
        const saved = data?.setProcessedFolderEnabled?.folder;
        if (!saved) throw new Error(t("status.apiError"));
        replaceFolder(saved);
      },
      "status.failedToUpdate",
    );

  const scan = (folder: ProcessedFolder) =>
    runBusy(
      `scan:${folder.id}`,
      async () => {
        const { data, error: mutationError } = await client
          .mutation<{ scanProcessedFolder?: ProcessedFolderScanResult | null }>(
            scanProcessedFolderMutation,
            {
              id: folder.id,
            },
          )
          .toPromise();
        if (mutationError) throw mutationError;
        const result = data?.scanProcessedFolder;
        if (!result) throw new Error(t("status.apiError"));
        replaceFolder(result.folder);
        setScanResult(result);
        refreshEntries();
      },
      "status.failedToUpdate",
    );

  const runPreview = (folder: ProcessedFolder) =>
    runBusy(
      `preview:${folder.id}`,
      async () => {
        const { data, error: queryError } = await client
          .query<PreviewQueryResult>(
            previewProcessedFolderScanQuery,
            { id: folder.id, limit: PREVIEW_LIMIT },
            { requestPolicy: "network-only" },
          )
          .toPromise();
        if (queryError) throw queryError;
        const result = data?.previewProcessedFolderScan;
        if (!result) throw new Error(t("status.apiError"));
        setPreview(result);
      },
      "status.failedToLoad",
    );

  const afterEntryAction = (entry: ProcessedFolderEntry | undefined) => {
    if (!entry) throw new Error(t("status.apiError"));
    refreshEntries();
    void fetchFolders();
  };

  const importAs = (entry: ProcessedFolderEntry, titleId: string) =>
    runBusy(
      `import:${entry.id}`,
      async () => {
        const { data, error: mutationError } = await client
          .mutation<{ importProcessedFolderEntry?: EntryPayload }>(
            importProcessedFolderEntryMutation,
            {
              entryId: entry.id,
              titleId,
            },
          )
          .toPromise();
        if (mutationError) {
          setImportEntry(null);
          throw mutationError;
        }
        afterEntryAction(data?.importProcessedFolderEntry?.entry);
        setImportEntry(null);
        setGlobalStatus(t("settings.processedFolders.importQueued"), {
          level: "SUCCESS",
        });
      },
      "status.failedToUpdate",
    );

  const retry = (entry: ProcessedFolderEntry) =>
    runBusy(
      `retry:${entry.id}`,
      async () => {
        const { data, error: mutationError } = await client
          .mutation<{ retryProcessedFolderEntry?: EntryPayload }>(
            retryProcessedFolderEntryMutation,
            { entryId: entry.id },
          )
          .toPromise();
        if (mutationError) throw mutationError;
        afterEntryAction(data?.retryProcessedFolderEntry?.entry);
      },
      "status.failedToUpdate",
    );

  const ignore = (entry: ProcessedFolderEntry) =>
    runBusy(
      `ignore:${entry.id}`,
      async () => {
        const { data, error: mutationError } = await client
          .mutation<{ ignoreProcessedFolderEntry?: EntryPayload }>(
            ignoreProcessedFolderEntryMutation,
            { entryId: entry.id },
          )
          .toPromise();
        if (mutationError) throw mutationError;
        afterEntryAction(data?.ignoreProcessedFolderEntry?.entry);
      },
      "status.failedToUpdate",
    );

  const confirmDelete = async () => {
    const folder = pendingDelete;
    if (!folder) return;
    await runBusy(
      `delete:${folder.id}`,
      async () => {
        const { error: mutationError } = await client
          .mutation<{ deleteProcessedFolder?: { id: string } | null }>(
            deleteProcessedFolderMutation,
            { id: folder.id },
          )
          .toPromise();
        if (mutationError) throw mutationError;
        setFolders((current) =>
          current.filter((existing) => existing.id !== folder.id),
        );
        if (selectedFolderId === folder.id) {
          setSelectedFolderId(null);
          setPreview(null);
          setScanResult(null);
        }
        setGlobalStatus(
          t("settings.processedFolders.deleted", { name: folder.name }),
          { level: "SUCCESS" },
        );
      },
      "status.failedToDelete",
    );
    setPendingDelete(null);
  };

  const selectedLibraryId =
    folders.find((folder) => folder.id === selectedFolderId)?.libraryId ?? null;
  const searchTitles = useCallback(
    async (query: string): Promise<ProcessedFolderTitleOption[]> => {
      if (!selectedLibraryId) return [];
      const { data, error: queryError } = await client
        .query<TitleSearchQueryResult>(catalogSearchTitlesQuery, {
          facet: null,
          libraryIds: [selectedLibraryId],
          query,
          limit: TITLE_SEARCH_LIMIT,
        })
        .toPromise();
      if (queryError) throw queryError;
      // The query is already scoped to the library; this keeps a title from
      // anywhere else out of the list even if the scope were ignored.
      return (data?.titles?.items ?? []).filter(
        (title) => title.libraryId === selectedLibraryId,
      );
    },
    [client, selectedLibraryId],
  );

  return (
    <>
      <SettingsProcessedFoldersSection
        canManage={canManage}
        loading={foldersLoading}
        folders={folders}
        libraries={libraries}
        selectedFolderId={selectedFolderId}
        entries={entries}
        entriesLoading={entriesLoading}
        statusFilter={statusFilter}
        pageSize={ENTRIES_PAGE_SIZE}
        offset={offset}
        scanResult={scanResult}
        preview={preview}
        busyKey={busyKey}
        error={error}
        formOpen={formOpen}
        formFolder={formFolder}
        formSaving={formSaving}
        formError={formError}
        importEntry={importEntry}
        onSelectFolder={selectFolder}
        onAddFolder={() => openForm(null)}
        onEditFolder={(folder) => openForm(folder)}
        onDeleteFolder={setPendingDelete}
        onFormOpenChange={(open) => {
          if (!open) setFormOpen(false);
        }}
        onFormSubmit={(values) => void submitForm(values)}
        onEnabledChange={(folder, enabled) => void setEnabled(folder, enabled)}
        onScan={(folder) => void scan(folder)}
        onPreview={(folder) => void runPreview(folder)}
        onClosePreview={() => setPreview(null)}
        onStatusFilterChange={(filter) => {
          setStatusFilter(filter);
          setOffset(0);
        }}
        onOffsetChange={setOffset}
        onImportAs={setImportEntry}
        onCloseImportAs={() => setImportEntry(null)}
        onConfirmImport={(entry, titleId) => void importAs(entry, titleId)}
        onSearchTitles={searchTitles}
        onRetry={(entry) => void retry(entry)}
        onIgnore={(entry) => void ignore(entry)}
      />
      <ConfirmDialog
        open={pendingDelete !== null}
        title={t("settings.processedFolders.deleteTitle")}
        description={t("settings.processedFolders.deleteConfirm", {
          name: pendingDelete?.name ?? "",
        })}
        confirmLabel={t("label.delete")}
        cancelLabel={t("label.cancel")}
        contentId="settings-processed-folder-delete-dialog"
        confirmButtonId="settings-processed-folder-delete-confirm"
        cancelButtonId="settings-processed-folder-delete-cancel"
        confirmButtonVariant="destructive"
        isBusy={busyKey !== null}
        onConfirm={confirmDelete}
        onCancel={() => setPendingDelete(null)}
      />
    </>
  );
}
