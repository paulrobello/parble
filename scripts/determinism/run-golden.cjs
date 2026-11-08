// Usage: node run-golden.cjs <wasm-bindgen nodejs module> <golden hash file> [<variants golden file>
//   [<mechanism golden file> [<playroom hash file>]]]
// Runs run_golden() from the wasm build and compares it with the committed native golden. With a
// variants file (P5-T1), also runs run_variants() (every marble type × size on the ramp) and
// compares it line by line. With a mechanism file (P2-T1), also runs run_mechanism_demo(). With a
// playroom file, also runs run_playroom() (the playroom kit run on chunk colliders).
"use strict";

const fs = require("node:fs");
const path = require("node:path");

const [modulePath, goldenPath, variantsPath, mechanismPath, playroomPath] = process.argv.slice(2);
if (!modulePath || !goldenPath) {
  console.error("usage: node run-golden.cjs <module.js> <golden.hash> [<variants.hash> [<mechanism.hash> [<playroom.hash>]]]");
  process.exit(2);
}

const { run_golden, run_variants, run_mechanism_demo, run_playroom } = require(path.resolve(modulePath));
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

if (mechanismPath) {
  const want = fs.readFileSync(mechanismPath, "utf8").trim();
  const got = run_mechanism_demo().trim();
  if (want !== got) {
    console.error(`FAIL: mechanism fixture run\n native:\n${want}\n wasm:\n${got}`);
    process.exit(1);
  }
  console.log(`determinism-wasm: mechanism fixture run matches native (${mechanismPath})`);
}

if (playroomPath) {
  const want = fs.readFileSync(playroomPath, "utf8").trim();
  const got = run_playroom();
  console.log(`playroom (native) ${want}`);
  console.log(`playroom (wasm)   ${got}`);
  if (got !== want) {
    console.error(`FAIL: wasm playroom hash ${got} != native ${want} (${playroomPath})`);
    process.exit(1);
  }
  console.log("determinism-wasm: playroom kit run matches native");
}
