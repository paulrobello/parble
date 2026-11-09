#!/usr/bin/env python3
"""Seam checks for the music loop derivatives (scripts/audio/music_loops.py, P4-T5).

Decodes every committed runtime file in audition/music-loops, renders its wrap point (the
samples up to loopEnd followed by the samples from loopStart, two loop iterations' worth of
junction) and asserts no discontinuity: the wrap step is bounded by the loop body's own
99.9th-percentile sample step, and the 50 ms RMS across the wrap matches the continuous audio
at loopStart. Controls show the metric rejects a hard cut and a level jump.

Run: python3 -m unittest discover -s scripts/audio -p 'test_*.py'
"""

from __future__ import annotations

import json
import sys
import unittest
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import music_loops as ml  # noqa: E402
import process as proc  # noqa: E402

REPO = HERE.parents[1]
LOOPS = REPO / "audition/music-loops"
SR = ml.SR


def tone(seconds: float, freqs: tuple[float, ...] = (220.0, 331.0, 497.0)) -> np.ndarray:
    t = np.arange(int(seconds * SR)) / SR
    m = sum(np.sin(2 * np.pi * f * t + i) for i, f in enumerate(freqs)) / len(freqs)
    env = 0.6 + 0.4 * np.sin(2 * np.pi * 0.37 * t)
    return np.stack([0.3 * m * env, 0.28 * m * env], axis=1)


class SeamMetricControlsTest(unittest.TestCase):
    def test_build_wraps_onto_the_audio_that_precedes_the_loop_start(self) -> None:
        x = tone(20.0)
        s, e = 3 * SR + 17, 15 * SR + 4321
        y = ml.build(x, s, e)
        self.assertEqual(len(y), e + int(ml.POSTROLL_S * SR))
        np.testing.assert_array_equal(y[:s], x[:s])
        np.testing.assert_array_equal(y[e:], x[s:s + int(ml.POSTROLL_S * SR)])
        # The last body sample is (almost fully) x[S-1], so the wrap to x[S] is a natural step.
        self.assertLess(np.max(np.abs(y[e - 1] - x[s - 1])), 1e-3)
        self.assertTrue(ml.seam_metrics(y, s, e)["pass"])

    def test_rejects_a_hard_cut(self) -> None:
        x = tone(20.0)
        s, e = 3 * SR + 17, 15 * SR + 4321
        y = np.concatenate([x[:e], x[s:s + SR]])  # no crossfade: e -> s is an arbitrary jump
        m = ml.seam_metrics(y, s, e)
        self.assertGreater(m["step_ratio"], ml.SEAM_STEP_RATIO_MAX)
        self.assertFalse(m["pass"])

    def test_rejects_a_level_jump_at_the_wrap(self) -> None:
        x = tone(20.0)
        s, e = 3 * SR + 17, 15 * SR + 4321
        y = ml.build(x, s, e)
        y[e - SR:e] *= 0.4  # the last second of the loop drops ~8 dB
        m = ml.seam_metrics(y, s, e)
        self.assertGreater(abs(m["rms_excess_db"]), ml.SEAM_RMS_EXCESS_DB_MAX)
        self.assertFalse(m["pass"])


@unittest.skipUnless((LOOPS / "manifest.json").is_file(), "no music loops built")
class CommittedLoopSeamTest(unittest.TestCase):
    manifest: dict

    @classmethod
    def setUpClass(cls) -> None:
        cls.manifest = json.loads((LOOPS / "manifest.json").read_text())

    def test_every_loop_decodes_to_its_layout_and_passes_the_seam_check(self) -> None:
        items = self.manifest["items"]
        self.assertEqual(sorted(i["cue_id"] for i in items),
                         ["music_build_calm", "music_build_playful", "music_night_lanterns",
                          "music_run_flow", "music_title"])
        for item in items:
            s, e = item["loop"]["start_sample"], item["loop"]["end_sample"]
            self.assertGreaterEqual(e - s, ml.MIN_LOOP_S * SR, item["cue_id"])
            for ext, f in item["files"].items():
                with self.subTest(cue=item["cue_id"], format=ext):
                    path = LOOPS / f["path"]
                    self.assertEqual(proc.sha256(path), f["sha256"])
                    y = ml.decode_stereo(path)
                    self.assertEqual(len(y), item["runtime"]["samples"])
                    # Two iterations' junction: ..y[e-1] | y[s].. rendered as one signal.
                    wrap = np.concatenate([y[s:e], y[s:s + SR]])
                    m = ml.seam_metrics(np.concatenate([y[:s], wrap]), s, e)
                    self.assertTrue(m["pass"], m)
                    self.assertLessEqual(m["step_ratio"], ml.SEAM_STEP_RATIO_MAX)
                    self.assertLessEqual(abs(m["rms_excess_db"]), ml.SEAM_RMS_EXCESS_DB_MAX)
                    self.assertLess(np.max(np.abs(y)), 1.0)


if __name__ == "__main__":
    unittest.main()
