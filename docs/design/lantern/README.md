# Paper lantern brightness (ref-12)

Paul's note on the playroom render (2026-11-10): the sphere lamps are over bright against
`docs/reference/ref-12-paper-lantern-brightness.png`. In the reference the paper glows a warm
off-white, the ribs show across the whole sphere, the centre is a little brighter than the edges and
the pool on the rug is modest. Ours was a clipped white disc with washed-out ribs and a bloom halo.

- `lantern-before-after.png`: reference, before and after, each scaled to the same lantern diameter
  (308 px). Ours is the small lantern in the playroom `blocks` view.
- `lantern-wide-before-after.png`: the playroom `rug` view, before and after.

Both shots are native, High quality:
`parble --scenario playroom --view blocks|rug --quality high --screenshot … --screenshot-after 5 --exit-after 6`.

## Change

The shade was emitting 30 cd/m² of warm amber, about 30 times past the ACES fitted shoulder, so
every pixel clipped and the bloom spilled past the silhouette. The fix is in the shade and ribs
only (`crates/parble-render/src/playroom.rs`):

- Paper emissive goes from 30 to 2.2 (`LANTERN_PAPER_NITS`). Its colour moves from warm amber to
  `LANTERN_PAPER` (`palette.rs`), which is amber a quarter of the way to white.
- Rib emissive goes from 5 to 0.8 (`LANTERN_RIB_NITS`), so the ribs read as darker lines.
- The shade and ribs use `LanternMaterial`, which is StandardMaterial plus `lantern.wesl`. The
  shader scales the emissive by `mix(edge, 1, (N·V)^power)`, with edge 0 and power 2, so the paper
  is brightest face on and dims towards the silhouette.
- The lantern point lights (26,000 and 14,000 lm), the lamp and the bloom settings are unchanged
  (`BLOOM_INTENSITY` 0.08 on every tier).

## Measurements

Measured with Pillow on 8-bit sRGB. The shade is an ellipse mask at 0.92 of the lantern radius. The
centre is d ≤ 0.3 radii and the edge is 0.75 to 0.92 radii. Rib contrast is the mean dip of the rib
rows below the paper either side, in the central vertical band. Bloom reach is the distance past the
silhouette, sideways, until luminance is within 5 of the background. Halo is the luminance of the
ring 1.02 to 1.25 radii above the lantern minus the ring at 1.6 to 1.9 radii.

| Metric | Reference | Before | After |
|---|---|---|---|
| Clipped shade pixels (all channels ≥ 250) | 0 % | 87.4 % | 0 % |
| Shade mean luminance | 182 | 253 | 195 |
| Shade mean RGB | 201, 180, 149 | 255, 253, 249 | 212, 193, 166 |
| Centre luminance | 218 | 253 | 224 |
| Edge luminance | 155 | 253 | 165 |
| Centre-to-edge ratio | 1.41 | 1.00 | 1.36 |
| Ribs found in the central band | 10 | 5 | 5 |
| Rib contrast | 0.10 | 0.05 | 0.26 |
| Bloom reach past the silhouette | 0.17 radii | 0.46 radii | 0.05 radii |
| Halo above the lantern | 12 | 28 | 0 |
| Floor mean luminance under the lantern | 43 | 180 | 179 |

The reference is a small, soft photo (215 × 208 px, JPEG). Its blur lowers the measured rib
contrast and lifts the halo and bloom reach, so the after numbers there are expected to sit on the
other side of it.

Wide `rug` view, whole-frame mean luminance: 61.8 before, 56.3 after. The rug alone (rows 430 and
below) is 51.5 before and 50.3 after. The drop is the lanterns' own pixels and their bloom haze;
the light the lanterns and lamp put on the rug is unchanged.

## Not changed, and why

- **The pool of light on the rug.** It is the lanterns' point lights, not the shade emissive. With
  the lantern lights at zero the floor under the small lantern falls from 179 to 46, so the lights
  make the pool. Scaling them by 0.65 only took it to 159, while darkening the whole playroom, so
  the lumens stay as they were. Our rug is a bright yellow-green tile and the reference's is dark,
  so their absolute floor values do not compare.
- **Rib count.** The reference has about 10 rib lines and our shade has 7. The geometry is
  unchanged.
- **The floor lamp's conical shade** (emissive 40) still clips in the rug view. It is not a sphere
  lamp, so it is outside this change.
