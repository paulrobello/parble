# Marble pattern textures (P5-T1)

Pattern swatches for the four sandbox marbles added in P5-T1. They were generated with Higgsfield `nano_banana_pro` (1k, 1:1, 1024x1024 PNG) on 2026-11-07 (02:45 UTC on 2026-11-08). The full records (prompt, job id, file hashes) are in [`provenance.json`](provenance.json). The prompts are in [`prompts/`](prompts/).

| File | Marble | Use |
|---|---|---|
| `swirl.png` | swirl | Ribbon colour on all six vane blades |
| `onionskin.png` | onionskin | Colour of the layer just under the surface |
| `opal.png` | opal | Milky periwinkle body with play of colour, after `docs/reference/ref-04.webp` (look-dev frame 07 for the play of colour) |
| `glow.png` | glow | Base colour and emissive texture of the glowing core |

The sources are stored exactly as delivered. The runtime derivatives are `assets/marbles/{swirl,onionskin,opal,glow}.png`, resampled to 256x256 with `sips -z 256 256`. `parble-render` embeds them with `include_bytes!`. Each texture feeds both the High-tier meshes and the Low/Medium fallback shader of its marble (see `docs/marbles.md`).

## Generation

- 4 generations, 4 kept, 0 rejected, under the 8-generation cap.
- 8 credits spent: four "Nano Banana Pro -2" transactions at 02:45 UTC, matching the four job ids. The balance went from 950 to 942.
- The provider exposes no seed.
- The prompts were text only. The opal prompt was written while looking at ref-04 and frame 07, but neither image was uploaded.

## Gates

| Gate | Status |
|---|---|
| Technical (`node assets-source/marbles/check-provenance.mjs p5-t1`) | pass |
| Creative | pending Paul's review of the lineup shot `docs/shots/p5-t1-lineup.png` |
| Rights | pending. Generated on Paul's Higgsfield plus plan. Commercial terms were not verified. |
| Owner | pending |
