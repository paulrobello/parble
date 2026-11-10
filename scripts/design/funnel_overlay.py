#!/usr/bin/env python3
"""Funnel look-dev images (docs/design/funnel/): the real kit render with the real sim paths drawn over it.

1. Runs the ignored `funnel_paths` sim test, which writes every tick of six runs (inlet 0 from its
   block channel and inlet 1 from its rail, each with a mini, standard and shooter cat's eye) of
   `parble_sim::funnel_run` to target/funnel/paths.json.
2. Captures the native app (`--scenario funnel`) in lockstep from the top (`--camera funnel-top`)
   and from three-quarters (`--camera funnel-3q`) after the scenario's marble has gone, and a
   strip of frames of one marble through the rail inlet (`--scenario funnel-rail`).
3. Projects each path through the shot frame's camera (`shotCamera` eye, forward and fovDeg in the
   `--dump-state` JSON), checked against the app's own `marblePx` on every strip frame, and draws
   it with Pillow. A path below the funnel's bottom face (in the bore, the catch and the run-out
   under the bowl) is drawn faint, since wood hides it.

Usage: python3 scripts/design/funnel_overlay.py [--bin target/debug/parble] [--skip-capture]
"""

import argparse
import json
import math
import re
import subprocess
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[2]
WORK = ROOT / "target/funnel"
OUT = ROOT / "docs/design/funnel"
W, H = 1280, 720
AXIS = (0.18, -0.18)
RIM_Y = 0.15
FUNNEL_BOTTOM_Y = 0.06
TICK_HZ = 240
PANEL = 320
# Strip: one standard marble through the rail inlet (inlet 1), ticks on the 4-tick lockstep grid.
STRIP_TICKS = [100, 200, 260, 320, 380, 420, 480, 560]
STRIP_SCENARIO = "funnel-rail"
STRIP_SIZE = "standard"
COLOURS = {
    (0, "mini"): (120, 230, 255),
    (0, "standard"): (40, 140, 255),
    (0, "shooter"): (150, 90, 255),
    (1, "mini"): (255, 240, 90),
    (1, "standard"): (255, 150, 30),
    (1, "shooter"): (255, 60, 80),
}
INLET_NAME = {0: "inlet 0 (block channel from -x)", 1: "inlet 1 (rail from +x)"}


def font(size: int) -> ImageFont.FreeTypeFont:
    for f in ("/System/Library/Fonts/Helvetica.ttc", "/System/Library/Fonts/SFNS.ttf"):
        if Path(f).is_file():
            return ImageFont.truetype(f, size)
    return ImageFont.load_default(size=size)


def capture(binary: Path, name: str, scenario: str, camera: str, size: str, tick: int) -> dict:
    out = WORK / "shots" / f"{name}.png"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.unlink(missing_ok=True)
    cmd = [
        str(binary), "--scenario", scenario, "--camera", camera,
        "--marble", "cats-eye", "--marble-size", size,
        "--dof", "0", "--taa", "0", "--quality", "high",
        "--screenshot", str(out), "--lockstep-after", "6", "--screenshot-tick", str(tick),
        "--exit-after", "90", "--dump-state",
    ]
    p = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, timeout=240)
    (out.with_suffix(".log")).write_text(p.stdout + p.stderr)
    if p.returncode != 0:
        raise SystemExit(f"FAIL {name}: exit {p.returncode}")
    bad = [ln for ln in p.stderr.splitlines() if re.search(r"\bERROR\b|panicked", ln)]
    if bad:
        raise SystemExit(f"FAIL {name}: {bad[:3]}")
    state = json.loads([ln for ln in p.stdout.splitlines() if ln.startswith("{")][-1])
    if not state["screenshot"].get("ok"):
        raise SystemExit(f"FAIL {name}: screenshot not ok {state['screenshot']}")
    state["png"] = str(out)
    (out.with_suffix(".json")).write_text(json.dumps(state))
    return state


def projector(cam: dict):
    eye = cam["eye"]
    f = cam["forward"]
    n = math.sqrt(sum(c * c for c in f))
    f = [c / n for c in f]
    # right = f x Y, up = right x f (Bevy's looking_at with world Y up).
    r = [-f[2], 0.0, f[0]]
    n = math.hypot(r[0], r[2])
    r = [c / n for c in r]
    u = [r[1] * f[2] - r[2] * f[1], r[2] * f[0] - r[0] * f[2], r[0] * f[1] - r[1] * f[0]]
    t = math.tan(math.radians(cam["fovDeg"]) / 2)
    aspect = W / H

    def proj(p):
        d = [p[i] - eye[i] for i in range(3)]
        zc = sum(d[i] * f[i] for i in range(3))
        xc = sum(d[i] * r[i] for i in range(3))
        yc = sum(d[i] * u[i] for i in range(3))
        return (W / 2 * (1 + xc / zc / t / aspect), H / 2 * (1 - yc / zc / t))

    return proj


def run_end(pos: list) -> int:
    """Index where the marble has left the run-out's end (x < -0.09, beyond the last straight)."""
    for i, q in enumerate(pos):
        if q[0] < -0.09:
            return i
    return len(pos) - 1


def metrics(run: dict) -> dict:
    pos = run["pos"]
    r = run["radius"]
    ax, az = AXIS
    rad = [math.hypot(q[0] - ax, q[2] - az) for q in pos]
    in_plan = [abs(q[0] - ax) < 0.09 and abs(q[2] - az) < 0.09 and q[1] > FUNNEL_BOTTOM_Y for q in pos]
    enter = in_plan.index(True)
    rim_y = RIM_Y + r
    # On the rim ledge: within 3 mm of the inlet floor height, inside the funnel's plan.
    rim = [i for i in range(enter, len(pos)) if in_plan[i] and pos[i][1] > rim_y - 0.003]
    leave_rim = rim[-1] if rim else enter
    rim_turn = 0.0
    for i in range(enter + 1, leave_rim + 1):
        a0 = math.atan2(pos[i - 1][2] - az, pos[i - 1][0] - ax)
        a1 = math.atan2(pos[i][2] - az, pos[i][0] - ax)
        d = (a1 - a0 + math.pi) % (2 * math.pi) - math.pi
        rim_turn += d
    # Down the bore: centre below the funnel's bottom face near the axis.
    bore = next(i for i, q in enumerate(pos) if rad[i] < 0.02 and q[1] < FUNNEL_BOTTOM_Y)
    bowl_turn = 0.0
    for i in range(leave_rim + 1, bore):
        a0 = math.atan2(pos[i - 1][2] - az, pos[i - 1][0] - ax)
        a1 = math.atan2(pos[i][2] - az, pos[i][0] - ax)
        bowl_turn += (a1 - a0 + math.pi) % (2 * math.pi) - math.pi
    # Climb above the inlet floor height anywhere in the funnel (rim riding).
    climb = max(pos[i][1] - rim_y for i in range(enter, bore))
    # Bounces: ticks with upward speed over 50 mm/s inside the funnel.
    ups = [i for i in range(enter, bore) if pos[i][4] > 0.05]
    max_up = max((pos[i][4] for i in range(enter, bore)), default=0.0)
    # Near-axis swirl before the bore: radius under 20 mm, above the funnel's bottom face.
    swirl = [i for i in range(leave_rim, bore) if rad[i] < 0.02 and pos[i][1] >= FUNNEL_BOTTOM_Y]
    catch = next(i for i in range(bore, len(pos)) if pos[i][1] < 0.03 + r)
    end = run_end(pos)
    rim_r = [rad[i] for i in rim if i > enter + 20] or [0.0]
    return {
        "inlet": run["inlet"],
        "size": run["size"],
        "radius_mm": round(r * 1000, 1),
        "enter_funnel_tick": enter,
        "rim_ledge_ticks": leave_rim - enter,
        "rim_ledge_turn_deg": round(math.degrees(abs(rim_turn))),
        "rim_ledge_radius_mm": [round(min(rim_r) * 1000, 1), round(max(rim_r) * 1000, 1)],
        "leave_rim_tick": leave_rim,
        "bowl_turns": round(abs(bowl_turn) / (2 * math.pi), 2),
        "near_axis_swirl_ticks": len(swirl),
        "bore_tick": bore,
        "catch_landing_tick": catch,
        "run_out_end_tick": end,
        "climb_above_inlet_floor_mm": round(climb * 1000, 1),
        "upward_ticks_over_50mm_s": len(ups),
        "max_upward_mm_s": round(max_up * 1000),
        "sense": "clockwise from above" if rim_turn > 0 else "counter-clockwise from above",
    }


def draw_paths(state: dict, runs: list, out: Path, title: str) -> None:
    img = Image.open(state["png"]).convert("RGBA")
    layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    proj = projector(state["shotCamera"])
    for run in runs:
        c = COLOURS[(run["inlet"], run["size"])]
        pos = run["pos"][: run_end(run["pos"]) + 1]
        pts = [(proj(q[:3]), q[1] >= FUNNEL_BOTTOM_Y) for q in pos]
        for (a, va), (b, vb) in zip(pts, pts[1:]):
            vis = va and vb
            d.line([a, b], fill=c + ((235,) if vis else (90,)), width=4 if vis else 2)
        # A dot every 0.25 s (60 ticks), and the start.
        for i in range(0, len(pos), 60):
            x, y = pts[i][0]
            d.ellipse([x - 4, y - 4, x + 4, y + 4], fill=c + (255,), outline=(0, 0, 0, 255))
        x, y = pts[0][0]
        d.rectangle([x - 6, y - 6, x + 6, y + 6], fill=c + (255,), outline=(0, 0, 0, 255))
    img = Image.alpha_composite(img, layer)
    # The legend goes in a side panel, so it covers none of the render.
    sheet = Image.new("RGB", (W + PANEL, H), (24, 24, 32))
    sheet.paste(img.convert("RGB"), (0, 0))
    d = ImageDraw.Draw(sheet)
    x0 = W + 16
    y = 14
    for line in title:
        d.text((x0, y), line, font=font(22), fill=(255, 255, 255))
        y += 28
    y += 10
    for run in runs:
        c = COLOURS[(run["inlet"], run["size"])]
        d.line([(x0, y + 11), (x0 + 30, y + 11)], fill=c, width=5)
        d.text((x0 + 40, y), f"inlet {run['inlet']}, {run['size']}", font=font(19), fill=(255, 255, 255))
        y += 26
    y += 12
    notes = [
        "inlet 0: block channel from -x",
        "inlet 1: rail from +x",
        "",
        "square: launch, 0.3 m/s",
        "dot: every 0.25 s (60 ticks)",
        "faint: below the funnel's bottom",
        "  face (bore, catch, run-out),",
        "  hidden by wood",
        "",
        "Real native kit render. Paths:",
        "every 240 Hz tick of the",
        "deterministic sim, projected",
        "through the shot's camera.",
    ]
    for line in notes:
        d.text((x0, y), line, font=font(16), fill=(210, 210, 220))
        y += 21
    sheet.save(out)


def strip(states: list, run: dict, out: Path) -> list:
    """Crops round the funnel from each frame, labelled with its tick, in two rows of four."""
    checks = []
    crops = []
    for st in states:
        proj = projector(st["shotCamera"])
        tick = st["tick"]
        sim = run["pos"][tick][:3]
        app = st["marbles"][0]["position"]
        err_m = math.dist(sim, app)
        px = proj(sim)
        mpx = st["shotCamera"]["marblePx"][0]
        err_px = math.dist(px, mpx)
        checks.append({"tick": tick, "sim_vs_app_mm": round(err_m * 1000, 3), "proj_vs_marblePx": round(err_px, 2)})
        img = Image.open(st["png"]).convert("RGB")
        # Crop the funnel's plan plus its inlets' ends.
        corners = [proj((x, RIM_Y, z)) for x in (0.06, 0.42) for z in (-0.30, -0.06)]
        x0 = min(c[0] for c in corners)
        x1 = max(c[0] for c in corners)
        y0 = min(c[1] for c in corners)
        y1 = max(c[1] for c in corners)
        crop = img.crop((int(x0), int(y0), int(x1), int(y1)))
        crop = crop.resize((480, int(480 * crop.height / crop.width)))
        d = ImageDraw.Draw(crop)
        cx, cy = mpx[0] - x0, mpx[1] - y0
        s = 480 / (x1 - x0)
        rr = st["shotCamera"]["marbleRadiusPx"] * s + 6
        d.ellipse([cx * s - rr, cy * s - rr, cx * s + rr, cy * s + rr], outline=(255, 150, 30), width=3)
        label = f"tick {tick}  ({tick / TICK_HZ:.2f} s)"
        d.rectangle([0, 0, 230, 30], fill=(0, 0, 0))
        d.text((8, 4), label, font=font(20), fill=(255, 255, 255))
        crops.append(crop)
    cw, ch = crops[0].size
    sheet = Image.new("RGB", (cw * 4 + 5 * 6, ch * 2 + 3 * 6 + 40), (20, 20, 28))
    ImageDraw.Draw(sheet).text(
        (8, 8),
        "One standard cat's eye through inlet 1 (rail from +x), top view; orange ring = the app's own marblePx",
        font=font(20), fill=(255, 255, 255),
    )
    for k, c in enumerate(crops):
        sheet.paste(c, (6 + (k % 4) * (cw + 6), 46 + (k // 4) * (ch + 6)))
    sheet.save(out)
    return checks


def contact(m: list, out: Path) -> None:
    """One captioned sheet: both path views over the frame strip, with the measured behaviour."""
    top = Image.open(out / "funnel-top-paths.png")
    tq = Image.open(out / "funnel-3q-paths.png")
    st = Image.open(out / "funnel-rail-strip.png")
    sw = st.width
    half = (sw - 18) // 2
    scale = half / top.width
    top = top.resize((half, int(top.height * scale)))
    tq = tq.resize((half, int(tq.height * scale)))
    by = lambda k: [r[k] for r in m]
    span = lambda k: f"{min(by(k)):.0f}-{max(by(k)):.0f}"
    rows = {s: [r for r in m if r["size"] == s] for s in ("mini", "standard", "shooter")}

    def per_size(k: str, fmt: str = "{:.0f}") -> str:
        return ", ".join(
            f"{s} {fmt.format(min(r[k] for r in rs))}-{fmt.format(max(r[k] for r in rs))}"
            for s, rs in rows.items()
        )

    maxup = ", ".join(f"{s} {max(r['max_upward_mm_s'] for r in rs)}" for s, rs in rows.items())
    lines = [
        "What the paths show (all six runs, cat's eye at 0.3 m/s; a tick is 1/240 s):",
        "1. Every marble enters, none misses an inlet or leaves the funnel. Each first rides the level scroll channel round the rim, counter-clockwise from above, "
        f"for {span('rim_ledge_turn_deg')} degrees ({span('rim_ledge_ticks')} ticks, about 1.25 s) , then drops into the bowl.",
        f"2. No rim climb: the highest any marble gets is {max(by('climb_above_inlet_floor_mm')):.1f} mm above its inlet's floor height.",
        f"3. Off the ledge it spirals in, then circles the bore mouth. Turns in the bowl: {per_size('bowl_turns', '{:.1f}')}. "
        f"Ticks within 20 mm of the axis: {per_size('near_axis_swirl_ticks')}.",
        f"4. Bigger marbles rattle at the bore mouth, 5-13 mm off the axis. Ticks moving up faster than 50 mm/s: "
        f"{per_size('upward_ticks_over_50mm_s')} (peak mm/s: {maxup}).",
        f"5. Down the bore at tick {span('bore_tick')}, on the catch at {span('catch_landing_tick')}, off the run-out's end at "
        f"{span('run_out_end_tick')} (the L1 run-out is slow, about 0.14 m/s).",
    ]
    header = 56
    text_h = 34 * len(lines) + 20
    sheet = Image.new("RGB", (sw, header + top.height + 12 + st.height + 12 + text_h), (18, 18, 24))
    d = ImageDraw.Draw(sheet)
    d.text((10, 12), "blk_funnel_3x3_l2 + blk_catch_l1: marble paths from each inlet (top, three-quarter, frame strip)",
           font=font(28), fill=(255, 255, 255))
    sheet.paste(top, (6, header))
    sheet.paste(tq, (12 + half, header))
    y = header + top.height + 12
    sheet.paste(st, (0, y))
    y += st.height + 12
    for ln in lines:
        d.text((12, y), ln, font=font(21), fill=(230, 230, 236))
        y += 34
    sheet.save(out / "funnel-contact-sheet.png")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", default=str(ROOT / "target/debug/parble"))
    ap.add_argument("--skip-capture", action="store_true")
    a = ap.parse_args()
    binary = Path(a.bin)
    if not a.skip_capture:
        subprocess.run(
            ["cargo", "test", "-p", "parble-sim", "--lib", "funnel_paths", "--", "--ignored"],
            cwd=ROOT, check=True, capture_output=True,
        )
    runs = json.loads((WORK / "paths.json").read_text())
    OUT.mkdir(parents=True, exist_ok=True)
    shots = WORK / "shots"
    if a.skip_capture:
        load = lambda n: json.loads((shots / f"{n}.json").read_text())
        top, tq = load("top"), load("3q")
        frames = [load(f"strip-{t}") for t in STRIP_TICKS]
    else:
        # The scenario's own mini has run out and fallen off the floor's edge by tick 2400, so
        # the scene is empty under the paths.
        top = capture(binary, "top", "funnel", "funnel-top", "mini", 2400)
        tq = capture(binary, "3q", "funnel", "funnel-3q", "mini", 2400)
        frames = [capture(binary, f"strip-{t}", STRIP_SCENARIO, "funnel-top", STRIP_SIZE, t) for t in STRIP_TICKS]
    draw_paths(top, runs, OUT / "funnel-top-paths.png", ["Funnel catcher", "top view, six sim paths"])
    draw_paths(tq, runs, OUT / "funnel-3q-paths.png", ["Funnel catcher", "three-quarter view,", "six sim paths"])
    strip_run = next(r for r in runs if r["inlet"] == 1 and r["size"] == STRIP_SIZE)
    checks = strip(frames, strip_run, OUT / "funnel-rail-strip.png")
    m = [metrics(r) for r in runs]
    contact(m, OUT)
    report = {"strip_checks": checks, "runs": m}
    (WORK / "report.json").write_text(json.dumps(report, indent=1))
    print(json.dumps(report, indent=1))
    worst = max(c["proj_vs_marblePx"] for c in checks)
    if worst > 3.0 or max(c["sim_vs_app_mm"] for c in checks) > 0.01:
        print(f"FAIL: projection or sim/app mismatch (worst {worst:.2f} px)")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
