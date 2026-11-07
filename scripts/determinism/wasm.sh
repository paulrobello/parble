#!/usr/bin/env bash
# P0-T4 wasm determinism parity: build parble-sim-wasm (release, wasm32-unknown-unknown), bind it with
# wasm-bindgen --target nodejs, run run_golden() under Node and compare with the committed golden.
# Exits non-zero on mismatch, printing both hashes.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

target=wasm32-unknown-unknown
golden=crates/parble-sim/tests/golden/ramp_10s.hash
out=target/determinism-wasm
tools=target/tools

# wasm-bindgen-cli must match the wasm-bindgen crate in Cargo.lock exactly, or the bindgen step
# rejects the module's schema version.
want=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/"/, "", $3); print $3; exit }' Cargo.lock)
test -n "$want" || { echo "ERROR: wasm-bindgen not found in Cargo.lock" >&2; exit 2; }
bindgen="$tools/bin/wasm-bindgen"
have=$("$bindgen" --version 2>/dev/null | awk '{ print $2 }' || true)
if [ "$have" != "$want" ]; then
  echo "installing wasm-bindgen-cli $want into $tools (found: ${have:-none})"
  cargo install --locked --root "$tools" wasm-bindgen-cli --version "$want"
fi

cargo build --locked --release -p parble-sim-wasm --target "$target"

rm -rf "$out"
"$bindgen" --target nodejs --out-dir "$out" "target/$target/release/parble_sim_wasm.wasm"

node scripts/determinism/run-golden.cjs "$out/parble_sim_wasm.js" "$golden"
