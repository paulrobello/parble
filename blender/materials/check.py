"""Seamless-tiling gate for the P1-T1 wood set. Exit 0 = pass, 1 = fail.

Run: Blender -b --factory-startup --python-exit-code 1 -P blender/materials/check.py
Checks every runtime PNG (assets/materials/wood_*/*.png) and every mip-1 extract of the UASTC files
(target/materials-stage/mips/*.png):
  seam ratio  = mean |last column - first column| over the mean adjacent-column difference of the 16
                columns either side of the wrap; same for rows. A wrapping tile scores about 1.0 and a
                cut seam far above. Pass when the wrap is no worse than the worst
                interior boundary at the same 4-px block phase (or <= SEAM_SOFT), and never above SEAM_MAX.
  mirror corr = |Pearson correlation| of the luminance with its own horizontal and vertical mirror.
                Low-frequency torus noise can mirror the tile (kaleidoscope); pass <= MIRROR_MAX.
A non-periodic crop of the first base colour is a negative control and must FAIL the seam test,
otherwise the check itself is broken. Writes blender/materials/review/tiling-3x3.png.
"""

from __future__ import annotations

import glob
import os
import sys

import bpy
import numpy as np

SEAM_SOFT = 1.5  # always acceptable: the interior per-column ratio itself spans about 0.6-1.6
SEAM_MAX = 2.0  # never acceptable, even if an interior boundary is worse
MIRROR_MAX = 0.10
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
REVIEW = os.path.join(os.path.dirname(os.path.abspath(__file__)), "review")


def load(path: str) -> np.ndarray:
    img = bpy.data.images.load(path, check_existing=False)
    img.colorspace_settings.name = "Non-Color"
    w, h = img.size
    px = np.empty(w * h * 4, dtype=np.float32)
    img.pixels.foreach_get(px)
    bpy.data.images.remove(img)
    return px.reshape(h, w, 4)[:, :, :3].astype(np.float64)


def seam(a: np.ndarray) -> tuple[float, float, float, float]:
    """Per axis: (wrap ratio, worst interior ratio at the same 4-px block phase).

    ratio(j) = |col j+1 - col j| over the mean of the 16 column differences either side of it. A local
    baseline matters because grain density varies across the tile. The wrap is compared with interior
    boundaries at the same position mod 4, because block-compressed mips put a 4x4 block edge on it.
    """

    def axis(b: np.ndarray) -> tuple[float, float]:
        d = np.abs(np.roll(b, -1, axis=1) - b).mean(axis=(0, 2))  # d[i] = |col i+1 - col i|; d[-1] is the wrap
        n = d.size
        idx = np.arange(n)
        ratios = np.empty(n)
        for j in range(n):
            nb = np.concatenate([idx[j - 16 : j] if j >= 16 else np.r_[idx[j - 16 :], idx[:j]], (idx[j + 1 : j + 17] % n) if j + 17 <= n else np.r_[idx[j + 1 :], idx[: (j + 17) - n]]])
            ratios[j] = d[j] / max(d[nb].mean(), 1e-9)
        same_phase = ratios[(idx % 4 == (n - 1) % 4) & (idx != n - 1)]
        return float(ratios[-1]), float(same_phase.max())

    x, xmax = axis(a)
    y, ymax = axis(np.swapaxes(a, 0, 1))
    return x, y, xmax, ymax


def mirror(a: np.ndarray) -> tuple[float, float]:
    lum = a.mean(axis=-1)
    lum = lum - lum.mean()
    if lum.std() < 1e-6:
        return 0.0, 0.0
    cx = np.corrcoef(lum.ravel(), lum[:, ::-1].ravel())[0, 1]
    cy = np.corrcoef(lum.ravel(), lum[::-1, :].ravel())[0, 1]
    return abs(float(cx)), abs(float(cy))


def write_png(path: str, rgb01: np.ndarray) -> None:
    h, w = rgb01.shape[:2]
    q = np.round(np.clip(rgb01, 0, 1) * 255.0) / 255.0
    rgba = np.concatenate([q, np.ones((h, w, 1))], axis=-1).astype(np.float32)
    img = bpy.data.images.new("PBM_check_out", w, h, alpha=True, float_buffer=False)
    img.colorspace_settings.name = "Non-Color"
    img.pixels.foreach_set(rgba.ravel())
    img.filepath_raw = path
    img.file_format = "PNG"
    img.save()
    bpy.data.images.remove(img)


def main() -> None:
    files = sorted(glob.glob(os.path.join(ROOT, "assets/materials/wood_*/*.png")))
    mips = sorted(glob.glob(os.path.join(ROOT, "target/materials-stage/mips/*.png")))
    if len(files) < 6:
        print(f"FAIL only {len(files)} runtime PNGs found; run make materials-build")
        sys.exit(1)
    if not mips:
        print("FAIL no mip-1 extracts; run blender/materials/compress.py")
        sys.exit(1)
    fails = 0
    rows: list[dict] = []
    tiles = []
    for path in files + mips:
        a = load(path)
        sx, sy, ix, iy = seam(a)
        mx, my = mirror(a)
        bad = sx > min(SEAM_MAX, max(ix, SEAM_SOFT)) or sy > min(SEAM_MAX, max(iy, SEAM_SOFT)) or mx > MIRROR_MAX or my > MIRROR_MAX
        fails += bad
        rel = os.path.relpath(path, ROOT)
        rows.append({"file": rel, "seamX": round(sx, 3), "seamY": round(sy, 3), "interiorMaxX": round(ix, 3),
                     "interiorMaxY": round(iy, 3), "mirrorX": round(mx, 3), "mirrorY": round(my, 3), "pass": not bad})
        print(f"{'FAIL' if bad else 'ok  '} {rel} {a.shape[1]}x{a.shape[0]} seam x={sx:.3f} (interior max {ix:.3f}) y={sy:.3f} (interior max {iy:.3f}) mirror x={mx:.3f} y={my:.3f}")
        if path in files and path.endswith(".base_color.png"):
            tiles.append(a)

    # Negative control: a crop that does not wrap must fail.
    base = load(files[0] if files[0].endswith(".base_color.png") else [f for f in files if f.endswith(".base_color.png")][0])
    crop = base[37:37 + 700, 101:101 + 700]
    cx, cy, _, _ = seam(crop)
    control_ok = cx > SEAM_MAX or cy > SEAM_MAX
    print(f"{'ok  ' if control_ok else 'FAIL'} negative control (non-periodic crop) seam x={cx:.3f} y={cy:.3f} must exceed {SEAM_MAX:.2f}")
    fails += not control_ok

    # Review sheet: each variant tiled 3x3 at 256 px per tile, side by side.
    os.makedirs(REVIEW, exist_ok=True)
    panels = []
    for a in tiles:
        n = a.shape[0] // 256
        small = a.reshape(256, n, 256, n, 3).mean(axis=(1, 3))
        panels.append(np.tile(small, (3, 3, 1)))
        panels.append(np.zeros((768, 16, 3)))
    write_png(os.path.join(REVIEW, "tiling-3x3.png"), np.concatenate(panels[:-1], axis=1))

    print(f"check.py: {len(files)} runtime + {len(mips)} mip files, {fails} failure(s)")
    import json

    with open(os.path.join(ROOT, "target/materials-stage/seam-report.json"), "w") as f:
        json.dump({"pass": fails == 0, "seamSoft": SEAM_SOFT, "seamMax": SEAM_MAX, "mirrorMax": MIRROR_MAX,
                   "files": len(files), "mipFiles": len(mips), "failures": int(fails), "rows": rows,
                   "negativeControl": {"seamX": round(cx, 3), "seamY": round(cy, 3), "failsAsExpected": bool(control_ok)}}, f, indent=2)
    sys.exit(1 if fails else 0)


main()
