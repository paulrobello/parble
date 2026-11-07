// Usage: node run-golden.cjs <wasm-bindgen nodejs module> <golden hash file>
// Runs run_golden() from the wasm build and compares it with the committed native golden.
"use strict";

const fs = require("node:fs");
const path = require("node:path");

const [modulePath, goldenPath] = process.argv.slice(2);
if (!modulePath || !goldenPath) {
  console.error("usage: node run-golden.cjs <module.js> <golden.hash>");
  process.exit(2);
}

const { run_golden } = require(path.resolve(modulePath));
const expected = fs.readFileSync(goldenPath, "utf8").trim();
const actual = run_golden();

console.log(`golden (native) ${expected}`);
console.log(`wasm            ${actual}`);
if (actual !== expected) {
  console.error(`FAIL: wasm hash ${actual} != golden ${expected} (${goldenPath})`);
  process.exit(1);
}
console.log("determinism-wasm: wasm matches native golden");
