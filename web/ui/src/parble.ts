// Typed view of a `.parble` file, schema v1 (crates/parble-core/src/file.rs), kept by hand.
// parble-fixtures.json holds valid and rejected files; the Rust tests and test/parble.test.ts both
// check it, so the two validators cannot drift. Rust is the authority on load: it also runs the
// migration chain, so the browser posts file text to the bridge untouched.

export const FORMAT = "parble";
export const SCHEMA_VERSION = 1;
export const PHYSICS_VERSION = 2;
/** Every integer in a file fits a JS number exactly. */
export const MAX_SAFE = Number.MAX_SAFE_INTEGER;
export const MAX_NAME_CHARS = 120;
export const MAX_DESCRIPTION_CHARS = 2_000;
const MAX_PIECE_ID_LEN = 48;
const PIECE_ID = /^(blk|rail|mech|terr|music|deco)_[a-z0-9]+(_[a-z0-9]+)*$/;

export type MarbleSize = "mini" | "standard" | "shooter";
export type ReleaseMode = { mode: "one" } | { mode: "all" } | { mode: "interval"; ticks: number };
export type IntMap = Record<string, number>;

export interface Placement {
  piece: string;
  anchor: [number, number, number];
  yaw: 0 | 1 | 2 | 3;
  mirror: boolean;
  /** Omitted when empty. */
  config?: IntMap;
}

export interface HopperConfig {
  placement: number | null;
  queue: MarbleSize[];
  release: ReleaseMode;
}

export interface MechanismState {
  placement: number;
  state: IntMap;
}

export interface Metadata {
  name: string;
  /** Omitted when empty. */
  description?: string;
  author?: string;
}

export interface ParbleFile {
  format: typeof FORMAT;
  schema: number;
  physics: number;
  kit_contract: number;
  seed: number;
  placements: Placement[];
  hopper: HopperConfig;
  mechanisms: MechanismState[];
  metadata: Metadata;
}

type Obj = Record<string, unknown>;
const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);
const isInt = (v: unknown, lo: number, hi: number): v is number =>
  Number.isInteger(v) && (v as number) >= lo && (v as number) <= hi;
const isU32 = (v: unknown): v is number => isInt(v, 0, 0xffff_ffff);
const isI32 = (v: unknown): v is number => isInt(v, -0x8000_0000, 0x7fff_ffff);
const hasOnly = (o: Obj, keys: readonly string[]): boolean => Object.keys(o).every((k) => keys.includes(k));
const hasAll = (o: Obj, keys: readonly string[]): boolean => keys.every((k) => k in o);
const chars = (s: string): number => [...s].length;

class Invalid extends Error {}
const fail = (msg: string): never => {
  throw new Invalid(msg);
};

function intMap(v: unknown, what: string): IntMap {
  if (!isObj(v)) return fail(`${what} is not an object`);
  for (const [k, x] of Object.entries(v)) {
    if (!isInt(x, -MAX_SAFE, MAX_SAFE)) fail(`${what} ${k} is not an integer within ±(2^53 - 1)`);
  }
  return v as IntMap;
}

function placement(v: unknown, i: number): Placement {
  if (!isObj(v) || !hasOnly(v, ["piece", "anchor", "yaw", "mirror", "config"])) {
    return fail(`placement ${i} has unknown fields`);
  }
  if (!hasAll(v, ["piece", "anchor", "yaw", "mirror"])) fail(`placement ${i} is missing fields`);
  const { piece, anchor, yaw, mirror } = v;
  if (typeof piece !== "string" || piece.length > MAX_PIECE_ID_LEN || !PIECE_ID.test(piece)) {
    fail(`placement ${i} piece id is invalid`);
  }
  if (!Array.isArray(anchor) || anchor.length !== 3 || !anchor.every(isI32)) fail(`placement ${i} anchor is invalid`);
  if (!isInt(yaw, 0, 3)) fail(`placement ${i} yaw must be 0..=3`);
  if (typeof mirror !== "boolean") fail(`placement ${i} mirror must be a bool`);
  if ("config" in v) intMap(v.config, `placement ${i} config`);
  return v as unknown as Placement;
}

function release(v: unknown): ReleaseMode {
  if (!isObj(v)) return fail("release is not an object");
  if ((v.mode === "one" || v.mode === "all") && hasOnly(v, ["mode"])) return v as ReleaseMode;
  if (v.mode === "interval" && hasOnly(v, ["mode", "ticks"]) && isU32(v.ticks)) {
    if (v.ticks === 0) fail("release interval must be at least 1 tick");
    return v as ReleaseMode;
  }
  return fail("release mode is invalid");
}

function hopper(v: unknown, placements: number): HopperConfig {
  if (!isObj(v) || !hasOnly(v, ["placement", "queue", "release"]) || !hasAll(v, ["placement", "queue", "release"])) {
    return fail("hopper fields are invalid");
  }
  if (v.placement !== null && !isU32(v.placement)) fail("hopper placement is invalid");
  if (typeof v.placement === "number" && v.placement >= placements) fail("hopper placement is out of range");
  const sizes = ["mini", "standard", "shooter"];
  if (!Array.isArray(v.queue) || !v.queue.every((m) => sizes.includes(m as string))) fail("hopper queue is invalid");
  release(v.release);
  return v as unknown as HopperConfig;
}

function metadata(v: unknown): Metadata {
  if (!isObj(v) || !hasOnly(v, ["name", "description", "author"]) || typeof v.name !== "string") {
    return fail("metadata is invalid");
  }
  for (const k of ["description", "author"] as const) {
    if (k in v && typeof v[k] !== "string") fail(`metadata ${k} must be a string`);
  }
  if (chars(v.name) > MAX_NAME_CHARS || chars((v.author as string | undefined) ?? "") > MAX_NAME_CHARS) {
    fail(`name and author are limited to ${MAX_NAME_CHARS} characters`);
  }
  if (chars((v.description as string | undefined) ?? "") > MAX_DESCRIPTION_CHARS) {
    fail(`description is limited to ${MAX_DESCRIPTION_CHARS} characters`);
  }
  return v as unknown as Metadata;
}

const TOP = [
  "format",
  "schema",
  "physics",
  "kit_contract",
  "seed",
  "placements",
  "hopper",
  "mechanisms",
  "metadata",
] as const;

/** `v` as a schema-v1 file, or an error message matching a case Rust refuses. */
export function checkParbleFile(v: unknown): { ok: true; file: ParbleFile } | { ok: false; error: string } {
  try {
    if (!isObj(v)) fail("top level is not an object");
    const o = v as Obj;
    if (o.format !== FORMAT) fail('format must be "parble"');
    if (!isU32(o.schema)) fail("missing or non-integer schema version");
    if ((o.schema as number) > SCHEMA_VERSION) fail(`schema ${o.schema} is newer than ${SCHEMA_VERSION}`);
    if (o.schema !== SCHEMA_VERSION) fail(`no migration from schema ${o.schema}`);
    if (!hasOnly(o, TOP) || !hasAll(o, TOP)) fail("unknown or missing top-level fields");
    if (!isU32(o.physics) || !isU32(o.kit_contract)) fail("version fields are invalid");
    if (!isInt(o.seed, 0, MAX_SAFE)) fail("seed must be an integer in 0..=2^53 - 1");
    if (!Array.isArray(o.placements)) fail("placements is not an array");
    const placements = (o.placements as unknown[]).map(placement);
    hopper(o.hopper, placements.length);
    if (!Array.isArray(o.mechanisms)) fail("mechanisms is not an array");
    let last = -1;
    for (const m of o.mechanisms as unknown[]) {
      if (!isObj(m) || !hasOnly(m, ["placement", "state"]) || !isU32(m.placement)) fail("mechanism is invalid");
      const { placement: p, state } = m as Obj;
      if ((p as number) >= placements.length) fail(`mechanism placement ${p} is out of range`);
      if ((p as number) <= last) fail("mechanisms must be sorted by placement without repeats");
      last = p as number;
      intMap(state, "mechanism state");
    }
    metadata(o.metadata);
    return { ok: true, file: o as unknown as ParbleFile };
  } catch (e) {
    if (e instanceof Invalid) return { ok: false, error: e.message };
    throw e;
  }
}

/** Parses `.parble` text; null when Rust would refuse it. */
export function parseParble(text: string): ParbleFile | null {
  let v: unknown;
  try {
    v = JSON.parse(text);
  } catch {
    return null;
  }
  const r = checkParbleFile(v);
  return r.ok ? r.file : null;
}
