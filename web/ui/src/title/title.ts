// The title screen (P5-T3): the approved key art, the wordmark, and an inlay bar with Build, Watch,
// the hopper marble picker and Settings. It is a front door: Build and Watch open the scene by URL
// (store.launchQuery), so the engine reads the chosen tier, camera and marble once at startup, exactly
// as it reads the agent switches. First run detects a quality tier (gpu.ts) and stores it.

import { fontFaces, versioned } from "../editor/style";
import { BUSES, type MixerSettings, loadMixer, saveMixer } from "../audio/mixer";
import { type Detection, adapterName, detectQuality } from "./gpu";
import {
  CAMERAS,
  type Camera,
  type Launch,
  MARBLE_SIZES,
  MARBLE_TYPES,
  type MarblePick,
  type MarbleSize,
  type MarbleType,
  type Settings,
  TIERS,
  type Tier,
  effectiveTier,
  launchQuery,
  loadSettings,
  saveSettings,
} from "./store";
import { TITLE_CSS } from "./style";

export const MARBLE_THUMB_DIR = "web/ui/thumbs/marbles/";

export const MARBLE_INFO: Record<MarbleType, { name: string; body: string; grams: [number, number, number] }> = {
  "cats-eye": { name: "Cat's eye", body: "Clear glass, coloured vanes", grams: [1.32, 5.4, 20.62] },
  swirl: { name: "Swirl", body: "Glass with fused ribbons", grams: [1.32, 5.4, 20.62] },
  onionskin: { name: "Onionskin", body: "Glass in a coloured skin", grams: [1.32, 5.4, 20.62] },
  opal: { name: "Opal", body: "Milky opal glass", grams: [1.32, 5.4, 20.62] },
  glow: { name: "Glow", body: "Glass, glowing core", grams: [1.32, 5.4, 20.62] },
  glazed: { name: "Glazed", body: "Glazed ceramic", grams: [1.26, 5.15, 19.63] },
  steel: { name: "Steel", body: "Chrome steel, heavy", grams: [4.09, 16.75, 63.9] },
};

export const SIZE_INFO: Record<MarbleSize, { name: string; mm: number }> = {
  mini: { name: "Mini", mm: 10 },
  standard: { name: "Standard", mm: 16 },
  shooter: { name: "Shooter", mm: 25 },
};

const TIER_NAME: Record<Tier, string> = { low: "Low", medium: "Medium", high: "High", ultra: "Ultra" };
const CAMERA_INFO: Record<Camera, { name: string; note: string }> = {
  director: { name: "Director", note: "Films the run, cutting between shots as the marbles travel." },
  orbit: { name: "Orbit", note: "You steer: drag to orbit, scroll to zoom." },
  overview: { name: "Overview", note: "One fixed shot over the whole track." },
};
const MIX_NAME: Record<(typeof BUSES)[number], string> = { master: "Master", music: "Music", sfx: "Effects" };

/** The legacy marble the editor runs with no pick looks like a standard cat's eye. */
const SHOWN_DEFAULT: MarblePick = { type: "cats-eye", size: "standard" };

const ICONS = {
  build: '<path d="M10 2.5l7 4v7l-7 4-7-4v-7z M10 7v6 M7 10h6"/>',
  watch: '<path d="M6 4l10 6-10 6z"/>',
  set: '<path d="M3 6h9 M15 6h2 M3 14h2 M8 14h9"/><circle cx="13.5" cy="6" r="1.8"/><circle cx="6.5" cy="14" r="1.8"/>',
};
const ic = (k: keyof typeof ICONS): string => `<svg class="i" viewBox="0 0 20 20" aria-hidden="true">${ICONS[k]}</svg>`;

/** A size glyph: a circle scaled to the marble's diameter against the shooter. */
const sizeGlyph = (mm: number): string =>
  `<svg viewBox="0 0 26 26" aria-hidden="true"><circle cx="13" cy="13" r="${((mm / 25) * 11.5).toFixed(2)}"/></svg>`;

export interface TitleDeps {
  doc: Document;
  win: Window;
  storage: Storage | null;
  /** Quality detection; defaults to the real WebGPU check. */
  detect?: () => Promise<Detection>;
  /** Navigate to a scene query; defaults to location.assign. */
  go?: (query: string) => void;
  /** Live mixer changes (the audio engine, once it exists). */
  onMixer?: (s: MixerSettings) => void;
}

export interface TitleState {
  shown: boolean;
  detecting: boolean;
  detection: Detection | null;
  settings: Settings;
  open: "settings" | "marbles" | null;
}

export interface TitleHandle {
  state(): TitleState;
  /** The query Build or Watch opens now. */
  launchQuery(kind: Launch): string;
  /** Resolves once first-run detection has finished (at once when a stored result exists). */
  ready: Promise<void>;
  /** Wire the live mixer once the audio engine exists. */
  setMixerSink(fn: (s: MixerSettings) => void): void;
}

const svgEscape = (s: string): string =>
  s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c] as string);

function artSources(win: Window): string {
  const v = (p: string): string => versioned(win, `assets/keyart/${p}`);
  const set = (stem: string, small: [string, number], big: [string, number], ext: string): string =>
    `${v(`${stem}-${small[0]}.${ext}`)} ${small[1]}w, ${v(`${stem}-${big[0]}.${ext}`)} ${big[1]}w`;
  const group = (media: string, stem: string, small: [string, number], big: [string, number]): string =>
    ["avif", "webp"]
      .map(
        (ext) => `<source media="${media}" type="image/${ext}" sizes="100vw" srcset="${set(stem, small, big, ext)}">`,
      )
      .join("");
  return (
    group("(min-aspect-ratio: 11/5)", "hero-21x9", ["1280x543", 1280], ["2560x1086", 2560]) +
    group("(max-aspect-ratio: 13/10)", "hero-portrait-9x16", ["720x1280", 720], ["1080x1920", 1080]) +
    group("all", "hero-16x9", ["1280x720", 1280], ["2560x1440", 2560]) +
    `<img src="${v("hero-16x9-1280x720.png")}" alt="" decoding="async">`
  );
}

export function mountTitle(deps: TitleDeps): TitleHandle {
  const { doc, win, storage } = deps;
  const go = deps.go ?? ((q: string) => win.location.assign(`${win.location.pathname}${q}`));
  let mixerSink = deps.onMixer ?? null;
  let settings = loadSettings(storage);
  let mixer = loadMixer(storage);
  const st: TitleState = { shown: true, detecting: false, detection: null, settings, open: null };

  const style = doc.createElement("style");
  style.id = "pt-style";
  style.textContent = `${fontFaces(win)}\n${TITLE_CSS}`;
  doc.head.append(style);

  const root = doc.createElement("main");
  root.id = "pt";
  root.setAttribute("aria-label", "parble title");
  const shownPick = (): MarblePick => settings.marble ?? SHOWN_DEFAULT;
  root.innerHTML = `
<picture class="art">${artSources(win)}</picture>
<div class="shade"></div>
<header class="mark">
  <h1>parble</h1>
  <div class="rule"><i class="pin"></i></div>
  <p class="tag">Build marble runs from wooden blocks and rails, then watch the marbles race.</p>
</header>
<nav class="bar inlay" aria-label="Start">
  <button type="button" class="go" data-a="build">${ic("build")}<span>Build a track</span><kbd>B</kbd></button>
  <button type="button" class="q" data-a="watch">${ic("watch")}<span>Watch the showcase</span><kbd>W</kbd></button>
  <span class="sep" aria-hidden="true"></span>
  <button type="button" class="q" data-a="marbles" aria-expanded="false" aria-controls="pt-marbles">
    <img class="swatch" alt="" src=""><span>Marbles</span><kbd>M</kbd></button>
  <button type="button" class="q" data-a="settings" aria-expanded="false" aria-controls="pt-settings">${ic("set")}<span>Settings</span><kbd>S</kbd></button>
  <span class="sep" aria-hidden="true"></span>
  <p class="gpu" aria-live="polite"><i class="pin"></i><span><span class="sc">Quality</span> <b></b></span><small></small></p>
</nav>
<section class="sheet inlay" id="pt-marbles" role="dialog" aria-modal="false" aria-labelledby="pt-marbles-h" hidden>
  <h2 id="pt-marbles-h"><span class="sc">Hopper ·</span>Marbles for your tracks</h2>
  <p class="lede">Every marble in the hopper's queue rolls as this one when you run a track you build. Set how many with the marble pins in the editor.</p>
  <section>
    <h3 id="pt-type-h">Type</h3>
    <div class="tiles" role="radiogroup" aria-labelledby="pt-type-h">
      ${MARBLE_TYPES.map(
        (t) => `<button type="button" class="tile" role="radio" aria-checked="false" data-type="${t}">
        <i class="pin"></i><img alt="" src="${svgEscape(versioned(win, `${MARBLE_THUMB_DIR}${t}.webp`))}" width="72" height="72" loading="lazy">
        <b>${MARBLE_INFO[t].name}</b><small>${MARBLE_INFO[t].body}</small></button>`,
      ).join("")}
    </div>
  </section>
  <section>
    <h3 id="pt-size-h">Size</h3>
    <div class="seg" role="radiogroup" aria-labelledby="pt-size-h">
      ${MARBLE_SIZES.map(
        (s) =>
          `<button type="button" role="radio" aria-checked="false" data-size="${s}">${sizeGlyph(SIZE_INFO[s].mm)}<span>${SIZE_INFO[s].name}</span><small>${SIZE_INFO[s].mm} mm</small></button>`,
      ).join("")}
    </div>
    <p class="detail"><b class="d-name"></b><span class="d-size"></span><span class="d-mass"></span></p>
  </section>
  <div class="foot"><button type="button" class="q" data-a="close">Done<kbd>Esc</kbd></button></div>
</section>
<section class="sheet inlay" id="pt-settings" role="dialog" aria-modal="false" aria-labelledby="pt-settings-h" hidden>
  <h2 id="pt-settings-h"><span class="sc">Settings ·</span>Picture, sound and camera</h2>
  <p class="lede">Saved in this browser. They apply when you open a scene from here.</p>
  <section>
    <h3 id="pt-q-h">Quality</h3>
    <div class="seg" role="radiogroup" aria-labelledby="pt-q-h">
      <button type="button" role="radio" aria-checked="false" data-tier="auto"><i class="pin"></i><span>Auto</span><small class="auto-t"></small></button>
      ${TIERS.map((t) => `<button type="button" role="radio" aria-checked="false" data-tier="${t}"><i class="pin"></i><span>${TIER_NAME[t]}</span></button>`).join("")}
    </div>
    <p class="note q-note"></p>
    <div class="foot" style="justify-content:flex-start;margin-top:10px"><button type="button" class="q" data-a="redetect">Check this GPU again</button></div>
  </section>
  <section>
    <h3>Sound</h3>
    <div class="mix">
      ${BUSES.map(
        (b) =>
          `<label for="pt-mix-${b}">${MIX_NAME[b]}</label><input type="range" id="pt-mix-${b}" data-bus="${b}" min="0" max="100" step="1"><output for="pt-mix-${b}"></output>`,
      ).join("")}
      <label class="chk"><input type="checkbox" id="pt-mix-mute"><span>Mute everything</span></label>
    </div>
  </section>
  <section>
    <h3 id="pt-cam-h">Showcase camera</h3>
    <div class="seg" role="radiogroup" aria-labelledby="pt-cam-h">
      ${CAMERAS.map((c) => `<button type="button" role="radio" aria-checked="false" data-camera="${c}"><i class="pin"></i><span>${CAMERA_INFO[c].name}</span></button>`).join("")}
    </div>
    <p class="note cam-note"></p>
  </section>
  <div class="foot"><button type="button" class="q" data-a="close">Done<kbd>Esc</kbd></button></div>
</section>`;
  doc.body.append(root);

  const $ = <T extends Element>(sel: string): T => root.querySelector(sel) as T;
  const $$ = <T extends Element>(sel: string): T[] => [...root.querySelectorAll(sel)] as T[];
  const art = $<HTMLImageElement>(".art img");
  const reveal = (): void => art.classList.add("in");
  if (art.complete && art.naturalWidth > 0) reveal();
  else art.addEventListener("load", reveal, { once: true });

  const save = (): void => {
    st.settings = settings;
    saveSettings(storage, settings);
    render();
  };

  const render = (): void => {
    const q = settings.quality;
    const tier = effectiveTier(q);
    const gpu = $<HTMLElement>(".gpu");
    gpu.classList.toggle("busy", st.detecting);
    $<HTMLElement>(".gpu b").textContent = st.detecting ? "Checking…" : tier ? TIER_NAME[tier] : "Medium";
    $<HTMLElement>(".gpu small").textContent = st.detecting
      ? "Timing a short test on your GPU"
      : q.override
        ? "Your choice"
        : q.detected
          ? `Detected · ${adapterName(q.adapter)}`
          : "Engine default";
    const auto = q.override === null;
    for (const b of $$<HTMLButtonElement>("[data-tier]")) {
      const t = b.dataset.tier;
      b.setAttribute("aria-checked", String(t === "auto" ? auto : t === q.override));
    }
    $<HTMLElement>(".auto-t").textContent = q.detected ? TIER_NAME[q.detected] : "";
    $<HTMLElement>(".q-note").innerHTML = q.detected
      ? `Detected <b>${TIER_NAME[q.detected]}</b> on <b>${svgEscape(adapterName(q.adapter))}</b>${
          q.benchMs !== null ? `, ${q.benchMs.toFixed(2)} ms per 1080p test pass` : ""
        }. Low and Medium use lighter glass shaders and fewer shadows.`
      : "No detection yet. Auto uses the engine default, Medium.";
    $<HTMLButtonElement>('[data-a="redetect"]').disabled = st.detecting;
    for (const b of $$<HTMLButtonElement>("[data-camera]"))
      b.setAttribute("aria-checked", String(b.dataset.camera === settings.camera));
    $<HTMLElement>(".cam-note").textContent = CAMERA_INFO[settings.camera].note;
    const pick = shownPick();
    for (const b of $$<HTMLButtonElement>("[data-type]"))
      b.setAttribute("aria-checked", String(b.dataset.type === pick.type));
    for (const b of $$<HTMLButtonElement>("[data-size]"))
      b.setAttribute("aria-checked", String(b.dataset.size === pick.size));
    const info = MARBLE_INFO[pick.type];
    const si = MARBLE_SIZES.indexOf(pick.size);
    $<HTMLElement>(".d-name").textContent = `${info.name}, ${SIZE_INFO[pick.size].name.toLowerCase()}`;
    $<HTMLElement>(".d-size").textContent = `${SIZE_INFO[pick.size].mm} mm`;
    $<HTMLElement>(".d-mass").textContent = `${(info.grams[si] as number).toFixed(1)} g`;
    const sw = $<HTMLImageElement>(".swatch");
    const swSrc = versioned(win, `${MARBLE_THUMB_DIR}${pick.type}.webp`);
    if (sw.getAttribute("src") !== swSrc) sw.setAttribute("src", swSrc);
    for (const b of $$<HTMLInputElement>("[data-bus]")) {
      const bus = b.dataset.bus as (typeof BUSES)[number];
      const v = Math.round(mixer[bus] * 100);
      b.value = String(v);
      b.style.setProperty("--v", `${v}%`);
      (b.nextElementSibling as HTMLOutputElement).textContent = String(v);
    }
    $<HTMLInputElement>("#pt-mix-mute").checked = mixer.muted;
    root.classList.toggle("sheet-open", st.open !== null);
    for (const k of ["settings", "marbles"] as const) {
      const sheet = $<HTMLElement>(`#pt-${k}`);
      sheet.hidden = st.open !== k;
      $<HTMLElement>(`[data-a="${k}"]`).setAttribute("aria-expanded", String(st.open === k));
    }
  };

  let opener: HTMLElement | null = null;
  const openSheet = (k: "settings" | "marbles" | null): void => {
    if (st.open === k) k = null;
    const was = st.open;
    st.open = k;
    render();
    if (k) {
      opener = $<HTMLElement>(`[data-a="${k}"]`);
      const first = $<HTMLElement>(`#pt-${k} [aria-checked="true"], #pt-${k} button`);
      first?.focus();
    } else if (was) opener?.focus();
  };

  const runDetect = (): Promise<void> => {
    st.detecting = true;
    render();
    const detect = deps.detect ?? (() => detectQuality(win.navigator.gpu, () => win.performance.now()));
    return detect().then((d) => {
      st.detection = d;
      st.detecting = false;
      settings = {
        ...settings,
        quality: {
          ...settings.quality,
          detected: d.tier,
          adapter: d.adapter,
          benchMs: d.benchMs,
          at: new Date().toISOString(),
        },
      };
      save();
    });
  };

  const launch = (kind: Launch): void => {
    root.classList.add("leaving");
    go(launchQuery(kind, settings, win.location.search));
  };

  root.addEventListener("click", (e) => {
    const b = (e.target as Element).closest("button");
    if (!b || !root.contains(b)) return;
    const a = b.dataset.a;
    if (a === "build" || a === "watch") launch(a);
    else if (a === "settings" || a === "marbles") openSheet(a);
    else if (a === "close") openSheet(null);
    else if (a === "redetect" && !st.detecting) void runDetect();
    else if (b.dataset.tier) {
      const t = b.dataset.tier;
      settings = { ...settings, quality: { ...settings.quality, override: t === "auto" ? null : (t as Tier) } };
      save();
    } else if (b.dataset.camera) {
      settings = { ...settings, camera: b.dataset.camera as Camera };
      save();
    } else if (b.dataset.type) {
      settings = { ...settings, marble: { ...shownPick(), type: b.dataset.type as MarbleType } };
      save();
    } else if (b.dataset.size) {
      settings = { ...settings, marble: { ...shownPick(), size: b.dataset.size as MarbleSize } };
      save();
    }
  });

  const mixChanged = (): void => {
    saveMixer(storage, mixer);
    mixerSink?.(mixer);
    render();
  };
  for (const input of $$<HTMLInputElement>("[data-bus]"))
    input.addEventListener("input", () => {
      mixer = { ...mixer, [input.dataset.bus as string]: Number(input.value) / 100 };
      mixChanged();
    });
  $<HTMLInputElement>("#pt-mix-mute").addEventListener("change", (e) => {
    mixer = { ...mixer, muted: (e.target as HTMLInputElement).checked };
    mixChanged();
  });

  const keys = (e: KeyboardEvent): void => {
    if (!st.shown || e.ctrlKey || e.metaKey || e.altKey) return;
    const tag = (e.target as HTMLElement | null)?.tagName;
    if (e.key === "Escape" && st.open) {
      e.preventDefault();
      openSheet(null);
      return;
    }
    if (tag === "INPUT" && (e.target as HTMLInputElement).type !== "checkbox") return;
    const k = e.key.toLowerCase();
    if (k === "b" || k === "w") launch(k === "b" ? "build" : "watch");
    else if (k === "m") openSheet("marbles");
    else if (k === "s") openSheet("settings");
    else return;
    e.preventDefault();
  };
  // Capture: the engine's canvas listens on the window too, and the title owns the keys while shown.
  win.addEventListener("keydown", keys, true);

  render();
  const ready = settings.quality.detected ? Promise.resolve() : runDetect();

  return {
    state: () => ({ ...st, settings }),
    launchQuery: (kind) => launchQuery(kind, settings, win.location.search),
    ready,
    setMixerSink: (fn) => {
      mixerSink = fn;
    },
  };
}
