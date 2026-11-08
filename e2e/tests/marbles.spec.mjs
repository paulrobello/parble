// P1-T6 / P5-T1 marble looks in GPU Chromium: every marble type on the Low (fallback shaders) and High
// (screen-space transmission) tiers of the `closeup` scenario, two P5-T1 sizes and the `lineup`
// scenario each render a lit frame with no console errors or WebGPU validation messages. Screenshots go to target/shots/web-marbles/ for review.
import { mkdirSync } from "node:fs";
import { expect, test } from "@playwright/test";

const SHOTS = new URL("../../target/shots/web-marbles/", import.meta.url);
// Bevy's wasm logger prints ERROR through console.log, styled as "%cERROR%c". wgpu reports
// validation failures as "uncaptured" / "invalid" console messages.
const BAD_TEXT = (t) => /%cERROR%c|\bERROR\b/.test(t) || /uncaptured|invalid|validation|panicked/i.test(t);

function watchErrors(page) {
  const errors = [];
  page.on("console", (m) => { if (m.type() === "error" || BAD_TEXT(m.text())) errors.push(m.text()); });
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("requestfailed", (r) => errors.push(`request failed: ${r.url()}`));
  return errors;
}

// Lit-scene stats of the canvas, plus the mean colour of the central 20 % (where the marble sits) and
// the share of its pixels with green above red by 30, which the wood floor never has.
async function canvasStats(png, page) {
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
    const w = Math.round(c.width * 0.2), h = Math.round(c.height * 0.2);
    const cd = g.getImageData((c.width - w) >> 1, (c.height - h) >> 1, w, h).data;
    const centre = [0, 0, 0];
    let green = 0, blue = 0;
    const centreColors = new Set();
    for (let i = 0; i < cd.length; i += 4) {
      for (let k = 0; k < 3; k++) centre[k] += cd[i + k];
      if (cd[i + 1] > cd[i] + 30) green++;
      if (cd[i + 2] > cd[i] + 30) blue++;
      centreColors.add((cd[i] >> 4) << 8 | (cd[i + 1] >> 4) << 4 | (cd[i + 2] >> 4));
    }
    const n = cd.length / 4;
    const px = d.length / 4;
    return { nonBlack: nonBlack / px, lit: lit / px, colors: colors.size, centre: centre.map((v) => Math.round(v / n)), green: green / n, blue: blue / n, centreColors: centreColors.size };
  }, png.toString("base64"));
}

mkdirSync(SHOTS, { recursive: true });

// Load `query`, wait for a lit frame, keep it as `name`.png and return the diag state and stats.
async function render(page, query, name) {
  await page.goto(`/?${query}`);
  await page.waitForFunction(() => window.__parble?.ready === true, null, { timeout: 60_000 });
  let stats = null;
  await expect.poll(async () => {
    const png = await page.locator("canvas#parble").screenshot();
    stats = await canvasStats(png, page);
    return stats.nonBlack > 0.5 && stats.lit > 0.02 && stats.colors > 16;
  }, { timeout: 30_000, intervals: [1_000] }).toBe(true);
  // Let pipelines settle, then keep the frame for review.
  await page.waitForTimeout(2_000);
  const png = await page.locator("canvas#parble").screenshot({ path: new URL(`${name}.png`, SHOTS).pathname });
  stats = await canvasStats(png, page);
  console.log(`${name}: ${JSON.stringify(stats)}`);
  return { d: await page.evaluate(() => window.__parble), stats };
}

// The marble covers the centre of the close-up, so each type has a colour the wood floor (r ≫ b,
// g < r) never shows there.
const CENTRE_CHECK = {
  "cats-eye": (s) => expect(s.green).toBeGreaterThan(0.02),
  glazed: (s) => expect(s.centre[0] - s.centre[2]).toBeLessThan(60),
  steel: (s) => expect(s.centre[0] - s.centre[2]).toBeLessThan(60),
  swirl: (s) => expect(s.blue).toBeGreaterThan(0.02),
  // Many-coloured streaks: far more distinct centre colours than a plain body (glazed, steel < 170).
  onionskin: (s) => expect(s.centreColors).toBeGreaterThan(200),
  opal: (s) => expect(s.centre[2]).toBeGreaterThan(s.centre[0]),
  glow: (s) => expect(s.centre[1]).toBeGreaterThan(s.centre[0]),
};

for (const marble of Object.keys(CENTRE_CHECK)) {
  for (const quality of ["low", "high"]) {
    test(`${marble} on ${quality} renders without errors`, async ({ page }) => {
      const errors = watchErrors(page);
      const { d, stats } = await render(page, `scenario=closeup&marble=${marble}&quality=${quality}`, `${marble}-${quality}`);
      expect(d.scenario).toBe("closeup");
      expect(d.marble).toBe(marble);
      expect(d.quality).toBe(quality);
      CENTRE_CHECK[marble](stats);
      expect(await page.evaluate(() => window.__parbleErrors)).toEqual([]);
      expect(errors).toEqual([]);
    });
  }
}

// P5-T1 sizes over the URL, and the lineup scenario with every type in one frame.
for (const [marble, size, quality] of [["swirl", "mini", "low"], ["opal", "shooter", "high"]]) {
  test(`${size} ${marble} on ${quality} renders without errors`, async ({ page }) => {
    const errors = watchErrors(page);
    const { d } = await render(page, `scenario=closeup&marble=${marble}&marble-size=${size}&quality=${quality}`, `${marble}-${size}-${quality}`);
    expect(d.marble).toBe(marble);
    expect(d.marbleSize).toBe(size);
    expect(await page.evaluate(() => window.__parbleErrors)).toEqual([]);
    expect(errors).toEqual([]);
  });
}

for (const quality of ["low", "high"]) {
  test(`lineup on ${quality} renders without errors`, async ({ page }) => {
    const errors = watchErrors(page);
    const { d } = await render(page, `scenario=lineup&quality=${quality}&dof=0`, `lineup-${quality}`);
    expect(d.scenario).toBe("lineup");
    expect(d.marbles.length).toBe(9);
    expect(await page.evaluate(() => window.__parbleErrors)).toEqual([]);
    expect(errors).toEqual([]);
  });
}
