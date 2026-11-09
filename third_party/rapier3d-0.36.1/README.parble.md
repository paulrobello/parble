# Vendored rapier3d 0.36.1

parble pins `rapier3d = "=0.36.1"` and patches it here through `[patch.crates-io]` in the root
`Cargo.toml`. The copy is the crates.io 0.36.1 package (`~/.cargo/registry/src/*/rapier3d-0.36.1`,
committed unmodified first, minus `.cargo-ok`). It is excluded from the parble workspace, so its
own tests and lints do not run in `make checkall`.

## What changed

A rolling ball sank into flat ground. Rapier finds contacts once per step and re-measures each
contact's separation per solver substep (and on recycled steps) from body-fixed anchors. A ball's
contact point stays at `centre + r·n` while the ball spins, but a body-fixed anchor rotates away
with the body and lifts by `r·(1 − cos θ)`. The solver reads that lift as separation and lets the
ball sink until it is cancelled. A 16 mm marble at 1.17 m/s turns 0.61 rad per 240 Hz step and
sank 0.65 mm into a flat trimesh, ending each tick at −20 mm/s.

The fix tracks a ball side's separation translation-only:

- `ContactManifoldData::centered_ball: [bool; 2]` (crate-private, `serde(default)`), set in
  `geometry/narrow_phase/pair_update.rs` when a solver-body side's collider shape is a ball whose
  centre is the body's centre of mass (within `1e-4·r`).
- `dynamics/solver/contact_constraint/mod.rs`: `centered_ball_lanes` (per-lane masks, false for a
  world-attached side) and `separation_points`, which rebuilds a ball side's contact point at
  build time from the body centre plus the frozen world arm (`ContactData::solver_dp1/2`) and half
  the frozen `SolverContact::dist` along the normal.
- `contact_with_twist_friction.rs` (the default `FrictionModel::Simplified`): a ball side's
  substep anchor is its body centre (local origin), with `dist` rebased to match. `update` is
  unchanged.
- `contact_with_coulomb_friction.rs`: the same rebase, and `update`/`update_rhs_wo_bias` take the
  normal separation from the body centre on ball sides. Friction keeps the body-fixed anchor: a
  rolling ball's material contact point does not slide.
- Test: `pipeline/physics_pipeline/test.rs::rolling_ball_does_not_sink_into_the_ground`.

Non-ball shapes, off-centre balls (compound children, offset colliders), capsules and multibody
contacts (`generic_contact_constraint.rs`) behave exactly as before.

Run the Rapier test from a copy outside the parble tree (Cargo otherwise finds the parble
workspace): `cp -R third_party/rapier3d-0.36.1 /tmp/r && cd /tmp/r && cargo test --lib
--features enhanced-determinism rolling_ball`.

## Per-triangle CCD filter

parble's exact channel contacts (`crates/parble-sim/src/chunk.rs`) replace the contacts of the
facet triangles under them, but the CCD pass after the solve swept the faceted mesh and clamped a
fast marble's motion against a facet it was resting on through the exact contact (0.79 mm advanced
in place of 1.56 mm on the steep slope, measured 2026-11-09). The hook lets the sweep leave out
exactly those triangles:

- `pipeline/physics_hooks.rs`: `PhysicsHooks::ccd_skip_triangle(&PairFilterContext, u32) ->
  bool`, default `false`. `collider1` is the fast collider, `collider2` the target.
- `dynamics/ccd/sweeps.rs`: for a pair `filter_contact_pair` keeps whose colliders enable
  `ActiveHooks::FILTER_CONTACT_PAIRS`, `sweep_fast_body` passes the hook through
  `cast_collider_pair` and `cast_sub_shape` to parry's `SweepCompositeFastShape::skip_triangle`
  (vendored parry3d 0.31.1, `third_party/parry3d-0.31.1/README.parble.md`). Other pairs sweep
  every triangle, as before.
- Test: `pipeline/physics_pipeline/test.rs::ccd_skip_triangle_hook_skips_only_the_named_triangles`.

The standalone copy needs the vendored parry too: append
`[patch.crates-io] parry3d = { path = "<copy of third_party/parry3d-0.31.1>" }` to its
`Cargo.toml` before `cargo test --lib --features enhanced-determinism ccd_`.
