# Marble types (P1-T6)

The physical table lives in `crates/parble-sim/src/marble.rs`. The render looks live in `crates/parble-render/src/marble/`. Select a type with `--marble <type>` natively or `?marble=<type>` on the web (see [agent-operability.md](agent-operability.md)).

## Physics table

All three are 16 mm in diameter (radius 8 mm), the kit contract's standard marble. Mass follows from density and the 2.1447 cm³ volume.

| Type (`--marble`) | Material | Density (kg/m³) | Mass (g) | Friction vs wood | Restitution vs wood | Rolling resistance |
|---|---|---|---|---|---|---|
| `cats-eye` | Soda-lime glass | 2,520 | 5.40 | 0.30 | 0.66 | 0.002 |
| `glazed` | Glazed ceramic (porcelain body, glass glaze) | 2,400 | 5.15 | 0.30 | 0.66 | 0.002 |
| `steel` | AISI 52100 chrome steel | 7,810 | 16.75 | 0.40 | 0.60 | 0.002 |
| (none) | P0 legacy marble | 2,500 | 5.36 | 0.4 (averaged with wood 0.5) | 0.6 (averaged with wood 0.4) | none |

The "solid glossy" marble is recorded as **glazed ceramic**, matching the audio cue sheet's voicing. Its glaze is a glass layer, so its contact values take the glass values.

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
- No source separates rolling resistance by ball material on wood, so all three types share 0.002.
- The friction for glass and glaze is a proxy pair, as noted above.

Refine these when the kit wood gets measured values.

## How the sim applies them

The kit is wood, so friction and restitution are marble-on-wood pair values. The marble collider sets Rapier combine rules so the pair value wins against a wood collider. Friction uses `Min`, which outranks wood's `Average`, so the contact uses the marble's value when it is at or below wood's 0.5. Restitution uses `Max`, so the contact uses the marble's value when it is at or above wood's 0.4. A unit test asserts both bounds for every type.

Rapier has no rolling-resistance model. The coefficient is applied as angular damping matched on energy loss at 0.5 m/s, a typical channel speed. With `I = 2/5·m·r²` this gives `k = 2.5·c_rr·g / v_ref`, which is 0.098 /s for `c_rr = 0.002`. The match is exact at the reference speed and approximate elsewhere.

## Determinism

Without `--marble`, the `ramp` scenario runs the **P0 legacy** marble (`marble::P0_LEGACY`). Those are the P0-T3 placeholder values with Rapier's default combine rules and no damping, so the golden hash `0c9206fc016d9b17` is unchanged. `--marble <type>` runs the same tray with that type's real values. Those runs are deterministic, but no golden is recorded for them yet.

## Render looks

| Type | High and above | Low and Medium |
|---|---|---|
| `cats-eye` | A clear sphere with Bevy screen-space specular transmission (`specular_transmission = 1`, IOR 1.5, roughness 0, thickness = radius so the screen-space lookup lands on the central vanes, no shadow casting) around opaque inner vane geometry: 4 twisted petals, alternating red and green. The camera gets `ScreenSpaceTransmission`. | An opaque sphere with the fallback shader `vane_glass.wesl`. It refracts the view ray into the marble (IOR 1.5), marches the same petal surfaces (16 steps on Low, 32 on Medium) and composites the vanes over a dim glass body. |
| `glazed` | Cobalt-blue ceramic body (roughness 0.45) under a clearcoat (1.0, roughness 0.04). | same |
| `steel` | Metallic, iron F0 (linear 0.562/0.565/0.578, Real-Time Rendering 4th ed., Table 9.2), roughness 0.06, reflecting the environment map. | same |

The environment map is a procedural night room built on the CPU (`marble/env.rs`). It has warm lantern lobes and a cool window, with every specular mip pre-convolved. It goes only on cameras that have no `EnvironmentMapLight`, as a stand-in until the P1-T5 environment probe.

The vane colours are two Higgsfield textures (`assets/marbles/`, provenance in `assets-source/marbles/`), embedded in the binary.
