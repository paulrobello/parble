# /// script
# requires-python = ">=3.11"
# dependencies = ["cairosvg", "pillow"]
# ///
"""Render the favicon suite (P5-T3) from web/favicon/favicon.svg: PNGs at 16, 32, 48, 180 (apple
touch), 192 and 512, and a 16+32+48 favicon.ico. Built with the favicon-generator skill's renderer
(cairosvg). Usage, from the repo root, with the native cairo library (Homebrew: brew install cairo):
  DYLD_FALLBACK_LIBRARY_PATH=/opt/homebrew/lib uv run scripts/web/favicon.py
"""

from io import BytesIO
from pathlib import Path

import cairosvg
from PIL import Image

DIR = Path("web/favicon")
SVG = (DIR / "favicon.svg").read_bytes()


def render(size: int) -> Image.Image:
    png = cairosvg.svg2png(bytestring=SVG, output_width=size, output_height=size)
    return Image.open(BytesIO(png)).convert("RGBA")


def main() -> None:
    for size, name in [
        (16, "favicon-16x16.png"),
        (32, "favicon-32x32.png"),
        (48, "favicon-48x48.png"),
        (180, "apple-touch-icon.png"),
        (192, "favicon-192x192.png"),
        (512, "favicon-512x512.png"),
    ]:
        render(size).save(DIR / name, optimize=True)
    render(48).save(DIR / "favicon.ico", sizes=[(16, 16), (32, 32), (48, 48)])


if __name__ == "__main__":
    main()
