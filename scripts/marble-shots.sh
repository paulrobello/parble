#!/usr/bin/env bash
# P1-T6 native screenshot check: every marble type at every quality tier in the `closeup`
# scenario, through the agent switches. Each run must exit 0 with a lit PNG and no logged error.
# The cat's eye shots must also show its green vane in the frame centre (scripts/marble-vane-check.py).
# A capture that never arrives (a cold shader cache can stall the first frames) is retried once.
# Usage: scripts/marble-shots.sh [outdir]   (default target/shots/marbles)
# Runs one at a time; it opens one window per capture.
set -uo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
out="${1:-$root/target/shots/marbles}"
mkdir -p "$out"
cargo build -p parble-app --manifest-path "$root/Cargo.toml" || exit 1
bin="$root/target/debug/parble"
fail=0
for marble in cats-eye glazed steel; do
  for quality in low medium high ultra; do
    png="$out/$marble-$quality.png"
    for attempt in 1 2; do
      rm -f "$png"
      "$bin" --scenario closeup --marble "$marble" --quality "$quality" \
        --screenshot "$png" --screenshot-after 4 --exit-after 5 >"$out/$marble-$quality.log" 2>&1
      code=$?
      grep -q "screenshot never arrived" "$out/$marble-$quality.log" || break
      echo "retry $marble/$quality (attempt $attempt: screenshot never arrived)"
    done
    if [ "$code" -ne 0 ]; then
      echo "FAIL $marble/$quality exit=$code (log: $out/$marble-$quality.log)"
      fail=1
    elif grep -E "ERROR|panicked|wgpu error|Validation Error" "$out/$marble-$quality.log" >/dev/null; then
      echo "FAIL $marble/$quality logged errors (log: $out/$marble-$quality.log)"
      fail=1
    elif [ "$marble" = cats-eye ] && ! python3 "$root/scripts/marble-vane-check.py" "$png"; then
      echo "FAIL $marble/$quality no vane colour in the centre of $png"
      fail=1
    else
      echo "ok   $marble/$quality $png"
    fi
  done
done
exit "$fail"
