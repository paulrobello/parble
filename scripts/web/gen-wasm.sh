#!/usr/bin/env bash
# P6-T2: builds parble-sim-wasm (the generate-and-validate job) for the generator Web Worker and
# stages it into <out-dir> as parble_sim_wasm.js (wasm-bindgen --target no-modules, loaded by the
# classic worker with importScripts) and parble_sim_wasm_bg.wasm. Its own size budget: the module
# loads only when a track is generated, never at startup, so it is not part of the 15 MB startup wasm
# budget (make web-size). Runs from `make ui-build` (Trunk's pre_build hook) after Vite.
# Usage: bash scripts/web/gen-wasm.sh <out-dir>
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
out="$(mkdir -p "$1" && cd "$1" && pwd)"
cd "$root"

target=wasm32-unknown-unknown
profile=wasm-release
bindgen="$(bash scripts/tools/wasm-bindgen.sh)"

cargo build --locked --profile "$profile" -p parble-sim-wasm --target "$target"
"$bindgen" --target no-modules --no-typescript --out-dir "$out" "target/$target/$profile/parble_sim_wasm.wasm"
