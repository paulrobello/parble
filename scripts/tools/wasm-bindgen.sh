#!/usr/bin/env bash
# Prints the path of a wasm-bindgen CLI matching the wasm-bindgen crate in Cargo.lock, installing it
# into target/tools first if needed. The CLI must match the crate exactly, or the bindgen step rejects
# the module's schema version. Used by scripts/determinism/wasm.sh and scripts/web/gen-wasm.sh.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

tools=target/tools
want=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/"/, "", $3); print $3; exit }' Cargo.lock)
test -n "$want" || { echo "ERROR: wasm-bindgen not found in Cargo.lock" >&2; exit 2; }
bindgen="$tools/bin/wasm-bindgen"
have=$("$bindgen" --version 2>/dev/null | awk '{ print $2 }' || true)
if [ "$have" != "$want" ]; then
  echo "installing wasm-bindgen-cli $want into $tools (found: ${have:-none})" >&2
  cargo install --locked --root "$tools" wasm-bindgen-cli --version "$want" >&2
fi
echo "$root/$bindgen"
