import { describe, expect, it } from "vitest";
import {
  CONTROLS_HINT,
  FLY_ONLY_CONTROLS_HINT,
  MAX_SPLITS,
  applyEvents,
  controlsHint,
  emptyHud,
  formatHud,
} from "../src/hud";

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
    const state = { ...emptyHud(), fps: 50, tick: 7 };
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

  it("lists each checkpoint split as run time, checkpoint and marble", () => {
    const state = applyEvents(emptyHud(), [
      { type: "frame", tick: 140, fps: 60 },
      { type: "split", tick: 131, instance: [3, 0, -4], marble: 2 },
      { type: "split", tick: 600, instance: [9, 1, 2], marble: 0 },
      { type: "split", tick: 1205, instance: [3, 0, -4], marble: 1 },
      { type: "frame", tick: 1210, fps: 60 },
    ]);
    expect(formatHud(state)).toBe(
      [
        "fps 60  tick 1210",
        "split cp1  0.546 s  marble 2",
        "split cp2  2.500 s  marble 0",
        "split cp1  5.021 s  marble 1",
      ].join("\n"),
    );
  });

  it("keeps the latest splits and clears them when the run restarts", () => {
    let state = applyEvents(emptyHud(), [{ type: "frame", tick: 10, fps: 60 }]);
    for (let i = 0; i < MAX_SPLITS + 2; i++) {
      state = applyEvents(state, [{ type: "split", tick: 20 + i, instance: [0, 0, 0], marble: i }]);
    }
    expect(state.splits.map((s) => s.marble)).toEqual([2, 3, 4, 5]);
    state = applyEvents(state, [{ type: "frame", tick: 4, fps: 60 }]);
    expect(state.splits).toEqual([]);
    expect(formatHud(state)).toBe("fps 60  tick 4");
  });

  it("names the fly camera's enter and exit keys", () => {
    expect(CONTROLS_HINT).toMatch(/F: fly camera/);
    expect(CONTROLS_HINT).toMatch(/F\/Esc back/);
  });

  it("drops the director's keys from the fly-only controls line", () => {
    expect(controlsHint(false)).toBe(CONTROLS_HINT);
    expect(controlsHint(true)).toBe(FLY_ONLY_CONTROLS_HINT);
    expect(FLY_ONLY_CONTROLS_HINT).toMatch(/WASD move {2}Q\/E down\/up {2}drag look/);
    expect(FLY_ONLY_CONTROLS_HINT).not.toMatch(/Esc|back|\bC\b|orbit|director/i);
  });
});
