#!/usr/bin/env node
// P1-T8 perf gate (`make perf-gate`, local only, never CI): paced p95 frame time over 600 rAF frames
// after READY at 1440p High, on the `showcase` and `stress2000` scenes, in GPU Chromium (Playwright,
// `channel: "chromium"`, platform GPU flags) and in Safari (safaridriver's W3C WebDriver: real Safari,
// not Playwright WebKit). The release build in target/web-dist is served on the gate's own port
// (5313, reserved in ~/.claude/used_ports.md), never the 5311 preview other suites use.
//
// Load guard (house rule): the host's 1-minute load average must be ≤ 15 before and after every
// sample. The gate waits (bounded) for it before each sample, and discards and re-runs a sample whose
// post-run load exceeded 15. A sample that never gets a clean before/after is `taken: false`, and
// the gate fails. Every sample records its before/after load.
//
// Env:
//   PARBLE_PERF_SMOKE=1        non-gating smoke: no load wait, one attempt, writes
//                              target/perf/p1-gate-smoke.json (never docs/perf/p1-gate.json).
//   PARBLE_PERF_BROWSERS       comma list, default "chromium,safari".
//   PARBLE_PERF_SCENES         comma list, default "showcase,stress2000".
//   PARBLE_PERF_FRAMES         frames per sample, default 600.
//   PARBLE_PERF_SETTLE_FRAMES  frames skipped after READY before sampling, default 60.
//   PARBLE_PERF_HEADED=1       headed Chromium (default headless).
//   PARBLE_PERF_PORT           static server port, default 5313.
//   PARBLE_LOAD_WAIT_MS        bounded wait for the load before a sample, default 20 min.
//   PARBLE_PERF_ATTEMPTS       tries per sample when the post-run load exceeds 15, default 3.
//   PARBLE_PERF_OUT            output path override.
//   PARBLE_PERF_DIST           build to serve, default target/web-dist (a before/after comparison).
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { cpus, platform, release } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";
import gpu from "./gpu-args.cjs";
import { LOAD_MAX, diagSummary, load1, pageDiag, stats, waitForLoad } from "./perf-helpers.mjs";

// The criterion is p95 ≤ 16.7 ms. A rAF delta is a whole number of display refreshes plus the
// browser's timestamp quantum: Chromium coarsens rAF times to 0.1 ms (a frame on every 60 Hz vsync
// reads 16.6–16.8 ms), Safari to 1 ms (16 or 17 ms). A missed refresh at 60 Hz reads ≥ 25 ms. So
// the check adds the browser's jitter allowance: the 95th-percentile frame made its vsync. The raw
// p95 is recorded unrounded.
const JITTER_MS = { chromium: 0.3, safari: 0.3 };
const passMs = (browser) => 16.7 + JITTER_MS[browser];

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "..");
const SMOKE = process.env.PARBLE_PERF_SMOKE === "1";
const BROWSERS = (process.env.PARBLE_PERF_BROWSERS ?? "chromium,safari").split(",").filter(Boolean);
const SCENES = (process.env.PARBLE_PERF_SCENES ?? "showcase,stress2000").split(",").filter(Boolean);
const FRAMES = Number(process.env.PARBLE_PERF_FRAMES ?? 600);
const SETTLE = Number(process.env.PARBLE_PERF_SETTLE_FRAMES ?? 60);
const HEADED = process.env.PARBLE_PERF_HEADED === "1";
const PORT = Number(process.env.PARBLE_PERF_PORT ?? 5313);
const LOAD_WAIT_MS = Number(process.env.PARBLE_LOAD_WAIT_MS ?? 20 * 60_000);
const ATTEMPTS = SMOKE ? 1 : Number(process.env.PARBLE_PERF_ATTEMPTS ?? 3);
const OUT = resolve(ROOT, process.env.PARBLE_PERF_OUT ?? (SMOKE ? "target/perf/p1-gate-smoke.json" : "docs/perf/p1-gate.json"));
const WIDTH = 2560;
const HEIGHT = 1440;
const BASE = `http://127.0.0.1:${PORT}`;
const url = (scene) => `${BASE}/?mute=1&quality=high&scenario=${scene}`;

// Runs in the page: waits for READY, skips `settle` frames, then calls `done` with `frames` rAF
// deltas and the page state. ES5 so Safari's execute/async takes its source as is. `diag` is
// `pageDiag` (perf-helpers.mjs), passed in as source: on a READY timeout its result is returned.
function sampler(frames, settle, done, diag) {
  var t0 = performance.now();
  function waitReady() {
    var p = window.__parble;
    if (!(p && p.ready)) {
      if (performance.now() - t0 > 180000) return done({ error: "READY never came", diag: diag() });
      return setTimeout(waitReady, 100);
    }
    var readyMs = performance.now() - t0, n = 0, last = 0, out = [];
    function loop(t) {
      if (n > settle) out.push(t - last);
      last = t;
      n++;
      if (out.length < frames) return requestAnimationFrame(loop);
      var c = document.querySelector("canvas#parble"), s = window.__parble;
      done({
        deltas: out,
        readyMs: readyMs,
        canvas: c ? [c.width, c.height] : null,
        dpr: window.devicePixelRatio,
        inner: [window.innerWidth, window.innerHeight],
        userAgent: navigator.userAgent,
        state: {
          scenario: s.scenario, quality: s.quality, tick: s.tick, fps: s.fps,
          marbles: s.marbles ? s.marbles.length : null,
          kitPieces: s.kit ? s.kit.pieces : null, kitChunks: s.kit ? s.kit.chunks : null,
          errors: window.__parbleErrors || null,
        },
        warmup: s.warmup || null,
        pipelines: s.pipelines || null,
      });
    }
    requestAnimationFrame(loop);
  }
  waitReady();
}
const SAMPLER = `(${sampler.toString()})(arguments[0], arguments[1], arguments[arguments.length - 1], ${pageDiag.toString()});`;

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

// ---- Chromium (Playwright, GPU flags) ----

async function chromiumSample(scene) {
  const browser = await chromium.launch({
    channel: "chromium",
    headless: !HEADED,
    args: [...gpu.GPU_ARGS, "--mute-audio", ...(HEADED ? ["--force-device-scale-factor=1"] : [])],
  });
  try {
    const page = await browser.newPage({ viewport: { width: WIDTH, height: HEIGHT }, deviceScaleFactor: 1 });
    const errors = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
    await page.goto(url(scene));
    const probe = await gpu.probeGpu(page);
    const problems = gpu.gpuProblems(probe);
    if (problems.length) throw new Error(`GPU check failed: ${problems.join("; ")}`);
    // A string expression goes through CDP's evaluate, which the page's CSP (no unsafe-eval) allows.
    const r = await page.evaluate(`new Promise((done) => (${sampler.toString()})(${FRAMES}, ${SETTLE}, done, ${pageDiag.toString()}))`);
    return { ...r, adapter: probe.adapter, renderer: probe.renderer, mode: HEADED ? "headed" : "headless", errors };
  } finally {
    await browser.close();
  }
}

// ---- Safari (safaridriver W3C WebDriver) ----

async function wd(base, method, p, body) {
  const r = await fetch(base + p, {
    method,
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(400_000),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(`${method} ${p}: ${JSON.stringify(j.value)}`);
  return j.value;
}

async function safariSample(scene) {
  const port = 20000 + Math.floor(Math.random() * 20000);
  const base = `http://127.0.0.1:${port}`;
  const driver = spawn("safaridriver", ["-p", String(port)], { stdio: "ignore" });
  let session = null;
  try {
    await new Promise((r) => setTimeout(r, 1500));
    session = (await wd(base, "POST", "/session", { capabilities: { alwaysMatch: { browserName: "safari" } } })).sessionId;
    const s = `/session/${session}`;
    await wd(base, "POST", `${s}/timeouts`, { script: 300_000, pageLoad: 120_000 });
    // Size the window so the canvas is 2560 × 1440 device pixels: on a Retina screen (dpr 2) that
    // is a 1280 × 720 CSS viewport. Safari has no device-scale override.
    await wd(base, "POST", `${s}/url`, { url: `${BASE}/__size-probe` });
    const m = await wd(base, "POST", `${s}/execute/sync`, {
      args: [],
      script: "return [window.devicePixelRatio, window.outerWidth - window.innerWidth, window.outerHeight - window.innerHeight];",
    });
    const [dpr, dx, dy] = m;
    await wd(base, "POST", `${s}/window/rect`, { x: 0, y: 0, width: Math.round(WIDTH / dpr) + dx, height: Math.round(HEIGHT / dpr) + dy });
    await wd(base, "POST", `${s}/url`, { url: url(scene) });
    const { adapter } = await wd(base, "POST", `${s}/execute/async`, { args: [], script: ADAPTER });
    const r = await wd(base, "POST", `${s}/execute/async`, { args: [FRAMES, SETTLE], script: SAMPLER });
    return { ...r, adapter, renderer: null, mode: "headed (Safari)", errors: r.state?.errors ?? [] };
  } finally {
    if (session) await wd(base, "DELETE", `/session/${session}`).catch(() => {});
    driver.kill();
  }
}

// ---- gate ----

async function sample(browser, scene) {
  const discarded = [];
  for (let attempt = 1; attempt <= ATTEMPTS; attempt++) {
    const loadBefore = SMOKE ? load1() : await waitForLoad(LOAD_WAIT_MS);
    if (loadBefore > LOAD_MAX && !SMOKE) {
      return { browser, scene, taken: false, reason: `1-min load ${loadBefore.toFixed(2)} > ${LOAD_MAX} after the ${LOAD_WAIT_MS / 60000} min wait`, loadBefore, discarded };
    }
    const r = browser === "safari" ? await safariSample(scene) : await chromiumSample(scene);
    const loadAfter = load1();
    if (r.error) {
      // Keep the loading state READY waited on (warm-up, pipelines, textures, page log) in the JSON.
      console.error(`${browser} ${scene}: ${r.error}\n  ${diagSummary(r.diag)}`);
      return { browser, scene, taken: false, error: r.error, diag: r.diag, adapter: r.adapter, mode: r.mode, errors: r.errors, discarded };
    }
    const st = stats(r.deltas);
    const res = {
      browser, scene, attempt,
      taken: loadBefore <= LOAD_MAX && loadAfter <= LOAD_MAX,
      loadBefore: +loadBefore.toFixed(2), loadAfter: +loadAfter.toFixed(2),
      mode: r.mode, adapter: r.adapter, renderer: r.renderer, userAgent: r.userAgent,
      canvas: r.canvas, dpr: r.dpr, inner: r.inner, readyMs: Math.round(r.readyMs),
      state: r.state, errors: r.errors,
      warmup: r.warmup, pipelines: r.pipelines,
      frameTimes: st,
      longFrames: r.deltas.flatMap((d, i) => (d > 50 ? [[i, +d.toFixed(1)]] : [])),
      passP95Ms: passMs(browser),
      pass: st.p95Ms <= passMs(browser),
      // The criterion read literally: p95 rounded to 0.1 ms at most 16.7 ms, no jitter allowance.
      passStrict: Math.round(st.p95Ms * 10) / 10 <= 16.7,
    };
    console.log(`${browser} ${scene}: p95 ${st.p95Ms.toFixed(2)} ms [pass ${res.pass}, strict ${res.passStrict}] (p50 ${st.p50Ms.toFixed(2)}, max ${st.maxMs.toFixed(1)}, dropped ${st.dropped}/${st.frames}) canvas ${r.canvas} load ${res.loadBefore} -> ${res.loadAfter}${res.taken ? "" : " NOT TAKEN"}`);
    // A warm-up ended by the ceiling or with stuck pipelines may still hitch mid-run (a late pipeline).
    const w = r.warmup ?? {};
    console.log(`  READY ${res.readyMs} ms, warm-up endedBy ${w.endedBy} on render frame ${w.doneFrame} (waitingOn ${w.waitingOn}, stuck ${w.stuck?.length ?? "?"})`);
    if (res.taken || SMOKE) return { ...res, discarded };
    discarded.push({ attempt, loadBefore: res.loadBefore, loadAfter: res.loadAfter, p95Ms: st.p95Ms });
    console.log(`  discarded: post-run load ${res.loadAfter} > ${LOAD_MAX}, re-running`);
  }
  return { browser, scene, taken: false, reason: `post-run load > ${LOAD_MAX} on all ${ATTEMPTS} attempts`, discarded };
}

async function main() {
  const srv = await serve();
  const samples = [];
  const problems = [];
  try {
    for (const browser of BROWSERS) {
      for (const scene of SCENES) {
        let s;
        try {
          s = await sample(browser, scene);
        } catch (e) {
          s = { browser, scene, taken: false, error: String(e?.message ?? e) };
        }
        samples.push(s);
        if (s.error) problems.push(`${browser} ${scene}: ${s.error}`);
        if (s.canvas && (s.canvas[0] !== WIDTH || s.canvas[1] !== HEIGHT)) {
          problems.push(`${browser} ${scene}: canvas ${s.canvas.join("x")} is not ${WIDTH}x${HEIGHT}`);
        }
        if (s.state && s.state.quality !== "high") problems.push(`${browser} ${scene}: quality ${s.state.quality}`);
        if (s.errors?.length) problems.push(`${browser} ${scene}: page errors: ${s.errors.slice(0, 3).join(" | ")}`);
      }
    }
  } finally {
    srv.kill();
  }
  const allTaken = samples.every((s) => s.taken);
  const result = {
    gate: "P1-T8 perf gate",
    gating: !SMOKE,
    note: SMOKE ? "SMOKE RUN: no load wait, not a gate result" : undefined,
    budgetMs: 16.7,
    jitterMs: JITTER_MS,
    jitterRule: "pass when the p95 rAF delta <= 16.7 ms + the browser's rAF timestamp quantum (jitterMs): the p95 frame made its 60 Hz vsync",
    loadMax: LOAD_MAX,
    resolution: [WIDTH, HEIGHT],
    quality: "high",
    frames: FRAMES,
    settleFrames: SETTLE,
    host: { platform: platform(), release: release(), cpu: cpus()[0]?.model, cores: cpus().length },
    when: new Date().toISOString(),
    samples,
    problems,
    allTaken,
    pass: !SMOKE && allTaken && problems.length === 0 && samples.every((s) => s.pass),
    passStrict: !SMOKE && allTaken && problems.length === 0 && samples.every((s) => s.passStrict),
  };
  mkdirSync(dirname(OUT), { recursive: true });
  writeFileSync(OUT, `${JSON.stringify(result, null, 2)}\n`);
  console.log(`wrote ${OUT}`);
  for (const p of problems) console.error("PROBLEM:", p);
  if (SMOKE) {
    console.log("perf-gate SMOKE (non-gating):", problems.length ? "problems above" : "ran clean");
    process.exit(problems.length ? 1 : 0);
  }
  console.log("perf-gate:", result.pass ? "PASS" : allTaken ? "FAIL" : problems.length ? "NOT TAKEN (problems above)" : "NOT TAKEN (load guard)");
  process.exit(result.pass ? 0 : 1);
}

main().catch((e) => {
  console.error("perf-gate:", e?.stack ?? e);
  process.exit(1);
});
