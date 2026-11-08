#!/usr/bin/env python3
"""P1-T6: exit 0 when the central 20 % of a cat's eye shot shows its green vane.

The wood floor and tray never have green above red, so more than 2 % of centre pixels with
g > r + 30 means the vane (fallback shader or transmission) rendered. Local only (needs Pillow).
"""
import sys

from PIL import Image

img = Image.open(sys.argv[1]).convert("RGB")
w, h = img.size
cw, ch = w // 5, h // 5
box = img.crop(((w - cw) // 2, (h - ch) // 2, (w + cw) // 2, (h + ch) // 2))
raw = box.tobytes()
n = len(raw) // 3
share = sum(1 for i in range(0, len(raw), 3) if raw[i + 1] > raw[i] + 30) / n
print(f"vane-check {sys.argv[1]}: green share {share:.3f}")
sys.exit(0 if share > 0.02 else 1)
