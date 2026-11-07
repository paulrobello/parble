# Phase 4: Audio

PRD: `PRD.md` §5.7, §6 P4. Phase goal: physics-reactive SFX and ambient music that make a run sound like real marbles on wood.

## Tasks

### P4-T1 — Cue sheet and Suno prompt pack
- `docs/audio/cue-sheet.md` lists every SFX:
  - roll loops per marble type (glass, solid, steel; later swirl/opal/glow share glass) × surface (block channel, rail trough, rug)
  - impact sets (light/medium/heavy × surface)
  - drop-into-hole, tunnel rumble, flap/toggle clack, splitter tick, one-way gate swing, trapdoor, hopper release, finish cup chime, checkpoint tick
  - UI clicks
- Each cue lists its duration, loop behaviour, variation count and loudness target.
- `docs/audio/suno-prompts.md` has 4–6 warm acoustic ambient cues (felt piano, marimba, soft strings) in the Suno guide's copy-ready Style / Exclude styles / Settings blocks, with no artist references.
- Verify: Paul approves the cue sheet and prompt pack.
- Depends on: none.

### P4-T2 — ElevenLabs SFX generation and processing
- Run `create-game-audio-assets` with the `elevenlabs` skill to generate every cue with variations.
- Process: trim, loudness-normalize, find loop points for roll loops, export OGG/Opus + AAC fallback.
- Keep originals unchanged under `assets-source/audio/`.
- `provenance.json` with prompts, model, date and generation count.
- Audition page served on 5310 at `/audition.html`, following the the-cube sfx audition page pattern.
- Verify: `validate-generated-assets` passes and Paul's audition picks are recorded.
- Depends on: P4-T1.

### P4-T3 — Rust audio event stream
- The bridge exports per frame:
  - contact events (marble type, surface id, impulse, world position)
  - per-marble roll samples (speed, surface, contact flag)
  - mechanism events
  - camera transform
- Coalesce contacts per marble per frame.
- Verify: unit tests on coalescing, and a bridge schema test shared with TS.
- Depends on: P2-T1, P0-T7.

### P4-T4 — Web Audio engine
- `web/audio` (TS):
  - roll loop voices per marble with pitch/gain/low-pass mapped from speed
  - surface crossfade
  - impact one-shots selected by impulse band with random variation and pitch jitter
  - PannerNode spatialization relative to the camera
  - voice cap and priority (nearest/loudest wins)
  - master/SFX/music buses
  - resume on first user gesture
- Uses placeholders until P4-T2 assets land.
- Verify: Vitest tests on the mapping functions and voice allocation. A Playwright run of the stress scenario records no audio errors.
- Depends on: P4-T3. Final assets come from P4-T2.

### P4-T5 — Music
- Paul generates and selects the Suno WAVs from the P4-T1 pack (owner action). Record song IDs, model and plan per the Suno guide.
- Prepare loop points and loudness. A crossfading playlist player sits on the music bus.
- Mixer settings UI.
- Verify: each loop plays seamlessly for 10 minutes in the browser, and Paul approves.
- Depends on: P4-T1, P4-T4.

### P4-T6 — Audio acceptance
- 10 marbles on the stress track: no clipping (peak meter), no voice starvation for the followed marble.
- Audio main-thread time stays inside the frame budget (perf trace).
- Paul does a listening review across marble types and surfaces.
- Verify: all PRD P4 acceptance boxes, evidence in `docs/audio/acceptance.md`.
- Depends on: P4-T2, P4-T4, P4-T5.

## Exit criteria
Every acceptance box under PRD §6 P4 checked with evidence.
