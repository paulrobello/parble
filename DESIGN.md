---
name: parble
description: Walnut Marquetry editor chrome over a night-playroom marble run
colors:
  walnut: "#22160f"
  walnut-veil: "rgba(34,22,15,.9)"
  walnut-raised: "#3a271b"
  maple-stringing: "#d9c2a0"
  stringing-rule: "rgba(217,194,160,.28)"
  brass: "#c49a52"
  brass-lit: "#d4ab62"
  run-ink: "#1d1209"
  pale-blue: "#a8c8d8"
  cream: "#f0e6d6"
  fault-red: "#e0705a"
  fault-text: "#f6d2c8"
typography:
  display:
    fontFamily: "Alegreya Sans, ui-sans-serif, system-ui, sans-serif"
    fontSize: "22px"
    fontWeight: 700
    lineHeight: 1.25
    letterSpacing: "0.01em"
  headline:
    fontFamily: "Alegreya Sans, ui-sans-serif, system-ui, sans-serif"
    fontSize: "17px"
    fontWeight: 700
    lineHeight: 1.25
  title:
    fontFamily: "Alegreya Sans, ui-sans-serif, system-ui, sans-serif"
    fontSize: "15px"
    fontWeight: 700
    lineHeight: 1.25
    fontFeature: "\"lnum\" 1, \"tnum\" 1"
  body:
    fontFamily: "Alegreya Sans, ui-sans-serif, system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.25
    fontFeature: "\"lnum\" 1"
  body-small:
    fontFamily: "Alegreya Sans, ui-sans-serif, system-ui, sans-serif"
    fontSize: "12px"
    fontWeight: 400
    lineHeight: 1.35
  label:
    fontFamily: "Alegreya Sans, ui-sans-serif, system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 400
    letterSpacing: "0.07em"
    fontFeature: "all-small-caps"
  key-cap:
    fontFamily: "Alegreya Sans, sans-serif"
    fontSize: "11px"
    fontWeight: 500
    lineHeight: 1
    fontFeature: "\"tnum\" 1"
rounded:
  key: "3px"
  control: "4px"
  tile: "5px"
  panel: "6px"
  pin: "50%"
spacing:
  hair: "1px"
  xs: "4px"
  sm: "6px"
  md: "10px"
  lg: "14px"
  xl: "18px"
  edge: "12px"
components:
  inlay-panel:
    backgroundColor: "{colors.walnut-veil}"
    textColor: "{colors.cream}"
    rounded: "{rounded.panel}"
  tool-button:
    textColor: "{colors.cream}"
    rounded: "{rounded.control}"
    padding: "4px"
  tool-button-active:
    backgroundColor: "{colors.walnut-raised}"
    textColor: "{colors.cream}"
    rounded: "{rounded.control}"
  button-run:
    backgroundColor: "{colors.brass}"
    textColor: "{colors.run-ink}"
    rounded: "{rounded.control}"
    padding: "8px 14px"
  button-run-hover:
    backgroundColor: "{colors.brass-lit}"
  button-quiet:
    textColor: "{colors.cream}"
    rounded: "{rounded.control}"
    padding: "6px 10px"
  button-warn:
    textColor: "{colors.fault-text}"
    rounded: "{rounded.control}"
    padding: "6px 10px"
  key-cap:
    textColor: "{colors.maple-stringing}"
    typography: "{typography.key-cap}"
    rounded: "{rounded.key}"
    padding: "2px 3px"
  drawer-tab:
    textColor: "{colors.maple-stringing}"
    padding: "6px 16px 7px 22px"
  drawer-tab-active:
    backgroundColor: "{colors.walnut-raised}"
    textColor: "{colors.cream}"
  piece-tile:
    rounded: "{rounded.tile}"
    size: "96px"
  piece-tile-active:
    backgroundColor: "{colors.walnut-raised}"
    rounded: "{rounded.tile}"
    size: "96px"
  issue-link:
    textColor: "{colors.pale-blue}"
    padding: "4px"
---

# Design System: parble

## Overview

**Creative North Star: "Walnut Marquetry"**

This system covers the track editor's chrome: the overlay over the full-screen WebGPU scene, scoped to `#pe` in `web/ui/src/editor/style.ts`. Its surfaces are dark walnut panels with thin maple stringing inlaid in them. Each panel floats 12px in from the viewport edge, so the scene shows on all four sides. The chrome is flat colour with inlay lines. Brass pins mark state, and a single brass pin travels along the inlay to the active tool, tab or tile. The wooden track pieces, both in the scene and in the drawer thumbnails, carry the real wood grain. The chrome never imitates them.

The chrome frames the run and does not compete with it. During a run it fades to a cinematic frame: thin maple hairlines, plus a small RUNNING capsule centred at the top. Each panel comes back when the pointer nears its edge. Density is that of a working tool. Every tool shows its key cap beside its label, and labels stay visible down to 1280px wide.

The system rejects the grey engine-editor shell: icon-only toolbars, permanent side docks, and photographic wood on chrome surfaces.

**Key Characteristics:**
- Walnut panels at 90% opacity over the scene, each with a 1px maple outer rule and a fainter inner stringing line inset 4px.
- State is shown by a raised-walnut fill with a brass edge, and the traveling brass pin sits on top of it.
- Every action button carries a label, and every keyed action shows its key cap.
- One typeface, Alegreya Sans, self-hosted at weights 400, 500 and 700. Labels are set in small caps.
- The chrome fades to hairlines during a run and returns when the pointer reaches an edge.

## Colors

The chrome is warm wood darks lit by one metal (brass). It has one cool accent (pale blue) and a fault red kept for errors and destructive actions.

### Primary
- **Brass** (`brass`): the state metal. Used for the round pins on the active tool, tab and tile, for the edges of active controls, for the filled Run button, for the caret and text selection, and for the marble budget pins. Brighter **Lit Brass** (`brass-lit`) is the Run button's hover. **Run Ink** (`run-ink`) is the near-black text on the Run button.

### Secondary
- **Pale Blue** (`pale-blue`): the cool accent and the colour of help. It covers focus rings, validation issue pins and their cell coordinates, the "Show" links, the box-select marquee, and in the engine the selection mark and linked ports.

### Tertiary
- **Fault Red** (`fault-red`): the invalid ghost and its tooltip edge, broken ports in the engine, and the border of destructive confirmation buttons ("Delete N", "Open anyway"). Their text uses **Fault Blush** (`fault-text`).

### Neutral
- **Walnut** (`walnut`): the ground colour. Panels use **Walnut Veil** (`walnut-veil`), the same hue at 90% opacity, so the scene glows faintly through.
- **Raised Walnut** (`walnut-raised`): the fill for active and hovered controls, the open drawer tab, and the selected tile.
- **Maple Stringing** (`maple-stringing`): small-caps labels, secondary text, key-cap glyphs, and the run-mode hairlines. Panel outer borders use it at 55% opacity.
- **Stringing Rule** (`stringing-rule`): the 1px inlay rule at 28% opacity. It forms the dividers between zones, separators, segment splits, tile borders and scrollbar thumbs. Issue rows and library rows use a fainter version at 14%.
- **Cream** (`cream`): primary text.

### Engine tints (crates/parble-render/src/kit/edit.rs)
The 3D edit visuals use the same palette as translucent materials:
- valid ghost: brass at alpha .42
- invalid ghost: fault red at .45
- port dots at .95: pale blue for a linked port, brass for an open port, red for a fault
- selection mark: pale blue at .28
- fault mark: red at .28

### Named Rules
**The Help Is Blue Rule.** Validation issues are help, not scolding. Their pins, coordinates and Show links are pale blue. Red is reserved for an invalid placement, a broken port, and a destructive confirmation.

**The One Metal Rule.** Brass is the only colour that means "active" or "go". Do not introduce a second accent for state.

## Typography

**Display Font:** Alegreya Sans (self-hosted woff2 at 400, 500 and 700, falling back to ui-sans-serif and system-ui)
**Label Font:** Alegreya Sans in all small caps, tracked at 0.07em

**Character:** one humanist sans with calligraphic roots. It is warm enough for the wood and plain enough for a tool. Hierarchy comes from weight and small caps, not from a second family.

### Hierarchy
- **Display** (700, 22px): the "parble" wordmark in the strip only.
- **Headline** (700, 17px): panel section headings. Each heading opens with a 15px weight-500 small-caps prefix, such as "Selected:", "Validation ·" or "Placing:".
- **Title** (700, 15px, lining tabular numerals): important numbers and the primary action. Used for the level range, the marble count, the Run label and step numerals.
- **Body** (400, 13px, line-height 1.25, lining numerals): the base for all chrome text. Drawer tab labels step up to 14px.
- **Body Small** (400, 12px, line-height 1.35): notes, tool captions, metadata and segment labels.
- **Label** (small caps, 0.07em tracking, maple): field labels in key-value grids, control labels ("Levels", "Marbles"), heading prefixes, and the RUNNING status.
- **Key Cap** (500, 11px, tabular numerals): key hints inside `kbd`.

### Named Rules
**The Lining Numerals Rule.** Every number in the chrome uses lining figures, and counts, coordinates and timers also use tabular figures. Alegreya's default old-style figures never appear.

**The Small Caps Label Rule.** Small caps name the value or section that follows them. Action buttons are set in sentence case.

## Layout

Panels are positioned absolutely over a fixed full-viewport layer that ignores pointer events. Only the panels themselves take input.

At 1440×900:
- The top strip is 52px tall, inset 12px.
- The right panel is 320px wide. It starts at 76px from the top and ends 72px above the drawer.
- The bottom drawer is 174px tall, or 36px when collapsed.
- The library sheet is 640px wide, or 50vw minus 12px at narrower widths. It sits on the right.

The strip is a single row of grouped controls separated by 1px inlay rules 28px tall. A flexible spacer pushes the run group and the Run button to the right end. The right panel holds a fixed upper section and a lower section that scrolls on its own. The drawer runs a horizontally scrolling row of 96px tiles with 14px gaps, beside a 300px hint column.

Spacing steps through 4, 6, 10, 14 and 18px. 18px is the inset inside a panel section, and 12px is the gap between a panel and the viewport edge.

Below 1280px wide, run-control captions drop to icon plus key cap, the build tools and Stop drop their key caps (the button titles keep them), the hint column hides, and the track name is truncated at 110px. The strip fits from 1180px wide.

## Elevation & Depth

Depth comes from layered translucency plus one soft ambient shadow. Panels sit as one plane over the 3D scene, and the panels themselves do not stack in tiers. Inside a panel, depth is tonal: active and hovered states use the raised walnut fill. Brass pins have a small sphere gradient and a 1px drop, so they read as set into the wood.

### Shadow Vocabulary
- **Panel float** (`box-shadow: 0 10px 28px rgba(0,0,0,.45), 0 2px 4px rgba(0,0,0,.4)`): every inlay panel, and the reskinned sound mixer.
- **Pin set** (`box-shadow: 0 1px 2px rgba(0,0,0,.6)`): brass pins and done-step pins.
- **Tile lift** (`box-shadow: 0 4px 10px rgba(0,0,0,.4)`): the selected piece tile only.

### Named Rules
**The No Blur Rule.** No backdrop-filter over the canvas. Translucency is plain alpha, and every animated property is opacity or transform, which keeps the frame budget.

## Shapes

Corners are gently rounded and tighten as elements get smaller:
- panels: 6px
- tiles: 5px
- controls: 4px
- key caps and the inner stringing: 3px

Drawer tabs round only their top corners. The pin is the only circle in the chrome. Each panel's inner stringing is a 1px maple line at 16% opacity, inset 4px. That double rule is the marquetry signature. Line icons are 18px, drawn on a 20-unit grid with a 1.4 round stroke in currentColor. Play and Stop are the only filled icons.

## Components

### Buttons
- **Shape:** 4px corners and a 1px transparent border at rest.
- **Tool button (strip):** the icon sits above the label and key cap, 46px minimum width. Hover fills with raised walnut and shows the stringing border. When active it keeps the raised walnut fill with a brass border, and the traveling pin marks it.
- **Run:** a solid brass fill with run-ink text at 15px weight 700, and a lighter brass border (`#e0bb78`). It brightens on hover. During a run it becomes Stop with an Esc key cap.
- **Quiet / action buttons:** stringing border with 6×10px padding, icon then label.
- **Warn:** fault-red border and blush text, used only for destructive confirmations.
- **Focus:** 2px pale-blue outline at a 2px offset on every focusable element.
- **Disabled:** 40% opacity.

### Key Caps
A maple glyph on a 25% black ground, with a 1px maple border at 50% opacity and a 2px bottom edge so it reads as a physical key. On the Run button the cap inverts to a walnut glyph on a 15% white ground.

### Segmented Control
A row of buttons inside one stringing-bordered 4px box. Segments are split by inlay rules, and the active segment is filled raised walnut. A piece option (the toggle's First side) is a segmented radio in the selection panel: one segment per side, named by compass word, the chosen one with a brass pin, and a maple note that it takes effect on the next Run or Reset.

### Drawer Tabs and Piece Tiles
Tabs are maple-text labels on an inlay baseline. The open tab is raised walnut, cream and bold. Tiles are 96px squares on a 22% black ground with stringing borders, holding 80px thumbnails rendered from the kit. Hover and selected fill raised walnut, and the selected tile adds a brass border and the tile lift shadow. Each scroll edge fades out over 48px when more tiles lie that way. A tile's top-right corner carries the placed count in maple 11px lining numerals ("×3"), empty when none is placed. Pressing a tile picks it and starts laying: drag onto the board to lay a run.

### Validation Issue Row
A three-column grid: a pale-blue pin, the issue text, and an underlined pale-blue "Show" link. The cell coordinate sits in pale blue weight 500 above the description. Rows are separated by 14% stringing rules. Show flies the camera to the piece, selects it, and blinks the engine's pale-blue selection mark twice (four 160ms phases). Under reduced motion it cuts to the piece and does not blink.

### Brass Pin (signature)
A 7px sphere drawn with a radial gradient (`#e8c985`, then brass, then `#7d5f2c`). There are blue and red variants, plus an "off" ring for an unspent marble or an idle state. One pin travels along the inlay to the active tool, tab or tile with an .18s ease-out transform. Marble budget pins and completed "Start here" steps use the same form.

### Cell Tooltip and Run Capsule
These are small inlay panels. The tooltip follows the ghost with a pin, the cell and level, and the word "valid". Its border turns red when the placement is invalid. The RUNNING capsule shows the released count over the budget, the elapsed time as m:ss, and a Space key cap for pause. It appears only while the chrome is faded.

Below 1280px the folded run controls show the same inlay as a fold tooltip under the button on hover or focus: the label, then its key cap when it has one. It never shows while the chrome is faded.

### Placement Ghost
A dashed brass outline (1.4px, dash 5 4, round caps) of the brush's footprint box, drawn in SVG over the scene with its 12 edges. It turns fault red when the placement is invalid. While a run is being laid, each laid piece keeps a fainter outline (brass at 70%, 1px, dash 3 4) and the tooltip reads "Laying a run · n pieces · release to finish". The ghost hides off the canvas, outside the place tool and during a run. The engine's tinted ghost mesh sits inside it.

### Library Thumbnail
Each Library row shows a 64×48 plan of the saved track: one square per placement on the cell grid, north up, blocks brass, rails maple, mechanisms pale blue and supports dark brass, drawn top level last. A track with no placements falls back to its initial letter.

## Do's and Don'ts

### Do:
- **Do** build every new chrome surface as an inlay panel: Walnut Veil ground, a 55% maple outer border, a 16% inner stringing inset 4px, and 6px corners.
- **Do** mark the active state with a raised walnut fill and a brass edge, and let the traveling brass pin move to it.
- **Do** show the key cap beside the label for every keyed action, and keep a mouse path for every action.
- **Do** use pale blue for help and focus, and red only for an invalid placement, a broken port or a destructive confirmation.
- **Do** fade the chrome during a run (.4s ease-out opacity), and restore each panel when the pointer reaches its edge.
- **Do** honour reduced motion by turning off the pin travel, fade and dim transitions.

### Don't:
- **Don't** put photographic wood grain on chrome surfaces. Grain belongs to the kit pieces and their thumbnails.
- **Don't** use backdrop-filter over the canvas, or animate anything other than opacity and transform.
- **Don't** ship icon-only toolbar buttons or a permanent grey side dock.
- **Don't** use red for validation issues.
- **Don't** use old-style figures, or a second typeface.
