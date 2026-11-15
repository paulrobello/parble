// P6-T2: generation runs in the generator Web Worker (web/ui/src/gen, parble-sim-wasm's
// generate_and_validate) and the main thread keeps drawing. window.__parbleGen.generate runs three
// jobs back to back on the running playroom scene (default params at seeds 7 and 72, then a longer
// track); every one must be accepted with a file whose seed is the accepted sub-seed, the worker
// files must be fetched by versioned URL, and no rAF frame during generation may exceed 25 ms. The
// same page with no generation is measured first as the baseline. Frame times are load-sensitive:
// the frame check runs only when the host's 1-minute load is ≤ 15 before and after (waiting up to
// PARBLE_LOAD_WAIT_MS for it); the functional checks always run. Evidence: target/perf/p6t2-gen.json.
import { mkdirSync, writeFileSync } from "node:fs";
import { cpus } from "node:os";
import { expect, test } from "@playwright/test";
import { LOAD_MAX, load1, stats, waitForLoad } from "../perf-helpers.mjs";

const FRAME_MAX_MS = 25;
const BASELINE_MS = 5_000;
const LOAD_WAIT_MS = Number(process.env.PARBLE_LOAD_WAIT_MS ?? 10 * 60_000);
const OUT = new URL("../../target/perf/p6t2-gen.json", import.meta.url);
const JOBS = [
  { seed: 7, params: {} },
  { seed: 72, params: {} },
  { seed: 3, params: { height: 12, length: 80, footprint: 14 } },
];

test.setTimeout(LOAD_WAIT_MS + 240_000);

async function frames(page, run) {
  await page.evaluate(() => {
    window.__ft = [];
    window.__ftStop = false;
    let last = performance.now();
    const loop = (t) => {
      window.__ft.push(t - last);
      last = t;
      if (!window.__ftStop) requestAnimationFrame(loop);
    };
    requestAnimationFrame(loop);
  });
  const out = await run();
  const ft = await page.evaluate(() => {
    window.__ftStop = true;
    return window.__ft.slice(1);
  });
  return { out, ft };
}

test("the generator worker accepts tracks while the main thread keeps every frame under 25 ms", async ({ page }) => {
  mkdirSync(new URL(".", OUT), { recursive: true });
  const errors = [];
  const urls = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
  page.on("request", (r) => { if (/gen-worker\.js|\/gen\//.test(r.url())) urls.push(r.url()); });

  const loadBefore = await waitForLoad(LOAD_WAIT_MS);
  await page.goto("/?mute=1");
  await page.waitForFunction(() => window.__parble?.ready === true, null, { timeout: 60_000 });
  await page.waitForTimeout(3_000);

  const base = await frames(page, () => page.waitForTimeout(BASELINE_MS));
  const gen = await frames(page, () =>
    page.evaluate(async (jobs) => {
      const out = [];
      for (const { seed, params } of jobs) {
        const r = await window.__parbleGen.generate(seed, params);
        const file = JSON.parse(r.file);
        out.push({
          seed,
          accepted: r.seed,
          fileSeed: file.seed,
          placements: file.placements.length,
          attempt: r.status.attempt,
          rejected: r.status.rejected.map((x) => x.reason),
          units: r.status.units,
          ms: Math.round(r.ms),
        });
      }
      return out;
    }, JOBS),
  );
  const loadAfter = load1();
  const taken = loadBefore <= LOAD_MAX && loadAfter <= LOAD_MAX;
  const result = {
    taken,
    reason: taken ? null : `1-min load ${loadBefore.toFixed(2)} / ${loadAfter.toFixed(2)} > ${LOAD_MAX}`,
    loadBefore,
    loadAfter,
    cpus: cpus().length,
    frameMaxMs: FRAME_MAX_MS,
    baseline: stats(base.ft),
    generating: stats(gen.ft),
    jobs: gen.out,
    workerUrls: [...new Set(urls)],
  };
  writeFileSync(OUT, `${JSON.stringify(result, null, 2)}\n`);
  console.log(JSON.stringify(result, null, 2));

  expect(errors).toEqual([]);
  for (const j of gen.out) {
    expect(j.fileSeed).toBe(j.accepted);
    expect(j.placements).toBeGreaterThan(3);
  }
  expect(gen.out[0].attempt).toBeGreaterThanOrEqual(1);
  for (const name of ["gen-worker.js", "parble_sim_wasm.js", "parble_sim_wasm_bg.wasm"]) {
    expect(urls.some((u) => u.includes(name) && /\?v=[0-9a-f]{10}$/.test(u)), name).toBe(true);
  }
  expect(gen.ft.length).toBeGreaterThan(30);
  test.skip(!taken, result.reason ?? "");
  expect(result.baseline.maxMs, "the idle page already drops frames over the limit").toBeLessThanOrEqual(FRAME_MAX_MS);
  expect(result.generating.maxMs).toBeLessThanOrEqual(FRAME_MAX_MS);
});
