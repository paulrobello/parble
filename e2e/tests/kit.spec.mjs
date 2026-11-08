// P1-T4 instanced chunked kit renderer in GPU Chromium: the 2,000-piece `kit2000` grid draws within
// one draw per (type × LOD × chunk), as one draw per (type, LOD, wood variant), every LOD is in frame
// with crossfading ranges, the three wood variants load in the device's texture family (BC7, else
// ASTC, else PNG) and every piece's baked AO and normal PNGs load, with no console errors or WebGPU
// validation messages. `?tex=astc` and `?tex=png` cap the family to exercise the fallbacks. Shots go
// to target/shots/web-kit/.
import { mkdirSync } from "node:fs";
import { expect, test } from "@playwright/test";

const SHOTS = new URL("../../target/shots/web-kit/", import.meta.url);
const BAD_TEXT = (t) => /%cERROR%c|\bERROR\b/.test(t) || /uncaptured|invalid|validation|panicked/i.test(t);

function watchErrors(page) {
  const errors = [];
  page.on("console", (m) => { if (m.type() === "error" || BAD_TEXT(m.text())) errors.push(m.text()); });
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("requestfailed", (r) => errors.push(`request failed: ${r.url()}`));
  return errors;
}

const FORMATS = { bc7: /^Bc7RgbaUnorm/, astc: /^Astc/, png: /^Rgba8Unorm/ };

// family.rs `select_family_capped`: BC7 if the device has BC, else ASTC, else PNG; a cap only steps down.
function expectedFamily(bc, astc, cap) {
  if (cap === "png") return "png";
  if (cap === "astc") return astc ? "astc" : "png";
  return bc ? "bc7" : astc ? "astc" : "png";
}

mkdirSync(SHOTS, { recursive: true });

for (const tex of [null, "astc", "png"]) {
  test(`kit2000 ${tex ?? "auto"} family draws within the bound`, async ({ page }) => {
    const errors = watchErrors(page);
    const assetUrls = [];
    page.on("request", (r) => {
      const u = new URL(r.url());
      if (u.pathname.startsWith("/assets/")) assetUrls.push(u);
    });
    await page.goto(`/?scenario=kit2000&quality=high${tex ? `&tex=${tex}` : ""}`);
    await page.waitForFunction(() => window.__parble?.ready === true, null, { timeout: 60_000 });
    // Draw counts lag the first frames, and the textures load asynchronously.
    await page.waitForFunction(
      () => {
        const k = window.__parble?.kit;
        return k && k.draws.main > 0 && k.textures.length > 0 && k.textures.every((t) => t.state !== "loading" && t.state !== "notLoaded");
      },
      null,
      { timeout: 60_000 },
    );
    await page.waitForTimeout(2_000);
    const k = await page.evaluate(() => window.__parble.kit);
    console.log(`kit ${tex ?? "auto"}: ${JSON.stringify({ family: k.family, deviceBc: k.deviceBc, deviceAstc: k.deviceAstc, textures: k.textures, draws: k.draws, drawBound: k.drawBound, chunks: k.chunks, pieces: k.pieces })}`);
    // Every kit asset the wasm fetched carries the version-dist content hash of its file
    // (scripts/web/version-dist.mjs, inlined as window.__PARBLE_HASHES), so a cache never serves stale
    // bytes for a changed texture.
    // A texture that failed to load drops its (type, LOD) out of the draw count; report why first.
    expect(k.textures.filter((t) => t.state !== "loaded"), errors.join("\n")).toEqual([]);
    const hashes = await page.evaluate(() => window.__PARBLE_HASHES);
    expect(assetUrls.length).toBeGreaterThanOrEqual(k.textures.length);
    const unversioned = assetUrls
      .filter((u) => u.searchParams.get("v") !== hashes[u.pathname.slice(1)])
      .map((u) => `${u.pathname}${u.search} (want v=${hashes[u.pathname.slice(1)]})`);
    expect(unversioned).toEqual([]);
    expect(k.pieces).toBe(2000);
    // One flat 50 × 40 grid at a 1.5-cell pitch: 10 × 8 chunks, 317 occupied (type, chunk) pairs.
    expect(k.chunks).toBe(80);
    expect(k.drawBound).toBe(317 * 3);
    expect(k.draws.mainViews).toBe(1);
    expect(k.draws.main).toBeGreaterThan(0);
    expect(k.draws.main).toBeLessThanOrEqual(k.drawBound);
    // All three LODs of all four types in all three wood variants are in frame, each as one
    // instanced draw.
    expect(k.variants).toBe(3);
    expect(k.variantPieces.every((n) => n > 0)).toBe(true);
    expect(k.draws.main).toBe(k.pieceTypes * 3 * k.variants);
    // More LOD instances than pieces: the pieces inside a fade band draw both LODs, dithered.
    expect(k.draws.mainInstances).toBeGreaterThan(k.pieces);
    expect(k.draws.ditheredInstances).toBe(k.draws.mainInstances);
    expect(typeof k.deviceBc).toBe("boolean");
    expect(typeof k.deviceAstc).toBe("boolean");
    expect(k.family).toBe(expectedFamily(k.deviceBc, k.deviceAstc, tex));
    // Wood: base_color and orm of each variant. Baked: AO and normal per piece type and LOD.
    expect(k.textures.filter((t) => t.kind === "wood")).toHaveLength(k.variants * 2);
    expect(k.textures.filter((t) => t.kind === "baked")).toHaveLength(k.pieceTypes * 3 * 2);
    for (const t of k.textures) {
      expect(t.state).toBe("loaded");
      if (t.kind === "wood") {
        expect(t.path).toContain(`.${k.family === "png" ? "png" : `${k.family}.ktx2`}`);
        expect(t.format).toMatch(FORMATS[k.family]);
      } else {
        expect(t.path).toMatch(/_lod[0-2]_(ao|normal)\.png$/);
        expect(t.format).toMatch(FORMATS.png);
      }
    }
    await page.locator("canvas#parble").screenshot({ path: new URL(`kit2000-${tex ?? "auto"}.png`, SHOTS).pathname });
    expect(errors).toEqual([]);
  });
}
