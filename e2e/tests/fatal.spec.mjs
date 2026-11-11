// Fatal page errors (index.html): a WebGPU device lost while loading must surface as
// `window.__parble.fatal` instead of an endless READY wait, and a device the page destroys itself
// (the title's GPU check) must not.
import { expect, test } from "@playwright/test";

const fatal = (page) => page.evaluate(() => window.__parble?.fatal ?? window.__parbleFatal ?? null);

test("a device lost while loading is reported as fatal, not a READY wait", async ({ page }) => {
  await page.goto("/?scenario=showcase&mute=1&quality=high&gpu_lose_ms=300");
  await page.waitForFunction(() => (window.__parble?.fatal ?? window.__parbleFatal) != null, null, { timeout: 30_000 });
  const f = await fatal(page);
  console.log("fatal:", JSON.stringify(f));
  expect(f.message).toMatch(/^WebGPU device lost \(destroyed\)/);
  expect(f.atMs).toBeGreaterThan(0);
  const s = await page.evaluate(() => ({ ready: window.__parble?.ready ?? null, errors: window.__parbleErrors }));
  expect(s.ready).not.toBe(true);
  expect(s.errors.some((e) => e.startsWith("WebGPU device lost"))).toBe(true);
  expect(s.errors.filter((e) => /RangeError/.test(e))).toEqual([]);
});

test("a device the page destroys itself is not fatal", async ({ page }) => {
  await page.goto("/?scenario=showcase&mute=1");
  await page.waitForFunction(() => window.__parble?.ready === true, null, { timeout: 60_000 });
  const mapped = await page.evaluate(async () => {
    const d = await (await navigator.gpu.requestAdapter()).requestDevice();
    d.destroy();
    await d.lost;
    // A lost device still maps a buffer at creation to its full size (the index.html shim for Safari).
    const b = d.createBuffer({ size: 64, usage: GPUBufferUsage.VERTEX, mappedAtCreation: true });
    const n = b.getMappedRange().byteLength;
    await new Promise((r) => setTimeout(r, 200));
    return n;
  });
  expect(mapped).toBe(64);
  expect(await fatal(page)).toBeNull();
  expect(await page.evaluate(() => window.__parbleErrors)).toEqual([]);
});
