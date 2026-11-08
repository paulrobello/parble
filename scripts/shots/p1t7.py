#!/usr/bin/env python3
"""P1-T7 shots test: one lockstep frame per quality tier, the TAA ghost check, and the DoF
side-by-side with ref-01. Native build (Metal/Vulkan through wgpu), 1280x720 physical.

Usage: python3 scripts/shots/p1t7.py [--bin target/debug/parble] [--out docs/shots]

Each capture runs the app in lockstep (`--lockstep-after`): the sim holds at tick 0 while the
pipelines compile, then steps exactly 4 ticks (1/60 s) per frame, so a given `--screenshot-tick`
is the same sim state and the same camera in every run. The ghost check renders the same frame
twice at High, with TAA and with `--taa 0`, and counts pixels in the marble's trail (its screen
positions over the previous frames, minus its current disc) whose largest channel differs by more
than GHOST_DIFF (a blue marble on tan wood is a chroma edge, so luma alone misses it). A leading-side band of the same shape is the noise floor (TAA vs no-AA edges and SSAO
noise differ there too, but no history can trail ahead of the marble). A synthetic ghost, the
marble disc blended at 50 % into its previous positions on the no-TAA frame, must fail the check,
so a pass is not vacuous. Exit status 1 when any check fails.

Needs numpy and Pillow.
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[2]
TIERS = ["low", "medium", "high", "ultra"]
LOCKSTEP_AFTER_S = 8
# Follow shot near the foot of the ramp (marble accelerating) for the per-tier shots.
TIER_TICK = 120
# Ghost captures: the follow shot at peak speed, and the fixed trackside shot as the marble
# crosses the frame at speed. Ticks are multiples of 4 (one lockstep frame).
GHOST_CASES = {"follow-peak": 100, "trackside-cross": 168}
# A pixel "differs" when its largest channel (0..1) moves more than this between TAA and no-TAA.
GHOST_DIFF = 0.05
# Pass: differing trail pixels, net of the leading-side noise floor, at most this fraction of the
# marble's disc area. One full-strength ghost copy is ~1.0; 0.25 allows edge shimmer only.
GHOST_MAX_DISC_FRACTION = 0.25


def run(binary: Path, out: Path, quality: str, tick: int, extra: list[str]) -> dict:
    out.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        str(binary),
        "--quality", quality,
        "--screenshot", str(out),
        "--lockstep-after", str(LOCKSTEP_AFTER_S),
        "--screenshot-tick", str(tick),
        "--exit-after", "90",
        "--dump-state",
        *extra,
    ]
    p = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, timeout=180)
    if p.returncode != 0:
        sys.stderr.write(p.stderr[-4000:])
        raise SystemExit(f"capture failed ({p.returncode}): {' '.join(cmd)}")
    line = [ln for ln in p.stdout.splitlines() if ln.startswith("{")][-1]
    state = json.loads(line)
    errors = [ln for ln in p.stderr.splitlines() if " ERROR " in ln or "ERROR " in ln[:40]]
    state["_errors"] = errors
    return state


def channel_diff(a: np.ndarray, b: np.ndarray) -> np.ndarray:
    return np.abs(a.astype(np.float32) - b.astype(np.float32)).max(-1) / 255.0


def capsule_mask(shape, pts, radius: float) -> np.ndarray:
    """Pixels within `radius` of the polyline through `pts`."""
    h, w = shape
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float32)
    m = np.zeros(shape, bool)
    if len(pts) == 1:
        pts = [pts[0], pts[0]]
    for (ax, ay), (bx, by) in zip(pts[:-1], pts[1:]):
        dx, dy = bx - ax, by - ay
        ll = dx * dx + dy * dy
        t = np.clip(((xx - ax) * dx + (yy - ay) * dy) / ll, 0, 1) if ll > 0 else 0.0
        px, py = ax + t * dx, ay + t * dy
        m |= (xx - px) ** 2 + (yy - py) ** 2 <= radius * radius
    return m


def disc(shape, c, r) -> np.ndarray:
    h, w = shape
    yy, xx = np.mgrid[0:h, 0:w]
    return (xx - c[0]) ** 2 + (yy - c[1]) ** 2 <= r * r


def ghost_metric(a: np.ndarray, b: np.ndarray, cam: dict) -> dict:
    trail = [tuple(p) for p in cam["marblePx"]]
    r = cam["marbleRadiusPx"]
    cur = trail[0]
    shape = a.shape[:2]
    band = r + 3
    behind = capsule_mask(shape, trail, band) & ~disc(shape, cur, r + 3)
    # Mirror the trail through the current centre: the leading side, where no history can lag.
    ahead_pts = [(2 * cur[0] - x, 2 * cur[1] - y) for x, y in trail]
    ahead = capsule_mask(shape, ahead_pts, band) & ~disc(shape, cur, r + 3)
    diff = channel_diff(a, b) > GHOST_DIFF
    disc_area = float(np.pi * r * r)
    trail_px = int((diff & behind).sum())
    lead_px = int((diff & ahead).sum())
    travel = float(np.hypot(trail[-1][0] - cur[0], trail[-1][1] - cur[1]))
    net = max(0, trail_px - lead_px)
    return {
        "marbleRadiusPx": round(r, 2),
        "trailTravelPx": round(travel, 1),
        "trailMaskPx": int(behind.sum()),
        "trailDiffPx": trail_px,
        "leadDiffPx": lead_px,
        "netGhostPx": net,
        "netGhostDiscFraction": round(net / disc_area, 3),
    }


def synthetic_ghost(b: np.ndarray, cam: dict) -> np.ndarray:
    """Blend the marble disc at 50 % into each previous trail position (a fake TAA ghost)."""
    out = b.astype(np.float32).copy()
    trail = cam["marblePx"]
    r = cam["marbleRadiusPx"]
    cx, cy = trail[0]
    h, w = b.shape[:2]
    x0, x1 = int(max(cx - r, 0)), int(min(cx + r + 1, w))
    y0, y1 = int(max(cy - r, 0)), int(min(cy + r + 1, h))
    patch = b[y0:y1, x0:x1].astype(np.float32)
    pm = disc(patch.shape[:2], (cx - x0, cy - y0), r)
    for px, py in trail[1:]:
        dx, dy = int(round(px - cx)), int(round(py - cy))
        ty0, tx0 = y0 + dy, x0 + dx
        if ty0 < 0 or tx0 < 0 or ty0 + patch.shape[0] > h or tx0 + patch.shape[1] > w:
            continue
        region = out[ty0:ty0 + patch.shape[0], tx0:tx0 + patch.shape[1]]
        region[pm] = 0.5 * region[pm] + 0.5 * patch[pm]
    return out.clip(0, 255).astype(np.uint8)


def load(p: Path) -> np.ndarray:
    return np.asarray(Image.open(p).convert("RGB"))


def side_by_side(ref: Path, shots: list[tuple[str, Path]], out: Path) -> None:
    h = 432
    tiles = [("ref-01", Image.open(ref).convert("RGB"))] + [
        (label, Image.open(p).convert("RGB")) for label, p in shots
    ]
    tiles = [(lbl, im.resize((round(im.width * h / im.height), h), Image.LANCZOS)) for lbl, im in tiles]
    gap = 8
    width = sum(im.width for _, im in tiles) + gap * (len(tiles) - 1)
    canvas = Image.new("RGB", (width, h + 28), (16, 16, 16))
    draw = ImageDraw.Draw(canvas)
    x = 0
    for label, im in tiles:
        canvas.paste(im, (x, 28))
        draw.text((x + 6, 7), label, fill=(230, 230, 230))
        x += im.width + gap
    out.parent.mkdir(parents=True, exist_ok=True)
    canvas.save(out)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", default=str(ROOT / "target/debug/parble"))
    ap.add_argument("--out", default=str(ROOT / "docs/shots"))
    ap.add_argument("--work", default=str(ROOT / "target/shots/p1t7"))
    args = ap.parse_args()
    binary, out, work = Path(args.bin), Path(args.out), Path(args.work)
    report: dict = {"tiers": {}, "ghost": {}, "thresholds": {
        "ghostChannelDiff": GHOST_DIFF, "ghostMaxDiscFraction": GHOST_MAX_DISC_FRACTION}}
    ok = True

    for tier in TIERS:
        png = out / f"p1-t7-{tier}.png"
        s = run(binary, png, tier, TIER_TICK, [])
        cam = s["shotCamera"]
        report["tiers"][tier] = {"png": str(png.relative_to(ROOT)), "post": cam["post"],
                                 "shot": cam["shot"], "screenshot": s["screenshot"],
                                 "errors": s["_errors"]}
        good = s["screenshot"].get("ok") and not s["_errors"]
        ok &= bool(good)
        print(f"{tier:7s} ok={good} post={json.dumps(cam['post'])}")

    for name, tick in GHOST_CASES.items():
        a_png, b_png = work / f"{name}-taa.png", work / f"{name}-notaa.png"
        sa = run(binary, a_png, "high", tick, [])
        sb = run(binary, b_png, "high", tick, ["--taa", "0"])
        cam = sa["shotCamera"]
        assert sa["shotCamera"]["marblePx"][0] == sb["shotCamera"]["marblePx"][0], "lockstep diverged"
        a, b = load(a_png), load(b_png)
        m = ghost_metric(a, b, cam)
        fake = synthetic_ghost(b, cam)
        Image.fromarray(fake).save(work / f"{name}-synthetic-ghost.png")
        mc = ghost_metric(fake, b, cam)
        speed = float(np.linalg.norm(sa["marbles"][0]["linvel"]))
        passed = m["netGhostDiscFraction"] <= GHOST_MAX_DISC_FRACTION
        # The control only means something when the marble moved on screen by at least its
        # diameter; the follow shot holds it nearly still in frame, so there is no trail to fake.
        moving = m["trailTravelPx"] >= 2 * m["marbleRadiusPx"]
        control_fires = mc["netGhostDiscFraction"] > GHOST_MAX_DISC_FRACTION if moving else None
        ok &= passed and control_fires is not False
        report["ghost"][name] = {"tick": tick, "speedMps": round(speed, 3), "shot": cam["shot"],
                                 "taa": m, "syntheticGhost": mc, "pass": passed,
                                 "controlFires": control_fires}
        # Keep the TAA frame and the diff for review.
        Image.open(a_png).save(out / f"p1-t7-ghost-{name}.png")
        print(f"ghost {name}: tick={tick} speed={speed:.3f} m/s metric={m['netGhostDiscFraction']} "
              f"(max {GHOST_MAX_DISC_FRACTION}) synthetic={mc['netGhostDiscFraction']} pass={passed} "
              f"controlFires={control_fires}")

    side_by_side(ROOT / "docs/reference/ref-01.webp",
                 [("High (f/%.1f)" % report["tiers"]["high"]["post"]["aperture"], out / "p1-t7-high.png"),
                  ("Ultra (f/%.1f)" % report["tiers"]["ultra"]["post"]["aperture"], out / "p1-t7-ultra.png")],
                 out / "p1-t7-dof-vs-ref01.png")
    report["pass"] = ok
    (out / "p1-t7-shots.json").write_text(json.dumps(report, indent=2) + "\n")
    print("shots-p1t7:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
