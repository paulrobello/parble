// Minimal track file controls (P3-T3): import by file picker or drag-drop, export as a download, run
// the loaded track and save it to the IndexedDB library. The polished editor UI arrives in P3-T5.
//
// Import posts the file's text to the bridge untouched; Rust migrates and validates it. Export asks
// Rust for the canonical text of the loaded track. The panel mirrors the latest results into data-*
// attributes (data-track, data-run-hash, data-run-ticks, data-error) for Playwright.

import type { BridgeCommand, BridgeEvent } from "./bridge";
import type { Library } from "./library";

/** One 10 s run at 240 Hz, the length of the P0 golden run. */
export const RUN_TICKS = 2_400;

export interface FilesState {
  track: string | null;
  runHash: string | null;
  runTicks: number | null;
  error: string | null;
}

export const emptyFiles = (): FilesState => ({ track: null, runHash: null, runTicks: null, error: null });

/** The state after `events`. */
export function applyFileEvents(state: FilesState, events: readonly BridgeEvent[]): FilesState {
  let s = state;
  for (const e of events) {
    if (e.type === "trackLoaded") s = { track: e.name, runHash: null, runTicks: null, error: null };
    else if (e.type === "runFinished") s = { ...s, runHash: e.hash, runTicks: e.ticks, error: null };
    else if (e.type === "bridgeError") s = { ...s, error: `${e.command}: ${e.message}` };
  }
  return s;
}

export function formatFiles(s: FilesState): string {
  if (s.error) return s.error;
  if (!s.track) return "no track loaded";
  return s.runHash ? `${s.track}  run ${s.runTicks} ticks  hash ${s.runHash}` : s.track;
}

/** A download file name for a track called `name`. */
export function fileName(name: string): string {
  const slug = name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return `${slug || "track"}.parble`;
}

/** Saves `text` through a temporary object-URL link. */
export function downloadText(doc: Document, name: string, text: string): void {
  const url = URL.createObjectURL(new Blob([text], { type: "application/json" }));
  const a = doc.createElement("a");
  a.href = url;
  a.download = name;
  a.style.display = "none";
  doc.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1_000);
}

export interface FilesPanel {
  /** Feed every drained bridge event here. */
  handle(events: readonly BridgeEvent[]): void;
}

type Post = (cmd: BridgeCommand) => void;

export function mountFiles(doc: Document, post: Post, library: Promise<Library> | null): FilesPanel {
  const root = doc.createElement("div");
  root.id = "parble-files";
  Object.assign(root.style, {
    position: "fixed",
    top: "8px",
    right: "8px",
    display: "flex",
    gap: "6px",
    alignItems: "center",
    padding: "4px 8px",
    borderRadius: "4px",
    background: "rgba(28, 26, 23, 0.72)",
    color: "#ece4d8",
    font: "12px/1.4 ui-monospace, Menlo, monospace",
    zIndex: "10",
  });
  const input = doc.createElement("input");
  input.type = "file";
  input.accept = ".parble,application/json";
  input.id = "parble-import";
  input.style.display = "none";
  const button = (id: string, label: string, onClick: () => void): HTMLButtonElement => {
    const b = doc.createElement("button");
    b.id = id;
    b.type = "button";
    b.textContent = label;
    b.addEventListener("click", onClick);
    return b;
  };
  const status = doc.createElement("span");
  status.id = "parble-files-status";
  const pending: ("download" | "library")[] = [];
  const exportAs = (why: "download" | "library"): void => {
    pending.push(why);
    post({ type: "exportTrack" });
  };
  root.append(
    input,
    button("parble-import-button", "Import", () => input.click()),
    button("parble-export", "Export", () => exportAs("download")),
    button("parble-run", "Run", () => post({ type: "runTrack", ticks: RUN_TICKS })),
    button("parble-save", "Save", () => exportAs("library")),
    status,
  );
  doc.body.append(root);

  const load = async (file: File): Promise<void> => post({ type: "loadTrack", json: await file.text() });
  input.addEventListener("change", () => {
    const f = input.files?.[0];
    input.value = "";
    if (f) void load(f);
  });
  doc.addEventListener("dragover", (e) => e.preventDefault());
  doc.addEventListener("drop", (e) => {
    e.preventDefault();
    const f = e.dataTransfer?.files[0];
    if (f) void load(f);
  });

  let state = emptyFiles();
  const show = (): void => {
    status.textContent = formatFiles(state);
    root.dataset.track = state.track ?? "";
    root.dataset.runHash = state.runHash ?? "";
    root.dataset.runTicks = state.runTicks === null ? "" : String(state.runTicks);
    root.dataset.error = state.error ?? "";
  };
  show();
  return {
    handle(events) {
      if (events.length === 0) return;
      for (const e of events) {
        if (e.type === "bridgeError" && e.command === "exportTrack") pending.shift();
        if (e.type !== "trackExported") continue;
        const why = pending.shift() ?? "download";
        if (why === "download") downloadText(doc, fileName(e.name), e.json);
        else if (library) library.then((lib) => lib.save(e.json)).catch((err) => console.warn("library save", err));
      }
      const next = applyFileEvents(state, events);
      if (next !== state) {
        state = next;
        show();
      }
    },
  };
}
