// stage-audio.mjs: only picked stems of runtime cues ship, copies are hash-checked, and the real picks
// resolve against the real manifest.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { buildCatalog, isRuntimeCue, MECHANISM_CUES, stage, TUNNEL_CUE } from "./stage-audio.mjs";

const sha = (s) => createHash("sha256").update(s).digest("hex");

function item(cue, stem, loop, family = cue.split("_")[0]) {
  return {
    cue_id: cue,
    stem,
    loop,
    duration_s: 1.5,
    files: {
      ogg: { path: `${family}/${stem}.ogg`, sha256: sha(`${stem}.ogg`) },
      m4a: { path: `${family}/${stem}.m4a`, sha256: sha(`${stem}.m4a`) },
    },
  };
}

const MANIFEST = {
  items: [
    item("roll_glass_block", "roll_glass_block_v01", true),
    item("roll_glass_block", "roll_glass_block_v02", true),
    item("roll_glass_rug", "roll_glass_rug_v07", true),
    item("impact_steel_rail_heavy", "impact_steel_rail_heavy_v01", false),
    item("impact_steel_rail_heavy", "impact_steel_rail_heavy_v02", false),
    item("ui_click", "ui_click_v01", false),
  ],
};
const PICKS = {
  _meta: { approved_by: "Paul" },
  notes: {},
  roll_glass_block: ["roll_glass_block_v02"],
  roll_glass_rug: ["roll_glass_rug_v07"],
  impact_steel_rail_heavy: ["impact_steel_rail_heavy_v01", "impact_steel_rail_heavy_v02"],
  ui_click: ["ui_click_v01"],
};

test("runtime cues are rolls, impacts, clacks, the mechanism signal cues and the tunnel rumble, never rug", () => {
  assert.ok(isRuntimeCue("roll_steel_rail"));
  assert.ok(isRuntimeCue("impact_glass_block_light"));
  assert.ok(isRuntimeCue("clack_glass_steel_medium"));
  for (const id of MECHANISM_CUES) assert.ok(isRuntimeCue(id), id);
  assert.ok(!isRuntimeCue("roll_glass_rug"));
  assert.ok(!isRuntimeCue("impact_solid_rug_heavy"));
  assert.ok(!isRuntimeCue("ui_click"));
  assert.ok(isRuntimeCue("tunnel_rumble"));
  assert.ok(!isRuntimeCue("tunnel_rumble_rug"));
});

test("the catalog holds only picked stems of runtime cues", () => {
  const c = buildCatalog(PICKS, MANIFEST);
  assert.deepEqual(Object.keys(c.cues), ["impact_steel_rail_heavy", "roll_glass_block"]);
  assert.deepEqual(
    c.cues.roll_glass_block.variants.map((v) => v.stem),
    ["roll_glass_block_v02"],
  );
  assert.equal(c.cues.roll_glass_block.loop, true);
  assert.equal(c.cues.impact_steel_rail_heavy.loop, false);
  assert.equal(c.cues.roll_glass_block.variants[0].ogg, "sfx/roll/roll_glass_block_v02.ogg");
});

test("a picked stem missing from the manifest fails", () => {
  assert.throws(() => buildCatalog({ roll_glass_block: ["roll_glass_block_v09"] }, MANIFEST), /not in the manifest/);
});

test("stage copies hash-checked files and writes sfx.json", () => {
  const dir = mkdtempSync(join(tmpdir(), "stage-audio-"));
  try {
    const sfx = join(dir, "sfx");
    for (const it of MANIFEST.items) {
      for (const f of Object.values(it.files)) {
        mkdirSync(join(sfx, f.path, ".."), { recursive: true });
        writeFileSync(join(sfx, f.path), `${it.stem}.${f.path.split(".").pop()}`);
      }
    }
    const out = join(dir, "out");
    assert.equal(stage({ picks: PICKS, manifest: MANIFEST, sfxDir: sfx, out }), 6);
    assert.ok(existsSync(join(out, "sfx/impact/impact_steel_rail_heavy_v02.m4a")));
    assert.ok(!existsSync(join(out, "sfx/roll/roll_glass_block_v01.ogg")));
    assert.ok(!existsSync(join(out, "sfx/roll/roll_glass_rug_v07.ogg")));
    assert.equal(JSON.parse(readFileSync(join(out, "sfx.json"), "utf8")).schema, "parble.sfx-runtime/1");
    writeFileSync(join(sfx, "roll/roll_glass_block_v02.ogg"), "tampered");
    assert.throws(() => stage({ picks: PICKS, manifest: MANIFEST, sfxDir: sfx, out }), /does not match/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("the real picks resolve against the real manifest", () => {
  const root = new URL("../../", import.meta.url);
  const picks = JSON.parse(readFileSync(new URL("docs/audio/audition-picks.json", root), "utf8"));
  const manifest = JSON.parse(readFileSync(new URL("audition/sfx/manifest.json", root), "utf8"));
  const c = buildCatalog(picks, manifest);
  // 6 roll loops (3 materials x block, rail), 18 impact sets, 18 clack sets, the 10 mechanism cues and
  // the tunnel rumble loop.
  const ids = Object.keys(c.cues);
  assert.equal(ids.filter((id) => id.startsWith("roll_")).length, 6);
  assert.equal(ids.filter((id) => id.startsWith("impact_")).length, 18);
  assert.equal(ids.filter((id) => id.startsWith("clack_")).length, 18);
  for (const id of MECHANISM_CUES) assert.ok(ids.includes(id), id);
  assert.equal(c.cues[TUNNEL_CUE].loop, true);
  assert.deepEqual(
    c.cues[TUNNEL_CUE].variants.map((v) => v.stem),
    ["tunnel_rumble_v01m", "tunnel_rumble_v02m", "tunnel_rumble_v03m"],
  );
  assert.equal(ids.length, 6 + 18 + 18 + MECHANISM_CUES.length + 1);
  assert.ok(ids.every((id) => c.cues[id].variants.length > 0));
});
