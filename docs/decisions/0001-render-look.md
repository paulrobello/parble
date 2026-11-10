# ADR 0001: Render look sign-off (P1-T9)

Status: **accepted**. Paul signed off the render look on 2026-11-10.

His review (Q19) set the showcase shots next to `ref-01` to `ref-04` and `ref-07`. He asked for one change, the paper lanterns, against a new reference (`docs/reference/ref-12-paper-lantern-brightness.png`). The fix went through three rounds:
- v1: "reduce brightness of the sphere lamps";
- v2: "2 and 3", dim the floor pool and make the shade glow from inside;
- v3: "v2 paper, old room", then "the paper texture needs more texture".

He answered "Approve v3" (main f4ce95c). When asked whether that also signs off the whole render look, he answered "Yes, write the ADR".

This record collects the look as shipped. The post stack, the tiers and the camera are specified in `docs/adr/0001-post-stack-and-tiers.md`, and this ADR does not repeat their reasoning.

## Post settings

Code: `crates/parble-render/src/post.rs` (`Quality::post`).

| Tier | Anti-aliasing | SSAO | Bloom | Depth of field | Shadow map | Shadow casters |
|---|---|---|---|---|---|---|
| Low | none | off | natural, 0.08 | off | off | 0 |
| Medium | MSAA 4x | off | natural, 0.08 | Gaussian, f/4.0, CoC ≤ 24 px | 2048² | 1 |
| High | TAA | Medium | natural, 0.08 | bokeh, f/2.8, CoC ≤ 48 px | 4096² | 3 |
| Ultra | TAA | High | natural, 0.08 | bokeh, f/2.0, CoC ≤ 64 px | 4096² | 3 |

- Tonemapping is ACES fitted on every tier. It matched ref-01 best in the look-dev comparison.
- The DoF focus follows the marble's depth every frame, with `max_depth` at 4 m.
- The tier is picked by `?quality=` or `--quality`. With neither, the title screen's first-run GPU detection picks it (P5-T3). Its thresholds are still to be re-measured under the load guard (card 01a1c67417ff).

## Light budget

The colours are in `crates/parble-render/src/palette.rs`. Every scene mixes a warm key with a pale blue fill or rim (PRD §5.5). Fill and rim lights never cast shadows, so the per-tier caster caps above hold.

| Scene | Warm | Pale blue | Casters at High |
|---|---|---|---|
| Plain sets (ramp, closeup, lineup, mechanisms, allpieces, kit2000, editor) | key sun `WARM_KEY`, 13,500 lux | fill sun 4,500 lux; ambient `PALE_BLUE` 500; night-blue backdrop; navy floor, reflectance 0.15 | 1 (key) |
| Playroom | lanterns at 0.5 × `assets/playroom/layout.json` lumens (`LANTERN_LIGHT_SCALE`); floor lamp 15,000 lm | moon 1,200 lux; rim sun 1,500 lux (`RIM_LUX`); ambient 2.0; environment probe 15 (`ENV_INTENSITY`); backdrop plate 5 nits | 3 (moon, big lantern, lamp) |

### Paper lanterns

The look ships as v3, in `crates/parble-render/src/playroom.rs` and `lantern.wesl`.

| Setting | Value |
|---|---|
| Shade material | `LanternMaterial`, a StandardMaterial extension, that receives no shadows |
| Paper glow | `LANTERN_PAPER` at 1.2 nits; edge 0.15 with falloff power 1; inner hot spot 1.6 × (N·V)^5 |
| Paper texture | world-space value-noise grain from 120 cycles/m, amplitude 0.8; fibres 0.3; puckers between the ribs 0.5 |
| Ribs | 0.45 nits |

Measured on the native close shot (blocks view, High):

| Metric | Reference ref-12 | Original | v3 |
|---|---|---|---|
| Clipped shade pixels | 0 % | 87.4 % | 0 % |
| Shade mean | 182 | 253 | 197 |
| Centre / edge | 1.41 | 1.00 | 1.34 |
| Paper texture, high-pass robust standard deviation | 0.017 | — | 0.016 |
| Halo above the lantern | 12 | 28 | 0 |
| Pool under the lantern | 43 | 180 | 147 |

The playroom is darker than before the lantern change: the wide-shot frame mean went from 61.8 to 52.7. Paul chose this over a fill boost that flattened the room. Images and method: `docs/design/lantern/`.

### Colour balance

Measured as blue / red on the native High shots, 2026-11-08:
- Lit wood stays warm: mechanisms front face 0.59, top 0.56.
- Faces the key misses go cool: 0.84.
- The floor reads navy: 5.3.
- The near playroom rug is 1.27, and the lantern-lit arch stays warm at 0.46.

Glow and cat's-eye marbles are checked by measured margins in `e2e/tests/marbles.spec.mjs`.

## Measured numbers

| Check | Result | Bar | Where |
|---|---|---|---|
| Perf, headless GPU Chromium, 1440p High (gpu-host-b M4, build c86a1ef, 2026-11-09) | showcase p95 16.8 ms, max 33.4 ms; stress2000 p95 16.8 ms, max 33.3 ms; 1 of 600 frames dropped each; load 1.7–2.3 | p95 ≤ 16.7 ms + 0.3 ms quantum | `docs/perf/p1-gate.json` |
| Perf, Safari 27.2, 1440p High (gpu-host-b M4, main 9e4b2fb, 2026-11-10) | showcase and stress2000 both p95 17.0 ms, max 19 ms, 0 of 600 dropped; load 1.4–3.4 | same | `docs/perf/p1-gate-safari.json` |
| TAA ghosting, follow camera at about 1.2 m/s (native, 2026-11-07) | follow-peak 0.000, trackside-cross 0.199; a synthetic ghost control reads 5.86 | ≤ 0.25 marble disc areas | ADR 0001 post stack, `docs/shots/p1-t7-ghost-*.png` |
| Wasm size (main f4ce95c) | 11.89 MB gzip (29.0 MB raw) | ≤ 15 MB gzip | `make web-size` |
| Look-dev shot references | showcase, stress2000, lineup and mechanisms all within their bars on main f4ce95c (`make shots-test` exit 0) | showcase 0.03 | `docs/shots/` |

Caveats:
- The perf numbers predate the lantern v3 shader. It adds one material pipeline, which the READY warm-up compiles before sampling, and it costs per pixel only on the lantern shades. A perf-gate rerun on the current main is still owed.
- The PRD box names an M4 Max. The gating runs above are on gpu-host-b, an Apple M4.
- Safari's earlier one-time 1.1 s frame was a pipeline created mid-run. The READY warm-up fixed it (`docs/perf/README.md`).

## Consequences

- PRD §6 P1's owner look-approval box can be checked against this record.
- Later look changes go through the same review: a reference, before/after images, measured numbers and Paul's approval. Any look-dev shot reference that moves is listed and shown to him before it merges.
