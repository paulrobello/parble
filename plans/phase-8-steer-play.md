# Phase 8: Steer-play mode

PRD: `PRD.md` §6 P8. Phase goal: a playable mode where the player nudges a marble to the end of a track, collects other marbles and swaps between them to pass size gates and heavy switches, against the clock.

## Tasks

### P8-T1 — Player input as deterministic sim input
- Input is sampled per physics tick:
  - WASD nudge force relative to the camera yaw, capped
  - a small hop with a cooldown, only when grounded
  - a speed cap
- Inputs are recorded as a per-tick stream so a run replays exactly.
- Verify: unit tests show a recorded input stream replays to the identical trajectory hash native and wasm.
- Depends on: P5-T6 (MVP done), P2-T1.

### P8-T2 — Play-mode track metadata and respawn
- `.parble` schema v2 adds a level block: start marble, collectible placements, goal, medal times. Migrations from v1.
- Checkpoints become respawn points in play mode. Falling off the world or stalling for more than N seconds respawns the marble at the last checkpoint.
- Verify: schema migration tests and a sim test of respawn.
- Depends on: P8-T1, P3-T3.

### P8-T3 — Collectibles and marble swapping
- Collectible marble pickups (sensor + spin animation) and an inventory.
- Swapping replaces the active marble in place with a collected type (radius, mass, physics). It is blocked when the new radius would intersect geometry.
- Size gates use the P7-T4 sieve and holes. Counterweight switches react to mass.
- Verify: sim tests show swap-to-mini passes a 12 mm gate and a shooter trips the counterweight switch.
- Depends on: P8-T2, P7-T4.

### P8-T4 — Timer, medals and ghost replay
- Run timer with checkpoint splits. Bronze/silver/gold thresholds from the level block.
- Best runs stored in IndexedDB as input streams. A ghost marble replays the best run.
- Verify: the ghost matches the recorded run's trajectory hash.
- Depends on: P8-T2.

### P8-T5 — Play HUD and test level
- HUD (timer, inventory, swap key, medals) via the `impeccable` skill.
- A test level that requires at least two swaps, and a scripted input replay that finishes it.
- Verify: all PRD P8 acceptance boxes.
- Depends on: P8-T3, P8-T4.

## Exit criteria
Every acceptance box under PRD §6 P8 checked with evidence.
