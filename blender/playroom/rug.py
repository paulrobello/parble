#!/usr/bin/env python3
"""P1-T5 rug material: a seamless square-in-square woven rug (ref-07, ref-08), scripted route.

Every map is a function of the pixel position modulo the tile, built from an integer cell grid and
periodic lattice noise (random lattices sampled with wrap-around), so the tile repeats by
construction. Deterministic: a fixed seed, numpy and Pillow only.

Usage: python3 blender/playroom/rug.py <out_dir>
Writes rug.base_color.png (sRGB) and rug.normal.png (linear, OpenGL +Y) at SIZE x SIZE, plus
rug.height.png (linear, review only) and rug.tiling-3x3.png (review only).
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image

SEED = 707
SIZE = 1024
CELLS = 16  # cells per tile edge
TILE_M = 0.64  # one tile in metres: 16 x 4 cm squares
LINE = 0.07  # grid line width as a fraction of a cell
INSET = 0.24  # inner square inset as a fraction of a cell
NORMAL_STRENGTH = 3.0

# sRGB albedo palette sampled by eye from ref-07 / ref-08 under neutral light.
PALETTE = np.array(
    [
        [196, 176, 40],  # ochre yellow
        [110, 150, 48],  # moss green
        [36, 112, 108],  # teal
        [170, 140, 36],  # amber ochre
        [140, 160, 50],  # olive
        [50, 124, 84],  # green teal
    ],
    dtype=np.float64,
) / 255.0
LINE_RGB = np.array([34, 66, 104], dtype=np.float64) / 255.0
FLECK_RGB = np.array([[220, 196, 70], [40, 90, 130], [90, 150, 60]], dtype=np.float64) / 255.0


def srgb_to_linear(c: np.ndarray) -> np.ndarray:
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def linear_to_srgb(c: np.ndarray) -> np.ndarray:
    c = np.clip(c, 0.0, 1.0)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * c ** (1 / 2.4) - 0.055)


def periodic_noise(rng: np.random.Generator, period: int, size: int) -> np.ndarray:
    """Smooth value noise with `period` lattice cells across the tile, wrapping at the edges."""
    lat = rng.random((period, period))
    t = np.arange(size) * period / size
    i0 = np.floor(t).astype(int)
    f = t - i0
    f = f * f * (3 - 2 * f)
    i1 = (i0 + 1) % period
    a = lat[np.ix_(i0, i0)]
    b = lat[np.ix_(i0, i1)]
    c = lat[np.ix_(i1, i0)]
    d = lat[np.ix_(i1, i1)]
    fy = f[:, None]
    fx = f[None, :]
    return (a * (1 - fx) + b * fx) * (1 - fy) + (c * (1 - fx) + d * fx) * fy


def fbm(rng: np.random.Generator, base: int, octaves: int, size: int) -> np.ndarray:
    out = np.zeros((size, size))
    amp, total = 1.0, 0.0
    for o in range(octaves):
        out += amp * periodic_noise(rng, base * 2**o, size)
        total += amp
        amp *= 0.55
    return out / total


def build() -> dict[str, np.ndarray]:
    rng = np.random.default_rng(SEED)
    px = SIZE // CELLS
    yy, xx = np.mgrid[0:SIZE, 0:SIZE]
    cy, cx = yy // px, xx // px
    # Position inside the cell in [0, 1), and distance to the cell border (0 at the edge, 0.5 centre).
    u = (xx % px + 0.5) / px
    v = (yy % px + 0.5) / px
    edge = np.minimum(np.minimum(u, 1 - u), np.minimum(v, 1 - v))

    outer = rng.integers(0, len(PALETTE), (CELLS, CELLS))
    inner = (outer + rng.integers(1, len(PALETTE), (CELLS, CELLS))) % len(PALETTE)
    shade = 0.82 + 0.3 * rng.random((CELLS, CELLS))

    lin_pal = srgb_to_linear(PALETTE)
    wobble = fbm(rng, 64, 3, SIZE) - 0.5
    col = np.where(
        (edge + 0.06 * wobble > INSET)[..., None], lin_pal[inner[cy, cx]], lin_pal[outer[cy, cx]]
    )
    col *= shade[cy, cx][..., None]

    # Woven pile: mid-scale mottling plus fine fibre speckle in two flecked colours.
    mottle = fbm(rng, 16, 4, SIZE)
    fibre = fbm(rng, 256, 2, SIZE)
    col *= ((0.6 + 0.8 * mottle) * (0.75 + 0.5 * fibre))[..., None]
    fleck = rng.random((SIZE, SIZE))
    which = rng.integers(0, len(FLECK_RGB), (SIZE, SIZE))
    fleck_mask = (fleck > 0.72)[..., None]
    col = np.where(fleck_mask, 0.7 * col + 0.3 * srgb_to_linear(FLECK_RGB)[which], col)

    # Grid lines (dark blue) with a soft woven edge.
    line_w = LINE / 2 + 0.012 * (mottle - 0.5)
    line_mask = np.clip((line_w - edge) / 0.012 + 0.5, 0, 1)
    col = col * (1 - line_mask[..., None]) + srgb_to_linear(LINE_RGB) * (0.8 + 0.4 * fibre)[
        ..., None
    ] * line_mask[..., None]

    height = 0.55 * (1 - line_mask) + 0.25 * fibre + 0.2 * mottle
    return {"base_color_linear": col, "height": height}


def normal_from_height(h: np.ndarray) -> np.ndarray:
    dx = (np.roll(h, -1, axis=1) - np.roll(h, 1, axis=1)) * 0.5
    dy = (np.roll(h, -1, axis=0) - np.roll(h, 1, axis=0)) * 0.5
    # OpenGL convention: +Y is image up, rows run down the image.
    n = np.stack([-dx * NORMAL_STRENGTH, dy * NORMAL_STRENGTH, np.ones_like(h)], axis=-1)
    n /= np.linalg.norm(n, axis=-1, keepdims=True)
    return n


def to_u8(a: np.ndarray) -> np.ndarray:
    return np.clip(np.round(a * 255.0), 0, 255).astype(np.uint8)


def main() -> None:
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    maps = build()
    base = to_u8(linear_to_srgb(maps["base_color_linear"]))
    Image.fromarray(base, "RGB").save(out / "rug.base_color.png", optimize=True)
    h = maps["height"]
    hn = (h - h.min()) / (h.max() - h.min())
    Image.fromarray(to_u8(hn), "L").save(out / "rug.height.png", optimize=True)
    n = normal_from_height(hn)
    Image.fromarray(to_u8(n * 0.5 + 0.5), "RGB").save(out / "rug.normal.png", optimize=True)
    Image.fromarray(np.tile(base, (3, 3, 1)), "RGB").resize((1536, 1536), Image.LANCZOS).save(
        out / "rug.tiling-3x3.png", optimize=True
    )
    # Seam check: the wrap-around step must look like any interior step.
    b = base.astype(np.float64)
    wrap = np.abs(b[:, 0] - b[:, -1]).mean() + np.abs(b[0] - b[-1]).mean()
    inner = np.abs(np.diff(b, axis=1)).mean() + np.abs(np.diff(b, axis=0)).mean()
    meta = {
        "seed": SEED,
        "sizePx": SIZE,
        "cellsPerTile": CELLS,
        "tileM": TILE_M,
        "seamWrapMeanStep": round(float(wrap), 3),
        "interiorMeanStep": round(float(inner), 3),
    }
    (out / "rug.meta.json").write_text(json.dumps(meta, indent=1) + "\n")
    print(json.dumps(meta))


if __name__ == "__main__":
    main()
