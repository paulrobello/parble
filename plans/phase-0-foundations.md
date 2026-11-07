# Phase 0: Foundations

PRD: `PRD.md` §6 P0. Phase goal: the workspace, both gates, a kit contract every later phase builds against, and a deterministic marble rolling down a primitive ramp natively and in Chrome/Safari WebGPU.

## Pins (verify still latest stable when the task starts; record the final pins in the commit)

| Package | Version | Source of pin |
|---|---|---|
| Rust toolchain | stable channel, targets `wasm32-unknown-unknown`, components `rustfmt`, `clippy` | hex-world `rust-toolchain.toml` |
| `bevy` | `=0.20.1`, `default-features = false` | hex-world; crates.io latest 2026-10-28 |
| `rapier3d` | `0.36.1`, features `dim3`, `f32`, `enhanced-determinism`. No `parallel`, no `simd-*`. | crates.io 2026-10-10 |
| `libm`, `rand_chacha`, `serde`, `serde_json`, `wasm-bindgen` | latest stable at task start | n/a |
| Trunk | 0.21.14 | hex-world CI |
| Node | ≥ 22.18 (CI: 24) | hex-world |
| `@biomejs/biome` 2.5.15, `typescript` 7.0.2, `vite` 8.3.5, `vitest` 5.1.0, `@playwright/test` 1.64.1 | as listed | the-cube `package.json` |
| gitleaks / pre-commit-hooks | exact `rev` pins | `~/.claude/guides/git-ci.md` |

Ports: 5310 Trunk dev server, 5311 static preview for Playwright (reserved in `~/.claude/used_ports.md`).

## Tasks

### P0-T1 — Workspace scaffold and gates
- Cargo workspace with crates `parble-core`, `parble-kit`, `parble-sim`, `parble-procgen`, `parble-sim-wasm`, `parble-render`, `parble-app`, each empty but compiling.
- `rust-toolchain.toml`, `.gitignore` (house template from git-ci.md), MIT `LICENSE`, `README.md` linking PRD, `CLAUDE.md` with gate and conventions.
- `wasm-release` profile modelled on hex-world (`opt-level = "z"`, `lto = "fat"`, `opt-level = 3` per hot crate).
- Makefile targets: `build`, `test`, `lint`, `fmt`, `fmt-check`, `typecheck`, `checkall`, `checkall-ci`, `pre-commit`, `dev` / `dev-start` / `dev-restart` / `dev-stop` (stop polls until 5310 is free, ≤ 2 s, SIGKILL escalation), `build-web`, `web-size`.
- `checkall` legs in fail-fast order: fmt-check → clippy (native + `--target wasm32-unknown-unknown`) → Biome/tsc → cargo test → Vitest.
- `.pre-commit-config.yaml` with gitleaks, detect-private-key, and local cargo fmt/clippy/test hooks per the Rust guide.
- Verify: `make checkall` and `make checkall-ci` exit 0 (captured exit codes, no pipes). `pre-commit run --all-files` passes.
- Depends on: none.

### P0-T2a — Kit contract document
- Write `docs/kit-contract.md`. It fixes:
  - cell 60 mm
  - horizontal port floor levels (15 mm steps; list them) and lateral tolerance
  - vertical drop port diameter
  - channel cross-section: 28 mm inner width, round bottom, wall thickness and lip radius for the block family and the rail family
  - marble radii (mini 10, standard 16, shooter 25 mm)
  - size-gate holes (12 mm, 19 mm)
  - gentle and steep slope gradients
  - yaw-only orientation with a mirror flag
  - footprint rules for multi-cell rails
  - piece naming
  - collision sidecar format (mesh + port list + sensor volumes + animated part names)
  - LOD and texel density targets
- Include a dimensioned diagram per family and the rail-to-block joint (see `docs/reference/ref-06-rail-into-block-tower.png`).
- Verify: an owner review comment is resolved. Every number used later in P1/P2 exists in this doc.
- Depends on: none.

### P0-T2b — Core types for the contract and track model
- `parble-core`:
  - `CellCoord`, `Yaw4`, `Mirror`, `PortLevel`, `PortFace`, `Port`
  - `PieceId`, `PieceDef` (footprint, ports)
  - `Track` (piece placements and hopper config) with serde
  - `PHYSICS_VERSION` and `SCHEMA_VERSION` constants
  - seeded RNG wrapper over `rand_chacha` with named sub-streams
- No Bevy, no Rapier dependency.
- Verify: `cargo test -p parble-core` covers port transform under yaw/mirror (all 8 combos), footprint rotation, and serde round-trip. `make checkall` passes.
- Depends on: P0-T1, P0-T2a.

### P0-T3 — Deterministic sim skeleton
- `parble-sim`: Rapier world at a fixed 240 Hz, length unit tuned to the cell, CCD on marbles.
- One standard 16 mm glass marble on a primitive trimesh ramp with a floor.
- `libm` for all trig. A `trajectory_hash()` over quantized marble positions and velocities every N ticks.
- A `tick(n)` API with no wall-clock input.
- Native golden test: run 10 s of sim and compare against a committed golden hash file.
- Verify: `cargo test -p parble-sim` passes twice in a row, and with `--release`, with the same hash.
- Depends on: P0-T2b.

### P0-T4 — wasm determinism parity
- `parble-sim-wasm`: a wasm-bindgen build exposing `run_golden()`.
- `scripts/determinism/` runs it under Node and compares against the native golden hash.
- Make target `determinism-wasm` added to `checkall-ci`.
- Verify: `make checkall-ci` passes. Deliberately perturbing the step in a scratch branch makes the target fail (record the evidence in the commit message).
- Depends on: P0-T3.

### P0-T5 — Bevy web shell
- `parble-app` + `parble-render`: Bevy 0.20.1 app with WebGPU (no WebGL fallback). It renders the P0-T3 ramp and marble from sim state each frame, with a simple directional light and camera.
- `index.html` + `Trunk.toml` (port 5310, addresses 127.0.0.1).
- Copy `web/wgsl-compat.js` and `web/frame-latency.js` from hex-world (default `frame_latency=3`).
- `web/unsupported.html` for browsers without WebGPU.
- Content-hash cache busting (port `scripts/web/version-dist.mjs`).
- Verify:
  - `make dev` serves on 5310.
  - Playwright (Chromium, `--use-angle=metal --enable-gpu --ignore-gpu-blocklist`) loads the page, the canvas shows non-black pixels, the console has no errors, and `UNMASKED_RENDERER_WEBGL`/adapter info names the GPU.
  - Safari renders (manual or WebDriver) with no console errors.
- Depends on: P0-T1, P0-T3.

### P0-T6 — Agent operability
- Native flags `--screenshot <path>`, `--screenshot-after <s>`, `--exit-after <s>`, `--scenario <name>`, `--seed <u64>`, `--quality <tier>`, `--dump-state`.
- The same switches as URL query parameters. A `window.__parble` diag object exposes ready flag, tick, marble states, fps and the trajectory hash.
- `docs/agent-operability.md`. Make targets `run-shot` and `run-diag`.
- Playwright smoke spec in `e2e/` against the 5311 preview. Local done-gate only, never in CI.
- Verify: `make run-shot` writes a non-black PNG and exits by itself. The smoke spec passes in Chromium.
- Depends on: P0-T5.

### P0-T7 — TS overlay and bridge scaffold
- `web/ui` (Vite, TS, Biome, Vitest) built into `web/ui/dist` by a Trunk `pre_build` hook, as in hex-world.
- Typed bridge: Rust exports a `Bridge` with `postCommand(json)` and a per-frame `drainEvents()`. TS has matching types generated or hand-kept with a schema test.
- Placeholder HUD showing fps and tick.
- Verify: `make checkall` includes the UI typecheck/lint/test legs and passes. The HUD renders in the smoke test.
- Depends on: P0-T5.

### P0-T8 — CI and Pages workflows (no push)
- `.github/workflows/ci.yml`, one job per concern: fmt+clippy, test, wasm determinism, web build + size, gitleaks. Actions pinned per git-ci.md.
- `deploy.yml` builds with Trunk and publishes to Pages on green main.
- No browser suite in CI.
- `actionlint` added to `checkall-ci`.
- Do not create the GitHub repo or push. P5-T5 handles that after Paul approves.
- Verify: `actionlint` passes and `make checkall-ci` passes.
- Depends on: P0-T4, P0-T5.

### P0-T9 — Look-dev targets (Higgsfield)
- Using `create-game-key-art` and the `art` skill (Higgsfield backend), generate 6–10 concept frames matching `docs/reference/`. Subjects:
  - macro block channel with a cat's eye marble
  - corkscrew rail around a funnel
  - night playroom rug with lanterns
  - wide shot of a tall tower track
  - glow-inlay panel
- Store the frames under `assets-source/lookdev/` with `provenance.json` (model, prompt, date, generation count, rights note).
- Verify: `validate-generated-assets` provenance check passes and Paul picks the target set.
- Depends on: none.

## Exit criteria
Every acceptance box under PRD §6 P0 checked with evidence in the card notes.
