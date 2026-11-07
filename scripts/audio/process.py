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


def build_loop(x: array.array, t: Target, gain_db: float, work: Path,
               args: argparse.Namespace) -> tuple[array.array, dict[str, Any], list[str]]:
    notes: list[str] = []
    pre = scaled(x, gain_db)
    raw = work / "loop_raw.wav"
    max_n, min_n = int(t.max_s * SR), int(t.min_s * SR)
    attempts: list[dict[str, Any]] = []
    if min_n <= len(pre) <= max_n:
        write_pcm(pre, raw, "pcm_s16le")
        ok, out = loop_check(args.loop_checker, raw)
        attempts.append({"crossfade_s": 0.0, "pass": ok})
        if ok:
            notes.append("raw take already loops cleanly; no crossfade")
            return pre, {"pass": True, "crossfade_s": 0.0, "attempts": attempts}, notes
    region_start, region_len = longest_clean_run(pre)
    if region_len < len(pre):
        notes.append(f"interior dropouts in take; loop region limited to longest clean run "
                     f"{region_start / SR:.2f}s + {region_len / SR:.2f}s")
        pre = array.array("f", pre[region_start:region_start + region_len])
    result = None
    for fade in LOOP_FADES_S:
        fade_n = int(fade * SR)
        region = pre[: min(len(pre), max_n + fade_n)]
        if len(region) - fade_n < min_n:
            continue
        write_pcm(region, raw, "pcm_s16le")
        rep = work / f"loop_xf_{fade}.wav"
        run([sys.executable, args.repair_loop, str(raw), str(rep), "--seconds", str(fade)])
        ok, out = loop_check(args.loop_checker, rep)
        attempts.append({"crossfade_s": fade, "pass": ok})
        result = (rep, fade, ok)
        if ok:
            break
    if result is None and len(pre) > 4 * int(LOOP_FADES_S[0] * SR):
        # Below the sheet's range either way; a seamless short loop auditions better than a
        # clicking one, and the duration shortfall is still reported.
        fade = LOOP_FADES_S[0]
        notes.append(f"clean region {len(pre) / SR:.2f}s too short for a {t.min_s}s loop; "
                     f"crossfading anyway, duration falls short")
        write_pcm(pre, raw, "pcm_s16le")
        rep = work / f"loop_xf_{fade}.wav"
        run([sys.executable, args.repair_loop, str(raw), str(rep), "--seconds", str(fade)])
        ok, out = loop_check(args.loop_checker, rep)
        attempts.append({"crossfade_s": fade, "pass": ok})
        result = (rep, fade, ok)
    if result is None:
        notes.append(f"clean region {len(pre) / SR:.2f}s too short to loop")
        return pre, {"pass": False, "crossfade_s": None, "attempts": attempts}, notes
    rep, fade, ok = result
    notes.append(f"repair-loop.py equal-power tail-over-head crossfade {fade}s")
    return decode(rep), {"pass": ok, "crossfade_s": fade, "attempts": attempts}, notes


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
        rotations = loop_rotations(y) if t.loop else []
        base, rot, tp_trims = y, 0, 0
        while True:
            y = array.array("f", base[rot:] + base[:rot]) if rot else base
            write_pcm(y, master, "pcm_s24le")
            over = 0.0
            for ext, cargs in (("ogg", OPUS_ARGS), ("m4a", AAC_ARGS)):
                dest = fam_out / f"{stem}.{ext}"
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
            if over > 0 and tp_trims < 3:
                tp_trims += 1
                item["processing"].append(f"encoded true peak over ceiling by {over:.2f} dB; "
                                          f"trimmed master {-(over + 0.1):.2f} dB, re-encoded")
                base = scaled(base, -(over + 0.1))
                total_gain -= over + 0.1
                continue
            if t.loop and not files["ogg"]["loop_check"]["pass"] and rotations:
                # The master wraps cleanly; codec edge error at the seam does not. Rotating
                # the (circular) loop to a low-slope zero crossing moves the seam.
                rot = rotations.pop(0)
                item["processing"].append(f"Opus seam failed loop check; rotated loop start "
                                          f"to sample {rot}")
                continue
            break

        write_pcm(y, tmp, "pcm_f32le")
        lufs_m, tp_m = measure(tmp, tile)
        item["gain_db"] = round(total_gain, 2)
        item["processing"].append(f"linear gain {total_gain:+.2f} dB; export Opus 96k + AAC 96k")
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
                                  "crossfade_s": item["loop_check_master"]["crossfade_s"]}
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
    lines += ["", "## Audition warnings", ""]
    if warn:
        lines += ["| stem | warning |", "| --- | --- |"]
        lines += [f"| `{i['stem']}` | {'; '.join(i['warnings'])} |" for i in warn]
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
    args = ap.parse_args()
    args.masters = args.masters or str(Path(args.out) / "_masters")
    args.music_out = args.music_out or str(Path(args.out).parent / "music")
    out = Path(args.out).resolve()
    if str(out).startswith(str(Path(args.repo).resolve() / "assets-source")):
        sys.exit("ERROR: --out must not be inside assets-source/")

    tools = preflight(args)
    targets = parse_cue_sheet(Path(args.cue_sheet))
    rows = load_provenance(args.provenance_glob)
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
                       "loop_crossfades_s": list(LOOP_FADES_S)},
        "items": items,
    }
    (out / "manifest.json").write_text(json.dumps(manifest, indent=1, sort_keys=False) + "\n")
    write_report(manifest, out / "processing-report.md")
    bad = sum(1 for i in items if i["issues"])
    errors = sum(1 for i in items if any("error" in s or "not found" in s for s in i["issues"]))
    print(f"processed {len(items)} variation(s): {len(items) - bad} in target, {bad} flagged, "
          f"{errors} error(s) -> {out}/manifest.json")
    if errors:
        return 2
    return 1 if (args.strict and bad) else 0


if __name__ == "__main__":
    sys.exit(main())
