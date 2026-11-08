// Pause and slow motion (crates/parble-app/src/playback.rs) in GPU Chromium: P pauses the sim with the
// HUD showing it, N steps one 60 Hz sample (4 ticks), `,` and `.` walk the time scale, fly still moves
// the camera while paused, and `?paused=1&timescale=0.25` start that way.
import { expect, test } from "@playwright/test";

const BAD_TEXT = (t) => /%cERROR%c|\bERROR\b/.test(t) || /uncaptured|invalid|validation|panicked/i.test(t);

function watchErrors(page) {
  const errors = [];
  page.on("console", (m) => { if (m.type() === "error" || BAD_TEXT(m.text())) errors.push(m.text()); });
  page.on("pageerror", (e) => errors.push(String(e)));
  return errors;
}

const state = (page) => page.evaluate(() => ({ tick: window.__parble.tick, playback: window.__parble.playback, eye: window.__parble.camera.eye }));
const hud = (page) => page.evaluate(() => document.getElementById("parble-hud")?.textContent ?? "");

test("P pauses, N steps, comma and period change the scale, fly works while paused", async ({ page }) => {
  const errors = watchErrors(page);
  // The editor never restarts its run, so the tick only moves forward.
  await page.goto("/?scenario=editor&quality=low&mute=1");
  await page.waitForFunction(() => window.__parble?.ready === true && window.__parble.frame > 10, null, { timeout: 60_000 });
  const canvas = page.locator("canvas#parble");
  const box = await canvas.boundingBox();
  await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);

  await page.keyboard.press("p");
  await expect.poll(async () => (await state(page)).playback.paused, { timeout: 5_000 }).toBe(true);
  const held = await state(page);
  await page.waitForTimeout(500);
  expect((await state(page)).tick).toBe(held.tick);
  await expect.poll(() => hud(page), { timeout: 5_000 }).toMatch(/  paused$/);

  await page.keyboard.press("n");
  await expect.poll(async () => (await state(page)).tick, { timeout: 5_000 }).toBe(held.tick + 4);

  // One press per wait: two presses inside one slow frame read as a single just-pressed key.
  const scale = async () => (await state(page)).playback.timeScale;
  await page.keyboard.press("Comma");
  await expect.poll(scale, { timeout: 5_000 }).toBe(0.5);
  await page.keyboard.press("Comma");
  await expect.poll(scale, { timeout: 5_000 }).toBe(0.25);
  await expect.poll(() => hud(page), { timeout: 5_000 }).toMatch(/  paused  0\.25x$/);

  // Fly still moves the eye while the sim is paused.
  await page.keyboard.press("f");
  await expect.poll(() => page.evaluate(() => window.__parble.camera.mode), { timeout: 5_000 }).toBe("fly");
  const before = (await state(page)).eye;
  // Held until the eye has moved: at host load a 500 ms hold can span no rendered frame.
  const moved = async () => {
    const eye = (await state(page)).eye;
    return Math.hypot(...eye.map((v, i) => v - before[i]));
  };
  await page.keyboard.down("w");
  await expect.poll(moved, { timeout: 10_000 }).toBeGreaterThan(0.05);
  await page.keyboard.up("w");
  const after = await state(page);
  expect(after.tick).toBe(held.tick + 4);
  await page.keyboard.press("Escape");

  // Resumed at 0.25x: about 60 ticks per real second instead of 240.
  await page.keyboard.press("p");
  await expect.poll(async () => (await state(page)).playback.paused, { timeout: 5_000 }).toBe(false);
  const t0 = await page.evaluate(() => [window.__parble.tick, performance.now()]);
  await page.waitForTimeout(1_000);
  const t1 = await page.evaluate(() => [window.__parble.tick, performance.now()]);
  const rate = ((t1[0] - t0[0]) * 1000) / (t1[1] - t0[1]);
  console.log(`0.25x tick rate ${rate.toFixed(1)}/s`);
  expect(rate).toBeGreaterThan(20);
  expect(rate).toBeLessThan(90);

  await page.keyboard.press("Period");
  await expect.poll(scale, { timeout: 5_000 }).toBe(0.5);
  await page.keyboard.press("Period");
  await expect.poll(scale, { timeout: 5_000 }).toBe(1);
  await expect.poll(() => hud(page), { timeout: 5_000 }).toMatch(/^fps \d+  tick \d+$/);
  expect(await page.evaluate(() => window.__parbleErrors)).toEqual([]);
  expect(errors).toEqual([]);
});

test("?paused=1&timescale=0.25 starts paused at quarter speed and is ready", async ({ page }) => {
  const errors = watchErrors(page);
  await page.goto("/?scenario=ramp&quality=low&mute=1&paused=1&timescale=0.25");
  await page.waitForFunction(() => window.__parble?.ready === true && window.__parble.frame > 10, null, { timeout: 60_000 });
  const s = await state(page);
  expect(s.playback).toEqual({ paused: true, timeScale: 0.25 });
  expect(s.tick).toBe(0);
  await expect.poll(() => hud(page), { timeout: 5_000 }).toMatch(/  paused  0\.25x$/);
  expect(errors).toEqual([]);
});
