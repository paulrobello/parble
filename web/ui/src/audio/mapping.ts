// Pure mappings from the audio stream (bridge.ts AudioFrame) to cue ids and voice parameters. Cue ids
// are the approved P4-T2 ids (docs/audio/cue-sheet.md), staged by scripts/web/stage-audio.mjs.

import type { AudioContact, MarbleKind, MarbleSize, Surface, Vec3 } from "../bridge";

/** Sound material of a marble type. Swirl, onionskin, opal and glow reuse the glass cues (cue sheet). */
export type Material = "glass" | "solid" | "steel";
const MATERIAL_ORDER: readonly Material[] = ["glass", "solid", "steel"];

export function materialOf(kind: MarbleKind): Material {
  if (kind === "steel") return "steel";
  if (kind === "glazed") return "solid";
  return "glass";
}

/** Marble diameter per size, m (docs/kit-contract.md, parble-core `MARBLE_*_DIAMETER_UM`). */
export const SIZE_DIAMETER_M: Readonly<Record<MarbleSize, number>> = { mini: 0.01, standard: 0.016, shooter: 0.025 };

/** Bulk density per material, kg/m³ (crates/parble-sim/src/marble.rs). */
const DENSITY_KG_M3: Readonly<Record<Material, number>> = { glass: 2_520, solid: 2_400, steel: 7_810 };

/** A marble's mass, kg: its material's density × the volume of its size. */
export function massKg(kind: MarbleKind, size: MarbleSize = "standard"): number {
  const r = SIZE_DIAMETER_M[size] / 2;
  return DENSITY_KG_M3[materialOf(kind)] * (4 / 3) * Math.PI * r ** 3;
}

/** Standard (16 mm) marble mass per material, kg. */
export const STANDARD_MASS_KG: Readonly<Record<Material, number>> = {
  glass: massKg("cats-eye"),
  solid: massKg("glazed"),
  steel: massKg("steel"),
};

/**
 * Pitch offset of a size, as a playback rate: (16 mm / diameter) ** SIZE_PITCH_EXPONENT. Minis and
 * shooters reuse their material's cues (cue sheet). A sphere's resonances scale with 1 / diameter
 * (exponent 1, about ±8 semitones); half that, about +4 semitones for a mini and −3.9 for a shooter,
 * keeps them the same material. Provisional until the P4-T6 listening review.
 */
export const SIZE_PITCH_EXPONENT = 0.5;

export function sizeRate(size: MarbleSize): number {
  return (SIZE_DIAMETER_M.standard / SIZE_DIAMETER_M[size]) ** SIZE_PITCH_EXPONENT;
}

/** Kit surface family of the cue ids; `marble` contacts use the clack cues instead. */
export function surfaceFamily(surface: Surface): "block" | "rail" | null {
  switch (surface) {
    case "wood":
    case "wood_end_grain":
      return "block";
    case "rail":
      return "rail";
    case "marble":
      return null;
  }
}

/** The roll loop cue for a marble type on a surface, or null when the surface has none. */
export function rollCueId(kind: MarbleKind, surface: Surface | null): string | null {
  const family = surface === null ? null : surfaceFamily(surface);
  return family === null ? null : `roll_${materialOf(kind)}_${family}`;
}

const clamp = (x: number, lo: number, hi: number): number => Math.min(hi, Math.max(lo, x));

/** Below this speed a roll is silent, m/s. */
export const ROLL_MIN_SPEED = 0.02;
/** Speed at full roll gain and the open filter, m/s. */
export const ROLL_FULL_SPEED = 1.5;
/** Speed the loops were recorded at (playback rate 1), m/s. */
export const ROLL_REF_SPEED = 0.5;
export const ROLL_RATE_RANGE = [0.7, 1.5] as const;
export const ROLL_CUTOFF_RANGE_HZ = [400, 16_000] as const;

export interface RollParams {
  /** Playback rate (pitch), with the size offset. */
  rate: number;
  /** Linear gain, 0..1. */
  gain: number;
  /** Low-pass cutoff, Hz. */
  cutoffHz: number;
}

/**
 * Roll loop pitch, gain and low-pass from speed (m/s) and marble size. Monotonic in speed, bounded,
 * silent at rest. The size offset multiplies the speed-clamped rate, so the clamp never absorbs it.
 */
export function rollParams(speed: number, size: MarbleSize = "standard"): RollParams {
  const s = Number.isFinite(speed) ? Math.max(0, speed) : 0;
  const t = clamp((s - ROLL_MIN_SPEED) / (ROLL_FULL_SPEED - ROLL_MIN_SPEED), 0, 1);
  const rate = clamp((Math.max(s, 0.01) / ROLL_REF_SPEED) ** 0.3, ROLL_RATE_RANGE[0], ROLL_RATE_RANGE[1]);
  const [lo, hi] = ROLL_CUTOFF_RANGE_HZ;
  const c = clamp(s / ROLL_FULL_SPEED, 0, 1);
  return { rate: rate * sizeRate(size), gain: t ** 0.6, cutoffHz: lo * (hi / lo) ** c };
}

export type Band = "light" | "medium" | "heavy";
/** Velocity change below which a contact start makes no sound, m/s. */
export const IMPACT_MIN_DV = 0.04;
/** Band upper edges, m/s: light below the first, medium below the second, heavy above. */
export const IMPACT_BANDS = [0.3, 0.9] as const;
const IMPACT_FULL_DV = 2;

/** The velocity change an impulse (N·s) gives a marble of `kind` and `size`, m/s. */
export function impactDeltaV(impulse: number, kind: MarbleKind, size: MarbleSize = "standard"): number {
  return Number.isFinite(impulse) ? Math.max(0, impulse) / massKg(kind, size) : 0;
}

export function impactBand(dv: number): Band | null {
  if (!(dv >= IMPACT_MIN_DV)) return null;
  if (dv < IMPACT_BANDS[0]) return "light";
  if (dv < IMPACT_BANDS[1]) return "medium";
  return "heavy";
}

/** One-shot gain from the velocity change: log-scaled 0.15..1 across IMPACT_MIN_DV..2 m/s. */
export function impactGain(dv: number): number {
  if (!(dv >= IMPACT_MIN_DV)) return 0;
  const g = clamp(Math.log(dv / IMPACT_MIN_DV) / Math.log(IMPACT_FULL_DV / IMPACT_MIN_DV), 0, 1);
  return 0.15 + 0.85 * g;
}

export interface ImpactChoice {
  cue: string;
  band: Band;
  gain: number;
  dv: number;
  /** The marble's size offset as a playback rate; the engine's pitch jitter multiplies it. */
  rate: number;
}

/** The impact cue for a coalesced contact, or null when it is too soft or has no cue. */
export function impactFor(c: AudioContact): ImpactChoice | null {
  const dv = impactDeltaV(c.impulse, c.kind, c.size);
  const band = impactBand(dv);
  if (band === null) return null;
  const a = materialOf(c.kind);
  let cue: string;
  if (c.surface === "marble") {
    if (c.otherKind === null) return null;
    const b = materialOf(c.otherKind);
    const [x, y] = MATERIAL_ORDER.indexOf(a) <= MATERIAL_ORDER.indexOf(b) ? [a, b] : [b, a];
    cue = `clack_${x}_${y}_${band}`;
  } else {
    cue = `impact_${a}_${surfaceFamily(c.surface)}_${band}`;
  }
  return { cue, band, gain: impactGain(dv), dv, rate: sizeRate(c.size) };
}

/** Playback rate for a random pitch jitter of up to ±`semitones`, from a uniform `u` in [0, 1). */
export function jitterRate(u: number, semitones: number): number {
  return 2 ** (((u * 2 - 1) * semitones) / 12);
}

/** PannerNode distance model, shared with voice priority so the loudest heard voice wins. */
export const PANNER = { refDistance: 0.3, rolloffFactor: 1, maxDistance: 40 } as const;

/** The PannerNode `inverse` distance gain at `d` metres. */
export function distanceGain(d: number): number {
  const { refDistance: ref, rolloffFactor: k, maxDistance: max } = PANNER;
  const x = clamp(d, ref, max);
  return ref / (ref + k * (x - ref));
}

export function distance(a: Vec3, b: Vec3): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

/** Kit cell pitch, m (docs/kit-contract.md), for placing mechanism sounds at their anchor cell. */
export const CELL_M = 0.06;

/**
 * The cue of each mechanism signal, by kit piece id and signal name (crates/parble-sim/src/behaviour.rs),
 * from the cue sheet's triggers. `drop_hole` takes the dropping marble's material. Sensor events and the
 * other signals (gate_open/close, finish_count, camera_cue, pass, forward/reverse, over, close) stay
 * silent, so each sound has one trigger.
 */
export const SIGNAL_CUES: ReadonlyMap<string, ReadonlyMap<string, string>> = new Map(
  Object.entries({
    mech_hopper: { release: "hopper_release" },
    mech_finish: { finish: "finish_cup_chime" },
    mech_checkpoint_l1: { split: "checkpoint_tick" },
    mech_splitter_l3: { set: "splitter_tick" },
    mech_toggle_l3: { set: "toggle_flap_clack" },
    mech_gate_oneway_l1: { open: "one_way_gate_swing" },
    mech_hole_oneway_l1: { open: "trapdoor_drop", drop: "drop_hole" },
  }).map(([piece, cues]) => [piece, new Map(Object.entries(cues))]),
);

/** The one-shot cue for `piece`'s signal `name`, or null when none is mapped. `material` is the marble's, for a drop. */
export function signalCueId(piece: string, name: string, material: Material = "glass"): string | null {
  const cue = SIGNAL_CUES.get(piece)?.get(name) ?? null;
  return cue === "drop_hole" ? `drop_hole_${material}` : cue;
}

/** Every cue id `signalCueId` can return, for staging and tests. */
export const SIGNAL_CUE_IDS: readonly string[] = [...SIGNAL_CUES.values()]
  .flatMap((m) => [...m.values()])
  .flatMap((c) => (c === "drop_hole" ? MATERIAL_ORDER.map((m) => `drop_hole_${m}`) : [c]))
  .sort();
