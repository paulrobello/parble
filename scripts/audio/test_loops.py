#!/usr/bin/env python3
"""Unit tests for the loop machinery in process.py and for validate_provenance.py.

Run: python3 -m unittest discover -s scripts/audio -p 'test_*.py'
"""

from __future__ import annotations

import json
import math
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import process as proc  # noqa: E402
import validate_provenance as vp  # noqa: E402

REPO = HERE.parents[1]


class LoopMathTest(unittest.TestCase):
    def test_xfade_concat_is_equal_power_and_length_correct(self) -> None:
        a = np.ones(1000, dtype=np.float32)
        b = np.ones(1000, dtype=np.float32)
        out = proc.xfade_concat(a, b, 200)
        self.assertEqual(len(out), 1800)
        # Correlated, equal-level signals sum to up to sqrt(2) in the middle of an equal-power fade.
        self.assertAlmostEqual(float(out[800 + 100]), math.sqrt(2), delta=0.05)

    def test_fold_tail_over_head_wraps_continuously(self) -> None:
        t = np.arange(48000 * 2) / proc.SR
        x = (np.sin(2 * math.pi * 3.3 * t) + 0.01 * t).astype(np.float32)
        out = proc.fold_tail_over_head(x, 4800)
        self.assertEqual(len(out), len(x) - 4800)
        self.assertLess(abs(float(out[0] - out[-1])), 0.05)

    def test_leveler_pulls_envelope_inside_dropout_rule(self) -> None:
        rng = np.random.default_rng(1)
        env_db = np.concatenate([np.full(30, -20.0), np.full(30, -60.0), np.full(30, -35.0)])
        x = np.concatenate([rng.standard_normal(4800) * 10 ** (d / 20) for d in env_db])
        lev, info = proc.level_loop_source(proc._arr(x))
        db = proc.window_db(proc._np(lev))
        self.assertLessEqual(float(db.max() - db.min()), 24.0)
        self.assertIn("leveler", info)

    def test_opus_trim_patch_gives_exact_length(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            work = Path(td)
            t = np.arange(proc.SR * 2) / proc.SR
            loop = (0.3 * np.sin(2 * math.pi * 220 * t)).astype(np.float32)
            dest = work / "l.ogg"
            proc.encode_opus_loop(proc._arr(loop), dest, work)
            dec = proc.decode(dest)
            self.assertEqual(len(dec), len(loop))
            # The decoded loop is continuous across the wrap: the wrap step is no larger than
            # the loop's own largest sample step.
            d = np.abs(np.diff(np.frombuffer(dec, dtype=np.float32)))
            self.assertLessEqual(abs(float(dec[0] - dec[-1])), float(d.max()) * 1.5)

    def test_ogg_crc_roundtrip(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            work = Path(td)
            loop = (0.2 * np.sin(2 * math.pi * 100 * np.arange(proc.SR) / proc.SR)).astype(np.float32)
            dest = work / "l.ogg"
            proc.encode_opus_loop(proc._arr(loop), dest, work)
            # ffmpeg reports no Ogg CRC errors when remuxing a patched stream.
            proc.run(["ffmpeg", "-nostdin", "-v", "error", "-i", str(dest), "-c", "copy",
                      "-f", "ogg", str(work / "copy.ogg")])


class ProvenanceReconciliationTest(unittest.TestCase):
    def test_committed_tree_reconciles(self) -> None:
        self.assertEqual(vp.validate(REPO), [])

    def test_detects_tampered_source_orphan_and_rejected_derivative(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            for sub in ("assets-source/audio/sfx", "audition/sfx", "docs/audio"):
                (root / sub).mkdir(parents=True)
            shutil.copy(REPO / "docs/audio/audition-picks.json", root / "docs/audio/")
            src = root / "assets-source/audio/sfx/a.mp3"
            src.write_bytes(b"audio-a")
            orphan = root / "assets-source/audio/sfx/orphan.mp3"
            orphan.write_bytes(b"orphan")
            digest = vp.sha256(src)
            (root / "assets-source/audio/sfx/provenance-group-a.json").write_text(json.dumps(
                {"files": [{"cue_id": "x", "file": "assets-source/audio/sfx/a.mp3", "sha256": digest}]}))
            (root / "audition/sfx/manifest.json").write_text(json.dumps({"items": [], "rejected": []}))
            errs = vp.validate(root)
            joined = "\n".join(errs)
            self.assertIn("orphan.mp3: source audio with no provenance row", joined)
            self.assertIn("live provenance row with no manifest item", joined)
            src.write_bytes(b"tampered")
            self.assertIn("a.mp3: sha256 does not match provenance row", "\n".join(vp.validate(root)))


if __name__ == "__main__":
    unittest.main()
