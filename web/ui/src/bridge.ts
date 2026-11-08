// Typed view of the wasm `Bridge` (crates/parble-app/src/bridge.rs), kept by hand. bridge-fixtures.json
// holds one example of every command and event; the Rust tests and test/bridge.test.ts both check it.

export const PROTOCOL = 1;

export type BridgeCommand =
  | { type: "ping"; id: number }
  | { type: "loadTrack"; json: string }
  | { type: "runTrack"; ticks: number }
  | { type: "exportTrack" };

export type BridgeEvent =
  | { type: "ready"; protocol: number }
  | { type: "frame"; tick: number; fps: number }
  | { type: "pong"; id: number }
  | { type: "trackLoaded"; name: string; schema: number; placements: number; marbles: number }
  | { type: "runFinished"; ticks: number; hash: string }
  | { type: "trackExported"; name: string; json: string }
  | { type: "bridgeError"; command: string; message: string };

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
] as const satisfies readonly BridgeEvent["type"][];

/** The wasm-bindgen class as Trunk exposes it on `window.wasmBindings`. */
export interface WasmBridge {
  postCommand(json: string): void;
  drainEvents(): string;
}

export interface WasmBindings {
  Bridge: new () => WasmBridge;
}

type Obj = Record<string, unknown>;

const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);
const isU32 = (v: unknown): v is number => Number.isInteger(v) && (v as number) >= 0 && (v as number) <= 0xffff_ffff;
const isU64 = (v: unknown): v is number => Number.isSafeInteger(v) && (v as number) >= 0;
const hasOnly = (o: Obj, keys: readonly string[]): boolean => Object.keys(o).every((k) => keys.includes(k));
const isStr = (v: unknown): v is string => typeof v === "string";
const isHash = (v: unknown): v is string => isStr(v) && /^[0-9a-f]{16}$/.test(v);

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
      return hasOnly(v, ["type", "tick", "fps"]) && isU64(v.tick) && typeof v.fps === "number"
        ? { type: "frame", tick: v.tick, fps: v.fps }
        : null;
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
