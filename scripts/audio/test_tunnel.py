#!/usr/bin/env python3
"""Tests for scripts/audio/tunnel.py. Run: python3 -m unittest scripts/audio/test_tunnel.py"""

from __future__ import annotations

import array
import math
import random
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import tunnel  # noqa: E402
from test_felt import tone_db  # noqa: E402

SR = tunnel.SR


def sine(hz: float, n: int, amp: float = 0.3) -> list[float]:
    return [amp * math.sin(2 * math.pi * hz * i / SR) for i in range(n)]


class TunnelTest(unittest.TestCase):
    def test_varispeed_lowers_pitch_by_the_semitones(self) -> None:
        x = sine(1000, SR)  # 1 s, a whole number of periods, so the loop wraps cleanly
        y = tunnel.varispeed(x, -4.0, loop=True)
        self.assertEqual(len(y), int(SR / 2 ** (-4 / 12)))
        low = 1000 * 2 ** (-4 / 12)
        self.assertGreater(tone_db(y, low) - tone_db(y, 1000), 30.0)

    def test_highs_cut_and_low_body_kept(self) -> None:
        x = [a + b for a, b in zip(sine(250, SR), sine(4000, SR))]
        y = tunnel.tunnel(x, loop=True)
        # After the -4 semitone shift the tones sit at 250 and 4000 Hz times 2 ** (-1/3).
        f = 2 ** (-4 / 12)
        lo_drop = tone_db(x, 250) - tone_db(y, 250 * f)
        hi_drop = tone_db(x, 4000) - tone_db(y, 4000 * f)
        self.assertGreater(hi_drop - lo_drop, 30.0)

    def test_loop_wrap_stays_continuous(self) -> None:
        rnd = random.Random(2)
        x = [sum(0.2 * math.sin(2 * math.pi * f * i / SR + p) for f, p in
                 ((150, 0.3), (310, 1.1), (520, 2.0))) + 0.01 * rnd.gauss(0, 1) for i in range(SR)]
        y = tunnel.tunnel(x, loop=True)
        steps = sorted(abs(y[i] - y[i - 1]) for i in range(1, len(y)))
        self.assertLess(abs(y[0] - y[-1]), steps[int(0.999 * len(steps))])

    def test_derive_is_deterministic_and_records_its_parent(self) -> None:
        rnd = random.Random(3)
        with tempfile.TemporaryDirectory() as td:
            src = Path(td) / "in.wav"
            tunnel.proc.write_pcm(array.array("f", (0.2 * rnd.gauss(0, 1) for _ in range(SR // 2))),
                                  src, "pcm_s16le")
            a = tunnel.derive(src, Path(td) / "a.flac", loop=True)
            b = tunnel.derive(src, Path(td) / "b.flac", loop=True)
            self.assertEqual(a["sha256"], b["sha256"])
            self.assertEqual(a["parent_sha256"], tunnel.proc.sha256(src))
            self.assertEqual(a["recipe"], "tunnel")
            self.assertEqual(a["varispeed"]["semitones"], tunnel.SEMITONES)


if __name__ == "__main__":
    unittest.main()
