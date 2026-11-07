"""Runtime PNG -> UASTC -> BC7 + ASTC KTX2 (zstd 18, full mip chain, wrap-mode mips) via pinned KTX-Software 4.4.2.

Usage: python3 blender/materials/compress.py   (run from the repo root after generate.py; stdlib only)
Validates every output with `ktx validate` and asserts vkFormat, supercompression and level count from
`ktx info`. Extracts mip 1 of each UASTC file to target/materials-stage/mips/ for the seam check.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
KTX = os.path.join(ROOT, "target/tools/ktx-4.4.2")
BIN = os.path.join(KTX, "KTX-Software-4.4.2-Darwin-arm64-tools.pkg/Payload/usr/local/bin")
LIB = os.path.join(KTX, "KTX-Software-4.4.2-Darwin-arm64-library.pkg/Payload/usr/local/lib")
RUNTIME = os.path.join(ROOT, "assets/materials")
STAGE = os.path.join(ROOT, "target/materials-stage")
# Per-channel transfer: base colour is sRGB, everything else is linear data.
CHANNELS = {
    "base_color": {"format": "R8G8B8A8_SRGB", "tf": "srgb", "extra": []},
    "normal": {"format": "R8G8B8A8_UNORM", "tf": "linear", "extra": ["--normalize"]},
    "orm": {"format": "R8G8B8A8_UNORM", "tf": "linear", "extra": []},
}
FAMILIES = {"bc7": {"BC7_SRGB_BLOCK", "BC7_UNORM_BLOCK"}, "astc": {"ASTC_4x4_SRGB_BLOCK", "ASTC_4x4_UNORM_BLOCK"}}
ENV = {**os.environ, "PATH": f"{BIN}:{os.environ.get('PATH', '')}", "DYLD_LIBRARY_PATH": LIB}


def ktx(*args: str) -> str:
    r = subprocess.run([os.path.join(BIN, "ktx"), *args], env=ENV, capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(f"compress.py: ktx {' '.join(args)} failed ({r.returncode}):\n{r.stdout}\n{r.stderr}")
    return r.stdout


def sha256(path: str) -> str:
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def info(path: str) -> dict:
    out = ktx("info", path)
    fmt = re.search(r"vkFormat:\s*VK_FORMAT_(\S+)", out)
    sc = re.search(r"supercompressionScheme:\s*KTX_SS_(\S+)", out)
    lv = re.search(r"levelCount:\s*(\d+)", out)
    w = re.search(r"pixelWidth:\s*(\d+)", out)
    tf = re.search(r"transfer(?:Function)?:\s*KHR_DF_TRANSFER_(\S+)", out)
    return {
        "vkFormat": fmt.group(1) if fmt else None,
        "supercompression": sc.group(1) if sc else None,
        "levels": int(lv.group(1)) if lv else None,
        "width": int(w.group(1)) if w else None,
        "transfer": tf.group(1) if tf else None,
    }


def main() -> None:
    if not os.path.exists(os.path.join(BIN, "ktx")):
        sys.exit("compress.py: KTX-Software missing; run make fetch-tools")
    version = ktx("--version").strip()
    if "v4.4.2" not in version:
        sys.exit(f"compress.py: expected KTX-Software v4.4.2, got {version}")
    os.makedirs(os.path.join(STAGE, "ktx"), exist_ok=True)
    os.makedirs(os.path.join(STAGE, "mips"), exist_ok=True)
    encodes = []
    variants = sorted(d for d in os.listdir(RUNTIME) if os.path.isdir(os.path.join(RUNTIME, d)) and d.startswith("wood_"))
    if not variants:
        sys.exit("compress.py: no variants under assets/materials; run generate.py first")
    for v in variants:
        for ch, c in CHANNELS.items():
            png = os.path.join(RUNTIME, v, f"{v}.{ch}.png")
            uastc = os.path.join(STAGE, "ktx", f"{v}.{ch}.uastc.ktx2")
            ktx("create", "--format", c["format"], "--assign-tf", c["tf"], *c["extra"], "--encode", "uastc",
                "--uastc-quality", "2", "--zstd", "18", "--generate-mipmap", "--mipmap-wrap", "wrap",
                "--testrun", png, uastc)
            mip1 = os.path.join(STAGE, "mips", f"{v}.{ch}.mip1.png")
            if os.path.exists(mip1):
                os.remove(mip1)
            ktx("extract", "--level", "1", "--transcode", "rgba8", uastc, mip1)
            encodes.append({"variant": v, "channel": ch, "family": "png", "format": "PNG RGBA8",
                            "transfer": c["tf"], "path": os.path.relpath(png, ROOT), "sha256": sha256(png), "bytes": os.path.getsize(png)})
            for fam, fmts in FAMILIES.items():
                out = os.path.join(RUNTIME, v, f"{v}.{ch}.{fam}.ktx2")
                ktx("transcode", "--target", fam, "--zstd", "18", uastc, out)
                ktx("validate", out)
                i = info(out)
                want_srgb = c["tf"] == "srgb"
                if i["vkFormat"] not in fmts or ("SRGB" in i["vkFormat"]) != want_srgb:
                    sys.exit(f"compress.py: {out} vkFormat {i['vkFormat']} wrong for {ch}")
                if i["supercompression"] != "ZSTD":
                    sys.exit(f"compress.py: {out} supercompression {i['supercompression']} != ZSTD")
                if i["levels"] != i["width"].bit_length():
                    sys.exit(f"compress.py: {out} has {i['levels']} levels, want full chain for {i['width']}")
                encodes.append({"variant": v, "channel": ch, "family": fam, "format": i["vkFormat"], "transfer": c["tf"],
                                "levels": i["levels"], "supercompression": i["supercompression"],
                                "path": os.path.relpath(out, ROOT), "sha256": sha256(out), "bytes": os.path.getsize(out)})
                print(f"ok   {os.path.relpath(out, ROOT)} {i['vkFormat']} {i['levels']} levels {os.path.getsize(out)} B")
    report = {
        "ktxSoftware": version,
        "settings": {"encode": "uastc", "uastcQuality": 2, "zstd": 18, "mipmap": "generated, filter lanczos4, wrap",
                     "normal": "--normalize", "transcode": ["bc7", "astc"], "testrun": True},
        "textureEncodes": encodes,
    }
    with open(os.path.join(STAGE, "compress-report.json"), "w") as f:
        json.dump(report, f, indent=2)
    total = {fam: sum(e["bytes"] for e in encodes if e["family"] == fam) for fam in ("bc7", "astc", "png")}
    print(f"compress.py: {len(encodes)} files, bytes {total}")


main()
