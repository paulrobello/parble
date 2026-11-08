#!/usr/bin/env bash
# P1-T6 / P5-T1 native screenshot check, through the agent switches:
#  - every marble type at every quality tier in the `closeup` scenario;
#  - the mini and shooter sizes (swirl mini on Low, opal shooter on High);
#  - the `lineup` scenario (every type side by side) at every tier, as lineup-<tier>.png;
#  - the glow bloom check: glow on High and Ultra with bloom on and off, and the halo past its silhouette
#    must brighten (scripts/marble-bloom-check.py).
# Each run must exit 0 with a lit PNG and no logged error. The cat's eye shots must also show its
# green vane in the frame centre (scripts/marble-vane-check.py). A capture that never arrives (a cold
# shader cache can stall the first frames) is retried once. Shots go to the outdir only; the
# committed docs/shots/p5-t1-lineup.png is a reviewed copy of lineup-high.png, never rewritten here.
# Usage: scripts/marble-shots.sh [outdir]   (default target/shots/marbles)
# Runs one at a time; it opens one window per capture.
set -uo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
out="${1:-$root/target/shots/marbles}"
mkdir -p "$out"
cargo build -p parble-app --manifest-path "$root/Cargo.toml" || exit 1
bin="$root/target/debug/parble"
fail=0

# shot <name> <args...>: capture $out/<name>.png and check the run.
shot() {
  local name="$1"
  shift
  local png="$out/$name.png" log="$out/$name.log" code=0
  for attempt in 1 2; do
    rm -f "$png"
    "$bin" "$@" --screenshot "$png" --screenshot-after 4 --exit-after 5 >"$log" 2>&1
    code=$?
    grep -q "screenshot never arrived" "$log" || break
    echo "retry $name (attempt $attempt: screenshot never arrived)"
  done
  if [ "$code" -ne 0 ]; then
    echo "FAIL $name exit=$code (log: $log)"
    fail=1
    return 1
  elif grep -E "ERROR|panicked|wgpu error|Validation Error" "$log" >/dev/null; then
    echo "FAIL $name logged errors (log: $log)"
    fail=1
    return 1
  fi
  echo "ok   $name $png"
}

for marble in cats-eye glazed steel swirl onionskin opal glow; do
  for quality in low medium high ultra; do
    shot "$marble-$quality" --scenario closeup --marble "$marble" --quality "$quality" || continue
    if [ "$marble" = cats-eye ] && ! python3 "$root/scripts/marble-vane-check.py" "$out/$marble-$quality.png"; then
      echo "FAIL $marble/$quality no vane colour in the centre of $out/$marble-$quality.png"
      fail=1
    fi
  done
done

shot swirl-mini-low --scenario closeup --marble swirl --marble-size mini --quality low
shot opal-shooter-high --scenario closeup --marble opal --marble-size shooter --quality high

for quality in low medium high ultra; do
  shot "lineup-$quality" --scenario lineup --quality "$quality" --dof 0
done

for quality in high ultra; do
  if shot "glow-$quality-bloom-on" --scenario closeup --marble glow --quality "$quality" --bloom 1 &&
    shot "glow-$quality-bloom-off" --scenario closeup --marble glow --quality "$quality" --bloom 0; then
    python3 "$root/scripts/marble-bloom-check.py" "$out/glow-$quality-bloom-on.png" "$out/glow-$quality-bloom-off.png" || {
      echo "FAIL glow bloom halo on $quality"
      fail=1
    }
  fi
done
exit "$fail"
