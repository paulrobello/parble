#!/usr/bin/env python3
"""Enclosed-tunnel treatment for the tunnel_rumble loops (Paul's 2026-11-08 listen: "pitch too
high", it should be lower and muffled, a marble rolling through an enclosed wooden tunnel).

  derive   parent take -> 48 kHz mono 24-bit FLAC with the tunnel recipe applied:
             1. varispeed down SEMITONES (read the take at 2 ** (SEMITONES / 12) samples per
                output sample, Catmull-Rom interpolation): pitch and roll rate both drop, and
                the take gets longer; process.py folds it to the cue sheet's loop length
             2. low-pass: LP_STAGES cascaded 2nd-order Butterworth sections at LP_HZ
             3. body: +SHELF_DB low shelf at SHELF_HZ and a +PEAK_DB peak at PEAK_HZ (Q PEAK_Q),
                the boxy resonance of a bore inside a block (RBJ cookbook)
             4. peak normalize to OUT_PEAK_DB (process.py sets the final level)
           Loops wrap: the interpolation reads around the ends, and the filters run over the last
           WARMUP_S of the take first (discarded), as felt.py does, so the head matches the tail.

The DSP is pure Python floating point and the FLAC is written with bitexact flags, so the same
parent bytes always give the same output bytes. Usage:

    scripts/audio/tunnel.py derive PARENT.mp3 OUT.flac [--loop]
"""

from __future__ import annotations

import argparse
import array
import json
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import felt  # noqa: E402
import process as proc  # noqa: E402

SR = proc.SR
RECIPE_VERSION = 1
SEMITONES = -4.0
LP_HZ = 1000.0
LP_STAGES = 2           # 2 x 12 dB/oct = 24 dB/oct
SHELF_HZ = 180.0
SHELF_DB = 3.0
PEAK_HZ = 260.0
PEAK_DB = 2.5
PEAK_Q = 1.2
OUT_PEAK_DB = -3.0
WARMUP_S = 0.5

RECIPE = {
    "recipe": "tunnel", "recipe_version": RECIPE_VERSION, "script": "scripts/audio/tunnel.py",
    "decode": f"ffmpeg -> {SR} Hz mono (channel average, as process.py)",
    "varispeed": {"semitones": SEMITONES, "interpolation": "Catmull-Rom, wrapped on loops"},
    "low_pass": {"hz": LP_HZ, "type": "Butterworth biquad cascade", "stages": LP_STAGES,
                 "slope_db_per_octave": 12 * LP_STAGES},
    "low_shelf": {"hz": SHELF_HZ, "gain_db": SHELF_DB, "slope": 1.0, "type": "RBJ cookbook"},
    "peak": {"hz": PEAK_HZ, "gain_db": PEAK_DB, "q": PEAK_Q, "type": "RBJ cookbook"},
    "loop_warmup_s": WARMUP_S, "output_peak_dbfs": OUT_PEAK_DB,
    "output": "FLAC 24-bit 48 kHz mono, ffmpeg bitexact",
}


def varispeed(x: list[float], semitones: float, loop: bool) -> list[float]:
    """`x` replayed at 2 ** (semitones / 12) of its speed (lower and longer for a negative shift)."""
    n = len(x)
    if n < 2:
        return list(x)
    step = 2 ** (semitones / 12)
    out_n = int(n / step) if loop else int((n - 1) / step) + 1

    def at(i: int) -> float:
        return x[i % n] if loop else x[min(max(i, 0), n - 1)]

    out = []
    for j in range(out_n):
        pos = j * step
        i = int(pos)
        t = pos - i
        p0, p1, p2, p3 = at(i - 1), at(i), at(i + 1), at(i + 2)
        out.append(p1 + 0.5 * t * (p2 - p0 + t * (2 * p0 - 5 * p1 + 4 * p2 - p3
                                                   + t * (3 * (p1 - p2) + p3 - p0))))
    return out


def peaking_coefs(fc: float, gain_db: float, q: float) -> tuple[tuple[float, float, float], tuple[float, float]]:
    big_a = 10 ** (gain_db / 40)
    w = 2 * math.pi * fc / SR
    alpha = math.sin(w) / (2 * q)
    cw = math.cos(w)
    a0 = 1 + alpha / big_a
    return (((1 + alpha * big_a) / a0, -2 * cw / a0, (1 - alpha * big_a) / a0),
            (-2 * cw / a0, (1 - alpha / big_a) / a0))


def tunnel(x: list[float], loop: bool) -> list[float]:
    y = varispeed(x, SEMITONES, loop)
    warm = min(int(WARMUP_S * SR), len(y)) if loop else 0
    y = (y[len(y) - warm:] + y) if warm else y
    for q in felt.butterworth_qs(LP_STAGES):
        y = felt.biquad(y, *felt.lowpass_coefs(LP_HZ, q))
    y = felt.biquad(y, *felt.lowshelf_coefs(SHELF_HZ, SHELF_DB))
    y = felt.biquad(y, *peaking_coefs(PEAK_HZ, PEAK_DB, PEAK_Q))
    y = y[warm:]
    pk = max((abs(v) for v in y), default=0.0)
    g = 10 ** (OUT_PEAK_DB / 20) / pk if pk > 0 else 1.0
    return [v * g for v in y]


def derive(parent: Path, out: Path, loop: bool) -> dict:
    x = list(proc.decode(parent))
    y = tunnel(x, loop)
    out.parent.mkdir(parents=True, exist_ok=True)
    proc.write_pcm(array.array("f", y), out, "flac")
    return {"parent_sha256": proc.sha256(parent), "sha256": proc.sha256(out),
            "bytes": out.stat().st_size, "samples": len(y), **RECIPE}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    d = sub.add_parser("derive")
    d.add_argument("parent", type=Path)
    d.add_argument("out", type=Path)
    d.add_argument("--loop", action="store_true")
    args = ap.parse_args()
    print(json.dumps(derive(args.parent, args.out, args.loop), indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
