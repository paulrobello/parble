// The runtime SFX catalog (sfx.json, written by scripts/web/stage-audio.mjs) and variant selection.

export const CATALOG_SCHEMA = "parble.sfx-runtime/1";

export interface Variant {
  stem: string;
  duration: number;
  ogg: string;
  m4a: string;
}

export interface Cue {
  loop: boolean;
  variants: Variant[];
}

export interface Catalog {
  schema: typeof CATALOG_SCHEMA;
  cues: Record<string, Cue>;
}

type Obj = Record<string, unknown>;
const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);
const isStr = (v: unknown): v is string => typeof v === "string";

function decodeVariant(v: unknown): Variant | null {
  return isObj(v) && isStr(v.stem) && typeof v.duration === "number" && v.duration > 0 && isStr(v.ogg) && isStr(v.m4a)
    ? { stem: v.stem, duration: v.duration, ogg: v.ogg, m4a: v.m4a }
    : null;
}

/** `v` as a catalog, or null when it is not one. Cues with no usable variant are dropped. */
export function decodeCatalog(v: unknown): Catalog | null {
  if (!isObj(v) || v.schema !== CATALOG_SCHEMA || !isObj(v.cues)) return null;
  const cues: Record<string, Cue> = {};
  for (const [id, c] of Object.entries(v.cues)) {
    if (!isObj(c) || typeof c.loop !== "boolean" || !Array.isArray(c.variants)) return null;
    const variants = c.variants.map(decodeVariant);
    if (variants.some((x) => x === null)) return null;
    if (variants.length > 0) cues[id] = { loop: c.loop, variants: variants as Variant[] };
  }
  return { schema: CATALOG_SCHEMA, cues };
}

/** Picks variants at random from a seeded `rng`, never the same one twice in a row for a cue. */
export class VariantPicker {
  readonly #rng: () => number;
  readonly #last = new Map<string, number>();

  constructor(rng: () => number) {
    this.#rng = rng;
  }

  pick(cue: string, variants: readonly Variant[]): Variant | null {
    const n = variants.length;
    if (n === 0) return null;
    const last = this.#last.get(cue);
    let i = Math.floor(this.#rng() * (last === undefined || n === 1 ? n : n - 1));
    if (last !== undefined && n > 1 && i >= last) i++;
    this.#last.set(cue, i);
    return variants[i] ?? null;
  }
}
