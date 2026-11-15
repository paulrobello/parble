# Phase 6: Procedural generation

PRD: `PRD.md` §6 P6. Phase goal: generated tracks that are always runnable, editable after generation, plus fill-between and infinite descent.

Design constraint: validation is a headless sim run at 240 Hz, tens of thousands of steps per track, and wasm is single-threaded. All generation and validation runs in a Web Worker built from `parble-sim-wasm` (no Bevy) in time-sliced batches, so the render thread never stalls.

## Tasks

### P6-T1 — Generator core
- `parble-procgen` grows a port graph on the grid from a hopper.
- Weighted piece choice from sliders: footprint, height, special-piece density, difficulty, allowed families.
- Collision-free footprint placement with backtracking. Supports and pillars placed under elevated pieces. Ends at a finish.
- Fully determined by (seed, params, catalog version).
- Verify: unit tests show the same seed + params give a byte-identical track, and generated tracks pass the P3-T2 validation engine with zero issues.
- Depends on: P2-T7, P3-T2.
- Status (2026-11-13): chain generator landed in `crates/parble-procgen` (`generate(defs, seed, params)`, `GENERATOR_VERSION` 1). The search places single-exit pieces and uses the validator's controlled-drop transfer (`L1` exit onto an `L3` inlet one layer below), so the `L1` hopper needs no lift. Pillar columns go under every elevated cell after the walk, which needed the validator to treat a support fed only by idle supports as idle. Specials generated: gate, checkpoint, rope arch. Not generated yet: splitter, toggle and join (branches need a second feed or a per-end finish), lifts, funnels, one-way holes, socket/spigot pieces.
- Measured physical pass rate (`cargo test -p parble-sim --release --test procgen_pass_rate -- --ignored --nocapture`, default params, every marble finishing within 60 s): 11 of 100 seeds (0 to 99) at the T1 commit. Marbles stall on rail curves and slopes, overshoot transfers entered above about 0.5 m/s and leave the track on high-speed rails. Static validity is the T1 bar; the pass rate is P6-T1b.
- P6-T1b and its follow-up (generator version 4): each open end carries an integer squared-speed estimate (`speed.rs`: gain per level, per-shape retention, catch and redirect reset values) and the search refuses steps the sim measured to fail (a transfer above 0.33 m/s, a turn above 0.5 m/s, a drop below 0.45 m/s, a creep below 0.17 m/s). `reach.rs` prunes any exit whose (level, speed, flat run) state has no way to a finish. Attempts are 2,000 nodes, so a bad early choice restarts sooner. `blk_drop_l3`, `rail_s_1x3_l1` and `rail_curve90_r90_l1` are left out. That reached 67 of 100 seeds (0 to 99), 72% over 300. The stall that capped it was a sim bug, not the kit: `ChannelSlope::holds` extends a slope's channel flat past its ports, so a gentle slope on a drop's row tagged the drop cell's floor triangles, and `covers` checks the plan only, so the catch's landing floor under the drop "covered" the marble above it and dropped those contacts (a marble at 0.5 m/s sank 6 mm and stopped 20 mm short of the bore). `ChannelSlope::reaches_height` now also requires the ball's height to be within the channel's range; regression test `a_slope_on_the_drops_row_does_not_stall_the_marble`. Final: 95 of 100 seeds (0 to 99), 281 of 300 (93.7%), no generation failures, every marble finishing within 60 s. The follow-up (marbles dropping through `L3` straights at 0.15 to 0.4 m/s, about 6% of tracks) was the same plan-only coverage in another branch: when a ball is over a slope's cell the bend branch clears the contacts of bend-tagged triangles, and a gentle slope four layers up in the same plan cell did that to the straights after a curve. That branch now uses `covers_from_above` (the ball is not below the slope's channel), the contact-clearing branch uses `holds_ball` (the ball is within its height range); the contact branches keep plan-only `covers` because a ball falls onto a landing floor from above. Regression test `a_slope_far_above_does_not_open_the_floor_of_straights_after_a_curve`. The generator also no longer starts a transfer straight from a catch's exit (4 of 18 failures). Now 294 of 300 (98%), 99 of 100 for seeds 0 to 99, no generation failures. Pass rate barely moves with length and the marble count does not matter.

### P6-T2 — Headless validation worker
- `parble-sim-wasm` exposes `generate_and_validate(seed, params, budget_ms)` with time slicing and resumable state.
- A Web Worker wrapper in TS.
- A failing candidate retries with the next sub-seed. A track is accepted only when every hopper marble reaches the finish inside a time limit.
- The final accepted seed is stored in the track metadata.
- Verify: Vitest worker tests. A Playwright run shows no main-thread frame over 25 ms during generation.
- Depends on: P6-T1, P0-T4.
- Status (2026-11-15): the job is `parble_sim::procgen_job` (validation needs the sim, so it lives in parble-sim, approved by Paul), sliced by `parble-sim-wasm`'s `generate_and_validate` in the `web/ui/src/gen` worker. The accepted seed is the track's own `seed`. The generator wasm has its own 2 MB gzip budget (1.33 MB), outside the 15 MB startup budget (approved by Paul).
- Long tracks (generator version 7): a walk descends about one port level per one and a half pieces, so at a flat-run limit of 3 a length above about seven pieces a layer of height ran out of height (90 pieces from layer 10 failed every seed). Above seven a layer the limit is now 8, and a length above 12 a layer (`GenParams::max_length`) is refused at once as `TooLong`, so the job does not retry it. Measured 2026-11-15: 100 from layer 10 and 160 from layer 16 generate on 5 of 5 seeds, 400 from layer 36 on 4 of 5, 500 from layer 48 on 5 of 5. In the sim 15 of 20 (length 100) and 18 of 20 (length 160) finish all three marbles; the job retries the rest. Tracks up to seven a layer are unchanged.
- Verified 2026-11-15: GPU Chromium on the 5311 preview (`e2e/tests/gen-worker.spec.mjs`, `target/perf/p6t2-gen.json`), at a 1-minute load of 13.6 before and 12.6 after: frames peaked at 16.8 ms idle (299 frames) and while generating (269), none dropped, against the 25 ms bar. Seeds 7, 72 and a length-80 track accepted in the worker in 795, 711 and 740 ms; seed 7 rejected once (a marble off the track at tick 2,221), its file's seed the accepted sub-seed. `make checkall-ci` passed on `ca6721f`.
- Supports dominate long tracks: one `blk_pillar` per layer under every elevated cell, so 100 pieces from layer 10 make 260 to 450 placements and 500 from layer 48 make 5,800 to 9,100. P6-T6's 500-piece track counts track pieces only, not supports (PRD P6).
- Validation job limits (2026-11-15 review, `docs/procgen-review-2026-11-15.md` phase 1): the tick limit counts track pieces only (2,400 ticks a marble plus 120 a track piece; supports had put a 500-piece track's limit at 1.6M ticks). A candidate is rejected the moment a marble leaves the track onto the open floor, and as stalled after two seconds with every marble released and none moving, instead of at the limit. Four exhausted searches in a row fail the job. Length 100 from layer 10, base 8: its first candidate is now rejected at tick 10,837 (limit 34,560) instead of running to 94,320. 500 from layer 48, base 1: accepted on its third candidate after an off-track and a stalled rejection, 47 s natively at a 1-minute load near 60.

### P6-T3 — Generator UI and generate-then-edit
- Generator panel with seed field, sliders, generate and regenerate.
- The result opens in the editor as a normal track.
- Verify: Playwright generates, edits one piece and runs with no errors.
- Depends on: P6-T2, P3-T5.

### P6-T4 — Fill-between
- In the editor the user picks two open ports. The generator routes a valid path between them (A* over port states with a height budget) and validates it with a headless run of that segment.
- Verify: unit tests on routing feasibility cases, including an unreachable pair (clear error). The Playwright flow passes.
- Depends on: P6-T1, P3-T4.

### P6-T5 — Infinite descent
- The worker generates and validates segments ahead of the followed marble. The main thread streams segment pieces and chunk colliders in.
- Segments far above the marble are recycled. The world is re-origined to keep float precision.
- Verify: a 30-minute soak in Playwright with bounded wasm memory (sample memory every minute; growth under 5 % after the first 5 minutes), no frame over budget from streaming, and determinism for a given seed.
- Depends on: P6-T2.

### P6-T6 — Procgen acceptance
- `make procgen-sweep` (native, release build) generates and validates 1,000 seeds across slider presets, all passing.
- Measure generation time for a 500-piece track in the worker and fix the budget number in PRD P6 (proposed ≤ 5 s).
- Verify: all PRD P6 acceptance boxes, evidence in `docs/perf/p6-procgen.json`.
- Depends on: P6-T2, P6-T3, P6-T4, P6-T5.

## Exit criteria
Every acceptance box under PRD §6 P6 checked with evidence.
