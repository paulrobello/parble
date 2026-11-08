#!/usr/bin/env python3
"""P5-T1 glow bloom check: compare a close-up captured with bloom on and off.

Usage: marble-bloom-check.py <bloom-on.png> <bloom-off.png> [min_ring_gain]

The marble sits in the frame centre (the `closeup` scenario). Its silhouette is the set of pixels
that differ from the bloom-off frame's background by more than the backdrop does, approximated as
the central disc whose radius is found from the bloom-off frame. Bloom spreads the emissive core's
light past that silhouette, so the mean luminance gain (on minus off) in a ring 1.1-1.6 radii out is
the halo. Exits 1 unless the ring gain is at least `min_ring_gain` (default 2.0, in 0-255 units).
"""
import sys

import numpy as np
from PIL import Image


def lum(path: str) -> np.ndarray:
    a = np.asarray(Image.open(path).convert("RGB"), dtype=np.float32)
    return 0.2126 * a[..., 0] + 0.7152 * a[..., 1] + 0.0722 * a[..., 2]


def main() -> int:
    on, off = lum(sys.argv[1]), lum(sys.argv[2])
    need = float(sys.argv[3]) if len(sys.argv) > 3 else 2.0
    h, w = off.shape
    cy, cx = h / 2.0, w / 2.0
    yy, xx = np.mgrid[0:h, 0:w]
    d = np.hypot(yy - cy, xx - cx)
    # Silhouette radius: where the bloom-off frame leaves the centre's colour, scanning outward.
    bg = np.median(off[d > 0.45 * h])
    radius = 0.0
    for r in np.arange(4.0, 0.45 * h, 1.0):
        ring = off[(d >= r) & (d < r + 1.0)]
        if abs(np.median(ring) - bg) < 6.0:
            radius = r
            break
    if radius == 0.0:
        print("bloom-check: could not find the marble silhouette")
        return 1
    ring = (d >= 1.1 * radius) & (d < 1.6 * radius)
    far = d > 0.45 * h
    gain = float((on - off)[ring].mean())
    far_gain = float((on - off)[far].mean())
    ok = gain >= need and gain > 2.0 * max(far_gain, 0.0)
    print(
        f"bloom-check: silhouette r={radius:.0f}px ring gain={gain:.2f} far gain={far_gain:.2f} "
        f"need>={need} ok={ok}"
    )
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
