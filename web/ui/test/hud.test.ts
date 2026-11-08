import { describe, expect, it } from "vitest";
import { applyEvents, emptyHud, formatHud } from "../src/hud";

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
});
