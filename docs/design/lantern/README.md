# Paper lantern brightness (ref-12)

Paul's note on the playroom render (2026-11-10): the sphere lamps are over bright against
`docs/reference/ref-12-paper-lantern-brightness.png`. In the reference the paper glows a warm
off-white, the ribs show across the whole sphere, the centre is a little brighter than the edges and
the pool on the rug is modest. Ours was a clipped white disc with washed-out ribs and a bloom halo.

Two passes, both on 2026-11-10:

- **v1** took the shade out of clipping and added an N·V falloff.
- **v2** followed Paul's review of v1. It dims the pool on the rug, with the room's other lights
  holding the overall level, and makes the shade read as paper lit from inside.

Images:

- `lantern-before-after.png`: reference, original before, and v2 after, each scaled to the same
  lantern diameter (308 px). Ours is the small lantern in the playroom `blocks` view.
- `lantern-wide-before-after.png`: the playroom `rug` view, original before and v2 after.

All shots are native, High quality:
`parble --scenario playroom --view blocks|rug --quality high --screenshot … --screenshot-after 5 --exit-after 6`.

## Change

All of it is in `crates/parble-render/src/playroom.rs`, `lantern.wesl` and `palette.rs`. Bloom is
not touched: `BLOOM_INTENSITY` is 0.08 on every tier.

| What | Original | v1 | v2 |
|---|---|---|---|
| Paper emissive (`LANTERN_PAPER_NITS`) | 30 × warm amber | 2.2 × `LANTERN_PAPER` | 1.2 × `LANTERN_PAPER` |
| Rib emissive (`LANTERN_RIB_NITS`) | 5 | 0.8 | 0.45 |
| Falloff `mix(edge, 1, (N·V)^power)` | none | edge 0, power 2 | edge 0.15, power 1 |
| Inner glow `hot · (N·V)^hot_power` | none | none | hot 1.6, power 5 |
| Paper albedo (linear) | 0.89, 0.79, 0.64 | same | 0.312, 0.276, 0.223 (× 0.35) |
| Rib albedo (linear) | 0.21, 0.15, 0.09 | same | 0.086, 0.059, 0.036 (× 0.4) |
| Shade receives shadows | yes | yes | no (`NotShadowReceiver`) |
| Lantern point lights (`LANTERN_LIGHT_SCALE`) | 26,000 / 14,000 lm | same | × 0.25 (6,500 / 3,500 lm) |
| Moon (`MOON_LUX_SCALE`) | 1,200 lux | same | × 2.2 (2,640 lux) |
| Rim (`RIM_LUX`) | 1,500 lux | same | 1,800 lux |
| Lamp (`LAMP_LUMENS_SCALE`) | 15,000 lm | same | × 3 (45,000 lm) |
| Ambient (`GlobalAmbientLight`) | 2.0 | same | 6.0 |
| Probe (`ENV_INTENSITY`) | 15 | same | 22.5 |

`LANTERN_PAPER` is warm amber a quarter of the way to white. The lumens in
`assets/playroom/layout.json` stay as they were, because `blender/playroom/probe.py` bakes the
lantern and lamp emission from them. The scales are applied in code, so the baked probe is not
stale relative to the layout.

### The dark band near the base (v1)

I checked three candidate causes:

- **The falloff.** It is radial, so it cannot make a top-to-bottom band. With the falloff off, the
  low and mid bands differ by only 6 luminance.
- **The inner point light.** It does not light the shade: the light sits behind the outer surface.
  With the paper albedo at zero, the low band does not change (166).
- **Room light and shadows.** These are the cause. The moon and lamp lit the top half of the
  near-white albedo paper (the up band fell from 199 to 177 with the albedo at zero). The lower
  half sat in cast shadow: with `NotShadowReceiver`, the low band rose from 166 to 181.

v2 gives the paper a low albedo and stops it receiving shadows. The low band is now 14 below the
up band, down from 33.

## Measurements

Measured with Pillow on 8-bit sRGB:

- The shade is an ellipse mask at 0.92 of the lantern radius. The centre is d ≤ 0.3 radii and the
  edge is 0.75 to 0.92 radii.
- The up, mid and low bands are the central 0.8-radius-wide column at 0.55 to 0.85 radii above
  centre, ±0.15 radii, and 0.55 to 0.85 radii below centre.
- Rib contrast is the mean dip of the rib rows below the paper either side, in the central band.
- Bloom reach is the distance past the silhouette, sideways, until luminance is within 5 of the
  background.
- The halo is the luminance of the ring 1.02 to 1.25 radii above the lantern, minus the ring at
  1.6 to 1.9 radii.
- The pool is the rug from 1.0 to 1.9 radii below the lantern centre.

### Close shot (`blocks` view)

| Metric | Reference | Original | v1 | v2 |
|---|---|---|---|---|
| Clipped shade pixels (all channels ≥ 250) | 0 % | 87.4 % | 0 % | 0 % |
| Shade mean luminance | 182 | 253 | 195 | 198 |
| Shade mean RGB | 201, 180, 149 | 255, 253, 249 | 212, 193, 166 | 214, 196, 171 |
| Centre luminance | 218 | 253 | 224 | 228 |
| Edge luminance | 155 | 253 | 165 | 172 |
| Centre-to-edge ratio | 1.41 | 1.00 | 1.36 | 1.33 |
| Up / mid / low band | n/a | 253 / 253 / 253 | 199 / 221 / 166 | 196 / 226 / 182 |
| Rib contrast | 0.10 | 0.05 | 0.26 | 0.23 |
| Bloom reach past the silhouette | 0.17 radii | 0.46 radii | 0.05 radii | about 0.05 radii |
| Halo above the lantern | 12 | 28 | 0 | 0 |
| Pool on the rug under the lantern | 43 | 180 | 179 | 130 |
| Rug away from the lantern (bottom left) | n/a | 105 | 105 | 91 |
| Arch block | n/a | 145 | 144 | 95 |
| Block tower | n/a | 151 | 151 | 101 |
| Whole frame | n/a | 104.2 | 96.4 | 80.6 |

### Wide shot (`rug` view)

| Metric | Original | v1 | v2 |
|---|---|---|---|
| Whole frame mean | 61.8 | 56.3 | 62.7 |
| Rug (rows 430 and below) | 51.5 | 50.3 | 67.2 |
| Toy blocks (tower and arch) | 103.6 | 102.4 | 77.3 |
| Pool under the big lantern | 90.4 | 75.7 | 67.8 |
| Pool under the right lantern | 197.5 | 196.1 | 156.5 |

## Trade-offs

**The pool cannot reach 43 while the room holds its level.** With the lantern lights off entirely,
the floor under the small lantern is still 46: that is the room's other light. Any fill that holds
the room's level raises the floor under the lantern too.

At 0.25 × the lumens the pool falls from 180 to 130. Relative to the rug around it, that is 1.43 ×,
down from 1.71 ×. I compared 0.1 to 0.4 ×, each with fill compensation.

The fill evens the scene out rather than restoring every region:

- The wide-shot mean holds: 62.7 against 61.8 originally.
- The open rug in the wide shot gets brighter, 67 against 51.
- The blocks, which the lanterns lit up close, get darker: 77 against 104 in the wide shot, and 95
  against 145 for the arch in the close shot.
- The moon is pale blue, so the room reads a little cooler than before.

The reference's rug is dark and ours is a bright yellow-green tile, so the absolute floor values do
not compare directly.

## Moved references

The `showcase` scene stands on the playroom rug under these lights, so its `make shots-test`
references moved. Against the old references, v2 diffed 0.0876 to 0.0893 across the four tiers
(bar 0.03), and v1 had diffed 0.0071 to 0.0077.

I regenerated `docs/shots/p1-t8-showcase-{low,medium,high,ultra}.png` with
`python3 scripts/shots/p1t8.py --update showcase` (the script's documented update path). Against
the new references the diffs are 0.0011 to 0.0015. The framing is the same; the lantern, its pool
and the light on the track changed.

Nothing else moved:

- The lineup, with the marble glow cores, diffs 0.0005 to 0.0007.
- stress2000 diffs 0.0044 to 0.0048.
- The mechanisms checks pass.

## Not changed

- **Rib count.** The reference has about 10 rib lines and our shade has 7. The geometry is
  unchanged.
- **The floor lamp's conical shade** (emissive 40) still clips in the rug view. It is not a sphere
  lamp. Its light was raised 3 × as fill.
