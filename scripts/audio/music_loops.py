#!/usr/bin/env python3
"""Loop derivatives of the owner-selected Suno music (P4-T5). Never writes to the WAV originals.

For each row of assets-source/audio/music/provenance.json:

  decode    48 kHz stereo float
  tempo     onset-envelope autocorrelation near the prompt's BPM (docs/audio/suno-prompts.md),
            beats per bar (3 or 4) from the autocorrelation at one and two bars
  points    loop start S on a beat in [START_MIN_S, START_MAX_S]; loop end E with E - S within a
            quarter beat of a whole number of bars, at least MIN_LOOP_S long, before the ending
            fade (the last second whose 1 s level is within TAIL_DB of the body median); the
            pair whose log-band spectra agree best over the XFADE + CONTEXT_S before S and
            before E wins, then E is refined to the sample by waveform cross-correlation
  seam      the loop body x[S:E) with its last F samples equal-power crossfaded into x[S-F:S),
            the audio that precedes S, so the wrap E -> S continues from the right sample
  layout    runtime file = intro x[0:S] + crossfaded body + POSTROLL_S of x[S:...]. The player
            loops [S, E) with AudioBufferSourceNode loopStart/loopEnd, so the intro plays once
            and the file is continuous on both sides of S and E (a decoder offset smaller than
            the crossfade still wraps onto continuous audio, which covers AAC priming)
  level     linear gain to LUFS_TARGET (measured on the loop body) under TP_CEILING
  encode    Opus and AAC stereo, bitexact; decoded files are remeasured and seam-checked

Writes <out>/<cue>.ogg, <out>/<cue>.m4a and <out>/manifest.json, and the "loopDerivatives"
record of the music provenance file. Deterministic: no timestamps, bitexact muxers.

    scripts/audio/music_loops.py                 # -> audition/music-loops
    scripts/audio/music_loops.py --out /tmp/ml   # scratch build (provenance untouched)
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path
from typing import Any

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import process as proc  # noqa: E402

SR = 48000
REPO = HERE.parents[1]
PROVENANCE = REPO / "assets-source/audio/music/provenance.json"
PROMPTS = REPO / "docs/audio/suno-prompts.md"
DEFAULT_OUT = REPO / "audition/music-loops"
PIPELINE_VERSION = 1

# No music row in docs/audio/cue-sheet.md: the level of the music previews Paul auditioned
# (process.py MUSIC_LUFS), under the same true-peak ceiling. The music bus sets the mix.
LUFS_TARGET = -18.0
LUFS_TOL = 1.0
TP_CEILING = -1.0
TP_MARGIN_DB = 0.3
OPUS = ["-c:a", "libopus", "-b:a", "128k", "-vbr", "on", "-application", "audio"]
AAC = ["-c:a", "aac", "-b:a", "128k"]

HOP = 480  # 10 ms analysis frames
NFFT = 2048
BANDS = 40
BPM_SPAN = 0.15
START_MIN_S = 4.0
START_MAX_S = 40.0
MIN_LOOP_S = 50.0
TAIL_DB = 6.0
XFADE_S = 1.0
CONTEXT_S = 2.0
REFINE_S = 0.02
POSTROLL_S = 0.3

# Seam gate (seam_metrics): the wrap step against the loop body's own 99.9th-percentile step,
# and the 50 ms RMS ratio across the wrap against the same ratio at S in the continuous audio.
SEAM_WIN_S = 0.05
SEAM_STEP_RATIO_MAX = 1.0
SEAM_RMS_EXCESS_DB_MAX = 1.5
SEAM_RMS_ABS_DB_MAX = 6.0


# ---------------------------------------------------------------- io


def decode_stereo(path: Path) -> np.ndarray:
    raw = proc.run(["ffmpeg", "-nostdin", "-v", "error", "-i", str(path), "-ar", str(SR), "-ac", "2",
                    "-f", "f32le", "-acodec", "pcm_f32le", "-"]).stdout
    return np.frombuffer(raw, dtype="<f4").reshape(-1, 2).astype(np.float64)


def write_stereo(x: np.ndarray, path: Path, codec: str = "pcm_s24le") -> None:
    with tempfile.NamedTemporaryFile(suffix=".f32", delete=False) as tmp:
        tmp.write(np.ascontiguousarray(x, dtype="<f4").tobytes())
    try:
        proc.run(["ffmpeg", "-nostdin", "-v", "error", "-y", "-f", "f32le", "-ar", str(SR), "-ac", "2",
                  "-i", tmp.name, "-c:a", codec, *proc.BITEXACT, str(path)])
    finally:
        os.unlink(tmp.name)


def pcm_sha256(x: np.ndarray) -> str:
    return hashlib.sha256(np.ascontiguousarray(x, dtype="<f4").tobytes()).hexdigest()


def prompt_bpm(cue: str, text: str) -> float:
    m = re.search(rf"## Cue \d+: `{re.escape(cue)}`.*?Approximately (\d+) BPM", text, re.S)
    if not m:
        raise SystemExit(f"no BPM for {cue} in {PROMPTS}")
    return float(m.group(1))


# ---------------------------------------------------------------- analysis


def frames(m: np.ndarray) -> np.ndarray:
    """Magnitude spectra of 10 ms hops (Hann, NFFT); frame i is centred on sample i * HOP."""
    pad = np.pad(m, (NFFT // 2, NFFT // 2))
    n = 1 + (len(pad) - NFFT) // HOP
    win = np.hanning(NFFT)
    out = np.empty((n, NFFT // 2 + 1))
    step = 1024
    for i in range(0, n, step):
        idx = np.arange(NFFT)[None, :] + HOP * np.arange(i, min(i + step, n))[:, None]
        out[i:i + step] = np.abs(np.fft.rfft(pad[idx] * win, axis=1))
    return out


def band_features(spec: np.ndarray) -> np.ndarray:
    """Log energy in BANDS log-spaced bands from 60 Hz to 12 kHz, per frame."""
    freqs = np.fft.rfftfreq(NFFT, 1 / SR)
    edges = np.geomspace(60, 12000, BANDS + 1)
    power = spec ** 2
    out = np.empty((spec.shape[0], BANDS))
    for b in range(BANDS):
        sel = (freqs >= edges[b]) & (freqs < edges[b + 1])
        out[:, b] = np.log10(power[:, sel].sum(axis=1) + 1e-10)
    return out


def onset_envelope(spec: np.ndarray) -> np.ndarray:
    logm = np.log1p(1000 * spec)
    flux = np.maximum(np.diff(logm, axis=0, prepend=logm[:1]), 0).sum(axis=1)
    return flux - np.convolve(flux, np.ones(50) / 50, mode="same")


def autocorr(env: np.ndarray) -> np.ndarray:
    n = len(env)
    f = np.fft.rfft(env - env.mean(), 2 * n)
    ac = np.fft.irfft(f * np.conj(f))[:n]
    return ac / ac[0]


def peak_lag(ac: np.ndarray, lo: float, hi: float) -> float:
    a, b = int(np.floor(lo)), int(np.ceil(hi))
    i = a + int(np.argmax(ac[a:b + 1]))
    if 0 < i < len(ac) - 1:
        y0, y1, y2 = ac[i - 1], ac[i], ac[i + 1]
        d = y0 - 2 * y1 + y2
        return i + (0.5 * (y0 - y2) / d if d != 0 else 0.0)
    return float(i)


def tempo(env: np.ndarray, bpm_hint: float) -> dict[str, float]:
    ac = autocorr(env)
    fps = SR / HOP
    beat = peak_lag(ac, fps * 60 / (bpm_hint * (1 + BPM_SPAN)), fps * 60 / (bpm_hint * (1 - BPM_SPAN)))
    # Refine on the lag of 16 beats, where a 10 ms frame is a sixteenth of the error.
    beat = peak_lag(ac, 16 * beat - 0.25 * beat, 16 * beat + 0.25 * beat) / 16
    score = {}
    for bpb in (3, 4):
        score[bpb] = float(np.mean([ac[int(round(k * bpb * beat))] for k in (1, 2, 4)]))
    bpb = max(score, key=lambda k: score[k])
    return {"bpm": round(60 * fps / beat, 3), "beat_frames": beat, "beats_per_bar": bpb,
            "bar_frames": bpb * beat, "meter_score": score}


def ending_guard(m: np.ndarray) -> int:
    """The last sample whose 1 s RMS level is within TAIL_DB of the median 1 s level."""
    w = SR
    n = len(m) // w
    lv = 20 * np.log10(np.sqrt((m[:n * w].reshape(n, w) ** 2).mean(axis=1)) + 1e-12)
    ok = np.nonzero(lv >= np.median(lv) - TAIL_DB)[0]
    return int((ok[-1] + 1) * w)


def beat_frames(env: np.ndarray, beat: float, lo: int, hi: int) -> list[int]:
    """Frames of the beat grid in [lo, hi), phased to the strongest onset comb."""
    steps = int(round(beat))
    phase = max(range(steps), key=lambda p: env[np.arange(lo + p, hi, beat).astype(int)].sum())
    return [int(round(f)) for f in np.arange(lo + phase, hi, beat)]


def similarity(feat: np.ndarray, s: int, width: int) -> np.ndarray:
    """Mean cosine similarity of the `width` frames before frame s against the `width` frames
    before every frame e (index e), over mean-removed, unit-norm band features."""
    z = feat - feat.mean(axis=0)
    z /= np.linalg.norm(z, axis=1, keepdims=True) + 1e-12
    tmpl = z[s - width:s]
    n = len(z)
    size = 1 << int(np.ceil(np.log2(n + width)))
    acc = np.zeros(size)
    for d in range(z.shape[1]):
        acc += np.fft.irfft(np.fft.rfft(z[:, d], size) * np.conj(np.fft.rfft(tmpl[:, d], size)), size)
    out = np.full(n + 1, -1.0)
    # acc[k] = sum_j z[k + j] . tmpl[j]: the window ending at e starts at k = e - width.
    e = np.arange(width, n + 1)
    out[e] = acc[e - width] / width
    return out


def find_loop(x: np.ndarray, bpm_hint: float) -> dict[str, Any]:
    m = x.mean(axis=1)
    spec = frames(m)
    feat = band_features(spec)
    env = onset_envelope(spec)
    t = tempo(env, bpm_hint)
    fps = SR / HOP
    beat, bar = t["beat_frames"], t["bar_frames"]
    guard = ending_guard(m)
    width = int((XFADE_S + CONTEXT_S) * fps)
    starts = beat_frames(env, beat, int(START_MIN_S * fps), int(START_MAX_S * fps))
    e_max = min(guard // HOP, len(feat) - 1)
    best: tuple[float, int, int, int] | None = None
    for s in starts:
        sim = similarity(feat, s, width)
        k_min = int(np.ceil(MIN_LOOP_S * fps / bar))
        for k in range(k_min, int((e_max - s) / bar) + 1):
            c = s + k * bar
            lo, hi = int(np.floor(c - beat / 4)), int(np.ceil(c + beat / 4))
            hi = min(hi, e_max)
            if lo > hi:
                continue
            e = lo + int(np.argmax(sim[lo:hi + 1]))
            score = float(sim[e])
            if best is None or score > best[0] + 1e-9:
                best = (score, s, e, k)
    if best is None:
        raise RuntimeError("no loop candidate")
    score, s_f, e_f, bars = best
    s_n, e_n = s_f * HOP, e_f * HOP
    # Sample refinement: align x[E-F:E] with x[S-F:S] by cross-correlation within ±REFINE_S.
    f_n = int(XFADE_S * SR)
    r = int(REFINE_S * SR)
    ref = m[s_n - f_n:s_n]
    seg = m[e_n - f_n - r:e_n + r]
    size = 1 << int(np.ceil(np.log2(len(seg) + len(ref))))
    cc = np.fft.irfft(np.fft.rfft(seg, size) * np.conj(np.fft.rfft(ref, size)), size)[:2 * r + 1]
    lag = int(np.argmax(cc)) - r
    e_n += lag
    a, b = m[e_n - f_n:e_n], ref
    corr = float(np.dot(a, b) / (np.linalg.norm(a) * np.linalg.norm(b) + 1e-12))
    return {
        "start": s_n, "end": e_n, "bars": bars, "spectral_similarity": round(score, 4),
        "xfade_waveform_correlation": round(corr, 4), "refine_lag_samples": lag,
        "bpm_hint": bpm_hint, "bpm": t["bpm"], "beats_per_bar": t["beats_per_bar"],
        "bar_s": round(bar / fps, 4),
        "loop_bars_measured": round((e_n - s_n) / (bar * HOP), 3),
        "ending_guard_s": round(guard / SR, 2), "start_candidates": len(starts),
    }


# ---------------------------------------------------------------- build


def build(x: np.ndarray, s: int, e: int) -> np.ndarray:
    """Intro x[0:S], body x[S:E) with its tail crossfaded into x[S-F:S), then x[S:S+postroll]."""
    f = int(XFADE_S * SR)
    if s < f:
        raise ValueError("loop start earlier than the crossfade length")
    t = (np.arange(f) + 0.5) / f
    fade_out = np.cos(0.5 * np.pi * t)[:, None]
    fade_in = np.sin(0.5 * np.pi * t)[:, None]
    body = x[s:e].copy()
    body[-f:] = x[e - f:e] * fade_out + x[s - f:s] * fade_in
    post = x[s:s + int(POSTROLL_S * SR)]
    return np.concatenate([x[:s], body, post])


def rms_db(v: np.ndarray) -> float:
    return float(20 * np.log10(np.sqrt(np.mean(v ** 2)) + 1e-12))


def seam_metrics(y: np.ndarray, start: int, end: int) -> dict[str, Any]:
    """Render the wrap [.. end) -> [start ..] and measure it against the loop body's own motion.

    step_ratio: largest per-channel |y[end-1] - y[start]| over the body's 99.9th-percentile
    sample step. rms_jump_db: 50 ms RMS after the wrap minus 50 ms RMS before it.
    natural_jump_db: the same ratio at `start` in the continuous audio (y[start-w:start] vs
    y[start:start+w]), which the wrap should reproduce; rms_excess_db is the difference."""
    w = int(SEAM_WIN_S * SR)
    body = y[start:end]
    p999 = float(np.percentile(np.abs(np.diff(body, axis=0)), 99.9))
    step = float(np.max(np.abs(y[end - 1] - y[start])))
    before, after = y[end - w:end], y[start:start + w]
    jump = rms_db(after) - rms_db(before)
    natural = rms_db(y[start:start + w]) - rms_db(y[start - w:start])
    out = {
        "wrap_step": round(step, 6), "body_step_p999": round(p999, 6),
        "step_ratio": round(step / p999, 4) if p999 > 0 else None,
        "rms_jump_db": round(jump, 3), "natural_jump_db": round(natural, 3),
        "rms_excess_db": round(jump - natural, 3),
    }
    out["pass"] = bool(out["step_ratio"] is not None and out["step_ratio"] <= SEAM_STEP_RATIO_MAX
                       and abs(out["rms_excess_db"]) <= SEAM_RMS_EXCESS_DB_MAX
                       and abs(jump) <= SEAM_RMS_ABS_DB_MAX)
    return out


def decoded_offset(dec: np.ndarray, master: np.ndarray) -> int:
    """Lag of the decoded file against the master (positive: decoded is late), within ±4096."""
    r = 4096
    a = dec[SR:SR * 3].mean(axis=1)
    b = master[SR:SR * 3].mean(axis=1)
    lags = range(-r, r + 1, 1)
    size = 1 << int(np.ceil(np.log2(len(a) + len(b))))
    cc = np.fft.irfft(np.fft.rfft(a, size) * np.conj(np.fft.rfft(b, size)), size)
    vals = [cc[k % size] for k in lags]
    return list(lags)[int(np.argmax(vals))]


def process_row(row: dict[str, Any], out: str, bpm: float) -> dict[str, Any]:
    cue = row["cue_id"]
    src = REPO / row["file"]
    digest = proc.sha256(src)
    if digest != row["sha256"]:
        raise RuntimeError(f"{cue}: source sha256 {digest} does not match provenance")
    x = decode_stereo(src)
    pts = find_loop(x, bpm)
    s, e = pts["start"], pts["end"]
    y = build(x, s, e)
    outd = Path(out)
    with tempfile.TemporaryDirectory() as wd:
        body = Path(wd) / "body.wav"
        write_stereo(y[s:e], body)
        lufs0, tp0 = proc.measure(body)
        full = Path(wd) / "full.wav"
        write_stereo(y, full)
        _, tp_full = proc.measure(full)
        gain = min(LUFS_TARGET - lufs0, TP_CEILING - TP_MARGIN_DB - max(tp0, tp_full))
        files: dict[str, Any] = {}
        for attempt in range(3):
            master = y * 10 ** (gain / 20)
            mwav = Path(wd) / "master.wav"
            write_stereo(master, mwav)
            over = 0.0
            for ext, cargs in (("ogg", OPUS), ("m4a", AAC)):
                dest = outd / f"{cue}.{ext}"
                tmp = dest.with_name(dest.stem + ".tmp" + dest.suffix)
                proc.run(["ffmpeg", "-nostdin", "-v", "error", "-y", "-i", str(mwav), "-ar", str(SR),
                          "-ac", "2", *cargs, *proc.BITEXACT, str(tmp)])
                os.replace(tmp, dest)
                dec = decode_stereo(dest)
                off = decoded_offset(dec, master)
                dwav = Path(wd) / f"dec_body.{ext}.wav"
                write_stereo(dec[s:e], dwav)
                lufs_e, _ = proc.measure(dwav)
                dfull = Path(wd) / f"dec.{ext}.wav"
                write_stereo(dec, dfull)
                _, tp_e = proc.measure(dfull)
                files[ext] = {
                    "path": f"{cue}.{ext}", "sha256": proc.sha256(dest), "bytes": dest.stat().st_size,
                    "decoded_samples": int(len(dec)), "decoded_offset_samples": off,
                    "lufs": round(lufs_e, 2), "true_peak_db": round(tp_e, 2),
                    "seam": seam_metrics(dec, s, e),
                }
                over = max(over, tp_e - TP_CEILING)
            if over <= 0:
                break
            gain -= over + 0.1
    master_seam = seam_metrics(master, s, e)
    issues = []
    for ext, f in files.items():
        if abs(f["lufs"] - LUFS_TARGET) > LUFS_TOL:
            issues.append(f"{ext} loop-body loudness {f['lufs']} LUFS outside {LUFS_TARGET}±{LUFS_TOL}")
        if f["true_peak_db"] > TP_CEILING:
            issues.append(f"{ext} true peak {f['true_peak_db']} dBTP over {TP_CEILING}")
        if not f["seam"]["pass"]:
            issues.append(f"{ext} seam check failed")
    if not master_seam["pass"]:
        issues.append("master seam check failed")
    return {
        "cue_id": cue,
        "source": {"file": row["file"], "sha256": digest, "samples": int(len(x))},
        "loop": {
            "start_sample": s, "end_sample": e, "length_samples": e - s,
            "start_s": round(s / SR, 4), "end_s": round(e / SR, 4), "length_s": round((e - s) / SR, 4),
            "crossfade_samples": int(XFADE_S * SR), "crossfade": "equal-power, tail into x[S-F:S)",
            **{k: v for k, v in pts.items() if k not in ("start", "end")},
        },
        "runtime": {"samples": int(len(y)), "sample_rate": SR, "channels": 2,
                    "loop_start_sample": s, "loop_end_sample": e,
                    "layout": "intro x[0:S] + crossfaded body [S:E) + postroll x[S:S+0.3 s]"},
        "gain_db": round(gain, 3), "source_body_lufs": round(lufs0, 2),
        "master_pcm_sha256": pcm_sha256(master), "master_seam": master_seam,
        "files": files, "issues": issues,
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--out", default=str(DEFAULT_OUT))
    ap.add_argument("--only", default=None, help="regex on the cue id")
    ap.add_argument("--jobs", type=int, default=max(1, min(5, (os.cpu_count() or 2) // 2)))
    args = ap.parse_args()
    out = Path(args.out).resolve()
    if str(out).startswith(str((REPO / "assets-source").resolve())):
        sys.exit("ERROR: --out must not be inside assets-source/")
    out.mkdir(parents=True, exist_ok=True)
    prov = json.loads(PROVENANCE.read_text())
    prompts = PROMPTS.read_text()
    rows = [r for r in prov["files"] if not args.only or re.search(args.only, r["cue_id"])]
    with ProcessPoolExecutor(max_workers=args.jobs) as pool:
        items = list(pool.map(process_row, rows, [str(out)] * len(rows),
                              [prompt_bpm(r["cue_id"], prompts) for r in rows]))
    items.sort(key=lambda i: i["cue_id"])
    ffv = proc.run(["ffmpeg", "-version"]).stdout.decode().splitlines()[0]
    manifest = {
        "schema": "parble.music-loops/1", "pipeline_version": PIPELINE_VERSION,
        "tools": {"ffmpeg": ffv, "numpy": np.__version__, "python": sys.version.split()[0],
                  "music_loops_py_sha256": proc.sha256(Path(__file__))},
        "parameters": {
            "sample_rate": SR, "channels": 2, "opus": " ".join(OPUS), "aac": " ".join(AAC),
            "lufs_target": LUFS_TARGET, "lufs_tolerance": LUFS_TOL, "true_peak_ceiling_db": TP_CEILING,
            "lufs_method": "integrated over one loop body [S, E)",
            "xfade_s": XFADE_S, "context_s": CONTEXT_S, "min_loop_s": MIN_LOOP_S,
            "start_range_s": [START_MIN_S, START_MAX_S], "tail_db": TAIL_DB, "postroll_s": POSTROLL_S,
            "seam_gate": {"window_s": SEAM_WIN_S, "step_ratio_max": SEAM_STEP_RATIO_MAX,
                          "rms_excess_db_max": SEAM_RMS_EXCESS_DB_MAX, "rms_abs_db_max": SEAM_RMS_ABS_DB_MAX},
        },
        "items": items,
    }
    (out / "manifest.json").write_text(json.dumps(manifest, indent=1) + "\n")
    if out == DEFAULT_OUT.resolve() and not args.only:
        prov["loopDerivatives"] = [{
            "cue_id": i["cue_id"], "source_sha256": i["source"]["sha256"],
            "loop_start_sample": i["loop"]["start_sample"], "loop_end_sample": i["loop"]["end_sample"],
            "sample_rate": SR, "gain_db": i["gain_db"],
            "lufs": i["files"]["ogg"]["lufs"], "true_peak_db": max(f["true_peak_db"] for f in i["files"].values()),
            "outputs": {ext: {"file": f"audition/music-loops/{f['path']}", "sha256": f["sha256"]}
                        for ext, f in i["files"].items()},
            "pipeline": "scripts/audio/music_loops.py", "manifest": "audition/music-loops/manifest.json",
            "status": {"technical": "pass" if not i["issues"] else "fail",
                       "creative": "pending Paul's listening approval", "owner": "pending"},
        } for i in items]
        PROVENANCE.write_text(json.dumps(prov, indent=2) + "\n")
    bad = [i for i in items if i["issues"]]
    for i in items:
        lp, o = i["loop"], i["files"]["ogg"]
        print(f"{i['cue_id']}: S={lp['start_sample']} E={lp['end_sample']} ({lp['length_s']} s, "
              f"{lp['bars']} bars @ {lp['bpm']} BPM {lp['beats_per_bar']}/4) sim={lp['spectral_similarity']} "
              f"lufs={o['lufs']} tp={o['true_peak_db']} seam={o['seam']} issues={i['issues']}")
    print(f"music loops: {len(items) - len(bad)} of {len(items)} pass -> {out}/manifest.json")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
