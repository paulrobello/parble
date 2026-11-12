// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import gpuHostA from "../../../docs/perf/gpu-tier-gpu-host-a.json";
import gpuHostB from "../../../docs/perf/gpu-tier-gpu-host-b.json";
import { MIXER_KEY } from "../src/audio/mixer";
import { TIER_MS, adapterName, median, tierFor } from "../src/title/gpu";
import {
  MARBLE_SIZES,
  MARBLE_TYPES,
  SETTINGS_KEY,
  defaultSettings,
  effectiveTier,
  launchQuery,
  parseSettings,
  titleQuery,
  titleWanted,
} from "../src/title/store";
import { MARBLE_INFO, mountTitle } from "../src/title/title";

class MemStorage {
  m = new Map<string, string>();
  getItem(k: string): string | null {
    return this.m.get(k) ?? null;
  }
  setItem(k: string, v: string): void {
    this.m.set(k, v);
  }
}

describe("titleWanted", () => {
  it("shows on a bare load and with title=1 only", () => {
    expect(titleWanted("")).toBe(true);
    expect(titleWanted("?title=1")).toBe(true);
    expect(titleWanted("?title=1&mute=1")).toBe(true);
    expect(titleWanted("?title=0")).toBe(false);
    for (const q of ["?mute=1", "?scenario=editor", "?screenshot=a.png", "?exit-after=4", "?quality=low"])
      expect(titleWanted(q)).toBe(false);
  });
});

describe("settings", () => {
  it("parses, defaults and refuses garbage field by field", () => {
    expect(parseSettings(null)).toEqual(defaultSettings());
    expect(parseSettings("{nope")).toEqual(defaultSettings());
    const s = parseSettings(
      JSON.stringify({
        quality: { detected: "high", override: "ultra", benchMs: 1.2, adapter: { vendor: "apple" } },
        camera: "orbit",
        marble: { type: "steel", size: "shooter" },
      }),
    );
    expect(s.quality.detected).toBe("high");
    expect(effectiveTier(s.quality)).toBe("ultra");
    expect(s.quality.adapter?.vendor).toBe("apple");
    expect(s.camera).toBe("orbit");
    expect(s.marble).toEqual({ type: "steel", size: "shooter" });
    const bad = parseSettings(
      JSON.stringify({ quality: { detected: "max" }, camera: "drone", marble: { type: "gold", size: "mini" } }),
    );
    expect(bad.quality.detected).toBeNull();
    expect(bad.camera).toBe("director");
    expect(bad.marble).toBeNull();
  });

  it("launches the editor with the pick and the showcase with the camera, at the effective tier", () => {
    const s = defaultSettings();
    expect(launchQuery("build", s, "")).toBe("?scenario=editor");
    s.quality.detected = "high";
    s.marble = { type: "opal", size: "mini" };
    s.camera = "overview";
    expect(launchQuery("build", s, "?title=1&mute=1")).toBe(
      "?scenario=editor&quality=high&marble=opal&marble-size=mini&mute=1",
    );
    expect(launchQuery("watch", s, "")).toBe("?scenario=showcase&quality=high&camera=overview");
    s.quality.override = "low";
    s.camera = "director";
    expect(launchQuery("watch", s, "")).toBe("?scenario=showcase&quality=low");
  });

  it("every launch query skips the title", () => {
    const s = defaultSettings();
    for (const k of ["build", "watch"] as const) expect(titleWanted(launchQuery(k, s, ""))).toBe(false);
  });
});

describe("quality detection", () => {
  it("maps the median pass time to a tier, a fallback adapter to low", () => {
    expect(tierFor(TIER_MS.ultra, false)).toBe("ultra");
    expect(tierFor(TIER_MS.ultra + 0.01, false)).toBe("high");
    expect(tierFor(TIER_MS.high + 0.01, false)).toBe("medium");
    expect(tierFor(TIER_MS.medium + 0.01, false)).toBe("low");
    expect(tierFor(0.1, true)).toBe("low");
    expect(median([5, 1, 3])).toBe(3);
    expect(median([4, 1, 3, 2])).toBe(2.5);
  });

  it("puts every measured host run at High (docs/perf/gpu-tier-*.json)", () => {
    type Rec = { browsers: Record<string, { runs: { benchMs: number | null; reason: string; discard: boolean }[] }> };
    for (const [host, rec] of [
      ["gpu-host-b", gpuHostB],
      ["gpu-host-a", gpuHostA],
    ] as [string, Rec][]) {
      for (const [browser, b] of Object.entries(rec.browsers)) {
        for (const r of b.runs.filter((x) => !x.discard && x.reason === "bench")) {
          expect(`${host} ${browser} ${tierFor(r.benchMs ?? Number.NaN, false)}`).toBe(`${host} ${browser} high`);
        }
      }
    }
  });

  it("names the adapter", () => {
    expect(adapterName(null)).toBe("unknown GPU");
    expect(adapterName({ vendor: "apple", architecture: "metal-3", device: "", description: "" })).toBe(
      "Apple metal-3",
    );
  });
});

describe("mountTitle", () => {
  const setup = (stored?: object) => {
    document.body.innerHTML = "";
    document.head.innerHTML = "";
    const storage = new MemStorage();
    if (stored) storage.setItem(SETTINGS_KEY, JSON.stringify(stored));
    const detect = vi.fn(async () => ({
      tier: "high" as const,
      adapter: { vendor: "apple", architecture: "metal-3", device: "", description: "" },
      benchMs: 1.1,
      reason: "bench" as const,
    }));
    const go = vi.fn();
    const onMixer = vi.fn();
    const t = mountTitle({ doc: document, win: window, storage: storage as unknown as Storage, detect, go, onMixer });
    return { t, storage, detect, go, onMixer, root: document.getElementById("pt") as HTMLElement };
  };
  const click = (root: HTMLElement, sel: string) => (root.querySelector(sel) as HTMLElement).click();

  it("detects on first run, stores the result and shows it", async () => {
    const { t, storage, detect, root } = setup();
    await t.ready;
    expect(detect).toHaveBeenCalledOnce();
    expect(JSON.parse(storage.getItem(SETTINGS_KEY) as string).quality.detected).toBe("high");
    expect(root.querySelector(".gpu b")?.textContent).toBe("High");
    expect(root.querySelector("h1")?.textContent).toBe("parble");
    expect(root.querySelector(".art img")).not.toBeNull();
  });

  it("skips detection when a result is stored", async () => {
    const { t, detect } = setup({ quality: { detected: "medium" } });
    await t.ready;
    expect(detect).not.toHaveBeenCalled();
  });

  it("settings: overrides the tier, sets the camera and drives the mixer", async () => {
    const { t, storage, root, onMixer } = setup({ quality: { detected: "high" } });
    await t.ready;
    click(root, '[data-a="settings"]');
    expect((root.querySelector("#pt-settings") as HTMLElement).hidden).toBe(false);
    expect(root.querySelectorAll("[data-tier]")).toHaveLength(5);
    click(root, '[data-tier="low"]');
    expect(t.state().settings.quality.override).toBe("low");
    expect(t.launchQuery("watch")).toContain("quality=low");
    click(root, '[data-tier="auto"]');
    expect(t.launchQuery("watch")).toContain("quality=high");
    click(root, '[data-camera="orbit"]');
    expect(t.launchQuery("watch")).toContain("camera=orbit");
    const music = root.querySelector('[data-bus="music"]') as HTMLInputElement;
    music.value = "40";
    music.dispatchEvent(new Event("input"));
    expect(onMixer).toHaveBeenLastCalledWith(expect.objectContaining({ music: 0.4 }));
    expect(JSON.parse(storage.getItem(MIXER_KEY) as string).music).toBe(0.4);
  });

  it("marble picker: every type and size, and Build carries the pick", async () => {
    const { t, root, go } = setup({ quality: { detected: "high" } });
    await t.ready;
    click(root, '[data-a="marbles"]');
    expect(root.querySelectorAll("[data-type]")).toHaveLength(MARBLE_TYPES.length);
    expect(root.querySelectorAll("[data-size]")).toHaveLength(MARBLE_SIZES.length);
    for (const ty of MARBLE_TYPES) expect(MARBLE_INFO[ty].grams).toHaveLength(3);
    click(root, '[data-type="steel"]');
    click(root, '[data-size="shooter"]');
    expect(root.querySelector('[data-type="steel"]')?.getAttribute("aria-checked")).toBe("true");
    expect(root.querySelector(".d-mass")?.textContent).toBe("63.9 g");
    click(root, '[data-a="build"]');
    expect(go).toHaveBeenCalledWith("?scenario=editor&quality=high&marble=steel&marble-size=shooter");
  });

  it("keys open sheets, Esc closes them and W launches the showcase", async () => {
    const { t, root, go } = setup({ quality: { detected: "medium" } });
    await t.ready;
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "m" }));
    expect(t.state().open).toBe("marbles");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(t.state().open).toBeNull();
    expect((root.querySelector("#pt-marbles") as HTMLElement).hidden).toBe(true);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "w" }));
    expect(go).toHaveBeenCalledWith("?scenario=showcase&quality=medium");
    // Back from the showcase restores this page from the back/forward cache: the title shows again.
    expect(root.classList.contains("leaving")).toBe(true);
    const restored = new Event("pageshow") as PageTransitionEvent;
    Object.defineProperty(restored, "persisted", { value: true });
    window.dispatchEvent(restored);
    expect(root.classList.contains("leaving")).toBe(false);
  });

  it("a scene's way back opens the title, keeping mute", () => {
    expect(titleQuery("?scenario=showcase&quality=high")).toBe("?title=1");
    expect(titleQuery("?scenario=showcase&mute=1")).toBe("?title=1&mute=1");
    for (const s of ["?scenario=showcase", "?scenario=editor&mute=1"]) expect(titleWanted(titleQuery(s))).toBe(true);
  });
});
