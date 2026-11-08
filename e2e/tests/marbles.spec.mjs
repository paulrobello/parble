// P1-T6 marble looks in GPU Chromium: every marble type on the Low (fallback vane shader) and High
// (screen-space transmission) tiers of the `closeup` scenario renders a lit frame with no console
// errors or WebGPU validation messages. Screenshots go to target/shots/web-marbles/ for review.
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
    let green = 0;
    for (let i = 0; i < cd.length; i += 4) {
      for (let k = 0; k < 3; k++) centre[k] += cd[i + k];
      if (cd[i + 1] > cd[i] + 30) green++;
    }
    const n = cd.length / 4;
    const px = d.length / 4;
    return { nonBlack: nonBlack / px, lit: lit / px, colors: colors.size, centre: centre.map((v) => Math.round(v / n)), green: green / n };
  }, png.toString("base64"));
}

mkdirSync(SHOTS, { recursive: true });

for (const marble of ["cats-eye", "glazed", "steel"]) {
  for (const quality of ["low", "high"]) {
    test(`${marble} on ${quality} renders without errors`, async ({ page }) => {
      const errors = watchErrors(page);
      await page.goto(`/?scenario=closeup&marble=${marble}&quality=${quality}`);
      await page.waitForFunction(() => window.__parble?.ready === true, null, { timeout: 60_000 });
      const d = await page.evaluate(() => window.__parble);
      expect(d.scenario).toBe("closeup");
      expect(d.marble).toBe(marble);
      expect(d.quality).toBe(quality);
      let stats = null;
      await expect.poll(async () => {
        const png = await page.locator("canvas#parble").screenshot();
        stats = await canvasStats(png, page);
        return stats.nonBlack > 0.5 && stats.lit > 0.02 && stats.colors > 16;
      }, { timeout: 30_000, intervals: [1_000] }).toBe(true);
      // Let pipelines settle, then keep the frame for review.
      await page.waitForTimeout(2_000);
      const png = await page.locator("canvas#parble").screenshot({ path: new URL(`${marble}-${quality}.png`, SHOTS).pathname });
      stats = await canvasStats(png, page);
      console.log(`${marble}/${quality}: ${JSON.stringify(stats)}`);
      // The marble covers the centre. A cat's eye must show its green vane (wood never has g > r);
      // the others must not read as the wood floor (r ≫ b) alone.
      if (marble === "cats-eye") expect(stats.green).toBeGreaterThan(0.02);
      else expect(stats.centre[0] - stats.centre[2]).toBeLessThan(60);
      expect(await page.evaluate(() => window.__parbleErrors)).toEqual([]);
      expect(errors).toEqual([]);
    });
  }
}
