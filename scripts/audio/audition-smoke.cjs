#!/usr/bin/env node
// Headless smoke test for audition/index.html.
// Usage: NODE_PATH=<dir containing playwright> node scripts/audio/audition-smoke.cjs [audition-dir]
// Serves the dir on a random loopback port, loads the page in Chromium, plays one SFX and
// one music track, picks one variation, exports, and fails on any console error.
"use strict";
const { spawn } = require("node:child_process");
const path = require("node:path");
const { chromium } = require("playwright");

const dir = path.resolve(process.argv[2] || path.join(__dirname, "../../audition"));
const port = 20000 + Math.floor(Math.random() * 20000);

(async () => {
  const server = spawn("python3", ["-m", "http.server", String(port), "--bind", "127.0.0.1",
    "--directory", dir], { stdio: "ignore" });
  const errors = [];
  let code = 1;
  try {
    await new Promise((r) => setTimeout(r, 800));
    const browser = await chromium.launch({ channel: "chromium", headless: true,
      args: ["--autoplay-policy=no-user-gesture-required"] });
    const page = await browser.newPage({ acceptDownloads: true });
    page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
    page.on("pageerror", (e) => errors.push(String(e)));
    await page.goto(`http://127.0.0.1:${port}/`);
    await page.waitForSelector("button.play");
    const manifest = await (await page.request.get(`http://127.0.0.1:${port}/sfx/manifest.json`)).json();
    const expected = manifest.items.filter((i) => i.files).length;
    const buttons = await page.locator("button.play").count();
    const status = await page.textContent("#status");
    const loopDefaults = await page.$$eval("section.cue", (cues) => cues.map((c) => {
      const box = c.querySelector(".var input[type=checkbox]"); return [c.dataset.cue, box.checked];
    }));
    const firstPlay = page.locator("button.play").first();
    await firstPlay.click();
    await page.waitForFunction(() => document.querySelector("button.play.playing"), null, { timeout: 5000 });
    const music = page.locator("section[data-cue^=music_] button.play").first();
    const musicCount = await page.locator("section[data-cue^=music_]").count();
    if (musicCount) {
      await music.click();
      await page.waitForTimeout(1200);
    }
    const timeText = await page.locator("section[data-cue^=music_] .time").first().textContent().catch(() => "");
    await page.click("#stop-all");
    const playingAfterStop = await page.locator("button.play.playing").count();
    await page.locator("input.pick").first().check();
    const [download] = await Promise.all([page.waitForEvent("download"), page.click("#export")]);
    const exported = JSON.parse(await page.inputValue("#export-text"));
    await page.reload();
    await page.waitForSelector("input.pick");
    const pickPersisted = await page.locator("input.pick").first().isChecked();
    await browser.close();
    const loopCues = loopDefaults.filter(([c, on]) => /^(roll_|tunnel_|music_)/.test(c) !== on);
    const result = { expected, buttons, status, musicCount, timeText, playingAfterStop,
      download: download.suggestedFilename(), exportedKeys: Object.keys(exported),
      pickPersisted, loopDefaultMismatches: loopCues, consoleErrors: errors };
    console.log(JSON.stringify(result, null, 1));
    code = buttons === expected && errors.length === 0 && playingAfterStop === 0 && pickPersisted
      && "notes" in exported && loopCues.length === 0 && (!musicCount || /\d:\d/.test(timeText)) ? 0 : 1;
  } finally {
    server.kill();
  }
  process.exit(code);
})().catch((e) => { console.error(e); process.exit(1); });
