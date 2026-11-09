import { describe, expect, it } from "vitest";
import { EDGE_PX, IDLE_MS, RunFade } from "../src/editor/fade";
import { type KeyLike, keyResult, ownsKeys } from "../src/editor/keys";
import {
  familyOf,
  familyText,
  focusOrbit,
  ghostText,
  issueText,
  levelLabel,
  pieceName,
  piecesOf,
  portsText,
  runTime,
  sliceDown,
  sliceUp,
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
    expect(sliceUp(4, 4)).toBeNull();
    expect(sliceUp(null, 4)).toBeNull();
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

  it("aims the focus orbit at the cell centre", () => {
    const o = focusOrbit([10, 2, -5], { yaw: 0.3, pitch: 0.2, radius: 3 });
    expect(o.target.map((v) => Math.round(v * 1000))).toEqual([600, 150, -300]);
    expect(o.yaw).toBe(0.3);
    expect(o.pitch).toBe(0.5);
    expect(o.radius).toBe(2);
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
    expect(keyResult(key("Tab"), d)).toEqual({ kind: "ui", command: "drawer" });
    expect(keyResult(key("Backslash"), d)).toEqual({ kind: "ui", command: "panel" });
    expect(keyResult(key("KeyX"), d)).toEqual({ kind: "ui", command: "sliceToggle" });
    expect(keyResult(key("Tab", { shiftKey: true }), d)).toBeNull();
    expect(keyResult(key("KeyF"), d)).toBeNull();
    expect(keyResult(key("KeyP"), d)).toBeNull();
    expect(keyResult(key("KeyC", { metaKey: true }), d)).toBeNull();
    expect(ownsKeys({ tagName: "INPUT" } as unknown as EventTarget)).toBe(true);
    expect(ownsKeys({ tagName: "BUTTON" } as unknown as EventTarget)).toBe(false);
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
