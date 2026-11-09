# Vendored parry3d 0.31.1

rapier3d 0.36.1 resolves `parry3d = 0.31.1`, and parble patches that version here through
`[patch.crates-io]` in the root `Cargo.toml`. The copy is the crates.io 0.31.1 package
(`~/.cargo/registry/src/*/parry3d-0.31.1`, committed unmodified first, minus `.cargo-ok`). It is
excluded from the parble workspace, so its own tests and lints do not run in `make checkall`.

## What changed

A per-triangle filter on the composite-shape sweep, so the CCD pass can leave out the facet
triangles that parble's exact channel contacts replace (see `crates/parble-sim/src/chunk.rs`,
`ExactHold`, and `third_party/rapier3d-0.36.1/README.parble.md`).

- `query/sweep_toi/composite.rs`: `SweepCompositeFastShape::skip_triangle:
  Option<&dyn Fn(u32) -> bool>`. In the 3D `TriMesh` arm of `sweep_time_of_impact_composite` a
  triangle the callback returns `true` for is skipped before the one-sided early-out. `None`
  sweeps every triangle, as before. Compound children recurse with `skip_triangle: None`: the
  triangle indices are those of a top-level mesh only.
- Test: `query::sweep_toi::composite::parble_tests::skipped_triangles_are_not_swept`.

Run the test from a copy outside the parble tree (Cargo otherwise finds the parble workspace):
`cp -R third_party/parry3d-0.31.1 /tmp/p && cd /tmp/p && cargo test --lib parble_tests`.
