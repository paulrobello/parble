// Pure mappings from the audio stream (bridge.ts AudioFrame) to cue ids and voice parameters. Cue ids
// are the approved P4-T2 ids (docs/audio/cue-sheet.md), staged by scripts/web/stage-audio.mjs.

import type { AudioContact, MarbleKind, Surface, Vec3 } from "../bridge";

/** Sound material of a marble type. Swirl, onionskin, opal and glow reuse the glass cues (cue sheet). */
export type Material = "glass" | "solid" | "steel";
const MATERIAL_ORDER: readonly Material[] = ["glass", "solid", "steel"];

export function materialOf(kind: MarbleKind): Material {
  if (kind === "steel") return "steel";
  if (kind === "glazed") return "solid";
  return "glass";
}

/**
 * Standard (16 mm) marble mass per material, kg: density (crates/parble-sim/src/marble.rs) × volume.
 * The wire carries no marble size, so minis and shooters are normalised by this mass too.
 */
const STANDARD_VOLUME_M3 = (4 / 3) * Math.PI * 0.008 ** 3;
export const STANDARD_MASS_KG: Readonly<Record<Material, number>> = {
  glass: 2_520 * STANDARD_VOLUME_M3,
  solid: 2_400 * STANDARD_VOLUME_M3,
  steel: 7_810 * STANDARD_VOLUME_M3,
};

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
  /** Playback rate (pitch). */
  rate: number;
  /** Linear gain, 0..1. */
  gain: number;
  /** Low-pass cutoff, Hz. */
  cutoffHz: number;
}

/** Roll loop pitch, gain and low-pass from speed (m/s). Monotonic in speed, bounded, silent at rest. */
export function rollParams(speed: number): RollParams {
  const s = Number.isFinite(speed) ? Math.max(0, speed) : 0;
  const t = clamp((s - ROLL_MIN_SPEED) / (ROLL_FULL_SPEED - ROLL_MIN_SPEED), 0, 1);
  const rate = clamp((Math.max(s, 0.01) / ROLL_REF_SPEED) ** 0.3, ROLL_RATE_RANGE[0], ROLL_RATE_RANGE[1]);
  const [lo, hi] = ROLL_CUTOFF_RANGE_HZ;
  const c = clamp(s / ROLL_FULL_SPEED, 0, 1);
  return { rate, gain: t ** 0.6, cutoffHz: lo * (hi / lo) ** c };
}

export type Band = "light" | "medium" | "heavy";
/** Velocity change below which a contact start makes no sound, m/s. */
export const IMPACT_MIN_DV = 0.04;
/** Band upper edges, m/s: light below the first, medium below the second, heavy above. */
export const IMPACT_BANDS = [0.3, 0.9] as const;
const IMPACT_FULL_DV = 2;

/** The velocity change an impulse (N·s) gives a standard marble of `kind`, m/s. */
export function impactDeltaV(impulse: number, kind: MarbleKind): number {
  return Number.isFinite(impulse) ? Math.max(0, impulse) / STANDARD_MASS_KG[materialOf(kind)] : 0;
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
}

/** The impact cue for a coalesced contact, or null when it is too soft or has no cue. */
export function impactFor(c: AudioContact): ImpactChoice | null {
  const dv = impactDeltaV(c.impulse, c.kind);
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
  return { cue, band, gain: impactGain(dv), dv };
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

/** The one-shot cue for a mechanism signal, or null when none is mapped. */
export function signalCueId(name: string): string | null {
  return name === "swing" ? "one_way_gate_swing" : null;
}
