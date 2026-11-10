// Title settings (P5-T3): the quality tier (detected on first run, overridable), the showcase camera
// and the hopper marble pick, persisted to localStorage as one JSON record. They reach the engine only
// through the scene URL the title launches (launchUrl), so a direct scene URL is never changed by them.

export const SETTINGS_KEY = "parble.settings/1";

export const TIERS = ["low", "medium", "high", "ultra"] as const;
export type Tier = (typeof TIERS)[number];

export const CAMERAS = ["director", "orbit", "overview"] as const;
export type Camera = (typeof CAMERAS)[number];

/** The `--marble` types (docs/marbles.md), in picker order. */
export const MARBLE_TYPES = ["cats-eye", "swirl", "onionskin", "opal", "glow", "glazed", "steel"] as const;
export type MarbleType = (typeof MARBLE_TYPES)[number];

export const MARBLE_SIZES = ["mini", "standard", "shooter"] as const;
export type MarbleSize = (typeof MARBLE_SIZES)[number];

export interface MarblePick {
  type: MarbleType;
  size: MarbleSize;
}

export interface AdapterInfo {
  vendor: string;
  architecture: string;
  device: string;
  description: string;
}

export interface QualityRecord {
  /** The first-run detection's tier; null until it has run. */
  detected: Tier | null;
  /** The player's choice; null follows the detected tier. */
  override: Tier | null;
  adapter: AdapterInfo | null;
  /** The benchmark's median GPU time for one 1080p test pass, ms. */
  benchMs: number | null;
  /** ISO time of the detection. */
  at: string | null;
}

export interface Settings {
  quality: QualityRecord;
  camera: Camera;
  /** null: the legacy standard marble (no `marble=` switch). */
  marble: MarblePick | null;
}

export const defaultSettings = (): Settings => ({
  quality: { detected: null, override: null, adapter: null, benchMs: null, at: null },
  camera: "director",
  marble: null,
});

type Obj = Record<string, unknown>;
const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);
const oneOf = <T extends string>(all: readonly T[], v: unknown): T | null =>
  typeof v === "string" && (all as readonly string[]).includes(v) ? (v as T) : null;
const str = (v: unknown): string => (typeof v === "string" ? v.slice(0, 200) : "");

function parseAdapter(v: unknown): AdapterInfo | null {
  if (!isObj(v)) return null;
  return {
    vendor: str(v.vendor),
    architecture: str(v.architecture),
    device: str(v.device),
    description: str(v.description),
  };
}

/** Settings from `raw` (the stored JSON), each field falling back to its default when missing or invalid. */
export function parseSettings(raw: string | null): Settings {
  const d = defaultSettings();
  if (!raw) return d;
  let v: unknown;
  try {
    v = JSON.parse(raw);
  } catch {
    return d;
  }
  if (!isObj(v)) return d;
  const q = isObj(v.quality) ? v.quality : {};
  const m = isObj(v.marble) ? v.marble : null;
  const type = m ? oneOf(MARBLE_TYPES, m.type) : null;
  const size = m ? oneOf(MARBLE_SIZES, m.size) : null;
  return {
    quality: {
      detected: oneOf(TIERS, q.detected),
      override: oneOf(TIERS, q.override),
      adapter: parseAdapter(q.adapter),
      benchMs: typeof q.benchMs === "number" && Number.isFinite(q.benchMs) && q.benchMs >= 0 ? q.benchMs : null,
      at: typeof q.at === "string" ? q.at.slice(0, 40) : null,
    },
    camera: oneOf(CAMERAS, v.camera) ?? d.camera,
    marble: type && size ? { type, size } : null,
  };
}

export function loadSettings(storage: Pick<Storage, "getItem"> | null): Settings {
  try {
    return parseSettings(storage?.getItem(SETTINGS_KEY) ?? null);
  } catch {
    return defaultSettings();
  }
}

/** Store `s`; false when storage refuses (private mode, quota). */
export function saveSettings(storage: Pick<Storage, "setItem"> | null, s: Settings): boolean {
  try {
    storage?.setItem(SETTINGS_KEY, JSON.stringify(s));
    return storage !== null;
  } catch {
    return false;
  }
}

/** The tier a launch uses: the override, else the detected tier, else null (the engine default). */
export const effectiveTier = (q: QualityRecord): Tier | null => q.override ?? q.detected;

/**
 * The title shows on a bare page load, or with `title=1`. Any other query (every agent switch,
 * `mute=1` included) or `title=0` goes straight to the scene.
 */
export function titleWanted(search: string): boolean {
  const q = new URLSearchParams(search);
  const t = q.get("title");
  if (t === "1") return true;
  if (t !== null) return false;
  return [...q.keys()].length === 0;
}

export type Launch = "build" | "watch";

/**
 * The scene query the title opens: the editor (`build`) with the marble pick, or the showcase
 * (`watch`) with the camera, both at the effective tier. `mute=1` is carried over.
 */
export function launchQuery(kind: Launch, s: Settings, search: string): string {
  const q = new URLSearchParams();
  q.set("scenario", kind === "build" ? "editor" : "showcase");
  const tier = effectiveTier(s.quality);
  if (tier) q.set("quality", tier);
  if (kind === "build" && s.marble) {
    q.set("marble", s.marble.type);
    q.set("marble-size", s.marble.size);
  }
  if (kind === "watch" && s.camera !== "director") q.set("camera", s.camera);
  if (new URLSearchParams(search).get("mute") === "1") q.set("mute", "1");
  return `?${q.toString()}`;
}
