# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

- Casual players who arrive at parble.pardev.net to build a marble run for fun. They need to place pieces, release marbles and watch the run without reading docs. They use a mouse first.
- Power builders, including the owner (Paul Robello), who build tracks of 1,000+ pieces spanning long vertical and horizontal distances. They want keyboard shortcuts, multi-select, group moves and fast iteration.
- The editor serves both in layers: the learnable surface comes first, and depth (shortcuts, selection sets, groups, slicing) shows itself as the player uses it (owner answer, 2026-11-09).

## Product Purpose

parble is a desktop-browser 3D wooden marble-run sandbox: build tracks from modular wooden pieces on a 3D snap grid (or generate them), drop marbles, and watch a cinematic follow camera track the run. The MVP loop is "build, release, watch". Success is a reference-quality tabletop look with physics that feel like real 16 mm glass on wood, and an editor that handles large tracks with live validation.

## Positioning

Real-feeling tabletop physics (deterministic Rapier at 240 Hz; same build, track and seed give the same run) inside a carved warm-wood night playroom rendered at reference-photo quality in the browser, with a camera director that films each run.

## Operating Context

- Desktop browsers with WebGPU (Chromium, Safari). Unsupported browsers get an unsupported page. No touch, no mobile or tablet.
- Full-screen Bevy WebGPU canvas; the HUD and editor UI are a TypeScript DOM overlay (`web/ui`) talking to the wasm app through a JSON bridge.
- Tracks are saved in an IndexedDB library and shared by `.parble` file export/import. No accounts or server backend.

## Capabilities and Constraints

- Editor operations (core): place, remove, rotate (yaw), mirror, move group, paste group, mechanism and hopper config; undo/redo; selection sets; footprint checks.
- In-engine interaction: orbit/pan/zoom around a pivot, ray picking, ghost preview with valid/invalid tint and per-port link hints, height slicing (show levels at or below N), box select, group drag.
- Validation: open ports, broken links, unreachable pieces, as typed issues with cells.
- Run controls: hopper settings, release one/all/interval, pause, reset, follow-camera director.
- Kit families: blocks (channels, curves, slopes, drops, joins, towers, funnel, catch), rails (straight, curve, S, joint), mechanisms (hopper, finish, splitter, toggle, one-way gate and hole, stepper lift, rope arch, checkpoint), supports (pillar).
- At most 10 marbles in flight. Ten marble types in mini, standard and shooter sizes.
- Frame budget: 60 fps at High on Apple M-series at 1440p; UI must not cost frames.

## Brand Commitments

- Name: parble.
- Look: a night playroom: carved rich warm-wood blocks, round rails, patterned rug, warm paper lanterns, deep blue falloff to black. Warm amber key with pale blue fill. Reference photos in `docs/reference/` set the bar (owner rulings 2026-11-07/08).
- Glow inlay strips appear on mechanical pieces only, never on track channels (owner ruling 2026-11-07).
- Editor chrome direction (plan P3-T5): warm-wood, dark playroom chrome, no stock AI look.

## Evidence on Hand

- Reference photos: `docs/reference/ref-01..11`.
- Look-dev and lineup shots: `docs/shots/`.
- Kit review sheets: `blender/kit/review/*.png` (approved by the owner 2026-11-09).
- Owner-approved music (Suno) and SFX; mixer panel exists (`web/ui/src/audio/mixer.ts`).
- No users, testimonials or metrics yet; do not invent any.

## Product Principles

1. The scene is the hero: the UI frames the wooden run and never competes with it.
2. Learnable first, fast always: everything a newcomer needs is visible; every action also has a key.
3. Validation is help, not scolding: problems point at the piece and how to fix it.
4. Build and run are one loop: releasing a marble is never more than one action away from editing.
5. No frame cost: the overlay stays light enough to hold the 60 fps budget on large tracks.
