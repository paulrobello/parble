import { describe, expect, it } from "vitest";
import { EDGE_PX, IDLE_MS, RunFade } from "../src/editor/fade";
import { ENGINE_KEYS, type KeyLike, keyResult, ownsKeys } from "../src/editor/keys";
import {
  arrowPath,
  cellAt,
  familyOf,
  familyText,
  boxPath,
  checkReleaseText,
  checkText,
  countBadge,
  faceWord,
  focusOrbit,
  ghostText,
  issueText,
  levelLabel,
  type NudgeZoneDiag,
  nudgeAccelText,
  nudgeEdit,
  nudgeHeadingText,
  nudgeMode,
  nudgeName,
  nudgeSizeText,
  nudgeSpeedText,
  pieceName,
  piecesOf,
  planThumb,
  portsText,
  runTime,
  sliceDown,
  sliceUp,
  BUILD_LEVEL_MAX,
  strokeText,
  zoneOf,
} from "../src/editor/model";

const key = (code: string, mods: Partial<KeyLike> = {}): KeyLike => ({
  key: code,
  code,
  shiftKey: false,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  ...mods,
});

describe("editor model", () => {
  it("names pieces and sorts them into the drawer families", () => {
    expect(pieceName("blk_slope_steep_l3")).toBe("Steep slope · 3");
    expect(pieceName("mech_hopper")).toBe("Hopper");
    expect(pieceName("rail_s_1x3_l2")).toBe("S-bend · 2");
    expect(familyOf("blk_pillar")).toBe("supports");
    expect(familyOf("blk_tower_socket_l1")).toBe("supports");
    expect(familyOf("rail_joint_l1")).toBe("rails");
    expect(familyOf("mech_toggle_l3")).toBe("mechanisms");
    expect(familyOf("blk_end_l3")).toBe("blocks");
    expect(piecesOf(["mech_arch_rope_l1", "mech_finish", "mech_hopper", "blk_end_l3"], "mechanisms")).toEqual([
      "mech_hopper",
      "mech_finish",
      "mech_arch_rope_l1",
    ]);
  });

  it("writes each issue as a plain sentence", () => {
    const base = { piece: null, anchor: null, cell: null, face: null, fault: null, otherPiece: null };
    expect(issueText({ ...base, kind: "openPort", piece: "blk_curve90_l3", face: "+x" })).toBe(
      "Open east port on Curve 90° · 3",
    );
    expect(
      issueText({
        ...base,
        kind: "brokenLink",
        piece: "rail_straight_l2",
        otherPiece: "blk_end_l3",
        fault: "classMismatch",
      }),
    ).toBe("Broken link: Straight · 2 into End stop · 3, a rail meets a block channel");
    expect(issueText({ ...base, kind: "unreachable", piece: "blk_pillar" })).toBe(
      "Unreachable: no marble gets to this Pillar",
    );
    expect(issueText({ ...base, kind: "missingHopper" })).toBe("Add a hopper so the marbles have a start");
    expect(issueText({ ...base, kind: "missingFinish" })).toBe("Add finish cups to end the run");
  });

  it("labels and steps the level slice", () => {
    expect(levelLabel(null, 4)).toBe("L0–L4");
    expect(levelLabel(2, 4)).toBe("≤ L2");
    expect(sliceDown(null, 4)).toBe(4);
    expect(sliceDown(0, 4)).toBe(0);
    expect(sliceUp(3, 4)).toBe(4);
    // Past the top piece `]` keeps raising the build level, from all to one above the top.
    expect(sliceUp(4, 4)).toBe(5);
    expect(sliceUp(null, 4)).toBe(5);
    expect(sliceUp(null, 0)).toBe(1);
    expect(sliceUp(BUILD_LEVEL_MAX, 4)).toBe(BUILD_LEVEL_MAX);
    expect(levelLabel(6, 0)).toBe("L6");
  });

  it("formats ports, families, the ghost tip and run time", () => {
    expect(
      portsText([
        { face: "-z", flow: "in" },
        { face: "+x", flow: "out" },
        { face: "-x", flow: "out" },
      ]),
    ).toBe("In N · Out E, W");
    expect(familyText({ blk: 6, rail: 1, mech: 0 })).toBe("6 blocks · 1 rail");
    const hover = { source: "cursor", hit: "ground", hitCell: null, hitAnchor: null, error: null };
    expect(ghostText({ ...hover, target: [11, 2, -4], valid: true })).toEqual({
      text: "[11, 2, −4] · L2 · valid",
      valid: true,
    });
    expect(ghostText({ ...hover, target: [1, 0, 0], valid: false, error: "cell (1,0,0) is occupied" })?.text).toBe(
      "[1, 0, 0] · L0 · cell taken",
    );
    expect(ghostText(null)).toBeNull();
    expect(runTime(600)).toBe("0:02");
    expect(runTime(240 * 75)).toBe("1:15");
  });

  it("draws the 12 edges of a projected footprint box, and nothing for a partial one", () => {
    const corners: [number, number][] = [
      [0, 0],
      [10, 0],
      [0, 10],
      [10, 10],
      [2, 2],
      [12, 2],
      [2, 12],
      [12, 12],
    ];
    const d = boxPath(corners);
    expect(d.match(/M/g)?.length).toBe(12);
    expect(d.startsWith("M0 0L10 0")).toBe(true);
    expect(d).toContain("M10 10L12 12");
    expect(boxPath(corners.slice(0, 7))).toBe("");
    expect(boxPath(null)).toBe("");
    expect(boxPath([[0.123, 4.56], ...corners.slice(1)]).startsWith("M0.1 4.6L")).toBe(true);
  });

  it("labels tile counts, stroke tips and option sides", () => {
    expect(countBadge(3)).toBe("×3");
    expect(countBadge(0)).toBe("");
    expect(countBadge(undefined)).toBe("");
    expect(strokeText(1)).toBe("Laying a run · 1 piece · release to finish");
    expect(strokeText(7)).toBe("Laying a run · 7 pieces · release to finish");
    expect(faceWord("+x")).toBe("East");
    expect(faceWord("-z")).toBe("North");
  });

  it("draws a Library plan thumbnail from the saved placements, top level last", () => {
    expect(planThumb([])).toBe("");
    const svg = planThumb([
      { piece: "blk_pillar", anchor: [0, 1, 0] },
      { piece: "blk_straight_l3", anchor: [0, 0, 0] },
      { piece: "rail_straight_l2", anchor: [3, 0, -2] },
      { piece: "mech_hopper", anchor: [1, 0, -1] },
    ]);
    expect(svg.startsWith('<svg viewBox="0 0 64 48"')).toBe(true);
    expect(svg.match(/<rect/g)?.length).toBe(4);
    // Blocks brass, rails maple, mechanisms pale blue, supports dark brass; the pillar (L1) on top.
    expect(svg).toContain('fill="#c49a52"');
    expect(svg).toContain('fill="#d9c2a0"');
    expect(svg).toContain('fill="#a8c8d8"');
    expect(svg.lastIndexOf('fill="#7d5f2c"')).toBeGreaterThan(svg.lastIndexOf('fill="#a8c8d8"'));
    // 4 columns by 3 rows of 8 px cells, centred, north (-z) up.
    expect(svg).toContain('<rect x="16" y="28" width="7.4"');
    expect(svg).toContain('<rect x="40" y="12" width="7.4"');
  });

  it("aims the focus orbit at the cell centre", () => {
    const o = focusOrbit([10, 2, -5], { yaw: 0.3, pitch: 0.2, radius: 3 });
    expect(o.target.map((v) => Math.round(v * 1000))).toEqual([600, 150, -300]);
    expect(o.yaw).toBe(0.3);
    expect(o.pitch).toBe(0.5);
    expect(o.radius).toBe(2);
  });
});

describe("marble check text", () => {
  const w = {
    kind: "stalled" as const,
    marble: "steel",
    also: [],
    size: "shooter",
    piece: "mech_hole_oneway_l1",
    anchor: null,
    cell: null,
    marbles: [9],
    rolling: 1,
    tick: 7200,
  };
  it("names the type, size, count and piece", () => {
    expect(checkText(w)).toBe("Steel shooter: 1 marble still rolling at the time limit on One-way hole · 1");
    expect(checkText({ ...w, marbles: [4, 6], rolling: 1, marble: "cats-eye", also: ["swirl"] })).toBe(
      "Cat's eye family shooter: 1 marble stops, 1 marble still rolling at the time limit on One-way hole · 1",
    );
    expect(checkText({ ...w, kind: "fellOff", marbles: [0, 1], rolling: 0, piece: "blk_straight_l1" })).toBe(
      "Steel shooter: 2 marbles fall off at Straight · 1",
    );
  });
  it("names the release it checked", () => {
    const r = { release: "all" as const, intervalTicks: 0, marbles: 10, seed: 2, sizes: ["standard"], allSizes: false };
    expect(checkReleaseText(r)).toBe("Release all · 10 standard marbles · seed 2");
    expect(checkReleaseText({ ...r, release: "interval", intervalTicks: 480, allSizes: true, marbles: 1 })).toBe(
      "Interval 2 s · 1 marble at every size · seed 2",
    );
  });
});

describe("editor keyboard map", () => {
  const d = { slice: null, maxLevel: 3 };
  it("maps tools, transforms, history and slicing to actions", () => {
    expect(keyResult(key("Digit1"), d)).toEqual({ kind: "actions", actions: [{ op: "tool", tool: "select" }] });
    expect(keyResult(key("Digit2"), d)).toEqual({ kind: "actions", actions: [{ op: "tool", tool: "place" }] });
    expect(keyResult(key("Digit3"), d)).toEqual({ kind: "actions", actions: [{ op: "tool", tool: "delete" }] });
    expect(keyResult(key("KeyR"), d)).toEqual({ kind: "actions", actions: [{ op: "rotate", by: 1 }] });
    expect(keyResult(key("KeyR", { shiftKey: true }), d)).toEqual({
      kind: "actions",
      actions: [{ op: "rotate", by: 3 }],
    });
    expect(keyResult(key("KeyZ", { metaKey: true }), d)).toEqual({ kind: "actions", actions: [{ op: "undo" }] });
    expect(keyResult(key("KeyZ", { ctrlKey: true, shiftKey: true }), d)).toEqual({
      kind: "actions",
      actions: [{ op: "redo" }],
    });
    expect(keyResult(key("BracketLeft"), d)).toEqual({ kind: "actions", actions: [{ op: "slice", level: 3 }] });
    expect(keyResult(key("BracketRight"), { slice: 1, maxLevel: 3 })).toEqual({
      kind: "actions",
      actions: [{ op: "slice", level: 2 }],
    });
    expect(keyResult(key("PageUp"), d)).toEqual({ kind: "actions", actions: [{ op: "move", offset: [0, 1, 0] }] });
    expect(keyResult(key("ArrowLeft"), d)).toEqual({ kind: "actions", actions: [{ op: "move", offset: [-1, 0, 0] }] });
  });

  it("hands overlay commands back and leaves browser and engine keys alone", () => {
    expect(keyResult(key("Space"), d)).toEqual({ kind: "ui", command: "space" });
    expect(keyResult(key("Escape"), d)).toEqual({ kind: "ui", command: "escape" });
    expect(keyResult(key("Backspace"), d)).toEqual({ kind: "ui", command: "remove" });
    expect(keyResult(key("KeyH"), d)).toEqual({ kind: "ui", command: "columns" });
    expect(keyResult(key("Tab"), d)).toEqual({ kind: "ui", command: "drawer" });
    expect(keyResult(key("Backslash"), d)).toEqual({ kind: "ui", command: "panel" });
    expect(keyResult(key("KeyX"), d)).toEqual({ kind: "ui", command: "sliceToggle" });
    expect(keyResult(key("Tab", { shiftKey: true }), d)).toBeNull();
    expect(keyResult(key("KeyN"), d)).toBeNull();
    expect(keyResult(key("Comma"), d)).toBeNull();
    expect(keyResult(key("KeyC", { metaKey: true }), d)).toBeNull();
    expect(ownsKeys({ tagName: "INPUT" } as unknown as EventTarget)).toBe(true);
    expect(ownsKeys({ tagName: "BUTTON" } as unknown as EventTarget)).toBe(false);
  });
});

describe("engine keys", () => {
  const d = { slice: null, maxLevel: 0 };
  it("P pauses, C toggles the follow cam and F hands fly to the engine", () => {
    expect([...ENGINE_KEYS].sort()).toEqual(["KeyC", "KeyF", "KeyP"]);
    expect(keyResult(key("KeyP"), d)).toEqual({ kind: "actions", actions: [{ op: "pause" }] });
    expect(keyResult(key("KeyC"), d)).toEqual({ kind: "ui", command: "follow" });
    expect(keyResult(key("KeyF"), d)).toEqual({ kind: "ui", command: "fly" });
    expect(keyResult(key("KeyP", { ctrlKey: true }), d)).toBeNull();
  });
});

describe("run fade", () => {
  it("fades after 1.2 s still, brings an edge back near it, and fades it again 1.2 s after leaving", () => {
    const f = new RunFade();
    expect(f.view(0).faded).toBe(false);
    f.setRunning(true, 0);
    f.move(500, 400, 1440, 900, 500);
    expect(f.view(500 + IDLE_MS - 1).faded).toBe(false);
    expect(f.view(500 + IDLE_MS).faded).toBe(true);
    f.move(700, EDGE_PX - 4, 1440, 900, 2_000);
    expect([...f.view(2_000).shown]).toEqual(["top"]);
    f.move(700, 400, 1440, 900, 2_500);
    expect([...f.view(2_500 + IDLE_MS - 1).shown]).toEqual(["top"]);
    expect([...f.view(2_500 + IDLE_MS).shown]).toEqual([]);
    f.move(1_430, 890, 1440, 900, 5_000);
    expect([...f.view(5_000).shown].sort()).toEqual(["bottom", "right"]);
    f.setRunning(false, 6_000);
    expect(f.view(9_000)).toEqual({ faded: false, shown: new Set() });
  });

  it("does nothing outside a run", () => {
    const f = new RunFade();
    f.move(0, 0, 100, 100, 0);
    expect(f.view(10_000).faded).toBe(false);
  });
});

describe("nudge zones", () => {
  const listed: NudgeZoneDiag = {
    index: 2,
    name: "",
    minUm: [30_000, 60_000, -390_000],
    maxUm: [750_000, 120_000, -330_000],
    yawMdeg: 0,
    pitchMdeg: 0,
    minSpeedUmS: 200_000,
    maxAccelUmS2: 1_000_000,
    cell: [6, 1, -6],
    outline: null,
    arrow: null,
  };
  const zone = zoneOf(listed);

  it("a listed zone goes back to its file shape, omitting the empty name and zero pitch", () => {
    expect(zone).toEqual({
      min_um: [30_000, 60_000, -390_000],
      max_um: [750_000, 120_000, -330_000],
      yaw_mdeg: 0,
      min_speed_um_s: 200_000,
      max_accel_um_s2: 1_000_000,
    });
    expect(zoneOf({ ...listed, name: "lift", pitchMdeg: 15_000 })).toMatchObject({ name: "lift", pitch_mdeg: 15_000 });
  });

  it("each inspector edit steps one field and stops at the file's limits", () => {
    expect(nudgeEdit(zone, { kind: "size", axis: 0, by: 1 })?.max_um).toEqual([810_000, 120_000, -330_000]);
    expect(nudgeEdit(zone, { kind: "size", axis: 1, by: -1 })).toBeNull();
    expect(nudgeEdit(zone, { kind: "size", axis: 0, by: -1 })?.max_um[0]).toBe(690_000);
    expect(nudgeEdit(zone, { kind: "yaw", by: -1 })?.yaw_mdeg).toBe(315_000);
    expect(nudgeEdit(zone, { kind: "yaw", by: 1 })?.yaw_mdeg).toBe(45_000);
    expect(nudgeEdit(zone, { kind: "pitch", by: 1 })?.pitch_mdeg).toBe(15_000);
    expect(nudgeEdit({ ...zone, pitch_mdeg: 90_000 }, { kind: "pitch", by: 1 })).toBeNull();
    expect(nudgeEdit({ ...zone, pitch_mdeg: 15_000 }, { kind: "pitch", by: -1 })).not.toHaveProperty("pitch_mdeg");
    expect(nudgeEdit(zone, { kind: "speed", by: 1 })?.min_speed_um_s).toBe(250_000);
    expect(nudgeEdit({ ...zone, min_speed_um_s: 50_000 }, { kind: "speed", by: -1 })).toBeNull();
    expect(nudgeEdit(zone, { kind: "accel", by: 1 })?.max_accel_um_s2).toBe(1_250_000);
    expect(nudgeEdit({ ...zone, max_accel_um_s2: 2_000_000 }, { kind: "accel", by: 1 })).toBeNull();
    // The edit never touches the zone it was given.
    expect(zone.max_um).toEqual([750_000, 120_000, -330_000]);
  });

  it("keeps a constant zone's mode through every edit, and omits top-up from the file shape", () => {
    expect(zoneOf({ ...listed, mode: "top_up" })).not.toHaveProperty("mode");
    expect(nudgeMode(listed)).toBe("top_up");
    const constant = zoneOf({ ...listed, mode: "constant" });
    expect(constant.mode).toBe("constant");
    expect(nudgeEdit(zone, { kind: "mode", mode: "constant" })).toEqual({ ...zone, mode: "constant" });
    expect(nudgeEdit(zone, { kind: "mode", mode: "top_up" })).toBeNull();
    expect(nudgeEdit(constant, { kind: "mode", mode: "constant" })).toBeNull();
    expect(nudgeEdit(constant, { kind: "mode", mode: "top_up" })).toEqual(zone);
    for (const e of [
      { kind: "size", axis: 0, by: 1 },
      { kind: "yaw", by: 1 },
      { kind: "pitch", by: 1 },
      { kind: "speed", by: 1 },
      { kind: "accel", by: 1 },
    ] as const)
      expect(nudgeEdit(constant, e)?.mode, e.kind).toBe("constant");
  });

  it("labels a zone's box, heading and rates in plain units", () => {
    expect(nudgeSizeText(listed)).toBe("12 × 1 × 1 cells");
    expect(nudgeHeadingText(0)).toBe("east");
    expect(nudgeHeadingText(90_000)).toBe("north");
    expect(nudgeHeadingText(315_000, -15_000)).toBe("south-east, down 15°");
    expect(nudgeHeadingText(12_500)).toBe("12.5°");
    expect(nudgeSpeedText(200_000)).toBe("0.20 m/s");
    expect(nudgeAccelText(1_000_000)).toBe("1.00 m/s²");
    expect(nudgeName(listed)).toBe("Zone 3");
    expect(nudgeName({ index: 0, name: "lift" })).toBe("lift");
  });

  it("finds the cell under the orbit target and draws an arrow with a head", () => {
    expect(cellAt([0.36, 0.07, -0.37])).toEqual([6, 1, -6]);
    expect(cellAt([0, -0.2, 0])).toEqual([0, 0, 0]);
    const d = arrowPath([
      [10, 50],
      [110, 50],
    ]);
    expect(d.match(/M/g)?.length).toBe(3);
    expect(d.startsWith("M10 50L110 50M110 50L101 ")).toBe(true);
    expect(arrowPath(null)).toBe("");
    expect(
      arrowPath([
        [5, 5],
        [5.2, 5],
      ]),
    ).toBe("");
  });
});
