import { describe, expect, it } from "vitest";
import picks from "../../../docs/audio/audition-picks.json";
import type { AudioContact, AudioFrame, AudioMechanism, AudioRoll, MarbleKind, Surface, Vec3 } from "../src/bridge";
import { type Catalog, decodeCatalog, VariantPicker } from "../src/audio/catalog";
import { AudioEngine, JITTER_SEMITONES, STALE_TICKS, TUNNEL_CUE } from "../src/audio/engine";
import {
  distanceGain,
  IMPACT_BANDS,
  IMPACT_MIN_DV,
  impactBand,
  impactDeltaV,
  impactFor,
  impactGain,
  jitterRate,
  massKg,
  PANNER,
  ROLL_CUTOFF_RANGE_HZ,
  ROLL_MIN_SPEED,
  ROLL_RATE_RANGE,
  ROLL_REF_SPEED,
  rollCueId,
  rollParams,
  SIGNAL_CUE_IDS,
  SIGNAL_CUES,
  SIZE_PITCH_EXPONENT,
  STANDARD_MASS_KG,
  signalCueId,
  sizeRate,
} from "../src/audio/mapping";
import { admit, oneShotPriority, seededRng, selectTop } from "../src/audio/voices";

const SPEEDS = Array.from({ length: 301 }, (_, i) => i * 0.01);

describe("roll loop mapping from speed", () => {
  it("is silent at rest and below the threshold", () => {
    expect(rollParams(0).gain).toBe(0);
    expect(rollParams(ROLL_MIN_SPEED).gain).toBe(0);
    expect(rollParams(-1).gain).toBe(0);
    expect(rollParams(Number.NaN).gain).toBe(0);
  });

  it("raises pitch, gain and cutoff monotonically with speed, within bounds", () => {
    let prev = rollParams(0);
    for (const s of SPEEDS) {
      const p = rollParams(s);
      expect(p.rate).toBeGreaterThanOrEqual(prev.rate);
      expect(p.gain).toBeGreaterThanOrEqual(prev.gain);
      expect(p.cutoffHz).toBeGreaterThanOrEqual(prev.cutoffHz);
      expect(p.rate).toBeGreaterThanOrEqual(ROLL_RATE_RANGE[0]);
      expect(p.rate).toBeLessThanOrEqual(ROLL_RATE_RANGE[1]);
      expect(p.gain).toBeLessThanOrEqual(1);
      expect(p.cutoffHz).toBeGreaterThanOrEqual(ROLL_CUTOFF_RANGE_HZ[0]);
      expect(p.cutoffHz).toBeLessThanOrEqual(ROLL_CUTOFF_RANGE_HZ[1]);
      prev = p;
    }
    expect(rollParams(0.1).gain).toBeLessThan(rollParams(1).gain);
    expect(rollParams(0.1).cutoffHz).toBeLessThan(rollParams(1).cutoffHz);
  });

  it("plays the loop at its recorded pitch at the reference speed and saturates when fast", () => {
    expect(rollParams(ROLL_REF_SPEED).rate).toBeCloseTo(1, 10);
    expect(rollParams(1.5).gain).toBeCloseTo(1, 10);
    expect(rollParams(1.5).cutoffHz).toBeCloseTo(ROLL_CUTOFF_RANGE_HZ[1], 6);
    expect(rollParams(10)).toEqual(rollParams(20));
  });

  it("picks the roll cue by marble material and surface", () => {
    expect(rollCueId("cats-eye", "wood")).toBe("roll_glass_block");
    expect(rollCueId("opal", "wood_end_grain")).toBe("roll_glass_block");
    expect(rollCueId("glazed", "rail")).toBe("roll_solid_rail");
    expect(rollCueId("steel", "rail")).toBe("roll_steel_rail");
    expect(rollCueId("steel", "marble")).toBeNull();
    expect(rollCueId("steel", null)).toBeNull();
  });
});

describe("marble size", () => {
  it("weighs each size by density × its own volume", () => {
    expect(massKg("cats-eye")).toBeCloseTo(STANDARD_MASS_KG.glass, 15);
    expect(massKg("steel", "mini") / massKg("steel")).toBeCloseTo((10 / 16) ** 3, 10);
    expect(massKg("glazed", "shooter") / massKg("glazed")).toBeCloseTo((25 / 16) ** 3, 10);
    expect(massKg("swirl", "shooter")).toBeCloseTo(massKg("cats-eye", "shooter"), 15);
  });

  it("offsets pitch up for a mini and down for a shooter", () => {
    expect(SIZE_PITCH_EXPONENT).toBe(0.5);
    expect(sizeRate("standard")).toBe(1);
    expect(sizeRate("mini")).toBeCloseTo(Math.sqrt(1.6), 10);
    expect(sizeRate("shooter")).toBeCloseTo(Math.sqrt(0.64), 10);
    const semitones = (r: number) => 12 * Math.log2(r);
    expect(semitones(sizeRate("mini"))).toBeCloseTo(4.07, 2);
    expect(semitones(sizeRate("shooter"))).toBeCloseTo(-3.86, 2);
  });

  it("applies the roll size offset on top of the speed clamp", () => {
    for (const size of ["mini", "standard", "shooter"] as const) {
      expect(rollParams(ROLL_REF_SPEED, size).rate).toBeCloseTo(sizeRate(size), 10);
      expect(rollParams(10, size).rate).toBeCloseTo(ROLL_RATE_RANGE[1] * sizeRate(size), 10);
      expect(rollParams(0.6, size).gain).toBe(rollParams(0.6).gain);
      expect(rollParams(0.6, size).cutoffHz).toBe(rollParams(0.6).cutoffHz);
    }
    expect(rollParams(10, "mini").rate).toBeGreaterThan(ROLL_RATE_RANGE[1]);
    expect(rollParams(0.03, "shooter").rate).toBeLessThan(ROLL_RATE_RANGE[0]);
  });
});

const contact = (o: Partial<AudioContact> = {}): AudioContact => ({
  marble: 0,
  kind: "cats-eye",
  size: "standard",
  surface: "wood",
  otherKind: null,
  impulse: 0.003,
  count: 1,
  position: [0, 0, 0],
  ...o,
});

describe("impact mapping from impulse", () => {
  it("normalises the impulse by the marble's mass", () => {
    expect(impactDeltaV(STANDARD_MASS_KG.glass, "swirl")).toBeCloseTo(1, 10);
    expect(impactDeltaV(0.003, "steel")).toBeLessThan(impactDeltaV(0.003, "cats-eye"));
    expect(impactDeltaV(Number.NaN, "steel")).toBe(0);
  });

  it("bands an impact by the real mass of the marble's size and carries its pitch offset", () => {
    const impulse = STANDARD_MASS_KG.glass * 0.5;
    expect(impactDeltaV(impulse, "cats-eye", "mini")).toBeCloseTo(0.5 * (16 / 10) ** 3, 10);
    expect(impactDeltaV(impulse, "cats-eye", "shooter")).toBeCloseTo(0.5 * (16 / 25) ** 3, 10);
    const at = (size: AudioContact["size"]) => impactFor(contact({ size, impulse }));
    expect(at("standard")).toMatchObject({ cue: "impact_glass_block_medium", rate: 1 });
    expect(at("mini")).toMatchObject({ cue: "impact_glass_block_heavy", rate: sizeRate("mini") });
    expect(at("shooter")).toMatchObject({ cue: "impact_glass_block_light", rate: sizeRate("shooter") });
    const clack = impactFor(contact({ kind: "steel", size: "mini", surface: "marble", otherKind: "glazed", impulse }));
    expect(clack?.dv).toBeCloseTo(impulse / massKg("steel", "mini"), 10);
    expect(clack?.rate).toBe(sizeRate("mini"));
  });

  it("selects the band by velocity change", () => {
    expect(impactBand(IMPACT_MIN_DV / 2)).toBeNull();
    expect(impactBand(IMPACT_MIN_DV)).toBe("light");
    expect(impactBand(IMPACT_BANDS[0] - 1e-6)).toBe("light");
    expect(impactBand(IMPACT_BANDS[0])).toBe("medium");
    expect(impactBand(IMPACT_BANDS[1])).toBe("heavy");
    expect(impactBand(50)).toBe("heavy");
  });

  it("scales gain monotonically with force, bounded to 1", () => {
    expect(impactGain(0)).toBe(0);
    let prev = 0;
    for (const dv of SPEEDS.map((s) => s * 2)) {
      const g = impactGain(dv);
      expect(g).toBeGreaterThanOrEqual(prev);
      expect(g).toBeLessThanOrEqual(1);
      prev = g;
    }
  });

  it("names block, rail and clack cues, unordered for marble pairs", () => {
    const m = STANDARD_MASS_KG.glass;
    expect(impactFor(contact({ impulse: m * 0.1 }))?.cue).toBe("impact_glass_block_light");
    expect(impactFor(contact({ surface: "wood_end_grain", impulse: m * 0.5 }))?.cue).toBe("impact_glass_block_medium");
    expect(impactFor(contact({ kind: "steel", surface: "rail", impulse: STANDARD_MASS_KG.steel }))?.cue).toBe(
      "impact_steel_rail_heavy",
    );
    const a = impactFor(contact({ kind: "steel", surface: "marble", otherKind: "glazed", impulse: 0.05 }));
    const b = impactFor(contact({ kind: "glazed", surface: "marble", otherKind: "steel", impulse: 0.05 }));
    expect(a?.cue).toBe("clack_solid_steel_heavy");
    expect(b?.cue).toBe("clack_solid_steel_heavy");
    expect(impactFor(contact({ surface: "marble", otherKind: null, impulse: 0.05 }))).toBeNull();
    expect(impactFor(contact({ impulse: 0 }))).toBeNull();
  });

  it("keeps pitch jitter within ± the semitone range", () => {
    expect(jitterRate(0.5, 1)).toBeCloseTo(1, 10);
    expect(jitterRate(0, 1)).toBeCloseTo(2 ** (-1 / 12), 10);
    expect(jitterRate(0.999999, 1)).toBeLessThanOrEqual(2 ** (1 / 12));
  });

  it("attenuates with distance like the PannerNode inverse model", () => {
    expect(distanceGain(0)).toBe(1);
    expect(distanceGain(PANNER.refDistance)).toBe(1);
    expect(distanceGain(PANNER.refDistance * 2)).toBeCloseTo(0.5, 10);
    expect(distanceGain(1)).toBeGreaterThan(distanceGain(2));
  });
});

describe("voice allocation", () => {
  it("seeded RNG repeats and stays in [0, 1)", () => {
    const a = seededRng(7);
    const b = seededRng(7);
    const xs = Array.from({ length: 1000 }, () => a());
    expect(xs).toEqual(Array.from({ length: 1000 }, () => b()));
    expect(xs.every((x) => x >= 0 && x < 1)).toBe(true);
    expect(seededRng(8)()).not.toBe(xs[0]);
  });

  it("keeps the loudest candidates, ties by id", () => {
    const c = [
      { id: 3, priority: 0.5 },
      { id: 1, priority: 0.9 },
      { id: 2, priority: 0.5 },
      { id: 4, priority: 0.1 },
    ];
    expect(selectTop(c, 3).map((x) => x.id)).toEqual([1, 2, 3]);
    expect(selectTop(c, 0)).toEqual([]);
    expect(selectTop(c, 10)).toHaveLength(4);
  });

  it("admits into a free slot, steals the quietest for a louder voice, drops a quieter one", () => {
    const active = [
      { id: 1, priority: 0.4 },
      { id: 2, priority: 0.2 },
      { id: 3, priority: 0.8 },
    ];
    expect(admit(active, 4, 0.01)).toEqual({ admit: true, evict: null });
    expect(admit(active, 3, 0.3)).toEqual({ admit: true, evict: 2 });
    expect(admit(active, 3, 0.2)).toEqual({ admit: false });
    expect(admit(active, 3, 0.1)).toEqual({ admit: false });
    expect(admit([], 0, 1)).toEqual({ admit: false });
  });

  it("decays a one-shot's priority over its length", () => {
    expect(oneShotPriority(1, 0, 2, 0)).toBe(1);
    expect(oneShotPriority(1, 0, 2, 1)).toBeCloseTo(0.5, 10);
    expect(oneShotPriority(1, 0, 2, 3)).toBe(0);
    expect(oneShotPriority(1, 0, 0, 0)).toBe(0);
  });
});

// Every cue id the engine can ask for, 3 variants each.
function testCatalog(): Catalog {
  const mats = ["glass", "solid", "steel"];
  const ids: [string, boolean][] = SIGNAL_CUE_IDS.map((id) => [id, false]);
  ids.push([TUNNEL_CUE, true]);
  for (const m of mats) {
    for (const f of ["block", "rail"]) {
      ids.push([`roll_${m}_${f}`, true]);
      for (const b of ["light", "medium", "heavy"]) ids.push([`impact_${m}_${f}_${b}`, false]);
    }
  }
  for (let i = 0; i < 3; i++)
    for (let j = i; j < 3; j++)
      for (const b of ["light", "medium", "heavy"]) ids.push([`clack_${mats[i]}_${mats[j]}_${b}`, false]);
  const cues = Object.fromEntries(
    ids.map(([id, loop]) => [
      id,
      {
        loop,
        variants: [1, 2, 3].map((n) => ({
          stem: `${id}_v0${n}`,
          duration: loop ? 6 : 0.5,
          ogg: "x.ogg",
          m4a: "x.m4a",
        })),
      },
    ]),
  );
  const c = decodeCatalog({ schema: "parble.sfx-runtime/1", cues });
  if (!c) throw new Error("test catalog");
  return c;
}

describe("catalog and variants", () => {
  it("decodes the staged catalog shape and refuses others", () => {
    expect(Object.keys(testCatalog().cues)).toHaveLength(SIGNAL_CUE_IDS.length + 6 + 18 + 18 + 1);
    expect(decodeCatalog({ schema: "other", cues: {} })).toBeNull();
    expect(decodeCatalog({ schema: "parble.sfx-runtime/1", cues: { a: { loop: 1, variants: [] } } })).toBeNull();
  });

  it("picks variants deterministically, never the same twice in a row, covering all", () => {
    const vs = testCatalog().cues.impact_glass_block_light?.variants ?? [];
    const run = (seed: number) => {
      const p = new VariantPicker(seededRng(seed));
      return Array.from({ length: 300 }, () => p.pick("impact_glass_block_light", vs)?.stem);
    };
    const a = run(11);
    expect(a).toEqual(run(11));
    expect(new Set(a).size).toBe(3);
    for (let i = 1; i < a.length; i++) expect(a[i]).not.toBe(a[i - 1]);
  });
});

// A recording stand-in for the Web Audio graph: enough of BaseAudioContext for the engine.
class Param {
  value = 0;
  setTargetAtTime(v: number): this {
    this.value = v;
    return this;
  }
}
class Node {
  connect<T>(n: T): T {
    return n;
  }
  disconnect(): void {}
}
class Gain extends Node {
  gain = new Param();
}
class Filter extends Node {
  type = "";
  frequency = new Param();
}
class Panner extends Node {
  positionX = new Param();
  positionY = new Param();
  positionZ = new Param();
  panningModel = "";
  distanceModel = "";
  refDistance = 0;
  rolloffFactor = 0;
  maxDistance = 0;
}
class Source extends Node {
  buffer: { stem: string; duration: number } | null = null;
  loop = false;
  playbackRate = new Param();
  onended: (() => void) | null = null;
  stopped = false;
  start(): void {}
  stop(): void {
    this.stopped = true;
  }
}
class FakeContext {
  currentTime = 0;
  destination = new Node();
  listener = Object.fromEntries(
    ["positionX", "positionY", "positionZ", "forwardX", "forwardY", "forwardZ", "upX", "upY", "upZ"].map((k) => [
      k,
      new Param(),
    ]),
  );
  sources: Source[] = [];
  createGain = () => new Gain();
  createDynamicsCompressor = () => new Node();
  createBiquadFilter = () => new Filter();
  createPanner = () => new Panner();
  createBufferSource = () => {
    const s = new Source();
    this.sources.push(s);
    return s;
  };
}

function engine(seed = 1) {
  const ctx = new FakeContext();
  const catalog = testCatalog();
  const e = new AudioEngine(ctx as unknown as BaseAudioContext, { catalog, seed, caps: { total: 24, rolls: 10 } });
  for (const c of Object.values(catalog.cues))
    for (const v of c.variants) e.addBuffer(v.stem, { stem: v.stem, duration: v.duration } as unknown as AudioBuffer);
  return { ctx, e };
}

const roll = (marble: number, o: Partial<AudioRoll> = {}): AudioRoll => ({
  marble,
  kind: "cats-eye" as MarbleKind,
  size: "standard",
  speed: 0.6,
  surface: "wood" as Surface,
  contact: true,
  position: [marble * 0.1, 0, 0] as Vec3,
  tunnel: false,
  ...o,
});

const frame = (tick: number, o: Partial<AudioFrame> = {}): AudioFrame => ({
  type: "audioFrame",
  tick,
  contacts: [],
  rolls: [],
  mechanisms: [],
  camera: { position: [0, 0.2, 0], forward: [1, 0, 0], up: [0, 1, 0] },
  ...o,
});

describe("engine voice budget", () => {
  it("keeps the roll budget for the nearest marbles", () => {
    const { e } = engine();
    e.consume(frame(4, { rolls: Array.from({ length: 40 }, (_, i) => roll(i)) }));
    const s = e.stats();
    expect(s.errors).toBe(0);
    expect(s.rollVoices).toBe(10);
    expect(s.rollsCulled).toBe(30);
    // Marbles move away from the listener, so marble 39 is the farthest; move it next to it.
    e.consume(
      frame(8, { rolls: Array.from({ length: 40 }, (_, i) => roll(i, i === 39 ? { position: [0, 0.2, 0] } : {})) }),
    );
    expect(e.stats().rollVoices).toBe(10);
  });

  it("reports the last frame's audible and voiced rolls, empty while paused", () => {
    const { e } = engine();
    const rolls = [...Array.from({ length: 12 }, (_, i) => roll(i)), roll(20, { contact: false })];
    e.consume(frame(4, { rolls }));
    const r = e.rollFrame();
    // 12 audible, the 10 nearest voiced; the airborne marble keeps neither.
    expect(r.audible).toHaveLength(12);
    expect(r.audible).not.toContain(20);
    expect([...r.voiced].sort((a, b) => a - b)).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    e.consume(frame(8, { paused: true }));
    expect(e.rollFrame()).toEqual({ audible: [], voiced: [] });
  });

  it("never sounds more than the cap, dropping or stealing for one-shots", () => {
    const { ctx, e } = engine();
    const rolls = Array.from({ length: 12 }, (_, i) => roll(i));
    const contacts = Array.from({ length: 100 }, (_, i) =>
      contact({ marble: i, impulse: 0.0005 + i * 0.0001, position: [0.05 * (i % 10), 0, 0] }),
    );
    for (let t = 4; t <= 40; t += 4) {
      ctx.currentTime += 1 / 60;
      e.consume(frame(t, { rolls, contacts }));
      expect(e.stats().activeVoices).toBeLessThanOrEqual(24);
    }
    const s = e.stats();
    expect(s.errors).toBe(0);
    expect(s.peakVoices).toBe(24);
    expect(s.rollVoices).toBe(10);
    expect(s.oneShotVoices).toBe(14);
    expect(s.dropped).toBeGreaterThan(0);
    expect(s.stolen).toBeGreaterThan(0);
  });

  it("holds the cap when one frame starts more voices than it, after one-shots filled the roll slots", () => {
    const { ctx, e } = engine();
    const loud = (n: number, from = 0) =>
      Array.from({ length: n }, (_, i) => contact({ marble: from + i, impulse: 0.003, position: [0, 0.2, 0] }));
    // No rolls: one-shots may take all 24 slots, including the 10 the rolls use.
    e.consume(frame(4, { contacts: loud(30) }));
    expect(e.stats().oneShotVoices).toBe(24);
    ctx.currentTime += 0.01;
    // Next frame: 12 rolls and 30 more one-shots start at once, 42 asks for 24 slots.
    e.consume(frame(8, { rolls: Array.from({ length: 12 }, (_, i) => roll(i)), contacts: loud(30, 100) }));
    const s = e.stats();
    expect(s.errors).toBe(0);
    expect(s.activeVoices).toBeLessThanOrEqual(24);
    expect(s.peakVoices).toBeLessThanOrEqual(24);
    expect(s.rollVoices).toBe(10);
    expect(s.oneShotVoices).toBe(14);
    expect(s.stolen).toBeGreaterThanOrEqual(10);
    // A run restart clears the rolls but not the one-shots; the rolls coming back still fit.
    ctx.currentTime += 0.01;
    e.consume(frame(0, { contacts: loud(30, 200) }));
    ctx.currentTime += 0.01;
    e.consume(frame(4, { rolls: Array.from({ length: 12 }, (_, i) => roll(i)) }));
    expect(e.stats().activeVoices).toBeLessThanOrEqual(24);
    expect(e.stats().peakVoices).toBeLessThanOrEqual(24);
  });

  it("lets a louder impact take the slot of the quietest one", () => {
    const { ctx, e } = engine();
    const quiet = Array.from({ length: 24 }, (_, i) => contact({ marble: i, impulse: 0.0003, position: [2, 0, 0] }));
    e.consume(frame(4, { contacts: quiet }));
    expect(e.stats().oneShotVoices).toBe(24);
    ctx.currentTime += 0.01;
    e.consume(frame(8, { contacts: [contact({ marble: 99, impulse: 0.01, position: [0, 0.2, 0] })] }));
    expect(e.stats()).toMatchObject({ oneShotVoices: 24, stolen: 1, dropped: 0, started: 25 });
    e.consume(frame(12, { contacts: [contact({ marble: 98, impulse: 0.0003, position: [3, 0, 0] })] }));
    expect(e.stats()).toMatchObject({ stolen: 1, dropped: 1 });
  });

  it("crossfades loops on a surface change and holds the last roll between samples", () => {
    const { ctx, e } = engine();
    e.consume(frame(4, { rolls: [roll(0)] }));
    const first = ctx.sources.length;
    e.consume(frame(5));
    e.consume(frame(6));
    expect(e.stats().rollVoices).toBe(1);
    expect(ctx.sources.length).toBe(first);
    e.consume(frame(8, { rolls: [roll(0, { surface: "rail" })] }));
    expect(e.stats().crossfades).toBe(1);
    expect(ctx.sources.at(-1)?.buffer?.stem).toMatch(/^roll_glass_rail_v0[123]$/);
    expect(ctx.sources[first - 1]?.stopped).toBe(true);
    e.consume(frame(8 + STALE_TICKS + 1));
    expect(e.stats().rollVoices).toBe(0);
  });

  it("layers the tunnel rumble on a marble's roll voice while it is in a redirect tunnel", () => {
    const { ctx, e } = engine();
    const tunnelSources = () => ctx.sources.filter((s) => s.buffer?.stem.startsWith(`${TUNNEL_CUE}_`));
    e.consume(frame(4, { rolls: [roll(0), roll(1)] }));
    expect(tunnelSources()).toHaveLength(0);
    expect(e.stats().tunnelLayers).toBe(0);

    e.consume(frame(8, { rolls: [roll(0, { tunnel: true, speed: 0.3 }), roll(1)] }));
    const [layer] = tunnelSources();
    expect(layer?.loop).toBe(true);
    // A layer of the roll voice: no extra voice, same speed-driven rate as the roll loop.
    expect(e.stats()).toMatchObject({ tunnelLayers: 1, rollVoices: 2, activeVoices: 2 });
    expect(layer?.playbackRate.value).toBeCloseTo(rollParams(0.3).rate, 10);
    e.consume(frame(12, { rolls: [roll(0, { tunnel: true, speed: 1.2 }), roll(1)] }));
    expect(layer?.playbackRate.value).toBeCloseTo(rollParams(1.2).rate, 10);
    expect(layer?.playbackRate.value).toBeGreaterThan(rollParams(0.3).rate);
    expect(tunnelSources()).toHaveLength(1);

    // Leaving the tunnel fades the layer out; the roll keeps sounding.
    e.consume(frame(16, { rolls: [roll(0, { speed: 1.2 }), roll(1)] }));
    expect(layer?.stopped).toBe(true);
    expect(e.stats()).toMatchObject({ tunnelLayers: 0, rollVoices: 2 });

    // Back in, then a run restart stops the new layer with its roll voice.
    e.consume(frame(20, { rolls: [roll(0, { tunnel: true })] }));
    const again = tunnelSources().at(-1);
    expect(again).not.toBe(layer);
    e.consume(frame(0));
    expect(again?.stopped).toBe(true);
    expect(e.stats()).toMatchObject({ tunnelLayers: 0, rollVoices: 0 });
  });

  it("silences an airborne marble without losing its slot, then a run restart clears every roll", () => {
    const { e } = engine();
    e.consume(frame(4, { rolls: [roll(0), roll(1)] }));
    e.consume(frame(8, { rolls: [roll(0, { contact: false, surface: null })] }));
    expect(e.stats().rollVoices).toBe(2);
    e.consume(frame(12, { rolls: [roll(0), roll(1)] }));
    e.consume(frame(0));
    expect(e.stats()).toMatchObject({ rollVoices: 0, resets: 1 });
  });

  it("releases every roll on pause, still plays a step's impact, and resumes on the next sample", () => {
    const { e } = engine();
    e.consume(frame(4, { rolls: [roll(0), roll(1)] }));
    expect(e.stats().rollVoices).toBe(2);
    e.consume(frame(4, { paused: true }));
    expect(e.stats().rollVoices).toBe(0);
    // Paused frames repeat the tick; nothing comes back without a sample.
    e.consume(frame(4, { paused: true }));
    e.consume(frame(8, { paused: true, rolls: [roll(0)], contacts: [contact({ impulse: 0.002 })] }));
    expect(e.stats()).toMatchObject({ rollVoices: 0, started: 1 });
    e.consume(frame(9));
    expect(e.stats().rollVoices).toBe(0);
    e.consume(frame(12, { rolls: [roll(0)] }));
    expect(e.stats().rollVoices).toBe(1);
  });

  it("holds one roll voice without restarts through 0.1x slow motion", () => {
    const { ctx, e } = engine();
    e.consume(frame(4, { rolls: [roll(0)] }));
    const sources = ctx.sources.length;
    // At 0.1x a frame runs 0 or 1 ticks, and a roll sample arrives every 4 ticks (~10 frames).
    for (let f = 1; f <= 100; f++) {
      const tick = 4 + Math.floor(f * 0.4);
      e.consume(frame(tick, tick % 4 === 0 && f % 10 === 0 ? { rolls: [roll(0, { speed: 0.6 })] } : {}));
      expect(e.stats().rollVoices).toBe(1);
    }
    expect(ctx.sources.length).toBe(sources);
    expect(e.stats().crossfades).toBe(0);
  });

  it("picks the same variants and jitter for the same seed and frames", () => {
    const play = (seed: number) => {
      const { ctx, e } = engine(seed);
      for (let t = 4; t <= 80; t += 4) {
        ctx.currentTime += 0.2;
        e.consume(
          frame(t, { contacts: [contact({ impulse: 0.002 }), contact({ marble: 1, kind: "steel", surface: "rail" })] }),
        );
      }
      return ctx.sources.map((s) => [s.buffer?.stem, s.playbackRate.value]);
    };
    const a = play(5);
    expect(a).toEqual(play(5));
    expect(a).not.toEqual(play(6));
    const lo = 2 ** (-JITTER_SEMITONES / 12);
    const hi = 2 ** (JITTER_SEMITONES / 12);
    expect(a.every(([, r]) => (r as number) >= lo && (r as number) <= hi)).toBe(true);
  });

  it("places the listener at the camera and plays the gate swing", () => {
    const { ctx, e } = engine();
    e.consume(
      frame(4, {
        camera: { position: [1, 2, 3], forward: [0, 0, -1], up: [0, 1, 0] },
        mechanisms: [
          { kind: "signal", tick: 4, instance: [1, 0, 2], piece: "mech_gate_oneway_l1", name: "open", value: 1 },
        ],
      }),
    );
    expect([ctx.listener.positionX?.value, ctx.listener.positionY?.value, ctx.listener.positionZ?.value]).toEqual([
      1, 2, 3,
    ]);
    expect(ctx.listener.forwardZ?.value).toBe(-1);
    expect(ctx.sources.at(-1)?.buffer?.stem).toMatch(/^one_way_gate_swing_v0[123]$/);
  });

  it("pitches impacts and rolls by marble size, with the jitter on top", () => {
    const { ctx, e } = engine();
    const lo = 2 ** (-JITTER_SEMITONES / 12);
    const hi = 2 ** (JITTER_SEMITONES / 12);
    for (const [i, size] of (["mini", "standard", "shooter"] as const).entries()) {
      ctx.currentTime += 1;
      e.consume(frame(4 + i * 4, { contacts: [contact({ marble: i, size, impulse: 0.002 })] }));
      const r = (ctx.sources.at(-1)?.playbackRate.value ?? 0) / sizeRate(size);
      expect(r).toBeGreaterThanOrEqual(lo);
      expect(r).toBeLessThanOrEqual(hi);
    }
    const { ctx: c2, e: e2 } = engine();
    e2.consume(frame(4, { rolls: [roll(0, { size: "mini", speed: ROLL_REF_SPEED })] }));
    expect(c2.sources.at(-1)?.playbackRate.value).toBeCloseTo(sizeRate("mini"), 10);
  });

  it("plays the matching cue for every mapped mechanism signal, and nothing for the rest", () => {
    const signal = (piece: string, name: string, value = 0): AudioMechanism => ({
      kind: "signal",
      tick: 4,
      instance: [0, 0, 0],
      piece,
      name,
      value,
    });
    const played = (mechanisms: AudioMechanism[], rolls: AudioRoll[] = []) => {
      const { ctx, e } = engine();
      e.consume(frame(4, { mechanisms, rolls }));
      expect(e.stats().errors).toBe(0);
      return ctx.sources.filter((s) => !s.loop).map((s) => s.buffer?.stem.replace(/_v0\d$/, ""));
    };
    const want: [string, string, string][] = [
      ["mech_hopper", "release", "hopper_release"],
      ["mech_finish", "finish", "finish_cup_chime"],
      ["mech_checkpoint_l1", "split", "checkpoint_tick"],
      ["mech_splitter_l3", "set", "splitter_tick"],
      ["mech_toggle_l3", "set", "toggle_flap_clack"],
      ["mech_gate_oneway_l1", "open", "one_way_gate_swing"],
      ["mech_hole_oneway_l1", "open", "trapdoor_drop"],
      ["mech_hole_oneway_l1", "drop", "drop_hole_glass"],
    ];
    for (const [piece, name, cue] of want) expect(played([signal(piece, name)]), `${piece} ${name}`).toEqual([cue]);
    const mapped = [...SIGNAL_CUES].flatMap(([piece, m]) => [...m.keys()].map((n) => `${piece} ${n}`));
    expect(mapped.sort()).toEqual(want.map(([p, n]) => `${p} ${n}`).sort());

    // The drop takes the dropping marble's material from its held roll.
    const steel = roll(3, { kind: "steel", contact: false, surface: null });
    expect(played([signal("mech_hole_oneway_l1", "drop", 3)], [steel])).toEqual(["drop_hole_steel"]);
    expect(signalCueId("mech_hole_oneway_l1", "drop", "solid")).toBe("drop_hole_solid");

    const silent: AudioMechanism[] = [
      ...[
        ["mech_hopper", "gate_open"],
        ["mech_hopper", "gate_close"],
        ["mech_finish", "finish_count"],
        ["mech_checkpoint_l1", "camera_cue"],
        ["mech_splitter_l3", "pass"],
        ["mech_toggle_l3", "pass"],
        ["mech_gate_oneway_l1", "forward"],
        ["mech_gate_oneway_l1", "reverse"],
        ["mech_gate_oneway_l1", "close"],
        ["mech_hole_oneway_l1", "over"],
        ["mech_hole_oneway_l1", "close"],
        ["", "open"],
        ["mech_mystery", "release"],
        ["toString", "name"],
        ["fixture_swing_flap", "swing"],
      ].map(([p, n]) => signal(p as string, n as string)),
      { kind: "sensorEnter", tick: 4, instance: [0, 0, 0], piece: "mech_gate_oneway_l1", sensor: 0, marble: 0 },
      { kind: "sensorExit", tick: 4, instance: [0, 0, 0], piece: "mech_hopper", sensor: 0, marble: 0 },
    ];
    expect(played(silent)).toEqual([]);
  });

  it("maps mechanism signals only to approved, staged cue ids", () => {
    const approved = new Set(Object.keys(picks).filter((k) => Array.isArray((picks as Record<string, unknown>)[k])));
    expect(SIGNAL_CUE_IDS.filter((id) => !approved.has(id))).toEqual([]);
    expect(SIGNAL_CUE_IDS).toHaveLength(10);
  });
});
