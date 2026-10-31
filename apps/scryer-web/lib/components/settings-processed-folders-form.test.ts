import assert from "node:assert/strict";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { createElement, type ComponentType, type Context } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer } from "vite";
import type { Translate } from "@/components/root/types";
import type { ProcessedFolder } from "@/components/views/settings/settings-processed-folders-section";

// Radix dialogs render through a portal that is empty on the server, so the
// form is rendered through a pass-through dialog for these markup checks.
const PASS_THROUGH_DIALOG = `
import { createElement } from "react";
const pass = ({ children, open, onOpenChange, asChild, ...props }) =>
  createElement("div", props, children);
export const Dialog = ({ children, open }) => (open ? children : null);
export const DialogClose = pass;
export const DialogContent = pass;
export const DialogDescription = pass;
export const DialogFooter = pass;
export const DialogHeader = pass;
export const DialogOverlay = pass;
export const DialogPortal = pass;
export const DialogTitle = pass;
export const DialogTrigger = pass;
`;

type Fields = Partial<
  Pick<ProcessedFolder, "name" | "rootPath" | "markerMode" | "markerValue">
>;

async function withFolderForm(
  run: (render: (fields: Fields) => string) => void,
) {
  const server = await createServer({
    root: fileURLToPath(new URL("../..", import.meta.url)),
    server: { middlewareMode: true },
    appType: "custom",
    logLevel: "silent",
    plugins: [
      {
        name: "processed-folder-form-fixture",
        enforce: "pre",
        load(id) {
          if (id.endsWith("/lib/context/ui-settings-context.tsx")) {
            return 'export function useUiDateTimeFormat() { return "LOCALE"; }';
          }
          if (id.endsWith("/components/ui/dialog.tsx")) {
            return PASS_THROUGH_DIALOG;
          }
        },
      },
    ],
  });
  try {
    const [view, translation] = await Promise.all([
      server.ssrLoadModule(
        "/components/views/settings/settings-processed-folders-section.tsx",
      ),
      server.ssrLoadModule("/lib/context/translate-context.tsx"),
    ]);
    const Section = view.SettingsProcessedFoldersSection as ComponentType<
      Record<string, unknown>
    >;
    const TranslateContext =
      translation.TranslateContext as Context<Translate | null>;
    const noop = () => {};
    const render = (fields: Fields) => {
      const folder: ProcessedFolder = {
        id: "pf-1",
        name: "Movie drop",
        rootPath: "/srv/processed/movies",
        libraryId: "lib-movies",
        libraryName: "Movies",
        enabled: true,
        minStableScans: 2,
        minAgeSeconds: 300,
        markerMode: "SIBLING_SUFFIX",
        markerValue: ".done",
        lastScanStartedAt: null,
        lastScanFinishedAt: null,
        counts: {
          observing: 0,
          stable: 0,
          importing: 0,
          imported: 0,
          skipped: 0,
          failed: 0,
          needsReview: 0,
          ignored: 0,
          rejected: 0,
          superseded: 0,
          gone: 0,
        },
        ...fields,
      };
      return renderToStaticMarkup(
        createElement(
          TranslateContext.Provider,
          { value: (key) => key },
          createElement(Section, {
            canManage: true,
            loading: false,
            folders: [],
            libraries: [
              {
                id: "lib-movies",
                name: "Movies",
                facet: "movie",
                roots: [],
              },
            ],
            selectedFolderId: null,
            entries: null,
            entriesLoading: false,
            statusFilter: "ALL",
            pageSize: 50,
            offset: 0,
            scanResult: null,
            preview: null,
            busyKey: null,
            error: null,
            formOpen: true,
            formFolder: folder,
            formSaving: false,
            formError: null,
            importEntry: null,
            onSearchTitles: async () => [],
            ...Object.fromEntries(
              [
                "onSelectFolder",
                "onAddFolder",
                "onEditFolder",
                "onDeleteFolder",
                "onFormOpenChange",
                "onFormSubmit",
                "onEnabledChange",
                "onScan",
                "onPreview",
                "onClosePreview",
                "onStatusFilterChange",
                "onOffsetChange",
                "onImportAs",
                "onCloseImportAs",
                "onConfirmImport",
                "onRetry",
                "onIgnore",
              ].map((name) => [name, noop]),
            ),
          }),
        ),
      );
    };
    run(render);
  } finally {
    await server.close();
  }
}

/** The element with this id, as markup, or null. */
function tag(html: string, id: string): string | null {
  return html.match(new RegExp(`<[a-z]+[^>]*\\sid="${id}"[^>]*>`))?.[0] ?? null;
}

function describedBy(html: string, id: string): string[] {
  const element = tag(html, id);
  assert.ok(element, `#${id} is rendered`);
  return element.match(/aria-describedby="([^"]*)"/)?.[1].split(" ") ?? [];
}

function assertDescribedBy(html: string, id: string, expected: string[]) {
  const ids = describedBy(html, id);
  assert.deepEqual(ids, expected, `#${id} aria-describedby`);
  for (const target of ids) {
    assert.ok(tag(html, target), `#${id} points at rendered #${target}`);
  }
}

test("each form field's aria-describedby names the help or error text that is rendered", async () => {
  await withFolderForm((render) => {
    const valid = render({});
    for (const id of [
      "settings-processed-folder-name",
      "settings-processed-folder-path",
      "settings-processed-folder-marker-value",
    ]) {
      assert.doesNotMatch(tag(valid, id) ?? "", /aria-invalid="true"/, id);
    }
    assertDescribedBy(valid, "settings-processed-folder-name", []);
    assertDescribedBy(valid, "settings-processed-folder-path", [
      "settings-processed-folder-path-help",
    ]);
    assertDescribedBy(valid, "settings-processed-folder-marker-value", [
      "settings-processed-folder-marker-help",
    ]);
    assertDescribedBy(valid, "settings-processed-folder-min-stable-scans", [
      "settings-processed-folder-min-stable-scans-help",
    ]);
    assertDescribedBy(valid, "settings-processed-folder-min-age", [
      "settings-processed-folder-min-age-help",
    ]);
    assert.equal(tag(valid, "settings-processed-folder-marker-error"), null);

    const untrimmed = render({
      name: " Movie drop",
      rootPath: "/srv/processed/movies ",
      markerValue: ".done ",
    });
    for (const id of [
      "settings-processed-folder-name",
      "settings-processed-folder-path",
      "settings-processed-folder-marker-value",
    ]) {
      assert.match(tag(untrimmed, id) ?? "", /aria-invalid="true"/, id);
    }
    assertDescribedBy(untrimmed, "settings-processed-folder-name", [
      "settings-processed-folder-name-error",
    ]);
    assertDescribedBy(untrimmed, "settings-processed-folder-path", [
      "settings-processed-folder-path-error",
      "settings-processed-folder-path-help",
    ]);
    assertDescribedBy(untrimmed, "settings-processed-folder-marker-value", [
      "settings-processed-folder-marker-error",
      "settings-processed-folder-marker-help",
    ]);

    // A marker that breaks the V1.2 rules: the help text is the error.
    const badMarker = render({ markerValue: "done" });
    assert.match(
      tag(badMarker, "settings-processed-folder-marker-value") ?? "",
      /aria-invalid="true"/,
    );
    assertDescribedBy(badMarker, "settings-processed-folder-marker-value", [
      "settings-processed-folder-marker-help",
    ]);
  });
});
