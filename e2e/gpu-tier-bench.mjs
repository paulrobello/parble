// GPU tier benchmark (card 01a1c67417ff): runs the title's first-run quality detection
// (web/ui/src/title/gpu.ts) several times on a fresh profile in GPU Chromium (headless, platform GPU
// flags) and in Safari (safaridriver), under the load guard, and records each run's median pass time.
// Serves target/web-dist (`make build-web` first) on its own port.
//
//   node e2e/gpu-tier-bench.mjs
//   PARBLE_TIER_BROWSERS  comma list, default "chromium,safari"
//   PARBLE_TIER_RUNS      detections per browser, default 7
//   PARBLE_TIER_OUT       output path, default target/perf/gpu-tier-<host>.json
//   PARBLE_TIER_WAIT_MS   most time to wait for load <= 15 before a run, default 20 min (0: smoke)

import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { hostname, release } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";
import gpu from "./gpu-args.cjs";
import { LOAD_MAX, load1, stats, waitForLoad } from "./perf-helpers.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "..");
const BROWSERS = (process.env.PARBLE_TIER_BROWSERS ?? "chromium,safari").split(",").filter(Boolean);
const RUNS = Number(process.env.PARBLE_TIER_RUNS ?? 7);
const PORT = Number(process.env.PARBLE_PERF_PORT ?? 5313);
const HOST = hostname().split(".")[0];
const OUT = resolve(ROOT, process.env.PARBLE_TIER_OUT ?? `target/perf/gpu-tier-${HOST}.json`);
const WAIT_MS = Number(process.env.PARBLE_TIER_WAIT_MS ?? 20 * 60_000);
const BASE = `http://127.0.0.1:${PORT}`;
const URL = `${BASE}/?mute=1&title=1`;

// ES5 for Safari's execute/async: waits for the title's detection and returns it.
const DETECT = `
  var done = arguments[arguments.length - 1], t0 = Date.now();
  (function wait() {
    var t = window.__parbleTitle;
    if (!t) { if (Date.now() - t0 > 60000) return done({ error: "no __parbleTitle" }); return setTimeout(wait, 100); }
    t.ready.then(function () { done(t.state().detection); }, function (e) { done({ error: String(e) }); });
  })();
`;

function serve() {
  const srv = spawn("python3", [resolve(HERE, "static-server.py"), String(PORT), resolve(ROOT, "target/web-dist")], {
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

async function chromiumRun() {
  const browser = await chromium.launch({ channel: "chromium", headless: true, args: [...gpu.GPU_ARGS, "--mute-audio"] });
  try {
    const page = await browser.newPage({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 });
    await page.goto(URL);
    const probe = await gpu.probeGpu(page);
    const problems = gpu.gpuProblems(probe);
    if (problems.length) throw new Error(`GPU check failed: ${problems.join("; ")}`);
    return await page.evaluate(`new Promise((done) => { ${DETECT.replace("var done = arguments[arguments.length - 1],", "var")} })`);
  } finally {
    await browser.close();
  }
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

async function safariRun() {
  const port = 20000 + Math.floor(Math.random() * 20000);
  const base = `http://127.0.0.1:${port}`;
  const driver = spawn("safaridriver", ["-p", String(port)], { stdio: "ignore" });
  let session = null;
  try {
    await new Promise((r) => setTimeout(r, 1500));
    session = (await wd(base, "POST", "/session", { capabilities: { alwaysMatch: { browserName: "safari" } } })).sessionId;
    const s = `/session/${session}`;
    await wd(base, "POST", `${s}/timeouts`, { script: 90_000, pageLoad: 60_000 });
    await wd(base, "POST", `${s}/url`, { url: URL });
    await wd(base, "POST", `${s}/execute/sync`, { args: [], script: "localStorage.clear(); return 0;" });
    await wd(base, "POST", `${s}/url`, { url: URL });
    return await wd(base, "POST", `${s}/execute/async`, { args: [], script: DETECT });
  } finally {
    if (session) await wd(base, "DELETE", `/session/${session}`).catch(() => {});
    driver.kill();
  }
}

const srv = await serve();
const out = { host: HOST, os: release(), date: new Date().toISOString(), runs: RUNS, loadMax: LOAD_MAX, browsers: {} };
let failed = false;
try {
  for (const b of BROWSERS) {
    const runs = [];
    for (let i = 0; i < RUNS; i++) {
      const before = await waitForLoad(WAIT_MS);
      const d = b === "safari" ? await safariRun() : await chromiumRun();
      const after = load1();
      runs.push({ ...d, loadBefore: before, loadAfter: after, discard: before > LOAD_MAX || after > LOAD_MAX });
      console.log(b, i + 1, JSON.stringify(d), `load ${before} -> ${after}`);
    }
    const ok = runs.filter((r) => !r.discard && r.reason === "bench").map((r) => r.benchMs);
    const s = ok.length ? stats(ok) : null;
    out.browsers[b] = { runs, kept: ok.length, benchMs: s, adapter: runs.find((r) => r.adapter)?.adapter ?? null };
    console.log(b, "kept", ok.length, "of", runs.length, s ? JSON.stringify(s) : "");
    if (!ok.length) failed = true;
  }
} finally {
  srv.kill();
}
mkdirSync(dirname(OUT), { recursive: true });
writeFileSync(OUT, `${JSON.stringify(out, null, 2)}\n`);
console.log("wrote", OUT);
process.exit(failed ? 1 : 0);
