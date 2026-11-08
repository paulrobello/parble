import type { BridgeEvent } from "./bridge";

export interface HudState {
  fps: number | null;
  tick: number | null;
  /** Playback (crates/parble-app/src/playback.rs); absent while running at 1x. */
  paused?: boolean;
  timeScale?: number;
}

export const emptyHud = (): HudState => ({ fps: null, tick: null });

/** The state after `events`; only the latest frame counts. */
export function applyEvents(state: HudState, events: readonly BridgeEvent[]): HudState {
  let next = state;
  for (const e of events) {
    if (e.type !== "frame") continue;
    next = { fps: e.fps, tick: e.tick };
    if (e.paused) next.paused = true;
    if (e.timeScale !== undefined && e.timeScale !== 1) next.timeScale = e.timeScale;
  }
  return next;
}

/** `fps N  tick N`, then `  paused` and the time scale (`  0.25x`) only when they apply. */
export function formatHud(state: HudState): string {
  const fps = state.fps === null ? "--" : Math.round(state.fps).toString();
  const tick = state.tick === null ? "--" : state.tick.toString();
  let text = `fps ${fps}  tick ${tick}`;
  if (state.paused) text += "  paused";
  if (state.timeScale !== undefined && state.timeScale !== 1) text += `  ${state.timeScale}x`;
  return text;
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

/** Camera controls (crates/parble-render/src/camera.rs), the sensor overlay (sensors.rs) and playback (crates/parble-app/src/playback.rs); keys reach the game only while the canvas has focus. */
export const CONTROLS_HINT =
  "click the scene, then F: fly camera  WASD move  Q/E down/up  drag look  wheel speed  F/Esc back  G sensors\n" +
  "P pause  , slower  . faster  N step (paused)";

/** A fixed, non-interactive controls line in the bottom-left corner, apart from the readout. */
export function mountControlsHint(doc: Document): HTMLElement {
  const el = doc.createElement("div");
  el.id = "parble-controls";
  Object.assign(el.style, {
    position: "fixed",
    bottom: "8px",
    left: "8px",
    padding: "4px 8px",
    borderRadius: "4px",
    background: "rgba(28, 26, 23, 0.6)",
    color: "#ece4d8",
    font: "11px/1.4 ui-monospace, Menlo, monospace",
    whiteSpace: "pre",
    pointerEvents: "none",
    userSelect: "none",
    zIndex: "10",
  });
  el.textContent = CONTROLS_HINT;
  doc.body.append(el);
  return el;
}
