# Phase 9: Level pack

PRD: `PRD.md` §6 P9. Phase goal: a curated set of at least 12 steer-play levels with medals and saved progress.

## Tasks

### P9-T1 — Level authoring tools
- Editor play-mode layer: place collectibles, set start marble, set goal, record a reference run, and derive medal times from it (gold = reference + margin).
- Verify: Playwright authors a mini level and plays its recorded replay.
- Depends on: P8-T5.

### P9-T2 — Author levels 1–6 (onboarding arc)
- Teach nudge, hop, checkpoints, first swap, size gates and counterweight, in that order.
- Verify: each level ships with a scripted replay finishing under gold, run in a sim test.
- Depends on: P9-T1.

### P9-T3 — Author levels 7–12 (mastery arc)
- Tall vertical levels with helixes, drop towers, toggles and splitters as hazards, and multi-swap routes.
- Verify: same as P9-T2.
- Depends on: P9-T1.

### P9-T4 — Level select and progress
- Level select screen (`impeccable`), lock/unlock rules, best times and medals in IndexedDB.
- Verify: Vitest and a Playwright flow (complete level 1 → level 2 unlocks).
- Depends on: P9-T1.

### P9-T5 — Playtest and release
- Paul playtests. Tune medal thresholds (Open Question Q5). Run the release gate and deploy per P5-T5 (push approval required).
- Verify: all PRD P9 acceptance boxes, and the live render verified.
- Depends on: P9-T2, P9-T3, P9-T4.

## Exit criteria
Every acceptance box under PRD §6 P9 checked with evidence.
