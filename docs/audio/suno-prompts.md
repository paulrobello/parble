# parble Suno music prompt pack

Status: P4-T1 draft for Paul's approval. These are five original instrumental prompts for Paul to paste into Suno, generate, audition and select (P4-T5 owner action). Nothing here is generated or automated, and no prompt grants rights or owner approval. Format follows `~/.claude/guides/suno.md`.

## How to use

- Use Custom mode with Instrumental On and leave lyrics empty.
- Paste a cue's **Style** block into the style field and its **Exclude styles** block into that separate field.
- Work down the **Settings** block in the order the v6 Custom-mode panel shows them.
- Generate a pair of takes per cue and keep the preferred one. Change one prompt or setting at a time when comparing.
- A loop request does not produce a verified loop. Loop points are found and checked in P4-T5 from the downloaded WAV. Prefer takes whose first and last bars have similar energy, with no fade-out, no vocal-like phrases and no long silences.

The palette for every cue is warm acoustic and ambient: felt piano, marimba and soft strings, recorded close in a warm wooden room. Music sits under constant marble rolling and clacking, so every cue keeps the high mids and the transient range clear, stays at low to medium energy and avoids percussion that sounds like marble impacts.

Weirdness is low (15–25%) for predictable, steady beds. Style Influence is high (75–85%) so the written brief dominates. Variety is 0 to keep the supplied style text, and Personalize is Off so the brief is tested on its own. These are starting values for this project, not Suno defaults.

## Cue 1: `music_build_calm` (editor, building a track)

Use: the main bed while Paul builds in the editor. The quietest and most neutral cue. Loop intent: a 2:00 take prepared as a seamless loop, then crossfaded with cue 2 in the playlist.

```text
Style:
Original instrumental warm acoustic ambient music for calmly building a wooden toy marble run at night. Gentle, cosy and quietly curious, low energy. Soft felt piano playing sparse broken chords, warm marimba with soft mallets in a simple repeating figure, and a quiet bed of sustained soft strings. Approximately 72 BPM with a relaxed, even pulse. An eight-bar opening, a steady repeating body where a short original motif returns with small changes, and a closing section with the same energy as the opening so it can loop. Two-minute arrangement. Warm, close wooden-room sound. Keep the high mids and upper range clear and leave plenty of space for small wooden clicks and rolling marble sounds.

Exclude styles:
vocals, singing, spoken word, choir, humming, drums, drum kit, hi-hats, claps, wood blocks, sound effects, synth leads, electronic beats, heavy sub-bass, big crescendos, dramatic swells, fade-out ending

Settings:
Instrumental: On
Vocal Gender: not selected (instrumental)
Duration: 2:00
Max Mode: Off
Weirdness: 15%
Style Influence: 85%
Variety: 0
Personalize / My Taste: Off
Model: record the model you select (unknown until supplied)
```

## Cue 2: `music_build_playful` (editor, alternate)

Use: second editor bed in the playlist, slightly brighter than cue 1 so a long session does not repeat one track. Loop intent: 2:00 seamless loop candidate.

```text
Style:
Original instrumental warm acoustic ambient music for playful tinkering with wooden toys. Light, gently bouncy and warm, low to medium energy. Marimba leading with a soft rolling original melody, felt piano answering in short phrases, pizzicato and softly bowed strings underneath. Approximately 88 BPM with a light swaying feel and generous rests between phrases. A four-bar opening, a repeating sixteen-bar body with small orchestration changes, and a closing that settles back to the opening texture for a loop. Two-minute arrangement. Close, warm room sound with a controlled low end. Leave the upper range open for marble clicks and rolling.

Exclude styles:
vocals, singing, spoken word, choir, humming, drums, drum kit, claps, wood blocks, glockenspiel, sound effects, synth leads, electronic beats, heavy sub-bass, big crescendos, fade-out ending

Settings:
Instrumental: On
Vocal Gender: not selected (instrumental)
Duration: 2:00
Max Mode: Off
Weirdness: 25%
Style Influence: 80%
Variety: 0
Personalize / My Taste: Off
Model: record the model you select (unknown until supplied)
```

## Cue 3: `music_run_flow` (watching a run)

Use: plays while marbles run under the follow camera. A gentle forward motion that suits the camera's tracking shot without competing with the run's sound. Loop intent: 2:00 seamless loop candidate.

```text
Style:
Original instrumental warm acoustic ambient music for watching marbles roll down a long wooden track at night. Flowing, gently absorbing and warm, medium-low energy. A steady soft marimba ostinato, felt piano placing an original motif over it, and soft legato strings rising slowly and settling again. Approximately 96 BPM with a smooth, continuous forward pulse. A short four-bar opening, a long steady body that keeps the same tempo and density, and a closing with energy similar to the opening for a loop. Two-minute arrangement. Close, warm room sound. Keep the dynamics even and leave space in the high mids for rolling, impact and mechanism sounds.

Exclude styles:
vocals, singing, spoken word, choir, humming, drums, drum kit, hi-hats, claps, wood blocks, sound effects, synth leads, electronic beats, heavy sub-bass, tempo changes, dramatic drops, big cinematic crescendos, fade-out ending

Settings:
Instrumental: On
Vocal Gender: not selected (instrumental)
Duration: 2:00
Max Mode: Off
Weirdness: 20%
Style Influence: 80%
Variety: 0
Personalize / My Taste: Off
Model: record the model you select (unknown until supplied)
```

## Cue 4: `music_night_lanterns` (ambient, idle)

Use: the sparsest bed, for idle moments, the paused state and long viewing of the night playroom. Loop intent: 2:30 seamless loop candidate.

```text
Style:
Original instrumental quiet ambient music for a dim playroom at night lit by warm paper lanterns. Still, warm and softly dreamy, very low energy. Slow sustained soft strings, distant felt piano notes with long gaps between them, and occasional very soft low marimba rolls. Approximately 60 BPM with a loose, unhurried feel. A slow eight-bar opening, a long sparse body where a few original notes return, and a closing that stays at the same quiet level for a loop. Two-and-a-half-minute arrangement. Warm, close room sound. Keep everything soft and leave the whole upper range open for small marble sounds.

Exclude styles:
vocals, singing, spoken word, choir, humming, drums, percussion, wood blocks, sound effects, synth pads, electronic textures, heavy sub-bass, swells, crescendos, long silences, fade-out ending

Settings:
Instrumental: On
Vocal Gender: not selected (instrumental)
Duration: 2:30
Max Mode: Off
Weirdness: 15%
Style Influence: 80%
Variety: 0
Personalize / My Taste: Off
Model: record the model you select (unknown until supplied)
```

## Cue 5: `music_title` (title screen)

Use: the title screen (P5) and first-run settings. A slightly fuller, welcoming statement of the same palette. Loop intent: 1:30 loop candidate. A take with a clean ending is also acceptable if it is only played once before crossfading to cue 1.

```text
Style:
Original instrumental warm acoustic ambient music for the title screen of a cosy wooden marble-run game. Welcoming, warm and gently wondrous, medium-low energy. Felt piano stating a simple original theme, marimba doubling it softly, and warm soft strings filling out the harmony. Approximately 80 BPM with a gentle lilt. A short four-bar introduction, the theme stated twice with fuller strings the second time, and a closing that returns to the introduction's texture for a loop. Ninety-second arrangement. Warm, close wooden-room sound with a controlled low end.

Exclude styles:
vocals, singing, spoken word, choir, humming, drums, drum kit, claps, sound effects, synth leads, electronic beats, heavy sub-bass, epic orchestral hits, big cinematic crescendos, fade-out ending

Settings:
Instrumental: On
Vocal Gender: not selected (instrumental)
Duration: 1:30
Max Mode: Off
Weirdness: 20%
Style Influence: 75%
Variety: 0
Personalize / My Taste: Off
Model: record the model you select (unknown until supplied)
```

## Handing back the selected takes

For each cue you keep, send:

1. The full-song **WAV** downloaded from the Suno web interface. Do not normalize, trim, resample or re-export it.
2. The cue id it fills (for example `music_run_flow`).
3. The song link or ID.
4. The model you selected.
5. The account plan at generation, only if you want it recorded per track.
6. Any change you made to the Style, Exclude styles or settings text above. If none, say "unchanged".

Drop the WAVs anywhere convenient and give the paths. P4-T5 copies them unchanged into `assets-source/audio/music/` with their original filename, SHA-256, byte size, duration, sample rate and channels recorded in provenance, then prepares loop derivatives separately. Missing metadata stays recorded as unknown.

Technical preparation, creative acceptance and release approval are recorded as separate statuses. Selecting a take approves it as a source only, not as a prepared loop or the final mix.
