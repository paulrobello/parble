// The Web Audio engine (P4-T4): consumes one bridge AudioFrame per HUD frame and drives
// - a roll loop voice per marble, its pitch, gain and low-pass mapped from speed, crossfading between
//   surface loops when the marble changes surface;
// - impact one-shots per coalesced contact, the cue picked by the band of the velocity change the
//   impulse gives the marble's real mass, the variant at random (seeded) with pitch jitter on top of
//   the marble size's pitch offset (rolls get the same offset);
// - mechanism one-shots per signal, the cue picked by piece and signal name;
// - PannerNode spatialisation against an AudioListener placed at the camera;
// - a voice budget where the loudest heard voices (gain × distance gain) win.
// Buses: sfx and music into master, master through a compressor into the destination.

import type { AudioContact, AudioFrame, AudioRoll, Vec3 } from "../bridge";
import { type Catalog, VariantPicker } from "./catalog";
import {
  CELL_M,
  distance,
  distanceGain,
  impactFor,
  jitterRate,
  materialOf,
  PANNER,
  rollCueId,
  rollParams,
  signalCueId,
} from "./mapping";
import { admit, DEFAULT_CAPS, oneShotPriority, type Ranked, seededRng, selectTop, type VoiceCaps } from "./voices";

/** A held roll older than this many sim ticks is dropped (the stream samples rolls every 4 ticks). */
export const STALE_TICKS = 60;
/** Surface crossfade time constant and stop delay, s. */
const XFADE_TC = 0.04;
const XFADE_S = 0.25;
/** Roll voice release time, s. */
const RELEASE_S = 0.2;
/** Parameter smoothing time constant, s. */
const SMOOTH_TC = 0.03;
/** Impact pitch jitter, ± semitones. */
export const JITTER_SEMITONES = 0.6;
const ROLL_LEVEL = 0.5;
const IMPACT_LEVEL = 0.8;
export const BUS_DEFAULTS = { master: 0.9, sfx: 1, music: 0.6 } as const;
export type Bus = keyof typeof BUS_DEFAULTS;

export interface AudioStats {
  /** Voices sounding now (roll loops plus one-shots), never above `cap.total`. */
  activeVoices: number;
  rollVoices: number;
  oneShotVoices: number;
  /** Most voices at once since start. */
  peakVoices: number;
  cap: VoiceCaps;
  /** One-shots started. */
  started: number;
  /** One-shots refused because every slot held a louder voice. */
  dropped: number;
  /** Voices whose slot went to a louder one-shot. */
  stolen: number;
  /** Rolls left silent because louder rolls filled the roll budget (counted per frame). */
  rollsCulled: number;
  crossfades: number;
  /** Tick regressions (run restarts, loaded tracks) that cleared every roll. */
  resets: number;
  frames: number;
  contacts: number;
  /** Cues requested with no decoded buffer yet. */
  missingBuffers: number;
  buffers: number;
  errors: number;
  lastError: string | null;
}

interface Layer {
  cue: string;
  src: AudioBufferSourceNode;
  gain: GainNode;
}

interface RollVoice {
  marble: number;
  filter: BiquadFilterNode;
  gain: GainNode;
  panner: PannerNode;
  layer: Layer | null;
}

interface OneShot extends Ranked {
  src: AudioBufferSourceNode;
  gain: GainNode;
  panner: PannerNode;
  base: number;
  start: number;
  duration: number;
}

interface Held {
  roll: AudioRoll;
  tick: number;
}

/** Nodes stopped and fading out, disconnected once `until` has passed. */
interface Tail {
  until: number;
  nodes: AudioNode[];
}

export interface EngineOptions {
  /** Route the output through a zero gain: the engine runs but is silent. */
  muted?: boolean;
  catalog: Catalog;
  seed?: number;
  caps?: VoiceCaps;
}

function setPosition(p: PannerNode, at: Vec3): void {
  if (p.positionX) {
    p.positionX.value = at[0];
    p.positionY.value = at[1];
    p.positionZ.value = at[2];
  } else p.setPosition(at[0], at[1], at[2]);
}

export class AudioEngine {
  readonly #ctx: BaseAudioContext;
  readonly #catalog: Catalog;
  readonly #caps: VoiceCaps;
  readonly #rng: () => number;
  readonly #picker: VariantPicker;
  readonly #buffers = new Map<string, AudioBuffer>();
  readonly #bus: Record<Bus, GainNode>;
  readonly #rolls = new Map<number, RollVoice>();
  readonly #held = new Map<number, Held>();
  readonly #oneShots = new Map<number, OneShot>();
  #tails: Tail[] = [];
  #listener: Vec3 = [0, 0, 0];
  #lastTick = -1;
  #nextId = 1;
  readonly #stats: AudioStats;

  constructor(ctx: BaseAudioContext, opts: EngineOptions) {
    this.#ctx = ctx;
    this.#catalog = opts.catalog;
    this.#caps = { ...(opts.caps ?? DEFAULT_CAPS) };
    this.#rng = seededRng(opts.seed ?? 0x5eed_ab1e);
    this.#picker = new VariantPicker(this.#rng);
    const master = ctx.createGain();
    master.gain.value = BUS_DEFAULTS.master;
    const limiter = ctx.createDynamicsCompressor();
    master.connect(limiter);
    if (opts.muted) {
      // `mute=1` (tests, headless and agent runs): the engine runs and counts voices, but nothing is heard.
      const silent = ctx.createGain();
      silent.gain.value = 0;
      limiter.connect(silent);
      silent.connect(ctx.destination);
    } else {
      limiter.connect(ctx.destination);
    }
    const sfx = ctx.createGain();
    sfx.gain.value = BUS_DEFAULTS.sfx;
    sfx.connect(master);
    const music = ctx.createGain();
    music.gain.value = BUS_DEFAULTS.music;
    music.connect(master);
    this.#bus = { master, sfx, music };
    this.#stats = {
      activeVoices: 0,
      rollVoices: 0,
      oneShotVoices: 0,
      peakVoices: 0,
      cap: { ...this.#caps },
      started: 0,
      dropped: 0,
      stolen: 0,
      rollsCulled: 0,
      crossfades: 0,
      resets: 0,
      frames: 0,
      contacts: 0,
      missingBuffers: 0,
      buffers: 0,
      errors: 0,
      lastError: null,
    };
  }

  /** The bus a later music player (P4-T5) or mixer UI connects to. */
  bus(name: Bus): GainNode {
    return this.#bus[name];
  }

  setBusGain(name: Bus, gain: number): void {
    this.#bus[name].gain.value = Math.max(0, gain);
  }

  addBuffer(stem: string, buffer: AudioBuffer): void {
    this.#buffers.set(stem, buffer);
    this.#stats.buffers = this.#buffers.size;
  }

  noteError(message: string): void {
    this.#stats.errors++;
    this.#stats.lastError = message.slice(0, 300);
  }

  stats(): AudioStats {
    return { ...this.#stats, cap: { ...this.#stats.cap } };
  }

  /** Apply one frame of the audio stream. Never throws: a failure is counted in `errors`. */
  consume(frame: AudioFrame): void {
    try {
      this.#consume(frame);
    } catch (err) {
      this.noteError(err instanceof Error ? `${err.name}: ${err.message}` : String(err));
    }
  }

  #consume(frame: AudioFrame): void {
    const now = this.#ctx.currentTime;
    const s = this.#stats;
    s.frames++;
    if (frame.tick < this.#lastTick) {
      for (const m of [...this.#rolls.keys()]) this.#releaseRoll(m, now);
      this.#held.clear();
      s.resets++;
    }
    this.#lastTick = frame.tick;
    if (frame.camera) this.#setListener(frame.camera.position, frame.camera.forward, frame.camera.up);
    for (const r of frame.rolls) this.#held.set(r.marble, { roll: r, tick: frame.tick });
    for (const [m, h] of this.#held) if (frame.tick - h.tick > STALE_TICKS) this.#held.delete(m);
    this.#prune(now);
    this.#updateRolls(now);
    s.contacts += frame.contacts.length;
    this.#impacts(frame.contacts, now);
    for (const e of frame.mechanisms) {
      if (e.kind !== "signal") continue;
      const cue = signalCueId(e.piece, e.name, materialOf(this.#held.get(e.value)?.roll.kind ?? "cats-eye"));
      const at: Vec3 = [e.instance[0] * CELL_M, (e.instance[1] + 0.5) * CELL_M, e.instance[2] * CELL_M];
      if (cue) this.#playOneShot(cue, 0.7, 1, at, now);
    }
    s.rollVoices = this.#rolls.size;
    s.oneShotVoices = this.#oneShots.size;
    s.activeVoices = s.rollVoices + s.oneShotVoices;
    s.peakVoices = Math.max(s.peakVoices, s.activeVoices);
  }

  #setListener(pos: Vec3, fwd: Vec3, up: Vec3): void {
    this.#listener = [...pos];
    const l = this.#ctx.listener;
    if (l.positionX) {
      l.positionX.value = pos[0];
      l.positionY.value = pos[1];
      l.positionZ.value = pos[2];
      l.forwardX.value = fwd[0];
      l.forwardY.value = fwd[1];
      l.forwardZ.value = fwd[2];
      l.upX.value = up[0];
      l.upY.value = up[1];
      l.upZ.value = up[2];
    } else {
      l.setPosition(pos[0], pos[1], pos[2]);
      l.setOrientation(fwd[0], fwd[1], fwd[2], up[0], up[1], up[2]);
    }
  }

  #updateRolls(now: number): void {
    const cands: (Ranked & { held: Held; cue: string; gain: number })[] = [];
    for (const [marble, held] of this.#held) {
      const r = held.roll;
      const cue = rollCueId(r.kind, r.surface) ?? this.#rolls.get(marble)?.layer?.cue ?? null;
      if (cue === null || !this.#catalog.cues[cue]) continue;
      const p = rollParams(r.speed, r.size);
      if (p.gain <= 0) continue;
      const gain = r.contact ? p.gain : 0;
      cands.push({
        id: marble,
        priority: p.gain * distanceGain(distance(this.#listener, r.position)),
        held,
        cue,
        gain,
      });
    }
    const budget = Math.min(this.#caps.rolls, this.#caps.total);
    const keep = selectTop(cands, budget);
    this.#stats.rollsCulled += cands.length - keep.length;
    const kept = new Set(keep.map((c) => c.id));
    for (const m of [...this.#rolls.keys()]) if (!kept.has(m)) this.#releaseRoll(m, now);
    for (const c of keep) {
      let v = this.#rolls.get(c.id);
      if (!v) {
        v = this.#newRollVoice(c.id);
        this.#rolls.set(c.id, v);
      }
      this.#setSurface(v, c.cue, now);
      const p = rollParams(c.held.roll.speed, c.held.roll.size);
      v.gain.gain.setTargetAtTime(c.gain * ROLL_LEVEL, now, SMOOTH_TC);
      v.filter.frequency.setTargetAtTime(p.cutoffHz, now, SMOOTH_TC);
      v.layer?.src.playbackRate.setTargetAtTime(p.rate, now, SMOOTH_TC);
      setPosition(v.panner, c.held.roll.position);
    }
  }

  #panner(): PannerNode {
    const p = this.#ctx.createPanner();
    p.panningModel = "equalpower";
    p.distanceModel = "inverse";
    p.refDistance = PANNER.refDistance;
    p.rolloffFactor = PANNER.rolloffFactor;
    p.maxDistance = PANNER.maxDistance;
    return p;
  }

  #newRollVoice(marble: number): RollVoice {
    const filter = this.#ctx.createBiquadFilter();
    filter.type = "lowpass";
    const gain = this.#ctx.createGain();
    gain.gain.value = 0;
    const panner = this.#panner();
    filter.connect(gain);
    gain.connect(panner);
    panner.connect(this.#bus.sfx);
    return { marble, filter, gain, panner, layer: null };
  }

  /** Start `cue`'s loop on `v`, crossfading out the loop it replaces. */
  #setSurface(v: RollVoice, cue: string, now: number): void {
    if (v.layer?.cue === cue) return;
    const buffer = this.#pickBuffer(cue);
    if (!buffer) return;
    const src = this.#ctx.createBufferSource();
    src.buffer = buffer;
    src.loop = true;
    const gain = this.#ctx.createGain();
    gain.gain.value = 0;
    gain.gain.setTargetAtTime(1, now, XFADE_TC);
    src.connect(gain);
    gain.connect(v.filter);
    src.start(now, this.#rng() * buffer.duration);
    const old = v.layer;
    if (old) {
      old.gain.gain.setTargetAtTime(0, now, XFADE_TC);
      old.src.stop(now + XFADE_S);
      this.#tails.push({ until: now + XFADE_S, nodes: [old.src, old.gain] });
      this.#stats.crossfades++;
    }
    v.layer = { cue, src, gain };
  }

  #releaseRoll(marble: number, now: number): void {
    const v = this.#rolls.get(marble);
    if (!v) return;
    this.#rolls.delete(marble);
    v.gain.gain.setTargetAtTime(0, now, RELEASE_S / 4);
    const nodes: AudioNode[] = [v.filter, v.gain, v.panner];
    if (v.layer) {
      v.layer.src.stop(now + RELEASE_S);
      nodes.push(v.layer.src, v.layer.gain);
    }
    this.#tails.push({ until: now + RELEASE_S, nodes });
  }

  #pickBuffer(cue: string): AudioBuffer | null {
    const c = this.#catalog.cues[cue];
    if (!c) return null;
    const loaded = c.variants.filter((v) => this.#buffers.has(v.stem));
    const v = this.#picker.pick(cue, loaded);
    if (!v) {
      this.#stats.missingBuffers++;
      return null;
    }
    return this.#buffers.get(v.stem) ?? null;
  }

  #impacts(contacts: readonly AudioContact[], now: number): void {
    const hits = [];
    for (const c of contacts) {
      const choice = impactFor(c);
      if (choice && this.#catalog.cues[choice.cue]) {
        hits.push({ c, choice, priority: choice.gain * distanceGain(distance(this.#listener, c.position)) });
      }
    }
    hits.sort((a, b) => b.priority - a.priority);
    for (const h of hits) this.#playOneShot(h.choice.cue, h.choice.gain, h.priority, h.c.position, now, h.choice.rate);
  }

  /** Start `cue` at `at`; `rate` is a pitch offset (a marble's size) under the random jitter. */
  #playOneShot(cue: string, gainValue: number, priority: number, at: Vec3, now: number, rate = 1): void {
    const active: Ranked[] = [];
    for (const o of this.#oneShots.values()) {
      active.push({ id: o.id, priority: oneShotPriority(o.base, o.start, o.duration, now) });
    }
    const cap = this.#caps.total - this.#rolls.size;
    const a = admit(active, cap, priority);
    if (!a.admit) {
      this.#stats.dropped++;
      return;
    }
    const buffer = this.#pickBuffer(cue);
    if (!buffer) return;
    if (a.evict !== null) {
      this.#stopOneShot(a.evict, now);
      this.#stats.stolen++;
    }
    const src = this.#ctx.createBufferSource();
    src.buffer = buffer;
    const playbackRate = rate * jitterRate(this.#rng(), JITTER_SEMITONES);
    src.playbackRate.value = playbackRate;
    const gain = this.#ctx.createGain();
    gain.gain.value = gainValue * IMPACT_LEVEL;
    const panner = this.#panner();
    setPosition(panner, at);
    src.connect(gain);
    gain.connect(panner);
    panner.connect(this.#bus.sfx);
    const id = this.#nextId++;
    src.onended = () => this.#finishOneShot(id);
    src.start(now);
    this.#oneShots.set(id, {
      id,
      priority,
      src,
      gain,
      panner,
      base: priority,
      start: now,
      duration: buffer.duration / playbackRate,
    });
    this.#stats.started++;
  }

  #stopOneShot(id: number, now: number): void {
    const o = this.#oneShots.get(id);
    if (!o) return;
    this.#oneShots.delete(id);
    o.gain.gain.setTargetAtTime(0, now, 0.01);
    o.src.stop(now + 0.05);
    this.#tails.push({ until: now + 0.05, nodes: [o.src, o.gain, o.panner] });
  }

  #finishOneShot(id: number): void {
    const o = this.#oneShots.get(id);
    if (!o) return;
    this.#oneShots.delete(id);
    for (const n of [o.src, o.gain, o.panner]) n.disconnect();
  }

  /** Drop finished one-shots (onended can lag a frame) and disconnect expired tails. */
  #prune(now: number): void {
    for (const o of [...this.#oneShots.values()]) if (now >= o.start + o.duration) this.#finishOneShot(o.id);
    const live: Tail[] = [];
    for (const t of this.#tails) {
      if (now >= t.until) for (const n of t.nodes) n.disconnect();
      else live.push(t);
    }
    this.#tails = live;
  }
}
