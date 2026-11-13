// The track editor overlay (P3-T5): docs/design/editor/README.md is the layout and interaction spec,
// .impeccable/surfaces/web-ui-src-editor.md the direction. It drives the engine through `parbleEdit`
// actions and reads `window.__parble.editor` once per animation frame (`update`). DOM writes happen
// only when a value changes; layout is read only when a brass pin moves to a new item.

import type { BridgeCommand, BridgeEvent } from "../bridge";
import { downloadText, fileName } from "../files";
import type { Library, TrackSummary } from "../library";
import { RunFade } from "./fade";
import { icon, iconSprite } from "./icons";
import { ENGINE_KEYS, keyResult, ownsKeys } from "./keys";
import {
  type Action,
  arrowPath,
  boxPath,
  cellAt,
  type CheckDiag,
  checkReleaseText,
  checkText,
  cellText,
  countBadge,
  type EditorDiag,
  FAMILIES,
  type Family,
  familyOf,
  faceWord,
  familyText,
  focusOrbit,
  ghostText,
  INTERVALS,
  issueText,
  levelLabel,
  MAX_MARBLES,
  type NudgeEdit,
  type NudgeZoneDiag,
  nudgeAccelText,
  nudgeEdit,
  nudgeHeadingText,
  NUDGE_MODE_LABELS,
  nudgeMode,
  nudgeName,
  nudgeSizeText,
  nudgeSpeedText,
  pieceName,
  type PieceOption,
  piecesOf,
  planThumb,
  portsText,
  type Release,
  runTime,
  type Send,
  sliceDown,
  sliceUp,
  START_LEVEL,
  strokeText,
  TICK_HZ,
  type Vec3i,
  zoneOf,
} from "./model";
import { CSS, fontFaces, THUMB_DIR, versioned } from "./style";

/** Deleting more than this many pieces asks once. */
export const CONFIRM_DELETE = 50;
/** Show flies the camera over this long. */
export const FLY_MS = 450;
const HINTS_KEY = "parble.editor.hints/1";

export interface Orbit {
  target: [number, number, number];
  yaw: number;
  pitch: number;
  radius: number;
}

export interface EditorDeps {
  doc: Document;
  win: Window;
  send: Send;
  post: (cmd: BridgeCommand) => void;
  library: Promise<Library> | null;
  now?: () => number;
  /** The orbit camera now (`window.__parble.camera.orbit`). */
  orbit?: () => Orbit | null;
  /** The camera mode now (`window.__parble.camera.mode`): in `fly` the engine owns every key. */
  cameraMode?: () => string | null;
}

/** Folded run controls (below 1280 px): the tooltip's label and key cap. */
const RUN_TIPS: Record<string, [string, string?]> = {
  one: ["Release one"],
  all: ["Release all"],
  interval: ["Interval"],
  pause: ["Pause", "Space"],
  reset: ["Reset"],
  follow: ["Follow cam", "C"],
};
/** Piece options by key: the inspector's label. */
const OPTION_LABEL: Record<string, string> = { mode: "Mode", first: "First side" };

export interface EditorOverlay {
  root: HTMLElement;
  /** Once per animation frame with the latest diag and the drained bridge events. */
  update(d: EditorDiag | null, events?: readonly BridgeEvent[]): void;
  /** Handle one key event (also wired to the window). Returns whether it was used. */
  key(e: KeyboardEvent): boolean;
  /** Whether the track has edits not saved to the library. */
  dirty(): boolean;
}

const h = <K extends keyof HTMLElementTagNameMap>(
  doc: Document,
  tag: K,
  attrs: Record<string, string> = {},
  html = "",
): HTMLElementTagNameMap[K] => {
  const el = doc.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v);
  if (html) el.innerHTML = html;
  return el;
};

const esc = (s: string): string =>
  s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c] as string);

/** A brass pin that travels along its container to the active item (or jumps under reduced motion, by CSS). */
class Pin {
  readonly el: HTMLElement;
  #at: HTMLElement | null = null;
  constructor(
    doc: Document,
    container: HTMLElement,
    readonly dx: number,
    readonly dy: number,
    cls = "",
  ) {
    this.el = h(doc, "i", { class: `pin travel hide ${cls}`, "aria-hidden": "true" });
    container.append(this.el);
  }
  to(target: HTMLElement | null): void {
    if (target === this.#at) return;
    this.#at = target;
    if (!target) {
      this.el.classList.add("hide");
      return;
    }
    this.el.style.transform = `translate(${target.offsetLeft + this.dx}px, ${target.offsetTop + this.dy}px)`;
    this.el.classList.remove("hide");
  }
  /** Re-measure (after the container scrolled or the item moved). */
  refresh(): void {
    const t = this.#at;
    this.#at = null;
    this.to(t);
  }
}

export function mountEditor(deps: EditorDeps): EditorOverlay {
  const { doc, win, send } = deps;
  const now = deps.now ?? (() => performance.now());
  const style = h(doc, "style", { id: "pe-style" });
  style.textContent = `${fontFaces(win)}\n${CSS}`;
  doc.head.append(style);
  doc.body.classList.add("parble-editor");

  const root = h(doc, "div", { id: "pe" });
  root.append(iconSprite(doc));
  let d: EditorDiag | null = null;
  const act = (a: Action): void => send(a);

  // ---- top strip -------------------------------------------------------------------------
  const top = h(doc, "header", { id: "pe-top", class: "inlay fz", "aria-label": "Editor tools" });
  top.innerHTML = `
<span class="brand">parble</span>
<button class="track" data-k="rename" title="Rename the track"></button>
<button class="t lib" data-k="library">${icon("lib")}Library</button><i class="sep"></i>
<div class="grp build" data-g="tools">
 <button class="t" data-tool="select" title="Select (1)">${icon("sel")}<span>Select <kbd>1</kbd></span></button>
 <button class="t" data-tool="place" title="Place (2)">${icon("plc")}<span>Place <kbd>2</kbd></span></button>
 <button class="t" data-tool="delete" title="Delete (3)">${icon("del")}<span>Delete <kbd>3</kbd></span></button>
</div>
<div class="grp build">
 <button class="t" data-k="rotate" title="Rotate a quarter turn (R, ⇧R the other way)">${icon("rot")}<span>Rotate <kbd>R</kbd></span></button>
 <button class="t" data-k="mirror" title="Mirror (M)">${icon("mir")}<span>Mirror <kbd>M</kbd></span></button>
 <button class="t" data-k="undo" title="Undo (⌘Z)">${icon("und")}<span>Undo <kbd>⌘Z</kbd></span></button>
 <button class="t" data-k="redo" title="Redo (⇧⌘Z)">${icon("red")}<span>Redo <kbd>⇧⌘Z</kbd></span></button>
</div><i class="sep"></i>
<div class="sl build"><span class="sc">Levels</span>
 <button data-k="down" aria-label="Show one level less ([)" title="Show one level less ([)">${icon("chl")}<kbd>[</kbd></button>
 <button class="v" data-k="sliceToggle" title="Toggle the slice and all levels (X)"></button>
 <button data-k="up" aria-label="Show one level more (])" title="Show one level more (])">${icon("chr")}<kbd>]</kbd></button>
 <button class="cl on" data-k="columns" aria-label="Ground columns" aria-keyshortcuts="H" aria-pressed="true" title="Ground columns: project the hovered and selected pieces down to the floor (H)">${icon("col")}<kbd>H</kbd></button>
</div>
<span class="sp"></span>
<div class="grp" data-g="run">
 <button class="t rc" data-k="one" aria-label="Release one" title="Release one marble">${icon("r1")}<span>Release one</span></button>
 <button class="t rc" data-k="all" aria-label="Release all" title="Release all marbles">${icon("ra")}<span>Release all</span></button>
 <button class="t rc" data-k="interval" aria-label="Interval" title="Release on an interval" aria-haspopup="menu" aria-expanded="false">${icon("int")}<span>Interval</span></button>
 <button class="t rc" data-k="pause" aria-label="Pause" aria-keyshortcuts="Space P" title="Pause (Space or P while running)" aria-pressed="false">${icon("pau")}<span>Pause <kbd class="rk">Space</kbd></span></button>
 <button class="t rc" data-k="reset" aria-label="Reset" title="Reset: marbles back to the hopper, edits kept">${icon("rst")}<span>Reset</span></button>
 <button class="t rc" data-k="follow" aria-label="Follow cam" aria-keyshortcuts="C" title="Follow cam: the director films the run (C)" aria-pressed="false">${icon("cam")}<span>Follow cam</span></button>
</div><i class="sep"></i>
<div class="mb"><span class="sc">Marbles<b data-k="count"></b></span><span class="row" data-k="pins"></span></div>
<button class="run" data-k="run"></button>
<div class="menu inlay" role="menu" data-k="menu"></div>`;
  const q = <T extends HTMLElement = HTMLElement>(el: ParentNode, sel: string): T => el.querySelector(sel) as T;
  const trackBtn = q<HTMLButtonElement>(top, '[data-k="rename"]');
  const toolsGrp = q(top, '[data-g="tools"]');
  const toolBtn = (t: string): HTMLButtonElement => q(top, `[data-tool="${t}"]`);
  const toolPin = new Pin(doc, toolsGrp, 5, 4);
  const runGrp = q(top, '[data-g="run"]');
  const pausePin = new Pin(doc, runGrp, 5, 4);
  const followPin = new Pin(doc, runGrp, 5, 4);
  const sliceBtn = q<HTMLButtonElement>(top, '[data-k="sliceToggle"]');
  const runBtn = q<HTMLButtonElement>(top, '[data-k="run"]');
  const countEl = q(top, '[data-k="count"]');
  const pinsEl = q(top, '[data-k="pins"]');
  const menu = q(top, '[data-k="menu"]');
  for (let i = 0; i < MAX_MARBLES; i++) {
    const b = h(doc, "button", { "data-n": String(i + 1), "aria-label": `${i + 1} marble${i ? "s" : ""}` });
    b.append(h(doc, "i", { class: "pin off" }));
    b.addEventListener("click", () => act({ op: "marbles", count: i + 1 }));
    pinsEl.append(b);
  }
  menu.innerHTML = INTERVALS.map(
    (s) => `<button role="menuitemradio" data-s="${s}"><i class="pin off"></i>Every ${s} s</button>`,
  ).join("");
  let intervalS: number = INTERVALS[1];
  let lastRelease: Release = "all";
  let lastSlice: number | null = null;

  // The release Run makes: the marble check runs exactly this one.
  const releaseOf = (release: Release): { release: Release; intervalTicks?: number } =>
    release === "interval" ? { release, intervalTicks: Math.round(intervalS * TICK_HZ) } : { release };
  const run = (release: Release): void => {
    lastRelease = release;
    act({ op: "run", ...releaseOf(release) });
  };
  let checkAllSizes = false;
  const check = (): void => act({ op: "check", ...releaseOf(lastRelease), allSizes: checkAllSizes });
  const toggleSlice = (): void => {
    if (!d) return;
    if (d.slice === null) act({ op: "slice", level: lastSlice ?? sliceDown(null, d.maxLevel) });
    else {
      lastSlice = d.slice;
      act({ op: "slice", level: null });
    }
  };
  const columnsBtn = q<HTMLButtonElement>(top, '[data-k="columns"]');
  const toggleColumns = (): void => {
    if (d) act({ op: "columns", on: !(d.columns?.on ?? true) });
  };
  const setMenu = (open: boolean): void => {
    menu.classList.toggle("open", open);
    q(top, '[data-k="interval"]').setAttribute("aria-expanded", String(open));
    if (open) {
      const b = q(top, '[data-k="interval"]');
      menu.style.left = `${b.offsetLeft}px`;
    }
  };

  // ---- right panel -----------------------------------------------------------------------
  const side = h(doc, "aside", { id: "pe-side", class: "inlay fz", "aria-label": "Selection and validation" });
  side.innerHTML = `
<button class="spine" data-k="panel" title="Collapse the panel (\\)" aria-label="Collapse the panel" aria-expanded="true"><kbd>\\</kbd></button>
<section class="sec" data-k="sel"></section>
<section class="sec" data-k="val"><h2><span class="sc">Validation ·</span><span data-k="vh"></span></h2><div data-k="vl"></div>
<div class="ck" data-k="ck"><h3><span class="sc">Marble check</span></h3>
<div class="ckb"><div class="seg" role="radiogroup" aria-label="Marble sizes" data-k="cks"></div>
<button class="btn" data-k="check" title="Run the release Run makes once per marble type, off the scene, and list where marbles fall off or stall">Check</button></div>
<div data-k="ckl"></div></div>
<div class="ck nz" data-k="nz" hidden><h3><span class="sc">Nudge zones ·</span><span data-k="nzh"></span></h3>
<div class="ckb"><button class="btn" data-k="nzAdd">Add zone</button><span class="note" data-k="nzw"></span></div>
<div data-k="nzl"></div></div>
<p class="note">Show moves the camera to the cell and selects the piece.</p></section>`;
  const selSec = q(side, '[data-k="sel"]');
  const vHead = q(side, '[data-k="vh"]');
  const vList = q(side, '[data-k="vl"]');
  const ckSec = q(side, '[data-k="ck"]');
  const ckBtn = q<HTMLButtonElement>(side, '[data-k="check"]');
  const ckSizes = q(side, '[data-k="cks"]');
  const ckList = q(side, '[data-k="ckl"]');
  const nzSec = q(side, '[data-k="nz"]');
  const nzHead = q(side, '[data-k="nzh"]');
  const nzAddBtn = q<HTMLButtonElement>(side, '[data-k="nzAdd"]');
  const nzWhere = q(side, '[data-k="nzw"]');
  const nzList = q(side, '[data-k="nzl"]');
  let confirmDelete = false;
  let deletedNote: string | null = null;

  // ---- drawer ----------------------------------------------------------------------------
  const tray = h(doc, "footer", { id: "pe-tray", class: "inlay fz", "aria-label": "Pieces" });
  tray.innerHTML = `<nav class="tabs" role="tablist" data-k="tabs"></nav>
<div class="body"><div class="tiles" data-k="tiles" role="listbox" aria-label="Pieces"></div>
<div class="hint" data-k="hint"><p data-k="htext"></p><div class="b">
<button data-k="hms" title="Switch to Select, then Shift-drag on empty space"><span class="sc">Multi-select</span><kbd>⇧ drag</kbd></button>
<button data-k="hsl" title="Toggle the level slice (X)"><span class="sc">Slice at level</span><kbd>X</kbd></button></div></div></div>`;
  const tabs = q(tray, '[data-k="tabs"]');
  const tiles = q(tray, '[data-k="tiles"]');
  const hintText = q(tray, '[data-k="htext"]');
  for (const f of FAMILIES) {
    const b = h(doc, "button", { role: "tab", "data-f": f.id, "aria-selected": "false" }, f.label);
    b.addEventListener("click", () => setFamily(f.id));
    tabs.append(b);
  }
  const collapse = h(
    doc,
    "button",
    { class: "collapse", "data-k": "drawer", title: "Collapse the drawer (Tab)" },
    `${icon("chd")}<kbd>Tab</kbd>`,
  );
  tabs.append(collapse);
  const tabPin = new Pin(doc, tabs, 9, 15);
  const tilePin = new Pin(doc, tiles, 6, 6);
  const edgeCue = (): void => {
    tiles.classList.toggle("more-l", tiles.scrollLeft > 2);
    tiles.classList.toggle("more-r", tiles.scrollLeft + tiles.clientWidth < tiles.scrollWidth - 2);
  };
  tiles.addEventListener("scroll", edgeCue, { passive: true });
  tiles.addEventListener(
    "wheel",
    (e) => {
      if (Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
      tiles.scrollLeft += e.deltaY;
      e.preventDefault();
    },
    { passive: false },
  );
  let family: Family = "blocks";
  let tileFor = "";
  let countFor = "";

  // ---- floating: tooltip, marquee, capsule, hairlines, sound, sheet --------------------------
  const tip = h(doc, "div", { id: "pe-tip", class: "inlay", role: "status" }, `<i class="pin"></i><span></span>`);
  const tipText = q(tip, "span");
  const tipPin = q(tip, ".pin");
  const marq = h(doc, "div", { id: "pe-marq" });
  const mlabel = h(doc, "div", { id: "pe-mlabel", class: "inlay" });
  const cap = h(doc, "div", { id: "pe-cap", class: "inlay", role: "status" });
  const hairs = ["top", "bottom", "right"].map((e) => h(doc, "i", { class: `hair h-${e}` }));
  const sheet = h(doc, "section", { id: "pe-sheet", class: "inlay", "aria-label": "Track library" });
  sheet.innerHTML = `<header><h2><span class="sc">Library</span></h2>
<button class="btn" data-k="save">${icon("save")}Save</button>
<button class="btn" data-k="import">${icon("imp")}Import</button>
<button class="btn" data-k="export">${icon("exp")}Export</button>
<button class="btn" data-k="close" aria-label="Close the library">${icon("x")}</button></header>
<div class="ask" data-k="ask"><span></span><button class="btn warn" data-k="yes">Open anyway</button><button class="btn" data-k="no">Keep editing</button></div>
<p class="status" data-k="status"></p>
<div class="list" data-k="list"></div>
<input type="file" accept=".parble,application/json" data-k="file" hidden>`;
  const sheetList = q(sheet, '[data-k="list"]');
  const sheetStatus = q(sheet, '[data-k="status"]');
  const ask = q(sheet, '[data-k="ask"]');
  const fileIn = q<HTMLInputElement>(sheet, '[data-k="file"]');

  // The dashed placement ghost: the brush footprint's box (and a drag-to-lay run's boxes), projected
  // by the engine, drawn under the chrome. Brass where valid, red where not.
  const SVG_NS = "http://www.w3.org/2000/svg";
  const ghost = doc.createElementNS(SVG_NS, "svg");
  ghost.setAttribute("id", "pe-ghost");
  ghost.setAttribute("aria-hidden", "true");
  const ghostRun = doc.createElementNS(SVG_NS, "path");
  ghostRun.setAttribute("class", "run");
  const ghostBox = doc.createElementNS(SVG_NS, "path");
  ghost.append(ghostRun, ghostBox);
  // Nudge zones (editor only, never in a run or its build), in their own layer under the ghost:
  // pale-blue outlines and arrows, the selected zone in brass.
  const nzLayer = doc.createElementNS(SVG_NS, "svg");
  nzLayer.setAttribute("id", "pe-nz");
  nzLayer.setAttribute("aria-hidden", "true");
  const nzPath = (cls: string): SVGPathElement => {
    const el = doc.createElementNS(SVG_NS, "path");
    el.setAttribute("class", cls);
    return el;
  };
  const nzBoxes = nzPath("nz");
  const nzArrows = nzPath("nza");
  const nzSelBox = nzPath("nz sel");
  const nzSelArrow = nzPath("nza sel");
  nzLayer.append(nzBoxes, nzArrows, nzSelBox, nzSelArrow);
  // Folded run controls (below 1280 px) show their label and key cap in this inlay tooltip.
  const tt = h(doc, "div", { id: "pe-tt", class: "inlay", role: "tooltip" });

  root.append(nzLayer, ghost, top, side, tray, tip, marq, mlabel, cap, ...hairs, sheet, tt);
  doc.body.append(root);

  const narrow = (): boolean => win.matchMedia?.("(max-width: 1279px)").matches ?? false;
  const showTip = (b: HTMLElement): void => {
    const t = RUN_TIPS[b.dataset.k ?? ""];
    if (!t || !narrow()) return;
    tt.innerHTML = `<span>${esc(t[0])}</span>${t[1] ? `<kbd>${esc(t[1])}</kbd>` : ""}`;
    const left = b.offsetLeft + runGrp.offsetLeft + top.offsetLeft;
    tt.style.transform = `translate(${Math.round(left + b.offsetWidth / 2)}px, ${top.offsetTop + top.offsetHeight + 6}px) translateX(-50%)`;
    tt.classList.add("show");
  };
  const hideTip = (): void => tt.classList.remove("show");
  for (const b of runGrp.querySelectorAll<HTMLElement>(".rc")) {
    b.addEventListener("pointerenter", () => showTip(b));
    b.addEventListener("focus", () => showTip(b));
    b.addEventListener("pointerleave", hideTip);
    b.addEventListener("blur", hideTip);
  }

  // ---- run fade & pointer ----------------------------------------------------------------------
  const fade = new RunFade();
  let pointer: { x: number; y: number; on: boolean } = { x: 0, y: 0, on: false };
  let press: { x: number; y: number; marquee: boolean } | null = null;
  let marqueeEnd: { x: number; y: number } | null = null;
  /** A drag-to-lay from a drawer tile: armed while the pointer is still over the chrome (so nothing
   * is laid under the drawer), open once it reaches the scene. */
  let laying: false | "armed" | "open" = false;
  let mlabelUntil = 0;
  const canvas = (): HTMLElement | null => doc.getElementById("parble");
  win.addEventListener("pointermove", (e) => {
    fade.move(e.clientX, e.clientY, win.innerWidth, win.innerHeight, now());
    pointer = { x: e.clientX, y: e.clientY, on: e.target === canvas() };
    if (laying === "armed" && pointer.on) {
      laying = "open";
      act({ op: "stroke", phase: "start" });
    }
    if (press?.marquee && Math.hypot(e.clientX - press.x, e.clientY - press.y) > 4) {
      const x = Math.min(press.x, e.clientX);
      const y = Math.min(press.y, e.clientY);
      marq.style.transform = `translate(${x}px, ${y}px)`;
      marq.style.width = `${Math.abs(e.clientX - press.x)}px`;
      marq.style.height = `${Math.abs(e.clientY - press.y)}px`;
      marq.classList.add("show");
    }
  });
  win.addEventListener("pointerdown", (e) => {
    if (e.target !== canvas() || e.button !== 0 || !d) return;
    const onSelected = d.hover?.hitAnchor && d.selected.some((a) => a.join() === d?.hover?.hitAnchor?.join());
    press = { x: e.clientX, y: e.clientY, marquee: d.tool === "select" && !onSelected };
  });
  win.addEventListener("pointerup", (e) => {
    if (press?.marquee && marq.classList.contains("show")) marqueeEnd = { x: e.clientX, y: e.clientY };
    press = null;
    marq.classList.remove("show");
    if (laying === "open") act({ op: "stroke", phase: "end" });
    laying = false;
  });
  // A mouse click on a strip, drawer or panel button hands the keys back to the scene: Bevy hears
  // its keys only while the canvas has focus. Keyboard activation (detail 0) keeps focus for Tab
  // navigation, and the buttons that open the rename field, the library or the interval menu keep it.
  const keepsFocus = new Set(["rename", "library", "interval"]);
  const refocus = (e: MouseEvent): void => {
    const b = (e.target as HTMLElement | null)?.closest<HTMLElement>("button");
    if (e.detail === 0 || !b || keepsFocus.has(b.dataset.k ?? "") || b.closest('[role="menu"]')) return;
    canvas()?.focus({ preventScroll: true });
  };
  for (const el of [top, tray, side]) el.addEventListener("click", refocus);
  // Drag-to-lay from the drawer: pressing a tile picks its piece and opens a stroke; while the
  // button is held, the engine lays one piece per cell the pointer enters on the scene, and the
  // release ends the stroke as one undo step. Pointer events only (no HTML drag and drop, which
  // would stop the moves reaching the canvas), and no pointer capture, for the same reason.
  tiles.addEventListener("pointerdown", (e) => {
    const b = (e.target as HTMLElement).closest<HTMLElement>("[data-piece]");
    if (!b?.dataset.piece || e.button !== 0) return;
    pick(b.dataset.piece);
    laying = "armed";
  });
  tiles.addEventListener("dragstart", (e) => e.preventDefault());

  // ---- hints (announced once each, then remembered) ---------------------------------------
  const seen = new Set<string>();
  try {
    for (const k of JSON.parse(win.localStorage?.getItem(HINTS_KEY) ?? "[]") as string[]) seen.add(k);
  } catch {
    /* garbage: start fresh */
  }
  const announce = (k: string): boolean => {
    if (seen.has(k)) return false;
    seen.add(k);
    try {
      win.localStorage?.setItem(HINTS_KEY, JSON.stringify([...seen]));
    } catch {
      /* storage full or blocked: the hint shows again next time */
    }
    return true;
  };
  let hint = "";

  // ---- library -------------------------------------------------------------------------
  const pending: ("download" | "library")[] = [];
  let savedEdits: number | null = null;
  let cleanOnNext = true;
  let openAfterAsk: (() => void) | null = null;
  const dirty = (): boolean => d !== null && savedEdits !== null && d.edits !== savedEdits && d.pieces > 0;
  const status = (text: string, bad = false): void => {
    sheetStatus.textContent = text;
    sheetStatus.classList.toggle("bad", bad);
  };
  const guard = (go: () => void): void => {
    if (!dirty()) {
      go();
      return;
    }
    openAfterAsk = go;
    q(ask, "span").textContent = "This track has unsaved edits.";
    ask.classList.add("open");
  };
  const load = (json: string): void => {
    cleanOnNext = true;
    deps.post({ type: "loadTrack", json });
  };
  const refreshList = async (): Promise<void> => {
    const lib = await deps.library;
    if (!lib) {
      sheetList.innerHTML = `<p class="note">The library needs IndexedDB, which this browser does not offer.</p>`;
      return;
    }
    const rows: (TrackSummary & { pieces: number; plan: string })[] = [];
    for (const s of await lib.list()) {
      const rec = await lib.get(s.id);
      let placements: { piece: string; anchor: [number, number, number] }[] = [];
      try {
        const raw = (JSON.parse(rec?.json ?? "{}") as { placements?: unknown[] }).placements ?? [];
        placements = raw.filter(
          (p): p is { piece: string; anchor: [number, number, number] } =>
            typeof (p as { piece?: unknown }).piece === "string" &&
            Array.isArray((p as { anchor?: unknown }).anchor) &&
            (p as { anchor: unknown[] }).anchor.length === 3,
        );
      } catch {
        /* a bad record lists with 0 pieces */
      }
      rows.push({ ...s, pieces: placements.length, plan: planThumb(placements) });
    }
    rows.sort((a, b) => b.saved - a.saved);
    sheetList.innerHTML = rows.length
      ? rows
          .map(
            (
              r,
            ) => `<div class="row"><div class="thumb" aria-hidden="true">${r.plan || esc(r.name.slice(0, 1).toUpperCase())}</div>
<div><div>${esc(r.name)}</div><div class="meta">${r.pieces} piece${r.pieces === 1 ? "" : "s"} · ${new Date(r.saved).toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" })}</div></div>
<button class="btn" data-open="${esc(r.id)}">Open</button><span></span></div>`,
          )
          .join("")
      : `<p class="note">No saved tracks yet. Save keeps this track in this browser; Export writes a .parble file to share.</p>`;
  };
  const setSheet = (open: boolean): void => {
    sheet.classList.toggle("open", open);
    ask.classList.remove("open");
    if (open) {
      status("");
      void refreshList();
    }
  };
  sheet.addEventListener("click", (e) => {
    const b = (e.target as HTMLElement).closest("button");
    if (!b) return;
    const k = b.dataset.k;
    if (k === "close") setSheet(false);
    else if (k === "save" || k === "export") {
      pending.push(k === "save" ? "library" : "download");
      act({ op: "export" });
    } else if (k === "import") guard(() => fileIn.click());
    else if (k === "yes") {
      ask.classList.remove("open");
      const go = openAfterAsk;
      openAfterAsk = null;
      go?.();
    } else if (k === "no") {
      ask.classList.remove("open");
      openAfterAsk = null;
    } else if (b.dataset.open) {
      const id = b.dataset.open;
      guard(() => {
        void deps.library?.then(async (lib) => {
          const rec = await lib.get(id);
          if (rec) {
            load(rec.json);
            status(`Opened ${rec.name}`);
          }
        });
      });
    }
  });
  fileIn.addEventListener("change", () => {
    const f = fileIn.files?.[0];
    fileIn.value = "";
    if (f) void f.text().then(load);
  });
  doc.addEventListener("dragover", (e) => e.preventDefault());
  doc.addEventListener("drop", (e) => {
    e.preventDefault();
    const f = e.dataTransfer?.files[0];
    if (f) guard(() => void f.text().then(load));
  });

  // ---- camera fly to an issue ------------------------------------------------------------------
  let fly: { from: Orbit; to: Orbit; t0: number; cell: Vec3i; anchor: Vec3i | null } | null = null;
  const reduced = (): boolean => win.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
  const show = (cell: Vec3i, anchor: Vec3i | null): void => {
    const from = deps.orbit?.() ?? null;
    const base = from ?? { target: [0, 0, 0] as [number, number, number], yaw: 0, pitch: 0.95, radius: 2.6 };
    const to = focusOrbit(cell, base);
    if (!from || reduced()) {
      act({ op: "camera", ...to });
      if (anchor) act({ op: "boxSelect", a: anchor, b: anchor });
      if (anchor && !reduced()) act({ op: "pulse", anchor });
      return;
    }
    fly = { from, to, t0: now(), cell: anchor ?? cell, anchor };
  };
  const stepFly = (): void => {
    if (!fly) return;
    const t = Math.min((now() - fly.t0) / FLY_MS, 1);
    const k = 1 - (1 - t) ** 3;
    const mix = (a: number, b: number): number => a + (b - a) * k;
    act({
      op: "camera",
      target: [0, 1, 2].map((i) => mix(fly?.from.target[i] ?? 0, fly?.to.target[i] ?? 0)) as [number, number, number],
      yaw: mix(fly.from.yaw, fly.to.yaw),
      pitch: mix(fly.from.pitch, fly.to.pitch),
      radius: mix(fly.from.radius, fly.to.radius),
    });
    if (t >= 1) {
      const { cell: c, anchor } = fly;
      fly = null;
      act({ op: "boxSelect", a: c, b: c });
      // Two pale-blue pulses of the selection mark on the piece (engine, real time).
      if (anchor) act({ op: "pulse", anchor });
    }
  };

  // ---- actions from clicks ---------------------------------------------------------------
  const commands: Record<string, () => void> = {
    rename: () => startRename(),
    library: () => setSheet(!sheet.classList.contains("open")),
    rotate: () => act({ op: "rotate", by: 1 }),
    mirror: () => act({ op: "mirror" }),
    undo: () => act({ op: "undo" }),
    redo: () => act({ op: "redo" }),
    down: () => d && act({ op: "slice", level: sliceDown(d.slice, d.maxLevel) }),
    up: () => d && act({ op: "slice", level: sliceUp(d.slice, d.maxLevel) }),
    sliceToggle: toggleSlice,
    columns: toggleColumns,
    one: () => run("one"),
    all: () => run("all"),
    interval: () => setMenu(!menu.classList.contains("open")),
    pause: () => act({ op: "pause" }),
    reset: () => act({ op: "reset" }),
    follow: () => d && act({ op: "follow", on: !d.run.follow }),
    run: () => (d?.run.active ? act({ op: "stop" }) : run(lastRelease)),
    check,
    ckQueue: () => {
      checkAllSizes = false;
      ckKey = "";
    },
    ckAll: () => {
      checkAllSizes = true;
      ckKey = "";
    },
    panel: () => {
      root.classList.toggle("side-closed");
      q(side, ".spine").setAttribute("aria-expanded", String(!root.classList.contains("side-closed")));
    },
    drawer: () => {
      root.classList.toggle("tray-closed");
      doc.body.style.setProperty("--pe-tray-h", root.classList.contains("tray-closed") ? "36px" : "174px");
      collapse.setAttribute(
        "title",
        root.classList.contains("tray-closed") ? "Open the drawer (Tab)" : "Collapse the drawer (Tab)",
      );
    },
    hms: () => act({ op: "tool", tool: "select" }),
    hsl: toggleSlice,
    gRotate: () => act({ op: "rotate", by: 1 }),
    gMirror: () => act({ op: "mirror" }),
    gDelete: () => remove(),
    nzAdd: () => addZone(),
    yesDelete: () => {
      confirmDelete = false;
      deletedNote = `Deleted ${d?.selection ?? 0} pieces.`;
      act({ op: "remove" });
      selKey = "";
    },
    noDelete: () => {
      confirmDelete = false;
      selKey = "";
    },
    undoDelete: () => {
      deletedNote = null;
      act({ op: "undo" });
      selKey = "";
    },
  };
  root.addEventListener("click", (e) => {
    const b = (e.target as HTMLElement).closest("button");
    if (!b || !root.contains(b) || sheet.contains(b)) return;
    if (b.dataset.tool) act({ op: "tool", tool: b.dataset.tool as "select" | "place" | "delete" });
    else if (b.dataset.s) {
      intervalS = Number(b.dataset.s);
      setMenu(false);
      run("interval");
    } else if (b.dataset.piece) pick(b.dataset.piece);
    else if (b.dataset.opt) {
      const [k, v] = b.dataset.opt.split(":");
      if (k && v !== undefined) act({ op: "mechConfig", key: k, value: Number(v) });
    } else if (b.dataset.nz) nudgeButton(b.dataset.nz);
    else if (b.dataset.nzmode) nudgeButton(`mode:${b.dataset.nzmode}`);
    else if (b.dataset.nzsel) {
      act({ op: "nudgeSelect", index: Number(b.dataset.nzsel) });
      act({ op: "tool", tool: "select" });
    } else if (b.dataset.nzmove) act({ op: "move", offset: b.dataset.nzmove.split(",").map(Number) as Vec3i });
    else if (b.dataset.show) {
      const [x, y, z, ax, ay, az] = b.dataset.show.split(",").map(Number) as number[];
      show([x, y, z] as Vec3i, Number.isFinite(ax) ? ([ax, ay, az] as Vec3i) : null);
    } else if (b.dataset.k && commands[b.dataset.k]) commands[b.dataset.k]?.();
    if (b.dataset.k !== "interval" && !b.dataset.s) setMenu(false);
  });

  /** The selected nudge zone, when no piece is selected (the two are never selected together). */
  const selectedZone = (s: EditorDiag | null): NudgeZoneDiag | null => {
    const n = s?.nudges;
    if (!s || !n || n.selected === null || s.selection > 0) return null;
    return n.zones.find((z) => z.index === n.selected) ?? null;
  };
  /** Add zone: over the selected pieces' cells, else over the cell at the camera's centre. Then the
   * Select tool, where R turns the zone (the Place tool's R turns the brush). */
  const addZone = (): void => {
    if (!d) return;
    if (d.selection > 0) act({ op: "nudgeAdd" });
    else {
      const c = cellAt(deps.orbit?.()?.target ?? [0, 0, 0]);
      act({ op: "nudgeAdd", cells: [c, c] });
    }
    act({ op: "tool", tool: "select" });
  };
  /** An inspector button, `size:<axis>:<±1>`, `<yaw|pitch|speed|accel>:<±1>` or
   * `mode:<top_up|constant>`: one nudgeSet. */
  const nudgeButton = (spec: string): void => {
    const z = selectedZone(d);
    if (!z) return;
    const [kind, a, b] = spec.split(":");
    const e: NudgeEdit =
      kind === "size"
        ? { kind, axis: Number(a) as 0 | 1 | 2, by: Number(b) as 1 | -1 }
        : kind === "mode"
          ? { kind, mode: a === "constant" ? "constant" : "top_up" }
          : { kind: kind as "yaw" | "pitch" | "speed" | "accel", by: Number(a) as 1 | -1 };
    const next = nudgeEdit(zoneOf(z), e);
    if (next) act({ op: "nudgeSet", index: z.index, zone: next });
  };

  const remove = (): void => {
    if (d && d.selection === 0 && selectedZone(d)) {
      act({ op: "remove" });
      return;
    }
    if (!d || d.selection === 0) return;
    if (d.selection > CONFIRM_DELETE) {
      confirmDelete = true;
      selKey = "";
      return;
    }
    act({ op: "remove" });
  };

  const pick = (piece: string): void => {
    act({ op: "brush", piece, yaw: d?.brush.piece === piece ? d.brush.yaw : 0, mirror: false });
    act({ op: "tool", tool: "place" });
  };

  const startRename = (): void => {
    const input = h(doc, "input", { class: "track-in", "aria-label": "Track name", maxlength: "120" });
    input.value = d?.name ?? "";
    trackBtn.replaceWith(input);
    input.focus();
    input.select();
    let done = false;
    const finish = (save: boolean): void => {
      if (done) return;
      done = true;
      const name = input.value.trim();
      if (save && name && name !== d?.name) act({ op: "rename", name });
      input.replaceWith(trackBtn);
      trackBtn.focus();
    };
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") finish(true);
      else if (e.key === "Escape") finish(false);
      e.stopPropagation();
    });
    input.addEventListener("blur", () => finish(true));
  };

  const setFamily = (f: Family): void => {
    family = f;
    tileFor = "";
    if (d) renderTiles(d);
  };

  // ---- renders ---------------------------------------------------------------------------
  const renderTiles = (s: EditorDiag): void => {
    const key = `${family}|${s.palette.length}`;
    if (key !== tileFor) {
      tileFor = key;
      tiles.innerHTML = piecesOf(s.palette, family)
        .map(
          (
            p,
          ) => `<div class="pc"><button class="tile" role="option" data-piece="${p}" aria-selected="false" title="${esc(pieceName(p))}">
<img alt="" src="${versioned(win, `${THUMB_DIR}${p}.png`)}" width="80" height="80" decoding="async" draggable="false"><b class="ct" data-ct="${p}"></b></button><span class="nm">${esc(pieceName(p))}</span></div>`,
        )
        .join("");
      tiles.append(tilePin.el);
      tiles.scrollLeft = 0;
      edgeCue();
      tilePin.refresh();
      countFor = "";
    }
    const counts = s.pieceCounts ?? {};
    const ck = `${key}|${s.edits}`;
    if (ck !== countFor) {
      countFor = ck;
      for (const b of tiles.querySelectorAll<HTMLElement>("[data-ct]")) {
        const n = counts[b.dataset.ct ?? ""];
        const text = countBadge(n);
        if (b.textContent !== text) b.textContent = text;
        b.setAttribute("aria-label", n ? `${n} placed` : "");
      }
    }
    for (const t of tabs.querySelectorAll<HTMLElement>("[data-f]")) {
      const on = t.dataset.f === family;
      t.classList.toggle("on", on);
      t.setAttribute("aria-selected", String(on));
      if (on) tabPin.to(t);
    }
    let active: HTMLElement | null = null;
    for (const t of tiles.querySelectorAll<HTMLElement>("[data-piece]")) {
      const on = s.tool === "place" && t.dataset.piece === s.brush.piece;
      t.classList.toggle("on", on);
      t.setAttribute("aria-selected", String(on));
      if (on) active = t;
    }
    tilePin.to(active);
  };

  /** Inspector rows for a piece's options: a segmented control per option, the brass pin on the set value. */
  const optionRows = (options: readonly PieceOption[]): string =>
    options
      .map(
        (o) =>
          `<span class="sc">${esc(OPTION_LABEL[o.key] ?? o.key)}</span><div class="seg opt" role="radiogroup" aria-label="${esc(OPTION_LABEL[o.key] ?? o.key)}">${o.choices
            .map(
              (c) =>
                `<button role="radio" data-opt="${esc(o.key)}:${c.value}" aria-checked="${c.value === o.value}"${c.value === o.value ? ' class="on"' : ""}>${c.value === o.value ? '<i class="pin"></i>' : ""}${esc(c.label ?? (c.face ? faceWord(c.face) : String(c.value)))}</button>`,
            )
            .join("")}</div>`,
      )
      .join("");

  /** A row of inspector buttons: [label, data attribute, aria-label]. */
  const nzRow = (label: string, buttons: [string, string, string][]): string =>
    `<span class="sc">${label}</span><div class="seg nzs">${buttons
      .map(([text, attr, aria]) => `<button ${attr} aria-label="${esc(aria)}">${text}</button>`)
      .join("")}</div>`;
  /** The zone's Mode: a segmented radio like a piece option's, the brass pin on the set mode. */
  const nzModeRow = (z: NudgeZoneDiag): string =>
    `<span class="sc">Mode</span><div class="seg opt" role="radiogroup" aria-label="Mode">${NUDGE_MODE_LABELS.map(
      ([m, label]) => {
        const on = nudgeMode(z) === m;
        return `<button role="radio" data-nzmode="${m}" aria-checked="${on}"${on ? ' class="on"' : ""}>${on ? '<i class="pin"></i>' : ""}${label}</button>`;
      },
    ).join("")}</div>`;
  const zoneInspector = (z: NudgeZoneDiag): string => {
    const constant = nudgeMode(z) === "constant";
    return `<h2><span class="sc">Nudge zone:</span>${esc(nudgeName(z))}</h2>
<div class="kv"><span class="sc">Box</span><span>${nudgeSizeText(z)} · ${cellText(z.cell)}</span>
<span class="sc">Heading</span><span>${esc(nudgeHeadingText(z.yawMdeg, z.pitchMdeg))}</span>
${nzModeRow(z)}
${
  constant
    ? ""
    : `<span class="sc">Min speed</span><span>${nudgeSpeedText(z.minSpeedUmS)}</span>
`
}<span class="sc">${constant ? "Push" : "Max push"}</span><span>${nudgeAccelText(z.maxAccelUmS2)}</span>
${nzRow("Size", [
  ["X−", 'data-nz="size:0:-1"', "Shrink along x"],
  ["X+", 'data-nz="size:0:1"', "Grow along x"],
  ["Y−", 'data-nz="size:1:-1"', "Shrink in height"],
  ["Y+", 'data-nz="size:1:1"', "Grow in height"],
  ["Z−", 'data-nz="size:2:-1"', "Shrink along z"],
  ["Z+", 'data-nz="size:2:1"', "Grow along z"],
])}
${nzRow("Move", [
  ["W", 'data-nzmove="-1,0,0"', "Move west a cell"],
  ["E", 'data-nzmove="1,0,0"', "Move east a cell"],
  ["N", 'data-nzmove="0,0,-1"', "Move north a cell"],
  ["S", 'data-nzmove="0,0,1"', "Move south a cell"],
  ["Up", 'data-nzmove="0,1,0"', "Move up a level"],
  ["Dn", 'data-nzmove="0,-1,0"', "Move down a level"],
])}
${nzRow("Aim", [
  ["↺ 45°", 'data-nz="yaw:1"', "Turn the heading 45° anticlockwise"],
  ["↻ 45°", 'data-nz="yaw:-1"', "Turn the heading 45° clockwise"],
  ["Tilt up", 'data-nz="pitch:1"', "Tilt the heading up 15°"],
  ["Tilt dn", 'data-nz="pitch:-1"', "Tilt the heading down 15°"],
])}
${
  constant
    ? ""
    : nzRow("Speed", [
        ["Slower", 'data-nz="speed:-1"', "Lower the minimum speed 0.05 m/s"],
        ["Faster", 'data-nz="speed:1"', "Raise the minimum speed 0.05 m/s"],
      ])
}
${nzRow("Push", [
  ["Softer", 'data-nz="accel:-1"', "Lower the push 0.25 m/s²"],
  ["Stronger", 'data-nz="accel:1"', "Raise the push 0.25 m/s², at most 2 m/s²"],
])}</div>
<div class="acts"><button class="btn" data-k="gDelete">${icon("del")}Delete zone <kbd>⌫</kbd></button></div>
<p class="note">Invisible in a run. ${
      constant
        ? "Every marble inside is pushed along the arrow at the push rate, however fast it is already going."
        : "A marble inside moving slower than the min speed along the arrow is pushed up to it, never harder than the max push."
    } Arrows and PgUp/PgDn move it, R turns it.</p>`;
  };

  let nzKey = "";
  const renderNudges = (s: EditorDiag): void => {
    const n = s.nudges;
    const key = n
      ? `${s.edits}|${n.count}|${n.selected}|${s.selection}|${n.zones.map((z) => `${z.cell}|${z.yawMdeg}|${z.minSpeedUmS}|${z.maxAccelUmS2}|${nudgeMode(z)}`).join(";")}`
      : "none";
    if (key === nzKey) return;
    nzKey = key;
    nzSec.hidden = !n;
    if (!n) return;
    nzHead.textContent = n.count === 0 ? "none" : String(n.count);
    nzAddBtn.title =
      s.selection > 0
        ? "Add a zone over the selected pieces' cells"
        : "Add a one-cell zone at the camera's centre (select pieces first to cover them)";
    nzWhere.textContent = s.selection > 0 ? "over the selection" : "at the camera's centre";
    if (n.count === 0) {
      nzList.innerHTML = `<p class="note">Invisible boxes that push slow marbles along an arrow, to carry them past a weak spot. Drawn here only while editing, never in a run.</p>`;
      return;
    }
    nzList.innerHTML =
      n.zones
        .map((z) => {
          const on = z.index === n.selected && s.selection === 0;
          return `<div class="iss${on ? " on" : ""}"><i class="pin${on ? "" : " bl"}"></i><i>${cellText(z.cell)}</i><button data-nzsel="${z.index}" aria-label="Select ${esc(nudgeName(z))}">${on ? "Selected" : "Select"}</button><span>${esc(nudgeName(z))} · ${esc(nudgeHeadingText(z.yawMdeg, z.pitchMdeg))} · ${nudgeMode(z) === "constant" ? `constant ${nudgeAccelText(z.maxAccelUmS2)}` : nudgeSpeedText(z.minSpeedUmS)}</span></div>`;
        })
        .join("") + (n.count > n.zones.length ? `<p class="note">and ${n.count - n.zones.length} more</p>` : "");
  };

  let selKey = "";
  const renderSelection = (s: EditorDiag): void => {
    const zone = selectedZone(s);
    const key = `${s.edits}|${s.selection}|${s.selected.join(";")}|${s.pieces}|${s.hoppers}|${s.finishes}|${s.tool}|${s.brush.piece}|${s.brush.yaw}|${s.brush.mirror}|${confirmDelete}|${deletedNote}|${zone ? JSON.stringify(zoneOf(zone)) + zone.index : ""}`;
    if (key === selKey) return;
    selKey = key;
    const info = s.selectionInfo;
    if (zone && !confirmDelete) {
      selSec.innerHTML = zoneInspector(zone);
      return;
    }
    if (s.pieces === 0 && !deletedNote) {
      selSec.innerHTML = `<h2><span class="sc">Start here</span></h2>
<ol class="steps"><li${s.hoppers ? ' class="done"' : ""}><span>Place a <b>Hopper</b>: Mechanisms, first tile. It holds the marbles. New pieces land on the build level, L${START_LEVEL} to start: <kbd>[</kbd> and <kbd>]</kbd> lower and raise it, <kbd>X</kbd> shows every level.</span></li>
<li><span>Lay a downhill run of blocks and rails from the hopper's spout.</span></li>
<li${s.finishes ? ' class="done"' : ""}><span>End it in <b>Finish cups</b>, then press <b>Run</b>.</span></li></ol>
<p class="note">Click a tile, then a cell in the scene. R turns the piece before you place it.</p>`;
      return;
    }
    if (confirmDelete) {
      selSec.innerHTML = `<h2><span class="sc">Delete ·</span>${s.selection} pieces?</h2>
<p class="note">That is more than ${CONFIRM_DELETE} pieces. Undo can bring them back.</p>
<div class="acts"><button class="btn warn" data-k="yesDelete">${icon("del")}Delete ${s.selection}</button><button class="btn" data-k="noDelete">Keep them</button></div>`;
      return;
    }
    if (deletedNote && s.selection === 0) {
      selSec.innerHTML = `<h2><span class="sc">Selection</span></h2><p class="note">${esc(deletedNote)}</p>
<div class="acts"><button class="btn" data-k="undoDelete">${icon("und")}Undo</button></div>`;
      return;
    }
    deletedNote = null;
    if (!info) {
      const b = s.brush;
      selSec.innerHTML =
        s.tool === "place"
          ? `<h2><span class="sc">Placing:</span>${esc(pieceName(b.piece))}</h2>
<div class="kv"><span class="sc">Turn</span><span>${b.yaw * 90}°${b.mirror ? " · mirrored" : ""}</span>
<span class="sc">Keys</span><span><kbd>R</kbd> turn · <kbd>M</kbd> mirror · <kbd>Esc</kbd> stop placing</span></div>`
          : `<h2><span class="sc">Selection</span></h2><p class="note">Nothing selected. With Select <kbd>1</kbd>, click a piece, Shift-click to add, or Shift-drag a box.</p>`;
      return;
    }
    const groupActs = `<div class="seg acts-seg"><button data-k="gRotate">${icon("rot")}Rotate <kbd>R</kbd></button><button data-k="gMirror">${icon("mir")}Mirror <kbd>M</kbd></button><button data-k="gDelete">${icon("del")}Delete <kbd>⌫</kbd></button></div>`;
    const levels = info.levels[0] === info.levels[1] ? `L${info.levels[0]}` : `L${info.levels[0]}–L${info.levels[1]}`;
    if (info.single) {
      const p = info.single;
      selSec.innerHTML = `<h2><span class="sc">Selected:</span>${esc(pieceName(p.piece))}</h2>
<div class="kv"><span class="sc">Ports</span><span>${esc(portsText(p.ports))}</span>
<span class="sc">Level</span><span>L${p.anchor[1]}</span>
<span class="sc">Cell</span><span>${cellText(p.anchor)}</span>
<span class="sc">Turn</span><span>${p.yaw * 90}°${p.mirror ? " · mirrored" : ""}</span>${optionRows(p.options ?? [])}</div>
${p.options?.length ? `<p class="note">Piece options take effect on the next Run or Reset.</p>` : ""}
${groupActs}`;
      return;
    }
    selSec.innerHTML = `<h2><span class="sc">Selected:</span>${info.count} pieces</h2>
<div class="kv"><span class="sc">Pieces</span><span>${esc(familyText(info.families))}</span><span class="sc">Levels</span><span>${levels}</span></div>
${groupActs}
<p class="note">Arrows nudge a cell, PgUp and PgDn a level. Drag a selected piece to move the group.</p>`;
  };

  let valKey = "";
  const renderValidation = (s: EditorDiag): void => {
    const key = `${s.edits}|${s.issueCount}|${s.pieces === 0}`;
    if (key === valKey) return;
    valKey = key;
    if (s.pieces === 0) {
      vHead.textContent = "nothing placed";
      vList.innerHTML = `<p class="note">Issues appear here as you build: open ports, broken links and pieces no marble reaches.</p>`;
      return;
    }
    if (s.issueCount === 0) {
      vHead.innerHTML = "Track is complete";
      vList.innerHTML = `<p class="ok"><i class="pin"></i>No open ports, broken links or unreachable pieces.</p>`;
      return;
    }
    vHead.textContent = `${s.issueCount} issue${s.issueCount === 1 ? "" : "s"}`;
    vList.innerHTML =
      s.issues
        .map((i) => {
          const cell = i.cell ?? i.anchor;
          const where = cell ? `<i>${cellText(cell)}</i>` : "<i>Track</i>";
          const btn = cell
            ? `<button data-show="${[...cell, ...(i.anchor ?? [])].join(",")}" aria-label="Show ${esc(issueText(i))}">Show</button>`
            : "<span></span>";
          return `<div class="iss"><i class="pin bl"></i>${where}${btn}<span>${esc(issueText(i))}</span></div>`;
        })
        .join("") +
      (s.issueCount > s.issues.length ? `<p class="note">and ${s.issueCount - s.issues.length} more</p>` : "");
  };

  // The marble check: warnings under the issues, never issues themselves. A result is stale after
  // an edit, or once the Run release differs from the release it checked.
  let ckKey = "";
  const renderCheck = (s: EditorDiag): void => {
    const c: CheckDiag | undefined = s.check;
    const now = releaseOf(lastRelease);
    const r = c?.release ?? null;
    const otherRelease =
      !!r && (r.release !== now.release || (now.release === "interval" && r.intervalTicks !== now.intervalTicks));
    const state = c?.state ?? "none";
    const key = `${state}|${Math.floor((c?.progress ?? 0) * 100)}|${c?.current}|${c?.warningCount}|${c?.edits}|${otherRelease}|${checkAllSizes}|${s.hoppers}|${s.pieces === 0}`;
    if (key === ckKey) return;
    ckKey = key;
    ckSec.hidden = s.pieces === 0;
    ckSizes.innerHTML = [
      ["ckQueue", "Queue size", !checkAllSizes],
      ["ckAll", "All sizes", checkAllSizes],
    ]
      .map(
        ([k, label, on]) =>
          `<button role="radio" data-k="${k}" aria-checked="${on}"${on ? ' class="on"' : ""}>${on ? '<i class="pin"></i>' : ""}${label}</button>`,
      )
      .join("");
    const running = state === "running";
    ckBtn.disabled = s.hoppers === 0;
    ckBtn.textContent = running ? "Checking" : state === "none" ? "Check" : "Check again";
    ckBtn.title =
      s.hoppers === 0
        ? "Place a hopper first: the check runs its release"
        : "Run the release Run makes once per marble type, off the scene, and list where marbles fall off or stall";
    const stale = state === "stale" || (state === "done" && otherRelease);
    ckSec.classList.toggle("stale", stale);
    if (!c || state === "none") {
      ckList.innerHTML = `<p class="note">Runs the Run release once per marble type and lists where marbles fall off or stall. These are warnings: a run can still go.</p>`;
      return;
    }
    if (state === "failed") {
      ckList.innerHTML = `<p class="note">The check could not run: ${esc(c.error ?? "unknown error")}</p>`;
      return;
    }
    if (running) {
      ckList.innerHTML = `<p class="note">Checking ${esc(c.current ?? "")} · ${Math.floor(c.progress * 100)}%</p>`;
      return;
    }
    const named = r ? `<p class="note">Checked: ${esc(checkReleaseText(r))}</p>` : "";
    const why = stale
      ? `<p class="note">Out of date: ${state === "stale" ? "the track changed" : "the Run release changed"} since this check. Check again.</p>`
      : "";
    if (c.warningCount === 0) {
      ckList.innerHTML = `${why}<p class="ok"><i class="pin"></i>Every marble type reaches a cup.</p>${named}`;
      return;
    }
    ckList.innerHTML =
      why +
      c.warnings
        .map((w) => {
          const cell = w.cell ?? w.anchor;
          const where = cell ? `<i>${cellText(cell)}</i>` : "<i>Track</i>";
          const btn = cell
            ? `<button data-show="${[...cell, ...(w.anchor ?? [])].join(",")}" aria-label="Show ${esc(checkText(w))}">Show</button>`
            : "<span></span>";
          return `<div class="iss wn"><i class="pin bl"></i>${where}${btn}<span>${esc(checkText(w))}</span></div>`;
        })
        .join("") +
      (c.warningCount > c.warnings.length ? `<p class="note">and ${c.warningCount - c.warnings.length} more</p>` : "") +
      named;
  };

  let topKey = "";
  const renderTop = (s: EditorDiag): void => {
    const r = s.run;
    const columnsOn = s.columns?.on ?? true;
    const key = `${s.tool}|${s.canUndo}|${s.canRedo}|${s.slice}|${s.maxLevel}|${columnsOn}|${s.name}|${r.active}|${r.paused}|${r.follow}|${r.queue}|${r.release}|${s.hoppers}|${intervalS}`;
    if (key === topKey) return;
    topKey = key;
    if (trackBtn.textContent !== s.name) trackBtn.textContent = s.name;
    for (const t of ["select", "place", "delete"]) {
      const b = toolBtn(t);
      const on = s.tool === t;
      b.classList.toggle("on", on);
      b.setAttribute("aria-pressed", String(on));
      if (on) toolPin.to(b);
    }
    q<HTMLButtonElement>(top, '[data-k="undo"]').disabled = !s.canUndo;
    q<HTMLButtonElement>(top, '[data-k="redo"]').disabled = !s.canRedo;
    sliceBtn.textContent = levelLabel(s.slice, s.maxLevel);
    sliceBtn.setAttribute("aria-label", s.slice === null ? "All levels shown" : `Levels up to ${s.slice} shown`);
    columnsBtn.classList.toggle("on", columnsOn);
    columnsBtn.setAttribute("aria-pressed", String(columnsOn));
    const noHopper = s.hoppers === 0;
    for (const k of ["one", "all", "interval"]) {
      const b = q<HTMLButtonElement>(top, `[data-k="${k}"]`);
      b.disabled = noHopper;
      b.classList.toggle("on", r.active && r.release === k);
    }
    const pause = q<HTMLButtonElement>(top, '[data-k="pause"]');
    pause.disabled = !r.active;
    pause.setAttribute("aria-pressed", String(r.paused));
    pausePin.to(r.active && r.paused ? pause : null);
    q<HTMLButtonElement>(top, '[data-k="reset"]').disabled = !r.active;
    const follow = q(top, '[data-k="follow"]');
    follow.setAttribute("aria-pressed", String(r.follow));
    followPin.to(r.follow ? follow : null);
    // While running, the pins count the marbles out of the budget; otherwise they set the budget.
    const lit = r.active ? Math.min(r.marbles, r.queue) : r.queue;
    countEl.textContent = `${lit} / ${r.active ? r.queue : MAX_MARBLES}`;
    pinsEl.querySelectorAll("button").forEach((b, i) => {
      b.querySelector(".pin")?.classList.toggle("off", i >= lit);
      b.setAttribute("aria-pressed", String(i + 1 === r.queue));
    });
    for (const b of menu.querySelectorAll<HTMLElement>("[data-s]")) {
      b.querySelector(".pin")?.classList.toggle("off", Number(b.dataset.s) !== intervalS);
      b.setAttribute("aria-checked", String(Number(b.dataset.s) === intervalS));
    }
    runBtn.classList.toggle("stop", r.active);
    runBtn.innerHTML = r.active ? `${icon("stop")}Stop <kbd>Esc</kbd>` : `${icon("play")}Run`;
    runBtn.disabled = !r.active && noHopper;
    runBtn.title = r.active
      ? "Stop the run (Esc)"
      : noHopper
        ? "Place a hopper first: Run releases its marbles"
        : "Release the hopper's marbles (Space)";
    root.classList.toggle("running", r.active);
  };

  let hintKey = "";
  const renderHint = (s: EditorDiag): void => {
    let next = hint;
    if (s.pieces >= 2 && announce("multi"))
      next = "Shift-drag on empty space with Select to box-select several pieces, then move them as one.";
    else if (s.maxLevel >= 1 && announce("slice"))
      next = "Pieces stack in levels. X slices the view at a level so lower ones are easy to reach.";
    else if (s.pieces >= 20 && announce("nudge"))
      next = "With pieces selected, the arrow keys nudge them one cell and PgUp or PgDn one level.";
    else if (s.run.active && announce("interval"))
      next = "Interval releases a marble every 0.5, 1 or 2 seconds. Pick it from the Interval menu.";
    if (!next || hintKey !== `${s.tool}`) {
      hintKey = `${s.tool}`;
      if (!next)
        next =
          s.tool === "place"
            ? "Pick a tile, then click a cell to place it. R turns it first."
            : s.tool === "delete"
              ? "Click a piece to remove it. Undo brings it back."
              : "Click a piece to select it. Shift-click adds to the selection.";
    }
    if (next !== hint) {
      hint = next;
      hintText.textContent = hint;
    }
  };

  let tipKey = "";
  const renderTip = (s: EditorDiag): void => {
    let text: string | null = null;
    let valid = true;
    if (s.stroke) {
      text = strokeText(s.stroke.placed);
    } else if (s.drag) {
      text = `Move ${cellText(s.drag.offset)} · ${s.drag.valid ? "valid" : "blocked"}`;
      valid = s.drag.valid;
    } else if (s.tool === "place" && pointer.on) {
      const g = ghostText(s.hover);
      if (g) ({ text, valid } = g);
    }
    const key = `${text}|${valid}`;
    if (key !== tipKey) {
      tipKey = key;
      tip.classList.toggle("show", text !== null);
      tip.classList.toggle("bad", !valid);
      tipPin.className = valid ? "pin" : "pin rd";
      if (text !== null) tipText.textContent = text;
    }
    if (text !== null) tip.style.transform = `translate(${pointer.x + 18}px, ${pointer.y + 22}px)`;
  };

  let ghostKey = "";
  const renderGhost = (s: EditorDiag): void => {
    const on = s.tool === "place" && pointer.on && !s.run.active;
    const box = on && !s.stroke ? boxPath(s.hover?.outline) : "";
    const run = on ? (s.stroke?.outlines ?? []).map((o) => boxPath(o)).join("") : "";
    const bad = !!box && !(s.hover?.valid ?? false);
    const k = `${box}|${run}|${bad}`;
    if (k === ghostKey) return;
    ghostKey = k;
    ghostBox.setAttribute("d", box);
    ghostBox.setAttribute("class", bad ? "bad" : "ok");
    ghostRun.setAttribute("d", run);
  };

  let nzGhostKey = "";
  const renderNudgeGhost = (s: EditorDiag): void => {
    const zones = s.run.active || s.run.building === true ? [] : (s.nudges?.zones ?? []);
    const sel = selectedZone(s)?.index ?? null;
    const part = (pick: boolean, f: (z: NudgeZoneDiag) => string): string =>
      zones
        .filter((z) => (z.index === sel) === pick)
        .map(f)
        .join("");
    const boxes = part(false, (z) => boxPath(z.outline));
    const arrows = part(false, (z) => arrowPath(z.arrow));
    const selBox = part(true, (z) => boxPath(z.outline));
    const selArrow = part(true, (z) => arrowPath(z.arrow));
    const k = `${boxes}|${arrows}|${selBox}|${selArrow}`;
    if (k === nzGhostKey) return;
    nzGhostKey = k;
    nzBoxes.setAttribute("d", boxes);
    nzArrows.setAttribute("d", arrows);
    nzSelBox.setAttribute("d", selBox);
    nzSelArrow.setAttribute("d", selArrow);
  };

  let capKey = "";
  let fadeKey = "";
  const renderRun = (s: EditorDiag): void => {
    const t = now();
    const building = s.run.building === true;
    root.classList.toggle("building", building);
    if (building) {
      const ck = `b|${s.run.buildOp}|${Math.floor((s.run.buildProgress ?? 0) * 100)}`;
      if (ck !== capKey) {
        capKey = ck;
        const label = s.run.buildOp === "run" ? "Building run" : "Building";
        cap.innerHTML = `<i class="pin"></i><span class="sc">${label}</span><b>${Math.floor((s.run.buildProgress ?? 0) * 100)}%</b><span>${s.pieces} pieces</span>`;
      }
    }
    fade.setRunning(s.run.active, t);
    const v = fade.view(t);
    const fk = `${v.faded}|${[...v.shown].join()}`;
    if (fk !== fadeKey) {
      fadeKey = fk;
      root.classList.toggle("faded", v.faded);
      doc.body.classList.toggle("pe-faded", v.faded);
      for (const e of ["top", "bottom", "right"] as const) {
        root.classList.toggle(`show-${e}`, v.shown.has(e));
        doc.body.classList.toggle(`pe-show-${e}`, v.shown.has(e));
      }
    }
    if (v.faded && !building) {
      const ck = `${s.run.marbles}|${Math.floor(s.run.tick / 24)}|${s.run.paused}`;
      if (ck !== capKey) {
        capKey = ck;
        cap.innerHTML = `<i class="pin"></i><span class="sc">${s.run.paused ? "Paused" : "Running"}</span><b>${s.run.marbles} / ${s.run.queue}</b><span>${runTime(s.run.tick)}</span><span><kbd>Space</kbd> ${s.run.paused ? "resume" : "pause"}</span>`;
      }
    }
  };

  const renderMarqueeLabel = (s: EditorDiag): void => {
    if (marqueeEnd && s.selectionInfo !== undefined) {
      const info = s.selectionInfo;
      const text = info
        ? `${info.count} piece${info.count === 1 ? "" : "s"} · ${info.levels[0] === info.levels[1] ? `L${info.levels[0]}` : `L${info.levels[0]}–L${info.levels[1]}`}`
        : "No pieces";
      mlabel.textContent = text;
      mlabel.style.transform = `translate(${marqueeEnd.x + 12}px, ${marqueeEnd.y + 12}px)`;
      mlabel.classList.add("show");
      mlabelUntil = now() + 1_400;
      marqueeEnd = null;
    } else if (mlabelUntil && now() > mlabelUntil) {
      mlabelUntil = 0;
      mlabel.classList.remove("show");
    }
  };

  let firstRun = true;
  const update = (next: EditorDiag | null, events: readonly BridgeEvent[] = []): void => {
    for (const e of events) {
      if (e.type === "trackExported") {
        const why = pending.shift();
        if (why === "download") {
          downloadText(doc, fileName(e.name), e.json);
          status(`Exported ${fileName(e.name)}`);
          if (d) savedEdits = d.edits;
        } else if (why === "library") {
          void deps.library
            ?.then((lib) => lib.save(e.json))
            .then(() => {
              status(`Saved ${e.name}`);
              if (d) savedEdits = d.edits;
              return refreshList();
            })
            .catch((err: unknown) => status(`Save failed: ${String(err)}`, true));
        }
      } else if (e.type === "trackLoaded") {
        cleanOnNext = true;
        status(`Loaded ${e.name}`);
      } else if (e.type === "bridgeError") {
        status(`${e.command} failed: ${e.message}`, true);
      }
    }
    if (!next) return;
    d = next;
    if (cleanOnNext) {
      cleanOnNext = false;
      savedEdits = next.edits;
    }
    if (firstRun) {
      firstRun = false;
      act({ op: "keys", enabled: false });
      if (next.pieces === 0) {
        family = "mechanisms";
        pick("mech_hopper");
        act({ op: "slice", level: START_LEVEL });
      } else family = familyOf(next.brush.piece);
    }
    stepFly();
    renderTop(next);
    renderTiles(next);
    renderSelection(next);
    renderValidation(next);
    renderCheck(next);
    renderNudges(next);
    renderHint(next);
    renderTip(next);
    renderGhost(next);
    renderNudgeGhost(next);
    renderRun(next);
    renderMarqueeLabel(next);
  };

  const key = (e: KeyboardEvent): boolean => {
    if (!d || ownsKeys(e.target) || e.isComposing) return false;
    const cv = canvas();
    // Synthetic keys on the canvas (pointer-lock's Escape, the F forwarded below) are the engine's.
    if (cv && e.target === cv && !e.isTrusted) return false;
    // The fly camera owns every key (WASD, Space, Q/E, Esc) until it is left.
    if (deps.cameraMode?.() === "fly") return false;
    // The engine hears its own keys while the canvas has focus; answering them too would double them.
    if (ENGINE_KEYS.has(e.code) && cv && (doc.activeElement === cv || e.target === cv)) return false;
    if (e.code === "Escape" && laying) {
      if (laying === "open") act({ op: "stroke", phase: "cancel" });
      laying = false;
      return true;
    }
    if (e.code === "Escape" && menu.classList.contains("open")) {
      setMenu(false);
      return true;
    }
    if (e.code === "Escape" && sheet.classList.contains("open")) {
      setSheet(false);
      return true;
    }
    const r = keyResult(e, d);
    if (!r) return false;
    const active = doc.activeElement;
    const focusFree = !active || active === doc.body || active === canvas();
    if ((e.code === "Space" || e.code === "Tab") && !focusFree) return false;
    if (r.kind === "actions") {
      for (const a of r.actions) act(a);
      return true;
    }
    switch (r.command) {
      case "space":
        if (d.run.active) act({ op: "pause" });
        else if (d.hoppers > 0) run(lastRelease);
        break;
      case "escape":
        if (confirmDelete) commands.noDelete?.();
        else if (d.run.active) act({ op: "stop" });
        else if (d.tool === "place") act({ op: "tool", tool: "select" });
        else act({ op: "clearSelection" });
        break;
      case "remove":
        remove();
        break;
      case "sliceToggle":
        toggleSlice();
        break;
      case "columns":
        toggleColumns();
        break;
      case "drawer":
        commands.drawer?.();
        break;
      case "panel":
        commands.panel?.();
        break;
      case "run":
        commands.run?.();
        break;
      case "follow":
        commands.follow?.();
        break;
      case "fly":
        // Fly is the engine's (it saves the view it leaves); hand it the key on the canvas.
        // pointer-lock.ts already asked for the lock from this same trusted key press.
        if (cv) {
          for (const type of ["keydown", "keyup"])
            cv.dispatchEvent(new KeyboardEvent(type, { code: "KeyF", key: "f", bubbles: true }));
        }
        break;
    }
    return true;
  };
  win.addEventListener("keydown", (e) => {
    if (key(e)) e.preventDefault();
  });

  return { root, update, key, dirty };
}
