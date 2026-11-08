# ADR 0001: Post stack, quality tiers and follow camera (P1-T7)

Status: accepted for P1-T7. The DoF strength awaits Paul's review against ref-01 (`docs/shots/p1-t7-dof-vs-ref01.png`). P1-T9 re-reviews the whole look once the environment (P1-T5) and the marble materials (P1-T6) land.

## Bevy features

No feature changes were needed. `3d_bevy_render` already enables `bevy_anti_alias` (TAA), `bevy_post_process` (bloom, DoF), `bevy_pbr` (SSAO) and, through `3d_api`, `tonemapping_luts`. The wasm growth is code only.

## Per-tier settings

Code: `crates/parble-render/src/post.rs` (`Quality::post`). Tier selection: `--quality` / `?quality=low|medium|high|ultra`.

| Tier | Anti-aliasing | SSAO | Bloom | Depth of field | Shadow map |
|---|---|---|---|---|---|
| Low | none (`Msaa::Off`) | off | natural, intensity 0.08 | off | off |
| Medium | MSAA 4x | off | natural, intensity 0.08 | Gaussian, f/4.0, CoC ≤ 24 px | 2048² |
| High | TAA (`Msaa::Off`) | Medium quality | natural, intensity 0.08 | bokeh, f/2.8, CoC ≤ 48 px | 4096² |
| Ultra | TAA (`Msaa::Off`) | High quality | natural, intensity 0.08 | bokeh, f/2.0, CoC ≤ 64 px | 4096² |

- TAA and MSAA are mutually exclusive in Bevy, and SSAO also requires `Msaa::Off` (Bevy logs an error otherwise). So MSAA appears only on Medium, which has no SSAO. TAA and SSAO share High and Ultra.
- The DoF focus is the marble's view-space depth every frame. `max_depth` is 4 m, so the backdrop at infinity gets a capped blur.
- Tonemapping: **ACES fitted** on every tier. In the look-dev comparison at the follow shot (TonyMcMapface, AgX, ACES fitted), ACES matched ref-01 best, with deep shadows, strong contrast and warm wood. TonyMcMapface was flatter, and AgX washed the wood out. ACES pushes bright blues towards magenta, so re-check it against P1-T6's glass marbles. `--tonemap tony|agx|aces|filmic|sbdt|reinhard` switches the tonemapper for that review.
- `--taa 0`, `--dof 0` and `--aperture <f>` are look-dev and test overrides. `--taa 0` drops a TAA tier to no AA, never to MSAA, so SSAO keeps running.

## Camera

Code: `crates/parble-render/src/camera.rs`. It is driven from the app layer: the app writes `DirectorClock` (tick, run, frame dt) after each sim step. The camera reads `MarbleFrame` and never writes to the sim. The determinism golden `0c9206fc016d9b17` is unchanged.

- Follow shot. The camera sits 0.14 m behind the marble, 0.07 m above it and 0.010 m to the side, which keeps it inside the 28 mm channel so the near wall never hides the marble. It looks 0.05 m ahead. The eye and the look point each follow a critically damped spring, with smoothing times of 0.15 s and 0.06 s. Each spring target leads by velocity × smoothing time, which cancels the steady lag at constant speed, so the springs damp only changes such as the ramp foot and wall rebounds.
- Cues. `Cues` is a list of `(tick, Shot)`, re-armed every run. The ramp scenario follows the marble from tick 0, cuts at tick 140 to a steep fixed trackside shot where the marble crosses the frame at about 1.17 m/s, and cuts back to the follow shot at tick 260. A cut snaps the springs and sets `TemporalAntiAliasing.reset`, so the previous shot's history never ghosts into the next one.
- `--camera director|orbit|overview`. Orbit is the editor camera: left drag orbits and the wheel zooms. `C` toggles between the director and orbit.

## TAA ghost check

`make shots-p1t7` runs `scripts/shots/p1t7.py` on the native build at 1280x720 physical.

- **Lockstep.** `--lockstep-after 8` holds the sim at tick 0 for 8 s while the pipelines compile, then advances exactly 4 ticks per frame. `--screenshot-tick` therefore captures the same state and the same camera on every run.
- **Measurement.** The same frame is rendered at High with TAA and again with `--taa 0`. The trail mask covers the marble's screen positions over the previous 7 frames, minus its current disc. Within the mask, a pixel counts when its largest channel differs by more than 0.05. The same shape mirrored ahead of the marble is the noise floor. The metric is the count of differing trail pixels minus the noise floor, divided by the marble disc area.
- **Threshold.** The metric must be ≤ 0.25 marble disc areas.
- **Positive control.** The no-TAA frame with the marble blended at 50 % into its previous positions must exceed the threshold. It measured 5.86, so a pass is not vacuous. The control runs only when the marble moved at least one diameter on screen.

Measured 2026-11-07, Apple M4 Max on native Metal:

| Case | Tick | Speed | Trail travel | Metric | Synthetic ghost |
|---|---|---|---|---|---|
| follow-peak (follow shot at peak speed) | 100 | 1.20 m/s | 5 px (marble held in frame) | 0.000 | n/a |
| trackside-cross (cue shot, marble crossing) | 168 | 1.17 m/s | ~630 px | 0.199 | 5.856 |

Peak speed in the ramp scene is about 1.24 m/s, at the ramp foot.
