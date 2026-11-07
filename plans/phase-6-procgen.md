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

### P6-T2 — Headless validation worker
- `parble-sim-wasm` exposes `generate_and_validate(seed, params, budget_ms)` with time slicing and resumable state.
- A Web Worker wrapper in TS.
- A failing candidate retries with the next sub-seed. A track is accepted only when every hopper marble reaches the finish inside a time limit.
- The final accepted seed is stored in the track metadata.
- Verify: Vitest worker tests. A Playwright run shows no main-thread frame over 25 ms during generation.
- Depends on: P6-T1, P0-T4.

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
