import { describe, expect, it } from "vitest";
import { checkParbleFile, MAX_SAFE, PHYSICS_VERSION, parseParble, SCHEMA_VERSION } from "../src/parble";
import fixtures from "../src/parble-fixtures.json";

describe(".parble fixtures (shared with crates/parble-core/src/file.rs)", () => {
  it("agree on the versions and the integer limit", () => {
    expect(fixtures.schema).toBe(SCHEMA_VERSION);
    expect(fixtures.physics).toBe(PHYSICS_VERSION);
    expect(fixtures.maxSafeInteger).toBe(MAX_SAFE);
  });

  it("accept every valid file unchanged", () => {
    expect(fixtures.valid.length).toBeGreaterThan(0);
    for (const c of fixtures.valid) {
      const r = checkParbleFile(c.file);
      expect(r, c.name).toEqual({ ok: true, file: c.file });
      expect(c.runHash, c.name).toMatch(/^[0-9a-f]{16}$/);
    }
  });

  it("refuse every file Rust refuses", () => {
    expect(fixtures.rejected.length).toBeGreaterThan(0);
    for (const c of fixtures.rejected) expect(checkParbleFile(c.file).ok, c.why).toBe(false);
  });

  it("parse text, refusing non-JSON", () => {
    const demo = fixtures.valid[0];
    expect(parseParble(JSON.stringify(demo?.file))).toEqual(demo?.file);
    expect(parseParble("not json")).toBeNull();
  });
});
