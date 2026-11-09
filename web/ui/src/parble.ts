// Typed view of a `.parble` file, schema v1 (crates/parble-core/src/file.rs), kept by hand.
// parble-fixtures.json holds valid and rejected files; the Rust tests and test/parble.test.ts both
// check it, so the two validators cannot drift. Rust is the authority on load: it also runs the
// migration chain, so the browser posts file text to the bridge untouched.

export const FORMAT = "parble";
export const SCHEMA_VERSION = 1;
export const PHYSICS_VERSION = 4;
/** Every integer in a file fits a JS number exactly. */
export const MAX_SAFE = Number.MAX_SAFE_INTEGER;
export const MAX_NAME_CHARS = 120;
export const MAX_DESCRIPTION_CHARS = 2_000;
const MAX_PIECE_ID_LEN = 48;
const PIECE_ID = /^(blk|rail|mech|terr|music|deco)_[a-z0-9]+(_[a-z0-9]+)*$/;

export type MarbleSize = "mini" | "standard" | "shooter";
export type ReleaseMode =
  | { mode: "sequential" }
  | { mode: "one" }
  | { mode: "all" }
  | { mode: "interval"; ticks: number };
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

/** Camera zones (crates/parble-core/src/camera_zone.rs): integer µm, millidegrees and ms. */
export type Vec3i = [number, number, number];
export type ZoneVolume =
  | { shape: "box"; min_um: Vec3i; max_um: Vec3i }
  | { shape: "cylinder"; base_um: Vec3i; height_um: number; radius_um: number };
export type ShotTarget =
  | { kind: "marble" }
  | { kind: "point"; at_um: Vec3i }
  | { kind: "axis"; base_um: Vec3i; height_um: number };
export type FocusOn = "marble" | "target";
export type ShotRule =
  | { rule: "follow"; back_um: number; up_um: number; ahead_um: number }
  | {
      rule: "frame";
      target: ShotTarget;
      focus?: FocusOn;
      margin_permille: number;
      elevation_mdeg: number;
      azimuth_mdeg?: number;
      fov_mdeg?: number;
    }
  | { rule: "orbit"; center_um: Vec3i; radius_um: number; up_um: number; speed_mdeg_s: number; focus?: FocusOn }
  | { rule: "fixed"; eye_um: Vec3i; look_um: Vec3i; focus?: FocusOn; fov_mdeg?: number };

export interface CameraZone {
  /** Omitted when empty. */
  name?: string;
  volume: ZoneVolume;
  shot: ShotRule;
  priority: number;
  blend_in_ms: number;
  blend_out_ms: number;
  /** Omitted when 0. */
  hold_ms?: number;
}

export const MAX_ZONES = 256;
const MAX_ZONE_MS = 10_000;

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
  /** Omitted when empty. */
  camera_zones?: CameraZone[];
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
  if ((v.mode === "sequential" || v.mode === "one" || v.mode === "all") && hasOnly(v, ["mode"])) {
    return v as ReleaseMode;
  }
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

const isVec3i = (v: unknown): v is Vec3i => Array.isArray(v) && v.length === 3 && v.every(isI32);

/** Checks the fields `o` must have (`need`) and may have (`may`), with a predicate per field. */
function fields(
  o: unknown,
  what: string,
  need: Record<string, (v: unknown) => boolean>,
  may: Record<string, (v: unknown) => boolean> = {},
): Obj {
  if (!isObj(o)) return fail(`${what} is not an object`);
  const all = { ...need, ...may };
  if (!hasOnly(o, Object.keys(all)) || !hasAll(o, Object.keys(need))) fail(`${what} has unknown or missing fields`);
  for (const [k, ok] of Object.entries(all)) if (k in o && !ok(o[k])) fail(`${what} ${k} is invalid`);
  return o;
}

const isFocus = (v: unknown) => v === "marble" || v === "target";
const isFov = (v: unknown) => isInt(v, 5_000, 120_000);
const lit = (s: string) => (v: unknown) => v === s;

function shotTarget(v: unknown, what: string): void {
  const kind = isObj(v) ? v.kind : undefined;
  if (kind === "marble") fields(v, what, { kind: lit("marble") });
  else if (kind === "point") fields(v, what, { kind: lit("point"), at_um: isVec3i });
  else if (kind === "axis") {
    fields(v, what, { kind: lit("axis"), base_um: isVec3i, height_um: (h) => isI32(h) && h > 0 });
  } else fail(`${what} kind is invalid`);
}

function cameraZone(v: unknown, i: number): void {
  const what = `camera zone ${i}`;
  const ms = (x: unknown) => isInt(x, 0, MAX_ZONE_MS);
  const z = fields(
    v,
    what,
    { volume: isObj, shot: isObj, priority: isI32, blend_in_ms: ms, blend_out_ms: ms },
    { name: (n) => typeof n === "string" && chars(n) <= MAX_NAME_CHARS, hold_ms: ms },
  );
  const vol = z.volume as Obj;
  if (vol.shape === "box") {
    fields(vol, `${what} volume`, { shape: lit("box"), min_um: isVec3i, max_um: isVec3i });
    const [lo, hi] = [vol.min_um as Vec3i, vol.max_um as Vec3i];
    if (lo.some((x, k) => x >= (hi[k] as number))) fail(`${what} box min_um must be below max_um`);
  } else if (vol.shape === "cylinder") {
    const pos = (x: unknown) => isI32(x) && x > 0;
    fields(vol, `${what} volume`, { shape: lit("cylinder"), base_um: isVec3i, height_um: pos, radius_um: pos });
  } else fail(`${what} volume shape is invalid`);
  const s = z.shot as Obj;
  const w = `${what} shot`;
  if (s.rule === "follow") {
    fields(s, w, { rule: lit("follow"), back_um: (b) => isI32(b) && b > 0, up_um: isI32, ahead_um: isI32 });
  } else if (s.rule === "frame") {
    fields(
      s,
      w,
      {
        rule: lit("frame"),
        target: isObj,
        margin_permille: (m) => isInt(m, 1_000, 5_000),
        elevation_mdeg: (e) => isInt(e, -80_000, 85_000),
      },
      { focus: isFocus, azimuth_mdeg: isI32, fov_mdeg: isFov },
    );
    shotTarget(s.target, `${w} target`);
  } else if (s.rule === "orbit") {
    fields(
      s,
      w,
      {
        rule: lit("orbit"),
        center_um: isVec3i,
        radius_um: (r) => isI32(r) && r > 0,
        up_um: isI32,
        speed_mdeg_s: isI32,
      },
      { focus: isFocus },
    );
  } else if (s.rule === "fixed") {
    fields(s, w, { rule: lit("fixed"), eye_um: isVec3i, look_um: isVec3i }, { focus: isFocus, fov_mdeg: isFov });
    if ((s.eye_um as Vec3i).every((x, k) => x === (s.look_um as Vec3i)[k])) fail(`${w} eye_um and look_um must differ`);
  } else fail(`${w} rule is invalid`);
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
    if (!hasOnly(o, [...TOP, "camera_zones"]) || !hasAll(o, TOP)) fail("unknown or missing top-level fields");
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
    if ("camera_zones" in o) {
      if (!Array.isArray(o.camera_zones) || o.camera_zones.length > MAX_ZONES) fail("camera_zones is invalid");
      (o.camera_zones as unknown[]).forEach(cameraZone);
    }
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
