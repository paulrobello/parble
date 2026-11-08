# Agent operability

parble can be launched, observed and stopped from a script, on native and on the web, without anyone at the keyboard (P0-T6). The switches are the same on both platforms. Natively they are flags. On the web they are URL query parameters.

## Switches

| Flag (native) | Query (web) | Default | Effect |
|---|---|---|---|
| `--screenshot <path>` | `screenshot=<name>` | off | Capture the primary window once. Natively the PNG is written to `<path>` and parent directories are created. On the web the browser downloads `<name>`. |
| `--screenshot-after <s>` | `screenshot-after=<s>` | `3` | Seconds of real time after startup before the capture. The first frames show only the backdrop while pipelines compile, so stay at 3 s or more. |
| `--exit-after <s>` | `exit-after=<s>` | off | Exit after `<s>` seconds of real time. If a screenshot is still pending, the app waits for it, up to 10 s longer. |
| `--scenario <name>` | `scenario=<name>` | `ramp` | Scene to load. `ramp` is the tray ramp with one marble, restarted every 10 s. `closeup` (P1-T6) rests one marble on the tray floor under a close fixed camera, for judging marble materials. `playroom` (P1-T5) is the ramp and its marble inside the night playroom: rug, room shell, paper lanterns, a warm lamp, toy blocks, the backdrop plate and the baked environment probe. The set dressing is render-only, so `playroom` runs the ramp's sim and reports the ramp's golden hash. `kit2000` (P1-T4) adds a 2,000-piece wall of the four kit blocks (50 × 10 × 4 cells, 14 chunks) beside the ramp under a fixed `set` shot (`Cues::kit`) that shows all three LODs and both crossfade bands. It is render-only too, so it keeps the ramp's golden hash. `lineup` (P5-T1) rests every marble type side by side on the floor in front of the tray, with a mini and a shooter swirl at the two ends, under one fixed set shot. It ignores `--marble` and `--marble-size`. |
| `--marble <type>` | `marble=<type>` | legacy | Marble type ([marbles.md](marbles.md)): `cats-eye`, `glazed`, `steel` (P1-T6), `swirl`, `onionskin`, `opal` or `glow` (P5-T1), each with its real physical values and its own look. Without it the scene uses the P0 legacy marble (golden physics, cat's eye look), so `ramp` still reproduces the golden hash. |
| `--marble-size <size>` | `marble-size=<size>` | `standard` | Marble size (P5-T1): `mini` (10 mm), `standard` (16 mm) or `shooter` (25 mm). The type keeps its material, and its mass follows from density × volume. Without `--marble`, `standard` is still the legacy marble and the other sizes are a cat's eye at that size. |
| `--view <name>` | `view=<name>` | none | Playroom fixed framing, played by the director as one `set` shot (`Cues::playroom` in `camera.rs`) whose DoF focus stays on the view's lantern instead of the marble. `rug` looks low across the rug towards the big lantern (ref-07). `blocks` looks at the toy-block arches and towers around the small lantern (ref-08). Without it the playroom uses the ramp's follow cues. `--camera overview` and `orbit` take precedence. Other scenarios ignore it. |
| `--tex <family>` | `tex=<family>` | device | Caps the kit wood texture family: `bc7`, `astc` or `png`. The renderer picks BC7 KTX2 when the device has BC compression, else ASTC KTX2, else PNG (`crates/parble-render/src/kit/family.rs`). The cap only steps down that order, so `png` always forces the PNG fallback. Only `kit2000` loads the wood maps. |
| `--seed <u64>` | `seed=<u64>` | `0` | Recorded and reported. No P0 scenario uses it yet. Procedural tracks consume it from P3. |
| `--quality <tier>` | `quality=<tier>` | `medium` | `low`, `medium`, `high` or `ultra`. Per-tier AA (MSAA or TAA), SSAO, bloom, DoF and shadows are in `docs/adr/0001-post-stack-and-tiers.md`. The glass marbles (cat's eye, swirl, onionskin, opal, glow) use fallback shaders on `low` and `medium` (the vane march takes 16 and 32 steps), and screen-space specular transmission on `high` and `ultra`. Shadow-casting lights are capped per tier: 0 on Low, 1 on Medium, 3 on High and Ultra. |
| `--camera <c>` | `camera=<c>` | `director` | `director`: the follow-marble director with the scenario's cued cuts. `orbit`: the editor orbit camera (left drag, wheel). `overview`: the fixed P0 top-down view. `C` toggles director and orbit. |
| `--taa <0\|1>`, `--dof <0\|1>`, `--bloom <0\|1>`, `--aperture <f>`, `--tonemap <name>` | same keys | tier | Look-dev and test overrides of the tier's post stack. `bloom` (P5-T1) exists for the glow marble's bloom check. `tonemap`: `tony`, `agx`, `aces`, `filmic`, `sbdt`, `reinhard`. |
| `--lockstep-after <s>` | `lockstep-after=<s>` | off | Hold the sim at tick 0 for `<s>` real seconds, then advance exactly 4 ticks (1/60 s) per frame, so a frame shows the same state on every run. The run hash is unchanged. |
| `--screenshot-tick <n>` | `screenshot-tick=<n>` | off | Capture on the first frame at or past sim tick `<n>` instead of by time, then exit as soon as the PNG is written. Native captures with `--screenshot` use a 1280x720 window at scale factor 1. |
| `--dump-state` | `dump-state` (or `=1`) | off | At auto-exit, print the state JSON. Natively it goes to stdout as one line. On the web it is logged to the console as `parble state: {...}`. |

Native flags accept both `--key value` and `--key=value`. An unknown flag or a bad value prints an error and exits with code 2. Unknown query keys are ignored on the web. A bad query value logs an error and the app falls back to the defaults.

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
  "exited": true
}
```

- `ready`: the sim has ticked and at least 3 frames have run. Pixels can lag behind it while pipelines compile, so a pixel check still needs to poll.
- `tick`: sim ticks in the current run (240 Hz).
- `run`: completed 10 s runs.
- `hash`: the trajectory hash of the current run so far, as 16 hex digits. It is a string because a `u64` does not fit a JS number.
- `lastRunHash`: the hash of the last completed run. A run always stops at exactly 2,400 ticks, so for `ramp` this equals the golden hash in `crates/parble-sim/tests/golden/ramp_10s.hash`.
- `marbles`: one entry per marble in spawn order. Positions are in metres, rotation is a quaternion `[x, y, z, w]`, and `linvel` is in m/s.
- `track`: the metadata name of the `.parble` track loaded over the bridge (`loadTrack`), or `null`. A loaded track replaces the scenario in the live sim and its 10 s restarts. Its placements run as kit chunk colliders through the id table in `crates/parble-sim/src/track_run.rs` (kit ids, legacy aliases, and pieces with no collider yet). `loadTrack` refuses a track with any other piece id, or with overlapping kit pieces, with a `bridgeError`.
- `trackRun`: the last bridge `runTrack`, as `{ "ticks": 2400, "hash": "…" }`, or `null`. The run is headless from tick 0, so its hash does not depend on frame timing. `web/ui/src/parble-fixtures.json` records the expected `runHash` of every valid fixture track; `make web-e2e` checks it in the browser (`e2e/tests/persistence.spec.mjs`).
- `marble`: the `--marble` type, `legacy` for the P0 marble, or `lineup` in the lineup scenario.
- `marbleSize`: the `--marble-size` value (P5-T1).
- `shadowLights`, `shadowLightCap`: lights casting shadows this frame and the tier's cap (0 on Low, 1 on Medium, 3 on High and Ultra). `enforce_shadow_cap` in `crates/parble-render/src/playroom.rs` turns off the lowest-priority shadows past the cap in every scenario. `playroom` on High uses all 3: the moon, the big lantern and the lamp.
- `view`: the playroom `--view` framing, or `null` when none was given.
- `kit` (only with `--scenario kit2000`): the P1-T4 renderer. `pieces`, `chunks`, `pieceTypes` and `drawBound` (one draw per occupied (type × chunk) × 3 LODs). `variants` is the number of wood variants and `variantPieces` the pieces using each (picked by a hash of the anchor cell). Each (type, LOD, variant) is one material, so the wall draws as `pieceTypes` × 3 × `variants` bins. `lodTriangles` per piece type and `lodBandsM`, the camera distances of the two crossfade bands. `family` and `textures` (`path`, `kind`, `state`, the loaded `format`): `kind` is `wood` for each variant's base_color and orm in the device's family, or `baked` for a piece's per-LOD AO and normal PNG. `draws` is read from the render world each frame: `main` is the main opaque pass's kit draws (one indirect draw per (pipeline, material, mesh) bin) over `mainViews` views, `mainInstances` the LOD instances in those bins and `ditheredInstances` the ones drawn with the crossfade dither pipeline. With GPU culling Bevy keeps its multidraw bins private, so `main` rebuilds them from the view's visible kit instances. `prepass` and `shadow` are `null` when that pass's bins cannot be rebuilt (shadow cascades). `multidrawSets` counts the main pass's multidraw batch sets.
- `screenshot`: `state` is one of `none`, `pending`, `capturing`, `done` (with `lit`, `colors` and `ok`), or `failed` (with `error`).
- `camera`: the main camera this frame: `mode`, `shot`, `cuts`, `eye`, the marble's screen position over the last frames (`marblePx`, physical pixels, newest first), `marbleRadiusPx`, and `post`, which holds the post components the live camera carries (`msaa`, `taa`, `ssao`, `bloom`, `dof`, `aperture`, `focusM`, `tonemapping`).
- `shotCamera`: the same object captured on the screenshot frame.
- `exited`: set when auto-exit fires.

All timing (screenshot, exit, fps) is real time in the app layer. The sim never receives wall-clock input, so none of these switches can change a trajectory hash.

## Make targets

```bash
make run-shot                        # native: PNG to target/shots/run-shot.png, then exit (non-zero unless lit)
make run-shot SHOT=docs/shots/x.png  # another path
make run-diag                        # native: run 3 s, print the state JSON, exit
make playroom-shots                  # native High shots of both playroom views into target/shots/
make shots-p1t7                      # native: per-tier shots, TAA ghost check, DoF side-by-side (P1-T7)
make marble-shots                    # native: closeup PNG per marble type and tier, two sizes, the lineup per tier and the glow bloom check, to target/shots/marbles (P1-T6, P5-T1)
make web-e2e                         # release build, then the Playwright suites (smoke, persistence, marbles) in GPU Chromium on 5311
make web-marbles                     # the marbles spec alone against an existing dist/ (P1-T6)
make web-smoke                       # the P0-T5 single-script smoke (random port)
```

The browser suites are local only and never run in CI. `e2e/` has its own package, pinning `@playwright/test` 1.64.1. Install it with `make e2e-deps`. Chromium launches with `channel: 'chromium'` and the platform GPU flags (`--use-angle=metal --enable-gpu --ignore-gpu-blocklist` on macOS). `PW_GPU=swiftshader` opts into the CPU renderer.

`make web-e2e` has Playwright start `python3 -m http.server` on 127.0.0.1:5311 over `dist/` and stop it afterwards. The URL switches can be tried by hand on any served build, for example `http://127.0.0.1:5310/?quality=low&dump-state&exit-after=5`.
