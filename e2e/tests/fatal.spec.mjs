// Fatal page errors (index.html): a WebGPU device lost past the app's recovery bound must surface as
// `window.__parble.fatal` instead of an endless READY wait or a rebuild loop, and a device the page
// destroys itself (the title's GPU check) must not.
import { expect, test } from "@playwright/test";

const fatal = (page) => page.evaluate(() => window.__parble?.fatal ?? window.__parbleFatal ?? null);

test("a device that keeps getting lost ends on the fatal path after 3 recoveries, with no loop", async ({ page }) => {
  // Every device, the rebuilt ones too, is lost 300 ms after it is created: 3 losses recover
  // (gpu-recovery.spec.mjs), the 4th within 30 s is fatal.
  await page.goto("/?scenario=showcase&mute=1&quality=high&gpu_lose_ms=300");
  await page.waitForFunction(() => (window.__parble?.fatal ?? window.__parbleFatal) != null, null, { timeout: 60_000 });
  const f = await fatal(page);
  console.log("fatal:", JSON.stringify(f));
  expect(f.message).toMatch(/^WebGPU device lost \(destroyed\)/);
  expect(f.recovery).toMatch(/not recovering/);
  expect(f.atMs).toBeGreaterThan(0);
  const s = await page.evaluate(() => ({
    ready: window.__parble?.ready ?? null,
    recoveries: window.__parble?.gpuRecoveries ?? null,
    gpu: { ...window.__parbleGpu },
    errors: window.__parbleErrors,
  }));
  expect(s.gpu.losses).toBe(4);
  expect(s.gpu.recovered).toBe(3);
  expect(s.errors.filter((e) => e.startsWith("WebGPU device lost"))).toHaveLength(4);
  expect(s.errors.filter((e) => /RangeError/.test(e))).toEqual([]);
  // No retry loop: the app stopped asking for devices and rendering.
  await page.waitForTimeout(3_000);
  const later = await page.evaluate(() => ({ gpu: { ...window.__parbleGpu }, frame: window.__parble?.pipelines?.renderFrame }));
  expect(later.gpu.devices).toBe(s.gpu.devices);
  expect(later.gpu.losses).toBe(4);
  await page.waitForTimeout(1_000);
  expect(await page.evaluate(() => window.__parble?.pipelines?.renderFrame)).toBe(later.frame);
  expect(await page.evaluate(() => window.__parbleGpu.devices)).toBe(s.gpu.devices);
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
