// New-version notice in GPU Chromium, muted: the page polls version.json (web/ui/src/version-check.ts).
// While the served id matches the page's build it stays quiet; once the route answers a different id
// the notice shows over the scene with Reload, and Reload reloads the page. Both the title screen and
// a scene show it. Screenshot: target/shots/version/notice.png.
import { mkdirSync } from "node:fs";
import { expect, test } from "@playwright/test";

const SHOTS = new URL("../../target/shots/version/", import.meta.url);
mkdirSync(SHOTS, { recursive: true });
const BAD_TEXT = (t) => /%cERROR%c|\bERROR\b/.test(t) || /uncaptured|validation|panicked/i.test(t);

const outcome = (page) =>
  page.evaluate(
    () => new Promise((r) => addEventListener("parbleversioncheck", (e) => r(e.detail), { once: true })),
  );

for (const [name, url] of [
  ["scene", "/?mute=1&scenario=showcase&version-poll=1"],
  ["title", "/?mute=1&title=1&version-poll=1"],
]) {
  test(`version notice: quiet on the same build, then Reload on a newer one (${name})`, async ({ page }) => {
    const errors = [];
    page.on("console", (m) => {
      if (m.type() === "error" || BAD_TEXT(m.text())) errors.push(m.text());
    });
    page.on("pageerror", (e) => errors.push(String(e)));
    let live = null;
    await page.route(/\/version\.json\?t=\d+$/, async (route) => {
      if (live === null) return route.continue();
      await route.fulfill({ json: { id: live } });
    });
    await page.goto(url);
    const build = await page.evaluate(() => window.__PARBLE_BUILD);
    expect(build).toMatch(/^[0-9a-f]{10}$/);

    expect(await outcome(page)).toBe("same");
    await expect(page.locator("#parble-update")).toHaveCount(0);

    live = "0123456789";
    expect(await outcome(page)).toBe("newer");
    const note = page.locator("#parble-update");
    await expect(note).toBeVisible();
    // Faded fully in, and on top: the element at its centre is the notice (over the title screen too).
    await expect(note).toHaveCSS("opacity", "1");
    const box = await note.boundingBox();
    expect(
      await page.evaluate(
        ([x, y]) => document.elementFromPoint(x, y)?.closest("#parble-update") !== null,
        [box.x + box.width / 2, box.y + box.height / 2],
      ),
    ).toBe(true);
    await expect(note).toContainText("New version available");
    await expect(note.locator("button.go")).toHaveText("Reload");
    await page.screenshot({ path: new URL(`notice-${name}.png`, SHOTS).pathname });

    await Promise.all([page.waitForEvent("load"), note.locator("button.go").click()]);
    await expect(page.locator("#parble-update")).toHaveCount(0);
    expect(errors).toEqual([]);
  });
}
