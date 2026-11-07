#!/usr/bin/env python3
"""Fixture tests for scripts/audio/process.py. Run: python3 -m unittest scripts/audio/test_process.py

Fixtures are synthesized with ffmpeg into a temp dir with a fake provenance file, and the
pipeline writes to a temp --out, so no real asset directory is touched.
"""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]

# (cue_id, family, loop, lavfi graph). Amplitudes are set explicitly via volume.
FIXTURES = [
    # Stereo heavy impact with 200 ms of leading silence and a decaying tail.
    ("impact_glass_block_heavy", "impact", False,
     "sine=f=2200:d=1:r=48000,volume=4,afade=t=out:st=0:d=0.25,adelay=200|200,"
     "aformat=channel_layouts=stereo,atrim=0:1.2"),
    ("ui_click", "ui", False,
     "anoisesrc=d=0.5:a=0.6:seed=3:r=48000,afade=t=out:st=0:d=0.03,adelay=50,atrim=0:0.5"),
    # Long ring-out to exercise the 2.0 s length cap.
    ("finish_cup_chime", "mechanism", False,
     "sine=f=1760:d=3:r=48000,volume=3,afade=t=out:st=0:d=3,adelay=30,atrim=0:3"),
    # Double hit at 120 ms, must be flagged as a second onset.
    ("clack_glass_glass_medium", "impact", False,
     "aevalsrc='0.8*sin(2*PI*3000*t)*(exp(-t*60)+gte(t,0.12)*exp(-(t-0.12)*60))':d=0.6:s=48000"),
    # 8 s noise loop with a 0.4 s fade-in head (fails raw, needs crossfade).
    ("roll_glass_block", "roll", True,
     "anoisesrc=d=8:a=0.2:color=pink:seed=7:r=48000,lowpass=f=3000,afade=t=in:d=0.4,"
     "aformat=channel_layouts=stereo"),
    ("tunnel_rumble", "drop_tunnel", True,
     "anoisesrc=d=6:a=0.3:color=brown:seed=9:r=48000,lowpass=f=400"),
]


def sha(p: Path) -> str:
    return hashlib.sha256(p.read_bytes()).hexdigest()


class PipelineTest(unittest.TestCase):
    tmp: tempfile.TemporaryDirectory[str]
    root: Path

    @classmethod
    def setUpClass(cls) -> None:
        cls.tmp = tempfile.TemporaryDirectory(delete=not os.environ.get("PARBLE_KEEP_FIXTURES"))
        cls.root = Path(cls.tmp.name)
        src = cls.root / "src"
        rows = []
        for cue, fam, loop, graph in FIXTURES:
            (src / fam).mkdir(parents=True, exist_ok=True)
            for v in (1, 2):
                out = src / fam / f"{cue}_v{v:02d}.mp3"
                subprocess.run(["ffmpeg", "-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i",
                                graph.replace("seed=", f"seed={v}"), "-ar", "44100",
                                "-c:a", "libmp3lame", "-b:a", "128k", str(out)], check=True)
                rows.append({"cue_id": cue, "family": fam, "loop": loop,
                             "file": str(out.relative_to(cls.root)), "sha256": sha(out),
                             "prompt": f"fixture {cue}"})
        (cls.root / "provenance-group-z.json").write_text(
            json.dumps({"provider": "fixture", "files": rows}))
        cls.run_pipeline("out1")
        cls.run_pipeline("out2")
        print(f"fixtures: {cls.root}", file=sys.stderr)
        cls.manifest = json.loads((cls.root / "out1/manifest.json").read_text())

    @classmethod
    def run_pipeline(cls, name: str) -> None:
        proc = subprocess.run([sys.executable, str(HERE / "process.py"), "--repo", str(cls.root),
                               "--provenance-glob", str(cls.root / "provenance-group-*.json"),
                               "--out", str(cls.root / name),
                               "--music-provenance-glob", str(cls.root / "no-music-*.json")], capture_output=True, text=True)
        if proc.returncode != 0:
            raise AssertionError(f"pipeline exit {proc.returncode}: {proc.stdout}{proc.stderr}")

    @classmethod
    def tearDownClass(cls) -> None:
        if not os.environ.get("PARBLE_KEEP_FIXTURES"):
            cls.tmp.cleanup()

    def items(self, cue: str) -> list[dict]:
        return [i for i in self.manifest["items"] if i["cue_id"] == cue]

    def test_every_variation_in_manifest(self) -> None:
        self.assertEqual(len(self.manifest["items"]), len(FIXTURES) * 2)
        for i in self.manifest["items"]:
            for ext in ("ogg", "m4a"):
                self.assertTrue((self.root / "out1" / i["files"][ext]["path"]).is_file())
            self.assertEqual(i["source"]["sha256"], i["source"]["sha256_recorded"])

    def test_one_shot_leading_silence_and_duration(self) -> None:
        for cue in ("impact_glass_block_heavy", "ui_click", "finish_cup_chime"):
            for i in self.items(cue):
                self.assertLess(i["master"]["leading_silence_ms"], 5.0, i["stem"])
                lo, hi = i["target"]["duration_s"]
                self.assertTrue(lo - 1e-3 <= i["duration_s"] <= hi + 1e-3, i["stem"])
        for i in self.items("finish_cup_chime"):
            self.assertAlmostEqual(i["duration_s"], 2.0, places=2)

    def test_true_peak_under_ceiling_or_flagged(self) -> None:
        for i in self.manifest["items"]:
            ceiling = i["target"]["true_peak_ceiling_db"]
            if i["true_peak_db"] > ceiling:
                self.assertTrue(any("true peak" in s for s in i["issues"]), i["stem"])

    def test_loudness_on_target_or_flagged(self) -> None:
        for i in self.manifest["items"]:
            t = i["target"]
            if abs(i["lufs"] - t["lufs"]) > t["tolerance_lu"]:
                self.assertTrue(any("loudness" in s for s in i["issues"]), i["stem"])
        for i in self.items("roll_glass_block") + self.items("tunnel_rumble"):
            self.assertLessEqual(abs(i["lufs"] - i["target"]["lufs"]), 1.0, i["stem"])

    def test_loops_pass_checker_on_encoded_ogg(self) -> None:
        for cue, lo, hi in (("roll_glass_block", 6, 8), ("tunnel_rumble", 4, 6)):
            for i in self.items(cue):
                self.assertTrue(i["loop_check"]["pass"], i["files"]["ogg"]["loop_check"])
                self.assertTrue(lo <= i["duration_s"] <= hi, i["duration_s"])
        # The fade-in head forces the crossfade path.
        self.assertTrue(all(i["loop_check"]["crossfade_s"] for i in self.items("roll_glass_block")))

    def test_double_hit_flagged(self) -> None:
        for i in self.items("clack_glass_glass_medium"):
            self.assertIsNotNone(i["second_onset_ms"])
            self.assertAlmostEqual(i["second_onset_ms"], 120, delta=10)
        for i in self.items("ui_click"):
            self.assertIsNone(i["second_onset_ms"])

    def test_deterministic_rerun(self) -> None:
        a, b = self.root / "out1", self.root / "out2"
        files_a = sorted(p.relative_to(a) for p in a.rglob("*") if p.is_file())
        files_b = sorted(p.relative_to(b) for p in b.rglob("*") if p.is_file())
        self.assertEqual(files_a, files_b)
        for rel in files_a:
            self.assertEqual(sha(a / rel), sha(b / rel), str(rel))

    def test_originals_untouched(self) -> None:
        prov = json.loads((self.root / "provenance-group-z.json").read_text())
        for row in prov["files"]:
            self.assertEqual(sha(self.root / row["file"]), row["sha256"])


if __name__ == "__main__":
    unittest.main()
