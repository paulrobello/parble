"""Build the P5-T2 key-art runtime derivatives from the immutable sources.

Usage: python3 -I assets-source/keyart/derive.py
Reads the sources in assets-source/keyart/, writes assets/keyart/*, and rewrites the
"derivatives" array of assets-source/keyart/provenance.json. Sources are never modified.
Requires ImageMagick (magick), cwebp and avifenc on PATH, and Pillow.
"""

from __future__ import annotations

import hashlib
import json
import subprocess
from pathlib import Path

from PIL import Image

SRC = Path(__file__).resolve().parent
ROOT = SRC.parent.parent
OUT = ROOT / "assets" / "keyart"

WEBP_Q = 82
AVIF_Q = 60
JPEG_Q = 88

MODERN = ["avif", "webp"]

# PNG fallbacks are built at one size per surface, chosen to stay under the repo's
# 1000 KB check-added-large-files limit for paths outside assets-source/.
# Each crop is (x, y, w, h) in source pixels. Zones are fractions of the derivative frame:
# (x0, y0, x1, y1). "title" must stay free for the wordmark, "ui" for buttons/progress.
CROPS: list[dict] = [
    {
        "name": "hero-21x9",
        "source": "hero-wide-21x9.png",
        "crop": (0, 0, 6336, 2688),
        "sizes": [(2560, 1086, MODERN), (1280, 543, MODERN + ["png"])],
        "zones": {"title": (0.30, 0.04, 0.70, 0.22), "ui": (0.30, 0.86, 0.70, 0.97)},
    },
    {
        "name": "hero-16x9",
        "source": "hero-wide-21x9.png",
        "crop": (1127, 0, 4779, 2688),
        "sizes": [(2560, 1440, MODERN), (1280, 720, MODERN + ["png"])],
        "zones": {"title": (0.25, 0.04, 0.75, 0.22), "ui": (0.25, 0.86, 0.75, 0.97)},
    },
    {
        "name": "hero-portrait-9x16",
        "source": "hero-portrait-9x16.png",
        "crop": (0, 10, 1536, 2731),
        "sizes": [(1080, 1920, MODERN), (720, 1280, MODERN), (648, 1152, ["png"])],
        "zones": {"title": (0.10, 0.04, 0.90, 0.20), "ui": (0.10, 0.88, 0.90, 0.97)},
    },
    {
        "name": "loading-16x9",
        "source": "loading-16x9.png",
        # Rows 1354+ of the source are a near-black letterbox band from the provider; cut above it.
        "crop": (363, 0, 2389, 1344),
        "sizes": [(1920, 1080, MODERN), (1280, 720, MODERN), (1152, 648, ["png"])],
        "zones": {"title": (0.04, 0.55, 0.34, 0.78), "ui": (0.04, 0.86, 0.34, 0.94)},
    },
    {
        "name": "social-card",
        "source": "hero-wide-21x9.png",
        "crop": (957, 0, 5120, 2688),
        "sizes": [(1200, 630, ["png", "jpg"])],
        "zones": {"title": (0.25, 0.04, 0.75, 0.22)},
    },
]


def sha256(p: Path) -> str:
    return hashlib.sha256(p.read_bytes()).hexdigest()


def run(cmd: list[str]) -> None:
    subprocess.run(cmd, check=True, capture_output=True)


def zone_luma(img: Image.Image, z: tuple[float, float, float, float]) -> float:
    w, h = img.size
    region = img.convert("L").crop((round(z[0] * w), round(z[1] * h), round(z[2] * w), round(z[3] * h)))
    hist = region.histogram()
    total = sum(hist)
    return round(sum(i * c for i, c in enumerate(hist)) / total / 255, 4)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    for stale in OUT.iterdir():
        stale.unlink()
    tmp = SRC / ".derive-tmp"
    tmp.mkdir(exist_ok=True)
    entries: list[dict] = []
    for c in CROPS:
        src = SRC / c["source"]
        x, y, w, h = c["crop"]
        with Image.open(src) as im:
            assert x + w <= im.width and y + h <= im.height, f"{c['name']}: crop exceeds source"
        for tw, th, formats in c["sizes"]:
            assert tw <= w and th <= h, f"{c['name']}: {tw}x{th} would upscale a {w}x{h} crop"
            master = tmp / f"{c['name']}-{tw}x{th}.png"
            run(["magick", str(src), "-crop", f"{w}x{h}+{x}+{y}", "+repage", "-colorspace", "sRGB",
                 "-filter", "Lanczos", "-resize", f"{tw}x{th}!", "-strip", f"PNG24:{master}"])
            with Image.open(master) as m:
                zones = {k: zone_luma(m, z) for k, z in c["zones"].items()}
            for fmt in formats:
                out = OUT / f"{c['name']}-{tw}x{th}.{fmt}"
                if fmt == "png":
                    run(["magick", str(master), "-strip", "-define", "png:compression-level=9", f"PNG24:{out}"])
                    tool = "magick (Lanczos resize, PNG24, compression-level 9)"
                elif fmt == "jpg":
                    run(["magick", str(master), "-strip", "-quality", str(JPEG_Q), "-sampling-factor", "4:2:0", str(out)])
                    tool = f"magick (JPEG q{JPEG_Q}, 4:2:0)"
                elif fmt == "webp":
                    run(["cwebp", "-quiet", "-q", str(WEBP_Q), "-m", "6", "-metadata", "none", str(master), "-o", str(out)])
                    tool = f"cwebp 1.6.0 -q {WEBP_Q} -m 6"
                else:
                    run(["avifenc", "-q", str(AVIF_Q), "-s", "6", "-y", "420", str(master), str(out)])
                    tool = f"avifenc 1.4.2 (aom 3.15.3) -q {AVIF_Q} -s 6 -y 420"
                # Prove the encoded file decodes back at the expected size.
                probe = subprocess.run(["magick", "identify", "-format", "%w %h", str(out)],
                                       check=True, capture_output=True, text=True).stdout.split()
                assert (int(probe[0]), int(probe[1])) == (tw, th), f"{out.name}: decoded {probe}"
                entries.append({
                    "file": f"assets/keyart/{out.name}",
                    "surface": c["name"],
                    "source": f"assets-source/keyart/{c['source']}",
                    "sourceSha256": sha256(src),
                    "crop": {"x": x, "y": y, "width": w, "height": h},
                    "format": fmt,
                    "width": tw,
                    "height": th,
                    "bytes": out.stat().st_size,
                    "sha256": sha256(out),
                    "tool": f"magick crop {w}x{h}+{x}+{y}, Lanczos resize to {tw}x{th}, sRGB, metadata stripped; then {tool}",
                    "decodeCheck": "magick identify decoded the file at the recorded size",
                    "safeZones": {k: list(z) for k, z in c["zones"].items()},
                    "zoneMeanLuma": zones,
                })
            master.unlink()
    tmp.rmdir()
    prov_path = SRC / "provenance.json"
    prov = json.loads(prov_path.read_text())
    prov["derivatives"] = entries
    prov_path.write_text(json.dumps(prov, indent=2) + "\n")
    for e in entries:
        print(f"{e['file']} {e['width']}x{e['height']} {e['bytes']}B zones={e['zoneMeanLuma']}")


if __name__ == "__main__":
    main()
