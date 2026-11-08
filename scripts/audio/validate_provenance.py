#!/usr/bin/env python3
"""Provenance reconciliation for the audio assets (validate-generated-assets: source versus
runtime separation, provenance and deterministic derivation, stale manifests).

Checks, against the committed tree and audition/sfx/manifest.json:

  1. every provenance row names an existing source file whose sha256 (and byte size, when
     recorded) matches the row
  2. every audio file under assets-source/audio has a provenance row (no unrecorded source)
  3. every derived row (derived_from) names a parent that exists, is itself a provenance row,
     and still has the recorded hash
  4. every manifest item traces to a non-rejected provenance row with the same source hash,
     and every provenance row that is not rejected has a manifest item (no stale manifest)
  5. rejected rows are in the manifest's rejected list, have no manifest item and no derivative
  6. every derivative on disk (ogg, m4a, master wav) is listed in the manifest with a matching
     sha256 (no orphan or edited derivative)
  7. every approved stem in docs/audio/audition-picks.json is a live manifest item, and every
     approved loop passes the master and Opus loop checks

Usage: scripts/audio/validate_provenance.py [--repo DIR]   (exit 1 on any failure)
"""

from __future__ import annotations

import argparse
import glob
import hashlib
import json
import sys
from pathlib import Path
from typing import Any

AUDIO_EXT = {".mp3", ".wav", ".flac", ".ogg", ".m4a"}


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def resolve(repo: Path, pf: Path, name: str) -> str:
    """Repo-relative path of a source named in a provenance file; same lookup order as
    process.py (repo root, then the provenance directory and its parents). Unresolved names
    are returned unchanged so the missing-file check reports them."""
    p = Path(name)
    for base in (repo, pf.parent, *pf.parent.parents):
        if (base / p).is_file():
            return str((base / p).relative_to(repo))
    return name


def load_rows(repo: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for pf in sorted(glob.glob(str(repo / "assets-source/audio/**/provenance*.json"), recursive=True)):
        data = json.loads(Path(pf).read_text())
        for row in data if isinstance(data, list) else data.get("files", []):
            if isinstance(row, dict) and row.get("file"):
                rows.append({**row, "_orig": row["file"], "file": resolve(repo, Path(pf), row["file"]),
                             "_provenance": str(Path(pf).relative_to(repo))})
    return rows


def validate(repo: Path) -> list[str]:
    errs: list[str] = []
    rows = load_rows(repo)
    by_file = {r["file"]: r for r in rows}
    if len(by_file) != len(rows):
        errs.append("duplicate provenance rows for the same file")

    for r in rows:
        p = repo / r["file"]
        if not p.is_file():
            errs.append(f"{r['file']}: source file missing ({r['_provenance']})")
            continue
        if r.get("sha256") and sha256(p) != r["sha256"]:
            errs.append(f"{r['file']}: sha256 does not match provenance row")
        if r.get("bytes") and p.stat().st_size != r["bytes"]:
            errs.append(f"{r['file']}: byte size does not match provenance row")

    src_root = repo / "assets-source/audio"
    for p in sorted(src_root.rglob("*")):
        if p.is_file() and p.suffix in AUDIO_EXT and str(p.relative_to(repo)) not in by_file:
            errs.append(f"{p.relative_to(repo)}: source audio with no provenance row")

    for r in rows:
        d = r.get("derived_from")
        if not d:
            continue
        parent = repo / d["file"]
        if d["file"] not in by_file:
            errs.append(f"{r['file']}: parent {d['file']} has no provenance row")
        if not parent.is_file():
            errs.append(f"{r['file']}: parent {d['file']} missing")
        elif sha256(parent) != d["sha256"]:
            errs.append(f"{r['file']}: parent {d['file']} hash differs from derived_from record")

    manifest_path = repo / "audition/sfx/manifest.json"
    manifest = json.loads(manifest_path.read_text())
    live = {r["file"]: r for r in rows if r.get("status") != "rejected"}
    rejected = {r["file"] for r in rows if r.get("status") == "rejected"}
    items = manifest["items"]
    orig_to_file = {r["_orig"]: r["file"] for r in rows}
    for i in items:
        i["source"]["file"] = orig_to_file.get(i["source"]["file"], i["source"]["file"])
    item_sources = {i["source"]["file"]: i for i in items}
    for i in items:
        row = by_file.get(i["source"]["file"])
        if row is None:
            errs.append(f"manifest item {i['stem']}: no provenance row")
        elif row.get("status") == "rejected":
            errs.append(f"manifest item {i['stem']}: provenance row is rejected")
        elif i["source"].get("sha256") != row.get("sha256"):
            errs.append(f"manifest item {i['stem']}: source hash differs from provenance row")
    for f in live:
        if f not in item_sources:
            errs.append(f"{f}: live provenance row with no manifest item (stale manifest)")
    listed_rejected = {orig_to_file.get(r["file"], r["file"]) for r in manifest.get("rejected", [])}
    for f in sorted(rejected):
        if f not in listed_rejected:
            errs.append(f"{f}: rejected row missing from manifest rejected list")
        if f in item_sources:
            errs.append(f"{f}: rejected row has a manifest item")
    rejected_stems = {Path(f).stem for f in rejected}

    expected: dict[str, str] = {}
    sfx = repo / "audition/sfx"
    for i in items:
        for f in i.get("files", {}).values():
            expected[f["path"]] = f["sha256"]
        if i.get("master"):
            expected[i["master"]["path"]] = i["master"]["sha256"]
    for rel, digest in sorted(expected.items()):
        base = sfx if not rel.startswith("../") else sfx
        p = (base / rel).resolve()
        if p.is_file() and sha256(p) != digest:
            errs.append(f"{rel}: derivative hash differs from manifest")
        elif not p.is_file() and "_masters" not in rel:
            errs.append(f"{rel}: derivative listed in manifest but missing")
    listed_names = {Path(rel).name for rel in expected}
    for p in sorted([*sfx.glob("*/*.ogg"), *sfx.glob("*/*.m4a")]):
        if p.name not in listed_names:
            errs.append(f"{p.relative_to(repo)}: derivative not in manifest (orphan)")
        if p.stem in rejected_stems:
            errs.append(f"{p.relative_to(repo)}: derivative of a rejected take")

    picks = json.loads((repo / "docs/audio/audition-picks.json").read_text())
    by_stem = {i["stem"]: i for i in items}
    for cue, stems in picks.items():
        if cue in ("_meta", "notes") or not isinstance(stems, list):
            continue
        for s in stems:
            item = by_stem.get(s)
            if item is None:
                errs.append(f"picks {cue}: {s} is not a live manifest item")
            elif item.get("loop") and item["family"] != "music":
                lc = item["loop_check"]
                if not (lc["master_pass"] and lc["pass"]):
                    errs.append(f"picks {cue}: loop {s} fails the master or Opus loop check")
    return errs


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--repo", default=str(Path(__file__).resolve().parents[2]))
    args = ap.parse_args()
    errs = validate(Path(args.repo))
    for e in errs:
        print(f"FAIL: {e}")
    print(f"provenance reconciliation: {'FAIL (' + str(len(errs)) + ')' if errs else 'PASS'}")
    return 1 if errs else 0


if __name__ == "__main__":
    sys.exit(main())
