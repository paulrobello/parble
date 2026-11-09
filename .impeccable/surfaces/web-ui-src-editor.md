---
version: 1
slug: "web-ui-src-editor"
primary_target: "web/ui/src/editor"
related_targets: []
---

# Surface: track editor overlay (P3-T5)

Mode: Operate. A TypeScript DOM overlay (`web/ui`) over the full-screen Bevy WebGPU scene, in desktop browsers only.

## Audience and job
Casual players learn it without docs, and power builders work fast on 1,000+ piece tracks. The job is to place, rotate, mirror and delete pieces; slice by height; fix validation issues; and release marbles and watch the run. Every action is reachable by mouse with its key shown. Build and run are one loop.

## Constraints (owner, 2026-11-09)
- During a run the chrome fades to cinematic and returns when the mouse nears an edge, and editing is allowed mid-run.
- It must not feel game-y. Nothing may be keyboard-only.
- No frame cost: the overlay must hold the 60 fps budget.

## Direction contract
THESIS: The chrome is one dark walnut panel inlaid with thin maple stringing. Inlay lines divide the zones, and small brass pins mark state. It refuses the grey engine-editor shell with icon-only toolbars and permanent side docks.
OWN-WORLD: The ground is walnut #22160f at about 90% over the scene, with raised walnut #3a271b. Maple stringing #d9c2a0 draws 1px inlay rules. Brass #c49a52 marks active and selected state as a round pin. Pale blue #a8c8d8 is the secondary accent, cream #f0e6d6 is text, and red is used only for faults. The type is Alegreya Sans with small-caps section labels. Wood is flat colour plus inlay, never photographic grain.
STORY: A newcomer sees every tool with its key cap, picks a piece from the drawer, places it with the ghost preview, and presses Run. A builder slices levels, multi-selects, and clears issues from the right panel.
FIRST VIEWPORT: The 1440×900 layout:
- A 52px top inlaid strip holds, left to right: brand and track name with Library; the tool group (Select 1, Place 2, Delete 3, Rotate R, Mirror M, Undo, Redo, each with a key cap); the level stepper L0–L4; the run controls; the marble budget; and Run.
- A 170px bottom drawer holds the piece tray behind family tabs.
- A 320px right panel above the drawer holds the selection and validation.
- The scene owns the centre, with the ghost placement and its cell tooltip.
- Signature interaction: a brass pin travels along the inlay to the active tool, tab or row. State moves as the pin, not as a fill.
FORM: Walnut Marquetry, the pick card (my rank 1). seed:0b66f1a51209106c
FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
