// P5-T3 title screen smoke in GPU Chromium, muted: the title shows on title=1 with the key art, runs
// first-run quality detection and stores it, Settings and the marble picker work, Build opens the
// editor with the chosen tier and marble (which a hopper run then releases), and the agent switches
// still go straight to the scene. Screenshots go to target/shots/title/ for review.
import { mkdirSync } from "node:fs";
import { expect, test } from "@playwright/test";

const SHOTS = new URL("../../target/shots/title/", import.meta.url);
mkdirSync(SHOTS, { recursive: true });
const BAD_TEXT = (t) => /%cERROR%c|\bERROR\b/.test(t) || /uncaptured|validation|panicked/i.test(t);

function watchErrors(page) {
  const errors = [];
  page.on("console", (m) => { if (m.type() === "error" || BAD_TEXT(m.text())) errors.push(m.text()); });
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("requestfailed", (r) => errors.push(`request failed: ${r.url()}`));
  return errors;
}

test("title: key art, detection, settings, marble picker, then Build opens the editor with the pick", async ({ page }) => {
  const errors = watchErrors(page);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/?mute=1&title=1");
  const title = page.locator("#pt");
  await expect(title).toBeVisible();
  await expect(page.locator("#pt h1")).toHaveText("parble");
  // The key art decodes (AVIF in Chromium).
  await page.waitForFunction(() => {
    const img = document.querySelector("#pt .art img");
    return img && img.complete && img.naturalWidth > 0 && img.currentSrc.includes("assets/keyart/hero-");
  }, null, { timeout: 15_000 });

  // First-run detection: adapter info and a benchmark, stored.
  await page.evaluate(() => window.__parbleTitle.ready);
  const q = await page.evaluate(() => window.__parbleTitle.state());
  console.log("title detection:", JSON.stringify(q.detection));
  expect(["low", "medium", "high", "ultra"]).toContain(q.settings.quality.detected);
  expect(q.detection.reason).toBe("bench");
  expect(q.detection.benchMs).toBeGreaterThan(0);
  expect(q.detection.adapter).not.toBeNull();
  const stored = await page.evaluate(() => JSON.parse(localStorage.getItem("parble.settings/1")));
  expect(stored.quality.detected).toBe(q.settings.quality.detected);
  // The music player holds the title cue while the title shows.
  await page.waitForFunction(() => window.__parbleAudio?.stats().music.forced === "title", null, { timeout: 30_000 });
  await page.screenshot({ path: new URL("title-1440x900.png", SHOTS).pathname });

  // Settings: the quality override, the mixer and the camera.
  await page.locator('#pt [data-a="settings"]').click();
  await expect(page.locator("#pt-settings")).toBeVisible();
  await page.locator('#pt [data-tier="low"]').click();
  await expect(page.locator('#pt [data-tier="low"]')).toHaveAttribute("aria-checked", "true");
  await page.locator("#pt-mix-music").fill("60");
  await page.waitForTimeout(400);
  await page.screenshot({ path: new URL("settings-1440x900.png", SHOTS).pathname });
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem("parble.mixer/1")).music)).toBeCloseTo(0.6);
  await page.locator('#pt [data-camera="orbit"]').click();
  await page.keyboard.press("Escape");
  await expect(page.locator("#pt-settings")).toBeHidden();

  // Marble picker: every type and size.
  await page.keyboard.press("m");
  await expect(page.locator("#pt-marbles")).toBeVisible();
  await expect(page.locator("#pt [data-type]")).toHaveCount(7);
  await expect(page.locator("#pt [data-size]")).toHaveCount(3);
  await page.waitForFunction(() => [...document.querySelectorAll("#pt .tile img")].every((i) => i.complete && i.naturalWidth > 0));
  await page.locator('#pt [data-type="steel"]').click();
  await page.locator('#pt [data-size="shooter"]').click();
  await page.waitForTimeout(400);
  await page.screenshot({ path: new URL("marbles-1440x900.png", SHOTS).pathname });

  const watchQuery = await page.evaluate(() => window.__parbleTitle.launchQuery("watch"));
  expect(watchQuery).toBe("?scenario=showcase&quality=low&camera=orbit&mute=1");

  // Build: the editor at the chosen tier with the picked marble.
  await page.locator('#pt [data-a="build"]').click();
  await page.waitForURL(/scenario=editor/);
  expect(page.url()).toContain("?scenario=editor&quality=low&marble=steel&marble-size=shooter&mute=1");
  await page.waitForFunction(() => window.__parble?.ready === true, null, { timeout: 60_000 });
  await expect(page.locator("#pt")).toHaveCount(0);
  const s = await page.evaluate(() => ({ marble: window.__parble.marble, size: window.__parble.marbleSize, quality: window.__parble.quality }));
  expect(s).toEqual({ marble: "steel", size: "shooter", quality: "low" });

  // A hopper run releases the picked marble.
  await page.evaluate(() => window.wasmBindings.parbleEdit(JSON.stringify({ op: "place", piece: "mech_hopper", cell: [0, 0, 0], id: 1 })));
  await page.waitForFunction(() => window.__parble.editor?.last?.id === 1, null, { timeout: 10_000 });
  await page.evaluate(() => window.wasmBindings.parbleEdit(JSON.stringify({ op: "run", release: "all", id: 2 })));
  await page.waitForFunction(() => window.__parble.editor?.run?.active === true && window.__parble.marbles.length > 0, null, { timeout: 30_000 });
  expect(errors).toEqual([]);
});

test("title: the key art crop follows the window's aspect", async ({ page }) => {
  const errors = watchErrors(page);
  for (const [w, h, want] of [[1920, 800, "hero-21x9-"], [1600, 900, "hero-16x9-"], [900, 1200, "hero-portrait-9x16-"]]) {
    await page.setViewportSize({ width: w, height: h });
    await page.goto("/?mute=1&title=1");
    await page.waitForFunction((want) => {
      const img = document.querySelector("#pt .art img");
      return img && img.complete && img.naturalWidth > 0 && img.currentSrc.includes(want);
    }, want, { timeout: 15_000 });
    await page.screenshot({ path: new URL(`title-${w}x${h}.png`, SHOTS).pathname });
  }
  expect(errors).toEqual([]);
});

test("title: agent switches and title=0 go straight to the scene", async ({ page }) => {
  const errors = watchErrors(page);
  for (const q of ["/?mute=1", "/?mute=1&title=0", "/?mute=1&scenario=ramp&quality=low"]) {
    await page.goto(q);
    await page.waitForFunction(() => window.__parble?.ready === true, null, { timeout: 60_000 });
    expect(await page.locator("#pt").count(), q).toBe(0);
    expect(await page.evaluate(() => window.__parbleTitle ?? null), q).toBeNull();
  }
  // The head carries the favicon and the link-preview tags.
  expect(await page.locator('link[rel="icon"][type="image/svg+xml"]').getAttribute("href")).toContain("web/favicon/favicon.svg");
  expect(await page.locator('meta[property="og:image"]').getAttribute("content")).toBe(
    "https://parble.pardev.net/assets/keyart/social-card-1200x630.png",
  );
  for (const href of ["favicon.ico", "web/favicon/favicon.svg", "web/favicon/apple-touch-icon.png", "assets/keyart/social-card-1200x630.png"]) {
    const r = await page.request.get(`/${href}`);
    expect(r.status(), href).toBe(200);
  }
  expect(errors).toEqual([]);
});

test("unsupported page: static, styled, key art and favicon load", async ({ page }) => {
  const errors = watchErrors(page);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/web/unsupported.html");
  await expect(page.locator("h1")).toHaveText("This browser can't draw parble");
  await page.waitForFunction(() => {
    const img = document.querySelector(".art img");
    return img && img.complete && img.naturalWidth > 0;
  });
  await page.evaluate(() => document.fonts.ready);
  expect(await page.evaluate(() => document.fonts.check('16px "Alegreya Sans"'))).toBe(true);
  await page.screenshot({ path: new URL("unsupported-1440x900.png", SHOTS).pathname });
  expect(errors).toEqual([]);
});

test("title: Back from the showcase shows the title again, and the showcase's Title button opens it", async ({ page }) => {
  const errors = watchErrors(page);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/?mute=1&title=1");
  await expect(page.locator("#pt")).toBeVisible();
  await page.locator('#pt [data-a="watch"]').click();
  await page.waitForURL(/scenario=showcase/);
  await page.waitForFunction(() => window.__parble?.ready === true, null, { timeout: 60_000 });
  await expect(page.locator("#pt")).toHaveCount(0);
  const home = page.locator("#parble-home");
  await expect(home).toHaveText("Title");

  // Back: the title, faded fully in, whether the page comes from the back/forward cache or reloads.
  await page.goBack();
  await expect(page.locator("#pt")).toBeVisible();
  await expect(page.locator("#pt")).not.toHaveClass(/leaving/);
  await expect(page.locator("#pt")).toHaveCSS("opacity", "1");
  await expect(page.locator("#pt h1")).toHaveText("parble");
  await page.screenshot({ path: new URL("title-back.png", SHOTS).pathname });

  // Forward to the showcase, then its Title button.
  await page.goForward();
  await page.waitForURL(/scenario=showcase/);
  await page.waitForFunction(() => window.__parble?.ready === true, null, { timeout: 60_000 });
  await page.locator("#parble-home").click();
  await page.waitForURL(/\?title=1&mute=1$/);
  await expect(page.locator("#pt")).toBeVisible();
  await expect(page.locator("#pt")).toHaveCSS("opacity", "1");
  await expect(page.locator("#parble-home")).toHaveCount(0);
  expect(errors).toEqual([]);
});
