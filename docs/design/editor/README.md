# Track editor overlay: layout and interactions (P3-T5 design)

Paul chose the direction **Walnut Marquetry** on 2026-11-09. The direction contract is in `.impeccable/surfaces/web-ui-src-editor.md`, and the product context is in `PRODUCT.md`. This document is the build spec for `web/ui/src/editor`. The overlay drives the engine editor through the existing `parbleEdit` actions (`crates/parble-app/src/editor.rs`, `Action`) and reads its state from `window.__parble.editor` (`EditorDiag`).

## Frames

| File | State |
|---|---|
| `sketch-marquetry.png` | Building: Place tool, ghost on a valid cell, splitter selected, 3 validation issues |
| `state-first-run.png` | Empty track: Start-here steps, Run disabled until a hopper exists |
| `state-multiselect-slice.png` | Box selection of 14 pieces with the slice at ≤ L2, group actions in the panel |
| `state-run-faded.png` | Running: chrome faded, hairline edge hints, run capsule at top |
| `state-run-edge-hover.png` | Running, pointer at the top edge: the top strip returns, build tools dimmed, Stop and Pause live |

The frames use the decision sketch over a playroom capture. Tile thumbnails are line drawings in the sketch, and the build swaps them for kit-rendered thumbnails.

## Layout (1440×900 reference)

All chrome is inset 12 px from the canvas edges and sits on walnut at 90% opacity, with maple stringing and brass pins. The scene stays interactive everywhere the chrome is not.

1. **Top strip, 52 px.** From left to right:
   - the brand, the track name (click to rename) and Library;
   - the tools: Select `1`, Place `2`, Delete `3`, Rotate `R`, Mirror `M`, Undo `⌘Z`, Redo `⇧⌘Z`;
   - the level stepper `[` `]`;
   - the run controls: Release one, Release all, Interval (a menu: 0.5, 1 or 2 s), Pause, Reset, Follow cam;
   - the marble budget (one pin per marble);
   - Run.
2. **Right panel, 320 px**, from below the top strip to above the drawer. It has two sections:
   - Selection, which shows piece properties, the group actions, or Start here when the track is empty.
   - Validation, which lists the issues.
   The panel collapses to a 28 px spine with a toggle (`\`) for builders who want the whole scene.
3. **Bottom drawer, 174 px.** It holds family tabs (Blocks, Rails, Mechanisms, Supports), 96 px tiles and a hint column. The drawer collapses to its 36 px tab row with `Tab`.
4. **Sound** sits above the drawer at the right. It is the existing mixer, restyled into the walnut set.

Below 1280 px wide, the run-control labels fold into icons with tooltips (key caps stay visible in the tooltips), and the hint column leaves the drawer. Desktop only, with no touch layout.

## Signature: the brass pin

State is a brass pin, never a fill. When the active tool, family tab, tile or segmented choice changes, the pin travels along the inlay line to its new item over 180 ms (ease-out). With `prefers-reduced-motion`, the pin jumps. Validation issues use pale-blue pins. Red is reserved for faults: an invalid ghost, a broken link under the cursor, and failed saves.

## Interactions

### Place
- Picking a tile sets the brush (`Brush { piece, yaw, mirror }`) and switches to Place.
- Hovering the scene shows the ghost (`Hover`). It is dashed brass where valid and red where a port will not meet or the cell is taken.
- A tooltip trails the ghost: `[x, y, z] · L2 · valid`, or the reason it is invalid.
- Click places the piece. Dragging across cells lays a run, one placement per cell entered, and the whole drag is one undo step.
- `R` and `M` act on the brush while placing and on the selection otherwise (`Rotate`, `Mirror`).
- `Esc` drops the brush and returns to Select.

### Select and move
- Click selects a piece. Shift-click adds to or removes from the selection.
- Shift-drag on empty space draws a pale-blue marquee (`BoxSelectScreen`). Its label shows the count and the level span.
- Dragging a selected piece moves the group (`Drag`). The live offset and its validity show in the trailing tooltip.
- Arrow keys nudge the selection one cell. `PgUp` and `PgDn` nudge one level.
- With one piece selected, the panel shows its ports, its level and its piece options (the splitter's alternation, a toggle's first side). With several selected, it shows their family counts, their level span, and Rotate, Mirror and Delete.

### Delete
- The Delete tool removes the piece under each click. `⌫` removes the selection.
- Deleting more than 50 pieces asks once, inline in the panel, with Undo offered after.

### Levels (slice)
- The stepper shows `L0–L4` (all levels) or `≤ L2` (sliced). `[` and `]` step it (`Slice`). `X` toggles between the last slice and all levels.
- Levels above the slice are hidden and cannot be picked, so they are never touched by mistake.

### Validation
- Issues update after every edit. Each row shows its cell, a plain sentence and **Show**.
- Show flies the camera to the cell over 450 ms (`Camera`), pulses a pale-blue outline on the piece twice and selects it.
- The section header shows the count. Zero issues reads "Track is complete" with a brass pin.
- Marble check sits under the issues: a Queue size / All sizes segmented choice and Check. It runs the release Run would make once per marble type, off the scene and over frames, and lists where marbles fall off or stall as pale-blue rows with the same Show. They are warnings, not issues, and never change the header. The release it checked is named under the rows. An edit, or a different Run release, marks the result out of date and dims the rows until Check again.

### Nudge zones
- A nudge zone is an invisible box that keeps slow marbles moving: a marble inside, slower than the zone's minimum speed along its arrow, is pushed up to it, never harder than the zone's push cap (defaults 0.2 m/s and 1 m/s², at most 2 of each). That is top-up mode, the default. The inspector's Mode radio switches a zone to Constant, which pushes every marble inside at the push rate whatever its speed; the min speed line and its Speed row are hidden then, since constant mode does not use them.
- The Nudge zones block sits under the marble check: a count, Add zone, and one row per zone with its cell, heading and speed, and Select. Add zone covers the selected pieces' cells, or the cell at the camera's centre with nothing selected. Add zone and Select switch to the Select tool, so `R` turns the zone.
- In the scene each zone is a dotted pale-blue box with a solid arrow; the selected one is brass. They are drawn only while editing, never during a run or its build.
- With the Select tool, a click that hits no piece selects the zone it passes through; the Delete tool deletes it.
- The selected zone's inspector (Selection) shows its box, heading, minimum speed and push, with segmented rows: Size (X, Y, Z one cell smaller or larger), Move (a cell west, east, north, south, a level up or down), Aim (45° either way, tilt 15° up or down), Speed (0.05 m/s) and Push (0.25 m/s²), and Delete zone. Arrows and `PgUp` `PgDn` move it, `R` turns it a quarter, `⌫` deletes it. Every change is one undo step.
- A zone and pieces are never selected together.

### Run
- Run, or `Space` when nothing has focus, releases the hopper's marbles and starts the sim.
- During a run, after 1.2 s without pointer movement, the chrome fades to cinematic over 400 ms. Hairline maple edges stay at 35%, and a small top capsule shows the marble count, the elapsed time and `Space` pause.
- The pointer within 48 px of an edge brings that edge's chrome back. Leaving it starts the 1.2 s timer again.
- While a run is on, the top strip dims the build tools to 45% but keeps them usable: editing mid-run is allowed and applies on the next sim tick. Run becomes Stop `Esc`, and Pause shows its state with a pin.
- Reset returns the marbles to the hopper and keeps the edits. Follow cam toggles the director and the orbit camera.

### Library
- Library opens a sheet over the right half: saved tracks with a thumbnail, piece count and date, plus Import and Export (`.parble`).
- Opening a track with unsaved edits asks once.

### First run
- An empty track shows Start here in the panel: three numbered steps, hopper, then a downhill run, then finish cups and Run.
- The Mechanisms tab opens with Hopper selected. Run stays disabled until a hopper exists, and its tooltip says why.

### Layered depth
Casual players see the defaults above. The power-builder affordances are drag-runs, marquee, nudge keys, the slice toggle, the collapsed spine and drawer, and the interval release. Each one is announced once, in the hint column the first time its trigger becomes relevant, and is never forced.

## Keyboard map
Every action has a visible button. Keys only shorten them.

| Key | Action |
|---|---|
| `1` `2` `3` | Select, Place, Delete |
| `R` / `⇧R` | Rotate a quarter turn clockwise / anticlockwise |
| `M` | Mirror |
| `⌘Z` / `⇧⌘Z` | Undo / Redo |
| `[` `]` / `X` | Slice down / up / toggle |
| Arrows, `PgUp` `PgDn` | Nudge the selection |
| `⌫` | Delete the selection |
| `Esc` | Drop the brush, clear the selection, or stop the run |
| `Space` | Run / Pause |
| `Tab` / `\` | Collapse the drawer / the panel |
| `F` / `C` | Fly camera / Director or orbit (existing) |

## Build constraints
- The overlay holds the 60 fps budget: no backdrop-filter blur over the canvas, no per-frame layout, and it reads `EditorDiag` at most once per animation frame.
- Fonts are self-hosted Alegreya Sans (400, 500, 700), with no Google Fonts request at runtime.
- Text contrast is at least 4.5:1 for cream and maple on 90% walnut over the darkest scene.
- Verification follows P3-T5: Vitest component tests per control, and a Playwright smoke that places, selects, slices, shows an issue, runs and fades.
