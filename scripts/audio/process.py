#!/usr/bin/env python3
"""Process raw ElevenLabs SFX originals into runtime derivatives for the audition page.

Reads every provenance-group-*.json, never writes to the source tree, and for each file:

  decode -> 48 kHz mono (amplitude average of channels, so peaks do not rise)
  one-shot: trim leading silence (<5 ms before onset), trim the tail with a cosine fade,
            cap length to the cue-sheet range
  loop:     keep the raw file when it already passes check-audio-loops.py, otherwise fold
            the tail over the head with repair-loop.py (equal-power crossfade) until it passes
  level:    linear gain to the cue's LUFS target, limited by its true-peak ceiling
            (loudnorm is not used: in linear mode it silently stops honouring both targets)
  export:   <out>/<family>/<stem>.ogg (Opus 96k) and .m4a (AAC 96k), 24-bit WAV master
            under <masters>; remeasure the decoded encodes and trim once if the encode
            overshoots the true-peak ceiling

Targets (LUFS, tolerance, true-peak ceiling, duration range) are parsed from the cue sheet.
One-shot loudness is the "active region" rule: the trimmed file tiled end to end with aloop
and measured by ebur128, which is the K-weighted loudness of the active region itself.

Rows whose provenance record carries "status": "rejected" (an owner verdict from the audition
board) are not processed: they go to the manifest's "rejected" list with the recorded reason
and their old derivatives are removed, so the audition page shows only live candidates.

Writes <out>/manifest.json and <out>/processing-report.md. Output is deterministic: no
timestamps, bitexact muxers, sorted items. Usage:

    scripts/audio/process.py                       # real data -> audition/sfx
    scripts/audio/process.py --provenance-glob 'tmp/x/*.json' --out /tmp/sfx
"""

from __future__ import annotations

import argparse
import array
import glob
import hashlib
import json
import math
import os
import re
import shutil
import subprocess
import sys
import tempfile
from concurrent.futures import ProcessPoolExecutor
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import numpy as np

SR = 48000
REPO = Path(__file__).resolve().parents[2]
SKILL = Path.home() / ".claude/skills/create-game-audio-assets/scripts"
PIPELINE_VERSION = 1

ONSET_DB = -36.0  # onset = first sample within this many dB of the file peak
PREROLL_S = 0.001
TAIL_DB = -45.0  # tail ends at the last 5 ms window within this many dB of the loudest window
TP_MARGIN_DB = 0.3  # aim this far under the ceiling on the master; encodes are remeasured
LEAD_MAX_MS = 5.0
# Second onset: a 5 ms window that rises this far above the decay minimum since the first
# hit while reaching within REONSET_PEAK_DB of the first hit's window level.
REONSET_RISE_DB = 10.0
REONSET_PEAK_DB = -15.0
REONSET_DIP_DB = 3.0
LOOP_FADES_S = (0.5, 0.75, 1.0, 1.5)
TILE_MIN_S = 4.0
# Extended-loop search (takes whose clean region is shorter than the cue's loop range, or whose
# plain crossfade fails): sub-regions on a 0.1 s grid, at least EXT_MIN_REGION_S long, whose head
# and tail levels agree within EXT_EDGE_DB and whose interior stays within EXT_RANGE_DB, are
# chained with equal-power crossfades into a longer take and folded like any other loop.
EXT_GRID_S = 0.1
EXT_MIN_REGION_S = 2.0
EXT_EDGE_S = 0.5
EXT_EDGE_DB = 3.0
EXT_RANGE_DB = 20.0
EXT_MAX_CANDIDATES = 10
# Leveler for takes whose envelope swings more than the checker's 24 dB dropout rule allows:
# 100 ms windows below the 90th-percentile level minus LEVEL_FLOOR_DB are raised toward that
# floor by at most LEVEL_BOOST_DB, windows above it plus LEVEL_CEIL_DB are pulled down by at most
# LEVEL_CUT_DB, with gain interpolated between window centres.
LEVEL_FLOOR_DB = 12.0
LEVEL_CEIL_DB = 6.0
LEVEL_BOOST_DB = 24.0
LEVEL_CUT_DB = 12.0
# Opus loop encode: the loop is encoded with this much of its own tail before it and head after
# it, then the Ogg pre-skip and final granule position are patched so the decoder discards both.
OPUS_CONTEXT_S = 0.1
OPUS_ARGS = ["-c:a", "libopus", "-b:a", "96k", "-vbr", "on", "-application", "audio"]
AAC_ARGS = ["-c:a", "aac", "-b:a", "96k"]
BITEXACT = ["-map_metadata", "-1", "-fflags", "+bitexact", "-flags:a", "+bitexact"]


@dataclass(frozen=True)
class Target:
    cue_id: str
    family: str
    loop: bool
    min_s: float
    max_s: float
    lufs: float
    tol: float
    tp: float


# ---------------------------------------------------------------- tooling


def run(cmd: list[str]) -> subprocess.CompletedProcess[bytes]:
    proc = subprocess.run(cmd, capture_output=True)
    if proc.returncode != 0:
        raise RuntimeError(f"{' '.join(cmd[:6])}... failed: {proc.stderr.decode()[-600:]}")
    return proc


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def preflight(args: argparse.Namespace) -> dict[str, str]:
    for tool in ("ffmpeg", "ffprobe"):
        if shutil.which(tool) is None:
            sys.exit(f"ERROR: {tool} not on PATH")
    enc = run(["ffmpeg", "-hide_banner", "-encoders"]).stdout.decode()
    for name in ("libopus", " aac "):
        if name not in enc:
            sys.exit(f"ERROR: ffmpeg lacks the {name.strip()} encoder")
    for script in (args.loop_checker, args.repair_loop):
        if not Path(script).is_file():
            sys.exit(f"ERROR: skill script missing: {script} (set --loop-checker/--repair-loop)")
    ffv = run(["ffmpeg", "-version"]).stdout.decode().splitlines()[0]
    return {
        "ffmpeg": ffv,
        "python": sys.version.split()[0],
        "loop_checker_sha256": sha256(Path(args.loop_checker)),
        "repair_loop_sha256": sha256(Path(args.repair_loop)),
        "process_py_sha256": sha256(Path(__file__)),
    }


# ---------------------------------------------------------------- cue sheet


def parse_cue_sheet(path: Path) -> dict[str, Target]:
    targets: dict[str, Target] = {}
    rng = r"([\d.]+)\s*[–-]\s*([\d.]+)\s*(ms|s)\b"
    for line in path.read_text().splitlines():
        if not line.startswith("| `"):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) < 7:
            continue
        cue_id, family, loop_cell, dur, _var, loud, peak = (c.strip("`") for c in cells[:7])
        loop = loop_cell == "loop"
        m = re.search(r"runtime loop " + rng, dur) if loop else re.search(r"trimmed to " + rng, dur)
        lm = re.search(r"(-?[\d.]+) LUFS.*±\s*([\d.]+)", loud)
        pm = re.search(r"(-?[\d.]+)\s*dBTP", peak)
        if not (m and lm and pm):
            raise SystemExit(f"cue sheet row not parseable: {line}")
        scale = 0.001 if m.group(3) == "ms" else 1.0
        targets[cue_id] = Target(
            cue_id, family, loop, float(m.group(1)) * scale, float(m.group(2)) * scale,
            float(lm.group(1)), float(lm.group(2)), float(pm.group(1)),
        )
    if not targets:
        raise SystemExit(f"no cue rows parsed from {path}")
    return targets


# ---------------------------------------------------------------- provenance


def load_provenance(pattern: str, required: bool = True) -> list[dict[str, Any]]:
    files = sorted(glob.glob(pattern))
    if not files and not required:
        return []
    if not files:
        sys.exit(f"ERROR: no provenance files match {pattern!r}; nothing to process")
    entries: list[dict[str, Any]] = []
    for pf in files:
        data = json.loads(Path(pf).read_text())
        rows = data if isinstance(data, list) else data.get("files", [])
        for row in rows:
            if isinstance(row, dict) and row.get("cue_id") and row.get("file"):
                entries.append({**row, "_provenance": pf, "_provider": _provider(data, row)})
    return entries


def _provider(data: Any, row: dict[str, Any]) -> dict[str, Any]:
    top = data if isinstance(data, dict) else {}
    keys = ("provider", "model", "endpoint", "output_format", "tier")
    return {k: row.get(k, top.get(k)) for k in keys if row.get(k, top.get(k)) is not None}


def rejected_entry(row: dict[str, Any], repo: str) -> dict[str, Any]:
    return {"cue_id": row["cue_id"], "family": row.get("family"), "stem": Path(row["file"]).stem,
            "file": row["file"], "provenance": os.path.relpath(row["_provenance"], repo),
            "reason": row.get("rejected_reason"), "rejected_by": row.get("rejected_by"),
            "rejected_on": row.get("rejected_on")}


def resolve_source(row: dict[str, Any], repo: Path) -> Path | None:
    p = Path(row["file"])
    if p.is_absolute():
        return p if p.is_file() else None
    prov_dir = Path(row["_provenance"]).resolve().parent
    for base in (repo, prov_dir, *prov_dir.parents):
        if (base / p).is_file():
            return base / p
    return None


# ---------------------------------------------------------------- audio io


def channels_of(path: Path) -> int:
    out = run(["ffprobe", "-v", "error", "-select_streams", "a:0", "-show_entries",
               "stream=channels", "-of", "csv=p=0", str(path)]).stdout.decode().strip()
    return int(out.split(",")[0])


def decode(path: Path) -> array.array:
    ch = channels_of(path)
    af = [f"aresample={SR}"]
    if ch > 1:
        gains = "+".join(f"{1 / ch:.10f}*c{i}" for i in range(ch))
        af.insert(0, f"pan=mono|c0={gains}")
    raw = run(["ffmpeg", "-nostdin", "-v", "error", "-i", str(path), "-af", ",".join(af),
               "-ac", "1", "-f", "f32le", "-acodec", "pcm_f32le", "-"]).stdout
    samples = array.array("f")
    samples.frombytes(raw)
    return samples


def write_pcm(samples: array.array, path: Path, codec: str) -> None:
    with tempfile.NamedTemporaryFile(suffix=".f32", delete=False) as tmp:
        tmp.write(samples.tobytes())
    try:
        run(["ffmpeg", "-nostdin", "-v", "error", "-y", "-f", "f32le", "-ar", str(SR), "-ac", "1",
             "-i", tmp.name, "-c:a", codec, *BITEXACT, str(path)])
    finally:
        os.unlink(tmp.name)


def encode(master: Path, dest: Path, codec_args: list[str]) -> None:
    tmp = dest.with_name(dest.stem + ".tmp" + dest.suffix)
    run(["ffmpeg", "-nostdin", "-v", "error", "-y", "-i", str(master), "-ar", str(SR), "-ac", "1",
         *codec_args, *BITEXACT, str(tmp)])
    os.replace(tmp, dest)


def _ogg_crc_table() -> list[int]:
    table = []
    for i in range(256):
        r = i << 24
        for _ in range(8):
            r = ((r << 1) ^ 0x04C11DB7) & 0xFFFFFFFF if r & 0x80000000 else (r << 1) & 0xFFFFFFFF
        table.append(r)
    return table


_OGG_CRC = _ogg_crc_table()


def _ogg_pages(data: bytearray) -> list[tuple[int, int]]:
    pages, i = [], 0
    while i < len(data):
        if data[i:i + 4] != b"OggS":
            raise RuntimeError("not an Ogg stream")
        nseg = data[i + 26]
        end = i + 27 + nseg + sum(data[i + 27:i + 27 + nseg])
        pages.append((i, end))
        i = end
    return pages


def _ogg_refresh_crc(data: bytearray, start: int, end: int) -> None:
    data[start + 22:start + 26] = b"\0\0\0\0"
    crc = 0
    for b in data[start:end]:
        crc = ((crc << 8) & 0xFFFFFFFF) ^ _OGG_CRC[(crc >> 24) ^ b]
    data[start + 22:start + 26] = crc.to_bytes(4, "little")


def patch_opus_trim(path: Path, lead: int, length: int) -> None:
    """Make an Ogg Opus stream decode to exactly `length` samples that start `lead` samples
    after the encoder's own pre-skip: raise OpusHead pre-skip by `lead` and set the final
    granule position to pre-skip + length. Both edited pages are re-CRC'd."""
    data = bytearray(path.read_bytes())
    pages = _ogg_pages(data)
    s0, e0 = pages[0]
    body = s0 + 27 + data[s0 + 26]
    if data[body:body + 8] != b"OpusHead":
        raise RuntimeError("first Ogg page is not OpusHead")
    pre_skip = int.from_bytes(data[body + 10:body + 12], "little") + lead
    data[body + 10:body + 12] = pre_skip.to_bytes(2, "little")
    _ogg_refresh_crc(data, s0, e0)
    sl, el = pages[-1]
    granule = pre_skip + length
    prev = int.from_bytes(data[pages[-2][0] + 6:pages[-2][0] + 14], "little", signed=True) \
        if len(pages) > 2 else 0
    coded = int.from_bytes(data[sl + 6:sl + 14], "little", signed=True)
    if not prev <= granule <= coded:
        raise RuntimeError(f"cannot trim Opus stream: granule {granule} outside [{prev}, {coded}]")
    data[sl + 6:sl + 14] = granule.to_bytes(8, "little", signed=True)
    _ogg_refresh_crc(data, sl, el)
    path.write_bytes(bytes(data))


def encode_opus_loop(y: array.array, dest: Path, work: Path) -> None:
    """Opus encode of a loop that decodes gaplessly: the stream carries OPUS_CONTEXT_S of the
    loop's own tail before it and head after it, so the codec state (and the lookahead at the
    end) matches what the circular signal needs; pre-skip and the final granule position then
    discard that context and the decoder returns exactly the loop."""
    ctx = int(OPUS_CONTEXT_S * SR)
    arr = _np(y)
    padded = np.concatenate([arr[-ctx:], arr, arr[:ctx]])
    src = work / "opus_loop_src.wav"
    write_pcm(_arr(padded), src, "pcm_s24le")
    tmp = dest.with_name(dest.stem + ".tmp" + dest.suffix)
    run(["ffmpeg", "-nostdin", "-v", "error", "-y", "-i", str(src), "-ar", str(SR), "-ac", "1",
         *OPUS_ARGS, *BITEXACT, str(tmp)])
    patch_opus_trim(tmp, ctx, len(arr))
    os.replace(tmp, dest)


def measure(path: Path, tile_samples: int | None = None) -> tuple[float, float]:
    """(LUFS, true peak dBTP). With tile_samples, LUFS is of the file tiled to >= 4 s."""

    def ebur(af: str) -> str:
        return run(["ffmpeg", "-nostdin", "-hide_banner", "-i", str(path), "-af", af,
                    "-f", "null", "-"]).stderr.decode()

    plain = ebur("ebur128=peak=true")
    tp = float(re.findall(r"Peak:\s+(-?[\d.]+|-inf)", plain)[-1].replace("-inf", "-200"))
    if tile_samples:
        loops = math.ceil(TILE_MIN_S * SR / max(tile_samples, 1))
        text = ebur(f"aloop=loop={loops}:size={tile_samples},ebur128")
    else:
        text = plain
    lufs = float(re.findall(r"I:\s+(-?[\d.]+) LUFS", text)[-1])
    return lufs, tp


def peak_abs(x: array.array) -> float:
    return max((abs(v) for v in x), default=0.0)


def scaled(x: array.array, gain_db: float) -> array.array:
    g = 10 ** (gain_db / 20)
    return array.array("f", (v * g for v in x))


def leading_silence_ms(x: array.array) -> float:
    pk = peak_abs(x)
    thr = pk * 10 ** (ONSET_DB / 20)
    for i, v in enumerate(x):
        if abs(v) >= thr:
            return i / SR * 1000
    return len(x) / SR * 1000


# ---------------------------------------------------------------- one-shots


def second_onset_ms(y: array.array) -> float | None:
    """Start of a second hit (flam, double hit or bounce) in 5 ms RMS windows, or None.

    A new onset is a rise of REONSET_RISE_DB from the decay floor after an earlier local
    peak, where the floor sits at least REONSET_DIP_DB under that peak and the new hit
    reaches within -REONSET_PEAK_DB of the loudest window.
    """
    win = int(0.005 * SR)
    env = [20 * math.log10(math.sqrt(sum(v * v for v in y[s:s + win]) / win) + 1e-12)
           for s in range(0, len(y) - win + 1, win)]
    if not env:
        return None
    top = max(env)
    peak = floor = env[0]
    for i in range(1, len(env)):
        if env[i] < floor:
            floor = env[i]
        if (env[i] - floor >= REONSET_RISE_DB and peak - floor >= REONSET_DIP_DB
                and env[i] >= top + REONSET_PEAK_DB):
            return round(i * win / SR * 1000, 1)
        if env[i] > peak and floor >= peak - REONSET_DIP_DB:
            peak = floor = env[i]
    return None


def trim_one_shot(x: array.array, t: Target) -> tuple[array.array, list[str]]:
    notes: list[str] = []
    pk = peak_abs(x)
    thr = pk * 10 ** (ONSET_DB / 20)
    onset = next(i for i, v in enumerate(x) if abs(v) >= thr)
    start = max(0, onset - int(PREROLL_S * SR))
    win = int(0.005 * SR)
    rms = []
    for s in range(start, len(x), win):
        chunk = x[s:s + win]
        rms.append(math.sqrt(sum(v * v for v in chunk) / len(chunk)))
    loud = max(rms)
    last = max(i for i, r in enumerate(rms) if r >= loud * 10 ** (TAIL_DB / 20))
    natural = (last + 1) * win
    length = min(max(natural, int(t.min_s * SR)), int(t.max_s * SR), len(x) - start)
    if natural > t.max_s * SR:
        notes.append(f"tail capped from {natural / SR:.3f}s to {t.max_s:.3f}s")
    y = array.array("f", x[start:start + length])
    fade = min(max(int(0.2 * length), int(0.010 * SR)), int(0.060 * SR), length)
    for i in range(fade):
        y[length - fade + i] *= 0.5 * (1 + math.cos(math.pi * (i + 1) / fade))
    pre = onset - start
    for i in range(pre):
        y[i] *= 0.5 * (1 - math.cos(math.pi * (i + 0.5) / max(pre, 1)))
    notes.append(f"trim start {start / SR * 1000:.1f} ms, length {length / SR * 1000:.1f} ms, "
                 f"cosine tail fade {fade / SR * 1000:.1f} ms")
    return y, notes


# ---------------------------------------------------------------- loops


def loop_check(checker: str, path: Path) -> tuple[bool, str]:
    proc = subprocess.run([sys.executable, checker, str(path)], capture_output=True)
    out = (proc.stdout.decode() + proc.stderr.decode()).strip()
    return proc.returncode == 0, out.replace(str(path.parent) + "/", "")


def longest_clean_run(x: array.array) -> tuple[int, int]:
    """(start, length) of the longest run of 100 ms windows with no dropout, using the
    loop checker's default rule (window more than 24 dB under the loudest window)."""
    win = int(0.1 * SR)
    lv = [math.sqrt(sum(v * v for v in x[s:s + win]) / win) + 1e-12
          for s in range(0, len(x) - win + 1, win)]
    if not lv:
        return 0, len(x)
    thr = max(lv) * 10 ** (-24 / 20)
    best, cur = (0, 0), None
    for i, v in enumerate(lv + [0.0]):
        if v >= thr and cur is None:
            cur = i
        elif v < thr and cur is not None:
            if i - cur > best[1]:
                best = (cur, i - cur)
            cur = None
    if best[1] == len(lv):
        return 0, len(x)
    return best[0] * win, best[1] * win


def _np(x: array.array) -> np.ndarray:
    return np.frombuffer(x, dtype=np.float32)


def _arr(x: np.ndarray) -> array.array:
    out = array.array("f")
    out.frombytes(np.ascontiguousarray(x, dtype=np.float32).tobytes())
    return out


def window_db(x: np.ndarray) -> np.ndarray:
    """RMS level of each whole 100 ms window, in dB re full scale (the loop checker's windows)."""
    win = int(0.1 * SR)
    n = len(x) // win
    w = x[: n * win].astype(np.float64).reshape(n, win)
    return 10 * np.log10((w * w).mean(axis=1) + 1e-12)


def xfade_concat(a: np.ndarray, b: np.ndarray, fade_n: int) -> np.ndarray:
    """a followed by b with the last/first fade_n samples blended equal-power (sin^2 + cos^2 = 1)."""
    ang = (np.arange(fade_n) + 0.5) / fade_n * (math.pi / 2)
    blend = a[-fade_n:] * np.cos(ang) + b[:fade_n] * np.sin(ang)
    return np.concatenate([a[:-fade_n], blend, b[fade_n:]]).astype(np.float32)


def fold_tail_over_head(arr: np.ndarray, fade_n: int) -> np.ndarray:
    """repair-loop.py's equal-power fold on float samples: the last fade_n samples are blended
    into the head and dropped, so the result wraps continuously."""
    kept = len(arr) - fade_n
    ang = (np.arange(fade_n) + 0.5) / fade_n * (math.pi / 2)
    out = arr[:kept].astype(np.float32).copy()
    out[:fade_n] = arr[:fade_n] * np.sin(ang) + arr[kept:kept + fade_n] * np.cos(ang)
    return out


def level_loop_source(x: array.array) -> tuple[array.array, dict[str, Any]]:
    """Slow gain ride that keeps the 100 ms level envelope within the loop checker's dropout rule."""
    arr = _np(x)
    env = window_db(arr)
    ref = float(np.percentile(env, 90))
    gain = np.clip((ref - LEVEL_FLOOR_DB) - env, 0, LEVEL_BOOST_DB) \
        - np.clip(env - (ref + LEVEL_CEIL_DB), 0, LEVEL_CUT_DB)
    win = int(0.1 * SR)
    centres = (np.arange(len(env)) + 0.5) * win
    g = np.interp(np.arange(len(arr)), centres, gain)
    out = arr.astype(np.float64) * 10 ** (g / 20)
    info = {"leveler": {"ref_db": round(ref, 1), "floor_db": -LEVEL_FLOOR_DB,
                        "ceil_db": LEVEL_CEIL_DB, "max_boost_db": round(float(gain.max()), 1),
                        "max_cut_db": round(float(-gain.min()), 1)}}
    return _arr(out), info


def extended_candidates(pre: array.array) -> list[tuple[int, int]]:
    """(start, end) sample ranges of `pre` to repeat, longest first: head and tail levels
    agree and the interior stays inside EXT_RANGE_DB, so chained copies join without a
    level step and without a dropout."""
    arr = _np(pre)
    grid = int(EXT_GRID_S * SR)
    edge = int(EXT_EDGE_S * SR)
    cands: list[tuple[int, float, int, int]] = []
    env = window_db(arr)
    n = len(arr) // grid
    for a in range(0, n):
        for b in range(a + int(EXT_MIN_REGION_S / EXT_GRID_S), n + 1):
            lo, hi = a * grid, b * grid
            head = 10 * np.log10(float((arr[lo:lo + edge].astype(np.float64) ** 2).mean()) + 1e-12)
            tail = 10 * np.log10(float((arr[hi - edge:hi].astype(np.float64) ** 2).mean()) + 1e-12)
            diff = abs(head - tail)
            if diff > EXT_EDGE_DB:
                continue
            inner = env[a:b]
            if len(inner) and float(inner.max() - inner.min()) > EXT_RANGE_DB:
                continue
            cands.append((hi - lo, diff, lo, hi))
    cands.sort(key=lambda c: (-c[0], c[1], c[2]))
    return [(lo, hi) for _, _, lo, hi in cands[:EXT_MAX_CANDIDATES]]


def fold_attempts(src: np.ndarray, t: Target, work: Path, args: argparse.Namespace,
                  attempts: list[dict[str, Any]], tag: str) -> tuple[Any, ...] | None:
    """Try each crossfade length on src[:max+fade]; first pass wins. Returns (path, fade, ok)."""
    max_n, min_n = int(t.max_s * SR), int(t.min_s * SR)
    result = None
    for fade in LOOP_FADES_S:
        fade_n = int(fade * SR)
        region = src[: min(len(src), max_n + fade_n)]
        if len(region) - fade_n < min_n:
            continue
        raw = work / f"loop_raw_{tag}.wav"
        write_pcm(_arr(region), raw, "pcm_s16le")
        rep = work / f"loop_xf_{tag}_{fade}.wav"
        run([sys.executable, args.repair_loop, str(raw), str(rep), "--seconds", str(fade)])
        ok, _ = loop_check(args.loop_checker, rep)
        attempts.append({"mode": tag, "crossfade_s": fade, "pass": ok})
        result = (rep, fade, ok)
        if ok:
            break
    return result


def extended_search(pre: array.array, t: Target, work: Path, args: argparse.Namespace,
                    attempts: list[dict[str, Any]]) -> tuple[Any, ...] | None:
    """Build the loop from repeated, crossfaded passes of a well-matched sub-region."""
    max_n = int(t.max_s * SR)
    arr = _np(pre)
    for lo, hi in extended_candidates(pre):
        region = arr[lo:hi]
        for fade in LOOP_FADES_S:
            fade_n = int(fade * SR)
            if fade_n * 3 > len(region):
                continue
            ext = region
            passes = 1
            while len(ext) < max_n + fade_n:
                ext = xfade_concat(ext, region, fade_n)
                passes += 1
            tag = f"ext{lo}_{hi}_{fade}"
            res = fold_attempts_single(ext, fade, t, work, args, attempts, tag)
            if res is not None and res[2]:
                return res[0], fade, True, {"region_s": [round(lo / SR, 2), round(hi / SR, 2)],
                                            "passes": passes}
    return None


def fold_attempts_single(ext: np.ndarray, fade: float, t: Target, work: Path,
                         args: argparse.Namespace, attempts: list[dict[str, Any]],
                         tag: str) -> tuple[Any, ...] | None:
    max_n, min_n = int(t.max_s * SR), int(t.min_s * SR)
    fade_n = int(fade * SR)
    region = ext[: min(len(ext), max_n + fade_n)]
    if len(region) - fade_n < min_n:
        return None
    raw = work / f"loop_raw_{tag}.wav"
    write_pcm(_arr(region), raw, "pcm_s16le")
    rep = work / f"loop_xf_{tag}.wav"
    run([sys.executable, args.repair_loop, str(raw), str(rep), "--seconds", str(fade)])
    ok, _ = loop_check(args.loop_checker, rep)
    attempts.append({"mode": "extended", "crossfade_s": fade, "pass": ok})
    return rep, fade, ok


def build_loop(x: array.array, t: Target, gain_db: float, work: Path,
               args: argparse.Namespace) -> tuple[array.array, dict[str, Any], list[str]]:
    notes: list[str] = []
    pre = full = scaled(x, gain_db)
    raw = work / "loop_raw.wav"
    max_n, min_n = int(t.max_s * SR), int(t.min_s * SR)
    attempts: list[dict[str, Any]] = []
    if min_n <= len(pre) <= max_n:
        write_pcm(pre, raw, "pcm_s16le")
        ok, out = loop_check(args.loop_checker, raw)
        attempts.append({"mode": "raw", "crossfade_s": 0.0, "pass": ok})
        if ok:
            notes.append("raw take already loops cleanly; no crossfade")
            return pre, {"pass": True, "crossfade_s": 0.0, "attempts": attempts}, notes
    region_start, region_len = longest_clean_run(pre)
    if region_len < len(pre):
        notes.append(f"interior dropouts in take; loop region limited to longest clean run "
                     f"{region_start / SR:.2f}s + {region_len / SR:.2f}s")
        pre = array.array("f", pre[region_start:region_start + region_len])
    result = fold_attempts(_np(pre), t, work, args, attempts, "plain")
    extra: dict[str, Any] = {}
    if result is None or not result[2] or len(decode(result[0])) < min_n:
        # The plain fold failed or came out under the sheet's length: chain crossfaded passes of
        # a sub-region whose head and tail levels match (levelling the take first when its
        # envelope swings more than the dropout rule allows).
        legacy = result
        ext = extended_search(pre, t, work, args, attempts)
        if ext is None:
            lev, info = level_loop_source(full)
            lev_start, lev_len = longest_clean_run(lev)
            lev = array.array("f", lev[lev_start:lev_start + lev_len])
            ext = extended_search(lev, t, work, args, attempts)
            if ext is not None:
                extra.update(info)
                notes.append(f"envelope swing needed levelling: {info['leveler']}")
        if ext is not None:
            rep, fade, ok, found = ext
            extra["extended"] = found
            notes.append(f"extended loop: {found['passes']} crossfaded passes of "
                         f"{found['region_s'][0]}-{found['region_s'][1]}s")
            result = (rep, fade, ok)
        elif result is None and len(pre) > 4 * int(LOOP_FADES_S[0] * SR):
            fade = LOOP_FADES_S[0]
            notes.append(f"clean region {len(pre) / SR:.2f}s too short for a {t.min_s}s loop; "
                         f"crossfading anyway, duration falls short")
            write_pcm(pre, raw, "pcm_s16le")
            rep = work / f"loop_xf_{fade}.wav"
            run([sys.executable, args.repair_loop, str(raw), str(rep), "--seconds", str(fade)])
            ok, _ = loop_check(args.loop_checker, rep)
            attempts.append({"mode": "short", "crossfade_s": fade, "pass": ok})
            result = (rep, fade, ok)
        else:
            result = legacy
    if result is None:
        notes.append(f"clean region {len(pre) / SR:.2f}s too short to loop")
        return pre, {"pass": False, "crossfade_s": None, "attempts": attempts, **extra}, notes
    rep, fade, ok = result
    notes.append(f"repair-loop.py equal-power tail-over-head crossfade {fade}s")
    return decode(rep), {"pass": ok, "crossfade_s": fade, "attempts": attempts, **extra}, notes


def loop_rotations(y: array.array) -> list[int]:
    """Candidate circular-rotation offsets: the lowest-slope rising zero crossing near
    20/40/60/80 % of the loop."""
    out = []
    span = int(0.010 * SR)
    for frac in (0.2, 0.4, 0.6, 0.8):
        c = int(frac * len(y))
        best = None
        for i in range(max(1, c - span), min(len(y), c + span)):
            if y[i - 1] < 0 <= y[i]:
                slope = y[i] - y[i - 1]
                if best is None or slope < best[0]:
                    best = (slope, i)
        if best:
            out.append(best[1])
    return out


# ---------------------------------------------------------------- per item


def process_item(row: dict[str, Any], t: Target | None, args: argparse.Namespace) -> dict[str, Any]:
    out_dir, masters = Path(args.out), Path(args.masters)
    src = resolve_source(row, Path(args.repo))
    stem = Path(row["file"]).stem
    family = row.get("family") or (t.family if t else "unknown")
    item: dict[str, Any] = {
        "cue_id": row["cue_id"], "family": family, "stem": stem,
        "source": {"file": row["file"], "provenance": os.path.relpath(row["_provenance"], args.repo),
                   "sha256_recorded": row.get("sha256"), **row["_provider"]},
        "prompt": row.get("prompt"), "issues": [], "warnings": [], "processing": [],
    }
    if t is None:
        item["issues"].append("cue_id not in cue sheet; not processed")
        return item
    loop = bool(row.get("loop", t.loop))
    if loop != t.loop:
        item["issues"].append(f"provenance loop={loop} disagrees with cue sheet loop={t.loop}")
    item["loop"] = t.loop
    item["target"] = {"lufs": t.lufs, "tolerance_lu": t.tol, "true_peak_ceiling_db": t.tp,
                      "duration_s": [t.min_s, t.max_s],
                      "lufs_method": "integrated over whole loop" if t.loop
                      else "active region (trimmed file tiled with aloop, ebur128)"}
    if src is None:
        item["issues"].append(f"source file not found: {row['file']}")
        return item
    digest = sha256(src)
    item["source"]["sha256"] = digest
    item["source"]["bytes"] = src.stat().st_size
    if row.get("sha256") and row["sha256"] != digest:
        item["issues"].append("source sha256 does not match provenance record")

    with tempfile.TemporaryDirectory() as wd:
        work = Path(wd)
        x = decode(src)
        if peak_abs(x) == 0:
            item["issues"].append("source decodes to silence")
            return item
        item["processing"].append(f"decode -> {SR} Hz mono (channel average), "
                                  f"{len(x) / SR:.3f}s")
        tmp = work / "probe.wav"
        if t.loop:
            write_pcm(x, tmp, "pcm_f32le")
            lufs0, tp0 = measure(tmp)
            gain = min(t.lufs - lufs0, t.tp - TP_MARGIN_DB - tp0)
            y, lc, notes = build_loop(x, t, gain, work, args)
            item["processing"] += notes
            item["loop_check_master"] = lc
            tile = None
        else:
            y, notes = trim_one_shot(x, t)
            item["processing"] += notes
            tile = len(y)
            item["second_onset_ms"] = second_onset_ms(y)
            write_pcm(y, tmp, "pcm_f32le")
            lufs0, tp0 = measure(tmp, tile)
            gain = min(t.lufs - lufs0, t.tp - TP_MARGIN_DB - tp0)
            y = scaled(y, gain)
        # Exact level trim on the final waveform (loop crossfades shift loudness slightly).
        write_pcm(y, tmp, "pcm_f32le")
        lufs1, tp1 = measure(tmp, tile)
        trim = min(t.lufs - lufs1, t.tp - TP_MARGIN_DB - tp1)
        total_gain = gain + trim
        y = scaled(y, trim)

        fam_out = out_dir / family
        fam_out.mkdir(parents=True, exist_ok=True)
        (masters / family).mkdir(parents=True, exist_ok=True)
        master = masters / family / f"{stem}.wav"
        files: dict[str, Any] = {}
        variants: list[tuple[str, array.array]] = [("as built", y)]
        if t.loop:
            # Fallback candidates, tried in order only while a loop check fails: the loop
            # rotated to a low-slope zero crossing (moves the seam and the head/tail windows),
            # then re-folded with a longer equal-power tail-over-head crossfade.
            for r in loop_rotations(y):
                variants.append((f"rotated to sample {r}", array.array("f", y[r:] + y[:r])))
            for fade in LOOP_FADES_S:
                if len(y) - int(fade * SR) >= int(t.min_s * SR):
                    variants.append((f"re-folded with a {fade}s crossfade",
                                     _arr(fold_tail_over_head(_np(y), int(fade * SR)))))
        vi, extra_db, tp_trims, best, final = 0, 0.0, 0, None, False
        cur: array.array | None = None
        while True:
            label, vbase = variants[vi]
            y = cur if cur is not None else (scaled(vbase, extra_db) if extra_db else vbase)
            cur = None
            write_pcm(y, master, "pcm_s24le")
            over = 0.0
            for ext, cargs in (("ogg", OPUS_ARGS), ("m4a", AAC_ARGS)):
                dest = fam_out / f"{stem}.{ext}"
                if t.loop and ext == "ogg":
                    encode_opus_loop(y, dest, work)
                else:
                    encode(master, dest, cargs)
                dec = decode(dest)
                dwav = work / f"dec_{ext}.wav"
                write_pcm(dec, dwav, "pcm_f32le")
                lufs_e, tp_e = measure(dwav, None if t.loop else len(dec))
                files[ext] = {"path": f"{family}/{stem}.{ext}", "sha256": sha256(dest),
                              "bytes": dest.stat().st_size, "lufs": round(lufs_e, 1),
                              "true_peak_db": round(tp_e, 1), "decoded_samples": len(dec),
                              "duration_s": round(len(dec) / SR, 4),
                              "leading_silence_ms": round(leading_silence_ms(dec), 2)}
                if t.loop:
                    ok, report = loop_check(args.loop_checker, dwav)
                    files[ext]["loop_check"] = {"pass": ok, "output": report}
                over = max(over, tp_e - t.tp)
            if t.loop:
                m_ok, m_out = loop_check(args.loop_checker, master)
                item["loop_check_master"].update({"pass": m_ok, "output": m_out, "variant": label})
            if over > 0 and tp_trims < 3:
                tp_trims += 1
                item["processing"].append(f"encoded true peak over ceiling by {over:.2f} dB; "
                                          f"trimmed master {-(over + 0.1):.2f} dB, re-encoded")
                extra_db -= over + 0.1
                cur = scaled(y, -(over + 0.1))
                continue
            if not t.loop or final:
                break
            score = 2 * int(files["ogg"]["loop_check"]["pass"] and item["loop_check_master"]["pass"]) \
                + int(files["m4a"]["loop_check"]["pass"])
            if best is None or score > best[0]:
                best = (score, vi, extra_db)
            if score == 3:
                break
            if vi + 1 < len(variants):
                item["processing"].append(f"loop check score {score}/3 for {label}; trying "
                                          f"{variants[vi + 1][0]}")
                vi, extra_db, tp_trims = vi + 1, 0.0, 0
                continue
            if best[1] != vi:
                # Out of candidates: re-run the best one (master + Opus first, AAC second).
                variants, vi, extra_db, tp_trims, final = [variants[best[1]]], 0, best[2], 3, True
                continue
            break
        total_gain += extra_db
        if t.loop and label != "as built":
            item["processing"].append(f"loop variant used: {label}")

        write_pcm(y, tmp, "pcm_f32le")
        lufs_m, tp_m = measure(tmp, tile)
        item["gain_db"] = round(total_gain, 2)
        item["processing"].append(f"linear gain {total_gain:+.2f} dB; export Opus 96k + AAC 96k"
                                  + (" (Opus loop-aware: context-padded encode, pre-skip and "
                                     "final granule patched)" if t.loop else ""))
        item["master"] = {"path": os.path.relpath(master, out_dir), "sha256": sha256(master),
                          "samples": len(y), "duration_s": round(len(y) / SR, 4),
                          "lufs": round(lufs_m, 1), "true_peak_db": round(tp_m, 1),
                          "leading_silence_ms": round(leading_silence_ms(y), 2)}
        item["samples"] = len(y)
        item["duration_s"] = round(len(y) / SR, 4)
        item["files"] = files
        item["lufs"] = files["ogg"]["lufs"]
        item["true_peak_db"] = max(files["ogg"]["true_peak_db"], files["m4a"]["true_peak_db"])
        if t.loop:
            item["loop_check"] = {"pass": files["ogg"]["loop_check"]["pass"],
                                  "master_pass": item["loop_check_master"]["pass"],
                                  "aac_pass": files["m4a"]["loop_check"]["pass"],
                                  "crossfade_s": item["loop_check_master"]["crossfade_s"],
                                  "variant": item["loop_check_master"].get("variant"),
                                  "extended": item["loop_check_master"].get("extended"),
                                  "leveler": item["loop_check_master"].get("leveler")}
        audit(item, t, tp0_ceiling_bound=(t.lufs - lufs0) > (t.tp - TP_MARGIN_DB - tp0))
    return item


def audit(item: dict[str, Any], t: Target, tp0_ceiling_bound: bool) -> None:
    issues = item["issues"]
    for ext in ("ogg", "m4a"):
        f = item["files"][ext]
        if abs(f["lufs"] - t.lufs) > t.tol:
            why = " (true-peak ceiling bound the gain: transient crest too high)" \
                if tp0_ceiling_bound else ""
            issues.append(f"{ext} loudness {f['lufs']} LUFS outside {t.lufs}±{t.tol}{why}")
        if f["true_peak_db"] > t.tp:
            issues.append(f"{ext} true peak {f['true_peak_db']} dBTP over ceiling {t.tp}")
    d = item["duration_s"]
    if not (t.min_s - 1e-3 <= d <= t.max_s + 1e-3):
        issues.append(f"duration {d}s outside {t.min_s}-{t.max_s}s")
    if not t.loop and item["master"]["leading_silence_ms"] > LEAD_MAX_MS:
        issues.append(f"leading silence {item['master']['leading_silence_ms']} ms > {LEAD_MAX_MS}")
    if item.get("second_onset_ms") is not None:
        item["warnings"].append(f"second onset (double hit or bounce) at {item['second_onset_ms']} ms; "
                      "audition before picking")
    if t.loop:
        lc = item["loop_check"]
        if not lc["master_pass"] or not lc["pass"]:
            issues.append("loop check failed on " + ("master" if not lc["master_pass"] else "ogg"))
        if not lc["aac_pass"]:
            issues.append("loop check failed on decoded m4a (AAC priming/padding; fallback only)")


# ---------------------------------------------------------------- music previews

MUSIC_LUFS, MUSIC_TP = -18.0, -1.0
MUSIC_OPUS = ["-c:a", "libopus", "-b:a", "160k", "-vbr", "on", "-application", "audio"]
MUSIC_AAC = ["-c:a", "aac", "-b:a", "160k"]


def process_music(row: dict[str, Any], args: argparse.Namespace) -> dict[str, Any]:
    """Stereo preview: linear gain to MUSIC_LUFS under MUSIC_TP. No trims, no loop edits."""
    src = resolve_source(row, Path(args.repo))
    cue = row["cue_id"]
    item: dict[str, Any] = {
        "cue_id": cue, "family": "music", "stem": cue, "loop": True,
        "source": {"file": row["file"], "provenance": os.path.relpath(row["_provenance"], args.repo),
                   "sha256_recorded": row.get("sha256"), **row["_provider"]},
        "prompt": row.get("prompt"), "issues": [], "warnings": [],
        "processing": ["preview only: whole file, stereo, no trim and no loop edit (P4-T5)"],
        "target": {"lufs": MUSIC_LUFS, "tolerance_lu": 1.0, "true_peak_ceiling_db": MUSIC_TP,
                   "duration_s": None, "lufs_method": "integrated over whole file"},
    }
    if src is None:
        item["issues"].append(f"source file not found: {row['file']}")
        return item
    digest = sha256(src)
    item["source"].update({"sha256": digest, "bytes": src.stat().st_size})
    if row.get("sha256") and row["sha256"] != digest:
        item["issues"].append("source sha256 does not match provenance record")
    lufs0, tp0 = measure(src)
    gain = min(MUSIC_LUFS - lufs0, MUSIC_TP - TP_MARGIN_DB - tp0)
    out_dir = Path(args.music_out)
    out_dir.mkdir(parents=True, exist_ok=True)
    files: dict[str, Any] = {}
    with tempfile.TemporaryDirectory() as wd:
        for attempt in range(2):
            over = 0.0
            for ext, cargs in (("ogg", MUSIC_OPUS), ("m4a", MUSIC_AAC)):
                dest = out_dir / f"{cue}.{ext}"
                tmp = dest.with_name(dest.stem + ".tmp" + dest.suffix)
                run(["ffmpeg", "-nostdin", "-v", "error", "-y", "-i", str(src), "-af",
                     f"volume={gain:.4f}dB", "-ar", str(SR), "-ac", "2", *cargs, *BITEXACT,
                     str(tmp)])
                os.replace(tmp, dest)
                dwav = Path(wd) / f"dec.{ext}.wav"
                run(["ffmpeg", "-nostdin", "-v", "error", "-y", "-i", str(dest), "-c:a",
                     "pcm_f32le", str(dwav)])
                lufs_e, tp_e = measure(dwav)
                frames = (dwav.stat().st_size - 44) // 8
                files[ext] = {"path": os.path.relpath(dest, args.out), "sha256": sha256(dest),
                              "bytes": dest.stat().st_size, "lufs": round(lufs_e, 1),
                              "true_peak_db": round(tp_e, 1), "duration_s": round(frames / SR, 3)}
                over = max(over, tp_e - MUSIC_TP)
            if over <= 0 or attempt == 1:
                break
            gain -= over + 0.1
    item["processing"].append(f"linear gain {gain:+.2f} dB; Opus 160k + AAC 160k stereo")
    item.update({"gain_db": round(gain, 2), "files": files, "lufs": files["ogg"]["lufs"],
                 "true_peak_db": max(f["true_peak_db"] for f in files.values()),
                 "duration_s": files["ogg"]["duration_s"],
                 "samples": round(files["ogg"]["duration_s"] * SR)})
    for ext, f in files.items():
        if abs(f["lufs"] - MUSIC_LUFS) > 1.0:
            why = " (true-peak ceiling bound the gain)" if gain < MUSIC_LUFS - lufs0 - 0.05 else ""
            item["issues"].append(f"{ext} loudness {f['lufs']} LUFS outside {MUSIC_LUFS}±1{why}")
        if f["true_peak_db"] > MUSIC_TP:
            item["issues"].append(f"{ext} true peak {f['true_peak_db']} dBTP over ceiling {MUSIC_TP}")
    return item


# ---------------------------------------------------------------- outputs


def write_report(manifest: dict[str, Any], path: Path) -> None:
    items = manifest["items"]
    bad = [i for i in items if i["issues"]]
    warn = [i for i in items if i.get("warnings")]
    lines = ["# SFX processing report", "",
             f"Processed {len(items)} variation(s): {len(bad)} out of target, "
             f"{len(warn)} with audition warnings.", "",
             "Generated by `scripts/audio/process.py`; targets come from `docs/audio/cue-sheet.md`. "
             "A loudness shortfall marked \"true-peak ceiling bound the gain\" is a deviation the "
             "pipeline cannot fix by linear gain: reaching the LUFS target would break the "
             "true-peak ceiling, and limiting would flatten the transient.", "",
             "## Out of target", ""]
    if bad:
        lines += ["| stem | LUFS (ogg) | TP | duration | issues |",
                  "| --- | ---: | ---: | ---: | --- |"]
        for i in bad:
            lines.append(f"| `{i['stem']}` | {i.get('lufs', '')} | {i.get('true_peak_db', '')} | "
                         f"{i.get('duration_s', '')} | {'; '.join(i['issues'])} |")
    else:
        lines.append("None.")
    loops = [i for i in items if i.get("loop") and i.get("loop_check") and i["family"] != "music"]
    if loops:
        p = manifest["parameters"]
        lines += ["", "## Loop checks", "",
                  f"{sum(1 for i in loops if i['loop_check']['master_pass'] and i['loop_check']['pass'])}"
                  f" of {len(loops)} loops pass the seamless-loop check on the master and the decoded "
                  f"Opus file (`check-audio-loops.py`). The AAC column is the fallback encode, where "
                  f"AAC priming can fail a loop the Opus file passes.", "",
                  f"Parameters: plain crossfades {p['loop_crossfades_s']} s; extended search "
                  f"{json.dumps(p['loop_extended'])}; leveler {json.dumps(p['loop_leveler'])}; Opus "
                  f"encoded with {p['opus_loop_context_s']} s of the loop's own tail and head as "
                  f"context and the Ogg pre-skip and final granule position patched so the decoder "
                  f"returns exactly the loop.", "",
                  "| stem | duration | master | opus | aac | route |", "| --- | ---: | --- | --- | --- | --- |"]
        for i in loops:
            lc = i["loop_check"]
            route = [lc.get("variant") or "as built"]
            if lc.get("extended"):
                route.append(f"extended: {lc['extended']['passes']} passes of "
                             f"{lc['extended']['region_s'][0]}-{lc['extended']['region_s'][1]} s")
            if lc.get("leveler"):
                route.append("leveled")
            ok = lambda v: "pass" if v else "FAIL"  # noqa: E731
            lines.append(f"| `{i['stem']}` | {i['duration_s']} | {ok(lc['master_pass'])} | "
                         f"{ok(lc['pass'])} | {ok(lc['aac_pass'])} | {'; '.join(route)} |")
    lines += ["", "## Audition warnings", ""]
    if warn:
        lines += ["| stem | warning |", "| --- | --- |"]
        lines += [f"| `{i['stem']}` | {'; '.join(i['warnings'])} |" for i in warn]
    else:
        lines.append("None.")
    rejected = manifest.get("rejected", [])
    lines += ["", "## Rejected by owner (not processed)", ""]
    if rejected:
        lines += ["| stem | rejected | reason |", "| --- | --- | --- |"]
        lines += [f"| `{r['stem']}` | {r.get('rejected_by') or ''} {r.get('rejected_on') or ''} | "
                  f"{r.get('reason') or ''} |" for r in rejected]
    else:
        lines.append("None.")
    path.write_text("\n".join(lines) + "\n")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--repo", default=str(REPO))
    ap.add_argument("--provenance-glob",
                    default=str(REPO / "assets-source/audio/sfx/provenance-group-*.json"))
    ap.add_argument("--cue-sheet", default=str(REPO / "docs/audio/cue-sheet.md"))
    ap.add_argument("--out", default=str(REPO / "audition/sfx"))
    ap.add_argument("--music-provenance-glob",
                    default=str(REPO / "assets-source/audio/music/provenance.json"),
                    help="music provenance; no match means no music group")
    ap.add_argument("--music-out", default=None, help="music previews (default <out>/../music)")
    ap.add_argument("--masters", default=None, help="24-bit WAV masters (default <out>/_masters)")
    ap.add_argument("--loop-checker", default=os.environ.get(
        "PARBLE_LOOP_CHECKER", str(SKILL / "check-audio-loops.py")))
    ap.add_argument("--repair-loop", default=os.environ.get(
        "PARBLE_REPAIR_LOOP", str(SKILL / "repair-loop.py")))
    ap.add_argument("--jobs", type=int, default=max(1, (os.cpu_count() or 2) // 2))
    ap.add_argument("--strict", action="store_true", help="exit 1 when any item is out of target")
    ap.add_argument("--only", default=None,
                    help="regex on the source stem; process just those rows (use with --out to a "
                         "scratch directory, the manifest then covers only them)")
    args = ap.parse_args()
    args.masters = args.masters or str(Path(args.out) / "_masters")
    args.music_out = args.music_out or str(Path(args.out).parent / "music")
    out = Path(args.out).resolve()
    if str(out).startswith(str(Path(args.repo).resolve() / "assets-source")):
        sys.exit("ERROR: --out must not be inside assets-source/")

    tools = preflight(args)
    targets = parse_cue_sheet(Path(args.cue_sheet))
    all_rows = load_provenance(args.provenance_glob)
    rows = [r for r in all_rows if r.get("status") != "rejected"
            and (not args.only or re.search(args.only, Path(r["file"]).stem))]
    rejected = sorted((rejected_entry(r, args.repo) for r in all_rows if r.get("status") == "rejected"),
                      key=lambda r: (r.get("family") or "", r["cue_id"], r["stem"]))
    out.mkdir(parents=True, exist_ok=True)
    with ProcessPoolExecutor(max_workers=args.jobs) as pool:
        futures = [pool.submit(process_item, r, targets.get(r["cue_id"]), args) for r in rows]
        items = []
        for r, f in zip(rows, futures):
            try:
                items.append(f.result())
            except Exception as exc:  # noqa: BLE001 - one bad file must not sink the batch
                items.append({"cue_id": r["cue_id"], "family": r.get("family"),
                              "stem": Path(r["file"]).stem, "warnings": [],
                              "issues": [f"processing error: {exc}"]})
    music_rows = load_provenance(args.music_provenance_glob, required=False)
    with ProcessPoolExecutor(max_workers=args.jobs) as pool:
        items += list(pool.map(process_music, music_rows, [args] * len(music_rows)))
    items.sort(key=lambda i: (i.get("family") or "", i["cue_id"], i["stem"]))

    produced = {Path(f["path"]).name for i in items for f in i.get("files", {}).values()}
    stale_dirs = [*out.glob("*/*"), *Path(args.music_out).glob("*")]
    for stale in sorted(stale_dirs):
        if stale.suffix in (".ogg", ".m4a") and stale.name not in produced:
            stale.unlink()

    manifest = {
        "schema": "parble.sfx-manifest/1", "pipeline_version": PIPELINE_VERSION,
        "tools": tools, "cue_sheet_sha256": sha256(Path(args.cue_sheet)),
        "parameters": {"sample_rate": SR, "channels": 1, "opus": " ".join(OPUS_ARGS),
                       "aac": " ".join(AAC_ARGS), "onset_db": ONSET_DB, "preroll_s": PREROLL_S,
                       "tail_db": TAIL_DB, "tp_margin_db": TP_MARGIN_DB,
                       "loop_crossfades_s": list(LOOP_FADES_S),
                       "loop_extended": {"grid_s": EXT_GRID_S, "min_region_s": EXT_MIN_REGION_S,
                                         "edge_s": EXT_EDGE_S, "edge_db": EXT_EDGE_DB,
                                         "range_db": EXT_RANGE_DB,
                                         "max_candidates": EXT_MAX_CANDIDATES},
                       "loop_leveler": {"floor_db": LEVEL_FLOOR_DB, "ceil_db": LEVEL_CEIL_DB,
                                        "boost_db": LEVEL_BOOST_DB, "cut_db": LEVEL_CUT_DB},
                       "opus_loop_context_s": OPUS_CONTEXT_S},
        "items": items,
        "rejected": rejected,
    }
    (out / "manifest.json").write_text(json.dumps(manifest, indent=1, sort_keys=False) + "\n")
    write_report(manifest, out / "processing-report.md")
    bad = sum(1 for i in items if i["issues"])
    errors = sum(1 for i in items if any("error" in s or "not found" in s for s in i["issues"]))
    print(f"processed {len(items)} variation(s): {len(items) - bad} in target, {bad} flagged, "
          f"{errors} error(s), {len(rejected)} rejected skipped -> {out}/manifest.json")
    if errors:
        return 2
    return 1 if (args.strict and bad) else 0


if __name__ == "__main__":
    sys.exit(main())
