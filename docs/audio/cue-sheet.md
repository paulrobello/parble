# parble SFX cue sheet

Status: P4-T1 draft for Paul's approval. Scope comes from `plans/phase-4-audio.md` P4-T1 and PRD §5.7. No audio has been generated. Generation, processing and provenance belong to P4-T2.

## Conventions

- **id** is stable snake_case and becomes the filename stem. Variations append `_v01`, `_v02`, and so on (for example `impact_glass_block_heavy_v03.ogg`). The P4-T4 engine selects cues by id, so ids do not change after approval.
- **family** is one of `roll`, `impact`, `drop_tunnel`, `mechanism`, `ui`. P4-T2 splits generation across agents by family.
- **Variations to generate** is the number of kept candidates per cue: at least 4 for one-shots and at least 3 for roll loops. Retries for rejected takes come on top (see the budget below).
- **Recording style.** Every prompt describes a close-mic'd real recording in a dry room with no music and no reverb tail. The finish cup chime is the only cue with an intended ring-out.
- **Marbles.** `glass` is clear glass. Swirl, onionskin, opal and glow (P5) reuse the glass cues. Minis and shooters reuse their material's cues with an engine pitch offset. The prompts say 16 mm as a real-world size reference. The physical radius comes from `docs/kit-contract.md`.
- **Solid marble material (decision for Paul).** The PRD calls it "solid glossy". An opaque glass marble sounds the same as a clear one, so the prompts describe a glazed ceramic marble to make the solid type audibly distinct, which the PRD P4 acceptance requires. Paul confirms or rejects this when approving the sheet.
- **Surfaces.** `block` is the carved hardwood block channel (all block pieces and mechanism bodies). `rail` is the round hardwood rail trough (rail family). `rug` is the floor rug for marbles that leave the track.
- **Impulse bands.** `light`, `medium` and `heavy` are chosen by the engine from contact impulse, with thresholds set in P4-T4. Each band keeps its natural level difference in the file, so the engine's per-hit gain only fine-tunes it.
- **Marble-on-marble pairs** are unordered: `clack_glass_steel` serves both glass-hits-steel and steel-hits-glass.

## ElevenLabs request settings

- Endpoint: `sfx` (text-to-sound-effects), which accepts 0.5–30 s. Every request duration below is inside that range.
- Loops: roll and tunnel cues use `--loop` with an explicit `--duration-seconds`. One-shots request slightly more than the target length and are trimmed. A loop request does not prove a seamless loop, so run `check-audio-loops.py` on every loop.
- Prompt influence: start at 0.6. Raise it toward 0.8 if takes drift into music or ambience.
- Source format: request `pcm_44100` when the account plan allows it, otherwise `mp3_44100_128`. Archive the returned bytes unchanged under `assets-source/audio/sfx/<family>/`.

## Output format and loudness

- Runtime: 48 kHz **mono** (every SFX is spatialized by a PannerNode), OGG/Opus at 96 kb/s plus an AAC (`.m4a`) fallback. Keep a 48 kHz 24-bit WAV intermediate.
- Roll and tunnel loops are measured as integrated loudness over the whole loop.
- One-shots under 1 s are too short for a meaningful whole-file integrated LUFS, so their LUFS target applies to the trimmed active region and the **true-peak ceiling is the primary gate**. Trim leading silence to under 5 ms so impacts land on the contact frame.
- Remeasure loudness and true peak after lossy encoding.

## Summary

| Family | Cues | Variations each | Generations kept |
| --- | ---: | ---: | ---: |
| `roll` | 9 | 3 | 27 |
| `impact` | 45 | 4 | 180 |
| `drop_tunnel` | 4 | 3/4 | 15 |
| `mechanism` | 7 | 4 | 28 |
| `ui` | 5 | 4 | 20 |
| **Total** | **70** | | **270** |

Generation estimate: 270 kept candidates. Allow a 25% retry margin for takes that drift into music, carry reverb or fail the loop check, which gives about 338 requests. Confirm the account billing mode and an approved request budget before P4-T2 starts, per the `elevenlabs` skill.

### All cues

| id | family | loop | duration | variations | loudness | true peak |
| --- | --- | --- | --- | ---: | --- | --- |
| `roll_glass_block` | roll | loop | 8 s request (`--duration-seconds 8 --loop`), runtime loop 6–8 s | 3 | -24 LUFS integrated ±1 | ≤ -6 dBTP |
| `roll_glass_rail` | roll | loop | 8 s request (`--duration-seconds 8 --loop`), runtime loop 6–8 s | 3 | -24 LUFS integrated ±1 | ≤ -6 dBTP |
| `roll_glass_rug` | roll | loop | 8 s request (`--duration-seconds 8 --loop`), runtime loop 6–8 s | 3 | -24 LUFS integrated ±1 | ≤ -6 dBTP |
| `roll_solid_block` | roll | loop | 8 s request (`--duration-seconds 8 --loop`), runtime loop 6–8 s | 3 | -24 LUFS integrated ±1 | ≤ -6 dBTP |
| `roll_solid_rail` | roll | loop | 8 s request (`--duration-seconds 8 --loop`), runtime loop 6–8 s | 3 | -24 LUFS integrated ±1 | ≤ -6 dBTP |
| `roll_solid_rug` | roll | loop | 8 s request (`--duration-seconds 8 --loop`), runtime loop 6–8 s | 3 | -24 LUFS integrated ±1 | ≤ -6 dBTP |
| `roll_steel_block` | roll | loop | 8 s request (`--duration-seconds 8 --loop`), runtime loop 6–8 s | 3 | -24 LUFS integrated ±1 | ≤ -6 dBTP |
| `roll_steel_rail` | roll | loop | 8 s request (`--duration-seconds 8 --loop`), runtime loop 6–8 s | 3 | -24 LUFS integrated ±1 | ≤ -6 dBTP |
| `roll_steel_rug` | roll | loop | 8 s request (`--duration-seconds 8 --loop`), runtime loop 6–8 s | 3 | -24 LUFS integrated ±1 | ≤ -6 dBTP |
| `impact_glass_block_light` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -30 LUFS (active region) ±2 | ≤ -12 dBTP |
| `impact_glass_block_medium` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -24 LUFS (active region) ±2 | ≤ -6 dBTP |
| `impact_glass_block_heavy` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -18 LUFS (active region) ±2 | ≤ -1 dBTP |
| `impact_glass_rail_light` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -30 LUFS (active region) ±2 | ≤ -12 dBTP |
| `impact_glass_rail_medium` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -24 LUFS (active region) ±2 | ≤ -6 dBTP |
| `impact_glass_rail_heavy` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -18 LUFS (active region) ±2 | ≤ -1 dBTP |
| `impact_glass_rug_light` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -30 LUFS (active region) ±2 | ≤ -12 dBTP |
| `impact_glass_rug_medium` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -24 LUFS (active region) ±2 | ≤ -6 dBTP |
| `impact_glass_rug_heavy` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -18 LUFS (active region) ±2 | ≤ -1 dBTP |
| `impact_solid_block_light` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -30 LUFS (active region) ±2 | ≤ -12 dBTP |
| `impact_solid_block_medium` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -24 LUFS (active region) ±2 | ≤ -6 dBTP |
| `impact_solid_block_heavy` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -18 LUFS (active region) ±2 | ≤ -1 dBTP |
| `impact_solid_rail_light` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -30 LUFS (active region) ±2 | ≤ -12 dBTP |
| `impact_solid_rail_medium` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -24 LUFS (active region) ±2 | ≤ -6 dBTP |
| `impact_solid_rail_heavy` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -18 LUFS (active region) ±2 | ≤ -1 dBTP |
| `impact_solid_rug_light` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -30 LUFS (active region) ±2 | ≤ -12 dBTP |
| `impact_solid_rug_medium` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -24 LUFS (active region) ±2 | ≤ -6 dBTP |
| `impact_solid_rug_heavy` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -18 LUFS (active region) ±2 | ≤ -1 dBTP |
| `impact_steel_block_light` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -30 LUFS (active region) ±2 | ≤ -12 dBTP |
| `impact_steel_block_medium` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -24 LUFS (active region) ±2 | ≤ -6 dBTP |
| `impact_steel_block_heavy` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -18 LUFS (active region) ±2 | ≤ -1 dBTP |
| `impact_steel_rail_light` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -30 LUFS (active region) ±2 | ≤ -12 dBTP |
| `impact_steel_rail_medium` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -24 LUFS (active region) ±2 | ≤ -6 dBTP |
| `impact_steel_rail_heavy` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -18 LUFS (active region) ±2 | ≤ -1 dBTP |
| `impact_steel_rug_light` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -30 LUFS (active region) ±2 | ≤ -12 dBTP |
| `impact_steel_rug_medium` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -24 LUFS (active region) ±2 | ≤ -6 dBTP |
| `impact_steel_rug_heavy` | impact | one-shot | 1 s request, trimmed to 150–600 ms | 4 | -18 LUFS (active region) ±2 | ≤ -1 dBTP |
| `clack_glass_glass_light` | impact | one-shot | 1 s request, trimmed to 100–400 ms | 4 | -30 LUFS (active region) ±2 | ≤ -12 dBTP |
| `clack_glass_glass_medium` | impact | one-shot | 1 s request, trimmed to 100–400 ms | 4 | -24 LUFS (active region) ±2 | ≤ -6 dBTP |
| `clack_glass_glass_heavy` | impact | one-shot | 1 s request, trimmed to 100–400 ms | 4 | -18 LUFS (active region) ±2 | ≤ -1 dBTP |
| `clack_glass_solid_light` | impact | one-shot | 1 s request, trimmed to 100–400 ms | 4 | -30 LUFS (active region) ±2 | ≤ -12 dBTP |
| `clack_glass_solid_medium` | impact | one-shot | 1 s request, trimmed to 100–400 ms | 4 | -24 LUFS (active region) ±2 | ≤ -6 dBTP |
| `clack_glass_solid_heavy` | impact | one-shot | 1 s request, trimmed to 100–400 ms | 4 | -18 LUFS (active region) ±2 | ≤ -1 dBTP |
| `clack_glass_steel_light` | impact | one-shot | 1 s request, trimmed to 100–400 ms | 4 | -30 LUFS (active region) ±2 | ≤ -12 dBTP |
| `clack_glass_steel_medium` | impact | one-shot | 1 s request, trimmed to 100–400 ms | 4 | -24 LUFS (active region) ±2 | ≤ -6 dBTP |
| `clack_glass_steel_heavy` | impact | one-shot | 1 s request, trimmed to 100–400 ms | 4 | -18 LUFS (active region) ±2 | ≤ -1 dBTP |
| `clack_solid_solid_light` | impact | one-shot | 1 s request, trimmed to 100–400 ms | 4 | -30 LUFS (active region) ±2 | ≤ -12 dBTP |
| `clack_solid_solid_medium` | impact | one-shot | 1 s request, trimmed to 100–400 ms | 4 | -24 LUFS (active region) ±2 | ≤ -6 dBTP |
| `clack_solid_solid_heavy` | impact | one-shot | 1 s request, trimmed to 100–400 ms | 4 | -18 LUFS (active region) ±2 | ≤ -1 dBTP |
| `clack_solid_steel_light` | impact | one-shot | 1 s request, trimmed to 100–400 ms | 4 | -30 LUFS (active region) ±2 | ≤ -12 dBTP |
| `clack_solid_steel_medium` | impact | one-shot | 1 s request, trimmed to 100–400 ms | 4 | -24 LUFS (active region) ±2 | ≤ -6 dBTP |
| `clack_solid_steel_heavy` | impact | one-shot | 1 s request, trimmed to 100–400 ms | 4 | -18 LUFS (active region) ±2 | ≤ -1 dBTP |
| `clack_steel_steel_light` | impact | one-shot | 1 s request, trimmed to 100–400 ms | 4 | -30 LUFS (active region) ±2 | ≤ -12 dBTP |
| `clack_steel_steel_medium` | impact | one-shot | 1 s request, trimmed to 100–400 ms | 4 | -24 LUFS (active region) ±2 | ≤ -6 dBTP |
| `clack_steel_steel_heavy` | impact | one-shot | 1 s request, trimmed to 100–400 ms | 4 | -18 LUFS (active region) ±2 | ≤ -1 dBTP |
| `drop_hole_glass` | drop_tunnel | one-shot | 1.5 s request, trimmed to 0.4–1.0 s | 4 | -22 LUFS (active region) ±2 | ≤ -3 dBTP |
| `drop_hole_solid` | drop_tunnel | one-shot | 1.5 s request, trimmed to 0.4–1.0 s | 4 | -22 LUFS (active region) ±2 | ≤ -3 dBTP |
| `drop_hole_steel` | drop_tunnel | one-shot | 1.5 s request, trimmed to 0.4–1.0 s | 4 | -22 LUFS (active region) ±2 | ≤ -3 dBTP |
| `tunnel_rumble` | drop_tunnel | loop | 6 s request (`--duration-seconds 6 --loop`), runtime loop 4–6 s | 3 | -26 LUFS integrated ±1 | ≤ -6 dBTP |
| `toggle_flap_clack` | mechanism | one-shot | 1 s request, trimmed to 150–400 ms | 4 | -22 LUFS (active region) ±2 | ≤ -3 dBTP |
| `splitter_tick` | mechanism | one-shot | 1 s request, trimmed to 80–250 ms | 4 | -26 LUFS (active region) ±2 | ≤ -6 dBTP |
| `one_way_gate_swing` | mechanism | one-shot | 1.5 s request, trimmed to 0.4–1.0 s | 4 | -24 LUFS (active region) ±2 | ≤ -6 dBTP |
| `trapdoor_drop` | mechanism | one-shot | 1.5 s request, trimmed to 0.4–1.0 s | 4 | -22 LUFS (active region) ±2 | ≤ -3 dBTP |
| `hopper_release` | mechanism | one-shot | 2 s request, trimmed to 0.5–1.5 s | 4 | -22 LUFS (active region) ±2 | ≤ -3 dBTP |
| `finish_cup_chime` | mechanism | one-shot | 2.5 s request, trimmed to 1.0–2.0 s (the bell ring-out is intended) | 4 | -20 LUFS (active region) ±2 | ≤ -3 dBTP |
| `checkpoint_tick` | mechanism | one-shot | 1 s request, trimmed to 80–250 ms | 4 | -26 LUFS (active region) ±2 | ≤ -6 dBTP |
| `ui_click` | ui | one-shot | 0.5 s request (API minimum), trimmed to 40–200 ms | 4 | -24 LUFS (active region) ±2 | ≤ -6 dBTP |
| `ui_toggle` | ui | one-shot | 0.5 s request (API minimum), trimmed to 40–200 ms | 4 | -24 LUFS (active region) ±2 | ≤ -6 dBTP |
| `ui_piece_place` | ui | one-shot | 0.5 s request (API minimum), trimmed to 40–200 ms | 4 | -24 LUFS (active region) ±2 | ≤ -6 dBTP |
| `ui_piece_remove` | ui | one-shot | 0.5 s request (API minimum), trimmed to 40–200 ms | 4 | -24 LUFS (active region) ±2 | ≤ -6 dBTP |
| `ui_piece_rotate` | ui | one-shot | 0.5 s request (API minimum), trimmed to 40–200 ms | 4 | -24 LUFS (active region) ±2 | ≤ -6 dBTP |

## Family `roll`: roll loops (3 marble types × 3 surfaces)

### `roll_glass_block`

- family: `roll`
- trigger: roll sample: glass marble in contact with the carved hardwood block channel; pitch, gain and low-pass follow speed
- description: Continuous roll of the glass marble on the carved hardwood block channel, recorded at a constant medium speed so the engine can pitch it up and down.
- duration: 8 s request (`--duration-seconds 8 --loop`), runtime loop 6–8 s
- loop: seamless loop (ElevenLabs `--loop`, then loop-point check)
- variations to generate: 3
- loudness: -24 LUFS integrated ±1, true peak ≤ -6 dBTP
- output: `roll_glass_block_vNN.ogg` (Opus, 48 kHz mono) and `roll_glass_block_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A 16 mm clear glass marble rolling steadily at constant medium speed along a smooth U-shaped channel carved into a solid maple toy block, steady rolling rumble with the faint grain texture of the wood, bright, glassy, slightly ringing character, continuous with no start or stop, close mic, dry room, real recording, no music, no reverb"

### `roll_glass_rail`

- family: `roll`
- trigger: roll sample: glass marble in contact with the round hardwood rail trough; pitch, gain and low-pass follow speed
- description: Continuous roll of the glass marble on the round hardwood rail trough, recorded at a constant medium speed so the engine can pitch it up and down.
- duration: 8 s request (`--duration-seconds 8 --loop`), runtime loop 6–8 s
- loop: seamless loop (ElevenLabs `--loop`, then loop-point check)
- variations to generate: 3
- loudness: -24 LUFS integrated ±1, true peak ≤ -6 dBTP
- output: `roll_glass_rail_vNN.ogg` (Opus, 48 kHz mono) and `roll_glass_rail_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A 16 mm clear glass marble rolling steadily at constant medium speed along two parallel smooth round beech dowel rails forming an open trough, light hollow rolling whirr with soft rhythmic ticking from the two rail contacts, bright, glassy, slightly ringing character, continuous with no start or stop, close mic, dry room, real recording, no music, no reverb"

### `roll_glass_rug`

- family: `roll`
- trigger: roll sample: glass marble in contact with the woven rug; pitch, gain and low-pass follow speed
- description: Continuous roll of the glass marble on the woven rug, recorded at a constant medium speed so the engine can pitch it up and down.
- duration: 8 s request (`--duration-seconds 8 --loop`), runtime loop 6–8 s
- loop: seamless loop (ElevenLabs `--loop`, then loop-point check)
- variations to generate: 3
- loudness: -24 LUFS integrated ±1, true peak ≤ -6 dBTP
- output: `roll_glass_rug_vNN.ogg` (Opus, 48 kHz mono) and `roll_glass_rug_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A 16 mm clear glass marble rolling steadily at constant medium speed along a low-pile woven wool rug laid over a wooden floor, soft muffled rolling rustle over woven fibres, very little resonance, bright, glassy, slightly ringing character, continuous with no start or stop, close mic, dry room, real recording, no music, no reverb"

### `roll_solid_block`

- family: `roll`
- trigger: roll sample: solid marble in contact with the carved hardwood block channel; pitch, gain and low-pass follow speed
- description: Continuous roll of the solid marble on the carved hardwood block channel, recorded at a constant medium speed so the engine can pitch it up and down.
- duration: 8 s request (`--duration-seconds 8 --loop`), runtime loop 6–8 s
- loop: seamless loop (ElevenLabs `--loop`, then loop-point check)
- variations to generate: 3
- loudness: -24 LUFS integrated ±1, true peak ≤ -6 dBTP
- output: `roll_solid_block_vNN.ogg` (Opus, 48 kHz mono) and `roll_solid_block_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A 16 mm opaque solid-colour glazed ceramic marble rolling steadily at constant medium speed along a smooth U-shaped channel carved into a solid maple toy block, steady rolling rumble with the faint grain texture of the wood, duller, denser, less ring than glass character, continuous with no start or stop, close mic, dry room, real recording, no music, no reverb"

### `roll_solid_rail`

- family: `roll`
- trigger: roll sample: solid marble in contact with the round hardwood rail trough; pitch, gain and low-pass follow speed
- description: Continuous roll of the solid marble on the round hardwood rail trough, recorded at a constant medium speed so the engine can pitch it up and down.
- duration: 8 s request (`--duration-seconds 8 --loop`), runtime loop 6–8 s
- loop: seamless loop (ElevenLabs `--loop`, then loop-point check)
- variations to generate: 3
- loudness: -24 LUFS integrated ±1, true peak ≤ -6 dBTP
- output: `roll_solid_rail_vNN.ogg` (Opus, 48 kHz mono) and `roll_solid_rail_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A 16 mm opaque solid-colour glazed ceramic marble rolling steadily at constant medium speed along two parallel smooth round beech dowel rails forming an open trough, light hollow rolling whirr with soft rhythmic ticking from the two rail contacts, duller, denser, less ring than glass character, continuous with no start or stop, close mic, dry room, real recording, no music, no reverb"

### `roll_solid_rug`

- family: `roll`
- trigger: roll sample: solid marble in contact with the woven rug; pitch, gain and low-pass follow speed
- description: Continuous roll of the solid marble on the woven rug, recorded at a constant medium speed so the engine can pitch it up and down.
- duration: 8 s request (`--duration-seconds 8 --loop`), runtime loop 6–8 s
- loop: seamless loop (ElevenLabs `--loop`, then loop-point check)
- variations to generate: 3
- loudness: -24 LUFS integrated ±1, true peak ≤ -6 dBTP
- output: `roll_solid_rug_vNN.ogg` (Opus, 48 kHz mono) and `roll_solid_rug_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A 16 mm opaque solid-colour glazed ceramic marble rolling steadily at constant medium speed along a low-pile woven wool rug laid over a wooden floor, soft muffled rolling rustle over woven fibres, very little resonance, duller, denser, less ring than glass character, continuous with no start or stop, close mic, dry room, real recording, no music, no reverb"

### `roll_steel_block`

- family: `roll`
- trigger: roll sample: steel marble in contact with the carved hardwood block channel; pitch, gain and low-pass follow speed
- description: Continuous roll of the steel marble on the carved hardwood block channel, recorded at a constant medium speed so the engine can pitch it up and down.
- duration: 8 s request (`--duration-seconds 8 --loop`), runtime loop 6–8 s
- loop: seamless loop (ElevenLabs `--loop`, then loop-point check)
- variations to generate: 3
- loudness: -24 LUFS integrated ±1, true peak ≤ -6 dBTP
- output: `roll_steel_block_vNN.ogg` (Opus, 48 kHz mono) and `roll_steel_block_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A 16 mm polished chrome steel ball bearing rolling steadily at constant medium speed along a smooth U-shaped channel carved into a solid maple toy block, steady rolling rumble with the faint grain texture of the wood, heavy, metallic, hard and sharp character, continuous with no start or stop, close mic, dry room, real recording, no music, no reverb"

### `roll_steel_rail`

- family: `roll`
- trigger: roll sample: steel marble in contact with the round hardwood rail trough; pitch, gain and low-pass follow speed
- description: Continuous roll of the steel marble on the round hardwood rail trough, recorded at a constant medium speed so the engine can pitch it up and down.
- duration: 8 s request (`--duration-seconds 8 --loop`), runtime loop 6–8 s
- loop: seamless loop (ElevenLabs `--loop`, then loop-point check)
- variations to generate: 3
- loudness: -24 LUFS integrated ±1, true peak ≤ -6 dBTP
- output: `roll_steel_rail_vNN.ogg` (Opus, 48 kHz mono) and `roll_steel_rail_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A 16 mm polished chrome steel ball bearing rolling steadily at constant medium speed along two parallel smooth round beech dowel rails forming an open trough, light hollow rolling whirr with soft rhythmic ticking from the two rail contacts, heavy, metallic, hard and sharp character, continuous with no start or stop, close mic, dry room, real recording, no music, no reverb"

### `roll_steel_rug`

- family: `roll`
- trigger: roll sample: steel marble in contact with the woven rug; pitch, gain and low-pass follow speed
- description: Continuous roll of the steel marble on the woven rug, recorded at a constant medium speed so the engine can pitch it up and down.
- duration: 8 s request (`--duration-seconds 8 --loop`), runtime loop 6–8 s
- loop: seamless loop (ElevenLabs `--loop`, then loop-point check)
- variations to generate: 3
- loudness: -24 LUFS integrated ±1, true peak ≤ -6 dBTP
- output: `roll_steel_rug_vNN.ogg` (Opus, 48 kHz mono) and `roll_steel_rug_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A 16 mm polished chrome steel ball bearing rolling steadily at constant medium speed along a low-pile woven wool rug laid over a wooden floor, soft muffled rolling rustle over woven fibres, very little resonance, heavy, metallic, hard and sharp character, continuous with no start or stop, close mic, dry room, real recording, no music, no reverb"

## Family `impact`: impacts (marble × surface × band) and marble-on-marble clacks (6 pairings × band)

### `impact_glass_block_light`

- family: `impact`
- trigger: contact event: glass marble against the carved hardwood block channel, impulse band light
- description: Single light impact of the glass marble on the carved hardwood block channel.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -30 LUFS (active region) ±2, true peak ≤ -12 dBTP
- output: `impact_glass_block_light_vNN.ogg` (Opus, 48 kHz mono) and `impact_glass_block_light_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A light glancing tap of a 16 mm clear glass marble onto the bare hardwood of a solid maple toy block, one single hit, short woody knock with a quick natural decay, bright, glassy, slightly ringing, close mic, dry room, real recording, no music, no reverb"

### `impact_glass_block_medium`

- family: `impact`
- trigger: contact event: glass marble against the carved hardwood block channel, impulse band medium
- description: Single medium impact of the glass marble on the carved hardwood block channel.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `impact_glass_block_medium_vNN.ogg` (Opus, 48 kHz mono) and `impact_glass_block_medium_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A firm single knock of a 16 mm clear glass marble onto the bare hardwood of a solid maple toy block, one single hit, short woody knock with a quick natural decay, bright, glassy, slightly ringing, close mic, dry room, real recording, no music, no reverb"

### `impact_glass_block_heavy`

- family: `impact`
- trigger: contact event: glass marble against the carved hardwood block channel, impulse band heavy
- description: Single heavy impact of the glass marble on the carved hardwood block channel.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -18 LUFS (active region) ±2, true peak ≤ -1 dBTP
- output: `impact_glass_block_heavy_vNN.ogg` (Opus, 48 kHz mono) and `impact_glass_block_heavy_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A hard drop from about 30 cm of a 16 mm clear glass marble onto the bare hardwood of a solid maple toy block, one single hit, short woody knock with a quick natural decay, bright, glassy, slightly ringing, close mic, dry room, real recording, no music, no reverb"

### `impact_glass_rail_light`

- family: `impact`
- trigger: contact event: glass marble against the round hardwood rail trough, impulse band light
- description: Single light impact of the glass marble on the round hardwood rail trough.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -30 LUFS (active region) ±2, true peak ≤ -12 dBTP
- output: `impact_glass_rail_light_vNN.ogg` (Opus, 48 kHz mono) and `impact_glass_rail_light_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A light glancing tap of a 16 mm clear glass marble onto a smooth round beech dowel rail, one single hit, short woody knock with a quick natural decay, bright, glassy, slightly ringing, close mic, dry room, real recording, no music, no reverb"

### `impact_glass_rail_medium`

- family: `impact`
- trigger: contact event: glass marble against the round hardwood rail trough, impulse band medium
- description: Single medium impact of the glass marble on the round hardwood rail trough.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `impact_glass_rail_medium_vNN.ogg` (Opus, 48 kHz mono) and `impact_glass_rail_medium_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A firm single knock of a 16 mm clear glass marble onto a smooth round beech dowel rail, one single hit, short woody knock with a quick natural decay, bright, glassy, slightly ringing, close mic, dry room, real recording, no music, no reverb"

### `impact_glass_rail_heavy`

- family: `impact`
- trigger: contact event: glass marble against the round hardwood rail trough, impulse band heavy
- description: Single heavy impact of the glass marble on the round hardwood rail trough.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -18 LUFS (active region) ±2, true peak ≤ -1 dBTP
- output: `impact_glass_rail_heavy_vNN.ogg` (Opus, 48 kHz mono) and `impact_glass_rail_heavy_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A hard drop from about 30 cm of a 16 mm clear glass marble onto a smooth round beech dowel rail, one single hit, short woody knock with a quick natural decay, bright, glassy, slightly ringing, close mic, dry room, real recording, no music, no reverb"

### `impact_glass_rug_light`

- family: `impact`
- trigger: contact event: glass marble against the woven rug, impulse band light
- description: Single light impact of the glass marble on the woven rug.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -30 LUFS (active region) ±2, true peak ≤ -12 dBTP
- output: `impact_glass_rug_light_vNN.ogg` (Opus, 48 kHz mono) and `impact_glass_rug_light_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A light glancing tap of a 16 mm clear glass marble onto a low-pile woven wool rug over a wooden floor, one single hit, short muffled thud, almost no ring, bright, glassy, slightly ringing, close mic, dry room, real recording, no music, no reverb"

### `impact_glass_rug_medium`

- family: `impact`
- trigger: contact event: glass marble against the woven rug, impulse band medium
- description: Single medium impact of the glass marble on the woven rug.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `impact_glass_rug_medium_vNN.ogg` (Opus, 48 kHz mono) and `impact_glass_rug_medium_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A firm single knock of a 16 mm clear glass marble onto a low-pile woven wool rug over a wooden floor, one single hit, short muffled thud, almost no ring, bright, glassy, slightly ringing, close mic, dry room, real recording, no music, no reverb"

### `impact_glass_rug_heavy`

- family: `impact`
- trigger: contact event: glass marble against the woven rug, impulse band heavy
- description: Single heavy impact of the glass marble on the woven rug.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -18 LUFS (active region) ±2, true peak ≤ -1 dBTP
- output: `impact_glass_rug_heavy_vNN.ogg` (Opus, 48 kHz mono) and `impact_glass_rug_heavy_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A hard drop from about 30 cm of a 16 mm clear glass marble onto a low-pile woven wool rug over a wooden floor, one single hit, short muffled thud, almost no ring, bright, glassy, slightly ringing, close mic, dry room, real recording, no music, no reverb"

### `impact_solid_block_light`

- family: `impact`
- trigger: contact event: solid marble against the carved hardwood block channel, impulse band light
- description: Single light impact of the solid marble on the carved hardwood block channel.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -30 LUFS (active region) ±2, true peak ≤ -12 dBTP
- output: `impact_solid_block_light_vNN.ogg` (Opus, 48 kHz mono) and `impact_solid_block_light_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A light glancing tap of a 16 mm opaque solid-colour glazed ceramic marble onto the bare hardwood of a solid maple toy block, one single hit, short woody knock with a quick natural decay, duller, denser, less ring than glass, close mic, dry room, real recording, no music, no reverb"

### `impact_solid_block_medium`

- family: `impact`
- trigger: contact event: solid marble against the carved hardwood block channel, impulse band medium
- description: Single medium impact of the solid marble on the carved hardwood block channel.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `impact_solid_block_medium_vNN.ogg` (Opus, 48 kHz mono) and `impact_solid_block_medium_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A firm single knock of a 16 mm opaque solid-colour glazed ceramic marble onto the bare hardwood of a solid maple toy block, one single hit, short woody knock with a quick natural decay, duller, denser, less ring than glass, close mic, dry room, real recording, no music, no reverb"

### `impact_solid_block_heavy`

- family: `impact`
- trigger: contact event: solid marble against the carved hardwood block channel, impulse band heavy
- description: Single heavy impact of the solid marble on the carved hardwood block channel.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -18 LUFS (active region) ±2, true peak ≤ -1 dBTP
- output: `impact_solid_block_heavy_vNN.ogg` (Opus, 48 kHz mono) and `impact_solid_block_heavy_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A hard drop from about 30 cm of a 16 mm opaque solid-colour glazed ceramic marble onto the bare hardwood of a solid maple toy block, one single hit, short woody knock with a quick natural decay, duller, denser, less ring than glass, close mic, dry room, real recording, no music, no reverb"

### `impact_solid_rail_light`

- family: `impact`
- trigger: contact event: solid marble against the round hardwood rail trough, impulse band light
- description: Single light impact of the solid marble on the round hardwood rail trough.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -30 LUFS (active region) ±2, true peak ≤ -12 dBTP
- output: `impact_solid_rail_light_vNN.ogg` (Opus, 48 kHz mono) and `impact_solid_rail_light_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A light glancing tap of a 16 mm opaque solid-colour glazed ceramic marble onto a smooth round beech dowel rail, one single hit, short woody knock with a quick natural decay, duller, denser, less ring than glass, close mic, dry room, real recording, no music, no reverb"

### `impact_solid_rail_medium`

- family: `impact`
- trigger: contact event: solid marble against the round hardwood rail trough, impulse band medium
- description: Single medium impact of the solid marble on the round hardwood rail trough.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `impact_solid_rail_medium_vNN.ogg` (Opus, 48 kHz mono) and `impact_solid_rail_medium_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A firm single knock of a 16 mm opaque solid-colour glazed ceramic marble onto a smooth round beech dowel rail, one single hit, short woody knock with a quick natural decay, duller, denser, less ring than glass, close mic, dry room, real recording, no music, no reverb"

### `impact_solid_rail_heavy`

- family: `impact`
- trigger: contact event: solid marble against the round hardwood rail trough, impulse band heavy
- description: Single heavy impact of the solid marble on the round hardwood rail trough.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -18 LUFS (active region) ±2, true peak ≤ -1 dBTP
- output: `impact_solid_rail_heavy_vNN.ogg` (Opus, 48 kHz mono) and `impact_solid_rail_heavy_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A hard drop from about 30 cm of a 16 mm opaque solid-colour glazed ceramic marble onto a smooth round beech dowel rail, one single hit, short woody knock with a quick natural decay, duller, denser, less ring than glass, close mic, dry room, real recording, no music, no reverb"

### `impact_solid_rug_light`

- family: `impact`
- trigger: contact event: solid marble against the woven rug, impulse band light
- description: Single light impact of the solid marble on the woven rug.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -30 LUFS (active region) ±2, true peak ≤ -12 dBTP
- output: `impact_solid_rug_light_vNN.ogg` (Opus, 48 kHz mono) and `impact_solid_rug_light_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A light glancing tap of a 16 mm opaque solid-colour glazed ceramic marble onto a low-pile woven wool rug over a wooden floor, one single hit, short muffled thud, almost no ring, duller, denser, less ring than glass, close mic, dry room, real recording, no music, no reverb"

### `impact_solid_rug_medium`

- family: `impact`
- trigger: contact event: solid marble against the woven rug, impulse band medium
- description: Single medium impact of the solid marble on the woven rug.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `impact_solid_rug_medium_vNN.ogg` (Opus, 48 kHz mono) and `impact_solid_rug_medium_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A firm single knock of a 16 mm opaque solid-colour glazed ceramic marble onto a low-pile woven wool rug over a wooden floor, one single hit, short muffled thud, almost no ring, duller, denser, less ring than glass, close mic, dry room, real recording, no music, no reverb"

### `impact_solid_rug_heavy`

- family: `impact`
- trigger: contact event: solid marble against the woven rug, impulse band heavy
- description: Single heavy impact of the solid marble on the woven rug.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -18 LUFS (active region) ±2, true peak ≤ -1 dBTP
- output: `impact_solid_rug_heavy_vNN.ogg` (Opus, 48 kHz mono) and `impact_solid_rug_heavy_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A hard drop from about 30 cm of a 16 mm opaque solid-colour glazed ceramic marble onto a low-pile woven wool rug over a wooden floor, one single hit, short muffled thud, almost no ring, duller, denser, less ring than glass, close mic, dry room, real recording, no music, no reverb"

### `impact_steel_block_light`

- family: `impact`
- trigger: contact event: steel marble against the carved hardwood block channel, impulse band light
- description: Single light impact of the steel marble on the carved hardwood block channel.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -30 LUFS (active region) ±2, true peak ≤ -12 dBTP
- output: `impact_steel_block_light_vNN.ogg` (Opus, 48 kHz mono) and `impact_steel_block_light_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A light glancing tap of a 16 mm polished chrome steel ball bearing onto the bare hardwood of a solid maple toy block, one single hit, short woody knock with a quick natural decay, heavy, metallic, hard and sharp, close mic, dry room, real recording, no music, no reverb"

### `impact_steel_block_medium`

- family: `impact`
- trigger: contact event: steel marble against the carved hardwood block channel, impulse band medium
- description: Single medium impact of the steel marble on the carved hardwood block channel.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `impact_steel_block_medium_vNN.ogg` (Opus, 48 kHz mono) and `impact_steel_block_medium_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A firm single knock of a 16 mm polished chrome steel ball bearing onto the bare hardwood of a solid maple toy block, one single hit, short woody knock with a quick natural decay, heavy, metallic, hard and sharp, close mic, dry room, real recording, no music, no reverb"

### `impact_steel_block_heavy`

- family: `impact`
- trigger: contact event: steel marble against the carved hardwood block channel, impulse band heavy
- description: Single heavy impact of the steel marble on the carved hardwood block channel.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -18 LUFS (active region) ±2, true peak ≤ -1 dBTP
- output: `impact_steel_block_heavy_vNN.ogg` (Opus, 48 kHz mono) and `impact_steel_block_heavy_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A hard drop from about 30 cm of a 16 mm polished chrome steel ball bearing onto the bare hardwood of a solid maple toy block, one single hit, short woody knock with a quick natural decay, heavy, metallic, hard and sharp, close mic, dry room, real recording, no music, no reverb"

### `impact_steel_rail_light`

- family: `impact`
- trigger: contact event: steel marble against the round hardwood rail trough, impulse band light
- description: Single light impact of the steel marble on the round hardwood rail trough.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -30 LUFS (active region) ±2, true peak ≤ -12 dBTP
- output: `impact_steel_rail_light_vNN.ogg` (Opus, 48 kHz mono) and `impact_steel_rail_light_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A light glancing tap of a 16 mm polished chrome steel ball bearing onto a smooth round beech dowel rail, one single hit, short woody knock with a quick natural decay, heavy, metallic, hard and sharp, close mic, dry room, real recording, no music, no reverb"

### `impact_steel_rail_medium`

- family: `impact`
- trigger: contact event: steel marble against the round hardwood rail trough, impulse band medium
- description: Single medium impact of the steel marble on the round hardwood rail trough.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `impact_steel_rail_medium_vNN.ogg` (Opus, 48 kHz mono) and `impact_steel_rail_medium_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A firm single knock of a 16 mm polished chrome steel ball bearing onto a smooth round beech dowel rail, one single hit, short woody knock with a quick natural decay, heavy, metallic, hard and sharp, close mic, dry room, real recording, no music, no reverb"

### `impact_steel_rail_heavy`

- family: `impact`
- trigger: contact event: steel marble against the round hardwood rail trough, impulse band heavy
- description: Single heavy impact of the steel marble on the round hardwood rail trough.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -18 LUFS (active region) ±2, true peak ≤ -1 dBTP
- output: `impact_steel_rail_heavy_vNN.ogg` (Opus, 48 kHz mono) and `impact_steel_rail_heavy_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A hard drop from about 30 cm of a 16 mm polished chrome steel ball bearing onto a smooth round beech dowel rail, one single hit, short woody knock with a quick natural decay, heavy, metallic, hard and sharp, close mic, dry room, real recording, no music, no reverb"

### `impact_steel_rug_light`

- family: `impact`
- trigger: contact event: steel marble against the woven rug, impulse band light
- description: Single light impact of the steel marble on the woven rug.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -30 LUFS (active region) ±2, true peak ≤ -12 dBTP
- output: `impact_steel_rug_light_vNN.ogg` (Opus, 48 kHz mono) and `impact_steel_rug_light_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A light glancing tap of a 16 mm polished chrome steel ball bearing onto a low-pile woven wool rug over a wooden floor, one single hit, short muffled thud, almost no ring, heavy, metallic, hard and sharp, close mic, dry room, real recording, no music, no reverb"

### `impact_steel_rug_medium`

- family: `impact`
- trigger: contact event: steel marble against the woven rug, impulse band medium
- description: Single medium impact of the steel marble on the woven rug.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `impact_steel_rug_medium_vNN.ogg` (Opus, 48 kHz mono) and `impact_steel_rug_medium_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A firm single knock of a 16 mm polished chrome steel ball bearing onto a low-pile woven wool rug over a wooden floor, one single hit, short muffled thud, almost no ring, heavy, metallic, hard and sharp, close mic, dry room, real recording, no music, no reverb"

### `impact_steel_rug_heavy`

- family: `impact`
- trigger: contact event: steel marble against the woven rug, impulse band heavy
- description: Single heavy impact of the steel marble on the woven rug.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -18 LUFS (active region) ±2, true peak ≤ -1 dBTP
- output: `impact_steel_rug_heavy_vNN.ogg` (Opus, 48 kHz mono) and `impact_steel_rug_heavy_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A hard drop from about 30 cm of a 16 mm polished chrome steel ball bearing onto a low-pile woven wool rug over a wooden floor, one single hit, short muffled thud, almost no ring, heavy, metallic, hard and sharp, close mic, dry room, real recording, no music, no reverb"

### `clack_glass_glass_light`

- family: `impact`
- trigger: contact event: marble against marble, glass vs glass (either order), impulse band light
- description: Marble-on-marble collision, glass against glass, light band.
- duration: 1 s request, trimmed to 100–400 ms
- loop: one-shot
- variations to generate: 4
- loudness: -30 LUFS (active region) ±2, true peak ≤ -12 dBTP
- output: `clack_glass_glass_light_vNN.ogg` (Opus, 48 kHz mono) and `clack_glass_glass_light_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A light tick of a 16 mm clear glass marble striking another 16 mm clear glass marble, one single click with no rolling afterwards, bright meeting bright, close mic, dry room, real recording, no music, no reverb"

### `clack_glass_glass_medium`

- family: `impact`
- trigger: contact event: marble against marble, glass vs glass (either order), impulse band medium
- description: Marble-on-marble collision, glass against glass, medium band.
- duration: 1 s request, trimmed to 100–400 ms
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `clack_glass_glass_medium_vNN.ogg` (Opus, 48 kHz mono) and `clack_glass_glass_medium_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A crisp clack of a 16 mm clear glass marble striking another 16 mm clear glass marble, one single click with no rolling afterwards, bright meeting bright, close mic, dry room, real recording, no music, no reverb"

### `clack_glass_glass_heavy`

- family: `impact`
- trigger: contact event: marble against marble, glass vs glass (either order), impulse band heavy
- description: Marble-on-marble collision, glass against glass, heavy band.
- duration: 1 s request, trimmed to 100–400 ms
- loop: one-shot
- variations to generate: 4
- loudness: -18 LUFS (active region) ±2, true peak ≤ -1 dBTP
- output: `clack_glass_glass_heavy_vNN.ogg` (Opus, 48 kHz mono) and `clack_glass_glass_heavy_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A hard sharp collision of a 16 mm clear glass marble striking another 16 mm clear glass marble, one single click with no rolling afterwards, bright meeting bright, close mic, dry room, real recording, no music, no reverb"

### `clack_glass_solid_light`

- family: `impact`
- trigger: contact event: marble against marble, glass vs solid (either order), impulse band light
- description: Marble-on-marble collision, glass against solid, light band.
- duration: 1 s request, trimmed to 100–400 ms
- loop: one-shot
- variations to generate: 4
- loudness: -30 LUFS (active region) ±2, true peak ≤ -12 dBTP
- output: `clack_glass_solid_light_vNN.ogg` (Opus, 48 kHz mono) and `clack_glass_solid_light_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A light tick of a 16 mm clear glass marble striking a 16 mm opaque solid-colour glazed ceramic marble, one single click with no rolling afterwards, bright meeting duller, close mic, dry room, real recording, no music, no reverb"

### `clack_glass_solid_medium`

- family: `impact`
- trigger: contact event: marble against marble, glass vs solid (either order), impulse band medium
- description: Marble-on-marble collision, glass against solid, medium band.
- duration: 1 s request, trimmed to 100–400 ms
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `clack_glass_solid_medium_vNN.ogg` (Opus, 48 kHz mono) and `clack_glass_solid_medium_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A crisp clack of a 16 mm clear glass marble striking a 16 mm opaque solid-colour glazed ceramic marble, one single click with no rolling afterwards, bright meeting duller, close mic, dry room, real recording, no music, no reverb"

### `clack_glass_solid_heavy`

- family: `impact`
- trigger: contact event: marble against marble, glass vs solid (either order), impulse band heavy
- description: Marble-on-marble collision, glass against solid, heavy band.
- duration: 1 s request, trimmed to 100–400 ms
- loop: one-shot
- variations to generate: 4
- loudness: -18 LUFS (active region) ±2, true peak ≤ -1 dBTP
- output: `clack_glass_solid_heavy_vNN.ogg` (Opus, 48 kHz mono) and `clack_glass_solid_heavy_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A hard sharp collision of a 16 mm clear glass marble striking a 16 mm opaque solid-colour glazed ceramic marble, one single click with no rolling afterwards, bright meeting duller, close mic, dry room, real recording, no music, no reverb"

### `clack_glass_steel_light`

- family: `impact`
- trigger: contact event: marble against marble, glass vs steel (either order), impulse band light
- description: Marble-on-marble collision, glass against steel, light band.
- duration: 1 s request, trimmed to 100–400 ms
- loop: one-shot
- variations to generate: 4
- loudness: -30 LUFS (active region) ±2, true peak ≤ -12 dBTP
- output: `clack_glass_steel_light_vNN.ogg` (Opus, 48 kHz mono) and `clack_glass_steel_light_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A light tick of a 16 mm clear glass marble striking a 16 mm polished chrome steel ball bearing, one single click with no rolling afterwards, bright meeting heavy, close mic, dry room, real recording, no music, no reverb"

### `clack_glass_steel_medium`

- family: `impact`
- trigger: contact event: marble against marble, glass vs steel (either order), impulse band medium
- description: Marble-on-marble collision, glass against steel, medium band.
- duration: 1 s request, trimmed to 100–400 ms
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `clack_glass_steel_medium_vNN.ogg` (Opus, 48 kHz mono) and `clack_glass_steel_medium_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A crisp clack of a 16 mm clear glass marble striking a 16 mm polished chrome steel ball bearing, one single click with no rolling afterwards, bright meeting heavy, close mic, dry room, real recording, no music, no reverb"

### `clack_glass_steel_heavy`

- family: `impact`
- trigger: contact event: marble against marble, glass vs steel (either order), impulse band heavy
- description: Marble-on-marble collision, glass against steel, heavy band.
- duration: 1 s request, trimmed to 100–400 ms
- loop: one-shot
- variations to generate: 4
- loudness: -18 LUFS (active region) ±2, true peak ≤ -1 dBTP
- output: `clack_glass_steel_heavy_vNN.ogg` (Opus, 48 kHz mono) and `clack_glass_steel_heavy_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A hard sharp collision of a 16 mm clear glass marble striking a 16 mm polished chrome steel ball bearing, one single click with no rolling afterwards, bright meeting heavy, close mic, dry room, real recording, no music, no reverb"

### `clack_solid_solid_light`

- family: `impact`
- trigger: contact event: marble against marble, solid vs solid (either order), impulse band light
- description: Marble-on-marble collision, solid against solid, light band.
- duration: 1 s request, trimmed to 100–400 ms
- loop: one-shot
- variations to generate: 4
- loudness: -30 LUFS (active region) ±2, true peak ≤ -12 dBTP
- output: `clack_solid_solid_light_vNN.ogg` (Opus, 48 kHz mono) and `clack_solid_solid_light_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A light tick of a 16 mm opaque solid-colour glazed ceramic marble striking another 16 mm opaque solid-colour glazed ceramic marble, one single click with no rolling afterwards, duller meeting duller, close mic, dry room, real recording, no music, no reverb"

### `clack_solid_solid_medium`

- family: `impact`
- trigger: contact event: marble against marble, solid vs solid (either order), impulse band medium
- description: Marble-on-marble collision, solid against solid, medium band.
- duration: 1 s request, trimmed to 100–400 ms
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `clack_solid_solid_medium_vNN.ogg` (Opus, 48 kHz mono) and `clack_solid_solid_medium_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A crisp clack of a 16 mm opaque solid-colour glazed ceramic marble striking another 16 mm opaque solid-colour glazed ceramic marble, one single click with no rolling afterwards, duller meeting duller, close mic, dry room, real recording, no music, no reverb"

### `clack_solid_solid_heavy`

- family: `impact`
- trigger: contact event: marble against marble, solid vs solid (either order), impulse band heavy
- description: Marble-on-marble collision, solid against solid, heavy band.
- duration: 1 s request, trimmed to 100–400 ms
- loop: one-shot
- variations to generate: 4
- loudness: -18 LUFS (active region) ±2, true peak ≤ -1 dBTP
- output: `clack_solid_solid_heavy_vNN.ogg` (Opus, 48 kHz mono) and `clack_solid_solid_heavy_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A hard sharp collision of a 16 mm opaque solid-colour glazed ceramic marble striking another 16 mm opaque solid-colour glazed ceramic marble, one single click with no rolling afterwards, duller meeting duller, close mic, dry room, real recording, no music, no reverb"

### `clack_solid_steel_light`

- family: `impact`
- trigger: contact event: marble against marble, solid vs steel (either order), impulse band light
- description: Marble-on-marble collision, solid against steel, light band.
- duration: 1 s request, trimmed to 100–400 ms
- loop: one-shot
- variations to generate: 4
- loudness: -30 LUFS (active region) ±2, true peak ≤ -12 dBTP
- output: `clack_solid_steel_light_vNN.ogg` (Opus, 48 kHz mono) and `clack_solid_steel_light_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A light tick of a 16 mm opaque solid-colour glazed ceramic marble striking a 16 mm polished chrome steel ball bearing, one single click with no rolling afterwards, duller meeting heavy, close mic, dry room, real recording, no music, no reverb"

### `clack_solid_steel_medium`

- family: `impact`
- trigger: contact event: marble against marble, solid vs steel (either order), impulse band medium
- description: Marble-on-marble collision, solid against steel, medium band.
- duration: 1 s request, trimmed to 100–400 ms
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `clack_solid_steel_medium_vNN.ogg` (Opus, 48 kHz mono) and `clack_solid_steel_medium_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A crisp clack of a 16 mm opaque solid-colour glazed ceramic marble striking a 16 mm polished chrome steel ball bearing, one single click with no rolling afterwards, duller meeting heavy, close mic, dry room, real recording, no music, no reverb"

### `clack_solid_steel_heavy`

- family: `impact`
- trigger: contact event: marble against marble, solid vs steel (either order), impulse band heavy
- description: Marble-on-marble collision, solid against steel, heavy band.
- duration: 1 s request, trimmed to 100–400 ms
- loop: one-shot
- variations to generate: 4
- loudness: -18 LUFS (active region) ±2, true peak ≤ -1 dBTP
- output: `clack_solid_steel_heavy_vNN.ogg` (Opus, 48 kHz mono) and `clack_solid_steel_heavy_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A hard sharp collision of a 16 mm opaque solid-colour glazed ceramic marble striking a 16 mm polished chrome steel ball bearing, one single click with no rolling afterwards, duller meeting heavy, close mic, dry room, real recording, no music, no reverb"

### `clack_steel_steel_light`

- family: `impact`
- trigger: contact event: marble against marble, steel vs steel (either order), impulse band light
- description: Marble-on-marble collision, steel against steel, light band.
- duration: 1 s request, trimmed to 100–400 ms
- loop: one-shot
- variations to generate: 4
- loudness: -30 LUFS (active region) ±2, true peak ≤ -12 dBTP
- output: `clack_steel_steel_light_vNN.ogg` (Opus, 48 kHz mono) and `clack_steel_steel_light_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A light tick of a 16 mm polished chrome steel ball bearing striking another 16 mm polished chrome steel ball bearing, one single click with no rolling afterwards, heavy meeting heavy, close mic, dry room, real recording, no music, no reverb"

### `clack_steel_steel_medium`

- family: `impact`
- trigger: contact event: marble against marble, steel vs steel (either order), impulse band medium
- description: Marble-on-marble collision, steel against steel, medium band.
- duration: 1 s request, trimmed to 100–400 ms
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `clack_steel_steel_medium_vNN.ogg` (Opus, 48 kHz mono) and `clack_steel_steel_medium_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A crisp clack of a 16 mm polished chrome steel ball bearing striking another 16 mm polished chrome steel ball bearing, one single click with no rolling afterwards, heavy meeting heavy, close mic, dry room, real recording, no music, no reverb"

### `clack_steel_steel_heavy`

- family: `impact`
- trigger: contact event: marble against marble, steel vs steel (either order), impulse band heavy
- description: Marble-on-marble collision, steel against steel, heavy band.
- duration: 1 s request, trimmed to 100–400 ms
- loop: one-shot
- variations to generate: 4
- loudness: -18 LUFS (active region) ±2, true peak ≤ -1 dBTP
- output: `clack_steel_steel_heavy_vNN.ogg` (Opus, 48 kHz mono) and `clack_steel_steel_heavy_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A hard sharp collision of a 16 mm polished chrome steel ball bearing striking another 16 mm polished chrome steel ball bearing, one single click with no rolling afterwards, heavy meeting heavy, close mic, dry room, real recording, no music, no reverb"

## Family `drop_tunnel`: drop into hole and tunnel rumble

### `drop_hole_glass`

- family: `drop_tunnel`
- trigger: drop hole or redirect drop entry: glass marble falls through a vertical port
- description: The glass marble drops into a round hole in a wooden block, falls a short way and lands inside on wood.
- duration: 1.5 s request, trimmed to 0.4–1.0 s
- loop: one-shot
- variations to generate: 4
- loudness: -22 LUFS (active region) ±2, true peak ≤ -3 dBTP
- output: `drop_hole_glass_vNN.ogg` (Opus, 48 kHz mono) and `drop_hole_glass_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A 16 mm clear glass marble rolling off an edge and dropping into a round hole bored in a solid maple block, a brief hollow knock as it falls inside and one landing tap on wood below, bright, glassy, slightly ringing, close mic, dry room, real recording, no music, no reverb"

### `drop_hole_solid`

- family: `drop_tunnel`
- trigger: drop hole or redirect drop entry: solid marble falls through a vertical port
- description: The solid marble drops into a round hole in a wooden block, falls a short way and lands inside on wood.
- duration: 1.5 s request, trimmed to 0.4–1.0 s
- loop: one-shot
- variations to generate: 4
- loudness: -22 LUFS (active region) ±2, true peak ≤ -3 dBTP
- output: `drop_hole_solid_vNN.ogg` (Opus, 48 kHz mono) and `drop_hole_solid_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A 16 mm opaque solid-colour glazed ceramic marble rolling off an edge and dropping into a round hole bored in a solid maple block, a brief hollow knock as it falls inside and one landing tap on wood below, duller, denser, less ring than glass, close mic, dry room, real recording, no music, no reverb"

### `drop_hole_steel`

- family: `drop_tunnel`
- trigger: drop hole or redirect drop entry: steel marble falls through a vertical port
- description: The steel marble drops into a round hole in a wooden block, falls a short way and lands inside on wood.
- duration: 1.5 s request, trimmed to 0.4–1.0 s
- loop: one-shot
- variations to generate: 4
- loudness: -22 LUFS (active region) ±2, true peak ≤ -3 dBTP
- output: `drop_hole_steel_vNN.ogg` (Opus, 48 kHz mono) and `drop_hole_steel_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A 16 mm polished chrome steel ball bearing rolling off an edge and dropping into a round hole bored in a solid maple block, a brief hollow knock as it falls inside and one landing tap on wood below, heavy, metallic, hard and sharp, close mic, dry room, real recording, no music, no reverb"

### `tunnel_rumble`

- family: `drop_tunnel`
- trigger: roll sample: any marble inside the redirect-drop internal tunnel, layered over its roll voice
- description: Hollow enclosed rumble of a marble rolling through a short tunnel bored inside a wooden block.
- duration: 6 s request (`--duration-seconds 6 --loop`), runtime loop 4–6 s
- loop: seamless loop (ElevenLabs `--loop`, then loop-point check)
- variations to generate: 3
- loudness: -26 LUFS integrated ±1, true peak ≤ -6 dBTP
- output: `tunnel_rumble_vNN.ogg` (Opus, 48 kHz mono) and `tunnel_rumble_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A small glass marble rolling through a short enclosed tunnel bored inside a solid hardwood block, hollow woody rumble with a boxy low resonance, continuous with no start or stop, close mic, dry room, real recording, no music, no reverb"

## Family `mechanism`: mechanisms

### `toggle_flap_clack`

- family: `mechanism`
- trigger: L/R toggle (flip-flop) flap switches sides
- description: A small wooden flap on a pivot pin snaps from one side to the other.
- duration: 1 s request, trimmed to 150–400 ms
- loop: one-shot
- variations to generate: 4
- loudness: -22 LUFS (active region) ±2, true peak ≤ -3 dBTP
- output: `toggle_flap_clack_vNN.ogg` (Opus, 48 kHz mono) and `toggle_flap_clack_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A small hardwood flap on a wooden pivot pin flipping over and knocking against its wooden stop, one sharp double clack, toy mechanism, close mic, dry room, real recording, no music, no reverb"

### `splitter_tick`

- family: `mechanism`
- trigger: random splitter vane settles to its seeded side
- description: A light wooden vane tick as the splitter commits left or right.
- duration: 1 s request, trimmed to 80–250 ms
- loop: one-shot
- variations to generate: 4
- loudness: -26 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `splitter_tick_vNN.ogg` (Opus, 48 kHz mono) and `splitter_tick_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A light wooden vane inside a toy marble splitter ticking once against its stop, small dry wooden tick, close mic, dry room, real recording, no music, no reverb"

### `one_way_gate_swing`

- family: `mechanism`
- trigger: one-way gate: hinged arch flap lifts as a marble passes and falls back
- description: A hinged wooden arch flap swings up and drops back with a soft knock.
- duration: 1.5 s request, trimmed to 0.4–1.0 s
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `one_way_gate_swing_vNN.ogg` (Opus, 48 kHz mono) and `one_way_gate_swing_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A small hinged wooden flap on a toy marble track pushed up by a rolling marble, a faint creak of the wooden hinge and a soft knock as it falls back closed, close mic, dry room, real recording, no music, no reverb"

### `trapdoor_drop`

- family: `mechanism`
- trigger: one-way hole: trapdoor tips open under a marble and snaps shut
- description: A wooden trapdoor tips under the marble, then springs closed.
- duration: 1.5 s request, trimmed to 0.4–1.0 s
- loop: one-shot
- variations to generate: 4
- loudness: -22 LUFS (active region) ±2, true peak ≤ -3 dBTP
- output: `trapdoor_drop_vNN.ogg` (Opus, 48 kHz mono) and `trapdoor_drop_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A small wooden trapdoor in a toy marble track tipping open under a marble and snapping shut again, a quick wooden clack then a firmer closing knock, close mic, dry room, real recording, no music, no reverb"

### `hopper_release`

- family: `mechanism`
- trigger: start hopper releases one marble
- description: The wooden hopper gate slides open, a marble rolls out and the gate clicks shut.
- duration: 2 s request, trimmed to 0.5–1.5 s
- loop: one-shot
- variations to generate: 4
- loudness: -22 LUFS (active region) ±2, true peak ≤ -3 dBTP
- output: `hopper_release_vNN.ogg` (Opus, 48 kHz mono) and `hopper_release_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A wooden toy hopper gate sliding open with a short wooden scrape, a glass marble rolling out, and the gate clicking shut, small toy mechanism, close mic, dry room, real recording, no music, no reverb"

### `finish_cup_chime`

- family: `mechanism`
- trigger: finish collector: marble arrives in the cup
- description: The marble lands in the finish cup and strikes a small bright bell.
- duration: 2.5 s request, trimmed to 1.0–2.0 s (the bell ring-out is intended)
- loop: one-shot
- variations to generate: 4
- loudness: -20 LUFS (active region) ±2, true peak ≤ -3 dBTP
- output: `finish_cup_chime_vNN.ogg` (Opus, 48 kHz mono) and `finish_cup_chime_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A glass marble dropping into a small wooden cup and striking a tiny brass bell inside, one gentle bright ding with a natural short ring-out, close mic, dry room, real recording, no music"

### `checkpoint_tick`

- family: `mechanism`
- trigger: checkpoint arch sensor pass
- description: A crisp wooden ratchet tick as a marble passes under the arch.
- duration: 1 s request, trimmed to 80–250 ms
- loop: one-shot
- variations to generate: 4
- loudness: -26 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `checkpoint_tick_vNN.ogg` (Opus, 48 kHz mono) and `checkpoint_tick_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A single crisp wooden ratchet click from a small wooden toy counter, one clean dry tick, close mic, dry room, real recording, no music, no reverb"

## Family `ui`: UI clicks

### `ui_click`

- family: `ui`
- trigger: generic button press
- description: A soft wooden button click.
- duration: 0.5 s request (API minimum), trimmed to 40–200 ms
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `ui_click_vNN.ogg` (Opus, 48 kHz mono) and `ui_click_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "Single soft click of a small wooden push button, short and tactile, close mic, dry room, real recording, no music, no reverb"

### `ui_toggle`

- family: `ui`
- trigger: toggle, checkbox or tab change
- description: A two-part wooden switch click.
- duration: 0.5 s request (API minimum), trimmed to 40–200 ms
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `ui_toggle_vNN.ogg` (Opus, 48 kHz mono) and `ui_toggle_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A small wooden toggle switch flicked once, a short two-part tactile click, close mic, dry room, real recording, no music, no reverb"

### `ui_piece_place`

- family: `ui`
- trigger: editor: piece snaps into the grid
- description: A wooden block set firmly down onto another block.
- duration: 0.5 s request (API minimum), trimmed to 40–200 ms
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `ui_piece_place_vNN.ogg` (Opus, 48 kHz mono) and `ui_piece_place_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A small wooden toy block set down and seating firmly onto another wooden block, one short solid wooden knock, close mic, dry room, real recording, no music, no reverb"

### `ui_piece_remove`

- family: `ui`
- trigger: editor: piece removed
- description: A wooden block lifted off with a light scrape.
- duration: 0.5 s request (API minimum), trimmed to 40–200 ms
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `ui_piece_remove_vNN.ogg` (Opus, 48 kHz mono) and `ui_piece_remove_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A small wooden toy block lifted off another block with a light short wooden scrape and tap, close mic, dry room, real recording, no music, no reverb"

### `ui_piece_rotate`

- family: `ui`
- trigger: editor: piece rotated 90 degrees
- description: A short double wooden click.
- duration: 0.5 s request (API minimum), trimmed to 40–200 ms
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `ui_piece_rotate_vNN.ogg` (Opus, 48 kHz mono) and `ui_piece_rotate_vNN.m4a` (AAC fallback)
- ElevenLabs prompt: "A small wooden block turned a quarter turn on a wooden peg, two quick light wooden clicks, close mic, dry room, real recording, no music, no reverb"
