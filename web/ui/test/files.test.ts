import { describe, expect, it } from "vitest";
import { applyFileEvents, emptyFiles, fileName, formatFiles } from "../src/files";

describe("files panel state", () => {
  it("tracks the loaded name, the last run and errors", () => {
    let s = applyFileEvents(emptyFiles(), [
      { type: "trackLoaded", name: "Demo", schema: 1, placements: 5, marbles: 4 },
    ]);
    expect(formatFiles(s)).toBe("Demo");
    s = applyFileEvents(s, [{ type: "runFinished", ticks: 2400, hash: "355343cf7b71522d" }]);
    expect(s).toEqual({ track: "Demo", runHash: "355343cf7b71522d", runTicks: 2400, error: null });
    s = applyFileEvents(s, [{ type: "bridgeError", command: "loadTrack", message: "bad" }]);
    expect(formatFiles(s)).toBe("loadTrack: bad");
    s = applyFileEvents(s, [{ type: "trackLoaded", name: "Next", schema: 1, placements: 0, marbles: 0 }]);
    expect(s).toEqual({ track: "Next", runHash: null, runTicks: null, error: null });
    expect(formatFiles(emptyFiles())).toBe("no track loaded");
  });

  it("names downloads from the track name", () => {
    expect(fileName("Round-trip demo")).toBe("round-trip-demo.parble");
    expect(fileName("  ***  ")).toBe("track.parble");
  });
});
