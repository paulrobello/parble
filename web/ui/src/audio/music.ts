// The music player (P4-T5): a crossfading playlist on the engine's music bus.
//
// Each cue is one runtime file (scripts/audio/music_loops.py): an intro, the loop body [loopStart,
// loopEnd) whose tail is already crossfaded into the audio before loopStart, and a short post-roll.
// One AudioBufferSourceNode per cue plays it with `loop`, `loopStart` and `loopEnd`, so the intro
// plays once and the body repeats gaplessly on the audio clock.
//
// The director picks a state from what the HUD sees (pause, rolling marbles, the editor) with a
// minimum dwell, and each state has a playlist (the "Use:" lines of docs/audio/suno-prompts.md):
//   build  music_build_calm, music_build_playful (the editor; alternate every LOOPS_PER_CUE loops)
//   run    music_run_flow (marbles rolling)
//   night  music_night_lanterns (paused, or idle for IDLE_S outside the editor)
//   title  music_title (the P5 title screen; forced with ?music=title until then)
// A state or playlist change crossfades the old and new sources with equal-power curves.
// Decoded buffers are large (~40 MB per stereo cue), so at most MAX_BUFFERS stay decoded.

export const MUSIC_SCHEMA = "parble.music-runtime/1";

export interface MusicCue {
  ogg: string;
  m4a: string;
  sampleRate: number;
  samples: number;
  loopStart: number;
  loopEnd: number;
}

export interface MusicCatalog {
  schema: typeof MUSIC_SCHEMA;
  cues: Record<string, MusicCue>;
}

export const MUSIC_STATES = ["title", "build", "run", "night"] as const;
export type MusicState = (typeof MUSIC_STATES)[number];

export const PLAYLISTS: Record<MusicState, readonly string[]> = {
  title: ["music_title"],
  build: ["music_build_calm", "music_build_playful"],
  run: ["music_run_flow"],
  night: ["music_night_lanterns"],
};

/** Loop iterations of a cue before a multi-cue playlist moves on. */
export const LOOPS_PER_CUE = 2;
/** Crossfade between cues, s. */
export const XFADE_S = 4;
/** Fade out when the music stops (`off`), s. */
export const STOP_FADE_S = 2;
/** Decoded buffers kept: the cue playing and the one fading in. */
export const MAX_BUFFERS = 2;
/** A state must be wanted this long before the music follows it, s. */
export const DWELL_S = 6;
/** Outside the editor, no rolling marble for this long is the idle (night) state, s. */
export const IDLE_S = 30;

type Obj = Record<string, unknown>;
const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);
const isCount = (v: unknown): v is number => typeof v === "number" && Number.isInteger(v) && v >= 0;

/** `v` as a music catalog, or null when it is not one (bad schema or loop points). */
export function decodeMusicCatalog(v: unknown): MusicCatalog | null {
  if (!isObj(v) || v.schema !== MUSIC_SCHEMA || !isObj(v.cues)) return null;
  const cues: Record<string, MusicCue> = {};
  for (const [id, c] of Object.entries(v.cues)) {
    if (!isObj(c) || typeof c.ogg !== "string" || typeof c.m4a !== "string") return null;
    const { sampleRate, samples, loopStart, loopEnd } = c;
    if (!isCount(sampleRate) || !isCount(samples) || !isCount(loopStart) || !isCount(loopEnd)) return null;
    if (sampleRate === 0 || !(loopStart < loopEnd && loopEnd <= samples)) return null;
    cues[id] = { ogg: c.ogg, m4a: c.m4a, sampleRate, samples, loopStart, loopEnd };
  }
  return { schema: MUSIC_SCHEMA, cues };
}

/** Loop points of `cue` in seconds. */
export const loopSeconds = (cue: MusicCue): { start: number; end: number; length: number } => ({
  start: cue.loopStart / cue.sampleRate,
  end: cue.loopEnd / cue.sampleRate,
  length: (cue.loopEnd - cue.loopStart) / cue.sampleRate,
});

/**
 * Where a looping source is after `elapsed` seconds of playback that began at file offset `offset`:
 * the file position and how many times it has wrapped from loopEnd back to loopStart.
 */
export function loopPosition(
  elapsed: number,
  offset: number,
  loop: { start: number; end: number },
): { position: number; wraps: number } {
  const t = offset + Math.max(0, elapsed);
  if (t < loop.end) return { position: t, wraps: 0 };
  const len = loop.end - loop.start;
  const past = t - loop.end;
  return { position: loop.start + (past % len), wraps: 1 + Math.floor(past / len) };
}

/** Equal-power fade curve (`n` points): sin from 0 to 1, or cos from 1 to 0 when `out`. */
export function equalPowerCurve(n: number, out: boolean): Float32Array {
  const c = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    const t = (i / (n - 1)) * (Math.PI / 2);
    c[i] = out ? Math.cos(t) : Math.sin(t);
  }
  return c;
}

export interface MusicInputs {
  /** Seconds, any monotonic clock. */
  now: number;
  paused: boolean;
  editor: boolean;
  /** Any marble rolling this frame. */
  moving: boolean;
}

/** Chooses the music state from the HUD's view of the game, with a dwell so it does not flap. */
export class MusicDirector {
  readonly #dwell: number;
  readonly #idle: number;
  #state: MusicState | null = null;
  #candidate: MusicState | null = null;
  #since = 0;
  #lastMove: number | null = null;
  #forced: MusicState | null = null;

  constructor(opts: { dwellS?: number; idleS?: number } = {}) {
    this.#dwell = opts.dwellS ?? DWELL_S;
    this.#idle = opts.idleS ?? IDLE_S;
  }

  /** Pin the state (?music=, the title screen, tests); null hands it back to the inputs. */
  force(state: MusicState | null): void {
    this.#forced = state;
    if (state) this.#state = state;
  }

  get forced(): MusicState | null {
    return this.#forced;
  }

  /** The state the inputs ask for now, before the dwell. */
  wanted(i: MusicInputs): MusicState {
    if (this.#lastMove === null || i.moving) this.#lastMove = i.now;
    if (i.paused) return "night";
    if (i.moving) return "run";
    if (i.editor) return "build";
    return i.now - this.#lastMove >= this.#idle ? "night" : "run";
  }

  update(i: MusicInputs): MusicState {
    const want = this.wanted(i);
    if (this.#forced) return this.#forced;
    if (this.#state === null) {
      this.#state = this.#candidate = want;
      this.#since = i.now;
      return want;
    }
    if (want !== this.#candidate) {
      this.#candidate = want;
      this.#since = i.now;
    }
    if (want !== this.#state && i.now - this.#since >= this.#dwell) this.#state = want;
    return this.#state;
  }
}

export interface MusicStats {
  state: MusicState | "off";
  cue: string | null;
  /** Source id of the cue sounding at full level; a new one per start, crossfade or seek. */
  source: number;
  loop: boolean;
  /** File position of the current source, s. */
  position: number;
  /** Times the current source has wrapped from loopEnd to loopStart. */
  wraps: number;
  loopStart: number;
  loopEnd: number;
  /** The current source's decoded buffer length, s. */
  bufferDuration: number;
  crossfades: number;
  /** Sources started (first start, crossfades, seeks). */
  starts: number;
  /** Decoded buffers held now, never above MAX_BUFFERS. */
  buffers: number;
  loads: number;
  loadErrors: number;
  /** A cue was wanted with no decoded buffer yet (counted per update). */
  waiting: number;
}

/** Fetch and decode one cue. */
export type MusicLoader = (cue: string) => Promise<AudioBuffer>;

interface Voice {
  id: number;
  cue: string;
  src: AudioBufferSourceNode;
  gain: GainNode;
  started: number;
  offset: number;
  loop: { start: number; end: number };
}

export class MusicPlayer {
  readonly #ctx: BaseAudioContext;
  readonly #out: AudioNode;
  readonly #catalog: MusicCatalog;
  readonly #load: MusicLoader;
  readonly #buffers = new Map<string, AudioBuffer>();
  readonly #loading = new Set<string>();
  #voice: Voice | null = null;
  #fading: { voice: Voice; until: number }[] = [];
  #state: MusicState | "off" = "off";
  #cue: string | null = null;
  #index = 0;
  #nextId = 1;
  readonly #stats: Pick<MusicStats, "crossfades" | "starts" | "loads" | "loadErrors" | "waiting"> = {
    crossfades: 0,
    starts: 0,
    loads: 0,
    loadErrors: 0,
    waiting: 0,
  };

  constructor(ctx: BaseAudioContext, out: AudioNode, catalog: MusicCatalog, load: MusicLoader) {
    this.#ctx = ctx;
    this.#out = out;
    this.#catalog = catalog;
    this.#load = load;
  }

  /** Follow `state` ("off" fades out). A new state starts its playlist from the top. */
  setState(state: MusicState | "off"): void {
    if (state === this.#state) return;
    this.#state = state;
    this.#index = 0;
    this.#cue = state === "off" ? null : (this.#playlist()[0] ?? null);
    this.update();
  }

  /** Call once per frame: starts the wanted cue once it is decoded, moves playlists on, drops old buffers. */
  update(): void {
    const now = this.#ctx.currentTime;
    this.#fading = this.#fading.filter((f) => {
      if (now < f.until) return true;
      f.voice.src.disconnect();
      f.voice.gain.disconnect();
      return false;
    });
    const v = this.#voice;
    const list = this.#playlist();
    if (v && v.cue === this.#cue && list.length > 1) {
      const { wraps } = loopPosition(now - v.started, v.offset, v.loop);
      if (wraps >= LOOPS_PER_CUE) {
        this.#index = (this.#index + 1) % list.length;
        this.#cue = list[this.#index] ?? null;
      }
    }
    if (this.#cue === null) {
      if (v) this.#fadeOut(v, now, STOP_FADE_S);
      this.#voice = null;
    } else if (v?.cue !== this.#cue) {
      const buffer = this.#buffers.get(this.#cue);
      if (buffer) this.#start(this.#cue, buffer, 0, now, v !== null);
      else {
        this.#stats.waiting++;
        this.#request(this.#cue);
      }
    }
    this.#evict();
  }

  /** Test hook: restart the current cue at file position `seconds` (a new source, no crossfade). */
  seek(seconds: number): boolean {
    const v = this.#voice;
    const buffer = v ? this.#buffers.get(v.cue) : undefined;
    if (!v || !buffer) return false;
    const now = this.#ctx.currentTime;
    this.#fadeOut(v, now, 0.01);
    this.#voice = null;
    this.#start(v.cue, buffer, Math.max(0, Math.min(seconds, buffer.duration - 0.01)), now, false);
    return true;
  }

  stats(): MusicStats {
    const v = this.#voice;
    const now = this.#ctx.currentTime;
    const at = v ? loopPosition(now - v.started, v.offset, v.loop) : { position: 0, wraps: 0 };
    return {
      ...this.#stats,
      state: this.#state,
      cue: v?.cue ?? null,
      source: v?.id ?? 0,
      loop: v?.src.loop ?? false,
      position: at.position,
      wraps: at.wraps,
      loopStart: v?.loop.start ?? 0,
      loopEnd: v?.loop.end ?? 0,
      bufferDuration: v?.src.buffer?.duration ?? 0,
      buffers: this.#buffers.size,
    };
  }

  #playlist(): readonly string[] {
    return this.#state === "off" ? [] : PLAYLISTS[this.#state].filter((c) => this.#catalog.cues[c]);
  }

  #request(cue: string): void {
    if (this.#loading.has(cue) || this.#buffers.has(cue)) return;
    this.#loading.add(cue);
    this.#load(cue).then(
      (buffer) => {
        this.#loading.delete(cue);
        this.#stats.loads++;
        this.#buffers.set(cue, buffer);
        this.update();
      },
      (err: unknown) => {
        this.#loading.delete(cue);
        this.#stats.loadErrors++;
        console.warn(`parble music: could not load ${cue}`, err);
      },
    );
  }

  #start(cue: string, buffer: AudioBuffer, offset: number, now: number, crossfade: boolean): void {
    const c = this.#catalog.cues[cue];
    if (!c) return;
    const loop = loopSeconds(c);
    const src = this.#ctx.createBufferSource();
    src.buffer = buffer;
    src.loop = true;
    src.loopStart = loop.start;
    src.loopEnd = loop.end;
    const gain = this.#ctx.createGain();
    src.connect(gain);
    gain.connect(this.#out);
    const old = this.#voice;
    if (crossfade && old) {
      gain.gain.value = 0;
      gain.gain.setValueCurveAtTime(equalPowerCurve(64, false), now, XFADE_S);
      this.#fadeOut(old, now, XFADE_S);
      this.#stats.crossfades++;
    } else {
      if (old) this.#fadeOut(old, now, STOP_FADE_S);
      gain.gain.value = 1;
    }
    src.start(now, offset);
    this.#voice = { id: this.#nextId++, cue, src, gain, started: now, offset, loop };
    this.#stats.starts++;
  }

  #fadeOut(v: Voice, now: number, seconds: number): void {
    const g = v.gain.gain;
    // From the level it holds now (it may be mid fade-in), so the fade never jumps.
    const from = g.value;
    if (g.cancelAndHoldAtTime) g.cancelAndHoldAtTime(now);
    else g.cancelScheduledValues(now);
    g.setValueCurveAtTime(
      equalPowerCurve(64, true).map((x) => x * from),
      now,
      seconds,
    );
    v.src.stop(now + seconds);
    this.#fading.push({ voice: v, until: now + seconds });
  }

  /** Keep the buffers of sounding cues and the wanted one, newest first, at most MAX_BUFFERS. */
  #evict(): void {
    if (this.#buffers.size <= MAX_BUFFERS) return;
    const keep = new Set<string>();
    if (this.#cue) keep.add(this.#cue);
    if (this.#voice) keep.add(this.#voice.cue);
    for (const f of [...this.#fading].reverse()) if (keep.size < MAX_BUFFERS) keep.add(f.voice.cue);
    for (const cue of [...this.#buffers.keys()]) {
      if (this.#buffers.size <= MAX_BUFFERS) break;
      if (!keep.has(cue)) this.#buffers.delete(cue);
    }
  }
}
