// P6 procgen review phases 5 and 6: a 500-piece generated track (500:48:14, segmented job, base 0)
// in GPU Chromium at 2560 × 1440, quality high. Opt-in (PARBLE_PERF=1, `make web-gen500-perf`).
// It generates the track in the worker while recording the main thread's rAF frames (none over
// 25 ms, as in gen-worker.spec), imports the file into the editor, waits for every chunk collider,
// then records 5 s idle (p95 within the perf gate's 60 Hz rule, 16.7 ms plus the rAF quantum) and
// one place + undo edit on the loaded track (the chunk rebuild inside one frame, as editor-perf).
// The host's 1-minute load must be ≤ 15 before and after, else the timing checks are skipped and
// the sample is reported as not taken. Evidence: target/perf/p6-gen500.json.
import { mkdirSync, writeFileSync } from "node:fs";
import { cpus } from "node:os";
import { expect, test } from "@playwright/test";
import { BUDGET_MS, LOAD_MAX, load1, stats, waitForLoad } from "../perf-helpers.mjs";

const FRAME_MAX_MS = 25;
const IDLE_P95_MS = 16.7 + 0.2;
const IDLE_MS = 5_000;
const LOAD_WAIT_MS = Number(process.env.PARBLE_LOAD_WAIT_MS ?? 20 * 60_000);
const OUT = new URL("../../target/perf/p6-gen500.json", import.meta.url);
const JOB = { seed: 0, params: { length: 500, height: 48, footprint: 14 } };

test.skip(process.env.PARBLE_PERF !== "1", "perf trace is opt-in: PARBLE_PERF=1");
test.setTimeout(LOAD_WAIT_MS + 900_000);
test.use({ viewport: { width: 2560, height: 1440 }, deviceScaleFactor: 1 });

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

let nextId = 1;
async function edit(page, action) {
  const id = nextId++;
  await page.evaluate(([a, id]) => window.wasmBindings.parbleEdit(JSON.stringify({ ...a, id })), [action, id]);
  await page.waitForFunction((id) => window.__parble?.editor?.last?.id === id, id, { timeout: 30_000 });
  return page.evaluate(() => window.__parble.editor.last);
}

async function settle(page) {
  await page.waitForFunction(() => window.__parble.editor.dirtyColliderChunks === 0, null, { timeout: 120_000 });
  for (let i = 0; i < 4; i++) await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => r())));
}

test("a generated 500-piece track generates in the worker and edits inside the frame budget", async ({ page }) => {
  mkdirSync(new URL(".", OUT), { recursive: true });
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });

  const loadBefore = await waitForLoad(LOAD_WAIT_MS);
  await page.goto("/?mute=1&scenario=editor&quality=high&editor-ui=0");
  await page.waitForFunction(() => window.__parble?.ready === true && window.__parble.editor && window.__parble.frame > 10, null, {
    timeout: 60_000,
  });
  await page.waitForTimeout(3_000);

  const gen = await frames(page, () =>
    page.evaluate(async ({ seed, params }) => {
      const r = await window.__parbleGen.generate(seed, params, { segmented: true });
      return { file: r.file, seed: r.seed, ms: Math.round(r.ms), rejected: r.status.rejected.length };
    }, JOB),
  );
  const file = JSON.parse(gen.out.file);
  const placements = file.placements.length;

  await page.locator("#parble-import").setInputFiles({
    name: "gen500.parble",
    mimeType: "application/json",
    buffer: Buffer.from(gen.out.file),
  });
  await page.waitForFunction((n) => window.__parble.editor.pieces === n, placements, { timeout: 120_000 });
  await settle(page);

  const idle = await frames(page, () => page.waitForTimeout(IDLE_MS));
  const empty = file.placements.reduce((m, p) => [Math.max(m[0], p.anchor[0]), Math.max(m[1], p.anchor[2])], [-1e9, -1e9]);
  const cell = [empty[0] + 2, 0, empty[1] + 2];
  const editRun = await frames(page, async () => {
    const placed = await edit(page, { op: "place", piece: "blk_straight_l3", cell, yaw: 0 });
    await settle(page);
    const undone = await edit(page, { op: "undo" });
    await settle(page);
    return { placed, undone };
  });
  const loadAfter = load1();
  const taken = loadBefore <= LOAD_MAX && loadAfter <= LOAD_MAX;
  const result = {
    taken,
    reason: taken ? null : `1-min load ${loadBefore.toFixed(2)} / ${loadAfter.toFixed(2)} > ${LOAD_MAX}`,
    loadBefore,
    loadAfter,
    cpus: cpus().length,
    job: { ...JOB, segmented: true, accepted: gen.out.seed, ms: gen.out.ms, rejected: gen.out.rejected },
    placements,
    trackPieces: file.placements.filter((p) => !p.piece.startsWith("blk_pillar")).length,
    generating: stats(gen.ft),
    idle: stats(idle.ft),
    edit: { stats: stats(editRun.ft), placed: editRun.out.placed, undone: editRun.out.undone },
    frameMaxMs: FRAME_MAX_MS,
    idleP95Ms: IDLE_P95_MS,
    budgetMs: BUDGET_MS,
  };
  writeFileSync(OUT, `${JSON.stringify(result, null, 2)}\n`);
  console.log(JSON.stringify({ ...result, edit: { stats: result.edit.stats } }, null, 2));

  expect(errors).toEqual([]);
  expect(result.trackPieces).toBeGreaterThanOrEqual(450);
  expect(placements).toBeLessThan(2_000);
  expect(editRun.out.placed.ok).toBe(true);
  test.skip(!taken, result.reason ?? "");
  expect(result.generating.maxMs).toBeLessThanOrEqual(FRAME_MAX_MS);
  expect(result.idle.p95Ms).toBeLessThanOrEqual(IDLE_P95_MS);
  expect(result.edit.stats.maxMs).toBeLessThanOrEqual(FRAME_MAX_MS);
});
