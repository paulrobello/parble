#!/usr/bin/env python3
"""P2-T6 shots test: the mechanism showcase (`--scenario mechanisms`) at two fixed sim ticks.

- toggle-mid-flip: the toggle's rocker part-way through its swing after the marble left
  (`TOGGLE_FLIP_SHOT_TICK` in crates/parble-sim/src/mech_showcase.rs).
- gate-open: the one-way gate's flap pushed well open (past 30°) by the passing marble
  (`GATE_OPEN_SHOT_TICK`).
- lift-climb: the stepper elevator's slats part-way up a stroke, carrying the marble
  (`LIFT_SHOT_TICK`).

Each capture runs the native build in lockstep (`--lockstep-after`), so the tick is the same sim
state on every run, and must exit 0 with a lit frame and no logged error. The part poses drawn on
the shot frame (`shotParts` in the `--dump-state` JSON) must show the toggle 25-75 % through its
swing and still moving, and the gate flap at its open angle. Shots go to target/shots/mechanisms;
the committed docs/shots/p2-t6-*.png are reviewed copies, never rewritten here.

Usage: python3 scripts/shots/mechanisms.py [--bin target/debug/parble] [--out DIR] [--quality Q]
"""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SHOWCASE = ROOT / "crates/parble-sim/src/mech_showcase.rs"
LOCKSTEP_AFTER_S = 6


def const(path: Path, name: str) -> int:
    m = re.search(rf"pub const {name}: \w+ = ([-\d_]+);", path.read_text())
    if not m:
        raise SystemExit(f"{name} not found in {path}")
    return int(m.group(1).replace("_", ""))


def limits(piece: str) -> list[int]:
    sidecar = json.loads((ROOT / f"assets/kit/{piece}/{piece}.kit.json").read_text())
    p = sidecar["animated_parts"][0]
    # A slide part (the stepper's slats) carries its limits in µm.
    return p["limits_um"] if p.get("joint") == "slide" else p["limits_mdeg"]


def capture(binary: Path, out: Path, quality: str, tick: int) -> dict:
    out.parent.mkdir(parents=True, exist_ok=True)
    out.unlink(missing_ok=True)
    cmd = [
        str(binary),
        "--scenario", "mechanisms",
        "--quality", quality,
        "--screenshot", str(out),
        "--lockstep-after", str(LOCKSTEP_AFTER_S),
        "--screenshot-tick", str(tick),
        "--exit-after", "90",
        "--dump-state",
    ]
    # A cold shader cache or a loaded host can stall the first frames: one retry.
    for attempt in (1, 2):
        try:
            p = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, timeout=180)
            break
        except subprocess.TimeoutExpired:
            if attempt == 2:
                raise SystemExit(f"FAIL {out.name}: no exit within 180 s, twice")
            print(f"retry {out.name}: no exit within 180 s")
    log = out.with_suffix(".log")
    log.write_text(p.stdout + p.stderr)
    if p.returncode != 0:
        raise SystemExit(f"FAIL {out.name}: exit {p.returncode} (log {log})")
    bad = [ln for ln in p.stderr.splitlines() if re.search(r"\bERROR\b|panicked|Validation Error", ln)]
    if bad:
        raise SystemExit(f"FAIL {out.name}: logged errors (log {log}): {bad[:3]}")
    state = json.loads([ln for ln in p.stdout.splitlines() if ln.startswith("{")][-1])
    if not state["screenshot"].get("ok") or not out.is_file():
        raise SystemExit(f"FAIL {out.name}: screenshot {state['screenshot']}")
    return state


def part(state: dict, piece: str) -> dict:
    parts = [p for p in state.get("shotParts") or [] if p["piece"] == piece]
    if len(parts) != 1:
        raise SystemExit(f"FAIL: shotParts has {len(parts)} {piece} parts: {state.get('shotParts')}")
    return parts[0]


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", default=str(ROOT / "target/debug/parble"))
    ap.add_argument("--out", default=str(ROOT / "target/shots/mechanisms"))
    ap.add_argument("--quality", default="high")
    a = ap.parse_args()
    out = Path(a.out)
    binary = Path(a.bin)
    fail = False

    tick = const(SHOWCASE, "TOGGLE_FLIP_SHOT_TICK")
    s = capture(binary, out / "toggle-mid-flip.png", a.quality, tick)
    t = part(s, "mech_toggle_l3")
    lo, hi = limits("mech_toggle_l3")
    travel = (hi - t["angleMdeg"]) / (hi - lo)
    moving = t["prevMdeg"] is not None and t["prevMdeg"] != t["tickMdeg"]
    ok = 0.25 <= travel <= 0.75 and moving
    fail |= not ok
    print(f"{'ok  ' if ok else 'FAIL'} toggle-mid-flip tick {tick}: rocker {t['angleMdeg']} mdeg "
          f"(prev tick {t['prevMdeg']}), {travel:.2f} of its {hi} -> {lo} swing; {out / 'toggle-mid-flip.png'}")

    tick = const(SHOWCASE, "GATE_OPEN_SHOT_TICK")
    s = capture(binary, out / "gate-open.png", a.quality, tick)
    g = part(s, "mech_gate_oneway_l1")
    want = 30_000  # the physical flap's angle at the shot tick is pinned in mech_showcase.rs
    ok = g["tickMdeg"] >= want and g["angleMdeg"] == g["tickMdeg"]
    fail |= not ok
    print(f"{'ok  ' if ok else 'FAIL'} gate-open tick {tick}: flap {g['angleMdeg']} mdeg "
          f"(want >= {want}); {out / 'gate-open.png'}")

    tick = const(SHOWCASE, "LIFT_SHOT_TICK")
    s = capture(binary, out / "lift-climb.png", a.quality, tick)
    sl = part(s, "mech_stepper_up1_l3")
    lo, hi = limits("mech_stepper_up1_l3")
    travel = (sl["angleMdeg"] - lo) / (hi - lo)
    moving = sl["prevMdeg"] is not None and sl["prevMdeg"] != sl["tickMdeg"]
    ok = 0.25 <= travel <= 0.75 and moving
    fail |= not ok
    print(f"{'ok  ' if ok else 'FAIL'} lift-climb tick {tick}: slats {sl['angleMdeg']} um "
          f"(prev tick {sl['prevMdeg']}), {travel:.2f} of the {hi} um stroke; {out / 'lift-climb.png'}")
    return 1 if fail else 0


if __name__ == "__main__":
    sys.exit(main())
