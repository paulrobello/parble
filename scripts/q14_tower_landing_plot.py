"""Plots the q14 tower landing figure from the trace dump of
`block_seam_test::dump_tower_traces`:

  PARBLE_TOWER_TRACE=/tmp/q14_traces.json cargo test -p parble-sim --release dump_tower_traces -- --ignored
  uv run --with matplotlib --with numpy python scripts/q14_tower_landing_plot.py /tmp/q14_traces.json docs/shots/q14-tower-landing.png
"""
import json
import sys

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
from matplotlib.patches import Circle

D = json.load(open(sys.argv[1]))
OUT = sys.argv[2]
HZ = 240.0
R = 8.0  # marble radius, mm
BAD_I, GOOD_I = 3, 9  # sweep index: 0.275 m/s 2 mm off (0.544 sideways), 0.350 m/s 2 mm off (0.003)

BG, FG = "#1E1E1E", "#E6E6E6"
RED, GRN, BLU, YEL = "#F44336", "#4CAF50", "#2196F3", "#FFC107"
XL = (-48, 95)


def prep(r):
    a = np.array(r["trace"])
    t = a[:, 0] / HZ * 1000
    x, y, z = a[:, 1] * 1000, a[:, 2] * 1000, a[:, 3] * 1000
    vz = a[:, 6]
    i0 = int(np.argmax(y < 60))
    vy = a[:, 5]
    jump = np.where((np.diff(vy) > 0.3) & (y[1:] < 45))[0]
    land = int(jump[0]) + 1 if len(jump) else i0
    end = int(np.argmax(x < -45))
    return dict(r=r, t=t, x=x, y=y, z=z, vz=vz, land=land, i0=i0, end=end)


def style(ax):
    ax.set_facecolor(BG)
    for s in ax.spines.values():
        s.set_color("#777")
    ax.tick_params(colors=FG)
    ax.xaxis.label.set_color(FG)
    ax.yaxis.label.set_color(FG)
    ax.title.set_color(FG)
    ax.grid(color="#333", lw=0.5)


def cells(ax, ybot):
    for xb in (-30, 30, 90):
        ax.axvline(xb, color="#888", lw=1, ls="--")
    for xc, name in ((60, "tower socket cell"), (0, "rail joint cell")):
        ax.text(xc, ybot, name, color="#aaa", ha="center", va="bottom", fontsize=9)


def floor_profile():
    # commit cbcbdd7: smoothstep, 15 mm fall over 37 mm, from the shaft side x=+77 to the port x=+40
    xs = np.linspace(40, 77, 60)
    s = (77 - xs) / 37
    return xs, 30 - 15 * (3 * s**2 - 2 * s**3)


bad, good = prep(D[BAD_I]), prep(D[GOOD_I])
fig, axes = plt.subplots(3, 2, figsize=(16, 12.5),
                         gridspec_kw=dict(height_ratios=[1.0, 1.15, 0.9], hspace=0.42, wspace=0.12))
fig.patch.set_facecolor(BG)

for col, (run, title, cc) in enumerate(((bad, "BAD run", RED), (good, "GOOD run", GRN))):
    r = run["r"]
    L = run["land"]
    sl = slice(max(run["i0"], L - 30), run["end"])
    x, y, z, t, vz = (run[k][sl] for k in ("x", "y", "z", "t", "vz"))
    lx, ly, lz = run["x"][L], run["y"][L], run["z"][L]
    step = 4
    idx = np.arange(0, len(x), step)

    ax = axes[0, col]
    style(ax)
    ax.axhline(0, color=BLU, lw=1.2, ls=":")
    ax.text(XL[1] - 1, 0.25, "track centreline z = 0", color=BLU, fontsize=9, ha="right", va="bottom")
    ax.plot(x, z, color=cc, lw=1.2, alpha=0.7)
    ax.scatter(x[idx], z[idx], c=t[idx], cmap="viridis", s=18, zorder=3)
    ax.scatter([lx], [lz], s=150, facecolors="none", edgecolors=YEL, lw=2.2, zorder=4)
    ax.annotate(f"landing: {lz:+.1f} mm off centre\n(x = {lx:.0f} mm)", (lx, lz),
                xytext=(92, -5.6 if lz > 0 else 4.2), color=YEL, fontsize=10,
                arrowprops=dict(arrowstyle="->", color=YEL), ha="right")
    ax.set_xlim(*XL)
    ax.set_ylim(-7, 7)
    ax.set_xlabel("x along the track (mm), marble travels right to left")
    ax.set_ylabel("z sideways (mm)")
    ax.set_title(f"{title}: entry {r['v']:.3f} m/s, {r['dz']*1000:.0f} mm off, exits at {r['side']:.3f} m/s sideways\n"
                 f"TOP VIEW (z stretched ~6x, a dot every {step} ticks = {step / HZ * 1000:.1f} ms)",
                 fontsize=11, loc="left")
    cells(ax, -6.9)

    ax = axes[1, col]
    style(ax)
    xs, fl = floor_profile()
    ax.fill_between([-30, 40], 0, 15, color="#3a3a3a")
    ax.fill_between(xs, 0, fl, color="#3a3a3a")
    ax.fill_between([77, 90], 0, 30, color="#3a3a3a")
    ax.plot(xs, fl, color="#bbb", lw=1.5)
    ax.text(-28, 6, "solid block", color="#999", fontsize=9)
    ax.text(70, 33, "landing floor\n(schematic)", color="#ccc", fontsize=9, ha="left")
    ax.plot(x, y, color=cc, lw=1.2, alpha=0.7)
    ax.scatter(x[idx], y[idx], c=t[idx], cmap="viridis", s=18, zorder=3)
    ax.add_patch(Circle((lx, ly), R, fill=False, ec=YEL, lw=1.6, zorder=4))
    ax.annotate("first impact\nfalling ~1.4 m/s", (lx, ly - R), xytext=(92, 3), ha="right", color=YEL, fontsize=10,
                arrowprops=dict(arrowstyle="->", color=YEL))
    ax.set_xlim(*XL)
    ax.set_ylim(0, 48)
    ax.set_aspect("equal")
    ax.set_xlabel("x along the track (mm)")
    ax.set_ylabel("y height (mm)")
    ax.set_title("SIDE VIEW (true scale, 16 mm marble drawn at the impact)", fontsize=11, loc="left")
    cells(ax, 40)

    ax = axes[2, col]
    style(ax)
    tt = t - run["t"][L]
    ax.plot(tt, vz, color=cc, lw=1.4)
    ax.axhline(0, color="#777", lw=1)
    for s in (0.1, -0.1):
        ax.axhline(s, color=GRN, lw=1, ls="--")
    ax.text(tt[-1], 0.105, "0.1 m/s original target", color=GRN, fontsize=9, ha="right", va="bottom")
    ax.axvline(0, color=YEL, lw=1, ls=":")
    ax.text(1.5, -0.66, "impact", color=YEL, fontsize=9)
    ax.set_xlim(-30, tt[-1] + 2)
    ax.set_ylim(-0.7, 0.7)
    ax.set_title(f"SIDEWAYS SPEED vz(t): peak |vz| leaving the socket = {r['side']:.3f} m/s",
                 fontsize=11, loc="left", color=cc)
    ax.set_xlabel("time from first impact (ms)")
    ax.set_ylabel("vz (m/s)")

fig.suptitle("Q14  Tower landing bounce: the marble falls 120 mm, hits the tunnel floor 1-5 mm off centre and rebounds (glass, e = 0.66)",
             color=FG, fontsize=15, y=0.985)
fig.text(0.5, 0.947, "Both runs stay on the rails. Full sweep of 24 runs: 0.003 to 0.544 m/s sideways, 0 off the rails, 2 under 0.1 m/s.",
         color="#bbb", ha="center", fontsize=11)
fig.text(0.5, 0.012,
         "Design question: should the landing floor absorb the bounce (marble lands dead, rolls straight out) or accept the sideways wobble?\n"
         "Floor outline is schematic (smoothstep, 15 mm over 37 mm, from commit cbcbdd7). Dashed lines are 60 mm cell edges. Dot colour = time, sim at 240 Hz.",
         color=FG, ha="center", fontsize=10)
fig.savefig(OUT, dpi=110, facecolor=BG)
print("landing", bad["land"], bad["z"][bad["land"]], good["land"], good["z"][good["land"]])
