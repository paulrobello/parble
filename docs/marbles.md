# Marble types (P1-T6, P5-T1)

The physical table lives in `crates/parble-sim/src/marble.rs`. The render looks live in `crates/parble-render/src/marble/`. Select a type with `--marble <type>` and a size with `--marble-size <size>` natively, or `?marble=<type>&marble-size=<size>` on the web (see [agent-operability.md](agent-operability.md)).

## Physics table

Size is a separate dimension from type: every type comes in the kit contract's three diameters, `mini` 10 mm, `standard` 16 mm and `shooter` 25 mm (`MarbleSize`, from `MARBLE_*_DIAMETER_UM` in `parble-core`). A size changes only the radius. The material values stay the type's, and mass follows from density × volume (0.5236 cm³, 2.1447 cm³ and 8.1812 cm³).

| Type (`--marble`) | Material | Density (kg/m³) | Mass mini / standard / shooter (g) | Friction vs wood | Restitution vs wood | Rolling resistance |
|---|---|---|---|---|---|---|
| `cats-eye` | Soda-lime glass | 2,520 | 1.32 / 5.40 / 20.62 | 0.30 | 0.66 | 0.002 |
| `glazed` | Glazed ceramic (porcelain body, glass glaze) | 2,400 | 1.26 / 5.15 / 19.63 | 0.30 | 0.66 | 0.002 |
| `steel` | AISI 52100 chrome steel | 7,810 | 4.09 / 16.75 / 63.90 | 0.40 | 0.60 | 0.002 |
| `swirl` (P5-T1) | Soda-lime glass, coloured glass ribbons | 2,520 | 1.32 / 5.40 / 20.62 | 0.30 | 0.66 | 0.002 |
| `onionskin` (P5-T1) | Soda-lime glass, coloured glass skin | 2,520 | 1.32 / 5.40 / 20.62 | 0.30 | 0.66 | 0.002 |
| `opal` (P5-T1) | Opal (milk) glass | 2,520 (proxy) | 1.32 / 5.40 / 20.62 | 0.30 | 0.66 | 0.002 |
| `glow` (P5-T1) | Soda-lime glass, phosphorescent core | 2,520 | 1.32 / 5.40 / 20.62 | 0.30 | 0.66 | 0.002 |
| (none) | P0 legacy marble, 16 mm only | 2,500 | 5.36 | 0.4 (averaged with wood 0.5) | 0.6 (averaged with wood 0.4) | none |

The "solid glossy" marble is recorded as **glazed ceramic**, matching the audio cue sheet's voicing. Its glaze is a glass layer, so its contact values take the glass values.

The four P5-T1 types are all glass, so they reuse the cat's eye's values and sources unchanged:

- Swirl ribbons and the onionskin skin are coloured glass fused into the same soda-lime body.
- Opal is opal (milk) glass: glass made translucent and milky by an opacifier ([Wikipedia, "Milk glass"](https://en.wikipedia.org/wiki/Milk_glass)). No source gives it its own density, so soda-lime's 2,520 kg/m³ is used as a proxy.
- Glow is clear glass around a phosphorescent core. The pigment is a small fraction of the volume, so the body is taken as all glass.

Because their specs equal the cat's eye's, a swirl, onionskin, opal or glow of a given size runs exactly the cat's eye trajectory of that size. A test asserts this both ways: equal specs give equal hashes and different specs give different hashes.

## Sources

| Value | Source | What it gives |
|---|---|---|
| Soda-lime glass density 2,520 | [Wikipedia, "Soda–lime glass"](https://en.wikipedia.org/wiki/Soda%E2%80%93lime_glass), cross-checked with [Engineering ToolBox, "Densities of Solids"](https://www.engineeringtoolbox.com/density-solids-d_1265.html) | 2.52 g/cm³ (ToolBox: common glass 2.4–2.8) |
| Porcelain density 2,400 | Engineering ToolBox, "Densities of Solids" | porcelain 2.3–2.5 g/cm³, midpoint taken |
| Chrome steel density 7,810 | [AZoM, "AISI 52100 Alloy Steel"](https://www.azom.com/article.aspx?ArticleID=6704) | 7.81 g/cm³ (ToolBox: steel 7.82) |
| Steel on wood friction 0.4 | [Engineering ToolBox, "Friction and Friction Coefficients"](https://www.engineeringtoolbox.com/friction-coefficients-d_778.html) | wood on clean metal, dry, 0.2–0.6, midpoint taken |
| Glass and glaze on wood friction 0.3 | Engineering ToolBox, "Friction and Friction Coefficients" | glass on wood is not tabulated. The nearest hard mineral pair, wood on stone (0.2–0.4), is used at its midpoint. This is a proxy. |
| Glass restitution 0.66 | [The Physics Factbook, "Coefficients of Restitution"](https://hypertextbook.com/facts/2006/restitution.shtml) | glass marble 0.658 (92 cm drop) |
| Steel restitution 0.60 | The Physics Factbook, same table | steel ball bearing 0.597 |
| Rolling resistance 0.002 | [Engineering ToolBox, "Rolling Resistance"](https://www.engineeringtoolbox.com/rolling-friction-resistance-d_1303.html) | ball bearings 0.001–0.002, upper end taken for a ball on wood |

Limits of the sources:

- The restitution values were measured on a hard concrete floor, not on wood. Wood is softer, so they are an upper bound for a marble on a wooden block.
- No source separates rolling resistance by ball material on wood, so every type shares 0.002 at every size.
- The friction for glass and glaze is a proxy pair, as noted above.

Refine these when the kit wood gets measured values.

## How the sim applies them

The kit is wood, so friction and restitution are marble-on-wood pair values. The marble collider sets Rapier combine rules so the pair value wins against a wood collider. Friction uses `Min`, which outranks wood's `Average`, so the contact uses the marble's value when it is at or below wood's 0.5. Restitution uses `Max`, so the contact uses the marble's value when it is at or above wood's 0.4. A unit test asserts both bounds for every type.

Rapier has no rolling-resistance model, so the sim applies Coulomb rolling resistance before each step (`marble::apply_rolling_resistance`). Each contact with a surface that is not another marble gives the ball an angular impulse `c_rr·r·J` against its spin, where `J` is that contact's normal impulse over the last step. A ball whose spin that impulse would reverse is stopped. On the level a rolling ball decelerates at a constant `5/7·c_rr·g`, 14.0 mm/s² for `c_rr = 0.002`, so a marble at 20 mm/s stops after about 1.4 s. Until 2026-11-08 the coefficient was angular damping matched on energy loss at 0.5 m/s, which faded at low speed and left slow marbles creeping.

## Determinism

Without `--marble`, the `ramp` scenario runs the **P0 legacy** marble (`marble::P0_LEGACY`). Those are the P0-T3 placeholder values with Rapier's default combine rules and no damping, so the golden hash `2813fade06774d5e` is unchanged. `--marble <type>` runs the same tray with that type's real values, and `--marble-size` spawns the marble 4 mm clear of the back wall at its own radius. `--marble-size standard` without `--marble` is still the legacy marble.

Every type × size ramp run (7 × 3) has a committed golden in `crates/parble-sim/tests/golden/ramp_10s_variants.hash`. The native test `ramp_10s_variants_match_golden` checks it, and `make determinism-wasm` (part of `checkall-ci`) runs the same 21 runs in the wasm build under Node and compares them line by line. The P0 golden file is untouched.

The all-pieces track (P2-T7) does not exist yet. Until it does, the ramp tray is the run every type × size is proven on: `every_type_and_size_rolls_down_the_ramp_and_stays_in_the_tray` checks every tick against the floor, walls and ends of the tray.

## Lineup

`--scenario lineup` rests every type side by side on the floor in front of the tray, with a mini and a shooter swirl at the two ends (`marble::LINEUP`), under one fixed shot (`Cues::lineup`). `make marble-shots` captures it on every tier as `target/shots/marbles/lineup-<tier>.png`. The reviewed High frame is committed as [`docs/shots/p5-t1-lineup.png`](shots/p5-t1-lineup.png); the script never rewrites it. The P1-T8 `shots-test` target does not exist yet, so the lineup is ready to be folded into it from `scripts/marble-shots.sh`.

## Render looks

| Type | High and above | Low and Medium |
|---|---|---|
| `cats-eye` | A clear sphere with Bevy screen-space specular transmission (`specular_transmission = 1`, IOR 1.5, roughness 0, thickness = radius so the screen-space lookup lands on the central vanes, no shadow casting) around opaque inner vane geometry: 4 twisted petals, alternating red and green. The camera gets `ScreenSpaceTransmission`. | An opaque sphere with the fallback shader `vane_glass.wesl`. It refracts the view ray into the marble (IOR 1.5), marches the same petal surfaces (16 steps on Low, 32 on Medium) and composites the vanes over a dim glass body. |
| `glazed` | Cobalt-blue ceramic body (roughness 0.45) under a clearcoat (1.0, roughness 0.04). | same |
| `steel` | Metallic, iron F0 (linear 0.562/0.565/0.578, Real-Time Rendering 4th ed., Table 9.2), roughness 0.06, reflecting the environment map. | same |
| `swirl` | The cat's eye structure with six wide ribbons twisted about one and a half turns pole to pole (`SWIRL_VANES`), every blade carrying the swirl ribbon texture. | `vane_glass.wesl` with the swirl vane shape and texture. |
| `onionskin` | A clear transmissive shell (thickness 0.1 × radius, so the lookup lands just under the surface) around an opaque core sphere at 0.9 × radius with the onionskin texture. | An opaque sphere with the fallback shader `core_glass.wesl`. It refracts the view ray (IOR 1.5), intersects the core sphere analytically, samples the same texture with the UV sphere's mapping and composites it over a dim glass body. |
| `opal` | A milky shell (tint periwinkle, roughness 0.08, specular transmission 0.8, diffuse transmission 0.2) around a core at 0.93 × radius with the milky blue opal texture (ref-04). | `core_glass.wesl` with the opal texture over a periwinkle body, plus a view-angle play of colour (blue, violet, teal) that grows toward the rim. |
| `glow` | A clear shell around a core at 0.7 × radius whose texture is also its emissive texture, at 25 × a saturated green-cyan (`GLOW_EMISSIVE`, `GLOW_TINT`, pre-exposure like the playroom lanterns). It blooms through the post stack's bloom on every tier. | `core_glass.wesl` writing the core colour × the same tint and gain to emissive. |

The glow bloom is measured, not eyeballed: `make marble-shots` captures glow on High and Ultra with `--bloom 1` and `--bloom 0`, and `scripts/marble-bloom-check.py` requires the mean luminance gain in a ring 1.1-1.6 silhouette radii out to be at least 2 (0-255 units) and more than twice any gain far from the marble.

The environment map is a procedural night room built on the CPU (`marble/env.rs`). It has warm lantern lobes and a cool window, with every specular mip pre-convolved. It goes only on cameras that have no `EnvironmentMapLight`, as a stand-in until the P1-T5 environment probe.

The vane colours are two Higgsfield textures (`assets/marbles/`, provenance in `assets-source/marbles/`), embedded in the binary. The P5-T1 swirl, onionskin, opal and glow patterns are four more (256² each, provenance in `assets-source/marbles/p5-t1/`, checked by `node assets-source/marbles/check-provenance.mjs p5-t1`). Every look scales with the marble's radius, so the sizes need no extra assets.
