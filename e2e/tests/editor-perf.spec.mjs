// P3-T4 perf trace: editing a 2,000-piece track in GPU Chromium. Opt-in (PARBLE_PERF=1, `make
// web-editor-perf`), since it is a measurement, not a smoke. The track is the editor's 2,000-piece
// fill (`editor-fill=2000`, 42 dense chunks). With every chunk collider built, it records the rAF
// frame times while idle and then while running a scripted mix of edits on and across chunk
// borders (rotate a 3 × 3 group, remove it and undo, stack a piece and undo). Each edit's
// timings come from window.__parble.editor.lastEdit and kit.edit. The host's 1-minute load average
// must be ≤ 15 before and after; if it does not drop there within the wait, the sample is reported
// as not taken. The result goes to target/perf/p3t4-editor.json.
import { mkdirSync, writeFileSync } from "node:fs";
import { loadavg } from "node:os";
import { expect, test } from "@playwright/test";
import { BUDGET_MS, LOAD_MAX, stats, waitForLoad as waitForLoadWithin } from "../perf-helpers.mjs";

const LOAD_WAIT_MS = Number(process.env.PARBLE_LOAD_WAIT_MS ?? 20 * 60_000);
const OUT = new URL("../../target/perf/p3t4-editor.json", import.meta.url);

test.skip(process.env.PARBLE_PERF !== "1", "perf trace is opt-in: PARBLE_PERF=1");
test.setTimeout(LOAD_WAIT_MS + 240_000);

let nextId = 1;
async function edit(page, action) {
  const id = nextId++;
  await page.evaluate(([a, id]) => window.wasmBindings.parbleEdit(JSON.stringify({ ...a, id })), [action, id]);
  await page.waitForFunction((id) => window.__parble?.editor?.last?.id === id, id, { timeout: 10_000 });
  return page.evaluate(() => window.__parble.editor.last);
}

const state = (page) => page.evaluate(() => window.__parble.editor);

async function settle(page) {
  await page.waitForFunction(() => window.__parble.editor.dirtyColliderChunks === 0, null, { timeout: 30_000 });
  for (let i = 0; i < 4; i++) await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => r())));
}

async function waitForLoad() {
  return waitForLoadWithin(LOAD_WAIT_MS);
}

test("editing a 2,000-piece track keeps the chunk rebuild inside one frame", async ({ page }) => {
  mkdirSync(new URL(".", OUT), { recursive: true });
  // PARBLE_PERF_DIAG=1 runs once without waiting for the load; the result is still marked not
  // taken unless the load was within LOAD_MAX before and after.
  const diagRun = process.env.PARBLE_PERF_DIAG === "1";
  const loadBefore = diagRun ? loadavg()[0] : await waitForLoad();
  if (loadBefore > LOAD_MAX && !diagRun) {
    const result = { taken: false, reason: `1-min load ${loadBefore.toFixed(2)} > ${LOAD_MAX}`, loadBefore };
    writeFileSync(OUT, `${JSON.stringify(result, null, 2)}\n`);
    console.log(`NOT TAKEN: ${result.reason}`);
    test.skip(true, result.reason);
    return;
  }
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
  await page.goto("/?mute=1&scenario=editor&editor-fill=2000");
  await page.waitForFunction(
    () => window.__parble?.editor?.pieces === 2000 && window.__parble.editor.stats.fill && window.__parble.editor.dirtyColliderChunks === 0,
    null,
    { timeout: 120_000 },
  );
  await edit(page, { op: "camera", target: [1.5, 0, -1.55], yaw: -0.4, pitch: 0.85, radius: 3.4 });
  await edit(page, { op: "tool", tool: "select" });
  await settle(page);
  const fill = (await page.evaluate(() => window.__parble.editor.stats.fill));

  await page.evaluate(() => {
    window.__ft = [];
    let last = performance.now();
    const loop = (t) => {
      window.__ft.push(t - last);
      last = t;
      if (!window.__ftStop) requestAnimationFrame(loop);
    };
    requestAnimationFrame(loop);
  });
  await page.waitForTimeout(3_000);
  const idle = await page.evaluate(() => window.__ft.splice(0));

  // Cells on chunk borders (chunks are 8 cells; the fill spans x 0..49, z −5..−44).
  const spots = [];
  for (const x of [6, 14, 22, 30, 38, 46]) for (const z of [-7, -15, -23, -31, -39]) spots.push([x, z]);
  const perEdit = [];
  const record = async (label) => {
    await settle(page);
    const [e, k] = await page.evaluate(() => [window.__parble.editor.lastEdit, window.__parble.kit.edit]);
    perEdit.push({ label, ...e, renderSpawnMs: k.lastMs, renderSpawnChunks: k.lastChunks });
  };
  for (const [x, z] of spots) {
    const a = [x, 0, z];
    const b = [x + 2, 0, z - 2];
    await edit(page, { op: "boxSelect", a, b });
    expect((await edit(page, { op: "rotate", by: 1 })).ok).toBe(true);
    await record("rotate3x3");
    expect((await edit(page, { op: "remove" })).ok).toBe(true);
    await record("remove3x3");
    expect((await edit(page, { op: "undo" })).ok).toBe(true);
    await record("undoRemove");
    expect((await edit(page, { op: "place", piece: "blk_end_l3", cell: [x + 1, 1, z - 1] })).ok).toBe(true);
    await record("placeStacked");
    expect((await edit(page, { op: "undo" })).ok).toBe(true);
    await record("undoPlace");
  }
  const editing = await page.evaluate(() => {
    window.__ftStop = true;
    return window.__ft.splice(0);
  });
  const loadAfter = loadavg()[0];
  const diag = await page.evaluate(() => ({ editor: window.__parble.editor, kit: window.__parble.kit.edit }));

  const worst = (k) => Math.max(...perEdit.map((e) => e[k]));
  const mean = (k) => perEdit.reduce((s, e) => s + e[k], 0) / perEdit.length;
  const result = {
    taken: loadBefore <= LOAD_MAX && loadAfter <= LOAD_MAX,
    loadBefore,
    loadAfter,
    userAgent: await page.evaluate(() => navigator.userAgent),
    pieces: diag.editor.pieces,
    colliderChunks: diag.editor.colliderChunks,
    renderChunks: diag.kit.chunks,
    fill,
    edits: perEdit.length,
    budgetMs: BUDGET_MS,
    perEditWorst: {
      editMs: worst("editMs"),
      colliderMsTotal: worst("colliderMs"),
      colliderFrames: worst("colliderFrames"),
      colliderChunks: worst("colliderChunks"),
      renderSpawnMs: worst("renderSpawnMs"),
      renderChunks: worst("renderChunks"),
    },
    perEditMean: {
      editMs: mean("editMs"),
      colliderMsTotal: mean("colliderMs"),
      colliderFrames: mean("colliderFrames"),
      renderSpawnMs: mean("renderSpawnMs"),
    },
    colliderFrameMsMax: diag.editor.stats.colliderFrameMsMax,
    colliderStepMsMax: diag.editor.stats.colliderStepMsMax,
    // Worst frame's rebuild work: the command + diff + hand-off, the budgeted collider stages and
    // the render chunk respawn, as if all three landed in one frame.
    worstFrameRebuildMs: worst("editMs") + diag.editor.stats.colliderFrameMsMax + worst("renderSpawnMs"),
    frameTimes: { idle: stats(idle), editing: stats(editing) },
    perEdit,
  };
  writeFileSync(OUT, `${JSON.stringify(result, null, 2)}\n`);
  const { perEdit: _, ...summary } = result;
  console.log(JSON.stringify(summary, null, 2));
  expect(errors).toEqual([]);
  expect(result.pieces).toBe(2000);
  expect(result.worstFrameRebuildMs).toBeLessThan(BUDGET_MS);
  if (!result.taken) {
    console.log(`NOT TAKEN: load ${loadBefore.toFixed(2)} before, ${loadAfter.toFixed(2)} after (max ${LOAD_MAX})`);
  }
});

// P3-T5 run-build latency: pressing Run (and Stop) on the same 2,000-piece fill, plus a hopper at
// [0, 0, 0] (the fill starts at z −5). Each sample is the longest rAF delta while the run swaps in,
// and the engine's own timing around the build and install (`editor.lastRun.buildMs`, absent on
// engines that predate it). The bar is the frame budget (PRD §6 P3: edits ≤ 1 frame amortized).
// Writes target/perf/p3t5-run-build.json before asserting, so a miss still records its numbers.
const RUN_OUT = new URL("../../target/perf/p3t5-run-build.json", import.meta.url);
const RUN_SAMPLES = 4;

test("Run on a 2,000-piece track builds the run inside one frame", async ({ page }) => {
  mkdirSync(new URL(".", RUN_OUT), { recursive: true });
  const diagRun = process.env.PARBLE_PERF_DIAG === "1";
  const loadBefore = diagRun ? loadavg()[0] : await waitForLoad();
  if (loadBefore > LOAD_MAX && !diagRun) {
    const result = { taken: false, reason: `1-min load ${loadBefore.toFixed(2)} > ${LOAD_MAX}`, loadBefore };
    writeFileSync(RUN_OUT, `${JSON.stringify(result, null, 2)}\n`);
    test.skip(true, result.reason);
    return;
  }
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
  await page.goto("/?mute=1&scenario=editor&editor-fill=2000&editor-ui=0");
  await page.waitForFunction(
    () => window.__parble?.editor?.pieces === 2000 && window.__parble.editor.stats.fill && window.__parble.editor.dirtyColliderChunks === 0,
    null,
    { timeout: 120_000 },
  );
  expect((await edit(page, { op: "place", piece: "mech_hopper", cell: [0, 0, 0] })).ok).toBe(true);
  await edit(page, { op: "camera", target: [1.5, 0, -1.55], yaw: -0.4, pitch: 0.85, radius: 3.4 });
  await settle(page);
  const sample = async (action, done) => {
    await page.evaluate(() => {
      window.__ft = [];
      let last = performance.now();
      window.__ftStop = false;
      const loop = (t) => {
        window.__ft.push(t - last);
        last = t;
        if (!window.__ftStop) requestAnimationFrame(loop);
      };
      requestAnimationFrame(loop);
    });
    await page.waitForTimeout(300);
    const t0 = Date.now();
    expect((await edit(page, action)).ok).toBe(true);
    await page.waitForFunction(done, null, { timeout: 30_000 });
    const roundTripMs = Date.now() - t0;
    await page.waitForTimeout(300);
    const ft = await page.evaluate(() => {
      window.__ftStop = true;
      return window.__ft.splice(0);
    });
    const lastRun = await page.evaluate(() => window.__parble.editor.lastRun ?? null);
    return { op: action.op, worstFrameMs: Math.max(...ft), roundTripMs, buildMs: lastRun?.buildMs ?? null, lastRun };
  };
  const samples = [];
  for (let i = 0; i < RUN_SAMPLES; i++) {
    samples.push(await sample({ op: "run", release: "all" }, () => window.__parble.editor.run.active === true));
    samples.push(await sample({ op: "stop" }, () => window.__parble.editor.run.active === false));
  }
  const loadAfter = loadavg()[0];
  const runs = samples.filter((s) => s.op === "run");
  const result = {
    taken: loadBefore <= LOAD_MAX && loadAfter <= LOAD_MAX,
    loadBefore,
    loadAfter,
    userAgent: await page.evaluate(() => navigator.userAgent),
    pieces: (await state(page)).pieces,
    budgetMs: BUDGET_MS,
    run: {
      worstFrameMs: Math.max(...runs.map((s) => s.worstFrameMs)),
      meanWorstFrameMs: runs.reduce((a, s) => a + s.worstFrameMs, 0) / runs.length,
      buildMs: runs.map((s) => s.buildMs),
    },
    samples,
  };
  writeFileSync(RUN_OUT, `${JSON.stringify(result, null, 2)}\n`);
  console.log(JSON.stringify(result, null, 2));
  expect(errors).toEqual([]);
  if (!result.taken) console.log(`NOT TAKEN: load ${loadBefore.toFixed(2)} before, ${loadAfter.toFixed(2)} after (max ${LOAD_MAX})`);
  expect(result.run.worstFrameMs).toBeLessThan(BUDGET_MS);
});
