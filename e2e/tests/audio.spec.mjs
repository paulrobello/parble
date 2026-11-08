// P4-T4 audio engine in GPU Chromium. The heaviest live audio scene the build has: a loaded track
// whose hopper releases 40 standard marbles (one every 0.2 s) onto the tray ramp, so they roll,
// pile up and clack into each other. Over 12 s the engine must start voices, record no engine,
// load or console errors, and never sound more voices than its cap. A second test feeds 64 rolling
// marbles and 64 contacts a frame through window.__parbleAudio.inject (decoded like a bridge event)
// to show the cap binds: voices are dropped or stolen, never added past it. A third injects one signal
// per mapped mechanism cue and checks each one starts a voice from a staged, decoded buffer.
import { expect, test } from "@playwright/test";

const BAD_TEXT = (t) => /%cERROR%c|\bERROR\b/.test(t) || /uncaptured|invalid|validation|panicked/i.test(t);
const MARBLES = 40;
const TRACK = {
  format: "parble",
  schema: 1,
  physics: 1,
  kit_contract: 1,
  seed: 1,
  placements: [],
  hopper: { placement: null, queue: Array(MARBLES).fill("standard"), release: { mode: "all" } },
  mechanisms: [],
  metadata: { name: "Audio stress" },
};

function watch(page) {
  const errors = [];
  const audioUrls = [];
  page.on("console", (m) => { if (m.type() === "error" || BAD_TEXT(m.text())) errors.push(m.text()); });
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("requestfailed", (r) => errors.push(`request failed: ${r.url()}`));
  page.on("request", (r) => { if (r.url().includes("/web/ui/audio/")) audioUrls.push(new URL(r.url())); });
  return { errors, audioUrls };
}

// Load the page, unlock audio with a click on the canvas and wait for every SFX file to decode.
async function unlock(page) {
  await page.goto("/");
  await page.waitForFunction(() => window.__parble?.ready === true, null, { timeout: 60_000 });
  expect(await page.evaluate(() => window.__parbleAudio.stats().state)).toBe("locked");
  await page.locator("canvas#parble").click({ position: { x: 20, y: 20 } });
  await page.waitForFunction(
    () => {
      const s = window.__parbleAudio.stats();
      return s.state === "running" && s.files > 0 && s.decoded + s.loadErrors === s.files;
    },
    null,
    { timeout: 30_000 },
  );
  return page.evaluate(() => window.__parbleAudio.stats());
}

test("a 40-marble run plays with no audio errors and a bounded voice count", async ({ page }) => {
  const { errors, audioUrls } = watch(page);
  const loaded = await unlock(page);
  console.log(`audio loaded: ${JSON.stringify(loaded)}`);
  expect(loaded.loadErrors).toBe(0);
  expect(loaded.decoded).toBe(loaded.files);

  // Every audio fetch carries its version-dist content hash (scripts/web/version-dist.mjs).
  const hashes = await page.evaluate(() => window.__PARBLE_HASHES);
  expect(audioUrls.length).toBe(loaded.files + 1);
  expect(audioUrls.filter((u) => u.searchParams.get("v") !== hashes[u.pathname.slice(1)]).map(String)).toEqual([]);

  await page.locator("#parble-import").setInputFiles({
    name: "audio-stress.parble",
    mimeType: "application/json",
    buffer: Buffer.from(JSON.stringify(TRACK)),
  });
  await page.waitForFunction(() => window.__parble?.track === "Audio stress", null, { timeout: 15_000 });

  const samples = [];
  const end = Date.now() + 12_000;
  while (Date.now() < end) {
    samples.push(
      await page.evaluate(() => {
        const s = window.__parbleAudio.stats();
        return { voices: s.activeVoices, rolls: s.rollVoices, marbles: window.__parble.marbles.length };
      }),
    );
    await page.waitForTimeout(200);
  }
  const s = await page.evaluate(() => window.__parbleAudio.stats());
  const maxVoices = Math.max(...samples.map((x) => x.voices));
  const maxRolls = Math.max(...samples.map((x) => x.rolls));
  const maxMarbles = Math.max(...samples.map((x) => x.marbles));
  console.log(`audio stress: ${JSON.stringify({ maxVoices, maxRolls, maxMarbles, stats: s })}`);

  expect(errors).toEqual([]);
  expect(s.errors, s.lastError ?? "").toBe(0);
  expect(s.loadErrors).toBe(0);
  expect(maxMarbles).toBeGreaterThanOrEqual(MARBLES);
  expect(s.contacts).toBeGreaterThan(0);
  expect(s.started).toBeGreaterThan(0);
  expect(maxRolls).toBeGreaterThan(0);
  expect(s.peakVoices).toBeLessThanOrEqual(s.cap.total);
  expect(maxVoices).toBeLessThanOrEqual(s.cap.total);
  expect(maxRolls).toBeLessThanOrEqual(s.cap.rolls);
});

test("injected 64-marble frames hit the voice cap without exceeding it", async ({ page }) => {
  const { errors } = watch(page);
  await unlock(page);
  const r = await page.evaluate(
    () =>
      new Promise((resolve) => {
        const n = 64;
        let tick = 1_000_000;
        let frames = 0;
        let max = 0;
        const step = () => {
          tick += 4;
          const sizes = ["mini", "standard", "shooter"];
          const rolls = Array.from({ length: n }, (_, i) => ({
            marble: i,
            kind: i % 3 === 0 ? "steel" : i % 3 === 1 ? "glazed" : "cats-eye",
            size: sizes[i % 3],
            speed: 0.2 + (i % 8) * 0.15,
            surface: i % 2 ? "rail" : "wood",
            contact: true,
            position: [Math.cos(i) * (0.2 + i * 0.02), 0.05, Math.sin(i) * (0.2 + i * 0.02)],
          }));
          const contacts = Array.from({ length: n }, (_, i) => ({
            marble: i,
            kind: "cats-eye",
            size: sizes[(i + 1) % 3],
            surface: i % 4 === 0 ? "marble" : "wood",
            otherKind: i % 4 === 0 ? "steel" : null,
            impulse: 0.0004 + ((i * 7 + frames) % 30) * 0.0003,
            count: 1,
            position: rolls[i].position,
          }));
          const ok = window.__parbleAudio.inject({
            type: "audioFrame",
            tick,
            contacts,
            rolls,
            mechanisms: [],
            camera: { position: [0, 0.3, 0.6], forward: [0, -0.4, -0.9], up: [0, 1, 0] },
          });
          if (!ok) return resolve({ error: "inject refused the frame" });
          max = Math.max(max, window.__parbleAudio.stats().activeVoices);
          if (++frames < 180) requestAnimationFrame(step);
          else resolve({ max, stats: window.__parbleAudio.stats() });
        };
        requestAnimationFrame(step);
      }),
  );
  console.log(`audio inject: ${JSON.stringify(r)}`);
  expect(r.error).toBeUndefined();
  const s = r.stats;
  expect(errors).toEqual([]);
  expect(s.injected).toBe(true);
  expect(s.errors, s.lastError ?? "").toBe(0);
  expect(r.max).toBeLessThanOrEqual(s.cap.total);
  expect(s.peakVoices).toBe(s.cap.total);
  expect(s.rollVoices).toBe(s.cap.rolls);
  expect(s.dropped + s.stolen).toBeGreaterThan(0);
  expect(s.rollsCulled).toBeGreaterThan(0);
});

test("every mapped mechanism signal plays its staged cue", async ({ page }) => {
  const { errors } = watch(page);
  await unlock(page);
  const signals = [
    ["mech_hopper", "release"],
    ["mech_finish", "finish"],
    ["mech_checkpoint_l1", "split"],
    ["mech_splitter_l3", "set"],
    ["mech_toggle_l3", "set"],
    ["mech_gate_oneway_l1", "open"],
    ["mech_hole_oneway_l1", "open"],
    ["mech_hole_oneway_l1", "drop"],
  ];
  const r = await page.evaluate((signals) => {
    const before = window.__parbleAudio.stats();
    const ok = window.__parbleAudio.inject({
      type: "audioFrame",
      tick: 2_000_000,
      contacts: [],
      rolls: [],
      mechanisms: signals.map(([piece, name], i) => ({
        kind: "signal",
        tick: 2_000_000,
        instance: [i, 0, 0],
        piece,
        name,
        value: 0,
      })),
      camera: { position: [0, 0.3, 0.6], forward: [0, -0.4, -0.9], up: [0, 1, 0] },
    });
    return { ok, before, after: window.__parbleAudio.stats() };
  }, signals);
  console.log(`audio mechanisms: ${JSON.stringify({ ok: r.ok, after: r.after })}`);
  expect(r.ok).toBe(true);
  expect(errors).toEqual([]);
  expect(r.after.errors, r.after.lastError ?? "").toBe(0);
  expect(r.after.started - r.before.started).toBe(signals.length);
  expect(r.after.missingBuffers - r.before.missingBuffers).toBe(0);
  expect(r.after.dropped - r.before.dropped).toBe(0);
});
