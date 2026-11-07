# Phase 1: Look-dev vertical slice

PRD: `PRD.md` §6 P1. Phase goal: one hand-placed showcase run that matches the references, plus a 2,000-piece stress scene that holds 60 fps at High in Chrome and Safari. This phase settles the look and the rendering architecture before any piece is mass-produced.

Inputs from Phase 0: `docs/kit-contract.md` (P0-T2a), sim (P0-T3), web shell (P0-T5), look-dev targets (P0-T9).

## Tasks

### P1-T1 — Procedural wood material set
- `create-tileable-pbr-materials` with Blender 5.2 procedural nodes. Rich warm wood, a step darker than the look-dev frames' tone (owner ruling 2026-11-07) with swirling figured grain as in `ref-02`/`ref-03`. Two or three variants so neighbouring pieces differ.
- Bake to tileable base color / normal / ORM.
- Export BC7 + ASTC KTX2 (zstd) with a PNG fallback via KTX-Software 4.4.2 (`make fetch-tools` as in hex-world).
- Generator and `.blend` live in `blender/materials/`.
- Verify: seamless tiling check, texel density per the kit contract, provenance file, and a side-by-side render with `ref-02` approved by Paul.
- Depends on: P0-T2a, P0-T9.

### P1-T2 — Blender kit generator scaffold + first four block pieces
- `blender-kit-pieces` route.
- Headless generator `blender/kit/generate.py` takes the kit contract parameters and emits, per piece:
  - high-poly and low-poly `.blend`
  - Cycles-baked normal/AO maps
  - LOD0–LOD2
  - a low-poly collision mesh
  - glTF sockets for ports
  - meshopt GLB
  - a collision/port sidecar (format per the kit contract)
  - provenance
- Pieces: straight, curve 90°, gentle slope, end stop.
- Edge bevels and visible seam gaps as in `ref-02`.
- Make targets `kit-build`, `kit-check` (ports match contract, collider present, budgets), `kit-determinism` (rebuild is byte-identical).
- Verify: `make kit-check` and `make kit-determinism` pass, and reimporting the GLBs passes `validate-generated-assets`.
- Depends on: P0-T2a. Final material hookup depends on P1-T1; a placeholder material is fine until then.

### P1-T3 — Kit loader and chunk collider merge
- `parble-kit` loads the sidecars without Bevy or Rapier.
- `parble-sim` builds one trimesh per 8×8×8 chunk from all piece collision meshes in sorted (cell, piece) order, with `TriMeshFlags::FIX_INTERNAL_EDGES`.
- Exposes an incremental chunk rebuild API.
- Seam test: a marble rolls 20 straights in a row and across the curve and slope joins. Assert the peak vertical velocity spike stays under a threshold. Proposed starting threshold is 2 mm/s above the smooth-ramp baseline; tune it and record the final value in the test.
- Verify: `cargo test -p parble-kit -p parble-sim` passes and the golden hashes stay deterministic native and wasm (`make checkall-ci`).
- Depends on: P0-T3, P1-T2.

### P1-T4 — Instanced chunked renderer
- `parble-render` draws pieces instanced per piece type per chunk (hex-world instanced layout).
- meshopt GLB decode via `meshoptimizer-rs`, KTX2 family selection (BC7 → ASTC → PNG) ported from hex-world `hex-assets`.
- LOD per chunk with dithered crossfade (avoid hex-world's hard-cut gap).
- Verify: the 2,000-piece scene loads and draws with ≤ 1 draw call per (piece type × LOD × chunk). The diag object reports draw calls.
- Depends on: P0-T5, P1-T2.

### P1-T5 — Night playroom environment
- Patterned rug floor material (`ref-07`/`ref-08`) via `create-tileable-pbr-materials`.
- Room shell falling off to dark blue.
- Paper lantern model (emissive shade + point light) and a warm lamp.
- Toy-block set dressing: arches and towers.
- Higgsfield backdrop plate for the out-of-focus background.
- Baked environment probe (Blender) for reflections and ambient light.
- At most 3 shadow-casting lights on High.
- Verify: Paul approves shots against `ref-07`/`ref-08`. Provenance is recorded for every generated texture and plate.
- Depends on: P0-T5, P0-T9.

### P1-T6 — Marble types and materials
- `parble-sim` marble type table: cat's eye glass 16 mm, solid glossy 16 mm, steel 16 mm. Each has radius, mass, friction, restitution and rolling resistance from real values; record the sources.
- Render materials:
  - glass uses specular transmission + IOR 1.5 with inner vane geometry
  - solid glossy uses a clearcoat-like look
  - steel is metallic with environment reflections
- Fallback glass shader fakes the inner vane volume, for Low/Medium.
- Vane pattern textures from Higgsfield, with provenance.
- Verify: transmission renders in Chrome and Safari with no validation errors. The fallback renders on Low. A screenshot check is added to `shots-test`.
- Depends on: P0-T3, P0-T5, P0-T9.

### P1-T7 — Post stack, quality tiers, follow camera
- Bevy TAA, SSAO, bloom, bokeh DoF, tonemapping chosen during look-dev and recorded in ADR.
- Quality tiers Low/Medium/High/Ultra as a resource. URL `?quality=` and native `--quality`.
- Follow-marble director:
  - low, close tracking shot with spring damping
  - focus distance on the marble
  - aperture per tier
  - cuts on scripted cues
- Orbit camera for later editor use.
- TAA ghosting check: a marble at max speed crossing the frame under the follow camera. Add a shot comparison.
- Verify: `shots-test` covers each tier with no ghost trail beyond the threshold set here. Paul approves the DoF strength against `ref-01`.
- Depends on: P0-T5.

### P1-T8 — Showcase and stress scenarios, perf gate
- `--scenario showcase`: a hand-placed run of ~40 pieces through the environment, with the follow camera.
- `--scenario stress2000`: 2,000 pieces across a tall tower with 10 marbles.
- `make shots-test` captures the fixed camera shots and compares them.
- `make perf-gate` runs the paced p95 over 600 frames after READY at 1440p High, Chromium with GPU flags, and Safari. The host-load guard requires 1-minute load ≤ 15 before and after; discard and re-run when exceeded. Book the host per the house load rules. Output goes to `docs/perf/p1-gate.json`.
- `make web-size` budget ≤ 15 MB gzip.
- Verify: perf gate p95 ≤ 16.7 ms in both browsers on both scenarios. Web size within budget.
- Depends on: P1-T3, P1-T4, P1-T5, P1-T6, P1-T7.

### P1-T9 — Look sign-off and ADR
- `docs/decisions/0001-render-look.md` records the post settings, light budget, tiers and measured numbers.
- Paul reviews showcase shots set next to `ref-01`–`ref-04`, `ref-07`.
- Verify: the ADR is committed and Paul's approval is recorded in the card notes.
- Depends on: P1-T8.

## Exit criteria
Every acceptance box under PRD §6 P1 checked with evidence.
