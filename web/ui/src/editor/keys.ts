// The editor keyboard map (docs/design/editor/README.md, Keyboard map). The overlay owns the keys on
// the web (it sends `keys {enabled:false}` so Bevy's editor shortcuts never double-fire); every key
// here only shortens a visible button. `P` (pause), `C` (follow cam) and `F` (fly) are the engine's
// own keys: Bevy only hears them while the canvas has focus, so the overlay answers them when focus
// is elsewhere (a strip button, the drawer) and leaves them to the engine otherwise. `,`, `.` and
// `N` stay with the engine alone.

import { type Action, type EditorDiag, sliceDown, sliceUp } from "./model";

export interface KeyLike {
  key: string;
  code: string;
  shiftKey: boolean;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
}

/** What a key does: engine actions, or an overlay command. */
export type KeyResult = { kind: "actions"; actions: Action[] } | { kind: "ui"; command: UiCommand } | null;

export type UiCommand = "drawer" | "panel" | "run" | "space" | "escape" | "remove" | "sliceToggle" | "follow" | "fly";

/** The engine's own keys (playback and camera), which Bevy handles itself while the canvas has focus. */
export const ENGINE_KEYS: ReadonlySet<string> = new Set(["KeyP", "KeyC", "KeyF"]);

const acts = (...actions: Action[]): KeyResult => ({ kind: "actions", actions });
const ui = (command: UiCommand): KeyResult => ({
  kind: "ui",
  command,
});

export function keyResult(e: KeyLike, d: Pick<EditorDiag, "slice" | "maxLevel">): KeyResult {
  const mod = e.metaKey || e.ctrlKey;
  if (e.altKey) return null;
  if (mod) {
    if (e.code === "KeyZ") return acts(e.shiftKey ? { op: "redo" } : { op: "undo" });
    if (e.code === "KeyY") return acts({ op: "redo" });
    return null;
  }
  switch (e.code) {
    case "Digit1":
      return acts({ op: "tool", tool: "select" });
    case "Digit2":
      return acts({ op: "tool", tool: "place" });
    case "Digit3":
      return acts({ op: "tool", tool: "delete" });
    case "KeyR":
      return acts({ op: "rotate", by: e.shiftKey ? 3 : 1 });
    case "KeyM":
      return acts({ op: "mirror" });
    case "KeyP":
      return acts({ op: "pause" });
    case "KeyC":
      return ui("follow");
    case "KeyF":
      return ui("fly");
    case "BracketLeft":
      return acts({ op: "slice", level: sliceDown(d.slice, d.maxLevel) });
    case "BracketRight":
      return acts({ op: "slice", level: sliceUp(d.slice, d.maxLevel) });
    case "KeyX":
      return ui("sliceToggle");
    case "ArrowLeft":
      return acts({ op: "move", offset: [-1, 0, 0] });
    case "ArrowRight":
      return acts({ op: "move", offset: [1, 0, 0] });
    case "ArrowUp":
      return acts({ op: "move", offset: [0, 0, -1] });
    case "ArrowDown":
      return acts({ op: "move", offset: [0, 0, 1] });
    case "PageUp":
      return acts({ op: "move", offset: [0, 1, 0] });
    case "PageDown":
      return acts({ op: "move", offset: [0, -1, 0] });
    case "Backspace":
    case "Delete":
      return ui("remove");
    case "Escape":
      return ui("escape");
    case "Space":
      return ui("space");
    case "Tab":
      return e.shiftKey ? null : ui("drawer");
    case "Backslash":
      return ui("panel");
    default:
      return null;
  }
}

/** Keys typed into a text field or a menu are the field's, not the editor's. */
export function ownsKeys(target: EventTarget | null): boolean {
  const el = target as { tagName?: string; isContentEditable?: boolean } | null;
  if (!el?.tagName) return false;
  if (el.isContentEditable) return true;
  return el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT";
}
