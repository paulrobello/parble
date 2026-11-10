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

/// Runs in the page (ES5, passed to it as source): the loading state READY waits for, from
/// `window.__parble` and the page-side error log. `warmup.waitingOn` names what held the warm-up on
/// its last frame (`textures`, `pending`, `recentCreation`, `noPipelines`), `pipelines.waiting`
/// lists the pipelines not created yet with their shader error and the frame they started waiting,
/// `texturesNotLoaded` the kit textures that are not `loaded`. Safari's WebDriver has no console or
/// log endpoint, so `errors` (index.html's `window.__parbleErrors`: console.error, Bevy ERROR lines,
/// uncaught errors, CSP blocks) is all of the console it can return.
export function pageDiag() {
  var s = window.__parble;
  var errors = window.__parbleErrors || null;
  if (!s) return { parble: null, href: location.href, errors: errors };
  var tex = s.kit && s.kit.textures ? s.kit.textures : [];
  return {
    ready: s.ready, tick: s.tick, run: s.run, frame: s.frame, fps: s.fps,
    scenario: s.scenario, quality: s.quality,
    warmup: s.warmup || null,
    pipelines: s.pipelines || null,
    kit: s.kit ? { family: s.kit.family, deviceBc: s.kit.deviceBc, deviceAstc: s.kit.deviceAstc, textures: tex.length } : null,
    texturesNotLoaded: tex.filter(function (t) { return t.state !== "loaded"; }),
    errors: errors,
  };
}

/// One line on what held the warm-up, from a `pageDiag()` result.
export function diagSummary(d) {
  if (!d) return "no diag";
  if (d.parble === null) return `window.__parble missing at ${d.href}`;
  const w = d.warmup ?? {};
  const p = d.pipelines ?? {};
  const waiting = (p.waiting ?? []).map((x) => `${x.name} ${x.state}${x.error ? ` (${x.error})` : ""}${x.stuck ? " STUCK" : ""}`);
  return [
    `ready ${d.ready} tick ${d.tick} frame ${d.frame}`,
    `warmup done ${w.done} endedBy ${w.endedBy} waitingOn ${w.waitingOn} texturesLoading ${w.texturesLoading}`,
    `pipelines renderFrame ${p.renderFrame} total ${p.total} ok ${p.ok} pending ${p.pending} stuck ${p.stuck} failed ${p.failed} lastCreated ${p.lastCreatedFrame} createdTotal ${p.createdTotal}`,
    ...(waiting.length ? [`waiting: ${waiting.join(" | ")}`] : []),
    ...((p.failures ?? []).length ? [`failures: ${p.failures.join(" | ")}`] : []),
    ...(d.texturesNotLoaded?.length ? [`textures not loaded: ${d.texturesNotLoaded.map((t) => `${t.path} ${t.state}${t.error ? ` (${t.error})` : ""}`).join(" | ")}`] : []),
    ...(d.errors?.length ? [`page errors: ${d.errors.slice(0, 5).join(" | ")}`] : []),
  ].join("\n  ");
}
