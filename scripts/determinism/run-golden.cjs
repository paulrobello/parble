// Usage: node run-golden.cjs <wasm-bindgen nodejs module> <golden hash file> [<variants golden file>]
// Runs run_golden() from the wasm build and compares it with the committed native golden. With a
// variants file (P5-T1), also runs run_variants() (every marble type × size on the ramp) and
// compares it line by line.
"use strict";

const fs = require("node:fs");
const path = require("node:path");

const [modulePath, goldenPath, variantsPath] = process.argv.slice(2);
if (!modulePath || !goldenPath) {
  console.error("usage: node run-golden.cjs <module.js> <golden.hash> [<variants.hash>]");
  process.exit(2);
}

const { run_golden, run_variants } = require(path.resolve(modulePath));
const expected = fs.readFileSync(goldenPath, "utf8").trim();
const actual = run_golden();

console.log(`golden (native) ${expected}`);
console.log(`wasm            ${actual}`);
if (actual !== expected) {
  console.error(`FAIL: wasm hash ${actual} != golden ${expected} (${goldenPath})`);
  process.exit(1);
}
console.log("determinism-wasm: wasm matches native golden");

if (variantsPath) {
  const want = fs.readFileSync(variantsPath, "utf8").trim().split("\n");
  const got = run_variants().trim().split("\n");
  let bad = 0;
  for (let i = 0; i < Math.max(want.length, got.length); i++) {
    if (want[i] !== got[i]) {
      console.error(`FAIL: native ${want[i] ?? "(none)"} != wasm ${got[i] ?? "(none)"}`);
      bad++;
    }
  }
  if (bad) process.exit(1);
  console.log(`determinism-wasm: ${got.length} marble type × size runs match native (${variantsPath})`);
}
