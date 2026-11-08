# Night playroom sources (P1-T5)

Sources and review images for the `--scenario playroom` environment. Runtime derivatives are in `assets/playroom/` and are embedded in the binary by `crates/parble-render/src/playroom.rs`. The full record (generators, sources and runtime files with sha256, the Higgsfield job, the rug and probe contracts) is [`provenance.json`](provenance.json). `python3 blender/playroom/provenance.py check` verifies every hash, and `make playroom-build` rebuilds everything.

| Asset | Source | Route | Runtime |
|---|---|---|---|
| Rug material | `rug/` (1024² base colour, normal, height, seed 707) | `blender/playroom/rug.py`: scripted, seamless by construction (integer cell grid and wrap-around lattice noise). 16 × 4 cm square-in-square cells per 0.64 m tile, after ref-07's square grid rather than look-dev frame 03's weave. | `rug.base_color.ktx2` (512², sRGB) and `rug.normal.ktx2` (256², linear, OpenGL +Y), RGBA8, zstd, full mips. Roughness 0.92 is a material constant. |
| Backdrop plate | `backdrop/backdrop-01.png` (Higgsfield `nano_banana_pro`, 2752×1536, job `4f8509a2`) | Prompt in `backdrop/prompts/`. Background only: no rug, lanterns or marbles. | `backdrop.ktx2`: 1024×576, extra blur, emissive plate in front of the back wall. |
| Toy blocks | none (generator only) | `blender/playroom/blocks.py`, headless Blender: cube, brick, plank, pillar, roof and arch on a 3 cm unit, bevelled, one pale-beech material. Render-only, never colliders. | `toy_*.glb`, plain glTF (no Draco or meshopt). |
| Environment probe | `probe/probe-faces.png` (review strip of the six mirror faces) | `blender/playroom/probe.py`: rebuilds the room from `assets/playroom/layout.json`, renders six 128² Cycles cube faces, prefilters on the CPU. | `env_specular.rgba16f` (64², 7 mips) and `env_diffuse.rgba16f` (16²), raw RGBA16F, attached to the camera as an `EnvironmentMapLight`. |

Lanterns, the lamp and the room shell are built from Bevy primitives in code. Their placement and light levels, and every block's position, come from `assets/playroom/layout.json`, which the probe bake reads too.

## Generation

- 1 Higgsfield generation of the 10 allowed, kept. 2 credits (balance 952 before, 950 after).
- No seed is exposed by the provider.

## Gates

| Gate | Status |
|---|---|
| Technical | `cargo test -p parble-render` (KTX2 decode, cubemap sizes, layout keeps clear of the tray, shadow cap), provenance check |
| Creative and owner | pending: Paul reviews `docs/shots/p1-t5-vs-ref07-ref08.png` |
| Rights | pending: the plate was generated on Paul's Higgsfield plus plan and commercial terms were not verified. Everything else is original scripted output. |
