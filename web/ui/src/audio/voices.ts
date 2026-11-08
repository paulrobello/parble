// Voice budget and priority: pure, so Vitest covers the allocation without an AudioContext.

/** Mulberry32: a small seeded PRNG, so variant picks and pitch jitter repeat under test. */
export function seededRng(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4_294_967_296;
  };
}

export interface VoiceCaps {
  /** All voices sounding at once: roll loops plus one-shots. */
  total: number;
  /** Roll loops, at most this many of `total`. */
  rolls: number;
}

export const DEFAULT_CAPS: VoiceCaps = { total: 24, rolls: 10 };

export interface Ranked {
  id: number;
  priority: number;
}

/** The `n` highest-priority candidates, ties broken by lower id, so the choice is stable frame to frame. */
export function selectTop<T extends Ranked>(candidates: readonly T[], n: number): T[] {
  return [...candidates].sort((a, b) => b.priority - a.priority || a.id - b.id).slice(0, Math.max(0, n));
}

export type Admission = { admit: true; evict: number | null } | { admit: false };

/**
 * Whether a new voice of `priority` gets one of `cap` slots held by `active`: a free slot, else the
 * quietest active voice's slot when the new one is strictly louder (it is stolen), else it is dropped.
 */
export function admit(active: readonly Ranked[], cap: number, priority: number): Admission {
  if (cap <= 0) return { admit: false };
  if (active.length < cap) return { admit: true, evict: null };
  let low: Ranked | null = null;
  for (const v of active) if (low === null || v.priority < low.priority) low = v;
  return low !== null && priority > low.priority ? { admit: true, evict: low.id } : { admit: false };
}

/** A one-shot's priority decays linearly to 0 over its length, so a fading tail is stolen first. */
export function oneShotPriority(base: number, start: number, duration: number, now: number): number {
  if (duration <= 0) return 0;
  return base * Math.min(1, Math.max(0, 1 - (now - start) / duration));
}
