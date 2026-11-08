// P2-T6 mechanism parts in GPU Chromium: `?scenario=mechanisms` runs a toggle, a one-way gate and a
// stepper elevator with one marble each (crates/parble-sim/src/mech_showcase.rs), restarting every 10 s. Sampled on every
// animation frame through a run, each drawn part angle (`window.__parble.kit.parts`) is the sim's
// angle interpolated between the last two physics ticks at the frame's alpha, and the gate flap's
// fall back onto its stop is drawn as a smooth, monotonic sweep across frames, with angles between
// ticks. Shots go to target/shots/web-mechanisms/.
import { mkdirSync } from "node:fs";
import { expect, test } from "@playwright/test";

const SHOTS = new URL("../../target/shots/web-mechanisms/", import.meta.url);
const BAD_TEXT = (t) => /%cERROR%c|\bERROR\b/.test(t) || /uncaptured|invalid|validation|panicked/i.test(t);
// The gate flap is a physical hinge (behaviour.rs GATE_FLAP): the marble pushes it open, gravity
// drops it back onto its stop, below GATE_CLOSED_MDEG, then it settles in a few small bounces.
const GATE_CLOSED_MDEG = 2_000;
const GATE_LIMIT_MDEG = 85_000;

mkdirSync(SHOTS, { recursive: true });

test("mechanism parts interpolate between physics ticks and sweep smoothly", async ({ page }) => {
  const errors = [];
  page.on("console", (m) => { if (m.type() === "error" || BAD_TEXT(m.text())) errors.push(m.text()); });
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("requestfailed", (r) => errors.push(`request failed: ${r.url()}`));
  await page.goto("/?mute=1&scenario=mechanisms&quality=medium");
  await page.waitForFunction(() => window.__parble?.ready === true && window.__parble.kit?.parts?.length === 11, null, {
    timeout: 60_000,
  });
  const k = await page.evaluate(() => window.__parble.kit);
  expect(k.parts.map((p) => p.piece)).toEqual([
    ...Array(8).fill("mech_arch_rope_l1"),
    "mech_toggle_l3",
    "mech_gate_oneway_l1",
    "mech_stepper_up1_l3",
  ]);
  // The toggle, the gate and the stepper each add one part mesh per (LOD, variant), the rope arch
  // (in its own chunk, with one block) eight: the bound counts them. The lift lane reaches the
  // second chunk of the row.
  expect(k.drawBound).toBe(3 * (1 + 2 + 1 + 2 + 1 + 2) + 3 * (1 + 9));

  // Every animation frame for 11 s: the run restarts every 10 s, so one whole run is in the sample.
  const samples = await page.evaluate(
    () =>
      new Promise((resolve) => {
        const out = [];
        const t0 = performance.now();
        const step = () => {
          const s = window.__parble;
          out.push({ tick: s.tick, run: s.run, frame: s.frame, parts: s.kit.parts });
          if (performance.now() - t0 < 11_000) requestAnimationFrame(step);
          else resolve(out);
        };
        requestAnimationFrame(step);
      }),
  );
  console.log(`mechanisms: ${samples.length} frames sampled`);
  expect(samples.length).toBeGreaterThan(300);

  // 1. Each drawn angle is lerp(prev tick, current tick, alpha), as for the marbles.
  let between = 0;
  for (const s of samples) {
    for (const p of s.parts) {
      expect(p.tickMdeg).not.toBeNull();
      const prev = p.prevMdeg ?? p.tickMdeg;
      expect(p.alpha).toBeGreaterThanOrEqual(0);
      expect(p.alpha).toBeLessThanOrEqual(1);
      expect(Math.abs(p.angleMdeg - (prev + (p.tickMdeg - prev) * p.alpha))).toBeLessThan(1);
      if (prev !== p.tickMdeg && p.angleMdeg !== p.tickMdeg && p.angleMdeg !== prev) between++;
    }
  }

  // 2. The gate flap's fall: consecutive frames from its widest swing until it first reaches its stop.
  const gate = samples.map((s) => ({ tick: s.tick, run: s.run, frame: s.frame, ...s.parts.find((p) => p.piece === "mech_gate_oneway_l1") }));
  const peak = gate.reduce((best, g, i) => (g.tickMdeg > gate[best].tickMdeg ? i : best), 0);
  const fall = gate.slice(peak).filter((g) => g.run === gate[peak].run);
  const fallSweep = fall.slice(0, fall.findIndex((g) => g.tickMdeg <= GATE_CLOSED_MDEG) + 1);
  const closing = fallSweep.filter((g) => g.prevMdeg !== null && g.prevMdeg > g.tickMdeg);
  console.log(
    `gate close frames: ${JSON.stringify(closing.map((g) => [g.tick, g.prevMdeg, g.tickMdeg, +g.alpha.toFixed(3), +g.angleMdeg.toFixed(0)]))}`,
  );
  expect(closing.length).toBeGreaterThanOrEqual(3);
  for (let i = 1; i < fallSweep.length; i++) {
    const [a, b] = [fallSweep[i - 1], fallSweep[i]];
    // Monotonic: the falling flap never jumps back open.
    expect(b.angleMdeg).toBeLessThanOrEqual(a.angleMdeg);
    // Smooth: no frame jumps more than a third of the flap's travel.
    expect(a.angleMdeg - b.angleMdeg).toBeLessThan(GATE_LIMIT_MDEG / 3);
  }
  // The drawn rotation changes frame to frame while the flap moves.
  // Under host load a requestAnimationFrame can fire twice for one app frame, so count app frames.
  const drawn = [...new Map(closing.map((g) => [g.frame, g])).values()];
  const rotations = new Set(drawn.map((g) => g.rotation.map((v) => v.toFixed(5)).join(",")));
  expect(rotations.size).toBe(drawn.length);
  // Frames drawn between two ticks (0 < alpha < 1) show an angle neither tick has.
  expect(between).toBeGreaterThan(0);

  await page.screenshot({ path: new URL("mechanisms.png", SHOTS).pathname });
  expect(errors).toEqual([]);
});
