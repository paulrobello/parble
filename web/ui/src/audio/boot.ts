// Browser glue for the audio engine: fetch the staged SFX (web/ui/audio, versioned from
// window.__PARBLE_HASHES), create and resume the AudioContext on the first user gesture, decode, and
// expose window.__parbleAudio for drivers (docs/agent-operability.md). The music player (music.ts)
// rides on the engine's music bus and fetches one cue at a time; the mixer panel (mixer.ts) sets the
// bus gains.

import { type AudioFrame, decodeEvent } from "../bridge";
import { type Catalog, decodeCatalog } from "./catalog";
import { AudioEngine, type AudioStats } from "./engine";
import { ROLL_MIN_SPEED } from "./mapping";
import { busGains, type MixerSettings, mountMixer } from "./mixer";
import {
  decodeMusicCatalog,
  MUSIC_STATES,
  type MusicCatalog,
  MusicDirector,
  MusicPlayer,
  type MusicState,
  type MusicStats,
} from "./music";

/** Site path of the staged audio (scripts/web/stage-audio.mjs into web/ui/dist/audio, copied to web/ui). */
const BASE = "web/ui/audio/";
const FETCH_CONCURRENCY = 6;
const GESTURES = ["pointerdown", "keydown", "touchend"] as const;

export type AudioState = "locked" | "starting" | "running" | "suspended" | "closed" | "unsupported";

export interface AudioDiagStats extends Partial<AudioStats> {
  state: AudioState;
  format: "ogg" | "m4a";
  cues: number;
  files: number;
  fetched: number;
  decoded: number;
  loadErrors: number;
  /** Frames handed over while the context was not running (skipped). */
  skippedFrames: number;
  injected: boolean;
  music: MusicDiagStats;
  mixer: MixerSettings;
  /** The AudioContext's rate, absent before the first gesture; decoded music is resampled to it. */
  sampleRate?: number;
}

export interface MusicDiagStats extends Partial<MusicStats> {
  /** Cues in music.json (0 until it loads). */
  cues: number;
  /** The pinned state (?music= or musicForce), or null when the director decides. */
  forced: MusicState | "off" | null;
  catalogError: boolean;
}

export interface AudioDiag {
  stats(): AudioDiagStats;
  /** Test hook: decode `raw` as a bridge event and feed it if it is an audioFrame. From then on live bridge frames are ignored. */
  inject(raw: unknown): boolean;
  /** Test hook: restart the playing music cue at file position `seconds`. */
  musicSeek(seconds: number): boolean;
  /** Test hook: pin the music state ("off" silences it), or null to hand it back to the director. */
  musicForce(state: MusicState | "off" | null): void;
  /** Test hook: RMS of the music bus over the last 2048 samples (before the master and the mute). */
  musicLevel(): number;
  /** Test hook: the master peak meter and the followed-marble check; the first call starts them, `reset` clears them. */
  meter(reset?: boolean): MeterReading;
  /** Test hook: record the main-thread ms of every fed frame (engine consume plus the music update); `false` stops. */
  timing(on: boolean): void;
  /** The recorded frame timings, ms, oldest first (at most 65,536). */
  timings(): number[];
}

/** The peak meter's analyser window: 32,768 samples (683 ms at 48 kHz), read every fed frame, so no sample is missed while frames come faster than that. */
const METER_FFT = 32_768;
/** Most frame timings `timing()` keeps. */
const TIMING_MAX = 65_536;

export interface MeterReading {
  /** Whether the meter runs (it starts on the first `meter()` call, once the engine exists). */
  running: boolean;
  /** Largest |sample| at the master output, after the limiter and before the mute gain. */
  peak: number;
  peakDb: number;
  /** Reads whose window held a sample at or above 1.0 (0 dBFS). */
  clippedReads: number;
  /** The same peak before the limiter (the master bus), for the limiter's share. */
  prePeak: number;
  reads: number;
  /** Longest wall time between two reads, ms: coverage is gapless while it stays under `windowMs`. */
  maxGapMs: number;
  windowMs: number;
  /** Fed frames whose followed marble (`__parble.camera.director.followed`) was rolling (`RollFrame.audible`). */
  focusFrames: number;
  /** Of those, frames it held no sounding roll voice (culled, stolen or no buffer). */
  focusSilent: number;
  /** The followed marbles seen while rolling. */
  focusMarbles: number[];
  /** Most marbles rolling (audible) in one frame. */
  maxRolling: number;
}

const MUSIC_PARAM = new Set<string>(["off", ...MUSIC_STATES]);

declare global {
  interface Window {
    __PARBLE_HASHES?: Record<string, string>;
    __parbleAudio?: AudioDiag;
    AudioContext?: typeof AudioContext;
    webkitAudioContext?: typeof AudioContext;
  }
}

function versioned(win: Window, path: string): string {
  const site = BASE + path;
  const v = win.__PARBLE_HASHES?.[site];
  return v ? `${site}?v=${v}` : site;
}

async function pool<T>(items: readonly T[], n: number, work: (item: T) => Promise<void>): Promise<void> {
  let next = 0;
  const run = async (): Promise<void> => {
    while (next < items.length) {
      const item = items[next++] as T;
      await work(item);
    }
  };
  await Promise.all(Array.from({ length: Math.min(n, items.length) }, run));
}

function storageOf(win: Window): Storage | null {
  try {
    return win.localStorage;
  } catch {
    return null;
  }
}

export interface AudioHandle {
  consume(frame: AudioFrame): void;
}

export function startAudio(win: Window): AudioHandle {
  const muted = new URLSearchParams(win.location.search).get("mute") === "1";
  const Ctx = win.AudioContext ?? win.webkitAudioContext;
  const probe = win.document.createElement("audio");
  const format: "ogg" | "m4a" = probe.canPlayType('audio/ogg; codecs="opus"') !== "" ? "ogg" : "m4a";
  const diag: AudioDiagStats = {
    state: Ctx ? "locked" : "unsupported",
    format,
    cues: 0,
    files: 0,
    fetched: 0,
    decoded: 0,
    loadErrors: 0,
    skippedFrames: 0,
    injected: false,
    music: { cues: 0, forced: null, catalogError: false },
    mixer: { master: 1, music: 1, sfx: 1, muted: false },
  };
  const musicParam = new URLSearchParams(win.location.search).get("music");
  let forced: MusicState | "off" | null =
    musicParam && MUSIC_PARAM.has(musicParam) ? (musicParam as MusicState | "off") : null;
  diag.music.forced = forced;
  const director = new MusicDirector();
  let musicCatalog: MusicCatalog | null = null;
  let player: MusicPlayer | null = null;
  let analyser: AnalyserNode | null = null;
  let mixer: MixerSettings = diag.mixer;
  let catalog: Catalog | null = null;
  let ctx: AudioContext | null = null;
  let engine: AudioEngine | null = null;
  const raw = new Map<string, ArrayBuffer>();
  let meter: {
    post: AnalyserNode;
    pre: AnalyserNode;
    buf: Float32Array<ArrayBuffer>;
    last: number;
    focus: Set<number>;
    r: MeterReading;
  } | null = null;
  let timing: number[] | null = null;

  const loadFail = (what: string, err: unknown): void => {
    diag.loadErrors++;
    console.warn(`parble audio: could not load ${what}`, err);
  };

  const applyMixer = (s: MixerSettings): void => {
    mixer = diag.mixer = s;
    if (!engine) return;
    const g = busGains(s);
    for (const bus of ["master", "music", "sfx"] as const) engine.setBusGain(bus, g[bus]);
  };

  const startMusic = (): void => {
    if (player || !ctx || !engine || !musicCatalog) return;
    const c = ctx;
    const cat = musicCatalog;
    // musicLevel's tap, connected now so its first read already holds audio (a fresh analyser reads zeros).
    analyser = c.createAnalyser();
    analyser.fftSize = 2048;
    engine.bus("music").connect(analyser);
    player = new MusicPlayer(c, engine.bus("music"), cat, async (cue) => {
      const entry = cat.cues[cue];
      if (!entry) throw new Error(`no music cue ${cue}`);
      const r = await fetch(versioned(win, format === "ogg" ? entry.ogg : entry.m4a));
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      return c.decodeAudioData(await r.arrayBuffer());
    });
  };

  const newEngine = (c: AudioContext, cat: Catalog): AudioEngine => {
    const e = new AudioEngine(c, { catalog: cat, muted });
    engine = e;
    applyMixer(mixer);
    startMusic();
    return e;
  };

  const loadMusic = async (): Promise<void> => {
    const res = await fetch(versioned(win, "music.json"));
    if (!res.ok) throw new Error(`music.json: HTTP ${res.status}`);
    musicCatalog = decodeMusicCatalog(await res.json());
    if (!musicCatalog) throw new Error("music.json is not a parble.music-runtime/1 catalog");
    diag.music.cues = Object.keys(musicCatalog.cues).length;
    startMusic();
  };

  const decode = (stem: string, bytes: ArrayBuffer): void => {
    if (!ctx || !engine) return;
    const e = engine;
    raw.delete(stem);
    ctx.decodeAudioData(bytes).then(
      (buf) => {
        e.addBuffer(stem, buf);
        diag.decoded++;
      },
      (err: unknown) => loadFail(`${stem} (decode)`, err),
    );
  };

  const load = async (): Promise<void> => {
    const res = await fetch(versioned(win, "sfx.json"));
    if (!res.ok) throw new Error(`sfx.json: HTTP ${res.status}`);
    catalog = decodeCatalog(await res.json());
    if (!catalog) throw new Error("sfx.json is not a parble.sfx-runtime/1 catalog");
    if (ctx && !engine) newEngine(ctx, catalog);
    const variants = Object.values(catalog.cues).flatMap((c) => c.variants);
    diag.cues = Object.keys(catalog.cues).length;
    diag.files = variants.length;
    await pool(variants, FETCH_CONCURRENCY, async (v) => {
      try {
        const r = await fetch(versioned(win, format === "ogg" ? v.ogg : v.m4a));
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        const bytes = await r.arrayBuffer();
        diag.fetched++;
        if (engine) decode(v.stem, bytes);
        else raw.set(v.stem, bytes);
      } catch (err) {
        loadFail(v.stem, err);
      }
    });
  };

  const syncState = (): void => {
    if (!ctx) return;
    const st = String(ctx.state);
    diag.state = st === "interrupted" ? "suspended" : (st as AudioState);
  };

  const unlock = (): void => {
    if (!Ctx) return;
    let c = ctx;
    if (!c) {
      c = ctx = new Ctx({ latencyHint: "interactive" });
      c.onstatechange = syncState;
      diag.state = "starting";
      if (catalog) newEngine(c, catalog);
      for (const [stem, bytes] of [...raw]) decode(stem, bytes);
    }
    if (c.state !== "running")
      c.resume().then(syncState, (err: unknown) => console.warn("parble audio: resume refused", err));
    else for (const g of GESTURES) win.removeEventListener(g, unlock, true);
  };

  if (Ctx) {
    for (const g of GESTURES) win.addEventListener(g, unlock, true);
    load().catch((err: unknown) => loadFail("the SFX catalog", err));
    loadMusic().catch((err: unknown) => {
      diag.music.catalogError = true;
      console.warn("parble audio: could not load the music catalog", err);
    });
    mountMixer(win.document, storageOf(win), applyMixer);
  }

  const steerMusic = (frame: AudioFrame): void => {
    const state = director.update({
      now: win.performance.now() / 1000,
      paused: frame.paused === true,
      editor: ((win as unknown as { __parble?: { editor?: unknown } }).__parble?.editor ?? null) !== null,
      moving: frame.rolls.some((r) => r.contact && r.speed > ROLL_MIN_SPEED),
    });
    if (!player) return;
    player.setState(forced ?? state);
    player.update();
  };

  const emptyReading = (running: boolean, windowMs: number): MeterReading => ({
    running,
    peak: 0,
    peakDb: Number.NEGATIVE_INFINITY,
    clippedReads: 0,
    prePeak: 0,
    reads: 0,
    maxGapMs: 0,
    windowMs,
    focusFrames: 0,
    focusSilent: 0,
    focusMarbles: [],
    maxRolling: 0,
  });

  const peakOf = (a: AnalyserNode, buf: Float32Array<ArrayBuffer>): number => {
    a.getFloatTimeDomainData(buf);
    let p = 0;
    for (const v of buf) p = Math.max(p, Math.abs(v));
    return p;
  };

  const readMeter = (): void => {
    if (!meter || !engine) return;
    const m = meter;
    const r = m.r;
    const t = win.performance.now();
    if (r.reads > 0) r.maxGapMs = Math.max(r.maxGapMs, t - m.last);
    m.last = t;
    r.reads++;
    const post = peakOf(m.post, m.buf);
    if (post >= 1) r.clippedReads++;
    r.peak = Math.max(r.peak, post);
    r.peakDb = 20 * Math.log10(r.peak);
    r.prePeak = Math.max(r.prePeak, peakOf(m.pre, m.buf));
    const followed = (win as unknown as { __parble?: { camera?: { director?: { followed?: number | null } } } })
      .__parble?.camera?.director?.followed;
    const rolls = engine.rollFrame();
    r.maxRolling = Math.max(r.maxRolling, rolls.audible.length);
    if (typeof followed === "number" && rolls.audible.includes(followed)) {
      r.focusFrames++;
      if (!rolls.voiced.includes(followed)) r.focusSilent++;
      if (!m.focus.has(followed)) {
        m.focus.add(followed);
        r.focusMarbles = [...m.focus].sort((a, b) => a - b);
      }
    }
  };

  const feed = (frame: AudioFrame): void => {
    const t0 = timing ? win.performance.now() : 0;
    const running = engine && ctx?.state === "running";
    if (running) engine?.consume(frame);
    else diag.skippedFrames++;
    steerMusic(frame);
    if (timing) {
      if (timing.length >= TIMING_MAX) timing.shift();
      timing.push(win.performance.now() - t0);
    }
    if (running) readMeter();
  };

  win.__parbleAudio = {
    stats: () => ({
      ...diag,
      ...(engine ? engine.stats() : {}),
      music: { ...diag.music, ...(player ? player.stats() : {}) },
      mixer: { ...mixer },
      ...(ctx ? { sampleRate: ctx.sampleRate } : {}),
    }),
    musicSeek: (s) => player?.seek(s) ?? false,
    musicForce: (state) => {
      forced = diag.music.forced = state;
      if (state) player?.setState(state);
    },
    musicLevel: () => {
      if (!analyser) return 0;
      const buf = new Float32Array(analyser.fftSize);
      analyser.getFloatTimeDomainData(buf);
      let sum = 0;
      for (const v of buf) sum += v * v;
      return Math.sqrt(sum / buf.length);
    },
    meter: (reset) => {
      if (!meter && engine && ctx) {
        const tap = (node: AudioNode): AnalyserNode => {
          const a = (ctx as AudioContext).createAnalyser();
          a.fftSize = METER_FFT;
          node.connect(a);
          return a;
        };
        meter = {
          post: tap(engine.output()),
          pre: tap(engine.bus("master")),
          buf: new Float32Array(METER_FFT),
          last: 0,
          focus: new Set(),
          r: emptyReading(true, (METER_FFT / ctx.sampleRate) * 1000),
        };
      } else if (meter && reset) {
        meter.focus.clear();
        meter.r = emptyReading(true, meter.r.windowMs);
      }
      return meter ? { ...meter.r, focusMarbles: [...meter.r.focusMarbles] } : emptyReading(false, 0);
    },
    timing: (on) => {
      timing = on ? (timing ?? []) : null;
    },
    timings: () => [...(timing ?? [])],
    inject: (v) => {
      const e = decodeEvent(v);
      if (e?.type !== "audioFrame") return false;
      diag.injected = true;
      feed(e);
      return true;
    },
  };

  return {
    consume: (frame) => {
      if (!diag.injected) feed(frame);
    },
  };
}
