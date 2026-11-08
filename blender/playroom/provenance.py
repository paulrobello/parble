#!/usr/bin/env python3
"""P1-T5 playroom provenance: assemble or check assets-source/playroom/provenance.json.

python3 blender/playroom/provenance.py          # (re)write the record from the files on disk
python3 blender/playroom/provenance.py check    # fail if any recorded hash no longer matches

The generation record (provider, job, prompt, credits) is fixed data below; everything else is
hashed from the tree, so a rebuild that changes an output shows up as a check failure.
"""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "assets-source/playroom/provenance.json"

GENERATORS = [
    "blender/playroom/rug.py",
    "blender/playroom/encode.sh",
    "blender/playroom/blocks.py",
    "blender/playroom/probe.py",
    "blender/playroom/provenance.py",
]
SOURCES = [
    "assets-source/playroom/rug/rug.base_color.png",
    "assets-source/playroom/rug/rug.normal.png",
    "assets-source/playroom/rug/rug.height.png",
    "assets-source/playroom/backdrop/backdrop-01.png",
    "assets-source/playroom/backdrop/prompts/backdrop-01.txt",
]
RUNTIME = [
    "assets/playroom/layout.json",
    "assets/playroom/rug.base_color.ktx2",
    "assets/playroom/rug.normal.ktx2",
    "assets/playroom/backdrop.ktx2",
    "assets/playroom/env_specular.rgba16f",
    "assets/playroom/env_diffuse.rgba16f",
    "assets/playroom/env_probe.json",
    "assets/playroom/toy_cube.glb",
    "assets/playroom/toy_brick.glb",
    "assets/playroom/toy_plank.glb",
    "assets/playroom/toy_pillar.glb",
    "assets/playroom/toy_roof.glb",
    "assets/playroom/toy_arch.glb",
]

BACKDROP_GENERATION = {
    "file": "assets-source/playroom/backdrop/backdrop-01.png",
    "provider": "Higgsfield",
    "cli": "higgsfield 1.2.2",
    "model": "nano_banana_pro",
    "jobId": "4f8509a2-cb92-4bcb-ab9c-d426fd138b04",
    "jobStatus": "completed",
    "createdAt": "2026-11-08T01:38:57Z",
    "settings": {"aspect_ratio": "16:9", "resolution": "2k", "batch_size": 1},
    "seed": "not exposed by provider",
    "promptFile": "assets-source/playroom/backdrop/prompts/backdrop-01.txt",
    "promptNote": "submitted with 'childs' for 'child's' (shell quoting); otherwise identical to the prompt file",
    "negativePrompt": "none (negative guidance inline: no rug, lanterns, marbles, people, text, logos, watermark)",
    "referenceImages": [],
    "sourcePx": [2752, 1536],
    "runtimeDerivation": "ImageMagick resize to 1024x576, Gaussian blur sigma 3, RGBA8 sRGB KTX2, zstd 19, full mip chain (encode.sh)",
}


def sha(path: str) -> dict:
    data = (ROOT / path).read_bytes()
    return {"path": path, "sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data)}


def assemble() -> dict:
    return {
        "schema": "parble-playroom-provenance/1",
        "task": "P1-T5 night playroom environment",
        "date": "2026-11-07",
        "authoringAgent": {"harness": "claude-code", "model": "claude-opus-5-5"},
        "routes": {
            "rug": "create-tileable-pbr-materials, scripted procedural route (numpy, seeded); tileable by construction",
            "toyBlocks": "blender-kit-pieces headless generator (simple rigid render-only pieces, no LODs or colliders)",
            "backdrop": "create-game-key-art via the art skill, Higgsfield backend",
            "probe": "headless Blender Cycles cube-face bake + CPU GGX/Lambert prefilter (probe.py)",
        },
        "tools": {
            "blender": "5.2.2 LTS (headless, --factory-startup)",
            "ktxSoftware": "4.4.2 (pinned, scripts/fetch-ktx.sh)",
            "imagemagick": "7 (Homebrew)",
            "python": "system python3 with numpy and Pillow (rug.py); Blender bundled python (blocks.py, probe.py)",
        },
        "generations": {
            "total": 1,
            "cap": 10,
            "credits": 2,
            "creditEvidence": "Higgsfield balance 952 before, 950 after; one 'Nano Banana Pro -2 spend' at 2026-11-08 01:38 UTC",
            "rejected": [],
        },
        "rugContract": {
            "tileM": 0.64,
            "cellsPerTile": 16,
            "cellM": 0.04,
            "seed": 707,
            "sourcePx": 1024,
            "runtimePx": {"base_color": 512, "normal": 256},
            "channels": {"base_color": "sRGB", "normal": "linear, OpenGL +Y, tangent space"},
            "roughness": "constant 0.92 in the material (no ORM map)",
            "metallic": "0 (fabric)",
            "seamCheck": json.loads((ROOT / "assets-source/playroom/rug/rug.meta.json").read_text()),
            "review": "assets-source/playroom/rug/rug.tiling-3x3.png",
        },
        "probeContract": json.loads((ROOT / "assets/playroom/env_probe.json").read_text()),
        "backdrop": BACKDROP_GENERATION,
        "generators": [sha(p) for p in GENERATORS],
        "sources": [sha(p) for p in SOURCES],
        "runtime": [sha(p) for p in RUNTIME],
        "rights": "pending: generated on Paul's Higgsfield plus plan, commercial terms not verified; rug, blocks and probe are original scripted output",
        "approval": "pending Paul's review against ref-07 and ref-08 (docs/shots/p1-t5-vs-ref07-ref08.png)",
    }


def main() -> None:
    rec = assemble()
    if sys.argv[1:] == ["check"]:
        old = json.loads(OUT.read_text())
        bad = [
            e["path"]
            for key in ("generators", "sources", "runtime")
            for e, n in zip(old[key], rec[key])
            if e != n
        ]
        if bad:
            raise SystemExit(f"provenance mismatch: {bad}")
        print("playroom provenance: ok")
        return
    OUT.write_text(json.dumps(rec, indent=1) + "\n")
    print(f"wrote {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
