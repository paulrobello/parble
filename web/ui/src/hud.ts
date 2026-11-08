import type { BridgeEvent } from "./bridge";

export interface HudState {
  fps: number | null;
  tick: number | null;
}

export const emptyHud = (): HudState => ({ fps: null, tick: null });

/** The state after `events`; only the latest frame counts. */
export function applyEvents(state: HudState, events: readonly BridgeEvent[]): HudState {
  let next = state;
  for (const e of events) {
    if (e.type === "frame") next = { fps: e.fps, tick: e.tick };
  }
  return next;
}

export function formatHud(state: HudState): string {
  const fps = state.fps === null ? "--" : Math.round(state.fps).toString();
  const tick = state.tick === null ? "--" : state.tick.toString();
  return `fps ${fps}  tick ${tick}`;
}

/** A fixed, non-interactive readout in the top-left corner of the page. */
export function mountHud(doc: Document): HTMLElement {
  const el = doc.createElement("div");
  el.id = "parble-hud";
  el.setAttribute("aria-live", "off");
  Object.assign(el.style, {
    position: "fixed",
    top: "8px",
    left: "8px",
    padding: "4px 8px",
    borderRadius: "4px",
    background: "rgba(28, 26, 23, 0.72)",
    color: "#ece4d8",
    font: "12px/1.4 ui-monospace, Menlo, monospace",
    whiteSpace: "pre",
    pointerEvents: "none",
    userSelect: "none",
    zIndex: "10",
  });
  el.textContent = formatHud(emptyHud());
  doc.body.append(el);
  return el;
}
