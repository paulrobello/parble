# parble — Product Requirements

| Field | Value |
|---|---|
| Product | parble: a browser 3D wooden marble-run sandbox |
| Repo | `~/Repos/parble` → `github.com/paulrobello/parble` (public) |
| Owner | Paul Robello |
| Status | Planned. No code yet. |
| Date | 2026-11-07 |
| Live URL (target) | https://parble.pardev.net (WebGPU required) |
| Dev ports | 5310 (Trunk dev server), 5311 (static preview for Playwright) |

## 1. Overview

parble is a desktop-browser marble-run game. Players build tracks from modular wooden pieces on a 3D snap grid, or generate them procedurally, then drop marbles and watch a cinematic follow camera track the run with shallow depth of field. The look is a night playroom: carved rich warm-wood blocks (a step darker than the approved look-dev frames' tone, owner ruling 2026-11-07) and smooth round rails standing on a patterned rug, lit by warm paper lanterns and a lamp, with deep blue falloff to black. The reference photos in `docs/reference/` set the visual bar.

The MVP is a sandbox ("build, release, watch"). The roadmap adds procedural generation, extended piece families, and a playable mode where the player steers a marble to the end of a track, collects other marbles and swaps between them to pass size gates.

### Reference images (`docs/reference/`)

| File | What it shows | Used for |
|---|---|---|
| `ref-01.webp` | Carved block channel, close macro shot, warm key light, heavy DoF | Block family profile, lighting mood, DoF strength |
| `ref-02.webp` | Cat's eye marble in a block channel, tiled block wall with seams | Marble glass look, visible piece seams, wood grain scale |
| `ref-03.webp` | Wave channels and an S-bend tube cutting through a wall | Terrain shapes (P7), seam-to-seam continuity |
| `ref-04.webp` | Blue opal marble on a bump channel | Opal marble, bump/wave pieces |
| `ref-05-corkscrew-funnel.webp` | Round rail corkscrew around a funnel bowl, S-rail spans | Rail family, helix piece, funnel (P7) |
| `ref-06-rail-into-block-tower.png` | Round rail entering a hole in a stacked block tower | Rail to block port standard, tower drop pieces |
| `ref-07-playroom-rug-lantern.webp` | Patterned rug floor, glowing paper lantern, scattered blocks | Environment, practical lights, floor material |
| `ref-08-playroom-blocks.webp` | Block towers and arches on the rug around a lantern | Set dressing, environment composition |
| `ref-09-glow-inlay-panels.webp` | Wood panels with thin emissive inlay strips | Optional light-strip accent on mechanical pieces only. Never on track channels or plain track pieces (owner ruling 2026-11-07, look-dev frame 08 rejected). |
| `ref-10-one-way-gate.png`, `ref-11-one-way-gate-side.png` | Hinged wooden arch flap over a channel | One-way gate piece |

## 2. Goals

1. **Reference-quality look in a browser.** Warm wood with visible grain and piece seams, glass marbles with refraction and inner vanes, practical-light lighting, SSAO, bloom and bokeh depth of field. Measured: owner approval of look-dev shots set side by side with the references.
2. **Physics that feel real at tabletop scale.** Marbles roll, bank, drop and click like 16 mm glass on wood, with no seam bumps between pieces. Runs are deterministic: same build, track and seed give the same run.
3. **Modular track building at large scale.** A 3D snap-grid editor handles tracks of 1000+ pieces spanning long vertical and horizontal distances, with live validation.
4. **Procedural tracks that always work.** Every generated track has passed a headless physics run before the player sees it.
5. **Rich reactive sound.** Roll, impact and mechanism sounds respond to speed, contact force, marble type and surface. Warm acoustic ambient music plays underneath.
6. **60 fps at High** on Apple M-series and RTX 3060-class desktop GPUs, at 1440p with the full post stack.

## 3. Non-Goals

- Mobile phones and tablets. Touch input is out of scope.
- Browsers without WebGPU. They get an unsupported page, not a WebGL fallback.
- Online gallery, accounts or any server backend. Sharing is via file export.
- Share-by-URL (deferred, see Open Questions).
- Multiplayer.
- Real-time ray-traced or path-traced global illumination. Lighting uses baked AO, a baked environment probe and screen-space effects.
- Simulated hinge joints for mechanisms. Mechanisms are kinematic (see §5.4).
- More than 10 marbles in flight at once.

## 4. Reference Projects

| Repo | What parble reuses |
|---|---|
| `~/Repos/hex-world` (primary) | Bevy `=0.20.1` → wasm32 WebGPU via Trunk 0.21.14; workspace-of-crates layout with a directed crate graph (ADR 0003); the TypeScript HUD overlay in `web/ui` built by a Trunk `pre_build` hook; content-hash cache busting (`scripts/web/version-dist.mjs`); `web/wgsl-compat.js` Safari shim (Bevy 0.20's `texture_and_sampler_let` WGSL dependency); `web/frame-latency.js` (`frame_latency=3` removes Chrome's uncapped WebGPU stall tail); the Blender kit pipeline with meshopt GLB + BC7/ASTC KTX2 (zstd) textures loaded by Bevy's `ktx2` feature, with no Basis transcoder; the provenance and kit-check gates; Makefile gate shape (`checkall`, `checkall-ci`, `web-size`, `perf-gate`, `shots-test`); agent-operability doc; the GitHub Pages deploy. Measured lesson: per-entity cost blew the frame budget until pieces moved to an instanced, chunked layout, so parble builds instancing in from P1. |
| `~/Repos/the-cube` | Rapier usage patterns, sfx audition page for auditioning generated audio in context, perf/balance docs under `docs/`, Playwright GPU flags. |
| `~/Repos/match3` | The full asset artifact set (`docs/art`, `docs/audio`, provenance) as a pattern for the audio and key-art docs. |

## 5. Architecture Direction

### 5.1 Stack (pins live in the plans, not in manifests yet)

| Concern | Choice |
|---|---|
| Language / engine | Rust stable, Bevy `=0.20.1` with default features off, wasm32-unknown-unknown |
| Bundler | Trunk 0.21.14 |
| Physics | `rapier3d` 0.36.1 used directly (not `bevy_rapier3d`), features `enhanced-determinism`, no `parallel` |
| Math in the sim | `libm` for every trig/transcendental call in gameplay and sim code |
| UI overlay | TypeScript in `web/ui` (Vite build, Biome lint, Vitest), talking to wasm through a typed bridge |
| Audio | TypeScript Web Audio engine in `web/ui/src/audio` (shipped in the HUD bundle), fed by a per-frame event buffer from Rust. Bevy audio stays off. |
| Assets | Blender 5.2 headless generators in `blender/`, KTX-Software 4.4.2, meshopt GLB |
| Persistence | IndexedDB track library + `.parble` JSON file export/import |
| Hosting | GitHub Pages at `parble.pardev.net` |

### 5.2 Crate graph (directed, acyclic)

```
parble-app ──► parble-render ──► parble-kit ──► parble-core
parble-app ──► parble-sim ──► parble-kit, parble-core   (rapier3d lives here only)
parble-app ──► parble-procgen ──► parble-sim
parble-render ──► parble-sim (read-only state for interpolation)
parble-sim-wasm ──► parble-procgen, parble-sim   (wasm-bindgen worker build, no Bevy)
```

- **parble-core** (no Bevy, no Rapier): grid math, the kit contract (cell size, port standard, channel classes), the track model and `.parble` serde schema with versioning, validation (open ports, broken links, reachability), seeded RNG (`rand_chacha`).
- **parble-kit** (no Bevy, no Rapier): piece catalog and loading of the per-piece collision mesh and port sidecar exported by the Blender pipeline.
- **parble-sim** (no Bevy): the Rapier world at a fixed 240 Hz with CCD, the chunk collider merge (one trimesh per chunk with `FIX_INTERNAL_EDGES`, built from kit meshes in a stable sorted order), marble types (radius, mass, friction, restitution, rolling resistance), kinematic mechanism state machines advanced by tick count, sensors, the hopper and finish collector, a trajectory hash, and the audio event stream. This crate is the determinism boundary.
- **parble-procgen**: seed + slider generation, fill-between routing, infinite-descent segments, and validation by headless sim runs.
- **parble-sim-wasm**: a small wasm-bindgen build of sim + procgen for a Web Worker, so procgen validation never blocks the render thread.
- **parble-render** (Bevy): instanced piece rendering per piece type with chunking, wood/marble/inlay materials, lighting rig, post stack and quality tiers, environment.
- **parble-app** (Bevy): app wiring, editor systems, cameras (orbit editor camera and follow-marble director), the wasm↔TS bridge, agent-operability flags.

### 5.3 Kit contract (fixed in P0, binding on every later phase)

- **Cell:** 60 mm cube. World units are metres.
- **Marbles:** mini 10 mm, standard 16 mm, shooter 25 mm. Max size 25 mm.
- **Channel:** one channel class, 28 mm inner width with a round bottom, shared by the block family (carved U-channel) and the rail family (round trough). Rails plug into block ports.
- **Size gates:** a 12 mm hole passes minis only, a 19 mm hole passes up to standard.
- **Ports:** a port is (cell face, floor height level). Side faces carry horizontal ports at floor height levels in 15 mm steps. Top and bottom faces carry vertical drop ports. Exact levels, tolerances and the slope gradients they imply are fixed by the P0 kit-contract task.
- **Orientation:** yaw only, 4 orientations, plus a mirror flag for chiral pieces. Pieces never tip on their sides.
- **Slope profiles:** the slope floors and the redirect's landing floor use a C2-continuous profile, the quintic smootherstep (steep decided 2026-11-08, gentle and landings approved 2026-11-08), so their seams stay under the seam bump limits. The tower socket lands a falling marble on a pad falling 3 % to the socket, then a C2 descent (kit-contract §7).
- **Footprints:** most block pieces fill 1 cell, and some span several cells (for example the 2×1×2 stepper lift and large funnels, owner ruling 2026-11-08). Rail pieces span several cells (for example S-rail 1×3, helix 2×2×N) and land their ends on grid ports.

### 5.4 Simulation rules

- Fixed 240 Hz step. Rapier's length unit is tuned to the cell. Marbles use CCD.
- Pieces in a chunk merge into one trimesh with `FIX_INTERNAL_EDGES` so marbles never bump at seams. A chunk rebuilds when the track is edited.
- Mechanisms (toggle, splitter, one-way gate, one-way hole, checkpoint, hopper) are **kinematic**: sensors detect the marble, a state machine decides the outcome (the splitter uses the seeded RNG in Random mode and alternates in Alternate mode), and an animated flap with a kinematic collider routes the marble.
- The redirect drop uses real internal tunnel geometry. The marble never teleports.
- Controlled free falls are allowed (owner ruling 2026-11-08): a marble may leave a port and fall a short way if its landing is controlled, for example into a catcher, funnel, cup or channel entry below. The validator treats such a drop as a link, not an open port, when the landing piece is within the allowed fall height and horizontal tolerance of the predicted trajectory. A launcher or catapult piece is planned later.
- Channel floors that a marble rolls or lands on (bends, slopes, joiners, redirect and tower landings) use exact analytic contacts instead of the faceted trimesh, so seams stay under the seam bump limits.
- A low-speed rolling-resistance model lets slow marbles settle instead of creeping forever (approved 2026-11-08; it moves every golden hash once, deliberately).
- Marbles keep their natural bounce on landings: the tower landing's sideways wobble is accepted as is (2026-11-08) and revisited only if play feels off.
- Determinism contract: identical trajectory hash for the same build, track and seed, natively and in wasm. Saves and replays carry a physics version. Old replays may diverge after a physics change, but seed + track still regenerate a run.

### 5.5 Rendering

- Instanced rendering per piece type in 8×8×8-cell chunks from the first scene (hex-world lesson).
- Lighting: Blender-baked AO per piece, a baked room environment probe, clustered lights for lanterns and the lamp, at most 3 shadow-casting lights on High, SSAO, TAA, bloom, bokeh DoF focused on the followed marble.
- Light colour (decided 2026-11-08): the references mix warm yellow light with a pale blue, and that contrast carries the mood. Scenes use a warm key with pale blue fill or rim, and lights and glow inlays come in warm amber and pale blue.
- Glass marbles use Bevy specular transmission with inner vane geometry. Low and Medium tiers use a fallback shader that fakes the inner volume.
- Quality tiers Low / Medium / High / Ultra, auto-selected at first launch and changeable in settings.
- Safari runs through the hex-world `wgsl-compat.js` shim. Every Bevy upgrade needs a Safari smoke test.

### 5.6 Cameras

- **Editor:** orbit / pan / zoom around a pivot, plus height slicing.
- **Run:** a camera director (decided 2026-11-08). The default shot is a low, close tracking shot with focus pulled to the marble. Every change of shot is a smooth blend (eased position, aim, field of view and focus), never a hard cut. Following never whips around: yaw rate and acceleration are limited, so corners and direction changes do not swing the camera. The aim leads the marble along its upcoming path and holds toward the centre of the action. Framing and focus cover the action area (the recent and upcoming path plus any active zone or mechanism), not just the marble (2026-11-08). When the followed marble enters a hidden path (a hole, tunnel or drop), the director predicts the exit from the track graph and moves so the exit is in view before the marble reappears. Track pieces never block the view: the director checks line of sight to the action, anticipates blockage from the upcoming path, and moves smoothly to a clear view (raise, swing round or pull back).
- **Camera zones:** a zone is a trigger volume with a shot rule. When the followed marble enters it, the director blends to the zone's shot and blends back when it leaves. Shot rules include follow (distance, height, lead), pull back and frame a target, orbit a point, and fixed. The target can be the marble, a point, or a vertical line (for example a helix or spiral axis: the camera pulls back and frames the axis while the marble winds around it). Zones have a priority and blend times.
- **Where zones come from:** pieces carry default camera hints (a helix frames its axis, a tower frames its drop, a checkpoint pass is a cue), and tracks can carry authored zones that the editor places and edits like pieces. A debug overlay draws zones and the active shot.

### 5.7 Audio

- SFX come from ElevenLabs: roll loops per marble type × surface (wood channel, rail, rug) and one-shot variation sets for impact, drop, click, flap, toggle, hopper and finish.
- The TS engine sets roll-loop pitch, gain and filter from speed, triggers impacts from contact force, spatializes relative to the camera and limits polyphony.
- Music comes from Suno: warm acoustic ambient beds (felt piano, marimba, soft strings), several loopable tracks with crossfades. Claude writes the prompts. Paul generates and selects the WAVs per `~/.claude/guides/suno.md`.

### 5.8 Asset pipeline

Asset work goes through the house asset skills:
- `blender-kit-pieces` under `create-modular-3d-assets` for pieces.
- `create-tileable-pbr-materials` for the procedural wood and rug.
- `create-game-audio-assets` with `elevenlabs` for SFX and music preparation.
- `create-game-key-art` with Higgsfield, via the `art` skill, for key art, look-dev targets, marble pattern textures and environment backdrop plates.
- `validate-generated-assets` before any asset is promoted.

Generation counts are recorded in provenance files. ElevenLabs and Higgsfield run on Paul's existing subscriptions without approval per batch.

### 5.9 Verification gate

| Gate | Contents |
|---|---|
| `make checkall` (fast, fail-fast order) | `cargo fmt --check` and TS format check → clippy (native and wasm32) and Biome lint → `tsc` → `cargo test` (core, kit, sim, procgen) and Vitest → native determinism golden test |
| `make checkall-ci` (CI parity) | `checkall` + the wasm determinism golden test (sim-wasm under Node must match the native trajectory hash) + `make web-size` (wasm ≤ 15 MB gzip) + gitleaks + actionlint |
| Local done-gate (browser, never in CI) | Playwright WebGPU smoke in Chrome and Safari, `shots-test` look-dev screenshot comparison, `perf-gate` on the 2,000-piece stress scene under the host-load guard (1-minute load ≤ 15 before and after) |

Pre-commit with gitleaks and detect-private-key is installed before the first push.

### 5.10 Agent operability

Per `~/.claude/guides/agent-operability.md`:
- Native flags: `--screenshot`, `--screenshot-after`, `--exit-after`, `--scenario <name>`, `--seed`, `--dump-state`, `--quality`.
- The same switches as URL query parameters on the web.
- A `window.__parble` diag object for Playwright.

## 6. Phases

Each phase ends with green gates and something demonstrable. P0–P5 make up the MVP.

### P0 — Foundations

Scope:
- `git init`, Cargo workspace, crate skeletons, rust-toolchain, Trunk, the TS overlay scaffold, Makefile gates, pre-commit, agent-operability flags.
- Kit contract document and the `parble-core` types that encode it.
- Deterministic sim skeleton: one marble rolling down a primitive ramp, with a golden trajectory hash matching natively and in wasm.
- Empty Bevy scene rendering in Chrome and Safari via WebGPU, with the wgsl-compat and frame-latency shims.
- CI workflow and Pages deploy workflow written. Going live waits for Paul's push approval.
- Look-dev targets generated with Higgsfield.

Acceptance:
- [x] `make checkall` and `make checkall-ci` pass. (Re-checked 2026-11-11 at e48ddaa: `docs/perf/p5-release.json`)
- [x] Golden trajectory hash is identical across native and the Node wasm run. (Re-checked 2026-11-11 at e48ddaa: `docs/perf/p5-release.json`)
- [x] `make dev` serves on 5310. The page renders a lit primitive ramp and marble in Chrome and Safari with no console errors. (Re-checked 2026-11-11 at e48ddaa: `docs/perf/p5-release.json`)
- [x] `--screenshot` / `--exit-after` work natively. (Re-checked 2026-11-11 at e48ddaa: `docs/perf/p5-release.json`)
- [x] `docs/kit-contract.md` fixes cell size, port positions and levels, channel cross-section, marble radii and gate diameters. (Re-checked 2026-11-11 at e48ddaa: `docs/perf/p5-release.json`)
- [x] Look-dev target images and their provenance are committed. (Re-checked 2026-11-11 at e48ddaa: `docs/perf/p5-release.json`)

### P1 — Look-dev vertical slice

Scope:
- Procedural wood material (Blender-baked) and rug material.
- Night playroom environment with lanterns, lamp, toy-block dressing and backdrop plate.
- First four Blender-generated block pieces (straight, curve, slope, end).
- Three marbles: cat's eye glass, solid glossy, steel.
- Post stack, follow camera, quality tiers, and one hand-placed showcase run.
- A 2,000-piece synthetic stress scene for the perf gate.

Acceptance:
- [x] Owner approves showcase shots set side by side with `ref-01`–`ref-04` and `ref-07`. (Paul, 2026-11-10: `docs/decisions/0001-render-look.md`)
- [x] Glass transmission renders in Chrome and Safari, the fallback shader renders on Low, and TAA shows no ghosting on a fast marble under the follow camera. (Re-checked 2026-11-11 at e48ddaa: `docs/perf/p5-release.json`)
- [x] No visible or physical bump at piece seams: the marble's vertical-velocity spike stays below the threshold set in the plan. (Re-checked 2026-11-11 at e48ddaa: `docs/perf/p5-release.json`)
- [x] Perf gate: paced p95 ≤ 16.7 ms at 1440p High on M4 Max in Chrome and Safari, on both the showcase and the 2,000-piece stress scene, under the load guard. (Re-checked 2026-11-11 at e48ddaa: `docs/perf/p5-release.json`)
- [x] wasm ≤ 15 MB gzip. (Re-checked 2026-11-11 at e48ddaa: `docs/perf/p5-release.json`)

### P2 — Core kit and mechanisms

Scope: the full MVP piece set from Blender generators, with collision sidecars and kinematic mechanisms.

1. Start hopper: holds a marble queue and releases one, all, or on an interval. Design rule (2026-11-08): only one marble is in motion at a time in normal play, so the default release waits until the moving marble has finished or come to rest. Several marbles moving at once stays supported as a nice-to-have (stress tests, "all" release), not a design target.
2. Finish collector cup: counts arrivals and records times.
3. Straight
4. Curve 90° (mirrorable)
5. Gentle slope (one height level per cell)
6. Steep slope
7. Drop hole (vertical port down)
8. Redirect drop (internal tunnel, exits sideways one level lower)
9. Splitter with a per-piece mode the player switches in the editor: Random (seeded, the default) or Alternate (left/right in turn, like the toggle)
10. Joiner
11. L/R toggle (flip-flop)
12. Checkpoint arch (sensor, split timer, camera cue)
13. One-way gate (hinged arch flap, `ref-10`)
14. One-way hole (trapdoor that drops marbles arriving from one direction and lets marbles from the other direction roll over)
15. Support pillar and tower block with ports (`ref-06`)
16. Rail family proof: rail straight, rail curve, S-rail 1×3, and a rail-to-block port joint

Added 2026-11-08:

17. Stepper elevator (`ref-09`): a row of vertical slats of rising height. Alternate slats move up and down, and each slat top is slanted so the marble steps onto the next higher slat. Height and length vary with the room available. The slats run only while a marble is on the lift, so the entry slat is always ready, and the moving slat resets smoothly if the marble is moved back off the start. It links a low port to a higher one, so tracks can climb and the validator sees a real link instead of a free-fall hand-off. Other elevator types stay in P7.
18. Rope-curtain arch: an arch over part of one cell's length with ropes hanging into the channel. A marble passing through loses about half its speed and the ropes swing. The swing starts cosmetic (driven from the sim's drag state) with the rope model kept swappable for real rope physics later.

Look: every moving part of a mechanism (flaps, blades, trapdoors, gates, slats) uses a medium-stain wood, darker than the body, for contrast.

Acceptance:
- [x] Every piece passes kit-check: ports match the contract, collider and LODs are present, provenance is recorded. (Re-checked 2026-11-11 at e48ddaa: `docs/perf/p5-release.json`)
- [x] A scripted sim test per mechanism proves the routing behaviour (toggle alternates, splitter matches the seeded sequence in Random mode and alternates in Alternate mode, one-way pieces block reverse travel). (Re-checked 2026-11-11 at e48ddaa: `docs/perf/p5-release.json`)
- [x] A test track using every piece runs 10 marbles to the finish deterministically. (Re-checked 2026-11-11 at e48ddaa: `docs/perf/p5-release.json`)

### P3 — Editor and persistence

Scope:
- Palette, place, rotate (yaw), mirror, delete.
- Undo/redo, multi-select, copy/paste, move groups.
- Live validation overlay (open ports, broken links, unreachable pieces).
- Marble check (owner request 2026-11-09): on demand from the validation panel, the editor runs the exact release the Run button would make once per marble type in a headless sim, off the render frame, and reports where each type falls off or stalls, as placement-anchored warnings with click-to-focus. They are warnings, never errors: Paul accepted that not every type completes every track, and in the later drivable-marble mode the player can push and hop a marble past a weak spot. A result goes stale on any edit.
- Nudge zones (owner request 2026-11-09): an invisible trigger volume, placed and edited like a camera zone and stored with the track (not a placement, so it never overlaps a piece), that gently pushes a marble along a set direction while its centre is inside. It keeps marbles moving past weak spots without a visible effect: the default only tops a marble up to a minimum speed along the direction, with a low capped acceleration. It is drawn only in the editor (a volume outline and direction arrow), never in a run. The push is tick-based in the deterministic sim, so a track with no nudge zones keeps its hash, and the marble check sees nudges like any other part of the track. Built (`Track::nudge_zones`, `crates/parble-core/src/nudge_zone.rs`): a zone is an axis-aligned µm box, a heading (yaw and pitch in millidegrees), a minimum speed and an acceleration cap. Each tick, in spawn and track order, a marble whose centre is inside and whose speed along the heading is below the minimum gains `min(cap · dt, shortfall)` along it, and nothing else (top-up mode: never slows a marble, never pushes sideways). Defaults 0.2 m/s and 1 m/s², the pull of a 1-in-10 incline, so a stopped marble reaches the minimum in 0.2 s over 2 cm; a file may raise the cap to 2 m/s² and the speed to 2 m/s, no further. Constant mode (owner decision 2026-11-10): a zone with `"mode": "constant"` pushes every marble inside at its acceleration (`max_accel_um_s2`, at most 2 m/s²) along the heading every tick, whatever its speed, in the same order and with the same libm direction; the minimum speed is unused. Top-up stays the default and is omitted from the file, so older files and every golden are unchanged (no schema bump). The zone inspector's Mode radio (Top up / Constant) switches it, one undoable `nudgeSet`.
- Height slicing and the orbit camera.
- Run controls: hopper settings, release, pause, reset.
- IndexedDB library and `.parble` export/import with a versioned schema and migrations.

Acceptance:
- [x] Build a 200-piece track from scratch and run it with no console errors. (2026-11-11: `e2e/tests/editor-acceptance.spec.mjs`, `docs/perf/p3-editor.json`)
- [x] Export, re-import and run reproduce the identical trajectory hash. (2026-11-11: `editor-acceptance.spec.mjs` on the 200-piece track, `persistence.spec.mjs` against the native hash)
- [x] Editing a 2,000-piece track keeps frame time inside budget (chunk rebuild ≤ 1 frame budget amortized). (2026-11-11: `make web-editor-perf`, worst-frame rebuild 10.9 ms, Run build with no dropped frame, `docs/perf/p3-editor.json`)
- [x] Undo/redo round-trips every operation (property test). (`crates/parble-core/tests/edit.rs` `full_undo_restores_original_bytes`, 512 cases)

### P4 — Audio

Scope:
- ElevenLabs SFX set with provenance and an audition page.
- TS Web Audio engine: roll loops driven by speed, impacts by force, mechanism one-shots, spatialization, voice limits.
- Suno prompt pack, owner-generated music, loop preparation, crossfading music player.
- Mixer settings.

Acceptance:
- [x] Each marble type sounds distinct on each surface (owner listening approval). (Re-checked 2026-11-11 at e48ddaa: `docs/perf/p5-release.json`)
- [x] No clipping or voice starvation with 10 marbles on the stress track. (Re-checked 2026-11-11 at e48ddaa: `docs/perf/p5-release.json`)
- [x] Audio CPU stays within the frame budget. (Re-checked 2026-11-11 at e48ddaa: `docs/perf/p5-release.json`)
- [x] Music loops seamlessly. (Re-checked 2026-11-11 at e48ddaa: `docs/perf/p5-release.json`)

### P5 — MVP polish and release

Scope:
- Marble picker with the full sandbox set: swirl, onionskin, opal, glow, minis and shooters.
- Title screen with key art, settings, first-run quality detection.
- Unsupported-browser page.
- Social card.
- Go live at `parble.pardev.net`.

Acceptance:
- [x] All P0–P4 criteria still hold. (2026-11-11 release gate at e48ddaa: `docs/perf/p5-release.json`)
- [ ] The live site renders in Chrome and Safari, verified by an actual page render and not an HTTP 200.
- [ ] The DNS → CNAME → Pages API sequence is followed in order.
- [ ] Owner MVP sign-off.

### P6 — Procedural generation

Scope:
- Seed + slider generator (footprint, height, density of special pieces, difficulty).
- Generated tracks open in the editor.
- Fill-between: route a valid path between two chosen ports.
- Infinite descent mode, which generates segments below the marble ahead of time.
- All validation runs headless in the sim-wasm Web Worker in time-sliced batches.
- Infinite descent validates segment by segment ahead of the marble and recycles segments far above it.

Acceptance:
- [ ] 1,000 generated seeds all reach the finish in headless validation, with no failure shipped to the player.
- [ ] Generation of a 500-piece track completes in under the time budget set in the plan, without dropping render frames.
- [ ] Infinite descent runs for 30 minutes with bounded memory.

### P7 — Extended piece families

Scope:
- **Vertical spans:** helix 1–4 turns at two radii, vertical drop tube, zig-zag switchback wall.
- **Terrain:** wave/bump channels, S-bends, funnel bowl spinner, half-pipe.
- **Screw lift** (owner request 2026-11-08): an Archimedes screw elevator. The marble enters at the bottom and is held against a fixed guide or tube while a rotating helical screw turns, so it climbs the spiral and leaves at a higher port. Multi-cell vertical footprint; the screw turns only while a marble is on it.
- **Large funnel catcher** (owner request 2026-11-08): a multi-cell funnel (for example 2×2 and 3×3 footprints) that catches controlled free falls and launcher shots over a wide area and spirals the marble down to a centre exit port. Marbles can also roll in from the track: the funnel has side entry ports at track level whose inlets feed the marble tangentially onto the bowl rim. Funnel adapter pieces join funnels to the rest of the kit: channel-to-rim inlets, rail-to-rim inlets, and centre-exit adapters that turn the drop out of the bottom into a channel or rail port.
- **Musical:** xylophone/chime bars, bell, wood-block knockers, each tuned to a scale.
- **Mechanisms:** further lift/elevator types (the stepper elevator ships in P2), seesaw/catapult, turnstile, trapdoor, size sieve, counterweight switch.
- **Decor:** placeable lanterns. Light-strip inlays (`ref-09`) are optional and only allowed on mechanical pieces (lift, seesaw, turnstile and similar). Track channels and plain track pieces never carry them.

All of these are fixed Blender catalog variants.

Acceptance:
- [ ] Each family passes kit-check and has scripted sim tests.
- [ ] Procgen can use the new pieces.
- [ ] Owner look approval against `ref-03`, `ref-05`, `ref-06`.

### P8 — Steer-play mode

Scope:
- Limited nudge force and a small hop with a speed cap.
- Checkpoints respawn the marble.
- Size-gated holes.
- Collectible marbles placed on tracks. The player swaps to a collected marble (size and mass) to pass gates or trip heavy switches.
- Timer and bronze/silver/gold medals.
- Seeded ghost replays.

Acceptance:
- [ ] A test level needs at least two marble swaps to finish, and a scripted input replay finishes it deterministically.
- [ ] A ghost replay matches the recorded run.

### P9 — Level pack

Scope: a curated, hand-authored level set with medal times, level select and progress saved locally.

Acceptance:
- [ ] At least 12 levels, each completed by a scripted replay under gold time.
- [ ] Owner playtest sign-off.

## 7. Risks

| ID | Risk | Mitigation |
|---|---|---|
| R1 | Glass marble refraction is too expensive or broken on WebGPU (especially Safari) | P1 gate tests transmission in both browsers. A fallback shader fakes the inner vanes. |
| R2 | Seam bumps between pieces ruin the rolling feel | Merged chunk trimesh with `FIX_INTERNAL_EDGES`, seam bump metric in the P1 gate |
| R3 | Small, fast marbles tunnel through walls | 240 Hz fixed step and CCD, Rapier length unit tuned to the cell |
| R4 | Native and wasm, or macOS arm64 and Ubuntu x86_64 CI, diverge and break replays and procgen validation | `enhanced-determinism` (cross-platform IEEE determinism), `libm`, tick-based mechanisms, sorted collider merge, no `parallel` or SIMD features. One committed golden hash is checked natively and in wasm by `checkall-ci`. It is also run on the Parallels Ubuntu VM before the first push, since `git-ci.md` records four red CI pushes from cross-arch Rapier divergence. |
| R5 | 1000+ pieces blow the frame budget (hex-world per-entity lesson) | Instanced chunked rendering from P1, 2,000-piece stress scene in the perf gate |
| R6 | Safari breaks on Bevy shader features | `wgsl-compat.js` shim, Safari smoke on every Bevy upgrade |
| R7 | Chrome uncapped stall tail | `frame_latency=3` default |
| R8 | Procgen validation is too slow (240 Hz × long runs, single-threaded wasm) | Web Worker sim-wasm, time-sliced batches, segment-wise validation for infinite descent |
| R9 | Generated assets drift from the look or carry unclear rights | Look-dev targets in P0, provenance, `validate-generated-assets`, owner approval gates |
| R10 | wasm size grows past budget | `web-size` in `checkall-ci`, wasm-release profile with `opt-level = "z"` and per-crate `opt-level = 3` for hot crates |
| R11 | Bevy 0.20 → 0.21 upgrade churn mid-project | Exact pin `=0.20.1`. Upgrades are a deliberate card with a Safari smoke test. |

## 8. Open Questions

| ID | Question | Default until answered |
|---|---|---|
| Q1 | Share-by-URL (compressed track or seed in the URL) | Not planned. File export only. |
| Q2 | Finish collector recycling marbles back to the hopper (endless run) | Not in MVP. Now possible with the P2 stepper elevator; decide before P5. |
| Q3 | Hand-drop a marble anywhere on the track | Not in MVP. Hopper only. |
| Q4 | Gamepad support for steer-play mode | Keyboard and mouse only |
| Q5 | Exact medal thresholds and level count beyond 12 | Set during P9 playtesting |
