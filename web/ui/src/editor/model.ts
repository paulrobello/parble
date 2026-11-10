// Typed view of `window.__parble.editor` (EditorDiag, crates/parble-app/src/editor.rs) and the
// `parbleEdit` actions the overlay sends, kept by hand like bridge.ts. Pure helpers only: names,
// families, issue sentences and labels, so Vitest can check every string the panel shows.

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
}

export interface EditorDiag {
  tool: Tool;
  brush: { piece: string; yaw: number; mirror: boolean };
  palette: string[];
  pieces: number;
  visiblePieces: number;
  slice: number | null;
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
  /** The last run, stop or reset: the engine's build and install time. */
  lastRun?: { op: string; buildMs: number; pieces: number } | null;
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
  | { op: "move"; offset: Vec3i }
  | { op: "boxSelect"; a: Vec3i; b: Vec3i; shift?: boolean }
  | { op: "camera"; target: [number, number, number]; yaw: number; pitch: number; radius: number }
  | { op: "keys"; enabled: boolean }
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
  | { op: "mechConfig"; key: string; value: number };

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

/** The stepper's value: "L0–L4" for all levels, "≤ L2" when sliced. */
export function levelLabel(slice: number | null, maxLevel: number): string {
  return slice === null ? `L0–L${Math.max(maxLevel, 0)}` : `≤ L${slice}`;
}

/** `[` one level down from all or the slice; never below 0. */
export const sliceDown = (slice: number | null, maxLevel: number): number => Math.max((slice ?? maxLevel + 1) - 1, 0);
/** `]` one level up; past the top shows all levels. */
export const sliceUp = (slice: number | null, maxLevel: number): number | null =>
  slice === null || slice + 1 > maxLevel ? null : slice + 1;

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
