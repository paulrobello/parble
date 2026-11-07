# Phase 3: Editor and persistence

PRD: `PRD.md` §6 P3. Phase goal: Paul can build, validate, run, save and reload a large track entirely in the browser.

## Tasks

### P3-T1 — Track edit operations in core
- `parble-core` command model: place, remove, rotate (yaw), mirror, move group, paste group, set mechanism config, set hopper config.
- Undo/redo stacks. Selection sets.
- Footprint collision checks for multi-cell rails.
- Verify: property tests (`proptest`) show any random op sequence followed by full undo restores the original track byte-for-byte. `make checkall` passes.
- Depends on: P0-T2b.

### P3-T2 — Validation engine
- Open-port detection, broken links (port level/face mismatch), and reachability from hopper to finish over the port graph (vertical drops included).
- Unreachable pieces flagged. Output is a list of typed issues with cells.
- Verify: unit tests on crafted tracks, one per issue type. Running on the P2-T7 track yields zero issues.
- Depends on: P0-T2b.

### P3-T3 — Persistence
- `.parble` JSON schema v1 (`SCHEMA_VERSION`, `PHYSICS_VERSION`, track, mechanism states, hopper config, metadata) with a migration framework.
- TS IndexedDB library (list, save, rename, duplicate, delete with confirm).
- Export by browser download and import by file picker or drag-drop.
- Verify: Rust and TS schema tests agree on a fixture set, and export → import → run gives an identical trajectory hash (Playwright, local done-gate).
- Depends on: P3-T1.

### P3-T4 — In-engine editor interaction
- Orbit/pan/zoom camera.
- Cell picking via ray against the grid and existing pieces, with a ghost preview showing valid/invalid tint and port alignment hints.
- Height slicing (show levels ≤ N), box select, move-group drag.
- Incremental chunk rebuild of render instances and colliders on edit.
- Verify: Playwright scripted placement of 50 pieces, and the frame-time trace while editing a 2,000-piece track stays within budget (chunk rebuild amortized under 1 frame).
- Depends on: P3-T1, P1-T3, P1-T4.

### P3-T5 — Editor UI overlay
- TS palette grouped by family with thumbnails rendered by the kit pipeline.
- Toolbar: select/place/delete/rotate/mirror/slice. Keyboard shortcuts and an undo/redo UI.
- Validation panel with click-to-focus. Run controls (release one/all/interval, pause, reset, follow-cam toggle). Library browser.
- Visual direction goes through the `impeccable` skill: warm-wood, dark playroom chrome, no stock AI look.
- Verify: Vitest component tests and a Playwright UI smoke. `make checkall` passes.
- Depends on: P0-T7, P3-T2, P3-T3, P3-T4.

### P3-T6 — Editor end-to-end acceptance
- Scripted build of a 200-piece track from scratch through the UI bridge, then run it and check for no console errors.
- Export/import hash check.
- 2,000-piece edit perf trace.
- Verify: all PRD P3 acceptance boxes, evidence in `docs/perf/p3-editor.json`.
- Depends on: P3-T5, P2-T7.

## Exit criteria
Every acceptance box under PRD §6 P3 checked with evidence.
