// Free-fly camera (crates/parble-render/src/camera.rs) in GPU Chromium, and the playroom kit run.
// kit2000 and lineup: click the canvas to focus it, F enters fly, held W moves the eye, a mouse drag
// turns the view, the wheel changes the speed, Esc returns to the director. Keys typed while a HUD
// control has focus never reach the camera. The playroom completes its kit run on the committed
// native hash.
import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";

const PLAYROOM_HASH = readFileSync(
  new URL("../../crates/parble-sim/tests/golden/playroom_4s.hash", import.meta.url),
  "utf8",
).trim();
const BAD_TEXT = (t) => /%cERROR%c|\bERROR\b/.test(t) || /uncaptured|invalid|validation|panicked/i.test(t);

function watchErrors(page) {
  const errors = [];
  page.on("console", (m) => { if (m.type() === "error" || BAD_TEXT(m.text())) errors.push(m.text()); });
  page.on("pageerror", (e) => errors.push(String(e)));
  return errors;
}

const camera = (page) => page.evaluate(() => window.__parble.camera);
const dist = (a, b) => Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);

for (const scenario of ["kit2000", "lineup"]) {
  test(`${scenario}: fly camera moves and looks with keyboard and mouse, Esc returns`, async ({ page }) => {
    const errors = watchErrors(page);
    await page.goto(`/?mute=1&scenario=${scenario}&quality=low`);
    await page.waitForFunction(() => window.__parble?.ready === true && window.__parble.frame > 10, null, {
      timeout: 60_000,
    });
    const canvas = page.locator("canvas#parble");
    const box = await canvas.boundingBox();
    const cx = box.x + box.width / 2;
    const cy = box.y + box.height / 2;

    // Keys typed into the HUD stay there.
    await page.focus("#parble-export");
    await page.keyboard.press("f");
    await page.waitForTimeout(300);
    expect((await camera(page)).mode).toBe("director");

    await page.mouse.click(cx, cy);
    expect(await page.evaluate(() => document.activeElement?.id)).toBe("parble");
    const start = await camera(page);
    await page.keyboard.press("f");
    await expect.poll(async () => (await camera(page)).mode, { timeout: 5_000 }).toBe("fly");
    const entered = await camera(page);
    // Entering fly keeps the view it took over.
    expect(dist(entered.eye, start.eye)).toBeLessThan(1e-3);

    await page.keyboard.down("w");
    await page.waitForTimeout(600);
    await page.keyboard.up("w");
    const moved = await camera(page);
    console.log(`${scenario} W: ${JSON.stringify(entered.eye)} -> ${JSON.stringify(moved.eye)}`);
    expect(dist(moved.eye, entered.eye)).toBeGreaterThan(0.05);
    // W flies along the view.
    const step = moved.eye.map((v, i) => v - entered.eye[i]);
    const along = step.reduce((s, v, i) => s + v * entered.forward[i], 0) / Math.hypot(...step);
    expect(along).toBeGreaterThan(0.95);

    await page.mouse.move(cx, cy);
    await page.mouse.down();
    await page.mouse.move(cx + 200, cy + 60, { steps: 12 });
    await page.mouse.up();
    await page.waitForTimeout(200);
    const turned = await camera(page);
    console.log(`${scenario} drag: ${JSON.stringify(moved.forward)} -> ${JSON.stringify(turned.forward)}`);
    expect(dist(turned.forward, moved.forward)).toBeGreaterThan(0.3);

    await page.mouse.wheel(0, -300);
    await expect.poll(async () => (await camera(page)).flySpeed, { timeout: 5_000 }).toBeGreaterThan(turned.flySpeed);

    await page.keyboard.press("Escape");
    await expect.poll(async () => (await camera(page)).mode, { timeout: 5_000 }).toBe("director");
    const back = await camera(page);
    expect(dist(back.eye, start.eye)).toBeLessThan(1e-3);
    expect(await page.evaluate(() => window.__parbleErrors)).toEqual([]);
    expect(errors).toEqual([]);
  });
}

test("playroom kit run completes on its committed hash", async ({ page }) => {
  const errors = watchErrors(page);
  await page.goto("/?mute=1&scenario=playroom&quality=low");
  await page.waitForFunction(() => window.__parble?.lastRunHash != null, null, { timeout: 60_000 });
  const d = await page.evaluate(() => window.__parble);
  console.log(`playroom lastRunHash=${d.lastRunHash} committed=${PLAYROOM_HASH} kit=${JSON.stringify({ pieces: d.kit.pieces, chunks: d.kit.chunks })}`);
  expect(d.lastRunHash).toBe(PLAYROOM_HASH);
  expect(d.marble).toBe("playroom");
  expect(d.marbles).toHaveLength(4);
  expect(d.kit.pieces).toBe(18);
  expect(errors).toEqual([]);
});
