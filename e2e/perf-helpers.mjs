// Shared by the opt-in perf runs (tests/editor-perf.spec.mjs, perf-gate.mjs): the host-load guard
// and frame-time statistics.
import { loadavg } from "node:os";

export const BUDGET_MS = 1000 / 60;
/// Most 1-minute load average a measurement may start or end at (house load rule).
export const LOAD_MAX = 15;

/// Waits, polling every 15 s, until the 1-minute load is ≤ LOAD_MAX or `waitMs` has passed, and
/// returns the load it ended at (still above LOAD_MAX when the wait ran out).
export async function waitForLoad(waitMs) {
  const start = Date.now();
  while (loadavg()[0] > LOAD_MAX && Date.now() - start < waitMs) {
    await new Promise((r) => setTimeout(r, 15_000));
  }
  return loadavg()[0];
}

export function load1() {
  return loadavg()[0];
}

/// Frame-time statistics of rAF deltas in ms.
export function stats(ms) {
  const s = [...ms].sort((a, b) => a - b);
  const q = (p) => s[Math.min(s.length - 1, Math.floor(p * s.length))];
  return {
    frames: s.length,
    meanMs: s.reduce((a, b) => a + b, 0) / s.length,
    p50Ms: q(0.5),
    p95Ms: q(0.95),
    p99Ms: q(0.99),
    maxMs: s[s.length - 1],
    // Frames longer than 1.5 budgets: a missed vsync (16.8 ms is rAF jitter at 60 Hz, not a drop).
    dropped: s.filter((v) => v > 1.5 * BUDGET_MS).length,
  };
}
