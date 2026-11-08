// Browser glue for the audio engine: fetch the staged SFX (web/ui/audio, versioned from
// window.__PARBLE_HASHES), create and resume the AudioContext on the first user gesture, decode, and
// expose window.__parbleAudio for drivers (docs/agent-operability.md).

import { type AudioFrame, decodeEvent } from "../bridge";
import { type Catalog, decodeCatalog } from "./catalog";
import { AudioEngine, type AudioStats } from "./engine";

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
}

export interface AudioDiag {
  stats(): AudioDiagStats;
  /** Test hook: decode `raw` as a bridge event and feed it if it is an audioFrame. From then on live bridge frames are ignored. */
  inject(raw: unknown): boolean;
}

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

export interface AudioHandle {
  consume(frame: AudioFrame): void;
}

export function startAudio(win: Window): AudioHandle {
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
  };
  let catalog: Catalog | null = null;
  let ctx: AudioContext | null = null;
  let engine: AudioEngine | null = null;
  const raw = new Map<string, ArrayBuffer>();

  const loadFail = (what: string, err: unknown): void => {
    diag.loadErrors++;
    console.warn(`parble audio: could not load ${what}`, err);
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
    if (ctx && !engine) engine = new AudioEngine(ctx, { catalog });
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
      if (catalog) engine = new AudioEngine(c, { catalog });
      for (const [stem, bytes] of [...raw]) decode(stem, bytes);
    }
    if (c.state !== "running")
      c.resume().then(syncState, (err: unknown) => console.warn("parble audio: resume refused", err));
    else for (const g of GESTURES) win.removeEventListener(g, unlock, true);
  };

  if (Ctx) {
    for (const g of GESTURES) win.addEventListener(g, unlock, true);
    load().catch((err: unknown) => loadFail("the SFX catalog", err));
  }

  const feed = (frame: AudioFrame): void => {
    if (engine && ctx?.state === "running") engine.consume(frame);
    else diag.skippedFrames++;
  };

  win.__parbleAudio = {
    stats: () => ({ ...diag, ...(engine ? engine.stats() : {}) }),
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
