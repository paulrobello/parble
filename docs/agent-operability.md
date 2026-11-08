# Agent operability

parble can be launched, observed and stopped from a script, on native and on the web, without anyone at the keyboard (P0-T6). The switches are the same on both platforms. Natively they are flags. On the web they are URL query parameters.

## Switches

| Flag (native) | Query (web) | Default | Effect |
|---|---|---|---|
| `--screenshot <path>` | `screenshot=<name>` | off | Capture the primary window once. Natively the PNG is written to `<path>` and parent directories are created. On the web the browser downloads `<name>`. |
| `--screenshot-after <s>` | `screenshot-after=<s>` | `3` | Seconds of real time after startup before the capture. The first frames show only the backdrop while pipelines compile, so stay at 3 s or more. |
| `--exit-after <s>` | `exit-after=<s>` | off | Exit after `<s>` seconds of real time. If a screenshot is still pending, the app waits for it, up to 10 s longer. |
| `--scenario <name>` | `scenario=<name>` | `ramp` | Scene to load. `ramp` is the tray ramp with one marble, restarted every 10 s. `closeup` (P1-T6) rests one marble on the tray floor under a close fixed camera, for judging marble materials. `playroom` (P1-T5) is the night playroom: rug, room shell, paper lanterns, a warm lamp, toy blocks, the backdrop plate and the baked environment probe. In place of the tray it stands a U-shaped marble run of 18 shipped kit pieces on the rug (`blk_end_l3` at both ends, `blk_straight_l3`, two `blk_curve90_l3`; `crates/parble-sim/src/playroom_run.rs`), drawn by the P1-T4 kit renderer and simulated on the P1-T3 chunk colliders. Four standard marbles (swirl, opal, onionskin, glow) start rolling at 0.34 m/s, run both curves and gather against the finish stop by about 2.75 s. The run restarts every 4 s (960 ticks) and has its own committed hash, `crates/parble-sim/tests/golden/playroom_4s.hash`, checked natively and by `make determinism-wasm`. It ignores `--marble` and `--marble-size`. `blk_slope_gentle_l3` is not in the run: its exit is `L2` and every other shipped port is `L3`, so no shipped piece can take a marble from it without a level mismatch. `kit2000` (P1-T4) adds a 2,000-piece showcase of the four kit blocks behind the ramp: one flat 50 × 40 grid with a half-cell gap between neighbours on both axes (1.5-cell, 0.09 m pitch), floating two cells (0.12 m) above the floor so the undersides show from the fly camera. It spans 4.47 × 3.57 m in 80 chunks, and the floor grows to cover it. A fixed `set` shot (`Cues::kit`) from beside the grid's near corner shows all three LODs and both crossfade bands. It is render-only too, so it keeps the ramp's golden hash. `lineup` (P5-T1) rests every marble type side by side on the floor in front of the tray, with a mini and a shooter swirl at the two ends, under one fixed set shot. It ignores `--marble` and `--marble-size`. `editor` (P3-T4) is the in-engine editor: the tray, no marble, the orbit camera with the editor input scheme, and kit pieces edited in place ([Editor](#editor)). It never restarts, so edits keep their incremental colliders. `mechanisms` (P2-T6) puts two short lanes behind the tray, each feeding one cat's eye into a P2-T5 mechanism (`crates/parble-sim/src/mech_showcase.rs`): an L3 straight into `mech_toggle_l3` (`first = 1`, so the marble leaves by the −Z branch and the rocker then swings to side 0), and `mech_gate_oneway_l1` between two L1 straights (the flap opens for the marble and swings shut over 24 ticks). Each mechanism runs its kit behaviour and the renderer draws its part at the sim's angle, interpolated between ticks. One fixed set shot (`Cues::mechanisms`) frames both. It restarts every 10 s and ignores `--marble`. `allpieces` (P2-T7) is the all-pieces test track behind the tray (`crates/parble-sim/src/allpieces.rs`): 46 placements that use all 16 PRD kit items (blocks, slopes, curves, splitter, joiners, redirects, drop, tower, pillar, rails, toggles, one-way gate, trapdoor hole, checkpoint, hopper and finish cups). The hopper releases 10 standard marbles (swirl, opal, onionskin, cat's eye, glow, repeated) every 2 s, and all 10 come to rest in the two finish cups behind the last toggle by about 27 s. The shipped kit has no piece that raises a marble's port level, so three hand-offs are free-fall drops from an open `L1` exit into an `L3` channel one layer below, and the track validator reports open ports there. Marble visuals that the hopper has not released yet are hidden. The run restarts every 30 s (7,200 ticks) and has its own committed hash, `crates/parble-sim/tests/golden/allpieces_30s.hash`, checked natively and by `make determinism-wasm`. One fixed set shot (`Cues::allpieces`) frames the whole track. It ignores `--marble` and `--marble-size`. |
| `--editor-fill <n>` | `editor-fill=<n>` | off | Editor only: start with an `n`-piece grid (at most 4,000) behind the tray, laid as one undoable paste. `2000` is the edit perf track (42 dense chunks). |
| `--marble <type>` | `marble=<type>` | legacy | Marble type ([marbles.md](marbles.md)): `cats-eye`, `glazed`, `steel` (P1-T6), `swirl`, `onionskin`, `opal` or `glow` (P5-T1), each with its real physical values and its own look. Without it the scene uses the P0 legacy marble (golden physics, cat's eye look), so `ramp` still reproduces the golden hash. |
| `--marble-size <size>` | `marble-size=<size>` | `standard` | Marble size (P5-T1): `mini` (10 mm), `standard` (16 mm) or `shooter` (25 mm). The type keeps its material, and its mass follows from density × volume. Without `--marble`, `standard` is still the legacy marble and the other sizes are a cat's eye at that size. |
| `--view <name>` | `view=<name>` | none | Playroom fixed framing, played by the director as one `set` shot (`Cues::playroom` in `camera.rs`) whose DoF focus stays on the view's lantern instead of the marble. `rug` looks low across the rug towards the big lantern (ref-07). `blocks` looks at the toy-block arches and towers around the small lantern (ref-08). Without it the playroom follows the lead marble round the kit run and cuts to a set shot over the whole run at tick 600 (`Cues::playroom_run`). `--camera overview` and `orbit` take precedence. Other scenarios ignore it. |
| `--tex <family>` | `tex=<family>` | device | Caps the kit wood texture family: `bc7`, `astc` or `png`. The renderer picks BC7 KTX2 when the device has BC compression, else ASTC KTX2, else PNG (`crates/parble-render/src/kit/family.rs`). The cap only steps down that order, so `png` always forces the PNG fallback. Only `kit2000` and `playroom` load the wood maps. |
| (web only) | `mute=1` | off | Silences the Web Audio output through a zero gain. The engine still runs, decodes and counts voices, so `window.__parbleAudio` stats stay valid. Use it for every agent and headless run; Playwright and the smoke scripts also launch Chromium with `--mute-audio`. |
| `--seed <u64>` | `seed=<u64>` | `0` | Recorded and reported. No P0 scenario uses it yet. Procedural tracks consume it from P3. |
| `--quality <tier>` | `quality=<tier>` | `medium` | `low`, `medium`, `high` or `ultra`. Per-tier AA (MSAA or TAA), SSAO, bloom, DoF and shadows are in `docs/adr/0001-post-stack-and-tiers.md`. The glass marbles (cat's eye, swirl, onionskin, opal, glow) use fallback shaders on `low` and `medium` (the vane march takes 16 and 32 steps), and screen-space specular transmission on `high` and `ultra`. Shadow-casting lights are capped per tier: 0 on Low, 1 on Medium, 3 on High and Ultra. |
| `--camera <c>` | `camera=<c>` | `director` | `director`: the follow-marble director with the scenario's cued cuts. `orbit`: the editor orbit camera (left drag, wheel). `overview`: the fixed P0 top-down view. `C` toggles director and orbit. `F` enters the free-fly camera in every scenario ([Camera controls](#camera-controls)). |
| `--taa <0\|1>`, `--dof <0\|1>`, `--bloom <0\|1>`, `--aperture <f>`, `--tonemap <name>` | same keys | tier | Look-dev and test overrides of the tier's post stack. The piece showcases (`kit2000`, `mechanisms`, `allpieces`) default to `dof=0` so every piece is sharp; `dof=1` turns it back on. `bloom` (P5-T1) exists for the glow marble's bloom check. `tonemap`: `tony`, `agx`, `aces`, `filmic`, `sbdt`, `reinhard`. |
| `--lockstep-after <s>` | `lockstep-after=<s>` | off | Hold the sim at tick 0 for `<s>` real seconds, then advance exactly 4 ticks (1/60 s) per frame, so a frame shows the same state on every run. The run hash is unchanged. |
| `--screenshot-tick <n>` | `screenshot-tick=<n>` | off | Capture on the first frame at or past sim tick `<n>` instead of by time, then exit as soon as the PNG is written. Native captures with `--screenshot` use a 1280x720 window at scale factor 1. |
| `--sensors` (or `--sensors=0\|1`) | `sensors=1` | off | Start with the mechanism sensor overlay on: every sensor volume of every mechanism instance (boxes and spheres from the sidecars, placed with the piece's anchor, yaw and mirror) drawn as a world-space wireframe over the scene, cyan while idle and magenta while a marble is inside. `G` toggles it in any scenario ([Camera controls](#camera-controls)). It is drawn with Bevy gizmos (`crates/parble-render/src/sensors.rs`) from the read-only `Sim::sensors`, so it cannot change a hash. |
| `--dump-state` | `dump-state` (or `=1`) | off | At auto-exit, print the state JSON. Natively it goes to stdout as one line. On the web it is logged to the console as `parble state: {...}`. |

Native flags accept both `--key value` and `--key=value`. `--dump-state` and `--sensors` take no value; give one with `=`. An unknown flag or a bad value prints an error and exits with code 2. Unknown query keys are ignored on the web. A bad query value logs an error and the app falls back to the defaults.

When `--screenshot` or `--exit-after` is set, the app redraws continuously even if its window never gets focus.

## Exit codes (native)

- `0`: clean exit. If a screenshot was requested, it was written and shows a lit scene.
- `2`: bad flags.
- `3`: the screenshot failed. Either it never arrived, writing it failed, or it is not a lit scene.

The lit-scene check uses the same thresholds as the web smoke. More than 50 % of pixels must be non-black (max channel above 8), more than 2 % must be lit (max channel above 60), and the frame must hold more than 16 distinct 4-bit colours. The backdrop alone passes the first test and fails the other two.

## State JSON and `window.__parble`

The web build replaces `window.__parble` every frame. `--dump-state` prints the same object. This one is from `make run-diag`:

```json
{
  "ready": true,
  "tick": 682,
  "run": 0,
  "frame": 249,
  "fps": 124.9248,
  "hash": "3f6e35c78c10f020",
  "lastRunHash": null,
  "marbles": [{"position": [0.34963298, 0.007915289, -1.3966178e-07], "rotation": [1.4082439e-06, 3.885606e-08, 0.36115447, -0.932506], "linvel": [0.25444952, 0.0, -1.9141713e-07]}],
  "scenario": "ramp",
  "track": null,
  "trackRun": null,
  "seed": 0,
  "quality": "medium",
  "marble": "legacy",
  "marbleSize": "standard",
  "shadowLights": 1,
  "shadowLightCap": 1,
  "view": null,
  "screenshot": {
    "state": "none"
  },
  "sensors": {"on": false, "volumes": 0, "occupied": 0},
  "exited": true
}
```

- `ready`: the sim has ticked and at least 3 frames have run. Pixels can lag behind it while pipelines compile, so a pixel check still needs to poll.
- `tick`: sim ticks in the current run (240 Hz).
- `run`: completed 10 s runs.
- `hash`: the trajectory hash of the current run so far, as 16 hex digits. It is a string because a `u64` does not fit a JS number.
- `lastRunHash`: the hash of the last completed run. A run always stops at exactly 2,400 ticks (960 in `playroom`), so for `ramp` this equals the golden hash in `crates/parble-sim/tests/golden/ramp_10s.hash` and for `playroom` the hash in `playroom_4s.hash`.
- `marbles`: one entry per marble in spawn order. Positions are in metres, rotation is a quaternion `[x, y, z, w]`, and `linvel` is in m/s.
- `track`: the metadata name of the `.parble` track loaded over the bridge (`loadTrack`), or `null`. A loaded track replaces the scenario in the live sim and its 10 s restarts. Its placements run as kit chunk colliders through the id table in `crates/parble-sim/src/track_run.rs` (kit ids, legacy aliases, and pieces with no collider yet). `loadTrack` refuses a track with any other piece id, or with overlapping kit pieces, with a `bridgeError`.
- `trackRun`: the last bridge `runTrack`, as `{ "ticks": 2400, "hash": "…" }`, or `null`. The run is headless from tick 0, so its hash does not depend on frame timing. `web/ui/src/parble-fixtures.json` records the expected `runHash` of every valid fixture track; `make web-e2e` checks it in the browser (`e2e/tests/persistence.spec.mjs`).
- `marble`: the `--marble` type, `legacy` for the P0 marble, `lineup` in the lineup scenario, `playroom` in the playroom, `mechanisms` in the mechanism showcase, or `allpieces` on the all-pieces track.
- `marbleSize`: the `--marble-size` value (P5-T1).
- `shadowLights`, `shadowLightCap`: lights casting shadows this frame and the tier's cap (0 on Low, 1 on Medium, 3 on High and Ultra). `enforce_shadow_cap` in `crates/parble-render/src/playroom.rs` turns off the lowest-priority shadows past the cap in every scenario. `playroom` on High uses all 3: the moon, the big lantern and the lamp.
- `view`: the playroom `--view` framing, or `null` when none was given.
- `kit` (only with `--scenario kit2000`, `playroom`, `mechanisms`, `allpieces` or `editor`): the P1-T4 renderer. `pieces`, `chunks`, `pieceTypes` (the piece types in the layout, or placed in the editor) and `drawBound` (one draw per occupied (type × chunk) × 3 LODs, plus 3 more per animated part of the type: each mechanism part is its own mesh, P2-T6). `variants` is the number of wood variants and `variantPieces` the pieces using each (picked by a hash of the anchor cell, or of the grid index in `kit2000`). Each (type, LOD, variant) is one material, so the kit draws as `pieceTypes` × 3 × `variants` bins. `lodTriangles` per piece type and `lodBandsM`, the camera distances of the two crossfade bands. `family` and `textures` (`path`, `kind`, `state`, the loaded `format`, and the asset server's `error` when `state` is `failed`): `kind` is `wood` for each variant's base_color and orm in the device's family, or `baked` for a piece's per-LOD AO and normal PNG. A piece's baked maps load the first frame it is in the layout or placed in the editor, so `kit2000` and `playroom` fetch only their own pieces' maps and the editor fetches six more per new piece type (they stay loaded after the last one is deleted). `draws` is read from the render world each frame: `main` is the main opaque pass's kit draws (one indirect draw per (pipeline, material, mesh) bin) over `mainViews` views, `mainInstances` the LOD instances in those bins and `ditheredInstances` the ones drawn with the crossfade dither pipeline. With GPU culling Bevy keeps its multidraw bins private, so `main` rebuilds them from the view's visible kit instances. `prepass` and `shadow` are `null` when that pass's bins cannot be rebuilt (shadow cascades). `multidrawSets` counts the main pass's multidraw batch sets.
- `screenshot`: `state` is one of `none`, `pending`, `capturing`, `done` (with `lit`, `colors` and `ok`), or `failed` (with `error`).
- `camera`: the main camera this frame: `mode` (`director`, `orbit` or `fly`), `shot`, `cuts`, `eye`, `forward` (unit view direction), `flySpeed` (m/s), the marble's screen position over the last frames (`marblePx`, physical pixels, newest first), `marbleRadiusPx`, and `post`, which holds the post components the live camera carries (`msaa`, `taa`, `ssao`, `bloom`, `dof`, `aperture`, `focusM`, `tonemapping`).
- `kit.parts` (P2-T6, only when a mechanism with animated parts is drawn): one entry per part, by anchor `cell`, `piece` and sidecar `part` index. `angleMdeg` is the angle drawn this frame, `prevMdeg` and `tickMdeg` the sim's angles at the previous and the current tick (`null` for an editor-placed piece, which has no behaviour and holds its sidecar rest angle), `alpha` the frame's interpolation factor (`angleMdeg` = `prevMdeg` + (`tickMdeg` − `prevMdeg`) × `alpha`, as for the marbles; 1 in lockstep), and `rotation` the LOD0 part's rotation in the piece frame.
- `sensors`: the sensor overlay. `on` is whether it is drawn (`--sensors`, `G`), `volumes` the sensors of every mechanism instance and `occupied` how many hold a marble at the current tick.
- `shotCamera`: the same object captured on the screenshot frame. `shotParts` is `kit.parts` on that frame.
- `editor` (only with `--scenario editor`): see [Editor](#editor).
- `camera.orbit`: the orbit camera's `target`, `yaw`, `pitch` and `radius`.
- `exited`: set when auto-exit fires.

All timing (screenshot, exit, fps) is real time in the app layer. The sim never receives wall-clock input, so none of these switches can change a trajectory hash.

## Audio and `window.__parbleAudio`

The Web Audio engine (P4-T4, `web/ui/src/audio`) runs in the HUD bundle, not in the wasm, so its state is on its own global: `window.__parble` is replaced by the wasm every frame and would drop anything added to it. Browsers only start audio after a user gesture, so the engine creates and resumes its `AudioContext` on the first pointer down, key press or touch on the page. Until then `state` is `locked` and audio frames are skipped. The SFX it plays are Paul's approved P4-T2 picks for the roll, impact and clack cues and the mechanism cues (hopper release, finish chime, checkpoint tick, splitter tick, toggle clack, gate swing, trapdoor and the three drop-hole cues), plus the tunnel rumble loop, staged into `web/ui/audio/` by `scripts/web/stage-audio.mjs` (run by `make ui-build`). Only the Ogg Opus files are fetched where the browser plays them, else the AAC fallback.

Each `audioFrame` contact and roll carries the marble's `size` (`mini`, `standard` or `shooter`), so impacts are banded by the velocity change the impulse gives that marble's real mass, and minis and shooters play their material's cues with a size pitch offset (`sizeRate` in `web/ui/src/audio/mapping.ts`). Each mechanism event carries its kit `piece` id, and `SIGNAL_CUES` there maps a piece's signal to its cue; sensor events and unmapped signals are silent. The rug cues are not staged: the sim has no rug collider (the tray and the open floor are wood slabs and report `wood`). The `blk_redirect_*` pieces are static blocks with no sensor, so a roll carries `tunnel: true` while the marble's centre is in a redirect's lower cell, the cell its tunnel runs through (`in_redirect_tunnel` in `crates/parble-app/src/audio.rs`); the engine then layers `tunnel_rumble` into that marble's roll voice with the roll's speed-driven rate, gain and low-pass, and fades it out when the flag clears. A redirect's drop entry is silent; the drop-hole cues play on the one-way hole's `drop`.

`window.__parbleAudio.stats()` returns:

- `state`: `locked`, `starting`, `running`, `suspended`, `closed` or `unsupported` (no Web Audio).
- `format` (`ogg` or `m4a`), `cues`, `files` (variants in the catalog), `fetched`, `decoded` and `loadErrors`.
- `activeVoices` (roll loops plus one-shots sounding now, never above `cap.total`), `rollVoices`, `oneShotVoices`, `tunnelLayers` (roll voices with the tunnel rumble layered in, not extra voices) and `peakVoices`. `cap` is `{ total: 24, rolls: 10 }`. Rolls keep their budget: when rolls return after one-shots filled their slots (a run restart, culled or stale rolls), the quietest one-shots are stopped and counted as `stolen`.
- `started` one-shots, `dropped` (refused: every slot held a louder voice), `stolen` (a louder one-shot or a returning roll took the slot), `rollsCulled` (rolls left silent per frame by the roll budget), `crossfades` (surface changes), `resets` (tick regressions: run restarts and loaded tracks).
- `frames`, `contacts`, `skippedFrames` (frames before the context ran), `missingBuffers`, `buffers`, `errors` and `lastError` (engine failures; the engine never throws into the HUD loop).
- `injected`: whether `inject` was used.

`window.__parbleAudio.inject(event)` is a test hook: it decodes `event` like a bridge event and feeds it to the engine if it is an `audioFrame`, returning `false` otherwise. After the first injected frame, live bridge frames are ignored, so a driver can stress the engine with more marbles than any scenario has. `make web-e2e` runs both (`e2e/tests/audio.spec.mjs`).

## Camera controls

Every scenario, `kit2000` and `lineup` included, has a free-fly camera (`Fly` in `crates/parble-render/src/camera.rs`). It works the same natively and on the web. On the web, click the scene first so the canvas has keyboard focus.

| Input | Effect |
|---|---|
| `F` | Enter fly from the current view and lock and hide the cursor (pointer lock on the web). In fly, `F` returns to the camera it replaced and releases the cursor. |
| `Esc` | Leave fly, back to the director (or orbit), and release the cursor. The director cuts straight back to its current cue. On the web the browser also releases pointer lock on `Esc`. |
| `W` `A` `S` `D` | Forward, left, back, right along the view. |
| `E` or `Space`, `Q` or `Shift` | Up, down (world Y). |
| Mouse | Look (yaw and pitch) from raw mouse motion while the cursor is locked. If the lock is refused, left or right drag looks instead, from cursor movement. |
| Wheel | Fly speed, ×1.25 per line, 0.02 to 5 m/s (starts at 0.3 m/s). |
| `C` | Director and orbit toggle (ignored while flying). |
| `G` | Mechanism sensor overlay on or off, in every camera mode (see `--sensors`). |

The camera moves only on input and uses real time, so it keeps working while `--lockstep-after` holds the sim. With no input it never leaves the director, so `--screenshot`, `--exit-after`, `?screenshot=` and `window.__parble` runs frame exactly as before. Keys typed into the HUD (the Import, Export, Run and Save controls) stay there, because the browser only delivers keys to the canvas while it has focus. The HUD shows the controls in its bottom-left line. `make web-e2e` drives the fly camera with the keyboard and mouse in GPU Chromium (`e2e/tests/flycam.spec.mjs`).

## Editor

`--scenario editor` (P3-T4, `crates/parble-app/src/editor.rs`) edits a track of kit pieces through the P3-T1 commands, with undo and redo. The palette is every builtin kit piece (the 29 sidecars in `assets/kit/`, one list in `parble_kit::builtin_piece_ids!`), in id order: blocks, mechanisms and rails, multi-cell footprints included, with each mechanism's animated parts drawn at their rest pose (a placed mechanism has no behaviour; a loaded track's mechanisms run theirs, and their parts move with the sim). The brush starts on `blk_straight_l3`. The `fill` layout keeps to the four one-cell P1-T4 showcase blocks (`blk_straight_l3`, `blk_curve90_l3`, `blk_slope_gentle_l3`, `blk_end_l3`). The editor UI overlay is P3-T5. A `loadTrack` or `runTrack` in this scenario restarts the live sim on the loaded file, and the editor starts over on that track: its placements become the editor's pieces, the selection is cleared and the undo and redo history is reset (the loaded track is the new base, so undo cannot reach edits from before it). `runTrack` therefore also discards edits made since the load. The editor has no save path until P3-T5. A loaded track does not restart every 10 s in the editor, so edits after a load keep their incremental colliders.

Every command, undo and redo diffs the placements by anchor. Only the changed placements are removed and placed in the live sim's chunk colliders, and only the render chunks holding their anchors are respawned. Dirty colliders are rebuilt in stages (gather, simplify, tag, install), as many as fit 4 ms a frame. A dense 64-piece chunk is about 17 ms of work natively, so one chunk spreads over a few frames instead of stalling one. `crates/parble-sim/tests/incremental_edit.rs` checks that 400 random edits (mirrored curves and rails included), rebuilt in place and in stages, leave exactly the colliders a fresh build of the final track has: the same trimeshes, active contact hooks and contact-hook tags (stitched edges, channel bends, channel slopes, rail arcs). An edit dirties the chunks of the pieces across its cells' faces, edges and corners, since a stitched edge on a cell corner looks up the diagonal cell.

Mouse and keys (click the canvas first):

| Input | Effect |
|---|---|
| Left click | Place: the brush at the ghost. Select: select the piece (Shift toggles), or clear on the ground. Delete: remove the piece. |
| Left drag (select tool) | From a selected piece: move the group by the cell offset on its level's plane. Elsewhere: box select the pieces whose centres fall in the rectangle (Shift adds). |
| Right drag, middle or Shift + right drag, wheel | Orbit, pan along the ground, zoom. |
| `1` `2` `3` | Place, select, delete tool. |
| `R`, `M`, `Tab` | Rotate and mirror (chiral pieces only) the brush in the place tool, the selection in the others, next palette piece. |
| `Delete` or `Backspace`, `Esc` | Remove the selection, clear it. |
| `Ctrl`/`Cmd` + `Z`, + `Shift` + `Z` or + `Y` | Undo, redo. |
| `[`, `]`, `X` | Slice one level down, up, show all. |

Picking walks the camera ray through the occupied cells, skipping pieces above the slice, and otherwise lands on the ground plane at the slice level. The place target is the empty cell in front of the hit face, or the ground cell. The ghost is the brush there, green when `Editor::check` accepts the placement and red when not, with one dot per port: green when a mating port across links, amber when nothing is across, red for a fault.

Agent actions: `window.wasmBindings.parbleEdit(json)` queues one action for the next frame. It throws on JSON that is not an action. Each action is an object with an `op` and an optional `id`. Pixels are CSS pixels from the canvas's top-left corner.

| `op` | Fields | Effect |
|---|---|---|
| `tool` | `tool`: `place`, `select`, `delete` | Switch tool. |
| `brush` | `piece`, `yaw`, `mirror` | Set the brush. |
| `hover`, `hoverCell`, `unhover` | `x`, `y` / `cell` | Pin the hover to a pixel or the place target to a cell, until the real cursor moves. |
| `click` | `x`, `y`, `shift` | The tool at a pixel. |
| `place` | `piece`, `cell`, `yaw`, `mirror` | Place directly. |
| `boxSelect` | `a`, `b`, `shift` | Select by inclusive cell box (pieces above the slice excluded). |
| `boxSelectScreen` | `x0`, `y0`, `x1`, `y1`, `shift` | Select by pixel rectangle. |
| `drag` | `x0`, `y0`, `x1`, `y1` | Move-group drag between two pixels. |
| `move`, `rotate`, `mirror`, `remove`, `clearSelection` | `offset` / `by` | Act on the selection (`rotate` and `mirror` act on the brush in the place tool). |
| `undo`, `redo` | | |
| `slice` | `level` (or `null`) | Show levels at most `level`. |
| `camera` | `target`, `yaw`, `pitch`, `radius` | Set the orbit. |
| `project` | `cells` | Answer `last.projected`: `[centreX, centreY, floorX, floorY]` per cell, or `null`. |
| `fill` | `count` | The `--editor-fill` grid as one undoable paste. |
| `clear` | | Remove every piece. |

`window.__parble.editor` holds `tool`, `brush`, `palette`, `pieces`, `visiblePieces`, `slice`, `selection` (count), `selected` (up to 64 anchors), `canUndo`, `canRedo`, `hover` (`source`, `hit`, `hitCell`, `hitAnchor`, `target`, `valid`, `error`, `ports`), `drag` (the live move drag's `offset` and `valid`), `colliderChunks`, `dirtyColliderChunks`, `seq`, `edits`, `last` (the last action's `id`, `op`, `ok`, `error`), `lastEdit` (`editMs` for the command, diff and hand-off, `changed`, `renderChunks`, `colliderMs`, `colliderChunks` and `colliderFrames` summed until its colliders are done) and `stats` (maxima over every edit, with the `fill` that laid the track kept apart). `kit.edit` has the renderer's `pieces`, `chunks`, `hiddenPieces` and the last respawn's `lastChunks` and `lastMs`.

`e2e/tests/editor.spec.mjs` (in `make web-e2e`) places 50 pieces with the real mouse and checks the ghost, picking, slicing, box select and a move drag. It also checks that a mirrored curve renders like the unmirrored curve of the same shape in the same cell (mirrored pieces draw reversed-winding LOD meshes, since the negative x scale would otherwise cull their outside), and that `loadTrack` and `runTrack` start the editor over on the loaded track. `make web-editor-perf` runs `e2e/tests/editor-perf.spec.mjs`, the 2,000-piece edit frame-time trace, into `target/perf/p3t4-editor.json`. It waits for the 1-minute load average to reach 15 or less and reports the sample as not taken when it does not.

## Make targets

```bash
make run-shot                        # native: PNG to target/shots/run-shot.png, then exit (non-zero unless lit)
make run-shot SHOT=docs/shots/x.png  # another path
make run-diag                        # native: run 3 s, print the state JSON, exit
make playroom-shots                  # native High shots of both playroom views into target/shots/
make shots-p1t7                      # native: per-tier shots, TAA ghost check, DoF side-by-side (P1-T7)
make marble-shots                    # native: closeup PNG per marble type and tier, two sizes, the lineup per tier and the glow bloom check, to target/shots/marbles (P1-T6, P5-T1)
make web-e2e                         # release build, then the Playwright suites (smoke, persistence, marbles, kit, flycam, editor) in GPU Chromium on 5311
make web-editor-perf                 # the P3-T4 2,000-piece edit frame-time trace against an existing target/web-dist (load-guarded)
make web-marbles                     # the marbles spec alone against an existing target/web-dist (P1-T6)
make web-smoke                       # the P0-T5 single-script smoke (random port)
```

The browser suites are local only and never run in CI. `e2e/` has its own package, pinning `@playwright/test` 1.64.1. Install it with `make e2e-deps`. Chromium launches with `channel: 'chromium'` and the platform GPU flags (`--use-angle=metal --enable-gpu --ignore-gpu-blocklist` on macOS). `PW_GPU=swiftshader` opts into the CPU renderer.

`make web-e2e` has Playwright start `python3 -m http.server` on 127.0.0.1:5311 over `dist/` and stop it afterwards. The URL switches can be tried by hand on any served build, for example `http://127.0.0.1:5310/?quality=low&dump-state&exit-after=5`.
