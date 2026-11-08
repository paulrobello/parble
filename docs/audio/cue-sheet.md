# parble SFX cue sheet

Status: P4-T1 draft for Paul's approval. Scope comes from `plans/phase-4-audio.md` P4-T1 and PRD §5.7. Generation, processing and provenance belong to P4-T2; the generated takes, their audition verdicts and prompt revisions 2 and 3 are recorded below and in `assets-source/audio/sfx/provenance-group-*.json`.

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

## Prompt revision 3 (2026-11-07)

Paul auditioned the revision 2 rug takes later on 2026-11-07 and rejected all of them (`impact_{glass,solid,steel}_rug_{light,medium,heavy}` v05–v08 and `roll_{glass,solid,steel}_rug` v04–v06): "the rug samples still sound like rolling on wood, it should sound like felt". Those 45 rows now carry `"status": "rejected"` in `provenance-group-d.json`. The `one_way_gate_swing` v05–v08 takes are not judged yet and are unchanged.

Diagnosis. The revision 2 prompts led with the action ("a small hard ball rolling", "a drop of a small hard ball"), and the model seems to render that action from a hard-surface recording whatever the surface words say. The measurements agree: the revision 2 rolls were already dark (centroid about 300–350 Hz, almost nothing above 2 kHz), but they had a crest factor near 24 dB and 7.5–10.6 click onsets per second, which is the rhythmic ticking of a ball on a hard floor. A ball on thick felt is close to silent: a low, dull brushing with no clicks, and a landing is a small soft "pff" with no attack.

Revision 3 describes the sound instead of the action and drops the words that point at wood ("rolling", "track", "floor", "tap", "knock", "drop", "ball"). Prompt influence is 0.7 (revision 2 used 0.6), so the model follows the felt description more literally. Two routes are on the audition board side by side:

- **Route A, generated** (impacts v09–v12, rolls v07–v09). Rolls: "very soft muffled brushing hush of thick wool felt being gently rubbed, a continuous steady low and dull fabric whisper, almost silent, no clicks, no hard surface, no wood, no ring, no music", with "very lightly rubbed" for glass and "gently rubbed with a little more weight, slightly deeper" for steel. Impacts: "one single very soft dull muffled thud of a small weight settling into a thick wool felt cushion, a quiet low pff with a soft rounded onset, no click, no hard surface, no wood, no ring, no bounce, no music, no reverb", with "light weight" or "heavy weight" for glass and steel, and "barely touching down" or "landing a little firmer but still soft" for the light and heavy bands. The exact prompt of every take is in its cue below and in `provenance-group-e.json`.
- **Route B, derived** (stem suffix `f`, for example `roll_glass_rug_v07f`). Each route A take run through `scripts/audio/felt.py derive`, a deterministic felt treatment: a transient limiter that holds the fast peak envelope to 3 dB over the 20 ms RMS level, a 12 ms raised-cosine attack smear on one-shots, an 8th-order (48 dB/octave) Butterworth low-pass at 1.5 kHz, a +3 dB low shelf at 250 Hz, then peak normalization to -3 dBFS. Loops are filtered circularly (the last 0.5 s runs through first and is discarded), so the wrap stays continuous. The output is 24-bit 48 kHz mono FLAC under `assets-source/audio/sfx/<family>/`, and each provenance row records the parent file and hash plus the full recipe. `scripts/audio/test_felt.py` covers it.

Prompt selection. Before the full set, each idea got two test takes on `roll_solid_rug` or `impact_solid_rug_medium` (14 requests). The roll candidates were A (above), B ("felt pad sliding over a felt blanket ... breathy fabric hush"), C ("like breath through thick wool cloth") and D (A with "deep, low ... murmur, no hiss, no airy noise"). B had a 25 dB crest factor. C had the fewest clicks (1.4–1.8 per second) but both takes were bright hiss (centroid 1.5–2.0 kHz). D was the darkest but had 5.5–7.9 clicks per second. A had 4.6–5.9 clicks per second and a 13–15 dB crest factor, with one dark take (266 Hz) and one bright one (2.0 kHz), so A went ahead. The impact candidates were A (the "tiny soft muffled pff ... on a thick felt pad" prompt), B (above) and C ("a small cloth pouch landing on a thick felt blanket ... puff of air and fabric"). C had the softest attack but read as broadband air noise (centroid up to 1.9 kHz, 30 % above 2 kHz). B was the darkest (centroid 305 Hz on take 1) and had the fewest clicks, so B went ahead. The prompt-test takes of the chosen ideas are kept as `roll_solid_rug_v07`/`v08` and `impact_solid_rug_medium_v09`/`v10`. The other 10 are listed under `discarded` in `provenance-group-e.json` and are not committed.

Medians per cue, measured on the source files with `scripts/audio/felt.py measure`. One-shots are measured over their active region (process.py's onset and tail rule), loops over the whole file. Crest is peak over RMS. Attack is the time from onset to the loudest 2 ms window, so longer means a softer landing. Clicks per second counts 5 ms windows of the first difference that stand 8 dB over the median of the previous 200 ms, counted in the band under 1.5 kHz for every file so that a bright noise floor cannot hide them. Share above 2 kHz and the centroid use energy above 150 Hz. Every cell is round 2 → round 3 generated → round 3 derived.

| cue | crest (dB) | attack (ms) | clicks per second | share above 2 kHz (%) | centroid (Hz) |
| --- | --- | --- | --- | --- | --- |
| `impact_glass_rug_heavy` | 24.45 → 22.34 → 20.73 | 9 → 17 → 18.5 | 6.58 → 4.85 → 2.45 | 0.2 → 0.97 → 0 | 434.3 → 360.2 → 245.8 |
| `impact_glass_rug_light` | 23.25 → 17.7 → 17.58 | 15 → 10.5 → 19 | 3.55 → 1.1 → 1.6 | 1.78 → 32.75 → 0.01 | 793.35 → 2867.65 → 354.65 |
| `impact_glass_rug_medium` | 28.84 → 23.78 → 21.54 | 19.5 → 270.5 → 203 | 10.34 → 3.12 → 3.98 | 0.28 → 1.97 → 0 | 778.7 → 762.35 → 250.85 |
| `impact_solid_rug_heavy` | 20.73 → 25.55 → 21.75 | 15.5 → 11 → 15.5 | 2.5 → 3.36 → 2.48 | 4.22 → 4.48 → 0.01 | 650.2 → 724.1 → 286.7 |
| `impact_solid_rug_light` | 25.34 → 23.52 → 22.85 | 13.5 → 13 → 20 | 2.48 → 1.39 → 1.65 | 4.83 → 1.66 → 0 | 1139.4 → 464 → 257.2 |
| `impact_solid_rug_medium` | 18.16 → 25.43 → 21.75 | 12 → 23 → 210.5 | 0.5 → 2.8 → 3.21 | 12.33 → 0.93 → 0 | 993.05 → 677.15 → 442.35 |
| `impact_steel_rug_heavy` | 19.16 → 22.15 → 18.52 | 26 → 180.5 → 364.5 | 0.62 → 4.92 → 5.26 | 2.4 → 0.09 → 0 | 448.35 → 299.55 → 253.35 |
| `impact_steel_rug_light` | 20.66 → 20.46 → 18.72 | 82 → 13.5 → 16.5 | 6.28 → 2.05 → 2 | 0.11 → 0.69 → 0 | 370.8 → 300.25 → 265.65 |
| `impact_steel_rug_medium` | 19.81 → 21.07 → 20.52 | 14.5 → 14 → 16 | 5.39 → 0.98 → 0.96 | 0.4 → 13.73 → 0.01 | 481.8 → 1296.5 → 303.8 |
| `roll_glass_rug` | 24.02 → 15.03 → 14.27 | n/a (loop) | 7.5 → 4.75 → 5.62 | 0.38 → 5.06 → 0 | 350.7 → 551.5 → 251.7 |
| `roll_solid_rug` | 24.55 → 14.85 → 14.8 | n/a (loop) | 10.62 → 4.75 → 6.25 | 0.14 → 27.69 → 0.02 | 323.9 → 2036.8 → 319.2 |
| `roll_steel_rug` | 23.74 → 16.03 → 16.77 | n/a (loop) | 8.5 → 8 → 8.12 | 0.28 → 5.21 → 0 | 301.4 → 562.2 → 304.9 |

What the numbers show:

- **Rolls.** Route A cut the median crest factor from 23.7–24.6 dB to 14.9–16.0 dB and the click rate from 7.5–10.6 to 4.8–8.0 per second, so the rolls lost most of their hard-surface ticking. The new prompts let some high-frequency hiss back in (centroid 552–2037 Hz, 5–28 % above 2 kHz). Route B removes that hiss (0 % above 2 kHz, centroid 252–319 Hz) and keeps the lower crest factor. Its click count does not fall below route A's: the click detector reads the envelope ripple of narrow-band low noise as onsets, so on these dark files that number is less trustworthy than the crest factor.
- **Impacts.** Route A is mixed. It is darker than round 2 in 6 of 9 cues, has fewer clicks in 6 of 9 and a lower crest factor in 5 of 9. Several takes still came back bright (`impact_glass_rug_light` 2.9 kHz centroid, `impact_steel_rug_medium` 1.3 kHz). Route B is the darkest in every cue (centroid 246–442 Hz, 0 % above 2 kHz) and has the lowest crest factor of the three in 6 of 9.
- None of these numbers can show whether a take sounds like felt, so Paul judges both routes on the audition board.

Loudness and loops after `make audio-process`:

- **Loudness.** Most rug takes cannot reach their LUFS target under the true-peak ceiling. This also held in round 2, where 7 of 36 impacts and 1 of 9 rolls were in target. In round 3, 4 of 36 route A impacts and 6 of 36 route B impacts are in target, with median shortfalls of 5.1 LU and 3.7 LU. For the rolls, 0 of 9 route A takes and 1 of 9 route B takes are in target (median shortfalls 6.1 LU and 5.5 LU). Felt is quiet and low-frequency, and K-weighted loudness discounts the low band while the true peak does not, so linear gain stops at the ceiling first. The pipeline flags these rows ("true-peak ceiling bound the gain") and does not limit or compress them. Getting to target would need compression, which is a decision for Paul.
- **Loops.** 7 of 9 route A rolls pass the loop check at 6–8 s. `roll_glass_rug_v09` fails the check on the Opus encode, and `roll_steel_rug_v07` is only 4.5 s because an interior dropout limits the clean region. 5 of 9 route B rolls pass. `roll_glass_rug_v07f` and `roll_solid_rug_v07f` fail on the Opus seam although their parents pass, `roll_glass_rug_v09f` fails like its parent, and `roll_steel_rug_v07f` is 4.5 s like its parent. The passing loops are `roll_glass_rug_v07`, `v08`, `v08f`, `roll_solid_rug_v07`, `v08`, `v08f`, `v09`, `v09f` and `roll_steel_rug_v08`, `v08f`, `v09`, `v09f`. These counts use the Opus runtime file. The AAC fallback also fails the check on `roll_glass_rug_v08`, `v08f`, `v09` and `roll_solid_rug_v08f` (AAC priming and padding). No round 3 one-shot has leading silence over 5 ms or a duration outside its range.

## Prompt revision 2 (2026-11-07)

Paul auditioned the P4-T2 takes on the audition board on 2026-11-07 and rejected three groups:

- All 12 rug cues (`impact_{glass,solid,steel}_rug_{light,medium,heavy}`, `roll_{glass,solid,steel}_rug`): "all the rug sounds are too glass / metallic sounding, carpet should sound like carpet not the marble material". Each revision 1 rug prompt named the marble material and appended its timbre descriptor ("bright, glassy, slightly ringing", "heavy, metallic, hard and sharp", "duller, denser"), and the model rendered that as the marble ringing.
- `one_way_gate_swing`: "sounds like the marble not the gate". The revision 1 prompt led with "pushed up by a rolling marble".
- `drop_hole_glass_v02` only. Its prompt is unchanged and v01, v03 and v04 stand.

Revision 2 changes only the prompts of those 13 cues. Durations, variation counts and loudness targets are unchanged.

- **Rug.** The carpet is the subject. Impacts describe a soft, dull, padded thump of a small hard ball on a thick wool carpet. Rolls describe the soft fibrous hush and rustle of the carpet pile. Every rug prompt ends with explicit negatives: no ring, no click, no glassy or metallic tone, no wood knock. The marble is "a small light / dense / heavy hard ball", so material only shades weight (lighter for glass, heavier and deeper for steel). It never names glass, ceramic or steel, and "over a wooden floor" is dropped because it invited a wood knock.
- **Gate.** The wooden flap mechanism is the subject. The hinge creaks, the flap swings and then taps shut against its wooden frame in one continuous motion, with no marble rolling and no glass. The first test take used two separate events and came back as two taps 950 ms apart with no audible creak, so the prompt now asks for one continuous motion within half a second.

The replacements are new versions (impacts v05–v08, rolls v04–v06, gate v05–v08), recorded in `assets-source/audio/sfx/provenance-group-d.json`. The rejected originals stay on disk for provenance and carry `"status": "rejected"` in their provenance rows. `scripts/audio/process.py` skips those rows and lists them under the manifest's `rejected` key, so the audition page shows only the new candidates. Approved picks are in `docs/audio/audition-picks.json`.

Medians of rejected originals and revision 2 takes, measured on the raw provider files. Centroid and the share above 4 kHz use energy above 150 Hz. Decay is the time from the loudest 5 ms window until the envelope stays 20 dB below it. Tonal peak is the largest spectral line above the local median between 50 Hz and 8 kHz, where a high value means a ringing partial.

Each line is centroid (Hz) · share above 4 kHz · 20 dB decay (ms) · tonal peak (dB), old → new. Loops have no decay.

- `impact_glass_rug_light`: 866.5 → 1000 · 0.05 % → 0.05 % · 427.5 → 100 · 36.45 → 10.55
- `impact_glass_rug_medium`: 572.5 → 897 · 0.55 % → 0 % · 765 → 102.5 · 18.75 → 8.95
- `impact_glass_rug_heavy`: 3907 → 521.5 · 49 % → 0 % · 212.5 → 155 · 21.65 → 13.65
- `impact_solid_rug_light`: 1186.5 → 765 · 0.05 % → 0 % · 185 → 80 · 32.2 → 17.5
- `impact_solid_rug_medium`: 273 → 345 · 0.2 % → 0.95 % · 427.5 → 180 · 22.3 → 15.8
- `impact_solid_rug_heavy`: 3456 → 664 · 22.85 % → 0 % · 235 → 250 · 17.9 → 16.95
- `impact_steel_rug_light`: 705 → 340.5 · 0 % → 0.1 % · 167.5 → 122.5 · 22.05 → 14
- `impact_steel_rug_medium`: 336 → 320 · 0 % → 0.05 % · 352.5 → 92.5 · 19.3 → 13.3
- `impact_steel_rug_heavy`: 492 → 295.5 · 0 % → 0 % · 142.5 → 150 · 15.5 → 21.1
- `roll_glass_rug`: 3398 → 353 · 37.4 % → 0 % · decay n/a (loop) · 16.3 → 5.7
- `roll_solid_rug`: 1220 → 327 · 0.4 % → 0 % · decay n/a (loop) · 8.9 → 7.6
- `roll_steel_rug`: 468 → 309 · 0.9 % → 0 % · decay n/a (loop) · 6.1 → 7.6
- `one_way_gate_swing`: 2018.5 → 1971 · 6.45 % → 12.7 % · 580 → 140 · 8.5 → 9.35

For the rug rolls, the centroid fell from 468–3398 Hz to 309–353 Hz and the band above 4 kHz went to 0 %. Their tonal peak fell for glass (16.3 → 5.7 dB) and solid (8.9 → 7.6 dB) and rose a little for steel (6.1 → 7.6 dB), so all three now sit low. For the rug impacts, the tonal peak went down in 8 of 9 cues and the 20 dB decay got shorter in 7 of 9. The centroid fell in 6 of 9 and rose for `impact_glass_rug_light`, `impact_glass_rug_medium` and `impact_solid_rug_medium`, all of which stay under 1 kHz with no energy above 4 kHz. `impact_steel_rug_heavy` is the exception on tonal peak (15.5 → 21.1 dB). Two of its originals peaked near 2.6 kHz, a metallic ring. Three of the four new takes peak at 59–105 Hz, a low thud resonance, and v05 peaks at 1.2 kHz. The gate is not darker, because a wooden creak and tap are brighter than a rolling marble. Its 20 dB decay fell from 580 ms to 140 ms. None of these numbers can show whether a take sounds like carpet or like a gate, so Paul still judges that on the audition board.

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
- ElevenLabs prompt (revision 2): "A small light hard ball rolling steadily at constant medium speed across a thick soft wool carpet, only the soft fibrous hush and gentle rustle of carpet pile brushing past, very light and airy, muffled and dark, continuous with no start or stop, no ring, no click, no glassy or metallic tone, no wood knock, close mic, dry room, real recording, no music, no reverb"
- ElevenLabs prompt (revision 3, prompt influence 0.7): "very soft muffled brushing hush of thick wool felt being very lightly rubbed, a continuous steady low and dull fabric whisper, almost silent, no clicks, no hard surface, no wood, no ring, no music"

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
- ElevenLabs prompt (revision 2): "A small dense hard ball rolling steadily at constant medium speed across a thick soft wool carpet, only the soft fibrous hush and gentle rustle of carpet pile brushing past, soft with a little more body, muffled and dark, continuous with no start or stop, no ring, no click, no glassy or metallic tone, no wood knock, close mic, dry room, real recording, no music, no reverb"
- ElevenLabs prompt (revision 3, prompt influence 0.7): "very soft muffled brushing hush of thick wool felt being gently rubbed, a continuous steady low and dull fabric whisper, almost silent, no clicks, no hard surface, no wood, no ring, no music"

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
- ElevenLabs prompt (revision 2): "A small heavy hard ball rolling steadily at constant medium speed across a thick soft wool carpet, only the soft fibrous hush and gentle rustle of carpet pile brushing past, slightly heavier and deeper, pressing into the pile, muffled and dark, continuous with no start or stop, no ring, no click, no glassy or metallic tone, no wood knock, close mic, dry room, real recording, no music, no reverb"
- ElevenLabs prompt (revision 3, prompt influence 0.7): "very soft muffled brushing hush of thick wool felt being gently rubbed with a little more weight, slightly deeper, a continuous steady low and dull fabric whisper, almost silent, no clicks, no hard surface, no wood, no ring, no music"

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
- ElevenLabs prompt (revision 2): "A light soft drop of a small light hard ball onto a thick soft wool carpet, the deep carpet pile swallows the hit: one single light, small, soft thump, muffled and dark, no ring, no click, no glassy or metallic tone, no wood knock, no bounce, close mic, dry room, real recording, no music, no reverb"
- ElevenLabs prompt (revision 3, prompt influence 0.7): "one single very soft dull muffled thud of a small light weight settling into a thick wool felt cushion, barely touching down, a quiet low pff with a soft rounded onset, no click, no hard surface, no wood, no ring, no bounce, no music, no reverb"

### `impact_glass_rug_medium`

- family: `impact`
- trigger: contact event: glass marble against the woven rug, impulse band medium
- description: Single medium impact of the glass marble on the woven rug.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `impact_glass_rug_medium_vNN.ogg` (Opus, 48 kHz mono) and `impact_glass_rug_medium_vNN.m4a` (AAC fallback)
- ElevenLabs prompt (revision 2): "A firm drop of a small light hard ball onto a thick soft wool carpet, the deep carpet pile swallows the hit: one single light, small, soft thump, muffled and dark, no ring, no click, no glassy or metallic tone, no wood knock, no bounce, close mic, dry room, real recording, no music, no reverb"
- ElevenLabs prompt (revision 3, prompt influence 0.7): "one single very soft dull muffled thud of a small light weight settling into a thick wool felt cushion, a quiet low pff with a soft rounded onset, no click, no hard surface, no wood, no ring, no bounce, no music, no reverb"

### `impact_glass_rug_heavy`

- family: `impact`
- trigger: contact event: glass marble against the woven rug, impulse band heavy
- description: Single heavy impact of the glass marble on the woven rug.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -18 LUFS (active region) ±2, true peak ≤ -1 dBTP
- output: `impact_glass_rug_heavy_vNN.ogg` (Opus, 48 kHz mono) and `impact_glass_rug_heavy_vNN.m4a` (AAC fallback)
- ElevenLabs prompt (revision 2): "A hard drop from about 30 cm of a small light hard ball onto a thick soft wool carpet, the deep carpet pile swallows the hit: one single light, small, soft thump, muffled and dark, no ring, no click, no glassy or metallic tone, no wood knock, no bounce, close mic, dry room, real recording, no music, no reverb"
- ElevenLabs prompt (revision 3, prompt influence 0.7): "one single very soft dull muffled thud of a small light weight settling into a thick wool felt cushion, landing a little firmer but still soft, a quiet low pff with a soft rounded onset, no click, no hard surface, no wood, no ring, no bounce, no music, no reverb"

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
- ElevenLabs prompt (revision 2): "A light soft drop of a small dense hard ball onto a thick soft wool carpet, the deep carpet pile swallows the hit: one single soft, dull, padded thump, muffled and dark, no ring, no click, no glassy or metallic tone, no wood knock, no bounce, close mic, dry room, real recording, no music, no reverb"
- ElevenLabs prompt (revision 3, prompt influence 0.7): "one single very soft dull muffled thud of a small weight settling into a thick wool felt cushion, barely touching down, a quiet low pff with a soft rounded onset, no click, no hard surface, no wood, no ring, no bounce, no music, no reverb"

### `impact_solid_rug_medium`

- family: `impact`
- trigger: contact event: solid marble against the woven rug, impulse band medium
- description: Single medium impact of the solid marble on the woven rug.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `impact_solid_rug_medium_vNN.ogg` (Opus, 48 kHz mono) and `impact_solid_rug_medium_vNN.m4a` (AAC fallback)
- ElevenLabs prompt (revision 2): "A firm drop of a small dense hard ball onto a thick soft wool carpet, the deep carpet pile swallows the hit: one single soft, dull, padded thump, muffled and dark, no ring, no click, no glassy or metallic tone, no wood knock, no bounce, close mic, dry room, real recording, no music, no reverb"
- ElevenLabs prompt (revision 3, prompt influence 0.7): "one single very soft dull muffled thud of a small weight settling into a thick wool felt cushion, a quiet low pff with a soft rounded onset, no click, no hard surface, no wood, no ring, no bounce, no music, no reverb"

### `impact_solid_rug_heavy`

- family: `impact`
- trigger: contact event: solid marble against the woven rug, impulse band heavy
- description: Single heavy impact of the solid marble on the woven rug.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -18 LUFS (active region) ±2, true peak ≤ -1 dBTP
- output: `impact_solid_rug_heavy_vNN.ogg` (Opus, 48 kHz mono) and `impact_solid_rug_heavy_vNN.m4a` (AAC fallback)
- ElevenLabs prompt (revision 2): "A hard drop from about 30 cm of a small dense hard ball onto a thick soft wool carpet, the deep carpet pile swallows the hit: one single soft, dull, padded thump, muffled and dark, no ring, no click, no glassy or metallic tone, no wood knock, no bounce, close mic, dry room, real recording, no music, no reverb"
- ElevenLabs prompt (revision 3, prompt influence 0.7): "one single very soft dull muffled thud of a small weight settling into a thick wool felt cushion, landing a little firmer but still soft, a quiet low pff with a soft rounded onset, no click, no hard surface, no wood, no ring, no bounce, no music, no reverb"

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
- ElevenLabs prompt (revision 2): "A light soft drop of a small heavy hard ball onto a thick soft wool carpet, the deep carpet pile swallows the hit: one single heavier, deeper, padded thud, muffled and dark, no ring, no click, no glassy or metallic tone, no wood knock, no bounce, close mic, dry room, real recording, no music, no reverb"
- ElevenLabs prompt (revision 3, prompt influence 0.7): "one single very soft dull muffled thud of a small heavy weight settling into a thick wool felt cushion, barely touching down, a quiet low pff with a soft rounded onset, no click, no hard surface, no wood, no ring, no bounce, no music, no reverb"

### `impact_steel_rug_medium`

- family: `impact`
- trigger: contact event: steel marble against the woven rug, impulse band medium
- description: Single medium impact of the steel marble on the woven rug.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -24 LUFS (active region) ±2, true peak ≤ -6 dBTP
- output: `impact_steel_rug_medium_vNN.ogg` (Opus, 48 kHz mono) and `impact_steel_rug_medium_vNN.m4a` (AAC fallback)
- ElevenLabs prompt (revision 2): "A firm drop of a small heavy hard ball onto a thick soft wool carpet, the deep carpet pile swallows the hit: one single heavier, deeper, padded thud, muffled and dark, no ring, no click, no glassy or metallic tone, no wood knock, no bounce, close mic, dry room, real recording, no music, no reverb"
- ElevenLabs prompt (revision 3, prompt influence 0.7): "one single very soft dull muffled thud of a small heavy weight settling into a thick wool felt cushion, a quiet low pff with a soft rounded onset, no click, no hard surface, no wood, no ring, no bounce, no music, no reverb"

### `impact_steel_rug_heavy`

- family: `impact`
- trigger: contact event: steel marble against the woven rug, impulse band heavy
- description: Single heavy impact of the steel marble on the woven rug.
- duration: 1 s request, trimmed to 150–600 ms
- loop: one-shot
- variations to generate: 4
- loudness: -18 LUFS (active region) ±2, true peak ≤ -1 dBTP
- output: `impact_steel_rug_heavy_vNN.ogg` (Opus, 48 kHz mono) and `impact_steel_rug_heavy_vNN.m4a` (AAC fallback)
- ElevenLabs prompt (revision 2): "A hard drop from about 30 cm of a small heavy hard ball onto a thick soft wool carpet, the deep carpet pile swallows the hit: one single heavier, deeper, padded thud, muffled and dark, no ring, no click, no glassy or metallic tone, no wood knock, no bounce, close mic, dry room, real recording, no music, no reverb"
- ElevenLabs prompt (revision 3, prompt influence 0.7): "one single very soft dull muffled thud of a small heavy weight settling into a thick wool felt cushion, landing a little firmer but still soft, a quiet low pff with a soft rounded onset, no click, no hard surface, no wood, no ring, no bounce, no music, no reverb"

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
- ElevenLabs prompt (revision 2): "A small light wooden flap on a toy wooden hinge swings up and falls straight back: a short soft creak of the wooden hinge, then one soft wooden tap as the flap closes against its wooden frame, all within half a second, one continuous motion, no marble rolling, no ball, no glass, no metal, close mic, dry room, real recording, no music, no reverb"

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
