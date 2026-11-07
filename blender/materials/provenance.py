"""Provenance for the P1-T1 wood set (stdlib only).

python3 blender/materials/provenance.py assemble  -> writes assets/materials/provenance.json from the
    build reports in target/materials-stage (bake, compress, seam). Refuses if the seam check failed.
python3 blender/materials/provenance.py check     -> exit 0 only if every recorded hash and byte count
    matches disk, every KTX2 passes `ktx validate`, required fields exist and validation.pass is true.
"""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
STAGE = os.path.join(ROOT, "target/materials-stage")
OUT = os.path.join(ROOT, "assets/materials/provenance.json")
KTX = os.path.join(ROOT, "target/tools/ktx-4.4.2")
BIN = os.path.join(KTX, "KTX-Software-4.4.2-Darwin-arm64-tools.pkg/Payload/usr/local/bin/ktx")
LIB = os.path.join(KTX, "KTX-Software-4.4.2-Darwin-arm64-library.pkg/Payload/usr/local/lib")
GENERATOR_FILES = ["blender/materials/generate.py", "blender/materials/compress.py", "blender/materials/check.py",
                   "blender/materials/provenance.py", "blender/materials/review.py", "scripts/fetch-ktx.sh"]
REQUIRED = ["setId", "task", "route", "generator", "generatorDeps", "authoringAgent", "blender", "toolVersions", "bake",
            "contract", "variants", "compression", "derivedFiles", "references", "license", "validation", "approvals"]


def sha256(rel: str) -> str:
    with open(os.path.join(ROOT, rel), "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def rec(rel: str) -> dict:
    return {"path": rel, "sha256": sha256(rel), "bytes": os.path.getsize(os.path.join(ROOT, rel))}


def load(name: str) -> dict:
    with open(os.path.join(STAGE, name)) as f:
        return json.load(f)


def assemble() -> None:
    bake, comp, seam = load("bake-report.json"), load("compress-report.json"), load("seam-report.json")
    if not seam["pass"] or not seam["negativeControl"]["failsAsExpected"]:
        sys.exit("provenance.py: seam check did not pass; refusing to assemble")
    variants = {}
    for name, v in bake["variants"].items():
        variants[name] = {
            "params": v["params"],
            "stats": v["stats"],
            "ultraSource": {ch: {"sizePx": f["size"], "sha256": f["sha256"], "bytes": f["bytes"], "storage": "target/materials-stage (rebuilt, not committed)"}
                            for ch, f in v["files"]["stage"].items()},
        }
    derived = [rec(e["path"]) | {"channel": e["channel"], "family": e["family"], "format": e["format"], "transfer": e["transfer"]}
               for e in comp["textureEncodes"]]
    refs = ["docs/reference/ref-02.webp", "docs/reference/ref-03.webp", "assets-source/lookdev/01-macro-channel-cats-eye.png",
            "assets-source/lookdev/04-tower-track-wide.png", "assets-source/lookdev/07-wave-channel-opal-marble.png"]
    prov = {
        "schema": "parble-material-provenance/1",
        "setId": "wood",
        "task": "P1-T1 procedural wood material set",
        "version": 1,
        "route": "scripted",
        "generator": rec("blender/materials/generate.py"),
        "generatorDeps": [rec(p) for p in GENERATOR_FILES[1:]],
        "blend": rec(bake["blend"]["path"]) | {"note": "node source saved by generate.py; rebuilt by every run"},
        "authoringAgent": {"harness": "claude-code", "model": "claude-opus-5-5"},
        "blender": bake["blender"],
        "blenderBuildHash": bake["blenderHash"],
        "toolVersions": {"ktxSoftware": comp["ktxSoftware"], "python": "Blender bundled + system python3 (stdlib only)"},
        "bake": bake["bake"],
        "contract": {
            "tileM": bake["tileM"],
            "texelDensityPxPerM": bake["texelDensityPxPerM"],
            "runtimePx": bake["runtimePx"],
            "tiers": "High = shipped 1024 mip 0; Low = mip 1; Ultra = 2048 bake (target/materials-stage, rebuild with make materials-build)",
            "normalConvention": bake["normalConvention"],
            "ormPacking": bake["ormPacking"],
            "heightAmplitudeM": bake["heightAmplitudeM"],
            "aoStrength": bake["aoStrength"],
            "aoNote": "faint latewood cavity term only; per-piece AO is baked per piece in P1-T2 (kit-contract §12)",
            "colorSpaces": {"base_color": "sRGB", "normal": "linear", "orm": "linear"},
            "wrap": "every map is a function of the UV torus (4D noise of cos/sin 2*pi*u, 2*pi*v) and integer ring counts",
            "variantSelection": "each piece instance picks a variant by a hash of its anchor cell (kit-contract §12)",
        },
        "variants": variants,
        "compression": comp["settings"] | {"ktxSoftware": comp["ktxSoftware"]},
        "derivedFiles": derived,
        "reviewFiles": [rec("blender/materials/review/side-by-side.png"), rec("blender/materials/review/tiling-3x3.png")],
        "references": [rec(r) for r in refs],
        "license": "MIT (project); procedural, no third-party texture inputs. References are style direction only.",
        "validation": {
            "pass": True,
            "checks": [
                f"seam + mirror check: {seam['files']} runtime PNGs and {seam['mipFiles']} UASTC mip-1 extracts, {seam['failures']} failures",
                f"negative control (non-periodic crop) fails as expected: seam x={seam['negativeControl']['seamX']} y={seam['negativeControl']['seamY']}",
                "ktx validate + vkFormat/ZSTD/full-mip assertions on every KTX2 (compress.py)",
            ],
            "seam": seam["rows"],
        },
        "approvals": [{"who": "Paul", "gate": "owner look approval vs ref-02 (side-by-side.png)", "status": "pending"}],
    }
    with open(OUT, "w") as f:
        json.dump(prov, f, indent=2)
        f.write("\n")
    print(f"provenance.py: wrote {os.path.relpath(OUT, ROOT)} ({len(derived)} derived files)")


def check() -> None:
    errors: list[str] = []
    if not os.path.exists(OUT):
        sys.exit("provenance.py: assets/materials/provenance.json missing")
    with open(OUT) as f:
        prov = json.load(f)
    for k in REQUIRED:
        if prov.get(k) in (None, "", [], {}):
            errors.append(f"missing field {k}")
    recs = [prov["generator"], prov["blend"], *prov["generatorDeps"], *prov["derivedFiles"], *prov["reviewFiles"], *prov["references"]]
    for r in recs:
        p = os.path.join(ROOT, r["path"])
        if not os.path.exists(p):
            errors.append(f"{r['path']}: missing on disk")
        elif sha256(r["path"]) != r["sha256"] or os.path.getsize(p) != r["bytes"]:
            errors.append(f"{r['path']}: hash or size differs from provenance")
    deps = {d["path"] for d in prov["generatorDeps"]}
    for g in GENERATOR_FILES[1:]:
        if g not in deps:
            errors.append(f"generatorDeps lacks {g}")
    on_disk = {os.path.relpath(os.path.join(dp, f), ROOT) for dp, _, fs in os.walk(os.path.join(ROOT, "assets/materials"))
               for f in fs if f.endswith((".png", ".ktx2"))}
    recorded = {d["path"] for d in prov["derivedFiles"]}
    for extra in sorted(on_disk - recorded):
        errors.append(f"{extra}: runtime file not in provenance")
    for d in prov["derivedFiles"]:
        if d["path"].endswith(".ktx2"):
            r = subprocess.run([BIN, "validate", os.path.join(ROOT, d["path"])], env={**os.environ, "DYLD_LIBRARY_PATH": LIB},
                               capture_output=True, text=True)
            if r.returncode != 0:
                errors.append(f"{d['path']}: ktx validate exit {r.returncode}: {r.stdout.strip()} {r.stderr.strip()}")
    if prov["validation"].get("pass") is not True:
        errors.append("validation.pass is not true")
    for e in errors:
        print(f"FAIL {e}")
    print(f"provenance.py: {len(recs)} hashed records, {sum(d['path'].endswith('.ktx2') for d in prov['derivedFiles'])} KTX2 validated, {len(errors)} error(s)")
    sys.exit(1 if errors else 0)


if __name__ == "__main__":
    {"assemble": assemble, "check": check}.get(sys.argv[1] if len(sys.argv) > 1 else "", lambda: sys.exit("usage: provenance.py assemble|check"))()
