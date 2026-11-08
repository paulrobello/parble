# Marble vane textures (P1-T6)

Colour swatches for the cat's eye marble's inner vanes: one red, one green, alternating per blade, after look-dev frame 01 (`../lookdev/01-macro-channel-cats-eye.png`). They were generated with Higgsfield `nano_banana_pro` (1k, 1:1, 1024x1024 PNG) on 2026-11-07 (01:34 UTC on 2026-11-08). The full records (prompt, job id, file hashes) are in [`provenance.json`](provenance.json). The prompts are in [`prompts/`](prompts/).

| File | Use |
|---|---|
| `vane-red.png` | Blades 0 and 2 |
| `vane-green.png` | Blades 1 and 3 |

The sources are stored exactly as delivered. The runtime derivatives are `assets/marbles/vane-{red,green}.png`, resampled to 256x256 with `sips -z 256 256`. `parble-render` embeds them with `include_bytes!`. The same textures feed the High-tier vane mesh and the Low/Medium fallback shader. Streaks run vertically, along the vane's height (UV v).

## Generation

- 2 generations, 2 kept, 0 rejected, under the 6-generation cap.
- 4 credits spent: two "Nano Banana Pro -2" transactions at 01:34 UTC, matching the two job ids. The balance went from 956 to 952.
- The provider exposes no seed.

## Gates

| Gate | Status |
|---|---|
| Technical (`node assets-source/marbles/check-provenance.mjs`) | pass |
| Creative | pending Paul's review against frame 01 |
| Rights | pending. Generated on Paul's Higgsfield plus plan. Commercial terms were not verified. The prompts were text only, with no reference images. |
| Owner | pending |
