// P4-T6 (C2) perf trace: the main-thread time the audio update takes per frame on the stress track
// (stress2000: the allpieces run, 10 marbles, 2,000 pieces) at 2560 × 1440 High, as in the P1-T8
// perf gate. Opt-in (PARBLE_PERF=1, `make web-audio-perf`), since it is a measurement, not a smoke.
// window.__parbleAudio.timing records performance.now() around each fed audio frame (the engine's
// consume plus the music director and player update) for two full 30 s run cycles, with rAF frame
// times beside it. The peak meter is not started, so its reads are not in the numbers. The host's
// 1-minute load average must be ≤ 15 before and after; a sample outside that is reported as not taken.
// The result goes to target/perf/p4t6-audio.json.
import { mkdirSync, writeFileSync } from "node:fs";
import { cpus, loadavg } from "node:os";
import { expect, test } from "@playwright/test";
import { BUDGET_MS, LOAD_MAX, stats, waitForLoad } from "../perf-helpers.mjs";

const LOAD_WAIT_MS = Number(process.env.PARBLE_LOAD_WAIT_MS ?? 20 * 60_000);
const OUT = new URL("../../target/perf/p4t6-audio.json", import.meta.url);
/** Audio main-thread budget per frame at p95, ms: 12 % of the 16.7 ms frame (docs/audio/acceptance.md). */
const AUDIO_P95_MS = 2;
const TRACE_S = 60;

test.skip(process.env.PARBLE_PERF !== "1", "perf trace is opt-in: PARBLE_PERF=1");
test.setTimeout(LOAD_WAIT_MS + 240_000);
test.use({ viewport: { width: 2560, height: 1440 }, deviceScaleFactor: 1 });

test("the audio update stays inside its share of the frame on the stress track", async ({ page }) => {
  mkdirSync(new URL(".", OUT), { recursive: true });
  // PARBLE_PERF_DIAG=1 runs once without waiting for the load; the result is still marked not taken
  // unless the load was within LOAD_MAX before and after.
  const diagRun = process.env.PARBLE_PERF_DIAG === "1";
  const loadBefore = diagRun ? loadavg()[0] : await waitForLoad(LOAD_WAIT_MS);
  if (loadBefore > LOAD_MAX && !diagRun) {
    const result = { taken: false, reason: `1-min load ${loadBefore.toFixed(2)} > ${LOAD_MAX}`, loadBefore };
    writeFileSync(OUT, `${JSON.stringify(result, null, 2)}\n`);
    test.skip(true, result.reason);
    return;
  }
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
  await page.goto("/?mute=1&quality=high&scenario=stress2000");
  await page.waitForFunction(() => window.__parble?.ready === true, null, { timeout: 60_000 });
  await page.locator("canvas#parble").click({ position: { x: 20, y: 20 } });
  await page.waitForFunction(
    () => {
      const s = window.__parbleAudio.stats();
      return s.state === "running" && s.files > 0 && s.decoded + s.loadErrors === s.files;
    },
    null,
    { timeout: 30_000 },
  );
  const crossOriginIsolated = await page.evaluate(() => window.crossOriginIsolated);
  const before = await page.evaluate(() => window.__parbleAudio.stats());
  await page.evaluate(() => {
    window.__ft = [];
    let last = performance.now();
    const loop = (t) => {
      window.__ft.push(t - last);
      last = t;
      if (!window.__ftStop) requestAnimationFrame(loop);
    };
    requestAnimationFrame(loop);
    window.__parbleAudio.timing(true);
  });
  let maxMarbles = 0;
  let maxRolls = 0;
  let maxVoices = 0;
  const end = Date.now() + TRACE_S * 1000;
  while (Date.now() < end) {
    const x = await page.evaluate(() => {
      const s = window.__parbleAudio.stats();
      return [window.__parble.marbles.length, s.rollVoices, s.activeVoices];
    });
    maxMarbles = Math.max(maxMarbles, x[0]);
    maxRolls = Math.max(maxRolls, x[1]);
    maxVoices = Math.max(maxVoices, x[2]);
    await page.waitForTimeout(1_000);
  }
  const [audioMs, frameMs, after, state] = await page.evaluate(() => {
    window.__ftStop = true;
    const ms = window.__parbleAudio.timings();
    window.__parbleAudio.timing(false);
    const s = window.__parble;
    return [
      ms,
      window.__ft.slice(1),
      window.__parbleAudio.stats(),
      { scenario: s.scenario, quality: s.quality, fps: s.fps, canvas: [innerWidth, innerHeight] },
    ];
  });
  const loadAfter = loadavg()[0];
  const audio = stats(audioMs);
  const result = {
    taken: loadBefore <= LOAD_MAX && loadAfter <= LOAD_MAX,
    when: new Date().toISOString(),
    loadBefore: Number(loadBefore.toFixed(2)),
    loadAfter: Number(loadAfter.toFixed(2)),
    loadMax: LOAD_MAX,
    diagRun,
    host: cpus()[0]?.model ?? null,
    traceS: TRACE_S,
    budgetMs: { audioP95: AUDIO_P95_MS, frame: BUDGET_MS },
    crossOriginIsolated,
    state,
    maxMarbles,
    maxRolls,
    maxVoices,
    audio: { ...audio, over1Ms: audioMs.filter((v) => v > 1).length, over2Ms: audioMs.filter((v) => v > 2).length },
    frameTimes: stats(frameMs),
    oneShotsStarted: after.started - before.started,
    contacts: after.contacts - before.contacts,
    audioErrors: after.errors,
    errors,
  };
  writeFileSync(OUT, `${JSON.stringify(result, null, 2)}\n`);
  console.log(`audio perf: ${JSON.stringify(result)}`);
  if (!result.taken) {
    if (diagRun) expect(audio.p95Ms).toBeLessThanOrEqual(AUDIO_P95_MS);
    console.log(`NOT TAKEN: load ${loadBefore.toFixed(2)} before, ${loadAfter.toFixed(2)} after (max ${LOAD_MAX})`);
    return;
  }
  expect(errors).toEqual([]);
  expect(after.errors, after.lastError ?? "").toBe(0);
  expect(maxMarbles).toBe(10);
  expect(audio.frames).toBeGreaterThan(TRACE_S * 30);
  expect(audio.p95Ms).toBeLessThanOrEqual(AUDIO_P95_MS);
});
