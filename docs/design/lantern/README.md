# Paper lantern brightness (ref-12)

Paul's note on the playroom render (2026-11-10): the sphere lamps are over bright against
`docs/reference/ref-12-paper-lantern-brightness.png`. In the reference the paper glows a warm
off-white, the ribs show across the whole sphere, the centre is a little brighter than the edges and
the pool on the rug is modest. Ours was a clipped white disc with washed-out ribs and a bloom halo.

There were three passes, all on 2026-11-10:

- **v1** took the shade out of clipping and added an N·V falloff.
- **v2** made the paper read as lit from inside and rebalanced the room's lights.
- **v3** is Paul's pick: the v2 paper with the original room lighting and a milder 0.5 × lantern
  light. It also gives the paper a rice-paper texture.

Images:

- `lantern-before-after.png`: reference, original and v3, each scaled to the same lantern diameter
  (308 px). Ours is the small lantern in the playroom `blocks` view.
- `lantern-wide-before-after.png`: the playroom `rug` view, original and v3.
- `showcase-before-after.png`: the `make shots-test` showcase High reference, main's next to v3's.

All shots are native, High quality:
`parble --scenario playroom --view blocks|rug --quality high --screenshot … --screenshot-after 5 --exit-after 6`.

## Change (v3 against main)

The changes are in `crates/parble-render/src/playroom.rs`, `lantern.wesl` and `palette.rs`. Bloom
is not touched: `BLOOM_INTENSITY` is 0.08 on every tier. The moon, rim, lamp, ambient and probe are
at main's values.

| What | Main | v3 |
|---|---|---|
| Paper emissive (`LANTERN_PAPER_NITS`) | 30 × warm amber | 1.2 × `LANTERN_PAPER` (amber a quarter of the way to white) |
| Rib emissive (`LANTERN_RIB_NITS`) | 5 | 0.45 |
| Falloff `mix(edge, 1, (N·V)^power)` | none | edge 0.15, power 1 |
| Inner glow `hot · (N·V)^hot_power` | none | hot 1.6, power 5 |
| Paper texture on the glow | none | grain 0.8 at 120 cycles/m (four octaves), fibres 0.3, puckers 0.5 |
| Paper albedo (linear) | 0.89, 0.79, 0.64 | 0.312, 0.276, 0.223 |
| Rib albedo (linear) | 0.21, 0.15, 0.09 | 0.086, 0.059, 0.036 |
| Shade receives shadows | yes | no (`NotShadowReceiver`) |
| Lantern lights (`LANTERN_LIGHT_SCALE`) | 26,000 / 14,000 lm | × 0.5 (13,000 / 7,000 lm) |

The lumens in `assets/playroom/layout.json` are unchanged, because `blender/playroom/probe.py` bakes
the probe from them. The 0.5 × scale is applied in code.

### Paper texture

`lantern.wesl` multiplies the transmitted glow by `1 + texture`, and the albedo by
`1 + texture / 2`. The texture has three parts:

- **Grain:** four octaves of world-space value noise, starting at 120 cycles per metre (about
  8 mm).
- **Fibres:** noise stretched 15:1 along two crossing directions, kept only where it peaks.
- **Puckers:** slow wrinkles across each band between the ribs, strongest midway between two ribs.

The noise is deterministic, and the lanterns are static, so it is the same on every frame and
every run. Each octave fades out as its period nears 2 to 4 pixels (`fwidth`). Fine detail
therefore drops away on a distant lantern or at Low, where there is no AA, instead of turning into
speckle. On the close shot the texture measures 0.016 at High and 0.018 at Low.

I did not measure shimmer with a moving camera. The fade is what guards against it.

### The dark band near the base (found in v2)

The falloff is radial, so it cannot make a top-to-bottom band. The inner light sits behind the outer
surface, so it does not light it. The cause was room light falling on the top half of the
near-white paper, plus a cast shadow over the bottom half. The fix is the low albedo and no shadows
on the shade.

## Measurements

Measured with Pillow on 8-bit sRGB:

- The shade is an ellipse mask at 0.92 of the lantern radius. The centre is d ≤ 0.3 radii and the
  edge is 0.75 to 0.92 radii.
- The up, mid and low bands are the central column at 0.55 to 0.85 radii above centre, ±0.15
  radii, and 0.55 to 0.85 radii below centre.
- The pool is the rug from 1.0 to 1.9 radii below the lantern centre.
- **Texture** is the paper's local luminance variation, measured at the reference's scale (160 px
  diameter). The luminance is high-pass filtered (minus a Gaussian blur at σ 3 px) and divided by
  that blur. The figure is the robust standard deviation (MAD × 1.4826) over the central shade
  (d ≤ 0.7), so the thin ribs do not dominate.

### Close shot (`blocks` view, High)

| Metric | Reference | Original | v2 | v3 |
|---|---|---|---|---|
| Clipped shade pixels (all channels ≥ 250) | 0 % | 87.4 % | 0 % | 0 % |
| Shade mean luminance | 182 | 253 | 198 | 197 |
| Shade mean RGB | 201, 180, 149 | 255, 253, 249 | 214, 196, 171 | 215, 195, 167 |
| Centre / edge (ratio) | 218 / 155 (1.41) | 253 / 253 (1.00) | 228 / 172 (1.33) | 227 / 170 (1.34) |
| Up / mid / low band | n/a | 253 / 253 / 253 | 196 / 226 / 182 | 193 / 224 / 182 |
| Rib contrast | 0.10 | 0.05 | 0.23 | 0.21 |
| Paper texture | 0.017 | 0.001 | 0.006 | 0.016 |
| Halo above the lantern | 12 | 28 | 0 | 0 |
| Pool on the rug under the lantern | 43 | 180 | 130 | 147 |
| Rug away from the lantern (bottom left) | n/a | 105 | 91 | 81 |
| Arch / tower blocks | n/a | 145 / 151 | 95 / 101 | 107 / 117 |
| Whole frame | n/a | 104.2 | 80.6 | 82.4 |

### Wide shot (`rug` view, High)

| Metric | Original | v2 | v3 |
|---|---|---|---|
| Whole frame mean | 61.8 | 62.7 | 52.7 |
| Rug (rows 430 and below) | 51.5 | 67.2 | 45.8 |
| Toy blocks (tower and arch) | 103.6 | 77.3 | 82.3 |
| Pool under the big lantern | 90.4 | 67.8 | 67.3 |
| Pool under the right lantern | 197.5 | 156.5 | 170.9 |

With no fill boost, v3 is darker than the original wherever the lanterns' light reached:

- The wide-shot mean is 15 % lower.
- The rug is 11 % lower.
- The blocks are 21 % lower.

The pool falls from 180 to 147. The reference's 43 is out of reach with the room's own light: with
the lanterns off, the floor there is still 46. The reference's rug is also dark, where ours is a
bright tile.

## Moved references

The `showcase` scene stands on the playroom rug, so it moved. Against main's
`docs/shots/p1-t8-showcase-*.png`, v3 diffed 0.0630 (Low), 0.0612 (Medium), 0.0610 (High) and 0.0611
(Ultra), against the 0.03 bar.

I regenerated the four references with `python3 scripts/shots/p1t8.py --update showcase` (the
script's documented update path). `showcase-before-after.png` shows main's High reference next to
v3's. The framing and track are the same; the lantern shade and the light around it changed.

Nothing else moved:

- The lineup, with the marble glow cores, diffs 0.0005 to 0.0007.
- stress2000 diffs 0.0044 to 0.0048.
- The mechanisms checks pass.

## Not changed

- **Rib count.** The reference has about 10 rib lines and our shade has 7. The geometry is
  unchanged.
- **The floor lamp's conical shade** (emissive 40) still clips in the rug view. It is not a sphere
  lamp.
