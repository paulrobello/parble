import { describe, expect, it } from "vitest";
import { CONTROLS_HINT, applyEvents, emptyHud, formatHud } from "../src/hud";

describe("HUD", () => {
  it("shows placeholders before the first frame", () => {
    expect(formatHud(emptyHud())).toBe("fps --  tick --");
  });

  it("shows the latest frame's rounded fps and tick", () => {
    const state = applyEvents(emptyHud(), [
      { type: "ready", protocol: 1 },
      { type: "frame", tick: 10, fps: 30 },
      { type: "pong", id: 1 },
      { type: "frame", tick: 24, fps: 59.6 },
    ]);
    expect(formatHud(state)).toBe("fps 60  tick 24");
  });

  it("keeps the last state when no frame arrives", () => {
    const state = { fps: 50, tick: 7 };
    expect(applyEvents(state, [])).toBe(state);
  });

  it("adds the pause and time scale only when they apply", () => {
    const at = (e: Partial<{ paused: true; timeScale: number }>) =>
      formatHud(applyEvents(emptyHud(), [{ type: "frame", tick: 8, fps: 60, ...e }]));
    expect(at({})).toBe("fps 60  tick 8");
    expect(at({ timeScale: 1 })).toBe("fps 60  tick 8");
    expect(at({ paused: true })).toBe("fps 60  tick 8  paused");
    expect(at({ timeScale: 0.25 })).toBe("fps 60  tick 8  0.25x");
    expect(at({ paused: true, timeScale: 0.1 })).toBe("fps 60  tick 8  paused  0.1x");
  });

  it("names the playback keys", () => {
    expect(CONTROLS_HINT).toMatch(/P pause {2}, slower {2}\. faster {2}N step/);
  });

  it("names the fly camera's enter and exit keys", () => {
    expect(CONTROLS_HINT).toMatch(/F: fly camera/);
    expect(CONTROLS_HINT).toMatch(/F\/Esc back/);
  });
});
