// csp-check.mjs: passes a page whose CSP lists every inline script's hash, and fails one whose inline
// script changed after hashing (hex-world's 2026-10-20 break: a swapped build id in the hash map).
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { test } from "node:test";
import { assertCsp, cspMissing } from "./csp-check.mjs";
import { hashInlineScripts } from "./version-dist.mjs";

const sha = (body) => `'sha256-${createHash("sha256").update(body).digest("base64")}'`;
const MAP = 'window.__PARBLE_HASHES={"a.glb":"0123456789"};window.__PARBLE_BUILD="aaaaaaaaaa";';
const GATE = "\n      if (!navigator.gpu) location.replace('web/unsupported.html');\n    ";
const page = (sources) =>
  "<!doctype html><html><head>\n" +
  `<meta\n  http-equiv="Content-Security-Policy"\n  content="default-src 'self'; script-src 'self' ${sources} 'wasm-unsafe-eval'; object-src 'none'"\n/>\n` +
  `<script id="parble-hashes">${MAP}</script>\n<script src="web/shim.js"></script>\n<script>${GATE}</script>\n` +
  "</head></html>\n";

test("a page whose script-src lists every inline script's hash passes", () => {
  const html = page(`${sha(MAP)} ${sha(GATE)}`);
  assert.deepEqual(cspMissing(html), []);
  assert.doesNotThrow(() => assertCsp(html));
});

test("an inline script changed after hashing fails, naming the script", () => {
  const html = page(`${sha(MAP)} ${sha(GATE)}`).replace('__PARBLE_BUILD="aaaaaaaaaa"', '__PARBLE_BUILD="bbbbbbbbbb"');
  const missing = cspMissing(html);
  assert.equal(missing.length, 1);
  assert.match(missing[0].head, /^window\.__PARBLE_HASHES=/);
  assert.throws(() => assertCsp(html), /1 inline script\(s\) in index\.html not in script-src/);
  // Rehashing the changed page (what the version e2e does for its simulated deploy) makes it pass.
  assert.deepEqual(cspMissing(hashInlineScripts(html)), []);
});

test("a missing policy, a missing script-src or 'unsafe-inline' fails", () => {
  assert.throws(() => cspMissing("<script>x</script>"), /no Content-Security-Policy/);
  assert.throws(
    () => cspMissing(`<meta http-equiv="Content-Security-Policy" content="default-src 'self'" /><script>x</script>`),
    /no script-src/,
  );
  assert.throws(() => cspMissing(page(`'unsafe-inline'`)), /unsafe-inline/);
});
