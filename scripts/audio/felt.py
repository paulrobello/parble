#!/usr/bin/env python3
"""Felt treatment for rug SFX (P4-T2 prompt revision 3), plus the softness metrics.

  derive   parent take -> 48 kHz mono 24-bit FLAC with the felt recipe applied:
             1. transient limiter: a fast peak envelope is held to at most LIMIT_DB above a
                slower RMS envelope, so clicks and attacks are pulled down to the body level
             2. one-shots only: raised-cosine attack smear over ATTACK_MS from the onset
             3. steep low-pass: LP_STAGES cascaded 2nd-order Butterworth sections at LP_HZ
             4. low shelf: +SHELF_DB at SHELF_HZ (RBJ cookbook, S = 1)
             5. peak normalize to OUT_PEAK_DB (process.py sets the final level)
           Loops are filtered circularly: the last WARMUP_S of the take is run through every
           stage first and discarded, so the filter state at the head matches the tail and
           the wrap stays continuous.
  measure  crest factor, steepest 1 ms envelope rise, click onsets per second, share of
           energy above 2 kHz and spectral centroid above 150 Hz (needs numpy).

The DSP is pure Python floating point and the FLAC is written with bitexact flags, so the
same parent bytes always give the same output bytes. Usage:

    scripts/audio/felt.py derive PARENT.mp3 OUT.flac [--loop]
    scripts/audio/felt.py measure FILE [FILE ...] [--loop]
"""

from __future__ import annotations

import argparse
import array
import json
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import process as proc  # noqa: E402

SR = proc.SR
RECIPE_VERSION = 1
LIMIT_DB = 3.0          # fast peak envelope may exceed the slow RMS envelope by this much
FAST_ATTACK_MS = 0.05
FAST_RELEASE_MS = 8.0
SLOW_MS = 20.0          # one-pole time constant of the slow RMS envelope
GAIN_SMOOTH_MS = 1.5    # one-pole smoothing of the limiter gain
ATTACK_MS = 12.0        # one-shot attack smear
LP_HZ = 1500.0
LP_STAGES = 4           # 4 x 12 dB/oct = 48 dB/oct
SHELF_HZ = 250.0
SHELF_DB = 3.0
OUT_PEAK_DB = -3.0
WARMUP_S = 0.5

RECIPE = {
    "recipe": "felt", "recipe_version": RECIPE_VERSION, "script": "scripts/audio/felt.py",
    "decode": f"ffmpeg -> {SR} Hz mono (channel average, as process.py)",
    "transient_limiter": {"limit_db_over_rms": LIMIT_DB, "fast_attack_ms": FAST_ATTACK_MS,
                          "fast_release_ms": FAST_RELEASE_MS, "slow_rms_ms": SLOW_MS,
                          "gain_smooth_ms": GAIN_SMOOTH_MS},
    "attack_smear_ms_one_shot": ATTACK_MS,
    "low_pass": {"hz": LP_HZ, "type": "Butterworth biquad cascade", "stages": LP_STAGES,
                 "slope_db_per_octave": 12 * LP_STAGES},
    "low_shelf": {"hz": SHELF_HZ, "gain_db": SHELF_DB, "slope": 1.0, "type": "RBJ cookbook"},
    "loop_warmup_s": WARMUP_S, "output_peak_dbfs": OUT_PEAK_DB,
    "output": "FLAC 24-bit 48 kHz mono, ffmpeg bitexact",
}


def _coef(ms: float) -> float:
    return math.exp(-1.0 / (ms * 0.001 * SR))


def limit_transients(x: list[float]) -> list[float]:
    fa, fr, sl, gs = _coef(FAST_ATTACK_MS), _coef(FAST_RELEASE_MS), _coef(SLOW_MS), _coef(GAIN_SMOOTH_MS)
    ratio = 10 ** (LIMIT_DB / 20)
    fast = ms = 0.0
    g = 1.0
    out = []
    for v in x:
        a = abs(v)
        fast = a + (fast - a) * (fa if a > fast else fr)
        ms = v * v + (ms - v * v) * sl
        ceiling = ratio * math.sqrt(ms) * math.sqrt(2)  # sine-peak of the RMS level
        target = min(1.0, ceiling / fast) if fast > 1e-9 else 1.0
        g = target + (g - target) * gs if target > g else target  # instant down, smooth up
        out.append(v * g)
    return out


def biquad(x: list[float], b: tuple[float, float, float], a: tuple[float, float]) -> list[float]:
    b0, b1, b2 = b
    a1, a2 = a
    x1 = x2 = y1 = y2 = 0.0
    out = []
    for v in x:
        y = b0 * v + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2
        x2, x1, y2, y1 = x1, v, y1, y
        out.append(y)
    return out


def lowpass_coefs(fc: float, q: float) -> tuple[tuple[float, float, float], tuple[float, float]]:
    w = 2 * math.pi * fc / SR
    alpha = math.sin(w) / (2 * q)
    cw = math.cos(w)
    a0 = 1 + alpha
    return (((1 - cw) / 2 / a0, (1 - cw) / a0, (1 - cw) / 2 / a0),
            (-2 * cw / a0, (1 - alpha) / a0))


def butterworth_qs(stages: int) -> list[float]:
    n = 2 * stages
    return [1 / (2 * math.cos(math.pi * (2 * k + 1) / (2 * n))) for k in range(stages)]


def lowshelf_coefs(fc: float, gain_db: float) -> tuple[tuple[float, float, float], tuple[float, float]]:
    big_a = 10 ** (gain_db / 40)
    w = 2 * math.pi * fc / SR
    cw, sw = math.cos(w), math.sin(w)
    alpha = sw / 2 * math.sqrt(2)  # S = 1
    sa = 2 * math.sqrt(big_a) * alpha
    a0 = (big_a + 1) + (big_a - 1) * cw + sa
    b0 = big_a * ((big_a + 1) - (big_a - 1) * cw + sa)
    b1 = 2 * big_a * ((big_a - 1) - (big_a + 1) * cw)
    b2 = big_a * ((big_a + 1) - (big_a - 1) * cw - sa)
    a1 = -2 * ((big_a - 1) + (big_a + 1) * cw)
    a2 = (big_a + 1) + (big_a - 1) * cw - sa
    return (b0 / a0, b1 / a0, b2 / a0), (a1 / a0, a2 / a0)


def attack_smear(x: list[float]) -> list[float]:
    pk = max((abs(v) for v in x), default=0.0)
    thr = pk * 10 ** (proc.ONSET_DB / 20)
    onset = next((i for i, v in enumerate(x) if abs(v) >= thr), 0)
    n = int(ATTACK_MS * 0.001 * SR)
    y = list(x)
    for i in range(onset, min(len(y), onset + n)):
        y[i] *= 0.5 * (1 - math.cos(math.pi * (i - onset + 0.5) / n))
    return y


def felt(x: list[float], loop: bool) -> list[float]:
    warm = min(int(WARMUP_S * SR), len(x)) if loop else 0
    y = (x[len(x) - warm:] + x) if warm else list(x)
    y = limit_transients(y)
    for q in butterworth_qs(LP_STAGES):
        y = biquad(y, *lowpass_coefs(LP_HZ, q))
    y = biquad(y, *lowshelf_coefs(SHELF_HZ, SHELF_DB))
    y = y[warm:]
    if not loop:
        y = attack_smear(y)
    pk = max((abs(v) for v in y), default=0.0)
    g = 10 ** (OUT_PEAK_DB / 20) / pk if pk > 0 else 1.0
    return [v * g for v in y]


def derive(parent: Path, out: Path, loop: bool) -> dict:
    x = list(proc.decode(parent))
    y = felt(x, loop)
    out.parent.mkdir(parents=True, exist_ok=True)
    proc.write_pcm(array.array("f", y), out, "flac")
    return {"parent_sha256": proc.sha256(parent), "sha256": proc.sha256(out),
            "bytes": out.stat().st_size, "samples": len(y), **RECIPE}


# ---------------------------------------------------------------- metrics


def metrics(path: Path, loop: bool) -> dict:
    import numpy as np

    x = np.asarray(proc.decode(path), dtype=np.float64)
    pk = float(np.max(np.abs(x)))
    if not loop:  # active region, same onset/tail rule as process.py
        on = int(np.argmax(np.abs(x) >= pk * 10 ** (proc.ONSET_DB / 20)))
        w = int(0.005 * SR)
        seg = x[on:]
        rms = np.sqrt(np.mean(seg[: len(seg) // w * w].reshape(-1, w) ** 2, axis=1))
        last = int(np.max(np.nonzero(rms >= rms.max() * 10 ** (proc.TAIL_DB / 20))))
        x = seg[: (last + 1) * w]
    rms_all = float(np.sqrt(np.mean(x ** 2)))
    # Envelope: 2 ms RMS at a 1 ms hop. Rise is the steepest 2 ms climb among windows within
    # 40 dB of the loudest; attack is onset (first sample within ONSET_DB of peak) to the
    # loudest window, so a longer attack is a softer "pff".
    w1, hop = int(0.002 * SR), int(0.001 * SR)
    sq = np.concatenate([[0.0], np.cumsum(x ** 2)])
    starts = np.arange(0, len(x) - w1 + 1, hop)
    env = 10 * np.log10((sq[starts + w1] - sq[starts]) / w1 + 1e-24)
    live = env >= env.max() - 40
    rises = (env[2:] - env[:-2])[live[2:]] / 2
    attack_ms = float(np.argmax(env)) * hop / SR * 1000
    # Clicks: 5 ms windows of the first difference (a +6 dB/oct tilt that exposes clicks)
    # standing 8 dB above the median of the previous 200 ms, 30 ms refractory. Counted in the
    # band under LP_HZ for every file, so a bright noise floor cannot mask clicks that a dark
    # take (or a derived one) shows.
    band_x = x.tolist()
    for q in butterworth_qs(LP_STAGES):
        band_x = biquad(band_x, *lowpass_coefs(LP_HZ, q))
    d = np.diff(np.asarray(band_x))
    w2 = int(0.005 * SR)
    e2 = 20 * np.log10(np.sqrt(np.mean(d[: len(d) // w2 * w2].reshape(-1, w2) ** 2, axis=1)) + 1e-12)
    clicks, last_click = 0, -999
    for i in range(1, len(e2)):
        base = np.median(e2[max(0, i - 40):i])
        if e2[i] - base > 8 and e2[i] > e2.max() - 40 and i - last_click > 6:
            clicks += 1
            last_click = i
    spec = np.abs(np.fft.rfft(x * np.hanning(len(x)))) ** 2
    f = np.fft.rfftfreq(len(x), 1 / SR)
    band = f >= 150
    total = float(spec[band].sum()) or 1e-30
    return {
        "file": str(path), "duration_s": round(len(x) / SR, 3),
        "crest_db": round(20 * math.log10(pk / rms_all), 2) if rms_all > 0 else None,
        "max_rise_db_per_ms": round(float(rises.max()), 2) if rises.size else None,
        "attack_ms": None if loop else round(attack_ms, 1),
        "clicks_per_s": round(clicks / (len(x) / SR), 2),
        "share_above_2k_pct": round(100 * float(spec[f >= 2000].sum()) / total, 2),
        "centroid_hp_hz": round(float((spec[band] * f[band]).sum()) / total, 1),
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    d = sub.add_parser("derive")
    d.add_argument("parent", type=Path)
    d.add_argument("out", type=Path)
    d.add_argument("--loop", action="store_true")
    m = sub.add_parser("measure")
    m.add_argument("files", nargs="+", type=Path)
    m.add_argument("--loop", action="store_true")
    args = ap.parse_args()
    if args.cmd == "derive":
        print(json.dumps(derive(args.parent, args.out, args.loop), indent=1))
    else:
        for f in args.files:
            print(json.dumps(metrics(f, args.loop)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
