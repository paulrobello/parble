// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { BridgeCommand } from "../src/bridge";
import type { Action, EditorDiag, PieceOption } from "../src/editor/model";
import { CONFIRM_DELETE, type EditorOverlay, FLY_MS, mountEditor } from "../src/editor/overlay";

const PALETTE = [
  "blk_curve90_l3",
  "blk_end_l3",
  "blk_pillar",
  "blk_straight_l3",
  "mech_finish",
  "mech_hopper",
  "mech_splitter_l3",
  "rail_straight_l2",
];

function diag(over: Partial<EditorDiag> = {}): EditorDiag {
  return {
    tool: "select",
    brush: { piece: "blk_straight_l3", yaw: 0, mirror: false },
    palette: PALETTE,
    pieces: 3,
    visiblePieces: 3,
    slice: null,
    selection: 0,
    selected: [],
    canUndo: true,
    canRedo: false,
    hover: null,
    drag: null,
    edits: 3,
    seq: 10,
    last: { id: null, op: "place", ok: true, error: null },
    keys: false,
    name: "Lantern Falls",
    run: {
      active: false,
      release: "all",
      intervalTicks: 240,
      queue: 3,
      marbles: 0,
      tick: 0,
      paused: false,
      follow: false,
    },
    hoppers: 1,
    finishes: 1,
    maxLevel: 4,
    issueCount: 0,
    issues: [],
    selectionInfo: null,
    ...over,
  };
}

let sent: Action[];
let posted: BridgeCommand[];
let clock: number;
let ui: EditorOverlay;
let orbit: { target: [number, number, number]; yaw: number; pitch: number; radius: number } | null;
let mode: string | null;

const $ = <T extends HTMLElement = HTMLElement>(sel: string): T => {
  const el = document.querySelector(sel);
  if (!el) throw new Error(`no ${sel}`);
  return el as T;
};
const click = (sel: string): void => $(sel).click();
const press = (code: string, mods: KeyboardEventInit = {}): boolean =>
  ui.key(new KeyboardEvent("keydown", { code, key: code, bubbles: true, ...mods }));

beforeEach(() => {
  document.head.innerHTML = "";
  document.body.innerHTML = '<canvas id="parble"></canvas>';
  document.body.className = "";
  localStorage.clear();
  sent = [];
  posted = [];
  clock = 0;
  orbit = null;
  mode = "orbit";
  ui = mountEditor({
    doc: document,
    win: window,
    send: (a) => sent.push(a),
    post: (c) => posted.push(c),
    library: null,
    now: () => clock,
    orbit: () => orbit,
    cameraMode: () => mode,
  });
});

afterEach(() => {
  ui.root.remove();
});

describe("editor overlay", () => {
  it("mounts once with self-hosted fonts and takes the keys from the engine", () => {
    ui.update(diag());
    expect(sent[0]).toEqual({ op: "keys", enabled: false });
    const css = $("#pe-style").textContent ?? "";
    expect(css).toContain("web/ui/fonts/alegreya-sans-latin-500-normal.woff2");
    expect(css).not.toMatch(/googleapis|gstatic/);
    expect(css).not.toContain("backdrop-filter");
    expect(document.body.classList.contains("parble-editor")).toBe(true);
  });

  it("tool buttons send the tool and mark the active one", () => {
    ui.update(diag({ tool: "place" }));
    expect($('[data-tool="place"]').getAttribute("aria-pressed")).toBe("true");
    expect($('[data-tool="select"]').getAttribute("aria-pressed")).toBe("false");
    click('[data-tool="delete"]');
    expect(sent.at(-1)).toEqual({ op: "tool", tool: "delete" });
    for (const k of ["select", "place", "delete"]) expect($(`[data-tool="${k}"] kbd`).textContent).toMatch(/^[123]$/);
  });

  it("rotate, mirror, undo and redo send their actions; redo is off with nothing to redo", () => {
    ui.update(diag());
    click('[data-k="rotate"]');
    click('[data-k="mirror"]');
    click('[data-k="undo"]');
    expect(sent.slice(-3)).toEqual([{ op: "rotate", by: 1 }, { op: "mirror" }, { op: "undo" }]);
    expect($<HTMLButtonElement>('[data-k="redo"]').disabled).toBe(true);
  });

  it("the level stepper slices down and up and toggles all levels", () => {
    ui.update(diag());
    expect($('[data-k="sliceToggle"]').textContent).toBe("L0–L4");
    click('[data-k="down"]');
    expect(sent.at(-1)).toEqual({ op: "slice", level: 4 });
    ui.update(diag({ slice: 2 }));
    expect($('[data-k="sliceToggle"]').textContent).toBe("≤ L2");
    click('[data-k="up"]');
    expect(sent.at(-1)).toEqual({ op: "slice", level: 3 });
    click('[data-k="sliceToggle"]');
    expect(sent.at(-1)).toEqual({ op: "slice", level: null });
    ui.update(diag({ slice: null }));
    expect(press("KeyX")).toBe(true);
    expect(sent.at(-1)).toEqual({ op: "slice", level: 2 });
  });

  it("run controls release, pause, reset and follow; Run becomes Stop during a run", () => {
    ui.update(diag());
    click('[data-k="one"]');
    expect(sent.at(-1)).toEqual({ op: "run", release: "one" });
    click('[data-k="interval"]');
    expect($('[data-k="menu"]').classList.contains("open")).toBe(true);
    click('[data-s="2"]');
    expect(sent.at(-1)).toEqual({ op: "run", release: "interval", intervalTicks: 480 });
    click('[data-k="follow"]');
    expect(sent.at(-1)).toEqual({ op: "follow", on: true });
    const running = diag({ run: { ...diag().run, active: true, release: "all", paused: true, follow: true } });
    ui.update(running);
    expect($('[data-k="run"]').textContent).toContain("Stop");
    expect($('[data-k="pause"]').getAttribute("aria-pressed")).toBe("true");
    click('[data-k="pause"]');
    click('[data-k="reset"]');
    click('[data-k="run"]');
    expect(sent.slice(-3)).toEqual([{ op: "pause" }, { op: "reset" }, { op: "stop" }]);
    expect(ui.root.classList.contains("running")).toBe(true);
  });

  it("Run waits for a hopper and says why", () => {
    ui.update(diag({ hoppers: 0, pieces: 0 }));
    const run = $<HTMLButtonElement>('[data-k="run"]');
    expect(run.disabled).toBe(true);
    expect(run.title).toMatch(/hopper/i);
    expect(press("Space")).toBe(true);
    expect(sent.some((a) => a.op === "run")).toBe(false);
  });

  it("the marble budget shows one pin per marble and sets the count", () => {
    ui.update(diag());
    expect($('[data-k="count"]').textContent).toBe("3 / 10");
    expect(document.querySelectorAll('[data-k="pins"] .pin:not(.off)').length).toBe(3);
    click('[data-n="7"]');
    expect(sent.at(-1)).toEqual({ op: "marbles", count: 7 });
  });

  it("the drawer switches families and a tile sets the brush and the Place tool", () => {
    ui.update(diag());
    expect($('[data-f="blocks"]').getAttribute("aria-selected")).toBe("true");
    expect(document.querySelectorAll("[data-piece]").length).toBe(3);
    expect($<HTMLImageElement>('[data-piece="blk_end_l3"] img').getAttribute("src")).toBe(
      "web/ui/thumbs/blk_end_l3.png",
    );
    click('[data-f="mechanisms"]');
    expect([...document.querySelectorAll<HTMLElement>("[data-piece]")].map((t) => t.dataset.piece)).toEqual([
      "mech_hopper",
      "mech_finish",
      "mech_splitter_l3",
    ]);
    click('[data-piece="mech_splitter_l3"]');
    expect(sent.slice(-2)).toEqual([
      { op: "brush", piece: "mech_splitter_l3", yaw: 0, mirror: false },
      { op: "tool", tool: "place" },
    ]);
    ui.update(diag({ tool: "place", brush: { piece: "mech_splitter_l3", yaw: 0, mirror: false } }));
    expect($('[data-piece="mech_splitter_l3"]').getAttribute("aria-selected")).toBe("true");
    click('[data-k="drawer"]');
    expect(ui.root.classList.contains("tray-closed")).toBe(true);
  });

  it("the first run opens on Mechanisms with the Hopper picked and shows Start here", () => {
    ui.update(diag({ pieces: 0, hoppers: 0, finishes: 0, edits: 0 }));
    expect(sent).toContainEqual({ op: "brush", piece: "mech_hopper", yaw: 0, mirror: false });
    expect($('[data-f="mechanisms"]').getAttribute("aria-selected")).toBe("true");
    expect($('[data-k="sel"]').textContent).toContain("Start here");
    expect(document.querySelectorAll(".steps li").length).toBe(3);
  });

  it("the panel shows one piece's ports and level, or a group's counts and actions", () => {
    ui.update(
      diag({
        selection: 1,
        selected: [[3, 2, 0]],
        selectionInfo: {
          count: 1,
          families: { mech: 1 },
          levels: [2, 2],
          single: {
            piece: "mech_splitter_l3",
            anchor: [3, 2, 0],
            yaw: 1,
            mirror: false,
            ports: [
              { face: "-z", flow: "in", hint: "linked" },
              { face: "+x", flow: "out", hint: "open" },
            ],
          },
        },
      }),
    );
    const sel = $('[data-k="sel"]').textContent ?? "";
    expect(sel).toContain("Splitter · 3");
    expect(sel).toContain("In N · Out E");
    expect(sel).toContain("L2");
    ui.update(
      diag({
        edits: 4,
        selection: 14,
        selected: [[0, 0, 0]],
        selectionInfo: { count: 14, families: { blk: 9, rail: 5 }, levels: [0, 2], single: null },
      }),
    );
    expect($('[data-k="sel"]').textContent).toContain("9 blocks · 5 rails");
    expect($('[data-k="sel"]').textContent).toContain("L0–L2");
    click('[data-k="gDelete"]');
    expect(sent.at(-1)).toEqual({ op: "remove" });
  });

  it(`deleting more than ${CONFIRM_DELETE} pieces asks once in the panel`, () => {
    const many = diag({
      selection: 60,
      selected: [[0, 0, 0]],
      selectionInfo: { count: 60, families: { blk: 60 }, levels: [0, 0], single: null },
    });
    ui.update(many);
    expect(press("Backspace")).toBe(true);
    expect(sent.some((a) => a.op === "remove")).toBe(false);
    ui.update(many);
    expect($('[data-k="sel"]').textContent).toContain("60 pieces?");
    click('[data-k="yesDelete"]');
    expect(sent.at(-1)).toEqual({ op: "remove" });
  });

  it("validation lists issues with Show, which flies the camera and selects the piece", () => {
    orbit = { target: [0, 0, 0], yaw: 0.2, pitch: 0.9, radius: 2.6 };
    ui.update(
      diag({
        issueCount: 2,
        issues: [
          {
            kind: "openPort",
            piece: "blk_curve90_l3",
            anchor: [12, 3, -4],
            cell: [12, 3, -4],
            face: "+x",
            fault: null,
            otherPiece: null,
          },
          { kind: "missingFinish", piece: null, anchor: null, cell: null, face: null, fault: null, otherPiece: null },
        ],
      }),
    );
    expect($('[data-k="vh"]').textContent).toBe("2 issues");
    expect(document.querySelectorAll(".iss").length).toBe(2);
    expect($(".iss").textContent).toContain("Open east port on Curve 90° · 3");
    click("[data-show]");
    clock = FLY_MS / 2;
    ui.update(diag());
    const mid = sent.at(-1);
    expect(mid?.op).toBe("camera");
    clock = FLY_MS;
    ui.update(diag());
    expect(sent.at(-3)?.op).toBe("camera");
    expect(sent.at(-2)).toEqual({ op: "boxSelect", a: [12, 3, -4], b: [12, 3, -4] });
    // Then the piece's selection mark pulses twice (engine, real time).
    expect(sent.at(-1)).toEqual({ op: "pulse", anchor: [12, 3, -4] });
    ui.update(diag({ edits: 9, issueCount: 0, issues: [] }));
    expect($('[data-k="vh"]').textContent).toBe("Track is complete");
  });

  it("the ghost tooltip trails the pointer with the cell, level and validity", () => {
    const canvas = $("#parble");
    canvas.dispatchEvent(new PointerEvent("pointermove", { clientX: 100, clientY: 200, bubbles: true }));
    ui.update(
      diag({
        tool: "place",
        hover: {
          source: "cursor",
          hit: "ground",
          hitCell: null,
          hitAnchor: null,
          target: [11, 2, -4],
          valid: true,
          error: null,
        },
      }),
    );
    const tip = $("#pe-tip");
    expect(tip.classList.contains("show")).toBe(true);
    expect(tip.textContent).toBe("[11, 2, −4] · L2 · valid");
    expect(tip.style.transform).toBe("translate(118px, 222px)");
  });

  it("a run fades the chrome after 1.2 s and an edge brings its chrome back", () => {
    const running = diag({ run: { ...diag().run, active: true, marbles: 3, tick: 480 } });
    ui.update(running);
    clock = 1_300;
    ui.update(running);
    expect(ui.root.classList.contains("faded")).toBe(true);
    expect($("#pe-cap").textContent).toContain("Running");
    expect($("#pe-cap").textContent).toContain("3 / 3");
    expect($("#pe-cap").textContent).toContain("0:02");
    window.dispatchEvent(new PointerEvent("pointermove", { clientX: 400, clientY: 10 }));
    ui.update(running);
    expect(ui.root.classList.contains("show-top")).toBe(true);
    ui.update(diag());
    expect(ui.root.classList.contains("faded")).toBe(false);
  });

  it("a building run shows the Building capsule with its progress until the run is in", () => {
    ui.update(diag({ run: { ...diag().run, building: true, buildProgress: 0.42, buildOp: "run" } }));
    expect(ui.root.classList.contains("building")).toBe(true);
    expect($("#pe-cap").textContent).toContain("Building run");
    expect($("#pe-cap").textContent).toContain("42%");
    ui.update(diag({ run: { ...diag().run, building: true, buildProgress: 0.9, buildOp: "run" } }));
    expect($("#pe-cap").textContent).toContain("90%");
    const running = diag({ run: { ...diag().run, active: true, marbles: 1, tick: 24 } });
    ui.update(running);
    expect(ui.root.classList.contains("building")).toBe(false);
    clock = 1_300;
    ui.update(running);
    expect($("#pe-cap").textContent).toContain("Running");
  });

  it("Esc stops a run, drops the brush, then clears the selection; Space pauses a run", () => {
    ui.update(diag({ run: { ...diag().run, active: true } }));
    press("Escape");
    expect(sent.at(-1)).toEqual({ op: "stop" });
    press("Space");
    expect(sent.at(-1)).toEqual({ op: "pause" });
    ui.update(diag({ tool: "place" }));
    press("Escape");
    expect(sent.at(-1)).toEqual({ op: "tool", tool: "select" });
    ui.update(diag({ tool: "select" }));
    press("Escape");
    expect(sent.at(-1)).toEqual({ op: "clearSelection" });
    press("Digit2");
    expect(sent.at(-1)).toEqual({ op: "tool", tool: "place" });
    press("Backslash");
    expect(ui.root.classList.contains("side-closed")).toBe(true);
  });

  it("renames the track from the strip", () => {
    ui.update(diag());
    click('[data-k="rename"]');
    const input = $<HTMLInputElement>(".track-in");
    input.value = "  Night Falls ";
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
    expect(sent.at(-1)).toEqual({ op: "rename", name: "Night Falls" });
  });

  it("the Library sheet exports the edited track and downloads it", () => {
    ui.update(diag());
    click('[data-k="library"]');
    expect($("#pe-sheet").classList.contains("open")).toBe(true);
    click('#pe-sheet [data-k="export"]');
    expect(sent.at(-1)).toEqual({ op: "export" });
    ui.update(diag(), [{ type: "trackExported", name: "Lantern Falls", json: "{}" }]);
    expect($('#pe-sheet [data-k="status"]').textContent).toBe("Exported lantern-falls.parble");
    expect(posted).toEqual([]);
  });

  it("opening a file with unsaved edits asks once", () => {
    ui.update(diag({ edits: 3 }));
    ui.update(diag({ edits: 5 }));
    click('[data-k="library"]');
    click('#pe-sheet [data-k="import"]');
    expect($('#pe-sheet [data-k="ask"]').classList.contains("open")).toBe(true);
    click('#pe-sheet [data-k="no"]');
    expect($('#pe-sheet [data-k="ask"]').classList.contains("open")).toBe(false);
  });

  it("drag-to-lay: a tile press arms a stroke that opens on the scene, release ends it, Esc cancels", () => {
    ui.update(diag({ tool: "place" }));
    sent = [];
    const tile = $('[data-piece="blk_straight_l3"]');
    const canvas = $("#parble");
    tile.dispatchEvent(new PointerEvent("pointerdown", { button: 0, bubbles: true }));
    expect(sent).toEqual([
      { op: "brush", piece: "blk_straight_l3", yaw: 0, mirror: false },
      { op: "tool", tool: "place" },
    ]);
    expect($<HTMLImageElement>(".tile img").getAttribute("draggable")).toBe("false");
    // Moving over the drawer lays nothing; the stroke opens when the pointer reaches the scene.
    tile.dispatchEvent(new PointerEvent("pointermove", { clientX: 300, clientY: 700, bubbles: true }));
    expect(sent.length).toBe(2);
    canvas.dispatchEvent(new PointerEvent("pointermove", { clientX: 300, clientY: 300, bubbles: true }));
    expect(sent.at(-1)).toEqual({ op: "stroke", phase: "start" });
    canvas.dispatchEvent(new PointerEvent("pointermove", { clientX: 310, clientY: 300, bubbles: true }));
    expect(sent.filter((a) => a.op === "stroke").length).toBe(1);
    // While it lays, the tooltip counts the pieces.
    ui.update(diag({ tool: "place", stroke: { placed: 4, level: 0, outlines: [] } }));
    expect($("#pe-tip").textContent).toBe("Laying a run · 4 pieces · release to finish");
    window.dispatchEvent(new PointerEvent("pointerup", { bubbles: true }));
    expect(sent.at(-1)).toEqual({ op: "stroke", phase: "end" });
    // A second release ends nothing more.
    let n = sent.length;
    window.dispatchEvent(new PointerEvent("pointerup", { bubbles: true }));
    expect(sent.length).toBe(n);
    // Released before reaching the scene: no stroke at all.
    tile.dispatchEvent(new PointerEvent("pointerdown", { button: 0, bubbles: true }));
    window.dispatchEvent(new PointerEvent("pointerup", { bubbles: true }));
    expect(sent.some((a, i) => i >= n && a.op === "stroke")).toBe(false);
    // Esc mid-stroke cancels it, and the release after sends nothing.
    tile.dispatchEvent(new PointerEvent("pointerdown", { button: 0, bubbles: true }));
    canvas.dispatchEvent(new PointerEvent("pointermove", { clientX: 300, clientY: 300, bubbles: true }));
    expect(press("Escape")).toBe(true);
    expect(sent.at(-1)).toEqual({ op: "stroke", phase: "cancel" });
    window.dispatchEvent(new PointerEvent("pointerup", { bubbles: true }));
    expect(sent.at(-1)).toEqual({ op: "stroke", phase: "cancel" });
    // A right press arms nothing.
    n = sent.length;
    tile.dispatchEvent(new PointerEvent("pointerdown", { button: 2, bubbles: true }));
    canvas.dispatchEvent(new PointerEvent("pointermove", { clientX: 300, clientY: 300, bubbles: true }));
    expect(sent.length).toBe(n);
  });

  it("the dashed ghost outlines the brush footprint, red when invalid, with the run's boxes", () => {
    const box: [number, number][] = [
      [100, 100],
      [140, 100],
      [100, 80],
      [140, 80],
      [110, 110],
      [150, 110],
      [110, 90],
      [150, 90],
    ];
    const hover = {
      source: "cursor",
      hit: "ground",
      hitCell: null,
      hitAnchor: null,
      target: [1, 0, 1] as [number, number, number],
      valid: true,
      error: null,
      outline: box,
    };
    ui.update(diag({ tool: "place", hover }));
    const path = (): SVGPathElement => document.querySelectorAll<SVGPathElement>("#pe-ghost path")[1] as SVGPathElement;
    // The pointer is not on the scene yet: nothing drawn.
    expect(path().getAttribute("d") ?? "").toBe("");
    $("#parble").dispatchEvent(new PointerEvent("pointermove", { clientX: 120, clientY: 95, bubbles: true }));
    ui.update(diag({ tool: "place", hover }));
    expect(path().getAttribute("d")?.match(/M/g)?.length).toBe(12);
    expect(path().getAttribute("class")).toBe("ok");
    ui.update(diag({ tool: "place", hover: { ...hover, valid: false, error: "occupied" } }));
    expect(path().getAttribute("class")).toBe("bad");
    // During a stroke the single box gives way to the laid run's boxes.
    ui.update(diag({ tool: "place", hover, stroke: { placed: 2, level: 0, outlines: [box, box] } }));
    expect(path().getAttribute("d")).toBe("");
    expect($("#pe-ghost .run").getAttribute("d")?.match(/M/g)?.length).toBe(24);
    // Not in the select tool, and not during a run.
    ui.update(diag({ tool: "select", hover }));
    expect(path().getAttribute("d")).toBe("");
    ui.update(diag({ tool: "place", hover, run: { ...diag().run, active: true } }));
    expect(path().getAttribute("d")).toBe("");
  });

  it("drawer tiles show how many of each piece are placed", () => {
    ui.update(diag({ pieceCounts: { blk_straight_l3: 2, blk_end_l3: 1 } }));
    click('[data-f="blocks"]');
    ui.update(diag({ pieceCounts: { blk_straight_l3: 2, blk_end_l3: 1 } }));
    expect($('[data-ct="blk_straight_l3"]').textContent).toBe("×2");
    expect($('[data-ct="blk_end_l3"]').textContent).toBe("×1");
    expect($('[data-ct="blk_curve90_l3"]').textContent).toBe("");
    ui.update(diag({ edits: 4, pieceCounts: { blk_straight_l3: 3 } }));
    expect($('[data-ct="blk_straight_l3"]').textContent).toBe("×3");
    expect($('[data-ct="blk_end_l3"]').textContent).toBe("");
  });

  it("a toggle's first side is a piece option in the panel", () => {
    const single = {
      piece: "mech_toggle_l3",
      anchor: [2, 0, 1] as [number, number, number],
      yaw: 1,
      mirror: false,
      ports: [],
      config: {},
      options: [
        {
          key: "first",
          value: 0,
          choices: [
            { value: 0, face: "+x" as const },
            { value: 1, face: "-x" as const },
          ],
        },
      ],
    };
    ui.update(
      diag({
        selection: 1,
        selected: [[2, 0, 1]],
        selectionInfo: { count: 1, families: { mech: 1 }, levels: [0, 0], single },
      }),
    );
    const opts = document.querySelectorAll<HTMLButtonElement>("[data-opt]");
    expect([...opts].map((b) => b.textContent)).toEqual(["East", "West"]);
    expect(opts[0]?.getAttribute("aria-checked")).toBe("true");
    expect(opts[0]?.querySelector(".pin")).not.toBeNull();
    expect($('[data-k="sel"]').textContent).toContain("First side");
    expect($('[data-k="sel"]').textContent).toContain("next Run or Reset");
    opts[1]?.click();
    expect(sent.at(-1)).toEqual({ op: "mechConfig", key: "first", value: 1 });
  });

  it("a splitter's mode is Random or Alternate, and Alternate adds its first side", () => {
    const mode = (value: number) => ({
      key: "mode",
      value,
      choices: [
        { value: 0, label: "Random" },
        { value: 1, label: "Alternate" },
      ],
    });
    const single = (options: PieceOption[]) => ({
      piece: "mech_splitter_l3",
      anchor: [7, 0, -6] as [number, number, number],
      yaw: 1,
      mirror: false,
      ports: [],
      config: {},
      options,
    });
    const show = (edits: number, options: PieceOption[]) =>
      ui.update(
        diag({
          edits,
          selection: 1,
          selected: [[7, 0, -6]],
          selectionInfo: { count: 1, families: { mech: 1 }, levels: [0, 0], single: single(options) },
        }),
      );
    show(1, [mode(0)]);
    let groups = document.querySelectorAll<HTMLElement>(".seg.opt");
    expect([...groups].map((g) => g.getAttribute("aria-label"))).toEqual(["Mode"]);
    let opts = document.querySelectorAll<HTMLButtonElement>("[data-opt]");
    expect([...opts].map((b) => b.textContent)).toEqual(["Random", "Alternate"]);
    expect(opts[0]?.getAttribute("aria-checked")).toBe("true");
    expect(opts[0]?.querySelector(".pin")).not.toBeNull();
    expect($('[data-k="sel"]').textContent).not.toContain("First side");
    opts[1]?.click();
    expect(sent.at(-1)).toEqual({ op: "mechConfig", key: "mode", value: 1 });
    // The engine answers with Alternate set and the first side offered.
    const first = {
      key: "first",
      value: 0,
      choices: [
        { value: 0, face: "+x" as const },
        { value: 1, face: "-x" as const },
      ],
    };
    show(2, [mode(1), first]);
    groups = document.querySelectorAll<HTMLElement>(".seg.opt");
    expect([...groups].map((g) => g.getAttribute("aria-label"))).toEqual(["Mode", "First side"]);
    opts = document.querySelectorAll<HTMLButtonElement>("[data-opt]");
    expect([...opts].map((b) => b.textContent)).toEqual(["Random", "Alternate", "East", "West"]);
    expect(opts[1]?.getAttribute("aria-checked")).toBe("true");
    expect(opts[2]?.getAttribute("aria-checked")).toBe("true");
    expect($('[data-k="sel"]').textContent).toContain("next Run or Reset");
    opts[3]?.click();
    expect(sent.at(-1)).toEqual({ op: "mechConfig", key: "first", value: 1 });
  });

  it("P, C and F work from the overlay when the canvas has no focus, and yield to the engine otherwise", () => {
    ui.update(diag({ run: { ...diag().run, active: true } }));
    sent = [];
    expect(press("KeyP")).toBe(true);
    expect(sent.at(-1)).toEqual({ op: "pause" });
    expect(press("KeyC")).toBe(true);
    expect(sent.at(-1)).toEqual({ op: "follow", on: true });
    const canvas = $("#parble");
    const heard: string[] = [];
    canvas.addEventListener("keydown", (e) => heard.push((e as KeyboardEvent).code));
    expect(press("KeyF")).toBe(true);
    expect(heard).toEqual(["KeyF"]);
    // The canvas has focus: Bevy hears its own keys, the overlay leaves them.
    canvas.tabIndex = 0;
    canvas.focus();
    const n = sent.length;
    expect(press("KeyP")).toBe(false);
    expect(sent.length).toBe(n);
    // Synthetic keys on the canvas (pointer-lock's Escape) are the engine's too.
    let answered: boolean | null = null;
    canvas.addEventListener("keydown", (e) => {
      answered = ui.key(e as KeyboardEvent);
    });
    canvas.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true }));
    expect(answered).toBe(false);
    canvas.blur();
    // In fly every key is the engine's (Space is up, Esc leaves fly).
    mode = "fly";
    expect(press("Space")).toBe(false);
    expect(press("Escape")).toBe(false);
    expect(sent.length).toBe(n);
  });

  it("below 1280 px the run controls show their label and key cap in an inlay tooltip", () => {
    const mm = window.matchMedia;
    window.matchMedia = ((q: string) =>
      ({ matches: q.includes("1279") }) as MediaQueryList) as typeof window.matchMedia;
    try {
      ui.update(diag());
      const pause = $('[data-k="pause"]');
      expect(pause.getAttribute("aria-label")).toBe("Pause");
      pause.dispatchEvent(new PointerEvent("pointerenter"));
      const tt = $("#pe-tt");
      expect(tt.classList.contains("show")).toBe(true);
      expect(tt.textContent).toBe("PauseSpace");
      expect(tt.querySelector("kbd")?.textContent).toBe("Space");
      pause.dispatchEvent(new PointerEvent("pointerleave"));
      expect(tt.classList.contains("show")).toBe(false);
      $('[data-k="one"]').dispatchEvent(new FocusEvent("focus"));
      expect(tt.textContent).toBe("Release one");
    } finally {
      window.matchMedia = mm;
    }
    // Wide: no tooltip (the label is on the button).
    window.matchMedia = (() => ({ matches: false }) as MediaQueryList) as typeof window.matchMedia;
    try {
      $('[data-k="one"]').dispatchEvent(new FocusEvent("blur"));
      $('[data-k="one"]').dispatchEvent(new FocusEvent("focus"));
      expect($("#pe-tt").classList.contains("show")).toBe(false);
    } finally {
      window.matchMedia = mm;
    }
  });
});
