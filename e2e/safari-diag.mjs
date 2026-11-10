#!/usr/bin/env node
// Loading diag (local only, never CI): loads the release build in real Safari through safaridriver's
// W3C WebDriver, polls `window.__parble` once a second, and writes the loading state READY waits on
// (warm-up, pipeline cache, kit textures, the page-side error log) as a time series to JSON, with a
// WebDriver screenshot per scene. It answers why READY did or did not come, where perf-gate.mjs
// only times frames after it. Safari's WebDriver has no console or log endpoint, so the page log
// (window.__parbleErrors) is all of the console this can read.
//
// Needs Safari's "Allow Remote Automation" and a GUI session (see docs/perf/README.md, gpu-host-b):
// a safaridriver started from an ssh shell cannot reach Safari. Keep Safari frontmost: a background
// window's rAF is throttled.
//
// Env:
//   PARBLE_DIAG_SCENES   comma list, default "showcase,stress2000".
//   PARBLE_DIAG_QUERY    extra URL query, default "quality=high&mute=1".
//   PARBLE_DIAG_SECONDS  poll duration per scene, default 60.
//   PARBLE_DIAG_BROWSER  "safari" (default) or "chromium" (GPU Chromium via Playwright, to check
//                        the script itself on a host without Safari automation).
//   PARBLE_DIAG_OUT      output path, default target/perf/safari-diag.json (screenshots next to it).
//   PARBLE_PERF_PORT     static server port, default 5313 (the perf gate's).
//   PARBLE_PERF_DIST     build to serve, default target/web-dist.
// Exit 0 when every scene reached READY, 1 otherwise.
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { cpus, platform, release } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { diagSummary, load1, pageDiag } from "./perf-helpers.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "..");
const SCENES = (process.env.PARBLE_DIAG_SCENES ?? "showcase,stress2000").split(",").filter(Boolean);
const QUERY = process.env.PARBLE_DIAG_QUERY ?? "quality=high&mute=1";
const SECONDS = Number(process.env.PARBLE_DIAG_SECONDS ?? 60);
const BROWSER = process.env.PARBLE_DIAG_BROWSER ?? "safari";
const OUT = resolve(ROOT, process.env.PARBLE_DIAG_OUT ?? (BROWSER === "safari" ? "target/perf/safari-diag.json" : `target/perf/${BROWSER}-diag.json`));
const PORT = Number(process.env.PARBLE_PERF_PORT ?? 5313);
const BASE = `http://127.0.0.1:${PORT}`;
const WIDTH = 2560;
const HEIGHT = 1440;
const DIAG = `return (${pageDiag.toString()})();`;
const ADAPTER = `
  var done = arguments[arguments.length - 1];
  if (!navigator.gpu) return done({ adapter: null });
  navigator.gpu.requestAdapter().then(function (a) {
    var i = a && a.info;
    done({ adapter: i ? (i.vendor + " " + i.architecture + " " + i.description).trim() : (a ? "adapter (no info)" : null) });
  }, function (e) { done({ adapter: null, error: String(e) }); });
`;

function serve() {
  const srv = spawn("python3", [resolve(HERE, "static-server.py"), String(PORT), resolve(ROOT, process.env.PARBLE_PERF_DIST ?? "target/web-dist")], {
    stdio: ["ignore", "pipe", "ignore"],
  });
  return new Promise((ok, fail) => {
    const t = setTimeout(() => fail(new Error(`static server on ${PORT} did not start`)), 15_000);
    srv.stdout.on("data", (d) => {
      if (String(d).includes("serving")) {
        clearTimeout(t);
        ok(srv);
      }
    });
    srv.on("exit", (code) => fail(new Error(`static server exited (${code}); is port ${PORT} free?`)));
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

// A browser behind one small interface: goto, run a sync script (WebDriver's execute/sync body),
// run an async one, screenshot, close.
async function openSafari() {
  const port = 20000 + Math.floor(Math.random() * 20000);
  const base = `http://127.0.0.1:${port}`;
  const driver = spawn("safaridriver", ["-p", String(port)], { stdio: "ignore" });
  await new Promise((r) => setTimeout(r, 1500));
  let session;
  try {
    session = (await wd(base, "POST", "/session", { capabilities: { alwaysMatch: { browserName: "safari" } } })).sessionId;
  } catch (e) {
    driver.kill();
    throw e;
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
    consoleErrors: () => null,
  };
  // Size the window so the canvas is 2560 × 1440 device pixels (perf-gate.mjs does the same).
  await b.goto(`${BASE}/__size-probe`);
  const [dpr, dx, dy] = await b.sync("return [window.devicePixelRatio, window.outerWidth - window.innerWidth, window.outerHeight - window.innerHeight];");
  await wd(base, "POST", `${s}/window/rect`, { x: 0, y: 0, width: Math.round(WIDTH / dpr) + dx, height: Math.round(HEIGHT / dpr) + dy });
  return b;
}

async function openChromium() {
  const { chromium } = await import("@playwright/test");
  const gpu = (await import("./gpu-args.cjs")).default;
  const browser = await chromium.launch({ channel: "chromium", headless: true, args: [...gpu.GPU_ARGS, "--mute-audio"] });
  const page = await browser.newPage({ viewport: { width: WIDTH, height: HEIGHT }, deviceScaleFactor: 1 });
  let errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => { if (m.type() === "error" || m.type() === "warning") errors.push(`${m.type()}: ${m.text()}`.slice(0, 500)); });
  // Scripts go through CDP's evaluate as strings (the page's CSP has no unsafe-eval).
  const fn = (script) => `(function () { ${script} })()`;
  return {
    goto: async (url) => {
      errors = [];
      await page.goto(url);
    },
    sync: (script) => page.evaluate(fn(script)),
    async: (script) => page.evaluate(`new Promise(function (done) { (function () { ${script} }).call(null, done); })`),
    shot: () => page.screenshot(),
    close: () => browser.close(),
    consoleErrors: () => errors.slice(0, 50),
  };
}

async function diagScene(b, scene) {
  const url = `${BASE}/?scenario=${scene}&${QUERY}`;
  const t0 = Date.now();
  await b.goto(url);
  const { adapter } = await b.async(ADAPTER).catch((e) => ({ adapter: null, error: String(e) }));
  const series = [];
  let readyMs = null;
  let last = null;
  while (Date.now() - t0 < SECONDS * 1000) {
    let d;
    try {
      d = await b.sync(DIAG);
    } catch (e) {
      d = { scriptError: String(e?.message ?? e) };
    }
    const t = Date.now() - t0;
    if (d.ready && readyMs === null) readyMs = t;
    last = d;
    // Compact per-poll entry; the full state (with recentCreated) is kept for the first READY poll
    // and the last poll.
    const p = d.pipelines ?? {};
    series.push({
      t, ready: d.ready, tick: d.tick, frame: d.frame, fps: d.fps,
      warmup: d.warmup,
      pipelines: p.total === undefined ? null : {
        renderFrame: p.renderFrame, total: p.total, ok: p.ok, pending: p.pending, stuck: p.stuck, failed: p.failed,
        lastCreatedFrame: p.lastCreatedFrame, createdTotal: p.createdTotal, waiting: p.waiting, failures: p.failures,
      },
      texturesNotLoaded: d.texturesNotLoaded?.map((x) => `${x.path} ${x.state}`),
      errors: d.errors?.length ?? null,
      scriptError: d.scriptError,
    });
    if (readyMs === t) console.log(`  ${scene}: READY at ${t} ms`);
    await new Promise((r) => setTimeout(r, 1000));
  }
  const png = OUT.replace(/\.json$/, `-${scene}.png`);
  await b.shot().then((buf) => writeFileSync(png, buf)).catch(() => {});
  console.log(`${scene}: ${readyMs === null ? `READY never came in ${SECONDS} s` : `READY at ${readyMs} ms`}\n  ${diagSummary(last)}`);
  return { scene, url, adapter, ready: readyMs !== null, readyMs, last, consoleErrors: b.consoleErrors(), screenshot: png, series };
}

async function main() {
  mkdirSync(dirname(OUT), { recursive: true });
  const srv = await serve();
  const scenes = [];
  try {
    for (const scene of SCENES) {
      // A fresh browser session per scene: a cold load, as the perf gate does.
      const b = BROWSER === "safari" ? await openSafari() : await openChromium();
      try {
        scenes.push(await diagScene(b, scene));
      } catch (e) {
        scenes.push({ scene, ready: false, error: String(e?.message ?? e) });
        console.error(`${scene}: ${e?.message ?? e}`);
      } finally {
        await b.close();
      }
    }
  } finally {
    srv.kill();
  }
  const result = {
    diag: "loading diag (READY, warm-up, pipelines, textures)",
    browser: BROWSER,
    query: QUERY,
    seconds: SECONDS,
    host: { platform: platform(), release: release(), cpu: cpus()[0]?.model, cores: cpus().length, load1: +load1().toFixed(2) },
    when: new Date().toISOString(),
    scenes,
  };
  writeFileSync(OUT, `${JSON.stringify(result, null, 2)}\n`);
  console.log(`wrote ${OUT}`);
  process.exit(scenes.every((s) => s.ready) ? 0 : 1);
}

main().catch((e) => {
  console.error("safari-diag:", e?.stack ?? e);
  process.exit(1);
});
