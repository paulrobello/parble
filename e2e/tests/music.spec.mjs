// P4-T5 music player and mixer in GPU Chromium (muted: ?mute=1 and --mute-audio; the gain graph still
// runs). The loop test seeks the playing cue to 1 s before its loop end three times and watches each
// wrap: the same source keeps playing with `loop` set, the reported position jumps back to the loop
// start, and an AnalyserNode on the music bus never reads silence across the wrap. The mixer test
// moves the music slider, sees the music bus fall silent, and reads the setting back after a reload.
//
// The 10-minute soak (P4-T5 verify) is the last test, skipped unless MUSIC_SOAK_S is set:
//   make music-soak                       # MUSIC_SOAK_S=600 on music_run_flow
//   make music-soak MUSIC_SOAK_S=900 MUSIC_SOAK_STATE=night
import { expect, test } from "@playwright/test";

const BAD_TEXT = (t) => /%cERROR%c|\bERROR\b/.test(t) || /uncaptured|invalid|validation|panicked/i.test(t);
/** RMS below this on the music bus is a gap (the quietest loop body windows sit near 1e-2). */
const SILENT_RMS = 1e-4;

function watch(page) {
  const errors = [];
  page.on("console", (m) => { if (m.type() === "error" || BAD_TEXT(m.text())) errors.push(m.text()); });
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("requestfailed", (r) => errors.push(`request failed: ${r.url()}`));
  return errors;
}

const stats = (page) => page.evaluate(() => window.__parbleAudio.stats());

async function openMusic(page, state) {
  await page.goto(`/?mute=1&music=${state}`);
  await page.waitForFunction(() => window.__parble?.ready === true, null, { timeout: 60_000 });
  await page.locator("canvas#parble").click({ position: { x: 20, y: 20 } });
  await page.waitForFunction(() => {
    const s = window.__parbleAudio.stats();
    return s.state === "running" && s.music.cue !== null && s.music.bufferDuration > 0;
  }, null, { timeout: 30_000 });
  return stats(page);
}

test("the music player loops gaplessly across its wrap point", async ({ page }) => {
  const errors = watch(page);
  const first = await openMusic(page, "run");
  const catalog = await page.evaluate(async () => (await fetch("web/ui/audio/music.json")).json());
  const cue = catalog.cues[first.music.cue];
  console.log(`music: ${first.music.cue} sampleRate ${first.sampleRate} loop ${first.music.loopStart}-${first.music.loopEnd} s`);
  expect(first.music).toMatchObject({ state: "run", cue: "music_run_flow", loop: true, forced: "run", loadErrors: 0 });
  expect(first.music.loopStart).toBeCloseTo(cue.loopStart / cue.sampleRate, 6);
  expect(first.music.loopEnd).toBeCloseTo(cue.loopEnd / cue.sampleRate, 6);
  expect(Math.abs(first.music.bufferDuration - cue.samples / cue.sampleRate)).toBeLessThan(0.001);
  expect(first.music.buffers).toBeLessThanOrEqual(2);

  for (let pass = 0; pass < 3; pass++) {
    expect(await page.evaluate((s) => window.__parbleAudio.musicSeek(s), first.music.loopEnd - 1)).toBe(true);
    const seeked = (await stats(page)).music;
    const samples = [];
    const end = Date.now() + 2_500;
    while (Date.now() < end) {
      samples.push(await page.evaluate(() => ({ m: window.__parbleAudio.stats().music, level: window.__parbleAudio.musicLevel() })));
      await page.waitForTimeout(50);
    }
    const last = samples.at(-1).m;
    const minLevel = Math.min(...samples.slice(2).map((s) => s.level));
    console.log(`wrap ${pass + 1}: source ${seeked.source} -> ${last.source}, position ${seeked.position.toFixed(3)} -> ${last.position.toFixed(3)}, wraps ${last.wraps}, min RMS ${minLevel.toExponential(2)}`);
    expect(samples.every((s) => s.m.source === seeked.source && s.m.loop)).toBe(true);
    expect(last.wraps).toBe(1);
    expect(last.position).toBeGreaterThan(last.loopStart);
    expect(last.position).toBeLessThan(last.loopStart + 2);
    expect(minLevel).toBeGreaterThan(SILENT_RMS);
  }
  const s = await stats(page);
  expect(s.music.loadErrors).toBe(0);
  expect(s.errors).toBe(0);
  expect(errors).toEqual([]);
});

test("without ?music the director plays the run cue while the ramp marble rolls", async ({ page }) => {
  const errors = watch(page);
  await page.goto("/?mute=1");
  await page.waitForFunction(() => window.__parble?.ready === true, null, { timeout: 60_000 });
  await page.locator("canvas#parble").click({ position: { x: 20, y: 20 } });
  await page.waitForFunction(() => window.__parbleAudio.stats().music.cue !== null, null, { timeout: 30_000 });
  expect((await stats(page)).music).toMatchObject({ forced: null, state: "run", cue: "music_run_flow", loop: true });
  expect(errors).toEqual([]);
});

test("the mixer sets the music bus and persists across a reload", async ({ page }) => {
  const errors = watch(page);
  await openMusic(page, "night");
  await page.evaluate(() => localStorage.removeItem("parble.mixer/1"));
  await page.waitForFunction(() => window.__parbleAudio.musicLevel() > 1e-3, null, { timeout: 10_000 });
  await page.locator("#parble-mixer-toggle").click();
  await expect(page.locator("#parble-mixer-music")).toBeVisible();
  await page.locator("#parble-mixer-music").fill("0");
  await page.locator("#parble-mixer-sfx").fill("40");
  expect((await stats(page)).mixer).toEqual({ master: 1, music: 0, sfx: 0.4, muted: false });
  await page.waitForFunction(() => window.__parbleAudio.musicLevel() < 1e-5, null, { timeout: 5_000 });

  await openMusic(page, "night");
  expect((await stats(page)).mixer).toEqual({ master: 1, music: 0, sfx: 0.4, muted: false });
  await page.locator("#parble-mixer-toggle").click();
  await expect(page.locator("#parble-mixer-music")).toHaveValue("0");
  await page.waitForTimeout(500);
  expect(await page.evaluate(() => window.__parbleAudio.musicLevel())).toBeLessThan(1e-5);
  await page.locator("#parble-mixer-music").fill("100");
  await page.waitForFunction(() => window.__parbleAudio.musicLevel() > 1e-3, null, { timeout: 5_000 });
  await page.evaluate(() => localStorage.removeItem("parble.mixer/1"));
  expect(errors).toEqual([]);
});

const SOAK_S = Number(process.env.MUSIC_SOAK_S ?? 0);
const SOAK_STATE = process.env.MUSIC_SOAK_STATE ?? "run";

test("soak: one loop plays for MUSIC_SOAK_S seconds with no gap, restart or error", async ({ page }) => {
  test.skip(!SOAK_S, "set MUSIC_SOAK_S (make music-soak)");
  test.setTimeout((SOAK_S + 180) * 1000);
  const errors = watch(page);
  const first = (await openMusic(page, SOAK_STATE)).music;
  const len = first.loopEnd - first.loopStart;
  const t0 = Date.now();
  let samples = 0;
  let silent = 0;
  let minLevel = Number.POSITIVE_INFINITY;
  let prevWraps = 0;
  let last = first;
  let source = first.source;
  let crossfades = first.crossfades;
  const cues = new Set([first.cue]);
  while (Date.now() - t0 < SOAK_S * 1000) {
    await page.waitForTimeout(250);
    const { m, level } = await page.evaluate(() => ({ m: window.__parbleAudio.stats().music, level: window.__parbleAudio.musicLevel() }));
    samples++;
    if (level < SILENT_RMS) {
      silent++;
      console.log(`soak ${((Date.now() - t0) / 1000).toFixed(1)} s: silent RMS ${level} at position ${m.position.toFixed(3)}`);
    }
    minLevel = Math.min(minLevel, level);
    // A playlist state (build alternates calm and playful) moves to its next cue by a crossfade;
    // any other source change is a restart.
    if (m.source !== source) {
      expect(m.crossfades).toBeGreaterThan(crossfades);
      console.log(`soak ${((Date.now() - t0) / 1000).toFixed(0)} s: playlist crossfade to ${m.cue}`);
      source = m.source;
      crossfades = m.crossfades;
      cues.add(m.cue);
      prevWraps = m.wraps;
    }
    expect(m.loop).toBe(true);
    expect(m.wraps).toBeGreaterThanOrEqual(prevWraps);
    if (m.wraps !== prevWraps) console.log(`soak ${((Date.now() - t0) / 1000).toFixed(0)} s: wrap ${m.wraps} at position ${m.position.toFixed(3)}`);
    prevWraps = m.wraps;
    last = m;
  }
  const s = await stats(page);
  const elapsed = (Date.now() - t0) / 1000;
  console.log(
    `soak done: ${elapsed.toFixed(0)} s on ${first.cue} (loop ${len.toFixed(2)} s), ${last.wraps} wraps, ` +
      `cues ${[...cues].join("+")}, ${samples} samples, ${silent} silent, min RMS ${minLevel.toExponential(2)}, starts ${s.music.starts}, ` +
      `errors ${s.errors}, loadErrors ${s.music.loadErrors}, console errors ${errors.length}`,
  );
  if (cues.size === 1) expect(last.wraps).toBeGreaterThanOrEqual(Math.floor((elapsed - (first.loopEnd - first.position)) / len));
  expect(silent).toBe(0);
  expect(s.music.starts).toBe(1 + s.music.crossfades - first.crossfades);
  expect(s.music.loadErrors).toBe(0);
  expect(s.errors).toBe(0);
  expect(errors).toEqual([]);
});
