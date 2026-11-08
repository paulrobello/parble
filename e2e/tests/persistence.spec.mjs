// P3-T3 local done-gate: export → import → run reproduces the trajectory hash. The fixture track is
// imported through the file picker and run over the bridge, exported as a download, the downloaded
// bytes are imported again and run again. Both browser hashes must equal each other and the runHash
// that crates/parble-app/src/persist.rs checks natively, and the second export must be byte-identical
// to the first (only kit-backed placements reach the sim, mechanisms and metadata do not, so the hash
// alone cannot catch a lossy round trip).
import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";

const FIXTURES = JSON.parse(readFileSync(new URL("../../web/ui/src/parble-fixtures.json", import.meta.url), "utf8"));
const DEMO = FIXTURES.valid.find((c) => c.name === "demo");

const panel = (page) => page.locator("#parble-files");

async function importBytes(page, name, buffer) {
  await page.locator("#parble-import").setInputFiles({ name, mimeType: "application/json", buffer });
}

async function run(page) {
  await page.locator("#parble-run").click();
  await expect(panel(page)).toHaveAttribute("data-run-hash", /^[0-9a-f]{16}$/, { timeout: 30_000 });
  const p = panel(page);
  return { hash: await p.getAttribute("data-run-hash"), ticks: Number(await p.getAttribute("data-run-ticks")) };
}

async function exportBytes(page) {
  const [download] = await Promise.all([page.waitForEvent("download"), page.locator("#parble-export").click()]);
  const chunks = [];
  for await (const c of await download.createReadStream()) chunks.push(c);
  return { name: download.suggestedFilename(), bytes: Buffer.concat(chunks) };
}

test("export, re-import and run reproduce the native trajectory hash", async ({ page }) => {
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => {
    if (m.type() === "error") errors.push(m.text());
  });
  await page.goto("/");
  await page.waitForFunction(() => window.__parble?.ready === true, null, { timeout: 60_000 });

  await importBytes(page, "demo.parble", Buffer.from(JSON.stringify(DEMO.file)));
  await expect(panel(page)).toHaveAttribute("data-track", DEMO.file.metadata.name);
  const first = await run(page);

  const exported = await exportBytes(page);
  expect(exported.name).toBe("round-trip-demo.parble");

  // A different track in between, so the second run cannot reuse the first load.
  await importBytes(page, "empty.parble", Buffer.from(JSON.stringify(FIXTURES.valid[1].file)));
  await expect(panel(page)).toHaveAttribute("data-track", "Empty");
  await expect(panel(page)).toHaveAttribute("data-run-hash", "");

  await importBytes(page, exported.name, exported.bytes);
  await expect(panel(page)).toHaveAttribute("data-track", DEMO.file.metadata.name);
  const second = await run(page);
  const reexported = await exportBytes(page);

  const diag = await page.evaluate(() => window.__parble);
  console.log(
    `first run ${first.hash} (${first.ticks} ticks), second run ${second.hash} (${second.ticks} ticks), ` +
      `native ${DEMO.runHash}, __parble.trackRun ${JSON.stringify(diag.trackRun)}, export ${exported.bytes.length} bytes`,
  );
  expect(first.ticks).toBe(FIXTURES.runTicks);
  expect(second).toEqual(first);
  expect(first.hash).toBe(DEMO.runHash);
  expect(diag.track).toBe(DEMO.file.metadata.name);
  expect(diag.trackRun).toEqual({ ticks: FIXTURES.runTicks, hash: DEMO.runHash });
  expect(reexported.bytes.equals(exported.bytes)).toBe(true);
  expect(JSON.parse(exported.bytes.toString("utf8"))).toEqual(DEMO.file);
  expect(await panel(page).getAttribute("data-error")).toBe("");
  expect(errors).toEqual([]);
});
