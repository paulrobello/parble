// P3-T5 editor overlay smoke in GPU Chromium at the 1440×900 reference layout: load the editor, place
// a piece through the drawer and a click, select, slice, show a validation issue, run, and confirm the
// run fade and the edge-hover return, with no console errors. It also saves the five spec states
// (docs/design/editor/README.md, Frames) as docs/design/editor/built-*.png.
import { mkdirSync } from "node:fs";
import { expect, test } from "@playwright/test";

const BAD_TEXT = (t) => /%cERROR%c|\bERROR\b/.test(t) || /uncaptured|invalid|validation error|panicked/i.test(t);
const SHOTS = new URL("../../docs/design/editor/", import.meta.url);

test.use({ viewport: { width: 1440, height: 900 } });

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
async function cellPx(page, cell) {
  const box = await page.locator("canvas#parble").boundingBox();
  const last = await edit(page, { op: "project", cells: [cell] });
  const p = last.projected[0];
  return { centre: [box.x + p[0], box.y + p[1]], floor: [box.x + p[2], box.y + p[3]] };
}
async function shot(page, name) {
  mkdirSync(SHOTS, { recursive: true });
  await frames(page, 4);
  await page.screenshot({ path: new URL(`built-${name}.png`, SHOTS).pathname });
}

test("editor overlay: drawer place, select, slice, show an issue, run, fade and return", async ({ page }) => {
  test.setTimeout(180_000);
  const errors = [];
  page.on("console", (m) => {
    if (m.type() === "error" || BAD_TEXT(m.text())) errors.push(m.text());
  });
  page.on("pageerror", (e) => errors.push(String(e)));
  const fontRequests = [];
  page.on("request", (r) => {
    if (/fonts\.(googleapis|gstatic)\.com/.test(r.url())) fontRequests.push(r.url());
  });
  await page.goto("/?mute=1&scenario=editor&quality=low");
  await page.waitForFunction(() => window.__parble?.ready === true && window.__parble.editor && window.__parble.frame > 10, null, {
    timeout: 60_000,
  });
  await expect(page.locator("#pe-top")).toBeVisible();
  // The overlay owns the keys, and the first run opens on Mechanisms with the Hopper picked.
  await page.waitForFunction(() => window.__parble.editor.keys === false && window.__parble.editor.brush.piece === "mech_hopper");
  await expect(page.locator('[data-f="mechanisms"]')).toHaveAttribute("aria-selected", "true");
  await expect(page.locator('[data-k="sel"]')).toContainText("Start here");
  await expect(page.locator('[data-k="run"]')).toBeDisabled();
  expect(await page.evaluate(() => document.fonts.check('500 13px "Alegreya Sans"'))).toBe(true);

  await edit(page, { op: "camera", target: [0.36, 0.03, -0.36], yaw: -0.5, pitch: 0.85, radius: 1.25 });
  await frames(page, 3);
  await page.mouse.move(720, 470);
  await shot(page, "first-run");

  // Place the hopper through the drawer tile and a click on the canvas.
  await page.locator('[data-piece="mech_hopper"]').click();
  const hopperCell = [4, 0, -6];
  const hp = await cellPx(page, hopperCell);
  expect(await page.evaluate(([x, y]) => document.elementFromPoint(x, y)?.id, hp.floor)).toBe("parble");
  await page.mouse.move(...hp.floor);
  await page.waitForFunction((c) => JSON.stringify(window.__parble.editor.hover?.target) === JSON.stringify(c), hopperCell);
  await expect(page.locator("#pe-tip")).toContainText("[4, 0, −6] · L0 · valid");
  await page.mouse.click(...hp.floor);
  await page.waitForFunction(() => window.__parble.editor.pieces === 1 && window.__parble.editor.hoppers === 1);
  await expect(page.locator('[data-k="run"]')).toBeEnabled();

  // A short track with a few levels (scripted), so there are issues, a slice and a group to select.
  const placed = [
    ["blk_straight_l3", [5, 0, -6], 1],
    ["blk_straight_l3", [6, 0, -6], 1],
    ["mech_splitter_l3", [7, 0, -6], 1],
    ["blk_curve90_l3", [8, 0, -6], 0],
    ["blk_straight_l3", [5, 0, -8], 1],
    ["blk_straight_l3", [6, 0, -8], 1],
    ["blk_slope_gentle_l3", [7, 0, -8], 1],
    ["blk_pillar", [9, 0, -8], 0],
    ["blk_pillar", [9, 1, -8], 0],
    ["blk_straight_l3", [9, 2, -8], 1],
    ["blk_pillar", [3, 0, -8], 0],
    ["blk_pillar", [3, 1, -8], 0],
    ["blk_end_l3", [3, 2, -8], 0],
    ["rail_straight_l2", [5, 0, -10], 1],
    ["rail_straight_l2", [6, 0, -10], 1],
  ];
  for (const [piece, cell, yaw] of placed) await edit(page, { op: "place", piece, cell, yaw });
  let s = await state(page);
  expect(s.pieces).toBeGreaterThanOrEqual(12);
  expect(s.issueCount).toBeGreaterThan(0);
  await expect(page.locator(".iss").first()).toBeVisible();

  // Select with the strip's Select tool and a click on the splitter.
  await page.locator('[data-tool="select"]').click();
  await page.waitForFunction(() => window.__parble.editor.tool === "select");
  const sp = await cellPx(page, [7, 0, -6]);
  await page.mouse.click(...sp.centre);
  await page.waitForFunction(() => window.__parble.editor.selection === 1);
  await expect(page.locator('[data-k="sel"]')).toContainText("Selected:");
  await expect(page.locator('[data-k="sel"]')).toContainText("Ports");

  // Spec frame 1: Place tool, ghost on a valid cell, a piece selected, issues listed.
  await page.locator('[data-f="blocks"]').click();
  await page.locator('[data-piece="blk_straight_l3"]').click();
  await page.waitForFunction(() => window.__parble.editor.tool === "place");
  const ghostCell = [6, 0, -4];
  const gp = await cellPx(page, ghostCell);
  await page.mouse.move(...gp.floor);
  await page.waitForFunction((c) => JSON.stringify(window.__parble.editor.hover?.target) === JSON.stringify(c), ghostCell);
  await expect(page.locator("#pe-tip")).toBeVisible();
  await shot(page, "building");

  // Slice with the stepper, then a 14-piece group selection under the slice (spec frame 3).
  await page.keyboard.press("Escape");
  await page.waitForFunction(() => window.__parble.editor.tool === "select");
  expect((await state(page)).maxLevel).toBe(2);
  await page.locator('[data-k="down"]').click();
  await page.locator('[data-k="down"]').click();
  await page.waitForFunction(() => window.__parble.editor.slice === 1);
  await expect(page.locator('[data-k="sliceToggle"]')).toHaveText("≤ L1");
  await edit(page, { op: "boxSelect", a: [0, 0, -12], b: [12, 1, 0] });
  s = await state(page);
  expect(s.selection).toBeGreaterThan(1);
  await expect(page.locator('[data-k="sel"]')).toContainText("pieces");
  await page.mouse.move(720, 450);
  await shot(page, "multiselect-slice");
  await page.locator('[data-k="sliceToggle"]').click();
  await page.waitForFunction(() => window.__parble.editor.slice === null);

  // Show on the first issue: the camera flies there and selects that piece.
  const before = await page.evaluate(() => window.__parble.camera.orbit);
  const issue = (await state(page)).issues.find((i) => i.anchor);
  await page.locator("[data-show]").first().click();
  await page.waitForFunction(
    (a) => window.__parble.editor.selection === 1 && JSON.stringify(window.__parble.editor.selected[0]) === JSON.stringify(a),
    issue.anchor,
    { timeout: 5_000 },
  );
  const after = await page.evaluate(() => window.__parble.camera.orbit);
  expect(after.target).not.toEqual(before.target);
  await edit(page, { op: "camera", target: [0.36, 0.03, -0.42], yaw: -0.5, pitch: 0.85, radius: 1.25 });

  // Run: marbles release, the chrome fades after 1.2 s still, the top edge brings the strip back.
  await page.locator('[data-k="run"]').click();
  await page.waitForFunction(() => window.__parble.editor.run.active === true, null, { timeout: 15_000 });
  await expect(page.locator('[data-k="run"]')).toContainText("Stop");
  await page.waitForFunction(() => window.__parble.editor.run.marbles > 0, null, { timeout: 10_000 });
  await page.mouse.move(700, 450);
  await page.waitForFunction(() => document.querySelector("#pe")?.classList.contains("faded"), null, { timeout: 5_000 });
  await page.waitForFunction(() => getComputedStyle(document.querySelector("#pe-top")).opacity === "0");
  await expect(page.locator("#pe-cap")).toContainText("Running");
  await shot(page, "run-faded");
  await page.mouse.move(700, 20, { steps: 4 });
  await page.waitForFunction(() => document.querySelector("#pe")?.classList.contains("show-top"));
  await page.waitForFunction(() => getComputedStyle(document.querySelector("#pe-top")).opacity === "1");
  await shot(page, "run-edge-hover");
  // Editing stays live mid-run, and Stop ends the run and brings everything back.
  await page.locator('[data-k="pause"]').click();
  await page.waitForFunction(() => window.__parble.editor.run.paused === true);
  await page.locator('[data-k="run"]').click();
  await page.waitForFunction(() => window.__parble.editor.run.active === false);
  await page.waitForFunction(() => !document.querySelector("#pe")?.classList.contains("faded"));

  expect(fontRequests).toEqual([]);
  expect(errors).toEqual([]);
});
