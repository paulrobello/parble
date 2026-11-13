// Typed view of `window.__parble.editor` (EditorDiag, crates/parble-app/src/editor.rs) and the
// `parbleEdit` actions the overlay sends, kept by hand like bridge.ts. Pure helpers only: names,
// families, issue sentences and labels, so Vitest can check every string the panel shows.

import type { NudgeMode, NudgeZone } from "../parble";

export type Vec3i = [number, number, number];
export type Tool = "place" | "select" | "delete";
export type Release = "one" | "all" | "interval" | "sequential";
export type Face = "+x" | "-x" | "+z" | "-z" | "top" | "bottom";
export type Hint = "linked" | "open" | "fault";

export interface HoverDiag {
  source: string;
  hit: string | null;
  hitCell: Vec3i | null;
  hitAnchor: Vec3i | null;
  target: Vec3i | null;
  valid: boolean;
  error: string | null;
  /** The brush footprint box at the target, 8 projected corners (CSS px, corner i = x | y<<1 | z<<2). */
  outline?: [number, number][] | null;
}

export interface IssueDiag {
  kind: string;
  piece: string | null;
  anchor: Vec3i | null;
  cell: Vec3i | null;
  face: Face | null;
  fault: string | null;
  otherPiece: string | null;
}

export interface SelectionDiag {
  count: number;
  families: Record<string, number>;
  levels: [number, number];
  single: {
    piece: string;
    anchor: Vec3i;
    yaw: number;
    mirror: boolean;
    ports: { face: Face; flow: "in" | "out" | "both"; hint: Hint }[];
    config?: Record<string, number>;
    options?: PieceOption[];
  } | null;
}

/**
 * A piece option the inspector offers (a toggle's or alternating splitter's first side, a splitter's
 * mode): a side choice carries the face it sends to, a named choice its label.
 */
export interface PieceOption {
  key: string;
  value: number;
  choices: { value: number; face?: Face; label?: string }[];
}

/** A drag-to-lay stroke in progress: pieces laid so far and the level it lays on. */
export interface StrokeDiag {
  placed: number;
  level: number | null;
  /** Outlines of the cells this stroke laid, as `HoverDiag.outline` (at most 64). */
  outlines?: [number, number][][];
}

export interface RunDiag {
  active: boolean;
  release: Release;
  intervalTicks: number;
  queue: number;
  marbles: number;
  tick: number;
  paused: boolean;
  follow: boolean;
  /** A run, stop or reset is building its sim over frames; the live sim holds still. */
  building?: boolean;
  /** The build's share done, 0 to 1. */
  buildProgress?: number;
  /** The op building: run, stop or reset. */
  buildOp?: string | null;
}

/** The release a marble check ran (CheckRelease, crates/parble-app/src/editor/check.rs). */
export interface CheckRelease {
  release: Release;
  intervalTicks: number;
  marbles: number;
  seed: number;
  sizes: string[];
  allSizes: boolean;
}

/** One marble spec at one size (`size` null: the queue's own sizes). */
export interface CheckRun {
  marble: string;
  also: string[];
  size: string | null;
  marbles: number;
  finished: number;
  fellOff: number;
  stalled: number;
  ticks: number;
}

/** Where marbles of one type and size fell off or stalled, anchored like an issue. */
export interface CheckWarning {
  kind: "fellOff" | "stalled";
  marble: string;
  also: string[];
  size: string;
  piece: string | null;
  anchor: Vec3i | null;
  cell: Vec3i | null;
  marbles: number[];
  /** Of a stall's marbles, how many were still rolling when the check ended. */
  rolling: number;
  tick: number;
}

/** `editor.check`: the marble check, warnings parallel to `issues`. */
export interface CheckDiag {
  state: "none" | "running" | "done" | "stale" | "failed";
  progress: number;
  current: string | null;
  release: CheckRelease | null;
  edits: number;
  runs: CheckRun[];
  warningCount: number;
  warnings: CheckWarning[];
  error: string | null;
  timing?: { frames: number; workMs: number; frameMsMax: number; unitMsMax: number; wallMs: number } | null;
}

export interface EditorDiag {
  tool: Tool;
  brush: { piece: string; yaw: number; mirror: boolean };
  palette: string[];
  pieces: number;
  visiblePieces: number;
  slice: number | null;
  /** The level Place lands on: the slice, or the last slice while all levels show. */
  buildLevel: number;
  selection: number;
  selected: Vec3i[];
  canUndo: boolean;
  canRedo: boolean;
  hover: HoverDiag | null;
  drag: { offset: Vec3i; valid: boolean } | null;
  edits: number;
  seq: number;
  last: { id: number | null; op: string; ok: boolean; error: string | null };
  keys: boolean;
  name: string;
  run: RunDiag;
  hoppers: number;
  finishes: number;
  maxLevel: number;
  issueCount: number;
  issues: IssueDiag[];
  selectionInfo: SelectionDiag | null;
  /** Placed pieces by id. */
  pieceCounts?: Record<string, number>;
  stroke?: StrokeDiag | null;
  /** The last run, stop or reset: the engine's build work (summed over its frames) and install. */
  lastRun?: {
    op: string;
    buildMs: number;
    pieces: number;
    frames?: number;
    frameMsMax?: number;
    wallMs?: number;
  } | null;
  /** The marble check; absent from builds before it. */
  check?: CheckDiag;
  /** Nudge zones; absent from builds before them. */
  nudges?: NudgeDiag;
  /** Ground columns under the hovered piece and the selection (cell columns drawn); absent from builds before them. */
  columns?: { on: boolean; hover: number; selected: number };
  /** What attaches to the one selected piece; null unless exactly one is selected, absent from builds before it. */
  attach?: AttachFit[] | null;
}

/** A piece that attaches to the selected piece, and the first placement (anchor, turn, mirror) that links. */
export interface AttachFit {
  piece: string;
  anchor: Vec3i;
  yaw: number;
  mirror: boolean;
}

/** One nudge zone as `editor.nudges` lists it: its track fields in camelCase and where it is on screen. */
export interface NudgeZoneDiag {
  index: number;
  name: string;
  /** "top_up" or "constant"; absent from builds before constant mode (read as top-up). */
  mode?: NudgeMode;
  minUm: Vec3i;
  maxUm: Vec3i;
  yawMdeg: number;
  pitchMdeg: number;
  minSpeedUmS: number;
  maxAccelUmS2: number;
  /** The cell holding the box's centre. */
  cell: Vec3i;
  /** The box's 8 projected corners (CSS px), or null off screen. */
  outline: [number, number][] | null;
  /** The direction arrow's projected tail and head, or null off screen. */
  arrow: [[number, number], [number, number]] | null;
}

export interface NudgeDiag {
  count: number;
  selected: number | null;
  zones: NudgeZoneDiag[];
}

export type Action =
  | { op: "tool"; tool: Tool }
  | { op: "brush"; piece: string; yaw?: number; mirror?: boolean }
  | { op: "rotate"; by: number }
  | { op: "mirror" }
  | { op: "remove" }
  | { op: "clearSelection" }
  | { op: "undo" }
  | { op: "redo" }
  | { op: "slice"; level: number | null }
  | { op: "buildLevel"; level: number }
  | { op: "sliceStep"; by: number }
  | { op: "buildStep"; by: number }
  | { op: "move"; offset: Vec3i }
  | { op: "boxSelect"; a: Vec3i; b: Vec3i; shift?: boolean }
  | { op: "camera"; target: [number, number, number]; yaw: number; pitch: number; radius: number }
  | { op: "keys"; enabled: boolean }
  | { op: "columns"; on: boolean }
  | { op: "marbles"; count: number }
  | { op: "run"; release: Release; intervalTicks?: number }
  | { op: "stop" }
  | { op: "reset" }
  | { op: "pause"; paused?: boolean }
  | { op: "follow"; on: boolean }
  | { op: "rename"; name: string }
  | { op: "export" }
  | { op: "stroke"; phase: "start" | "end" | "cancel" }
  | { op: "pulse"; anchor: Vec3i }
  | { op: "mechConfig"; key: string; value: number }
  | { op: "check"; release: Release; intervalTicks?: number; allSizes?: boolean }
  | { op: "nudgeAdd"; cells?: [Vec3i, Vec3i]; yawMdeg?: number }
  | { op: "nudgeSelect"; index: number | null }
  | { op: "nudgeSet"; index?: number; zone: NudgeZone }
  | { op: "nudgeRemove"; index?: number };

export type Send = (a: Action) => void;

/** Sim ticks per second (crates/parble-sim `TICK_HZ`). */
export const TICK_HZ = 240;
/** Kit cell edge, metres (crates/parble-render/src/kit `CELL_M`). */
export const CELL_M = 0.06;
export const MAX_MARBLES = 10;
/** Interval release choices, seconds. */
export const INTERVALS = [0.5, 1, 2] as const;

export type Family = "blocks" | "rails" | "mechanisms" | "supports";
export const FAMILIES: readonly { id: Family; label: string }[] = [
  { id: "blocks", label: "Blocks" },
  { id: "rails", label: "Rails" },
  { id: "mechanisms", label: "Mechanisms" },
  { id: "supports", label: "Supports" },
];

const SUPPORTS = new Set(["blk_pillar", "blk_tower_socket_l1"]);

export function familyOf(piece: string): Family {
  if (SUPPORTS.has(piece)) return "supports";
  if (piece.startsWith("rail_")) return "rails";
  if (piece.startsWith("mech_")) return "mechanisms";
  return "blocks";
}

const NAMES: Record<string, string> = {
  catch: "Catch",
  curve90: "Curve 90°",
  drop: "Drop",
  end: "End stop",
  funnel: "Funnel 3×3",
  join: "Join",
  pillar: "Pillar",
  redirect: "Redirect",
  slope: "Slope",
  straight: "Straight",
  tower: "Tower socket",
  arch: "Rope arch",
  checkpoint: "Checkpoint",
  finish: "Finish cups",
  gate: "One-way gate",
  hole: "One-way hole",
  hopper: "Hopper",
  splitter: "Splitter",
  stepper: "Stepper lift",
  toggle: "Toggle",
  joint: "Rail joint",
  s: "S-bend",
};

/** A player-facing name: `blk_slope_steep_l3` → "Steep slope · 3", `rail_straight_l2` → "Straight · 2". */
export function pieceName(piece: string): string {
  const parts = piece.split("_");
  const shape = parts[1] ?? piece;
  let name = NAMES[shape] ?? shape;
  if (shape === "slope") name = parts[2] === "steep" ? "Steep slope" : "Gentle slope";
  if (shape === "stepper" && parts[2] === "up1") name = "Stepper lift";
  const level = /_l(\d)$/.exec(piece)?.[1];
  return level ? `${name} · ${level}` : name;
}

/** The palette's pieces in `family`, in palette order (finish and hopper first among mechanisms). */
export function piecesOf(palette: readonly string[], family: Family): string[] {
  const out = palette.filter((p) => familyOf(p) === family);
  if (family !== "mechanisms") return out;
  const first = ["mech_hopper", "mech_finish"].filter((p) => out.includes(p));
  return [...first, ...out.filter((p) => !first.includes(p))];
}

/** The piece id without its level suffix: `rail_s_1x3_l2` → `rail_s_1x3`. */
export const levelBase = (piece: string): string => piece.replace(/_l\d$/, "");

/** Drawer tiles for `pieces`: each rail shape that comes at several levels is one tile listing its
 * variants in level order (one height picker), every other piece a tile of its own. */
export function drawerTiles(pieces: readonly string[]): string[][] {
  const out: string[][] = [];
  const at = new Map<string, string[]>();
  for (const p of pieces) {
    const base = familyOf(p) === "rails" ? levelBase(p) : p;
    const group = at.get(base);
    if (group) group.push(p);
    else {
      const g = [p];
      at.set(base, g);
      out.push(g);
    }
  }
  return out.map((g) => [...g].sort());
}

const FACE: Record<Face, string> = {
  "+x": "east",
  "-x": "west",
  "+z": "south",
  "-z": "north",
  top: "top",
  bottom: "bottom",
};
const FACE_SHORT: Record<Face, string> = { "+x": "E", "-x": "W", "+z": "S", "-z": "N", top: "Top", bottom: "Bottom" };

export const cellText = (c: Vec3i): string => `[${c.map((v) => String(v).replace("-", "−")).join(", ")}]`;

const FAULTS: Record<string, string> = {
  noMatingPort: "nothing to connect to",
  levelMismatch: "the heights do not match",
  classMismatch: "a rail meets a block channel",
  flowConflict: "both sides flow the same way",
};

/** The plain sentence for one validation issue. */
export function issueText(i: IssueDiag): string {
  const name = i.piece ? pieceName(i.piece) : "piece";
  const other = i.otherPiece ? pieceName(i.otherPiece) : "the next piece";
  switch (i.kind) {
    case "openPort":
      return `Open ${i.face ? `${FACE[i.face]} ` : ""}port on ${name}`;
    case "brokenLink":
      return `Broken link: ${name} into ${other}${i.fault && FAULTS[i.fault] ? `, ${FAULTS[i.fault]}` : ""}`;
    case "unreachable":
      return `Unreachable: no marble gets to this ${name}`;
    case "finishUnreachable":
      return "No marble reaches these finish cups";
    case "missingHopper":
      return "Add a hopper so the marbles have a start";
    case "missingFinish":
      return "Add finish cups to end the run";
    case "overlap":
      return `${name} overlaps ${other}`;
    case "unknownPiece":
      return `Unknown piece ${i.piece ?? ""}`.trim();
    case "levelSuffixMismatch":
      return `${name} sits at the wrong height for its ports`;
    case "hopperConfigInvalid":
      return "The hopper setting points at a missing hopper";
    default:
      return i.kind;
  }
}

const MARBLES: Record<string, string> = {
  "cats-eye": "Cat's eye",
  glazed: "Glazed",
  steel: "Steel",
  swirl: "Swirl",
  onionskin: "Onionskin",
  opal: "Opal",
  glow: "Glow",
};

/** A marble type's name; a type standing for others of its physics is their family. */
export function marbleName(marble: string, also: readonly string[] = []): string {
  const name = MARBLES[marble] ?? marble;
  return also.length ? `${name} family` : name;
}

const plural = (n: number, one: string): string => `${n} ${one}${n === 1 ? "" : "s"}`;

/** The plain sentence for one marble-check warning. */
export function checkText(w: CheckWarning): string {
  const who = `${marbleName(w.marble, w.also)} ${w.size}`;
  const n = w.marbles.length;
  const piece = w.piece ? pieceName(w.piece) : "the track";
  if (w.kind === "fellOff") return `${who}: ${plural(n, "marble")} ${n === 1 ? "falls" : "fall"} off at ${piece}`;
  const still = n - w.rolling;
  const parts: string[] = [];
  if (still) parts.push(`${plural(still, "marble")} ${still === 1 ? "stops" : "stop"}`);
  if (w.rolling) parts.push(`${plural(w.rolling, "marble")} still rolling at the time limit`);
  return `${who}: ${parts.join(", ")} on ${piece}`;
}

const RELEASES: Record<Release, string> = {
  one: "Release one",
  all: "Release all",
  interval: "Interval",
  sequential: "Sequential",
};

/** The release a check ran: "Release all · 10 standard marbles · seed 2". */
export function checkReleaseText(r: CheckRelease): string {
  const mode = r.release === "interval" ? `Interval ${r.intervalTicks / TICK_HZ} s` : RELEASES[r.release];
  const marbles = `marble${r.marbles === 1 ? "" : "s"}`;
  const queue = r.allSizes
    ? `${r.marbles} ${marbles} at every size`
    : `${r.marbles} ${r.sizes.join(" and ")} ${marbles}`;
  return `${mode} · ${queue} · seed ${r.seed}`;
}

/** An empty board starts building here, so the hopper has levels to fall through below it
 *  (the showcase's hopper stands at L7). */
export const START_LEVEL = 6;

/** The shown levels: "L0–L4" for all levels, "≤ L2" when sliced, "L6" above every piece. */
export function levelLabel(slice: number | null, maxLevel: number): string {
  if (slice === null) return `L0–L${Math.max(maxLevel, 0)}`;
  return slice > maxLevel ? `L${slice}` : `≤ L${slice}`;
}

/** One level less than all or the slice, never below 0: where X slices to the first time. `<` and `>`
 *  send `sliceStep` instead, so the engine steps from its own state and quick presses add up. */
export const sliceDown = (slice: number | null, maxLevel: number): number => Math.max((slice ?? maxLevel + 1) - 1, 0);

/** Ports as "In N · Out E, W". */
export function portsText(ports: { face: Face; flow: string }[]): string {
  const of = (flow: string): string[] => ports.filter((p) => p.flow === flow).map((p) => FACE_SHORT[p.face]);
  const parts: string[] = [];
  const ins = of("in");
  const outs = of("out");
  const both = of("both");
  if (ins.length) parts.push(`In ${ins.join(", ")}`);
  if (outs.length) parts.push(`Out ${outs.join(", ")}`);
  if (both.length) parts.push(`Either ${both.join(", ")}`);
  return parts.join(" · ") || "None";
}

/** The trailing ghost tooltip: "[11, 3, −4] · L2 · valid", or the reason it is not. */
export function ghostText(h: HoverDiag | null): { text: string; valid: boolean } | null {
  if (!h?.target) return null;
  const base = `${cellText(h.target)} · L${h.target[1]}`;
  return h.valid
    ? { text: `${base} · valid`, valid: true }
    : { text: `${base} · ${shortError(h.error)}`, valid: false };
}

function shortError(e: string | null): string {
  if (!e) return "blocked";
  if (/overlap|occupied|taken/i.test(e)) return "cell taken";
  if (/port|link/i.test(e)) return "ports will not meet";
  return e.length > 48 ? `${e.slice(0, 47)}…` : e;
}

/** Group counts as "6 blocks · 5 rails · 3 mechanisms". */
export function familyText(f: Record<string, number>): string {
  const label: Record<string, [string, string]> = {
    blk: ["block", "blocks"],
    rail: ["rail", "rails"],
    mech: ["mechanism", "mechanisms"],
    support: ["support", "supports"],
  };
  return Object.entries(f)
    .filter(([, n]) => n > 0)
    .map(([k, n]) => `${n} ${(label[k] ?? [k, k])[n === 1 ? 0 : 1]}`)
    .join(" · ");
}

/** Run time from sim ticks, "0:14". */
export function runTime(tick: number): string {
  const s = Math.floor(tick / TICK_HZ);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** The orbit that looks at `cell` (its centre) from the current yaw, a little closer. */
export function focusOrbit(
  cell: Vec3i,
  from: { yaw: number; pitch: number; radius: number },
): { target: [number, number, number]; yaw: number; pitch: number; radius: number } {
  return {
    target: [cell[0] * CELL_M, (cell[1] + 0.5) * CELL_M, cell[2] * CELL_M],
    yaw: from.yaw,
    pitch: Math.min(Math.max(from.pitch, 0.5), 1.2),
    radius: Math.min(Math.max(from.radius * 0.7, 0.6), 2.0),
  };
}

/** The view's compass heading: degrees clockwise from north (−z) towards east (+x); null looking straight up or down. */
export function viewBearingDeg(forward: readonly [number, number, number] | null | undefined): number | null {
  if (!forward) return null;
  const [x, , z] = forward;
  if (Math.hypot(x, z) < 1e-4) return null;
  return ((Math.atan2(x, -z) * 180) / Math.PI + 360) % 360;
}

/** Where world bearing `worldDeg` sits on a dial whose top is the view's heading: a unit vector, y down. */
export function dialPoint(worldDeg: number, viewDeg: number): [number, number] {
  const a = ((worldDeg - viewDeg) * Math.PI) / 180;
  return [Math.sin(a), -Math.cos(a)];
}

/** Compass word for a side face, as the ports line uses (`+x` east, `-z` north). */
export const faceWord = (f: Face): string => FACE[f].replace(/^./, (c) => c.toUpperCase());

/** The drawer tile badge: "×3" for a placed count, "" for none. */
export const countBadge = (n: number | undefined): string => (n && n > 0 ? `×${n}` : "");

const EDGES: readonly [number, number][] = [
  [0, 1],
  [2, 3],
  [4, 5],
  [6, 7],
  [0, 2],
  [1, 3],
  [4, 6],
  [5, 7],
  [0, 4],
  [1, 5],
  [2, 6],
  [3, 7],
];

/** SVG path data for the 12 edges of a projected box (8 corners, corner i = x | y<<1 | z<<2). */
export function boxPath(corners: readonly (readonly [number, number])[] | null | undefined): string {
  if (corners?.length !== 8) return "";
  const r = (v: number): string => (Math.round(v * 10) / 10).toString();
  return EDGES.map(([a, b]) => {
    const p = corners[a] as readonly [number, number];
    const q = corners[b] as readonly [number, number];
    return `M${r(p[0])} ${r(p[1])}L${r(q[0])} ${r(q[1])}`;
  }).join("");
}

/** The trailing tooltip during a drag-to-lay stroke. */
export function strokeText(placed: number): string {
  return `Laying a run · ${placed} piece${placed === 1 ? "" : "s"} · release to finish`;
}

const PLAN_FILL: Record<Family, string> = {
  blocks: "#c49a52",
  rails: "#d9c2a0",
  mechanisms: "#a8c8d8",
  supports: "#7d5f2c",
};

/**
 * A plan-view thumbnail of a saved track for the Library: one square per placement anchor seen
 * from above (higher levels drawn last), coloured by family, fitted into `w`×`h`. Empty when the
 * track has no placements.
 */
export function planThumb(
  placements: readonly { piece: string; anchor: [number, number, number] }[],
  w = 64,
  h = 48,
): string {
  if (placements.length === 0) return "";
  let [x0, x1, z0, z1] = [Infinity, -Infinity, Infinity, -Infinity];
  for (const p of placements) {
    x0 = Math.min(x0, p.anchor[0]);
    x1 = Math.max(x1, p.anchor[0]);
    z0 = Math.min(z0, p.anchor[2]);
    z1 = Math.max(z1, p.anchor[2]);
  }
  const pad = 4;
  const cols = x1 - x0 + 1;
  const rows = z1 - z0 + 1;
  const s = Math.min((w - 2 * pad) / cols, (h - 2 * pad) / rows, 8);
  const ox = (w - cols * s) / 2;
  const oy = (h - rows * s) / 2;
  const r = (v: number): string => (Math.round(v * 100) / 100).toString();
  const cells = [...placements]
    .sort((a, b) => a.anchor[1] - b.anchor[1])
    .map((p) => {
      const x = ox + (p.anchor[0] - x0) * s;
      const y = oy + (p.anchor[2] - z0) * s;
      return `<rect x="${r(x)}" y="${r(y)}" width="${r(Math.max(s - 0.6, 0.8))}" height="${r(Math.max(s - 0.6, 0.8))}" fill="${PLAN_FILL[familyOf(p.piece)]}"/>`;
    })
    .join("");
  return `<svg viewBox="0 0 ${w} ${h}" width="${w}" height="${h}" aria-hidden="true">${cells}</svg>`;
}

// ---- nudge zones (crates/parble-core/src/nudge_zone.rs) ---------------------------------------

/** Micrometres per cell. */
export const CELL_UM = 60_000;
/** Steps of the zone inspector's buttons. */
export const NUDGE_YAW_STEP = 45_000;
export const NUDGE_PITCH_STEP = 15_000;
export const NUDGE_SPEED_STEP = 50_000;
export const NUDGE_ACCEL_STEP = 250_000;
/** The file's limits: speed and acceleration at most 2 m/s and 2 m/s², pitch within ±90°. */
export const NUDGE_MAX = 2_000_000;
export const NUDGE_PITCH_MAX = 90_000;

/** One inspector edit: grow or shrink an axis by a cell (on its high side), or step an aim or rate. */
export type NudgeEdit =
  | { kind: "size"; axis: 0 | 1 | 2; by: 1 | -1 }
  | { kind: "yaw" | "pitch" | "speed" | "accel"; by: 1 | -1 }
  | { kind: "mode"; mode: NudgeMode };

/** A listed zone's mode, top-up when the build does not report one. */
export const nudgeMode = (z: Pick<NudgeZoneDiag, "mode">): NudgeMode => z.mode ?? "top_up";

/** The zone inspector's Mode choices, in order. */
export const NUDGE_MODE_LABELS: readonly [NudgeMode, string][] = [
  ["top_up", "Top up"],
  ["constant", "Constant"],
];

/** A listed zone back in its `.parble` shape (snake_case; the empty name, zero pitch and top-up
 * mode omitted). */
export function zoneOf(z: NudgeZoneDiag): NudgeZone {
  const out: NudgeZone = {
    min_um: [...z.minUm],
    max_um: [...z.maxUm],
    yaw_mdeg: z.yawMdeg,
    min_speed_um_s: z.minSpeedUmS,
    max_accel_um_s2: z.maxAccelUmS2,
  };
  if (z.name) out.name = z.name;
  if (nudgeMode(z) !== "top_up") out.mode = nudgeMode(z);
  if (z.pitchMdeg) out.pitch_mdeg = z.pitchMdeg;
  return out;
}

const clamp = (v: number, lo: number, hi: number): number => Math.min(Math.max(v, lo), hi);

/** The zone after one inspector edit, or null when the edit changes nothing (at a limit). */
export function nudgeEdit(z: NudgeZone, e: NudgeEdit): NudgeZone | null {
  const next: NudgeZone = { ...z, min_um: [...z.min_um], max_um: [...z.max_um] };
  switch (e.kind) {
    case "size": {
      const span = z.max_um[e.axis] - z.min_um[e.axis];
      if (e.by < 0 && span <= CELL_UM) return null;
      next.max_um[e.axis] = z.max_um[e.axis] + e.by * CELL_UM;
      return next;
    }
    case "yaw":
      next.yaw_mdeg = (((z.yaw_mdeg + e.by * NUDGE_YAW_STEP) % 360_000) + 360_000) % 360_000;
      return next;
    case "pitch": {
      const p = clamp((z.pitch_mdeg ?? 0) + e.by * NUDGE_PITCH_STEP, -NUDGE_PITCH_MAX, NUDGE_PITCH_MAX);
      if (p === (z.pitch_mdeg ?? 0)) return null;
      if (p) next.pitch_mdeg = p;
      else delete next.pitch_mdeg;
      return next;
    }
    case "speed": {
      const v = clamp(z.min_speed_um_s + e.by * NUDGE_SPEED_STEP, NUDGE_SPEED_STEP, NUDGE_MAX);
      if (v === z.min_speed_um_s) return null;
      next.min_speed_um_s = v;
      return next;
    }
    case "accel": {
      const v = clamp(z.max_accel_um_s2 + e.by * NUDGE_ACCEL_STEP, NUDGE_ACCEL_STEP, NUDGE_MAX);
      if (v === z.max_accel_um_s2) return null;
      next.max_accel_um_s2 = v;
      return next;
    }
    case "mode":
      if (e.mode === (z.mode ?? "top_up")) return null;
      if (e.mode === "top_up") delete next.mode;
      else next.mode = e.mode;
      return next;
  }
}

const HEADING: Record<number, string> = {
  0: "east",
  45000: "north-east",
  90000: "north",
  135000: "north-west",
  180000: "west",
  225000: "south-west",
  270000: "south",
  315000: "south-east",
};

const deg = (mdeg: number): string => `${(mdeg / 1000).toFixed(1).replace(/\.0$/, "")}°`;

/** The zone's size in cells along x, y and z: "12 × 1 × 1 cells". */
export function nudgeSizeText(z: Pick<NudgeZoneDiag, "minUm" | "maxUm">): string {
  const n = [0, 1, 2].map((i) => Math.round(((z.maxUm[i] ?? 0) - (z.minUm[i] ?? 0)) / CELL_UM));
  return `${n.join(" × ")} cells`;
}

/** The push direction: "north-east" (the ports line's compass, +x east, −z north), then any tilt. */
export function nudgeHeadingText(yawMdeg: number, pitchMdeg = 0): string {
  const yaw = HEADING[yawMdeg] ?? deg(yawMdeg);
  if (!pitchMdeg) return yaw;
  return `${yaw}, ${pitchMdeg > 0 ? "up" : "down"} ${deg(Math.abs(pitchMdeg))}`;
}

/** A speed (µm/s) or acceleration (µm/s²) in metres: "0.20 m/s", "1.00 m/s²". */
export const nudgeSpeedText = (umS: number): string => `${(umS / 1e6).toFixed(2)} m/s`;
export const nudgeAccelText = (umS2: number): string => `${(umS2 / 1e6).toFixed(2)} m/s²`;

/** A listed zone's label: its name, else "Zone 3" (one-based). */
export const nudgeName = (z: Pick<NudgeZoneDiag, "index" | "name">): string => z.name || `Zone ${z.index + 1}`;

/** The cell holding a point in metres (the orbit target): where Add zone goes with nothing selected. */
export function cellAt(p: readonly [number, number, number]): Vec3i {
  return [Math.round(p[0] / CELL_M), Math.max(Math.floor(p[1] / CELL_M), 0), Math.round(p[2] / CELL_M)];
}

/** SVG path data for a projected direction arrow: the shaft, then a two-stroke head at the tip. */
export function arrowPath(arrow: readonly (readonly [number, number])[] | null | undefined): string {
  if (arrow?.length !== 2) return "";
  const [t, hd] = arrow as [readonly [number, number], readonly [number, number]];
  const dx = hd[0] - t[0];
  const dy = hd[1] - t[1];
  const len = Math.hypot(dx, dy);
  if (len < 1) return "";
  const r = (v: number): string => (Math.round(v * 10) / 10).toString();
  const ux = dx / len;
  const uy = dy / len;
  const head = Math.min(9, len * 0.4);
  const wing = (s: number): string =>
    `M${r(hd[0])} ${r(hd[1])}L${r(hd[0] - ux * head - uy * head * 0.55 * s)} ${r(hd[1] - uy * head + ux * head * 0.55 * s)}`;
  return `M${r(t[0])} ${r(t[1])}L${r(hd[0])} ${r(hd[1])}${wing(1)}${wing(-1)}`;
}
