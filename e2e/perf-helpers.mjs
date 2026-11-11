// Shared by the opt-in perf runs (tests/editor-perf.spec.mjs, perf-gate.mjs, safari-diag.mjs,
// safari-repeat.mjs): the host-load guard, frame-time statistics, the static server and Safari sessions.
import { spawn } from "node:child_process";
import { loadavg } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));

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
  var fatal = (s && s.fatal) || window.__parbleFatal || null;
  if (!s) return { parble: null, href: location.href, errors: errors, fatal: fatal };
  var tex = s.kit && s.kit.textures ? s.kit.textures : [];
  return {
    ready: s.ready, tick: s.tick, run: s.run, frame: s.frame, fps: s.fps,
    scenario: s.scenario, quality: s.quality,
    warmup: s.warmup || null,
    pipelines: s.pipelines || null,
    kit: s.kit ? { family: s.kit.family, deviceBc: s.kit.deviceBc, deviceAstc: s.kit.deviceAstc, textures: tex.length } : null,
    texturesNotLoaded: tex.filter(function (t) { return t.state !== "loaded"; }),
    errors: errors,
    fatal: fatal,
  };
}

/// One line on what held the warm-up, from a `pageDiag()` result.
export function diagSummary(d) {
  if (!d) return "no diag";
  if (d.parble === null) return `window.__parble missing at ${d.href}${d.fatal ? `; FATAL page error: ${d.fatal.message}` : ""}`;
  const w = d.warmup ?? {};
  const p = d.pipelines ?? {};
  const waiting = (p.waiting ?? []).map((x) => `${x.name} ${x.state}${x.error ? ` (${x.error})` : ""}${x.stuck ? " STUCK" : ""}`);
  return [
    ...(d.fatal ? [`FATAL page error at ${d.fatal.atMs} ms: ${d.fatal.message}`] : []),
    `ready ${d.ready} tick ${d.tick} frame ${d.frame}`,
    `warmup done ${w.done} endedBy ${w.endedBy} waitingOn ${w.waitingOn} texturesLoading ${w.texturesLoading}${w.texturesFailed?.length ? ` texturesFailed ${w.texturesFailed.join(" | ")}` : ""}`,
    `pipelines renderFrame ${p.renderFrame} total ${p.total} ok ${p.ok} pending ${p.pending} stuck ${p.stuck} failed ${p.failed} lastCreated ${p.lastCreatedFrame} createdTotal ${p.createdTotal}`,
    ...(waiting.length ? [`waiting: ${waiting.join(" | ")}`] : []),
    ...((p.failures ?? []).length ? [`failures: ${p.failures.join(" | ")}`] : []),
    ...(d.texturesNotLoaded?.length ? [`textures not loaded: ${d.texturesNotLoaded.map((t) => `${t.path} ${t.state}${t.error ? ` (${t.error})` : ""}`).join(" | ")}`] : []),
    ...(d.errors?.length ? [`page errors: ${d.errors.slice(0, 5).join(" | ")}`] : []),
  ].join("\n  ");
}

/// Starts e2e/static-server.py on `port` serving `dist`, resolving with the child once it listens.
export function serveDist(port, dist) {
  const srv = spawn("python3", [resolve(HERE, "static-server.py"), String(port), dist], {
    stdio: ["ignore", "pipe", "ignore"],
  });
  return new Promise((ok, fail) => {
    const t = setTimeout(() => fail(new Error(`static server on ${port} did not start`)), 15_000);
    srv.stdout.on("data", (d) => {
      if (String(d).includes("serving")) {
        clearTimeout(t);
        ok(srv);
      }
    });
    srv.on("exit", (code) => fail(new Error(`static server exited (${code}); is port ${port} free?`)));
  });
}

async function wd(base, method, p, body) {
  const r = await fetch(base + p, {
    method,
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(120_000),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(`${method} ${p}: ${JSON.stringify(j.value)}`);
  return j.value;
}

/// A fresh Safari session through its own safaridriver (W3C WebDriver), its window sized so the
/// canvas is `width` × `height` device pixels (measured on `${site}/__size-probe`). Returns
/// `{ goto, sync, async, shot, close }`; `sync` and `async` run a WebDriver execute script body.
/// Needs Safari's "Allow Remote Automation" and a GUI session (docs/perf/README.md).
export async function openSafari(site, width, height) {
  const port = 20000 + Math.floor(Math.random() * 20000);
  const base = `http://127.0.0.1:${port}`;
  const driver = spawn("safaridriver", ["-p", String(port)], { stdio: "ignore" });
  await new Promise((r) => setTimeout(r, 1500));
  let session = null;
  try {
    // A second back-to-back session has failed once with `fetch failed`: retry once after a pause.
    for (let attempt = 1; !session; attempt++) {
      try {
        session = (await wd(base, "POST", "/session", { capabilities: { alwaysMatch: { browserName: "safari" } } })).sessionId;
      } catch (e) {
        if (attempt >= 2) throw e;
        console.error(`safari session: ${e?.message ?? e}; retrying in 5 s`);
        await new Promise((r) => setTimeout(r, 5000));
      }
    }
    const s = `/session/${session}`;
    await wd(base, "POST", `${s}/timeouts`, { script: 60_000, pageLoad: 120_000 });
    const b = {
      goto: (url) => wd(base, "POST", `${s}/url`, { url }),
      sync: (script) => wd(base, "POST", `${s}/execute/sync`, { args: [], script }),
      async: (script) => wd(base, "POST", `${s}/execute/async`, { args: [], script }),
      shot: async () => Buffer.from(await wd(base, "GET", `${s}/screenshot`), "base64"),
      close: async () => {
        await wd(base, "DELETE", s).catch(() => {});
        driver.kill();
      },
    };
    await b.goto(`${site}/__size-probe`);
    const [dpr, dx, dy] = await b.sync("return [window.devicePixelRatio, window.outerWidth - window.innerWidth, window.outerHeight - window.innerHeight];");
    await wd(base, "POST", `${s}/window/rect`, { x: 0, y: 0, width: Math.round(width / dpr) + dx, height: Math.round(height / dpr) + dy });
    return b;
  } catch (e) {
    if (session) await wd(base, "DELETE", `/session/${session}`).catch(() => {});
    driver.kill();
    throw e;
  }
}
