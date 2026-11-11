// Lost WebGPU device recovery (crates/parble-app/src/gpu_recovery.rs, index.html): a loss while
// loading and a loss mid-run both rebuild the renderer. Rendering resumes and READY comes back, the
// sim keeps ticking on the same scene, `window.__parble.gpuRecoveries` counts the rebuild, and
// nothing is reported as fatal. The bound (more than 3 losses in 30 s) is in fatal.spec.mjs.
import { expect, test } from "@playwright/test";

const SCENE = "/?scenario=showcase&mute=1&quality=high";

async function canvasStats(page) {
  const png = await page.locator("canvas#parble").screenshot();
  return page.evaluate(async (b64) => {
    const img = new Image();
    img.src = "data:image/png;base64," + b64;
    await img.decode();
    const c = document.createElement("canvas");
    c.width = img.width;
    c.height = img.height;
    const g = c.getContext("2d");
    g.drawImage(img, 0, 0);
    const d = g.getImageData(0, 0, c.width, c.height).data;
    let nonBlack = 0;
    const colors = new Set();
    for (let i = 0; i < d.length; i += 4) {
      if (Math.max(d[i], d[i + 1], d[i + 2]) > 8) nonBlack++;
      colors.add(((d[i] >> 4) << 8) | ((d[i + 1] >> 4) << 4) | (d[i + 2] >> 4));
    }
    return { nonBlack: nonBlack / (d.length / 4), colors: colors.size };
  }, png.toString("base64"));
}

const state = (page) =>
  page.evaluate(() => {
    const p = window.__parble ?? {};
    return {
      ready: p.ready,
      tick: p.tick,
      scenario: p.scenario,
      marbles: p.marbles?.length ?? 0,
      kitPieces: p.kit?.pieces,
      mainDraws: p.kit?.draws?.main ?? 0,
      shadowDraws: p.kit?.draws?.shadow ?? 0,
      renderFrame: p.pipelines?.renderFrame ?? 0,
      pipelinesOk: p.pipelines?.ok ?? 0,
      gpuRecoveries: p.gpuRecoveries,
      gpu: window.__parbleGpu,
      fatal: p.fatal ?? window.__parbleFatal ?? null,
      errors: window.__parbleErrors,
    };
  });

/// Waits for the recovered renderer to be READY with kit pieces drawn, then checks it keeps going.
async function expectRecovered(page, before) {
  await page.waitForFunction(
    () => window.__parble?.gpuRecoveries === 1 && window.__parble?.ready === true,
    null,
    { timeout: 60_000 },
  );
  // Kit pieces drawn again: a mesh not re-uploaded after the loss draws nothing.
  await expect.poll(async () => (await state(page)).mainDraws, { timeout: 15_000 }).toBeGreaterThan(0);
  const a = await state(page);
  await page.waitForTimeout(1_000);
  const b = await state(page);
  console.log("recovered:", JSON.stringify({ ...b, errors: b.errors.length }));
  expect(b.fatal).toBeNull();
  expect(b.gpuRecoveries).toBe(1);
  expect(b.gpu.losses).toBe(1);
  expect(b.gpu.recovered).toBe(1);
  expect(b.ready).toBe(true);
  expect(b.tick).toBeGreaterThan(a.tick);
  expect(b.renderFrame).toBeGreaterThan(a.renderFrame);
  expect(b.pipelinesOk).toBeGreaterThan(0);
  expect(b.scenario).toBe("showcase");
  expect(b.kitPieces).toBe(before?.kitPieces ?? b.kitPieces);
  expect(b.marbles).toBeGreaterThan(0);
  expect(b.shadowDraws).toBeGreaterThan(0);
  // Only the loss itself in the page's error log: no throw, no failed re-upload.
  expect(b.errors.filter((e) => !e.startsWith("WebGPU device lost") && !/Caught DeviceLost/.test(e))).toEqual([]);
  await expect
    .poll(async () => {
      const s = await canvasStats(page);
      return s.nonBlack > 0.5 && s.colors > 16;
    }, { timeout: 15_000, intervals: [1_000] })
    .toBe(true);
  return b;
}

test("a device lost while loading recovers: READY comes, the sim and the kit are intact", async ({ page }) => {
  await page.goto(`${SCENE}&gpu_lose_once_ms=300`);
  const b = await expectRecovered(page, null);
  expect(b.kitPieces).toBe(52);
});

test("a device lost mid-run recovers without restarting the sim", async ({ page }) => {
  await page.goto(SCENE);
  await page.waitForFunction(() => window.__parble?.ready === true, null, { timeout: 60_000 });
  await expect.poll(async () => (await state(page)).mainDraws, { timeout: 15_000 }).toBeGreaterThan(0);
  const before = await state(page);
  expect(before.gpuRecoveries).toBe(0);
  const marbleAt = await page.evaluate(() => window.__parble.marbles[0].position);
  await page.evaluate(() => window.__parbleLoseGpu());
  const b = await expectRecovered(page, before);
  // The same run continued: the tick only moved forward and no marble was dropped.
  expect(b.tick).toBeGreaterThan(before.tick);
  expect(b.marbles).toBeGreaterThanOrEqual(before.marbles);
  const marbleNow = await page.evaluate(() => window.__parble.marbles[0].position);
  expect(marbleNow).toHaveLength(3);
  console.log("marble 0:", JSON.stringify(marbleAt), "->", JSON.stringify(marbleNow));
});
