#!/usr/bin/env python3
"""Tests for scripts/audio/felt.py. Run: python3 -m unittest scripts/audio/test_felt.py"""

from __future__ import annotations

import array
import math
import random
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import felt  # noqa: E402

SR = felt.SR


def tone_db(x: list[float], hz: float) -> float:
    """Goertzel magnitude of one frequency, in dB."""
    k = 2 * math.cos(2 * math.pi * hz / SR)
    s1 = s2 = 0.0
    for v in x:
        s1, s2 = v + k * s1 - s2, s1
    return 10 * math.log10(s1 * s1 + s2 * s2 - k * s1 * s2 + 1e-30)


def crest_db(x: list[float]) -> float:
    rms = math.sqrt(sum(v * v for v in x) / len(x))
    return 20 * math.log10(max(abs(v) for v in x) / rms)


class FeltTest(unittest.TestCase):
    def test_high_band_removed_low_band_kept(self) -> None:
        n = SR  # 1 s; 200 Hz and 4 kHz both fit a whole number of periods
        x = [0.3 * math.sin(2 * math.pi * 200 * i / SR) + 0.3 * math.sin(2 * math.pi * 4000 * i / SR)
             for i in range(n)]
        y = felt.felt(x, loop=True)
        drop_low = tone_db(x, 200) - tone_db(y, 200)
        drop_high = tone_db(x, 4000) - tone_db(y, 4000)
        self.assertGreater(drop_high - drop_low, 40.0)

    def test_clicks_softened(self) -> None:
        rnd = random.Random(1)
        x = [0.05 * rnd.gauss(0, 1) for _ in range(SR)]
        for c in range(1000, SR, 4800):  # 10 clicks/s
            x[c] += 0.9
        y = felt.felt(x, loop=True)
        self.assertLess(crest_db(y), crest_db(x) - 6.0)

    def test_loop_wrap_stays_continuous(self) -> None:
        rnd = random.Random(2)
        n = SR
        x = [sum(0.2 * math.sin(2 * math.pi * f * i / SR + p) for f, p in
                 ((150, 0.3), (310, 1.1), (520, 2.0))) + 0.01 * rnd.gauss(0, 1) for i in range(n)]
        y = felt.felt(x, loop=True)
        steps = sorted(abs(y[i] - y[i - 1]) for i in range(1, n))
        self.assertLess(abs(y[0] - y[-1]), steps[int(0.999 * len(steps))])

    def test_one_shot_attack_smeared(self) -> None:
        x = [0.0] * 2000 + [0.8 * math.exp(-i / 2400) * math.sin(2 * math.pi * 300 * i / SR)
                            for i in range(SR // 2)]
        y = felt.felt(x, loop=False)
        onset = next(i for i, v in enumerate(y) if abs(v) > 1e-3)
        first_ms = max(abs(v) for v in y[onset:onset + SR // 1000])
        self.assertLess(first_ms, 0.1 * max(abs(v) for v in y))

    def test_derive_is_deterministic(self) -> None:
        rnd = random.Random(3)
        with tempfile.TemporaryDirectory() as td:
            src = Path(td) / "in.wav"
            felt.proc.write_pcm(array.array("f", (0.2 * rnd.gauss(0, 1) for _ in range(SR))), src,
                                "pcm_s16le")
            a = felt.derive(src, Path(td) / "a.flac", loop=True)
            b = felt.derive(src, Path(td) / "b.flac", loop=True)
            self.assertEqual(a["sha256"], b["sha256"])
            self.assertEqual(a["parent_sha256"], felt.proc.sha256(src))
            self.assertEqual(a["recipe_version"], felt.RECIPE_VERSION)


if __name__ == "__main__":
    unittest.main()
