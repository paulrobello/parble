import { describe, expect, it } from "vitest";
import { BUS_DEFAULTS } from "../src/audio/engine";
import { busGains, defaultMixer, loadMixer, MIXER_KEY, parseMixer, saveMixer } from "../src/audio/mixer";
import {
  DWELL_S,
  decodeMusicCatalog,
  equalPowerCurve,
  IDLE_S,
  LOOPS_PER_CUE,
  loopPosition,
  loopSeconds,
  MAX_BUFFERS,
  type MusicCatalog,
  MusicDirector,
  MusicPlayer,
  PLAYLISTS,
  XFADE_S,
} from "../src/audio/music";

const SR = 48_000;
const cue = (start: number, end: number, samples = (end + 1) * SR) => ({
  ogg: "x.ogg",
  m4a: "x.m4a",
  sampleRate: SR,
  samples,
  loopStart: start * SR,
  loopEnd: end * SR,
});
const CATALOG: MusicCatalog = {
  schema: "parble.music-runtime/1",
  cues: {
    music_build_calm: cue(4, 80),
    music_build_playful: cue(20, 100),
    music_run_flow: cue(4, 84),
    music_night_lanterns: cue(5, 90),
    music_title: cue(15, 67),
  },
};

describe("music catalog", () => {
  it("decodes loop points and refuses bad ones", () => {
    const raw = JSON.parse(JSON.stringify(CATALOG));
    expect(decodeMusicCatalog(raw)).toEqual(CATALOG);
    expect(decodeMusicCatalog({ ...raw, schema: "x" })).toBeNull();
    const bad = (o: object) => decodeMusicCatalog({ ...raw, cues: { a: { ...raw.cues.music_title, ...o } } });
    expect(bad({ loopEnd: 1 })).toBeNull();
    expect(bad({ loopEnd: 99 * SR })).toBeNull();
    expect(bad({ sampleRate: 0 })).toBeNull();
    expect(bad({ loopStart: 1.5 })).toBeNull();
    expect(loopSeconds(CATALOG.cues.music_title as never)).toEqual({ start: 15, end: 67, length: 52 });
  });

  it("maps every state to cues the catalog has", () => {
    for (const list of Object.values(PLAYLISTS)) for (const c of list) expect(CATALOG.cues[c]).toBeDefined();
  });
});

describe("loop position", () => {
  const loop = { start: 10, end: 30 };
  it("plays the intro once, then wraps the body", () => {
    expect(loopPosition(5, 0, loop)).toEqual({ position: 5, wraps: 0 });
    expect(loopPosition(29.5, 0, loop)).toEqual({ position: 29.5, wraps: 0 });
    expect(loopPosition(30, 0, loop)).toEqual({ position: 10, wraps: 1 });
    expect(loopPosition(55, 0, loop)).toEqual({ position: 15, wraps: 2 });
    expect(loopPosition(1, 29.5, loop).wraps).toBe(1);
    expect(loopPosition(1, 29.5, loop).position).toBeCloseTo(10.5, 10);
  });

  it("uses equal-power fades", () => {
    const fin = equalPowerCurve(64, false);
    const fout = equalPowerCurve(64, true);
    expect(fin[0]).toBe(0);
    expect(fin[63]).toBeCloseTo(1, 6);
    expect(fout[0]).toBe(1);
    expect(fout[63]).toBeCloseTo(0, 6);
    for (let i = 0; i < 64; i++) expect((fin[i] ?? 0) ** 2 + (fout[i] ?? 0) ** 2).toBeCloseTo(1, 6);
  });
});

describe("music director", () => {
  const at = (now: number, o: Partial<{ paused: boolean; editor: boolean; moving: boolean }> = {}) => ({
    now,
    paused: false,
    editor: false,
    moving: false,
    ...o,
  });

  it("takes the first state at once, then follows only after the dwell", () => {
    const d = new MusicDirector();
    expect(d.update(at(0, { editor: true }))).toBe("build");
    expect(d.update(at(1, { editor: true, moving: true }))).toBe("build");
    expect(d.update(at(1 + DWELL_S - 0.1, { editor: true, moving: true }))).toBe("build");
    expect(d.update(at(1 + DWELL_S, { editor: true, moving: true }))).toBe("run");
  });

  it("does not flap when the wanted state flickers", () => {
    const d = new MusicDirector();
    d.update(at(0, { moving: true }));
    for (let t = 0; t < 60; t += 0.5) expect(d.update(at(t, { moving: true, paused: t % 2 === 0 }))).toBe("run");
  });

  it("goes to night when paused or idle outside the editor, and to build when the editor is still", () => {
    const d = new MusicDirector();
    d.update(at(0, { moving: true }));
    expect(d.update(at(1, { paused: true }))).toBe("run");
    expect(d.update(at(1 + DWELL_S, { paused: true }))).toBe("night");
    const idle = new MusicDirector();
    idle.update(at(0, { moving: true }));
    expect(idle.wanted(at(IDLE_S - 1))).toBe("run");
    expect(idle.wanted(at(IDLE_S))).toBe("night");
    expect(idle.wanted(at(IDLE_S, { editor: true }))).toBe("build");
  });

  it("holds a forced state until released", () => {
    const d = new MusicDirector();
    d.force("title");
    expect(d.update(at(0, { moving: true }))).toBe("title");
    expect(d.forced).toBe("title");
    d.force(null);
    expect(d.update(at(100, { moving: true }))).toBe("title");
    expect(d.update(at(100 + DWELL_S, { moving: true }))).toBe("run");
  });
});

class Param {
  value = 1;
  events: { kind: string; at: number; duration?: number; curve?: number[] }[] = [];
  setValueCurveAtTime(curve: Float32Array, at: number, duration: number): void {
    this.events.push({ kind: "curve", at, duration, curve: [...curve] });
  }
  cancelAndHoldAtTime(at: number): void {
    this.events.push({ kind: "hold", at });
  }
  cancelScheduledValues(at: number): void {
    this.events.push({ kind: "cancel", at });
  }
  setTargetAtTime(v: number, at: number): void {
    this.value = v;
    this.events.push({ kind: "target", at });
  }
}
class Node {
  connected: unknown[] = [];
  disconnected = false;
  connect<T>(n: T): T {
    this.connected.push(n);
    return n;
  }
  disconnect(): void {
    this.disconnected = true;
  }
}
class Gain extends Node {
  gain = new Param();
}
class Source extends Node {
  buffer: { duration: number } | null = null;
  loop = false;
  loopStart = 0;
  loopEnd = 0;
  startedAt: number | null = null;
  offset = 0;
  stopAt: number | null = null;
  start(when: number, offset = 0): void {
    this.startedAt = when;
    this.offset = offset;
  }
  stop(when: number): void {
    this.stopAt = when;
  }
}
class FakeContext {
  currentTime = 0;
  sources: Source[] = [];
  gains: Gain[] = [];
  createGain = () => {
    const g = new Gain();
    this.gains.push(g);
    return g;
  };
  createBufferSource = () => {
    const s = new Source();
    this.sources.push(s);
    return s;
  };
}

function setup() {
  const ctx = new FakeContext();
  const out = new Node();
  const loads: string[] = [];
  const pending = new Map<string, () => void>();
  const player = new MusicPlayer(ctx as unknown as BaseAudioContext, out as unknown as AudioNode, CATALOG, (c) => {
    loads.push(c);
    return new Promise((resolve) => {
      const entry = CATALOG.cues[c];
      pending.set(c, () => resolve({ duration: (entry?.samples ?? 0) / SR } as AudioBuffer));
    });
  });
  const resolveLoad = async (c: string) => {
    pending.get(c)?.();
    pending.delete(c);
    await Promise.resolve();
    await Promise.resolve();
  };
  return { ctx, out, loads, player, resolveLoad };
}

describe("music player", () => {
  it("starts a looping source on the music bus once the cue decodes", async () => {
    const { ctx, out, loads, player, resolveLoad } = setup();
    player.setState("run");
    expect(loads).toEqual(["music_run_flow"]);
    expect(ctx.sources).toHaveLength(0);
    expect(player.stats().waiting).toBe(1);
    await resolveLoad("music_run_flow");
    expect(ctx.sources).toHaveLength(1);
    const src = ctx.sources[0] as Source;
    expect(src.loop).toBe(true);
    expect(src.loopStart).toBe(4);
    expect(src.loopEnd).toBe(84);
    expect(src.startedAt).toBe(0);
    expect(src.offset).toBe(0);
    expect(ctx.gains[0]?.connected).toContain(out);
    const s = player.stats();
    expect(s).toMatchObject({ state: "run", cue: "music_run_flow", loop: true, source: 1, starts: 1, crossfades: 0 });
    expect(s.bufferDuration).toBe(85);
  });

  it("reports the position wrapping on the audio clock without a new source", async () => {
    const { ctx, player, resolveLoad } = setup();
    player.setState("run");
    await resolveLoad("music_run_flow");
    for (const [t, pos, wraps] of [
      [83, 83, 0],
      [84, 4, 1],
      [84 + 80 + 1, 5, 2],
    ] as const) {
      ctx.currentTime = t;
      player.update();
      expect(player.stats()).toMatchObject({ position: pos, wraps, source: 1, starts: 1 });
    }
    expect(ctx.sources).toHaveLength(1);
  });

  it("crossfades with equal-power curves when the state changes, and keeps the old cue until the new one decodes", async () => {
    const { ctx, player, resolveLoad } = setup();
    player.setState("run");
    await resolveLoad("music_run_flow");
    ctx.currentTime = 10;
    player.setState("night");
    expect(player.stats().cue).toBe("music_run_flow");
    expect(ctx.sources[0]?.stopAt).toBeNull();
    await resolveLoad("music_night_lanterns");
    const [oldG, newG] = ctx.gains;
    expect(newG?.gain.value).toBe(0);
    expect(newG?.gain.events).toEqual([expect.objectContaining({ kind: "curve", at: 10, duration: XFADE_S })]);
    const curveIn = newG?.gain.events[0]?.curve ?? [];
    expect(curveIn[0]).toBe(0);
    expect(curveIn.at(-1)).toBeCloseTo(1, 6);
    const curveOut = oldG?.gain.events.find((e) => e.kind === "curve")?.curve ?? [];
    expect(curveOut[0]).toBe(1);
    expect(curveOut.at(-1)).toBeCloseTo(0, 6);
    expect(ctx.sources[0]?.stopAt).toBe(10 + XFADE_S);
    expect(player.stats()).toMatchObject({ cue: "music_night_lanterns", crossfades: 1, source: 2 });
    ctx.currentTime = 10 + XFADE_S;
    player.update();
    expect(ctx.sources[0]?.disconnected).toBe(true);
  });

  it(`moves a multi-cue playlist on after ${LOOPS_PER_CUE} loops and holds at most ${MAX_BUFFERS} buffers`, async () => {
    const { ctx, loads, player, resolveLoad } = setup();
    player.setState("build");
    await resolveLoad("music_build_calm");
    const calm = loopSeconds(CATALOG.cues.music_build_calm as never);
    ctx.currentTime = calm.end + calm.length * (LOOPS_PER_CUE - 1) - 0.01;
    player.update();
    expect(loads).toEqual(["music_build_calm"]);
    ctx.currentTime += 0.02;
    player.update();
    expect(loads).toEqual(["music_build_calm", "music_build_playful"]);
    await resolveLoad("music_build_playful");
    expect(player.stats()).toMatchObject({ cue: "music_build_playful", crossfades: 1, buffers: 2 });
    ctx.currentTime += XFADE_S + 1;
    player.setState("run");
    await resolveLoad("music_run_flow");
    expect(player.stats().buffers).toBeLessThanOrEqual(MAX_BUFFERS);
    expect(player.stats().cue).toBe("music_run_flow");
  });

  it("seeks with a new source and fades out on off", async () => {
    const { ctx, player, resolveLoad } = setup();
    player.setState("title");
    await resolveLoad("music_title");
    ctx.currentTime = 2;
    expect(player.seek(66)).toBe(true);
    expect(ctx.sources[1]?.offset).toBe(66);
    expect(player.stats()).toMatchObject({ source: 2, position: 66, wraps: 0 });
    ctx.currentTime = 3.5;
    expect(player.stats()).toMatchObject({ source: 2, wraps: 1 });
    expect(player.stats().position).toBeCloseTo(15.5, 6);
    player.setState("off");
    expect(player.stats()).toMatchObject({ state: "off", cue: null, source: 0 });
    expect(ctx.sources[1]?.stopAt).toBeGreaterThan(3.5);
    expect(player.seek(1)).toBe(false);
  });

  it("counts a failed load and keeps running", async () => {
    const ctx = new FakeContext();
    const p = new MusicPlayer(ctx as unknown as BaseAudioContext, new Node() as unknown as AudioNode, CATALOG, () =>
      Promise.reject(new Error("404")),
    );
    const warn = console.warn;
    console.warn = () => {};
    try {
      p.setState("run");
      await Promise.resolve();
      await Promise.resolve();
    } finally {
      console.warn = warn;
    }
    expect(p.stats()).toMatchObject({ loadErrors: 1, cue: null });
  });
});

class MemStorage {
  data = new Map<string, string>();
  getItem(k: string): string | null {
    return this.data.get(k) ?? null;
  }
  setItem(k: string, v: string): void {
    this.data.set(k, v);
  }
}

describe("mixer settings", () => {
  it("defaults reproduce the engine's bus gains", () => {
    expect(busGains(defaultMixer())).toEqual(BUS_DEFAULTS);
  });

  it("maps a slider position to the bus default times its square, and mute zeroes the master", () => {
    const g = busGains({ master: 0.5, music: 0.5, sfx: 0, muted: false });
    expect(g.master).toBeCloseTo(BUS_DEFAULTS.master * 0.25, 10);
    expect(g.music).toBeCloseTo(BUS_DEFAULTS.music * 0.25, 10);
    expect(g.sfx).toBe(0);
    expect(busGains({ ...defaultMixer(), muted: true }).master).toBe(0);
  });

  it("round-trips through storage", () => {
    const st = new MemStorage();
    const s = { master: 0.8, music: 0.25, sfx: 0.6, muted: true };
    expect(saveMixer(st, s)).toBe(true);
    expect(st.data.has(MIXER_KEY)).toBe(true);
    expect(loadMixer(st)).toEqual(s);
  });

  it("falls back per field on garbage and when storage throws", () => {
    expect(parseMixer(null)).toEqual(defaultMixer());
    expect(parseMixer("{not json")).toEqual(defaultMixer());
    expect(parseMixer("[1,2]")).toEqual(defaultMixer());
    expect(parseMixer(JSON.stringify({ master: 2, music: -1, sfx: "x", muted: 1 }))).toEqual({
      master: 1,
      music: 0,
      sfx: 1,
      muted: false,
    });
    expect(parseMixer(JSON.stringify({ music: 0.3 }))).toEqual({ ...defaultMixer(), music: 0.3 });
    const throwing = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("quota");
      },
    };
    expect(loadMixer(throwing)).toEqual(defaultMixer());
    expect(saveMixer(throwing, defaultMixer())).toBe(false);
    expect(loadMixer(null)).toEqual(defaultMixer());
    expect(saveMixer(null, defaultMixer())).toBe(false);
  });
});
