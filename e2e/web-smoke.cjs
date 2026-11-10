#!/usr/bin/env node
// P0-T5 web shell smoke (local only, never CI): loads the page in GPU Chromium, waits for the canvas
// to show a lit scene and the HUD (web/ui) a moving tick, fails on any console error, and prints the
// WebGPU adapter.
// Usage: node e2e/web-smoke.cjs [url] [screenshot.png]   (after npm ci --prefix e2e)
//   url defaults to serving ./target/web-dist (make build-web) on a random loopback port.
// PW_GPU=swiftshader opts into the CPU renderer; the default is the platform GPU (Metal on macOS).
"use strict";
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");
const { chromium } = require("@playwright/test");

const { GPU_ARGS } = require("./gpu-args.cjs");
// Bevy's wasm logger prints ERROR through console.log, styled as "%cERROR%c" (hex-world e2e).
const BAD_TEXT = (t) => /%cERROR%c|\bERROR\b/.test(t) || /uncaptured|invalid|panicked/i.test(t);

(async () => {
  let url = process.argv[2];
  const shot = process.argv[3];
  let server = null;
  if (!url) {
    const port = 20000 + Math.floor(Math.random() * 20000);
    server = spawn("python3", ["-m", "http.server", String(port), "--bind", "127.0.0.1",
      "--directory", path.resolve(__dirname, "../target/web-dist")], { stdio: "ignore" });
    await new Promise((r) => setTimeout(r, 800));
    url = `http://127.0.0.1:${port}/?title=0`;
  }
  const errors = [];
  let code = 1;
  try {
    const browser = await chromium.launch({ channel: "chromium", headless: true, args: [...GPU_ARGS, "--mute-audio"] });
    const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
    page.on("console", (m) => { if (m.type() === "error" || BAD_TEXT(m.text())) errors.push(m.text()); });
    page.on("pageerror", (e) => errors.push(String(e)));
    page.on("requestfailed", (r) => errors.push(`request failed: ${r.url()}`));
    await page.goto(url);
    await page.waitForTimeout(1_000);
    if (page.url().includes("unsupported.html")) throw new Error("redirected to unsupported.html: no WebGPU adapter");

    // Poll until the canvas shows enough non-backdrop pixels (the wasm compiles, then pipelines build).
    let stats = null;
    const deadline = Date.now() + 60_000;
    while (Date.now() < deadline) {
      await page.waitForTimeout(1_000);
      const png = await page.locator("canvas#parble").screenshot();
      stats = await page.evaluate(async (b64) => {
        const img = new Image();
        img.src = "data:image/png;base64," + b64;
        await img.decode();
        const c = document.createElement("canvas");
        c.width = img.width; c.height = img.height;
        const g = c.getContext("2d");
        g.drawImage(img, 0, 0);
        const d = g.getImageData(0, 0, c.width, c.height).data;
        let nonBlack = 0, lit = 0, colors = new Set();
        for (let i = 0; i < d.length; i += 4) {
          const m = Math.max(d[i], d[i + 1], d[i + 2]);
          if (m > 8) nonBlack++;
          if (m > 60) lit++;
          colors.add((d[i] >> 4) << 8 | (d[i + 1] >> 4) << 4 | (d[i + 2] >> 4));
        }
        const px = d.length / 4;
        return { w: c.width, h: c.height, nonBlack: nonBlack / px, lit: lit / px, colors: colors.size };
      }, png.toString("base64"));
      if (stats.nonBlack > 0.5 && stats.lit > 0.02 && stats.colors > 16) {
        // Late assets (glTF scenes load asynchronously) get 1.5 s more before the saved shot.
        if (shot) {
          await page.waitForTimeout(1_500);
          fs.mkdirSync(path.dirname(shot), { recursive: true });
          fs.writeFileSync(shot, await page.locator("canvas#parble").screenshot());
        }
        break;
      }
    }
    console.log("canvas:", JSON.stringify(stats));
    // P0-T7 HUD overlay (web/ui): fps and tick from the wasm bridge, and the tick moving. The ramp
    // restarts every 10 s, so the check is "changed", not "increased".
    const hudText = () => page.evaluate(() => document.getElementById("parble-hud")?.textContent ?? null);
    const hudA = await hudText();
    await page.waitForTimeout(600);
    const hudB = await hudText();
    const HUD = /^fps \d+  tick \d+$/;
    const hud = { a: hudA, b: hudB, ok: HUD.test(hudA ?? "") && HUD.test(hudB ?? "") && hudA !== hudB };
    console.log("hud:", JSON.stringify(hud));
    const adapter = await page.evaluate(async () => {
      const a = await navigator.gpu.requestAdapter();
      const i = a && a.info;
      const gl = document.createElement("canvas").getContext("webgl");
      const ext = gl && gl.getExtension("WEBGL_debug_renderer_info");
      return {
        webgpu: i ? { vendor: i.vendor, architecture: i.architecture, device: i.device, description: i.description } : null,
        unmaskedRenderer: ext ? gl.getParameter(ext.UNMASKED_RENDERER_WEBGL) : null,
        wgslCompat: window.__wgslCompat || null,
        frameLatency: window.__frameLatency || null,
        build: window.__PARBLE_BUILD || null,
        pageErrors: window.__parbleErrors || null,
        state: window.__parble ? {
          scenario: window.__parble.scenario, view: window.__parble.view, quality: window.__parble.quality,
          shadowLights: window.__parble.shadowLights, shadowLightCap: window.__parble.shadowLightCap,
        } : null,
      };
    });
    console.log("adapter:", JSON.stringify(adapter));
    await browser.close();
    const fails = [];
    if (!stats || stats.nonBlack <= 0.5 || stats.lit <= 0.02 || stats.colors <= 16) fails.push("canvas never showed a lit scene");
    if (!adapter.webgpu) fails.push("no WebGPU adapter");
    if (!hud.ok) fails.push("HUD missing, malformed or not ticking");
    if (!Array.isArray(adapter.pageErrors)) fails.push("window.__parbleErrors missing (index.html error log)");
    else if (adapter.pageErrors.length) fails.push(`${adapter.pageErrors.length} page-logged error(s)`);
    if (/swiftshader/i.test(JSON.stringify(adapter)) && process.env.PW_GPU !== "swiftshader") fails.push("SwiftShader adapter in a GPU run");
    console.log(`console errors: ${errors.length}`);
    for (const e of errors) console.log("  error:", e);
    if (errors.length) fails.push(`${errors.length} console error(s)`);
    for (const f of fails) console.error("FAIL:", f);
    code = fails.length ? 1 : 0;
    if (!code) console.log("web-smoke: PASS");
  } catch (e) {
    console.error("FAIL:", e && e.stack ? e.stack : e);
    for (const m of errors) console.error("  error:", m);
  } finally {
    if (server) server.kill();
    process.exit(code);
  }
})();
