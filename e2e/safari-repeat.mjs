#!/usr/bin/env node
// READY repeat harness (local only, never CI): loads scenes in real Safari N times, each in a fresh
// safaridriver session (a cold load, sized like the perf gate: 2560 × 1440 device pixels, muted), and
// reports how many loads never reached READY. Like perf-gate.mjs, each load asks for a WebGPU adapter
// right after navigating and then waits for READY inside the page (a 100 ms poll in one execute/async
// script). A load fails on its timeout or at once on a fatal page error (`window.__parble.fatal`,
// index.html), and keeps the page-side error log (`window.__parbleErrors`, uncaught errors with their
// stacks) and the loading state. Written for the intermittent Safari showcase READY hang
// (docs/perf/README.md, "Safari READY repeat").
//
// Needs Safari's "Allow Remote Automation" and a GUI session, with Safari frontmost (a background
// window's rAF is throttled).
//
// Env:
//   PARBLE_REPEAT_N        loads, default 20.
//   PARBLE_REPEAT_SCENES   comma list cycled load by load, default "showcase" ("showcase,stress2000"
//                          alternates them, as the perf gate's Safari samples do).
//   PARBLE_REPEAT_QUERY    extra URL query, default "quality=high&mute=1".
//   PARBLE_REPEAT_SECONDS  READY timeout per load, default 50.
//   PARBLE_REPEAT_ADAPTER  "0" skips the adapter request after navigating (default on, as the gate).
//   PARBLE_REPEAT_OUT      output path, default target/perf/safari-repeat.json.
//   PARBLE_PERF_PORT       static server port, default 5313 (the perf gate's).
//   PARBLE_PERF_DIST       build to serve, default target/web-dist.
// Exit 0 when every load reached READY, 1 otherwise.
import { mkdirSync, writeFileSync } from "node:fs";
import { cpus, platform, release } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { diagSummary, load1, openSafari, pageDiag, serveDist } from "./perf-helpers.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "..");
const N = Number(process.env.PARBLE_REPEAT_N ?? 20);
const SCENES = (process.env.PARBLE_REPEAT_SCENES ?? "showcase").split(",").filter(Boolean);
const QUERY = process.env.PARBLE_REPEAT_QUERY ?? "quality=high&mute=1";
// Below the session's 60 s script timeout (perf-helpers.mjs openSafari).
const SECONDS = Math.min(55, Number(process.env.PARBLE_REPEAT_SECONDS ?? 50));
const ADAPTER_PROBE = process.env.PARBLE_REPEAT_ADAPTER !== "0";
const OUT = resolve(ROOT, process.env.PARBLE_REPEAT_OUT ?? "target/perf/safari-repeat.json");
const PORT = Number(process.env.PARBLE_PERF_PORT ?? 5313);
const BASE = `http://127.0.0.1:${PORT}`;
const ADAPTER = `
  var done = arguments[arguments.length - 1];
  if (!navigator.gpu) return done({ adapter: null });
  navigator.gpu.requestAdapter().then(function (a) {
    var i = a && a.info;
    done({ adapter: i ? (i.vendor + " " + i.architecture + " " + i.description).trim() : (a ? "adapter (no info)" : null) });
  }, function (e) { done({ adapter: null, error: String(e) }); });
`;

// Runs in the page (ES5): polls every 100 ms until READY, a fatal page error or `ms`, then calls
// `done` with the loading state (`pageDiag`, passed in as source).
// A fatal error is given 1 s to settle first, so a device loss that resolves after the throw it
// caused is in the snapshot (`fatal.deviceLost`).
function waiter(ms, done, diag) {
  var t0 = performance.now(), fatalAt = null;
  (function poll() {
    var p = window.__parble;
    var fatal = (p && p.fatal) || window.__parbleFatal || null;
    if (fatal && fatalAt === null) fatalAt = performance.now();
    var settled = fatalAt !== null && performance.now() - fatalAt > 1000;
    if ((p && p.ready && !fatal) || settled || performance.now() - t0 > ms) {
      fatal = window.__parbleFatal || fatal;
      var d = diag();
      d.fatal = fatal;
      d.waitedMs = Math.round(performance.now() - t0);
      return done(d);
    }
    setTimeout(poll, 100);
  })();
}
const WAIT = `(${waiter.toString()})(${SECONDS * 1000}, arguments[arguments.length - 1], ${pageDiag.toString()});`;

async function load(i, scene) {
  const loadBefore = +load1().toFixed(2);
  const b = await openSafari(BASE, 2560, 1440);
  try {
    const t0 = Date.now();
    await b.goto(`${BASE}/?scenario=${scene}&${QUERY}`);
    const adapter = ADAPTER_PROBE ? (await b.async(ADAPTER).catch((e) => ({ error: String(e) }))).adapter : undefined;
    const d = await b.async(WAIT).catch((e) => ({ scriptError: String(e?.message ?? e) }));
    const ms = Date.now() - t0;
    const ok = !!d.ready && !d.fatal;
    const r = { i, scene, ok, ms, adapter, loadBefore, loadAfter: +load1().toFixed(2), fatal: d.fatal ?? null, errors: d.errors ?? null };
    if (!ok) {
      r.diag = d;
      writeFileSync(OUT.replace(/\.json$/, `-${i}.png`), await b.shot().catch(() => Buffer.alloc(0)));
    }
    console.log(`${i} ${scene}: ${ok ? `READY at ${ms} ms` : `FAILED after ${ms} ms\n  ${diagSummary(d)}${d.fatal ? `\n  fatal stack:\n${d.fatal.stack}${d.fatal.deviceLost ? `\n  then: ${d.fatal.deviceLost}` : ""}` : ""}`} (load ${r.loadBefore} -> ${r.loadAfter})`);
    return r;
  } finally {
    await b.close();
  }
}

async function main() {
  mkdirSync(dirname(OUT), { recursive: true });
  const srv = await serveDist(PORT, resolve(ROOT, process.env.PARBLE_PERF_DIST ?? "target/web-dist"));
  const loads = [];
  const result = {
    harness: "Safari READY repeat",
    scenes: SCENES, query: QUERY, seconds: SECONDS, adapterProbe: ADAPTER_PROBE, n: N,
    dist: process.env.PARBLE_PERF_DIST ?? "target/web-dist",
    host: { platform: platform(), release: release(), cpu: cpus()[0]?.model, cores: cpus().length },
    when: new Date().toISOString(),
    loads,
  };
  const write = () => {
    result.failures = loads.filter((l) => l.ok === false).length;
    result.byScene = Object.fromEntries(SCENES.map((s) => {
      const ran = loads.filter((l) => l.scene === s && l.ok !== null);
      return [s, { loads: ran.length, failures: ran.filter((l) => !l.ok).length }];
    }));
    writeFileSync(OUT, `${JSON.stringify(result, null, 2)}\n`);
  };
  try {
    for (let i = 1; i <= N; i++) {
      const scene = SCENES[(i - 1) % SCENES.length];
      try {
        loads.push(await load(i, scene));
      } catch (e) {
        // A session that could not start is not a page failure: record it apart.
        loads.push({ i, scene, ok: null, sessionError: String(e?.message ?? e) });
        console.error(`${i} ${scene}: session error: ${e?.message ?? e}`);
      }
      write();
    }
  } finally {
    srv.kill();
  }
  const ran = loads.filter((l) => l.ok !== null).length;
  for (const [s, c] of Object.entries(result.byScene)) console.log(`${s}: ${c.failures} of ${c.loads} Safari loads failed READY`);
  console.log(`${result.failures} of ${ran} failed (${N - ran} session errors); wrote ${OUT}`);
  process.exit(result.failures === 0 && ran === N ? 0 : 1);
}

main().catch((e) => {
  console.error("safari-repeat:", e?.stack ?? e);
  process.exit(1);
});
