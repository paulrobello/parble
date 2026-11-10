# Audio acceptance (P4-T6)

Evidence for the PRD §6 P4 acceptance boxes. Measured 2026-11-09 on gpu-host-a (Apple M4 Max) in Playwright's GPU Chromium (`channel: 'chromium'`, headless, ANGLE Metal, `--mute-audio`), against a release `make build-web` of the P4-T6 branch.

| PRD P4 box | Status | Evidence |
|---|---|---|
| Each marble type sounds distinct on each surface (owner listening approval) | Approved by Paul, 2026-11-10 | [Listening review](#listening-review) below |
| No clipping or voice starvation with 10 marbles on the stress track | Measured, passes | [C1](#c1-no-clipping-and-no-voice-starvation) |
| Audio CPU stays within the frame budget | Measured, passes (p95 0.2 ms against 2 ms, load-guarded on gpu-host-b) | [C2](#c2-audio-main-thread-time) |
| Music loops seamlessly | Automated checks pass (P4-T5), approved by Paul, 2026-11-10 | `e2e/tests/music.spec.mjs`, `make music-soak`, listening review item L3 |

## C1: no clipping and no voice starvation

Command: `cd e2e && ./node_modules/.bin/playwright test tests/audio.spec.mjs -g "stress track"` (it is part of `make web-e2e`).

Scene: `/?mute=1&scenario=stress2000`, the stress track. That is the `allpieces` run, whose hopper releases 10 glass marbles (swirl, opal, onionskin, cat's eye, glow, repeated) every 2 s, next to the 2,000-piece render-only tower. The test unlocks audio with a click, waits for all 204 SFX files to decode, starts the meter and records for 32 s of real time. That covers one whole 30 s run cycle and its restart (`resets` ≥ 1). The music director plays `music_run_flow` on the music bus for the whole window, so the peak includes music and SFX together at the default mix (mixer sliders at 100).

How the peak is measured: `window.__parbleAudio.meter()` connects an `AnalyserNode` (`fftSize` 32,768, 683 ms at 48 kHz) to the engine's master output after its limiter (the `DynamicsCompressorNode` behind the master gain) and before the `mute=1` zero gain. So it reads the samples the speakers would get while the page stays silent. On every fed audio frame it reads the whole window with `getFloatTimeDomainData`, which returns unclamped floats, and keeps the largest absolute sample. The longest gap between two reads was 33 ms against the 683 ms window, so every output sample was read at least once. This is a sample peak, not an intersample true peak. The criterion is that no sample reaches 1.0 (0 dBFS).

How starvation is measured: on the same frames the hook reads the followed marble (`window.__parble.camera.director.followed`, the same sim marble index the audio stream's `marble` carries) and the engine's `rollFrame()`. A frame counts when the followed marble is rolling by the engine's own test (in contact, fast enough that its roll gain is above zero, with a roll cue). On each such frame the marble must hold a roll voice with a sounding loop.

Result, 2026-11-09 (load not guarded, since this is not a timing measurement; the 1-minute load was about 55. The `make web-e2e` run at 21:40 read the same peak, 0.322, and the same 0 clipped reads and 0 silent focus frames):

| Measure | Value | Criterion |
|---|---|---|
| Output sample peak | 0.322 (−9.8 dBFS) | < 1.0 |
| Reads holding a sample ≥ 1.0 | 0 of 1,934 | 0 |
| Peak before the limiter (master bus) | 0.233 (−12.7 dBFS) | for information |
| Longest gap between reads | 33.1 ms | < 683 ms window |
| Marbles in the run / most rolling at once | 10 / 8 | 10 marbles |
| Most roll voices / most voices at once | 7 / 24 (`peakVoices`) | cap 10 rolls / 24 total |
| One-shots started, stolen, dropped | 372, 6, 0 | |
| Frames the followed marble rolled | 1,419 (marbles 0–3, 5–7, 9 were followed) | > 0 |
| Of those, frames with no roll voice for it | 0 | 0 |
| Engine, load and page errors | 0 | 0 |

No mix change was needed. The limiter barely acts at this level: the output peak sits about 10 dB under full scale.

The starvation result also holds by construction, and the test guards it against a later change. With 10 marbles and a roll budget of 10, the roll selection (`selectTop` in `voices.ts`) never culls a roll, and one-shots never take roll slots: a one-shot may only use `total − rolls` slots, and returning rolls stop the quietest one-shots (`#fitOneShots`). The full voice cap was hit (24) and 6 one-shots were stolen, all from other one-shots. If the roll cap or the marble count changes so rolls compete, the nearest and loudest win, and the followed marble is normally the nearest; this test is the check.

## C2: audio main-thread time

Command: `make build-web && make web-audio-perf` (`e2e/tests/audio-perf.spec.mjs`, opt-in, `PARBLE_PERF=1`). Output: `target/perf/p4t6-audio.json`.

Scene: `/?mute=1&quality=high&scenario=stress2000` at 2560 × 1440, device scale 1, the P1-T8 perf gate's setup. After the audio unlock and decode, `window.__parbleAudio.timing(true)` records `performance.now()` around every fed audio frame for 60 s (two full run cycles): the engine's `consume` (roll voices, impacts, mechanism cues, panners, voice budget) plus the music director and player update. The peak meter is not started in this run, so its reads are not counted. rAF frame times are recorded beside it.

Budget: p95 ≤ 2 ms per frame, 12 % of the 16.7 ms frame at 60 Hz. The rest of the frame belongs to the Bevy frame in the wasm (sim step, render encode) and the HUD. The P1-T8 gate (`docs/perf/p1-gate.json`) shows `stress2000` at 1440p High holding vsync (p95 16.8 ms, 1 dropped frame in 600), but it does not measure how much main-thread time is spare, so the audio share is set as a small fixed slice, not from measured headroom. Web Audio's rendering (mixing, filters, panning, the compressor) runs on the browser's audio thread, so this number is only the main-thread work.

Timer resolution: the page is not cross-origin isolated, so Chromium coarsens `performance.now()` to 100 µs (with jitter). Per-frame values are quantized to about 0.1 ms. That is fine against a 2 ms budget, and the mean over thousands of frames is still meaningful.

Load guard: the 1-minute load average must be ≤ 15 before and after the trace. The spec waits up to 20 minutes for it before starting and reports the sample as not taken if the load is above 15 at either end.

Result, 2026-11-09 (Pacific time; the JSON stamps are UTC). No sample passed the load guard. gpu-host-a was shared with other agents the whole evening and its 1-minute load never stayed at or below 15 for a full run. Every run that started recorded the same audio numbers:

| Run (PST) | Load before / after | Taken | Audio frames | Mean | p95 | p99 | Max | Frames > 1 ms | rAF p95 |
|---|---|---|---|---|---|---|---|---|---|
| 21:14, `PARBLE_PERF_DIAG=1` | 238.5 / 101.3 | no (diagnostic) | 3,610 | 0.053 ms | 0.2 ms | 0.2 ms | 0.5 ms | 0 | 16.8 ms |
| 21:44, guarded | 14.88 / 16.46 | no (after > 15) | 3,610 | 0.052 ms | 0.2 ms | 0.2 ms | 0.3 ms | 0 | 16.8 ms |
| 22:03, guarded | 14.34 / 15.55 | no (after > 15) | 3,609 | 0.056 ms | 0.2 ms | 0.2 ms | 0.5 ms | 0 | 16.7 ms |
| 22:24, guarded | 26.5 at the end of the 20 min wait | no (never started) | | | | | | | |
| 2026-11-10 02:31, guarded, gpu-host-b (Apple M4) | 4.76 / 4.51 | **yes** | 3,613 | 0.059 ms | 0.2 ms | 0.2 ms | 0.3 ms | 0 | 16.7 ms |

In every run the stress track had 10 marbles, up to 7 roll voices and 23 voices at once, and about 724 one-shots and 810 contacts in 60 s. The page and engine recorded no errors. The p95 of 0.2 ms is 10 % of the 2 ms budget and about 1.2 % of the 16.7 ms frame. The worst frame (0.5 ms) is a quarter of the budget, measured even at load 238. The rAF frame times held vsync, with 1 to 4 frames over 25 ms in 3,600.

Status: passes. The 2026-11-10 02:31 PST run on gpu-host-b (Apple M4, booked window, release `make build-web` of main) passed the load guard at both ends and recorded `taken: true`: p95 0.2 ms against the 2 ms budget, max 0.3 ms, no frame over 1 ms, rAF p95 16.7 ms with 2 dropped frames in 3,612. The record is [`p4t6-audio-gpu-host-b.json`](p4t6-audio-gpu-host-b.json).

## Listening review

Approved by Paul on 2026-11-10 (check-in answer to Q18, "Q18 approved"), covering L1 to L4 and the music loops.

How to run it:

- Audition board: `make audition` serves `audition/` on http://127.0.0.1:5312/ (it may already be running). It plays the processed SFX takes (Paul's approved picks are listed in `docs/audio/audition-picks.json`), so the roll loops and impacts of each material (glass, solid, steel) on each surface (block channel, rail) can be compared side by side.
- In the app: `make dev` serves http://127.0.0.1:5310/ (or `make build-web` and any static server on `target/web-dist`). Leave out `mute=1`, and click the canvas once to start audio. The Sound button (bottom right) opens the mixer.
  - Marble type on the block channel: `/?scenario=ramp&marble=cats-eye`, then `marble=glazed` (solid) and `marble=steel`. The ramp is the wooden tray, which reports the block surface.
  - The stress mix: `/?scenario=stress2000` (10 glass marbles; all shipped sandbox types except glazed and steel are glass and share the glass cues).
  - Rails and mechanisms: `/?scenario=showcase` (the all-pieces run on the playroom rug under the follow camera: rails, drops, the tower, toggles, the one-way gate, the trapdoor and the finish cups).
  - Music: `/?music=run` (or `build`, `night`, `title`) pins a state. To hear the loop seam without waiting, run `__parbleAudio.musicSeek(__parbleAudio.stats().music.loopEnd - 5)` in the console after the unlock.

Checklist:

- [x] L1. Each marble type sounds distinct on each surface: glass, solid (glazed) and steel, each rolling and hitting on the block channel and on a rail. Minis and shooters keep their material's sound at a higher or lower pitch.
- [x] L2. The 10-marble stress mix (`stress2000`): no harshness, pumping or distortion when many marbles clack together, rolls stay audible under the impacts, and the music sits under the SFX.
- [x] L3. Each music loop's seam (run, build calm, build playful, night, title) is inaudible, and state changes crossfade smoothly.
- [x] L4. Mechanism cues (hopper release, toggle and splitter ticks, gate swing, trapdoor, drop holes, finish chime) match what is seen.

Known limit: the rug cues are not staged (Paul rejected them on 2026-11-07, `docs/audio/cue-sheet.md`), and the sim reports the rug floor as wood, so there is no rug surface to judge yet.

Owner approval (Paul): approved ("Q18 approved", relayed at the 2026-11-10 check-in) Date: 2026-11-10
