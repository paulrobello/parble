// P0-T6 smoke: the release build in GPU Chromium exposes window.__parble, renders a lit scene, honours
// the URL query switches and logs no errors.
import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";

const GOLDEN = readFileSync(new URL("../../crates/parble-sim/tests/golden/ramp_10s.hash", import.meta.url), "utf8").trim();
// Bevy's wasm logger prints ERROR through console.log, styled as "%cERROR%c".
const BAD_TEXT = (t) => /%cERROR%c|\bERROR\b/.test(t) || /uncaptured|invalid|panicked/i.test(t);

function watchErrors(page) {
  const errors = [];
  page.on("console", (m) => { if (m.type() === "error" || BAD_TEXT(m.text())) errors.push(m.text()); });
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("requestfailed", (r) => errors.push(`request failed: ${r.url()}`));
  return errors;
}

async function canvasStats(page) {
  const png = await page.locator("canvas#parble").screenshot();
  return page.evaluate(async (b64) => {
    const img = new Image();
    img.src = "data:image/png;base64," + b64;
    await img.decode();
    const c = document.createElement("canvas");
    c.width = img.width; c.height = img.height;
    const g = c.getContext("2d");
    g.drawImage(img, 0, 0);
    const d = g.getImageData(0, 0, c.width, c.height).data;
    let nonBlack = 0, lit = 0;
    const colors = new Set();
    for (let i = 0; i < d.length; i += 4) {
      const m = Math.max(d[i], d[i + 1], d[i + 2]);
      if (m > 8) nonBlack++;
      if (m > 60) lit++;
      colors.add((d[i] >> 4) << 8 | (d[i + 1] >> 4) << 4 | (d[i + 2] >> 4));
    }
    const px = d.length / 4;
    return { nonBlack: nonBlack / px, lit: lit / px, colors: colors.size };
  }, png.toString("base64"));
}

const diag = (page) => page.evaluate(() => window.__parble ?? null);

test("window.__parble reports a live sim and the canvas shows a lit scene", async ({ page }) => {
  const errors = watchErrors(page);
  await page.goto("/");
  await page.waitForFunction(() => window.__parble?.ready === true, null, { timeout: 60_000 });
  const first = await diag(page);
  console.log("__parble:", JSON.stringify(first));
  expect(first.tick).toBeGreaterThan(0);
  expect(first.hash).toMatch(/^[0-9a-f]{16}$/);
  expect(first.marbles).toHaveLength(1);
  for (const k of ["position", "linvel"]) expect(first.marbles[0][k]).toHaveLength(3);
  expect(first.marbles[0].rotation).toHaveLength(4);
  expect(first.scenario).toBe("ramp");
  expect(first.quality).toBe("medium");

  await expect.poll(async () => {
    const s = await canvasStats(page);
    return s.nonBlack > 0.5 && s.lit > 0.02 && s.colors > 16;
  }, { timeout: 30_000, intervals: [1_000] }).toBe(true);
  const later = await diag(page);
  expect(later.tick).toBeGreaterThan(first.tick);
  expect(later.fps).toBeGreaterThan(1);

  const adapter = await page.evaluate(async () => {
    const a = await navigator.gpu.requestAdapter();
    return a?.info ? `${a.info.vendor} ${a.info.architecture} ${a.info.description}` : null;
  });
  console.log("adapter:", adapter);
  expect(adapter).not.toBeNull();
  if (process.env.PW_GPU !== "swiftshader") expect(adapter).not.toMatch(/swiftshader/i);
  expect(await page.evaluate(() => window.__parbleErrors)).toEqual([]);
  expect(errors).toEqual([]);
});

test("a completed 10 s run reports the golden trajectory hash", async ({ page }) => {
  const errors = watchErrors(page);
  await page.goto("/");
  await page.waitForFunction(() => window.__parble?.lastRunHash != null, null, { timeout: 75_000 });
  const d = await diag(page);
  console.log(`run=${d.run} lastRunHash=${d.lastRunHash} golden=${GOLDEN}`);
  expect(d.lastRunHash).toBe(GOLDEN);
  expect(errors).toEqual([]);
});

test("URL query switches: quality, seed, screenshot download, dump-state and exit-after", async ({ page }) => {
  const errors = watchErrors(page);
  const logs = [];
  page.on("console", (m) => logs.push(m.text()));
  const download = page.waitForEvent("download", { timeout: 60_000 });
  await page.goto("/?quality=low&seed=7&screenshot=parble-shot.png&screenshot-after=3&exit-after=4&dump-state");
  const dl = await download;
  expect(dl.suggestedFilename()).toBe("parble-shot.png");
  await page.waitForFunction(() => window.__parble?.exited === true, null, { timeout: 60_000 });
  const d = await diag(page);
  console.log("__parble after exit:", JSON.stringify(d));
  expect(d.quality).toBe("low");
  expect(d.seed).toBe(7);
  expect(d.screenshot).toMatchObject({ state: "done", ok: true });
  await expect.poll(() => logs.some((t) => t.includes("parble state: {"))).toBe(true);
  expect(errors).toEqual([]);
});
