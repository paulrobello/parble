# Phase 7: Extended piece families

PRD: `PRD.md` §6 P7. Phase goal: the full catalog for tall, expressive tracks. Each family is a fixed set of Blender-generated catalog variants, uses kinematic mechanisms where needed, and is available to procgen.

Every task below goes through the `blender-kit-pieces` generator, the kit-check gate, provenance and sim tests, and adds `shots-test` coverage.

## Tasks

### P7-T1 — Vertical spans
- Helix/corkscrew rail around a central post at 1, 2, 3 and 4 turns × 2 radii (`ref-05`).
- Vertical drop tube with a side entry and side exit.
- Zig-zag switchback wall.
- Tower block variants with ports on multiple levels (`ref-06`).
- Verify: kit-check passes. Sim tests: exit speed band per helix variant, and no marble leaves the helix at max entry speed.
- Depends on: P2-T7.

### P7-T2 — Terrain shapes
- Wave/bump channels and the S-bend (`ref-03`, `ref-04`), funnel bowl spinner (`ref-05`), half-pipe.
- Verify: kit-check passes. Sim tests: funnel orbit time band, and the bump channel keeps the marble in at max speed.
- Depends on: P2-T7.

### P7-T3 — Musical pieces
- Xylophone/chime bar run, bell, wood-block knocker. Each bar is tuned to a note of a chosen scale (configurable per piece in the editor).
- Tuned SFX are generated with ElevenLabs or synthesized, with provenance. Notes trigger from contact events.
- Verify: sim test fires the expected note sequence for a scripted run. Paul's listening approval.
- Depends on: P2-T7, P4-T4.

### P7-T4 — Mechanisms
- Lift/elevator (kinematic, carries marbles up, enables endless loops; Open Question Q2).
- Seesaw/catapult (kinematic tip on weight threshold), turnstile, trapdoor, size sieve (12/19 mm holes per the kit contract), counterweight switch triggered by mass.
- Verify: kit-check plus one scripted sim test per mechanism. Size sieve separates minis, standards and shooters correctly.
- Depends on: P2-T7.

### P7-T5 — Decor and placeable lights
- Glow-inlay panels (`ref-09`) as decorative wall pieces with emissive strips.
- Placeable paper lanterns and lamps as pieces, counted against the shadow-light budget: only the nearest 3 cast shadows on High.
- Verify: `perf-gate` passes with 20 placed lanterns, and Paul approves against `ref-09`.
- Depends on: P2-T7, P1-T5.

### P7-T6 — Procgen integration
- Add the new families to the generator's weighted choice and slider presets (for example "tall tower" favours helix/drop tubes).
- Verify: `make procgen-sweep` with the new families, 1,000 seeds passing.
- Depends on: P7-T1, P7-T2, P7-T3, P7-T4, P6-T6.

### P7-T7 — Look approval
- Paul reviews shots against `ref-03`, `ref-05`, `ref-06`, `ref-09`.
- Verify: approval recorded in card notes.
- Depends on: P7-T1, P7-T2, P7-T5.

## Exit criteria
Every acceptance box under PRD §6 P7 checked with evidence.
