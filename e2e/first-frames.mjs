#!/usr/bin/env node
// Cold-load frame timeline (local only, never CI): loads each scene in a fresh GPU Chromium (new
// profile, so the HTTP and GPU shader caches are cold), records every rAF delta from the first frame
// to READY + `PARBLE_FF_AFTER` frames, with `window.__parble`'s tick, marble count and pipeline cache
// counts on each frame, and the page console with timestamps. Every frame over 50 ms is listed with
// what changed on it. `PARBLE_FF_TRACE=1` also writes a Chrome trace per scene.
//
// Load guard (house rule): waits (bounded) for a 1-minute load ≤ 15 before each scene and reports
// the load before and after; a sample that ends above 15 is marked `taken: false`.
//
// Env: PARBLE_FF_SCENES (default "showcase,stress2000"), PARBLE_FF_AFTER (frames after READY,
// default 660 = the perf gate's settle + sample), PARBLE_FF_PORT (default 5313, the perf gate's own
// port: never run both at once), PARBLE_FF_OUT (default target/perf/first-frames.json),
// PARBLE_FF_DIST (default target/web-dist), PARBLE_LOAD_WAIT_MS (default 20 min), PARBLE_FF_NOGUARD=1 (diagnosis only: skip the load wait).
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";
import gpu from "./gpu-args.cjs";
import { LOAD_MAX, load1, stats, waitForLoad } from "./perf-helpers.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "..");
const SCENES = (process.env.PARBLE_FF_SCENES ?? "showcase,stress2000").split(",").filter(Boolean);
const AFTER = Number(process.env.PARBLE_FF_AFTER ?? 660);
const PORT = Number(process.env.PARBLE_FF_PORT ?? 5313);
const OUT = resolve(ROOT, process.env.PARBLE_FF_OUT ?? "target/perf/first-frames.json");
const TRACE = process.env.PARBLE_FF_TRACE === "1";
const DIST = resolve(ROOT, process.env.PARBLE_FF_DIST ?? "target/web-dist");
const NOGUARD = process.env.PARBLE_FF_NOGUARD === "1";
const LOAD_WAIT_MS = Number(process.env.PARBLE_LOAD_WAIT_MS ?? 20 * 60_000);
const url = (scene) => `http://127.0.0.1:${PORT}/?mute=1&quality=high&scenario=${scene}`;

// Runs in the page from its first script: one entry per rAF with the delta and the app state.
function recorder(after) {
  const frames = [];
  window.__ffFrames = frames;
  let last = 0;
  let readyAt = -1;
  function loop(t) {
    const p = window.__parble;
    frames.push({
      t: +t.toFixed(1),
      dt: last ? +(t - last).toFixed(1) : 0,
      frame: p ? p.frame : null,
      tick: p ? p.tick : null,
      ready: !!(p && p.ready),
      marbles: p?.marbles ? p.marbles.length : null,
      pipelines: p?.pipelines ? { ...p.pipelines, created: undefined, recentCreated: undefined } : null,
      created: p?.pipelines?.created ?? null,
      warm: p?.warmup ? p.warmup.done : null,
      // Kit textures loaded (or failed) of all requested.
      textures: p?.kit?.textures ? [p.kit.textures.filter((t) => t.state === "loaded" || t.state === "failed").length, p.kit.textures.length] : null,
    });
    last = t;
    if (p?.ready && readyAt < 0) readyAt = frames.length - 1;
    if (readyAt >= 0 && frames.length - 1 - readyAt >= after) {
      window.__ffDone = { frames, readyAt };
      return;
    }
    requestAnimationFrame(loop);
  }
  requestAnimationFrame(loop);
}

function serve() {
  const srv = spawn("python3", [resolve(HERE, "static-server.py"), String(PORT), DIST], {
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

async function run(scene) {
  const loadBefore = NOGUARD ? load1() : await waitForLoad(LOAD_WAIT_MS);
  const browser = await chromium.launch({ channel: "chromium", headless: true, args: [...gpu.GPU_ARGS, "--mute-audio"] });
  try {
    const page = await browser.newPage({ viewport: { width: 2560, height: 1440 }, deviceScaleFactor: 1 });
    const consoleLog = [];
    page.on("console", (m) => consoleLog.push({ type: m.type(), text: m.text() }));
    page.on("pageerror", (e) => consoleLog.push({ type: "pageerror", text: String(e) }));
    // Stamp each console line with the page clock, so it lines up with the rAF timeline.
    await page.addInitScript(`(${recorder.toString()})(${AFTER});
      for (const k of ["log", "info", "warn", "error", "debug"]) {
        const f = console[k].bind(console);
        console[k] = (...a) => f("[" + performance.now().toFixed(1) + "]", ...a);
      }`);
    if (TRACE) {
      await browser.startTracing(page, {
        path: resolve(dirname(OUT), `first-frames-${scene}.trace.json`),
        categories: ["devtools.timeline", "disabled-by-default-devtools.timeline", "disabled-by-default-devtools.timeline.frame", "gpu", "gpu.dawn", "disabled-by-default-gpu.dawn", "v8.execute", "blink"],
      });
    }
    await page.goto(url(scene));
    const probe = await gpu.probeGpu(page);
    const problems = gpu.gpuProblems(probe);
    if (problems.length) throw new Error(`GPU check failed: ${problems.join("; ")}`);
    // A string expression goes through CDP's evaluate, which the page's CSP (no unsafe-eval) allows;
    // waitForFunction does not.
    const t0 = Date.now();
    while (!(await page.evaluate("!!window.__ffDone"))) {
      if (Date.now() - t0 > 240_000) throw new Error(`${scene}: READY + ${AFTER} frames never came`);
      await new Promise((r) => setTimeout(r, 250));
    }
    const { frames, readyAt } = await page.evaluate("window.__ffDone");
    if (TRACE) await browser.stopTracing();
    const loadAfter = load1();
    const after = frames.slice(readyAt + 1).map((f) => f.dt);
    const settled = frames.slice(readyAt + 61).map((f) => f.dt);
    const long = frames.flatMap((f, i) => (f.dt > 50 ? [{ i, sinceReady: i - readyAt, ...f }] : []));
    const res = {
      scene,
      taken: loadBefore <= LOAD_MAX && loadAfter <= LOAD_MAX,
      loadBefore: +loadBefore.toFixed(2),
      loadAfter: +loadAfter.toFixed(2),
      adapter: probe.adapter,
      renderer: probe.renderer,
      readyFrame: readyAt,
      readyMs: frames[readyAt]?.t,
      afterReady: stats(after),
      afterSettle60: stats(settled),
      longFrames: long,
      // Pipeline creations after READY: anything here was compiled while the player was watching.
      createdAfterReady: frames.slice(readyAt + 1).flatMap((f, j) => (f.created?.length ? [{ sinceReady: j + 1, dt: f.dt, created: f.created }] : [])),
      console: consoleLog.filter((c) => c.type !== "debug").slice(0, 400),
      frames,
    };
    console.log(
      `${scene}: READY at frame ${readyAt} (${res.readyMs} ms); after READY max ${res.afterReady.maxMs.toFixed(1)} ms; after settle 60 max ${res.afterSettle60.maxMs.toFixed(1)} ms p95 ${res.afterSettle60.p95Ms.toFixed(1)} dropped ${res.afterSettle60.dropped}; load ${res.loadBefore} -> ${res.loadAfter}${res.taken ? "" : " NOT TAKEN"}`,
    );
    for (const l of long) console.log(`  long frame ${l.i} (READY${l.sinceReady >= 0 ? "+" : ""}${l.sinceReady}): ${l.dt} ms tick ${l.tick} marbles ${l.marbles} created ${JSON.stringify(l.created)}`);
    return res;
  } finally {
    await browser.close();
  }
}

const srv = await serve();
const results = [];
try {
  for (const s of SCENES) results.push(await run(s));
} finally {
  srv.kill();
}
mkdirSync(dirname(OUT), { recursive: true });
writeFileSync(OUT, `${JSON.stringify({ when: new Date().toISOString(), results }, null, 2)}\n`);
console.log(`wrote ${OUT}`);
