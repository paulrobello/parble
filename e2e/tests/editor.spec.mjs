// P3-T4 editor interaction in GPU Chromium (crates/parble-app/src/editor.rs). Scripted placement of
// 50 pieces with the real mouse: 40 on the ground and 10 stacked on top by picking existing pieces,
// with the ghost preview checked before every click. Then ghost validity and port hints, height
// slicing (state, picking and pixels), box select by mouse drag and by cell box, a move-group drag,
// undo/redo and the delete tool. Actions and pixel projections go through
// window.wasmBindings.parbleEdit; state comes from window.__parble.editor.
import { expect, test } from "@playwright/test";

const BAD_TEXT = (t) => /%cERROR%c|\bERROR\b/.test(t) || /uncaptured|invalid|validation|panicked/i.test(t);
const PALETTE = ["blk_straight_l3", "blk_curve90_l3", "blk_slope_gentle_l3", "blk_end_l3"];
// Rows of ten on the ground behind the tray: cells x 0..9, z −5..−8.
const ground = Array.from({ length: 40 }, (_, i) => [i % 10, 0, -5 - Math.floor(i / 10)]);
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

let nextId = 1;

async function frames(page, n = 2) {
  for (let i = 0; i < n; i++) await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => r())));
}

async function edit(page, action) {
  const id = nextId++;
  await page.evaluate(([a, id]) => window.wasmBindings.parbleEdit(JSON.stringify({ ...a, id })), [action, id]);
  await page.waitForFunction((id) => window.__parble?.editor?.last?.id === id, id, { timeout: 10_000 });
  return page.evaluate(() => window.__parble.editor.last);
}

const state = (page) => page.evaluate(() => window.__parble.editor);

/// Page pixels of cells: { centre: [x, y], floor: [x, y] } (the canvas starts at `box`).
async function project(page, box, cells) {
  const last = await edit(page, { op: "project", cells });
  expect(last.ok).toBe(true);
  return last.projected.map((p) => ({
    centre: [box.x + p[0], box.y + p[1]],
    floor: [box.x + p[2], box.y + p[3]],
  }));
}

async function hoverAt(page, [x, y], want) {
  await page.mouse.move(x, y);
  await page.waitForFunction(
    (want) => {
      const h = window.__parble?.editor?.hover;
      return h && JSON.stringify(h.target) === JSON.stringify(want);
    },
    want,
    { timeout: 5_000 },
  );
  return (await state(page)).hover;
}

/// Mean (r, g, b) of a 16 × 16 patch around a page pixel.
async function patch(page, [x, y]) {
  const png = await page.screenshot({ clip: { x: x - 8, y: y - 8, width: 16, height: 16 } });
  return page.evaluate(async (b64) => {
    const img = new Image();
    img.src = `data:image/png;base64,${b64}`;
    await img.decode();
    const c = new OffscreenCanvas(img.width, img.height);
    const g = c.getContext("2d");
    g.drawImage(img, 0, 0);
    const d = g.getImageData(0, 0, img.width, img.height).data;
    const s = [0, 0, 0];
    for (let i = 0; i < d.length; i += 4) for (let k = 0; k < 3; k++) s[k] += d[i + k];
    return s.map((v) => v / (d.length / 4));
  }, png.toString("base64"));
}

test("editor: ghost, picking, slicing, box select and move drag over 50 placed pieces", async ({ page }) => {
  const errors = [];
  page.on("console", (m) => { if (m.type() === "error" || BAD_TEXT(m.text())) errors.push(m.text()); });
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto("/?scenario=editor&quality=low");
  await page.waitForFunction(() => window.__parble?.ready === true && window.__parble.editor && window.__parble.frame > 10, null, {
    timeout: 60_000,
  });
  const canvas = page.locator("canvas#parble");
  const box = await canvas.boundingBox();
  expect((await page.evaluate(() => window.__parble.camera.mode))).toBe("orbit");
  // Nearly top-down over the build area (cell (7, 0, −7) at the centre).
  await edit(page, { op: "camera", target: [0.42, 0, -0.42], yaw: 0, pitch: 1.2, radius: 1.5 });
  await frames(page, 3);
  expect((await state(page)).palette).toEqual(PALETTE);

  // 40 ground placements with the real mouse; the ghost must sit on the hovered cell first.
  const groundPx = await project(page, box, ground);
  for (let i = 0; i < ground.length; i++) {
    if (i % 10 === 0) {
      await edit(page, { op: "brush", piece: PALETTE[(i / 10) % 4], yaw: 0 });
    }
    const [x, y] = groundPx[i].floor;
    expect(await page.evaluate(([x, y]) => document.elementFromPoint(x, y)?.id, [x, y])).toBe("parble");
    const h = await hoverAt(page, [x, y], ground[i]);
    expect(h.hit).toBe("ground");
    expect(h.valid).toBe(true);
    await page.mouse.click(x, y);
    await page.waitForFunction((n) => window.__parble.editor.pieces === n, i + 1, { timeout: 5_000 });
    if (i === 5) {
      // R turns the brush (the canvas has focus after the first click).
      await page.keyboard.press("r");
      await page.waitForFunction(() => window.__parble.editor.brush.yaw === 1);
    }
  }
  let s = await state(page);
  expect(s.pieces).toBe(40);
  expect(s.lastEdit.op).toBe("place");

  // 10 stacked on the first row by picking the pieces' top faces.
  await edit(page, { op: "brush", piece: "blk_straight_l3", yaw: 0 });
  const tops = ground.slice(0, 10).map(([x, , z]) => [x, 1, z]);
  const topPx = await project(page, box, tops);
  for (let i = 0; i < tops.length; i++) {
    const h = await hoverAt(page, topPx[i].floor, tops[i]);
    expect(h.hit).toBe("piece");
    expect(h.hitAnchor).toEqual(ground[i]);
    expect(h.valid).toBe(true);
    await page.mouse.click(...topPx[i].floor);
    await page.waitForFunction((n) => window.__parble.editor.pieces === n, 41 + i, { timeout: 5_000 });
  }
  s = await state(page);
  const kit = await page.evaluate(() => window.__parble.kit);
  console.log(`placed ${s.pieces}: kit ${JSON.stringify(kit.edit)}, colliders ${s.colliderChunks}, last ${JSON.stringify(s.lastEdit)}`);
  expect(s.pieces).toBe(50);
  expect(kit.pieces).toBe(50);
  expect(kit.edit.pieces).toBe(50);
  await page.waitForFunction(() => window.__parble.editor.dirtyColliderChunks === 0);
  expect((await state(page)).colliderChunks).toBeGreaterThan(0);

  // Ghost: an occupied cell is invalid; next to an isolated straight one side links.
  let last = await edit(page, { op: "hoverCell", cell: [3, 0, -6] });
  await frames(page);
  s = await state(page);
  expect(s.hover.valid).toBe(false);
  expect(s.hover.error).toMatch(/share cell/);
  await edit(page, { op: "place", piece: "blk_straight_l3", cell: [20, 0, -12], yaw: 0 });
  const hints = [];
  for (const d of [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]]) {
    await edit(page, { op: "hoverCell", cell: [20 + d[0], 0, -12 + d[2]] });
    await frames(page);
    const h = (await state(page)).hover;
    expect(h.valid).toBe(true);
    hints.push(h.ports.map((p) => p.hint));
  }
  console.log(`port hints around a straight: ${JSON.stringify(hints)}`);
  expect(hints.flat()).toContain("linked");
  expect(hints.flat()).toContain("open");
  // The ghost is drawn: green over a valid empty cell, nothing once unhovered.
  const [ghostPx] = await project(page, box, [[3, 0, -10]]);
  await edit(page, { op: "hoverCell", cell: [3, 0, -10] });
  await frames(page, 3);
  const withGhost = await patch(page, ghostPx.centre);
  await edit(page, { op: "unhover" });
  await page.mouse.move(box.x + 5, box.y + 5);
  await frames(page, 3);
  const without = await patch(page, ghostPx.centre);
  console.log(`ghost patch ${withGhost.map(Math.round)} vs empty ${without.map(Math.round)}`);
  expect(withGhost[1] - withGhost[0]).toBeGreaterThan(without[1] - without[0] + 20);
  await edit(page, { op: "undo" }); // the isolated straight

  // Height slicing: level 0 hides the 10 stacked pieces, picking passes to the piece below, and
  // the pixels over a stacked piece change.
  const stackedPx = topPx[4].floor;
  let h = await hoverAt(page, stackedPx, [4, 2, -5]);
  expect(h.hitAnchor).toEqual([4, 1, -5]);
  const before = await patch(page, (await project(page, box, [[4, 1, -5]]))[0].centre);
  await edit(page, { op: "slice", level: 0 });
  await frames(page, 3);
  s = await state(page);
  expect(s.slice).toBe(0);
  expect(s.visiblePieces).toBe(40);
  expect((await page.evaluate(() => window.__parble.kit.edit)).hiddenPieces).toBe(10);
  await page.mouse.move(stackedPx[0] + 1, stackedPx[1]);
  await page.waitForFunction(() => JSON.stringify(window.__parble.editor.hover?.hitAnchor) === "[4,0,-5]");
  const after = await patch(page, (await project(page, box, [[4, 1, -5]]))[0].centre);
  console.log(`slice 0: patch over stacked piece ${before.map(Math.round)} -> ${after.map(Math.round)}`);
  expect(Math.abs(before[0] - after[0]) + Math.abs(before[1] - after[1]) + Math.abs(before[2] - after[2])).toBeGreaterThan(6);
  // A cell box spanning both levels selects only the visible level while sliced.
  last = await edit(page, { op: "boxSelect", a: [0, 0, -5], b: [9, 1, -5] });
  expect((await state(page)).selection).toBe(10);
  await edit(page, { op: "slice", level: null });
  await edit(page, { op: "boxSelect", a: [0, 0, -5], b: [9, 1, -5] });
  expect((await state(page)).selection).toBe(20);
  await edit(page, { op: "clearSelection" });

  // Straight down for the drags, so the stacked row in front never hides row z = −6.
  await edit(page, { op: "camera", target: [0.42, 0, -0.42], yaw: 0, pitch: 1.45, radius: 1.5 });
  await frames(page, 3);
  // Box select with a mouse drag in the select tool: row z = −6, x 2..6.
  await page.keyboard.press("2");
  await page.waitForFunction(() => window.__parble.editor.tool === "select");
  const want = [2, 3, 4, 5, 6].map((x) => [x, 0, -6]);
  const wantPx = await project(page, box, want);
  const xs = wantPx.map((p) => p.centre[0]);
  const ys = wantPx.map((p) => p.centre[1]);
  const rect = [Math.min(...xs) - 12, Math.min(...ys) - 12, Math.max(...xs) + 12, Math.max(...ys) + 12];
  // A frame between the browser events, so the press lands where the drag starts.
  await page.mouse.move(rect[0], rect[1]);
  await frames(page);
  await page.mouse.down();
  await frames(page);
  await page.mouse.move(rect[2], rect[3], { steps: 6 });
  await frames(page);
  await page.mouse.up();
  await page.waitForFunction(() => window.__parble.editor.selection > 0, null, { timeout: 5_000 });
  s = await state(page);
  console.log(`box select ${JSON.stringify(rect.map(Math.round))}: ${JSON.stringify(s.selected)}`);
  expect(s.selected).toEqual(want);

  // Move-group drag: grab a selected piece and drag it 10 cells along +x.
  const [grab, dest] = await project(page, box, [[2, 0, -6], [12, 0, -6]]);
  await page.mouse.move(...grab.centre);
  await frames(page);
  await page.mouse.down();
  await frames(page);
  await page.mouse.move(...dest.floor, { steps: 10 });
  await page.waitForFunction(() => window.__parble.editor.drag?.offset?.[0] === 10, null, { timeout: 5_000 });
  expect((await state(page)).drag.valid).toBe(true);
  await frames(page);
  await page.mouse.up();
  await page.waitForFunction(() => window.__parble.editor.lastEdit?.op === "drag", null, { timeout: 5_000 });
  s = await state(page);
  const moved = want.map(([x, y, z]) => [x + 10, y, z]);
  console.log(`move drag: ${JSON.stringify(s.lastEdit)} -> ${JSON.stringify(s.selected)}`);
  expect(s.selected).toEqual(moved);
  expect(s.pieces).toBe(50);
  await edit(page, { op: "hoverCell", cell: [2, 0, -6] });
  await frames(page);
  expect((await state(page)).hover.valid).toBe(true);

  // Undo and redo the move, then the delete tool removes one piece and undo restores it.
  expect((await edit(page, { op: "undo" })).ok).toBe(true);
  await edit(page, { op: "boxSelect", a: [2, 0, -6], b: [6, 0, -6] });
  expect((await state(page)).selected).toEqual(want);
  expect((await edit(page, { op: "redo" })).ok).toBe(true);
  await edit(page, { op: "boxSelect", a: [12, 0, -6], b: [16, 0, -6] });
  expect((await state(page)).selected).toEqual(moved);
  await edit(page, { op: "unhover" });
  await page.keyboard.press("3");
  const [del] = await project(page, box, [[9, 0, -8]]);
  await page.mouse.click(...del.centre);
  await page.waitForFunction(() => window.__parble.editor.pieces === 49, null, { timeout: 5_000 });
  await edit(page, { op: "undo" });
  s = await state(page);
  expect(s.pieces).toBe(50);
  await page.waitForFunction(() => window.__parble.editor.dirtyColliderChunks === 0);
  console.log(`final: ${JSON.stringify({ pieces: s.pieces, edits: s.edits, stats: s.stats })}`);
  expect(errors).toEqual([]);
  expect(same(s.palette, PALETTE)).toBe(true);
});
