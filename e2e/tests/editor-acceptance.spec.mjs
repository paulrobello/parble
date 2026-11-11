// P3-T6 editor acceptance in GPU Chromium (PRD §6 P3). A 200-piece track is built from scratch
// through the UI bridge (window.wasmBindings.parbleEdit): the 52 placements of the all-pieces route
// (crates/parble-sim/src/allpieces.rs LAYOUT, a hopper-to-cup run) and 148 more pieces from the
// rest of the palette, off the route. Then it is run from the hopper with no console errors, and
// exported, re-imported and run twice: both runs give the same trajectory hash and the second
// export is byte-identical to the first. Evidence goes to target/perf/p3t6-acceptance.json.
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { expect, test } from "@playwright/test";

const OUT = new URL("../../target/perf/p3t6-acceptance.json", import.meta.url);
const BAD_TEXT = (t) => /%cERROR%c|\bERROR\b/.test(t) || /uncaptured|invalid|validation|panicked/i.test(t);

// The route, read from the Rust source so the two cannot drift.
const SRC = readFileSync(new URL("../../crates/parble-sim/src/allpieces.rs", import.meta.url), "utf8");
const LAYOUT_SRC = SRC.slice(SRC.indexOf("pub const LAYOUT"), SRC.indexOf("];", SRC.indexOf("pub const LAYOUT")));
const ROUTE = [...LAYOUT_SRC.matchAll(/\("([a-z0-9_]+)", \[(-?\d+), (-?\d+), (-?\d+)\], (\d)\)/g)].map((m) => ({
  piece: m[1],
  cell: [Number(m[2]), Number(m[3]), Number(m[4])],
  yaw: Number(m[5]),
}));

// Off the route: every builtin piece but the hopper (a track runs one), cycled with all four yaws
// on a 4-cell grid behind the route (z ≤ −12) at layer 1, so the largest footprints (x 0..2,
// y −1..1, z −2..0) never touch a neighbour or the route (z −4..−8).
const KIT = new URL("../../assets/kit/", import.meta.url);
const PALETTE = readdirSync(KIT)
  .filter((d) => existsSync(new URL(`${d}/${d}.kit.json`, KIT)) && d !== "mech_hopper")
  .sort();
const EXTRA = Array.from({ length: 200 - ROUTE.length }, (_, i) => ({
  piece: PALETTE[i % PALETTE.length],
  cell: [4 * (i % 12), 1, -12 - 4 * Math.floor(i / 12)],
  yaw: Math.floor(i / PALETTE.length) % 4,
}));

let nextId = 1;
async function edit(page, action) {
  const id = nextId++;
  await page.evaluate(([a, id]) => window.wasmBindings.parbleEdit(JSON.stringify({ ...a, id })), [action, id]);
  await page.waitForFunction((id) => window.__parble?.editor?.last?.id === id, id, { timeout: 10_000 });
  return page.evaluate(() => window.__parble.editor.last);
}

const panel = (page) => page.locator("#parble-files");

async function exportBytes(page, trigger) {
  const [download] = await Promise.all([page.waitForEvent("download"), trigger()]);
  const chunks = [];
  for await (const c of await download.createReadStream()) chunks.push(c);
  return Buffer.concat(chunks);
}

async function importAndRun(page, name, bytes) {
  await page.locator("#parble-import").setInputFiles({ name, mimeType: "application/json", buffer: bytes });
  await page.waitForFunction(() => window.__parble.editor.pieces === 200, null, { timeout: 30_000 });
  await page.evaluate(() => {
    const p = document.querySelector("#parble-files");
    p.dataset.runHash = "";
  });
  await page.locator("#parble-run").click();
  await expect(panel(page)).toHaveAttribute("data-run-hash", /^[0-9a-f]{16}$/, { timeout: 60_000 });
  expect(await panel(page).getAttribute("data-error")).toBe("");
  return { hash: await panel(page).getAttribute("data-run-hash"), ticks: Number(await panel(page).getAttribute("data-run-ticks")) };
}

test("P3 acceptance: build a 200-piece track from scratch, run it, export, re-import and run", async ({ page }) => {
  test.setTimeout(240_000);
  expect(ROUTE.length).toBe(52);
  const errors = [];
  page.on("console", (m) => {
    if (m.type() === "error" || BAD_TEXT(m.text())) errors.push(m.text());
  });
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto("/?mute=1&scenario=editor&editor-ui=0");
  await page.waitForFunction(() => window.__parble?.ready === true && window.__parble.editor && window.__parble.frame > 10, null, {
    timeout: 60_000,
  });

  const t0 = Date.now();
  for (const [i, p] of [...ROUTE, ...EXTRA].entries()) {
    const last = await edit(page, { op: "place", ...p, mirror: false }).catch(async (e) => {
      throw new Error(`place ${i} ${JSON.stringify(p)}: ${e.message}; errors ${JSON.stringify(errors)}; ${JSON.stringify(await page.evaluate(() => ({ frame: window.__parble.frame, last: window.__parble.editor.last, fatal: window.__parble.fatal ?? null })))}`);
    });
    expect(last.ok, `${p.piece} at ${p.cell} yaw ${p.yaw}: ${last.error ?? ""}`).toBe(true);
  }
  const buildMs = Date.now() - t0;
  await page.waitForFunction(() => window.__parble.editor.dirtyColliderChunks === 0, null, { timeout: 30_000 });
  const built = await page.evaluate(() => window.__parble.editor);
  expect(built.pieces).toBe(200);
  expect(built.hoppers).toBe(1);

  // Run from the hopper as the all-pieces track does: ten marbles, one every 480 ticks.
  expect((await edit(page, { op: "run", release: "interval", intervalTicks: 480 })).ok).toBe(true);
  await page.waitForFunction(() => window.__parble.editor.run.active && !window.__parble.editor.run.building, null, {
    timeout: 30_000,
  });
  const runStart = (await page.evaluate(() => window.__parble.editor.run)).tick;
  await page.waitForFunction((t) => window.__parble.editor.run.tick >= t + 2_400 && window.__parble.editor.run.marbles >= 5, runStart, {
    timeout: 60_000,
  });
  const run = await page.evaluate(() => window.__parble.editor.run);
  expect((await edit(page, { op: "stop" })).ok).toBe(true);

  // Export, import and run, export again, import and run again.
  expect((await edit(page, { op: "rename", name: "P3-T6 acceptance" })).ok).toBe(true);
  const first = await exportBytes(page, () => edit(page, { op: "export" }));
  const file = JSON.parse(first.toString("utf8"));
  expect(file.placements.length).toBe(200);
  const runA = await importAndRun(page, "p3t6.parble", first);
  const second = await exportBytes(page, () => edit(page, { op: "export" }));
  expect(second.equals(first)).toBe(true);
  const runB = await importAndRun(page, "p3t6-again.parble", second);
  expect(runB).toEqual(runA);

  const result = {
    pieces: built.pieces,
    route: ROUTE.length,
    extra: EXTRA.length,
    pieceTypes: new Set([...ROUTE, ...EXTRA].map((p) => p.piece)).size,
    placeOpsMs: buildMs,
    issues: built.issueCount,
    run: { ticks: run.tick - runStart, marbles: run.marbles, queue: run.queue },
    exportBytes: first.length,
    exportIdentical: second.equals(first),
    runHash: runA.hash,
    runTicks: runA.ticks,
    rerunHash: runB.hash,
    consoleErrors: errors.length,
    userAgent: await page.evaluate(() => navigator.userAgent),
  };
  mkdirSync(new URL(".", OUT), { recursive: true });
  writeFileSync(OUT, `${JSON.stringify(result, null, 2)}\n`);
  console.log(JSON.stringify(result, null, 2));
  expect(errors).toEqual([]);
});
