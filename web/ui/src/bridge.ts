// Typed view of the wasm `Bridge` (crates/parble-app/src/bridge.rs, audio types in audio.rs), kept by
// hand. bridge-fixtures.json holds an example of every command and event plus events both sides
// refuse; the Rust tests and test/bridge.test.ts both check it.

export const PROTOCOL = 3;

export type BridgeCommand =
  | { type: "ping"; id: number }
  | { type: "loadTrack"; json: string }
  | { type: "runTrack"; ticks: number }
  | { type: "exportTrack" };

export type BridgeEvent =
  | { type: "ready"; protocol: number }
  | FrameEvent
  | { type: "pong"; id: number }
  | { type: "trackLoaded"; name: string; schema: number; placements: number; marbles: number }
  | { type: "runFinished"; ticks: number; hash: string }
  | { type: "trackExported"; name: string; json: string }
  | { type: "bridgeError"; command: string; message: string }
  | AudioFrame
  | SplitEvent;

/** A checkpoint pass: `tick` is sim ticks since the run started (240 Hz), `instance` the checkpoint's anchor cell. */
export interface SplitEvent {
  type: "split";
  tick: number;
  instance: Vec3;
  marble: number;
}

/** One rendered frame. `paused` and `timeScale` (crates/parble-app/src/playback.rs) are sent only when not running at 1x. */
export interface FrameEvent {
  type: "frame";
  tick: number;
  fps: number;
  paused?: true;
  timeScale?: number;
}

/** Marble type (crates/parble-sim `MarbleKind::name`). */
export const MARBLE_KINDS = ["cats-eye", "glazed", "steel", "swirl", "onionskin", "opal", "glow"] as const;
export type MarbleKind = (typeof MARBLE_KINDS)[number];

/** Marble size (crates/parble-sim `MarbleSize::name`): 10, 16 and 25 mm. */
export const MARBLE_SIZES = ["mini", "standard", "shooter"] as const;
export type MarbleSize = (typeof MARBLE_SIZES)[number];

/** Contact surface: a kit surface, or `marble` for another marble. */
export const SURFACES = ["wood", "rail", "wood_end_grain", "marble"] as const;
export type Surface = (typeof SURFACES)[number];

export type Vec3 = [number, number, number];

/** One marble's contact starts in a frame, coalesced to the strongest (crates/parble-app/src/audio.rs). */
export interface AudioContact {
  marble: number;
  kind: MarbleKind;
  size: MarbleSize;
  surface: Surface;
  /** The other marble's type when `surface` is `marble`. */
  otherKind: MarbleKind | null;
  /** Largest normal impulse, N·s. */
  impulse: number;
  /** Contact starts coalesced into this one. */
  count: number;
  position: Vec3;
}

/**
 * One marble's roll state in a frame: latest speed (m/s), latest touching surface, any contact, and
 * whether the marble is inside a redirect's tunnel (a missing `tunnel` reads as false).
 */
export interface AudioRoll {
  marble: number;
  kind: MarbleKind;
  size: MarbleSize;
  speed: number;
  surface: Surface | null;
  contact: boolean;
  position: Vec3;
  tunnel: boolean;
}

/** A mechanism event at its instance's anchor cell; `piece` is the kit piece id ("" when unknown). */
export type AudioMechanism =
  | { kind: "sensorEnter"; tick: number; instance: Vec3; piece: string; sensor: number; marble: number }
  | { kind: "sensorExit"; tick: number; instance: Vec3; piece: string; sensor: number; marble: number }
  | { kind: "signal"; tick: number; instance: Vec3; piece: string; name: string; value: number };

export interface AudioCamera {
  position: Vec3;
  forward: Vec3;
  up: Vec3;
}

/** The audio telemetry since the last drain; at most one per drain. */
export interface AudioFrame {
  type: "audioFrame";
  tick: number;
  contacts: AudioContact[];
  rolls: AudioRoll[];
  mechanisms: AudioMechanism[];
  camera: AudioCamera | null;
  /** Playback is paused: the engine releases its roll loops. Sent only when true. */
  paused?: true;
}

/** Every wire tag, in the Rust enums' declaration order. */
export const COMMAND_TYPES = [
  "ping",
  "loadTrack",
  "runTrack",
  "exportTrack",
] as const satisfies readonly BridgeCommand["type"][];
export const EVENT_TYPES = [
  "ready",
  "frame",
  "pong",
  "trackLoaded",
  "runFinished",
  "trackExported",
  "bridgeError",
  "audioFrame",
  "split",
] as const satisfies readonly BridgeEvent["type"][];

/** The wasm-bindgen class as Trunk exposes it on `window.wasmBindings`. */
export interface WasmBridge {
  postCommand(json: string): void;
  drainEvents(): string;
}

export interface WasmBindings {
  Bridge: new () => WasmBridge;
  /** Editor actions (crates/parble-app/src/editor.rs); present in every wasm build, used in the editor scenario. */
  parbleEdit?(json: string): void;
}

type Obj = Record<string, unknown>;

const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);
const isU32 = (v: unknown): v is number => Number.isInteger(v) && (v as number) >= 0 && (v as number) <= 0xffff_ffff;
const isU64 = (v: unknown): v is number => Number.isSafeInteger(v) && (v as number) >= 0;
const hasOnly = (o: Obj, keys: readonly string[]): boolean => Object.keys(o).every((k) => keys.includes(k));
const isStr = (v: unknown): v is string => typeof v === "string";
const isHash = (v: unknown): v is string => isStr(v) && /^[0-9a-f]{16}$/.test(v);
const isNum = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v);
const isI32 = (v: unknown): v is number =>
  Number.isInteger(v) && (v as number) >= -0x8000_0000 && (v as number) <= 0x7fff_ffff;
const isI64 = (v: unknown): v is number => Number.isSafeInteger(v);
const isVec3 = (v: unknown, el: (x: unknown) => boolean = isNum): v is Vec3 =>
  Array.isArray(v) && v.length === 3 && v.every(el);
const isKind = (v: unknown): v is MarbleKind => (MARBLE_KINDS as readonly unknown[]).includes(v);
const isSize = (v: unknown): v is MarbleSize => (MARBLE_SIZES as readonly unknown[]).includes(v);
const isSurface = (v: unknown): v is Surface => (SURFACES as readonly unknown[]).includes(v);
const isListOf = <T>(v: unknown, decode: (x: unknown) => T | null): T[] | null => {
  if (!Array.isArray(v)) return null;
  const out: T[] = [];
  for (const x of v) {
    const d = decode(x);
    if (d === null) return null;
    out.push(d);
  }
  return out;
};

function decodeContact(v: unknown): AudioContact | null {
  if (!isObj(v) || !hasOnly(v, ["marble", "kind", "size", "surface", "otherKind", "impulse", "count", "position"]))
    return null;
  const otherKind = v.otherKind ?? null;
  return isU32(v.marble) &&
    isKind(v.kind) &&
    isSize(v.size) &&
    isSurface(v.surface) &&
    (otherKind === null || isKind(otherKind)) &&
    isNum(v.impulse) &&
    isU32(v.count) &&
    isVec3(v.position)
    ? {
        marble: v.marble,
        kind: v.kind,
        size: v.size,
        surface: v.surface,
        otherKind,
        impulse: v.impulse,
        count: v.count,
        position: [...v.position],
      }
    : null;
}

function decodeRoll(v: unknown): AudioRoll | null {
  if (!isObj(v) || !hasOnly(v, ["marble", "kind", "size", "speed", "surface", "contact", "position", "tunnel"]))
    return null;
  const surface = v.surface ?? null;
  const tunnel = v.tunnel ?? false;
  return isU32(v.marble) &&
    isKind(v.kind) &&
    isSize(v.size) &&
    isNum(v.speed) &&
    (surface === null || isSurface(surface)) &&
    typeof v.contact === "boolean" &&
    isVec3(v.position) &&
    typeof tunnel === "boolean"
    ? {
        marble: v.marble,
        kind: v.kind,
        size: v.size,
        speed: v.speed,
        surface,
        contact: v.contact,
        position: [...v.position],
        tunnel,
      }
    : null;
}

function decodeMechanism(v: unknown): AudioMechanism | null {
  if (!isObj(v) || !isU64(v.tick) || !isVec3(v.instance, isI32) || !isStr(v.piece)) return null;
  const { tick, piece } = v;
  const instance: Vec3 = [...v.instance];
  switch (v.kind) {
    case "sensorEnter":
    case "sensorExit":
      return hasOnly(v, ["kind", "tick", "instance", "piece", "sensor", "marble"]) && isU32(v.sensor) && isU32(v.marble)
        ? { kind: v.kind, tick, instance, piece, sensor: v.sensor, marble: v.marble }
        : null;
    case "signal":
      return hasOnly(v, ["kind", "tick", "instance", "piece", "name", "value"]) && isStr(v.name) && isI64(v.value)
        ? { kind: "signal", tick, instance, piece, name: v.name, value: v.value }
        : null;
    default:
      return null;
  }
}

function decodeCamera(v: unknown): AudioCamera | null {
  return isObj(v) &&
    hasOnly(v, ["position", "forward", "up"]) &&
    isVec3(v.position) &&
    isVec3(v.forward) &&
    isVec3(v.up)
    ? { position: [...v.position], forward: [...v.forward], up: [...v.up] }
    : null;
}

function decodeAudioFrame(v: Obj): AudioFrame | null {
  if (!hasOnly(v, ["type", "tick", "contacts", "rolls", "mechanisms", "camera", "paused"]) || !isU64(v.tick))
    return null;
  if (v.paused !== undefined && typeof v.paused !== "boolean") return null;
  const contacts = isListOf(v.contacts, decodeContact);
  const rolls = isListOf(v.rolls, decodeRoll);
  const mechanisms = isListOf(v.mechanisms, decodeMechanism);
  const rawCamera = v.camera ?? null;
  const camera = rawCamera === null ? null : decodeCamera(rawCamera);
  if (!contacts || !rolls || !mechanisms || (rawCamera !== null && camera === null)) return null;
  const frame: AudioFrame = { type: "audioFrame", tick: v.tick, contacts, rolls, mechanisms, camera };
  if (v.paused) frame.paused = true;
  return frame;
}

function decodeFrame(v: Obj): FrameEvent | null {
  if (!hasOnly(v, ["type", "tick", "fps", "paused", "timeScale"]) || !isU64(v.tick) || typeof v.fps !== "number")
    return null;
  if (v.paused !== undefined && typeof v.paused !== "boolean") return null;
  if (v.timeScale !== undefined && !isNum(v.timeScale)) return null;
  const frame: FrameEvent = { type: "frame", tick: v.tick, fps: v.fps };
  if (v.paused) frame.paused = true;
  if (v.timeScale !== undefined && v.timeScale !== 1) frame.timeScale = v.timeScale;
  return frame;
}

/** `v` as a command, or null when Rust would refuse it (unknown type, missing or extra fields). */
export function decodeCommand(v: unknown): BridgeCommand | null {
  if (!isObj(v)) return null;
  switch (v.type) {
    case "ping":
      return hasOnly(v, ["type", "id"]) && isU32(v.id) ? { type: "ping", id: v.id } : null;
    case "loadTrack":
      return hasOnly(v, ["type", "json"]) && isStr(v.json) ? { type: "loadTrack", json: v.json } : null;
    case "runTrack":
      return hasOnly(v, ["type", "ticks"]) && isU32(v.ticks) ? { type: "runTrack", ticks: v.ticks } : null;
    case "exportTrack":
      return hasOnly(v, ["type"]) ? { type: "exportTrack" } : null;
    default:
      return null;
  }
}

/** `v` as an event, or null when it is not one Rust sends. */
export function decodeEvent(v: unknown): BridgeEvent | null {
  if (!isObj(v)) return null;
  switch (v.type) {
    case "ready":
      return hasOnly(v, ["type", "protocol"]) && isU32(v.protocol) ? { type: "ready", protocol: v.protocol } : null;
    case "frame":
      return decodeFrame(v);
    case "pong":
      return hasOnly(v, ["type", "id"]) && isU32(v.id) ? { type: "pong", id: v.id } : null;
    case "trackLoaded":
      return hasOnly(v, ["type", "name", "schema", "placements", "marbles"]) &&
        isStr(v.name) &&
        isU32(v.schema) &&
        isU32(v.placements) &&
        isU32(v.marbles)
        ? { type: "trackLoaded", name: v.name, schema: v.schema, placements: v.placements, marbles: v.marbles }
        : null;
    case "runFinished":
      return hasOnly(v, ["type", "ticks", "hash"]) && isU32(v.ticks) && isHash(v.hash)
        ? { type: "runFinished", ticks: v.ticks, hash: v.hash }
        : null;
    case "trackExported":
      return hasOnly(v, ["type", "name", "json"]) && isStr(v.name) && isStr(v.json)
        ? { type: "trackExported", name: v.name, json: v.json }
        : null;
    case "bridgeError":
      return hasOnly(v, ["type", "command", "message"]) && isStr(v.command) && isStr(v.message)
        ? { type: "bridgeError", command: v.command, message: v.message }
        : null;
    case "audioFrame":
      return decodeAudioFrame(v);
    case "split":
      return hasOnly(v, ["type", "tick", "instance", "marble"]) &&
        isU64(v.tick) &&
        isVec3(v.instance, isI32) &&
        isU32(v.marble)
        ? { type: "split", tick: v.tick, instance: [...v.instance], marble: v.marble }
        : null;
    default:
      return null;
  }
}

/** The bridge with typed commands and decoded events. */
export class Bridge {
  readonly #raw: WasmBridge;

  constructor(raw: WasmBridge) {
    this.#raw = raw;
  }

  post(cmd: BridgeCommand): void {
    this.#raw.postCommand(JSON.stringify(cmd));
  }

  /** The events since the last drain, oldest first. Unknown entries are skipped. */
  drain(): BridgeEvent[] {
    const parsed: unknown = JSON.parse(this.#raw.drainEvents());
    if (!Array.isArray(parsed)) return [];
    const out: BridgeEvent[] = [];
    for (const v of parsed) {
      const e = decodeEvent(v);
      if (e) out.push(e);
    }
    return out;
  }
}
