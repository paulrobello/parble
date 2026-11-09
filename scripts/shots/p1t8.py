#!/usr/bin/env python3
"""P1-T8 shots test: the showcase and the 2,000-piece stress scene, one fixed lockstep shot per
quality tier, each checked and compared. Native build, 1280x720 physical.

Usage: python3 scripts/shots/p1t8.py [--bin target/debug/parble] [--work DIR] [--update]

Each capture runs in lockstep (`--lockstep-after`), so its `--screenshot-tick` is the same sim state
and the same camera on every run: the showcase's director following the lead marble on the playroom
rug, and the stress scene's one set shot of the track and the tower. Every shot must be lit (the
app's own check), log no ERROR, and report its scenario, tier, the marbles the hopper has released
by then (one every 480 ticks: 2 in the showcase shot, all 10 in the stress shot) and its kit pieces
(46 and 2,000). Then two comparisons on a 160x90 box-filtered copy, mean absolute difference over
RGB in 0..1:

- against its committed reference, docs/shots/p1-t8-<scene>-<tier>.png (640x360), at most
  REF_MAX_DIFF: the scene, framing and tier look have not moved;
- against the same scene's High shot, at most TIER_MAX_DIFF: every tier frames the same shot.

`--update` writes the references from this run (new scenes' own goldens; review them first).
Exit status 1 when any check fails. Needs numpy and Pillow.
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
TIERS = ["low", "medium", "high", "ultra"]
LOCKSTEP_AFTER_S = 8
# Showcase: the director following marble 0 down the track on the rug. Stress: the set shot once
# the hopper has released all 10 marbles. Ticks are multiples of 4 (one lockstep frame).
SCENES = {
    "showcase": {"tick": 800, "pieces": 46, "marbles": 2},
    "stress2000": {"tick": 4400, "pieces": 2000, "marbles": 10},
}
REF_SIZE = (640, 360)
CMP_SIZE = (160, 90)
REF_MAX_DIFF = 0.03
TIER_MAX_DIFF = 0.12


def capture(binary: Path, out: Path, scene: str, quality: str, tick: int) -> dict:
    out.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        str(binary),
        "--scenario", scene,
        "--quality", quality,
        "--screenshot", str(out),
        "--lockstep-after", str(LOCKSTEP_AFTER_S),
        "--screenshot-tick", str(tick),
        "--exit-after", "150",
        "--dump-state",
    ]
    p = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, timeout=240)
    if p.returncode != 0:
        sys.stderr.write(p.stderr[-3000:])
        raise SystemExit(f"capture failed ({p.returncode}): {' '.join(cmd)}")
    state = json.loads([ln for ln in p.stdout.splitlines() if ln.startswith("{")][-1])
    state["_errors"] = [ln for ln in p.stderr.splitlines() if "ERROR" in ln]
    return state


def small(img: Image.Image) -> np.ndarray:
    return np.asarray(img.convert("RGB").resize(CMP_SIZE, Image.BOX), dtype=np.float32) / 255.0


def diff(a: np.ndarray, b: np.ndarray) -> float:
    return float(np.abs(a - b).mean())


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", default=str(ROOT / "target/debug/parble"))
    ap.add_argument("--work", default=str(ROOT / "target/shots/p1t8"))
    ap.add_argument("--refs", default=str(ROOT / "docs/shots"))
    ap.add_argument("--update", action="store_true")
    a = ap.parse_args()
    binary, work, refs = Path(a.bin), Path(a.work), Path(a.refs)
    report: dict = {"thresholds": {"refMaxDiff": REF_MAX_DIFF, "tierMaxDiff": TIER_MAX_DIFF}, "scenes": {}}
    ok = True

    for scene, spec in SCENES.items():
        shots: dict[str, np.ndarray] = {}
        rows = report["scenes"][scene] = {}
        for tier in TIERS:
            png = work / f"{scene}-{tier}.png"
            s = capture(binary, png, scene, tier, spec["tick"])
            kit = s.get("kit") or {}
            checks = {
                "lit": bool(s["screenshot"].get("ok")),
                "noErrors": not s["_errors"],
                "scenario": s["scenario"] == scene,
                "quality": s["quality"] == tier,
                "marbles": len(s["marbles"]) == spec["marbles"],
                "kitPieces": kit.get("pieces") == spec["pieces"],
            }
            img = Image.open(png)
            shots[tier] = small(img)
            ref = refs / f"p1-t8-{scene}-{tier}.png"
            if a.update:
                img.convert("RGB").resize(REF_SIZE, Image.LANCZOS).save(ref)
            ref_diff = diff(shots[tier], small(Image.open(ref))) if ref.exists() else None
            checks["reference"] = ref_diff is not None and ref_diff <= REF_MAX_DIFF
            rows[tier] = {"png": str(png.relative_to(ROOT)), "tick": s["tick"], "refDiff": ref_diff,
                          "checks": checks, "errors": s["_errors"][:5],
                          "kit": {k: kit.get(k) for k in ("pieces", "chunks", "drawBound")}}
            good = all(checks.values())
            ok &= good
            failed = [k for k, v in checks.items() if not v]
            print(f"{'ok  ' if good else 'FAIL'} {scene:10s} {tier:6s} tick {s['tick']} "
                  f"ref diff {ref_diff if ref_diff is None else round(ref_diff, 4)} "
                  f"(max {REF_MAX_DIFF}){' failed: ' + ', '.join(failed) if failed else ''}")
        for tier in TIERS:
            d = diff(shots[tier], shots["high"])
            good = d <= TIER_MAX_DIFF
            rows[tier]["vsHighDiff"] = d
            ok &= good
            print(f"{'ok  ' if good else 'FAIL'} {scene:10s} {tier:6s} vs high {d:.4f} (max {TIER_MAX_DIFF})")

    report["pass"] = ok
    work.mkdir(parents=True, exist_ok=True)
    (work / "p1t8-shots.json").write_text(json.dumps(report, indent=2) + "\n")
    print("shots-p1t8:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
