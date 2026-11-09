// Mixer settings (P4-T5): master, music and SFX levels and a mute switch, persisted to localStorage
// and applied to the engine's buses (engine.ts BUS_DEFAULTS). A level is a 0-1 slider position; the
// bus gain is the bus default times the position squared, so the default position (1) keeps the
// engine's mix and the slider's travel is closer to even in loudness.

import { BUS_DEFAULTS, type Bus } from "./engine";

export const MIXER_KEY = "parble.mixer/1";
export const BUSES = ["master", "music", "sfx"] as const satisfies readonly Bus[];

export interface MixerSettings {
  master: number;
  music: number;
  sfx: number;
  muted: boolean;
}

export const defaultMixer = (): MixerSettings => ({ master: 1, music: 1, sfx: 1, muted: false });

const level = (v: unknown, fallback: number): number =>
  typeof v === "number" && Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : fallback;

/** Settings from `raw` (the stored JSON), each field falling back to its default when missing or invalid. */
export function parseMixer(raw: string | null): MixerSettings {
  const d = defaultMixer();
  if (!raw) return d;
  let v: unknown;
  try {
    v = JSON.parse(raw);
  } catch {
    return d;
  }
  if (typeof v !== "object" || v === null || Array.isArray(v)) return d;
  const o = v as Record<string, unknown>;
  return {
    master: level(o.master, d.master),
    music: level(o.music, d.music),
    sfx: level(o.sfx, d.sfx),
    muted: typeof o.muted === "boolean" ? o.muted : d.muted,
  };
}

/** The stored settings; defaults when storage is unavailable or holds garbage. */
export function loadMixer(storage: Pick<Storage, "getItem"> | null): MixerSettings {
  try {
    return parseMixer(storage?.getItem(MIXER_KEY) ?? null);
  } catch {
    return defaultMixer();
  }
}

/** Store `s`; false when storage refuses (private mode, quota). */
export function saveMixer(storage: Pick<Storage, "setItem"> | null, s: MixerSettings): boolean {
  try {
    storage?.setItem(MIXER_KEY, JSON.stringify(s));
    return storage !== null;
  } catch {
    return false;
  }
}

/** Bus gains for `s`. Mute zeroes the master. */
export function busGains(s: MixerSettings): Record<Bus, number> {
  return {
    master: s.muted ? 0 : BUS_DEFAULTS.master * s.master ** 2,
    music: BUS_DEFAULTS.music * s.music ** 2,
    sfx: BUS_DEFAULTS.sfx * s.sfx ** 2,
  };
}

export interface MixerPanel {
  settings(): MixerSettings;
}

const LABELS: Record<(typeof BUSES)[number], string> = { master: "Master", music: "Music", sfx: "SFX" };

/**
 * A "Sound" button in the bottom-right corner that opens the mixer: one slider per bus and a mute
 * box. Every change is stored and handed to `apply`.
 */
export function mountMixer(doc: Document, storage: Storage | null, apply: (s: MixerSettings) => void): MixerPanel {
  let s = loadMixer(storage);
  const root = doc.createElement("div");
  root.id = "parble-mixer";
  Object.assign(root.style, {
    position: "fixed",
    bottom: "8px",
    right: "8px",
    display: "flex",
    flexDirection: "column",
    alignItems: "flex-end",
    gap: "6px",
    padding: "4px 8px",
    borderRadius: "4px",
    background: "rgba(28, 26, 23, 0.72)",
    color: "#ece4d8",
    font: "12px/1.4 ui-monospace, Menlo, monospace",
    zIndex: "10",
  });
  const panel = doc.createElement("div");
  panel.id = "parble-mixer-panel";
  Object.assign(panel.style, { display: "none", gridTemplateColumns: "auto 120px 3ch", gap: "4px 8px" });
  const toggle = doc.createElement("button");
  toggle.id = "parble-mixer-toggle";
  toggle.type = "button";
  toggle.textContent = "Sound";
  toggle.setAttribute("aria-expanded", "false");
  toggle.setAttribute("aria-controls", panel.id);
  let open = false;
  toggle.addEventListener("click", () => {
    open = !open;
    panel.style.display = open ? "grid" : "none";
    toggle.setAttribute("aria-expanded", String(open));
  });

  const changed = (): void => {
    saveMixer(storage, s);
    apply(s);
  };
  for (const bus of BUSES) {
    const label = doc.createElement("label");
    label.htmlFor = `parble-mixer-${bus}`;
    label.textContent = LABELS[bus];
    const input = doc.createElement("input");
    input.type = "range";
    input.id = `parble-mixer-${bus}`;
    input.min = "0";
    input.max = "100";
    input.step = "1";
    input.value = String(Math.round(s[bus] * 100));
    const readout = doc.createElement("span");
    readout.textContent = input.value;
    input.addEventListener("input", () => {
      s = { ...s, [bus]: Number(input.value) / 100 };
      readout.textContent = input.value;
      changed();
    });
    panel.append(label, input, readout);
  }
  const muteLabel = doc.createElement("label");
  muteLabel.htmlFor = "parble-mixer-mute";
  muteLabel.textContent = "Mute";
  const mute = doc.createElement("input");
  mute.type = "checkbox";
  mute.id = "parble-mixer-mute";
  mute.checked = s.muted;
  mute.addEventListener("change", () => {
    s = { ...s, muted: mute.checked };
    changed();
  });
  panel.append(muteLabel, mute);
  root.append(panel, toggle);
  doc.body.append(root);
  apply(s);
  return { settings: () => ({ ...s }) };
}
