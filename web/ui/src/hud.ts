import type { BridgeEvent } from "./bridge";

/** Sim ticks per second (crates/parble-sim `TICK_HZ`). */
export const TICK_HZ = 240;
/** Checkpoint splits kept on screen, newest last. */
export const MAX_SPLITS = 4;

/** One checkpoint pass: run time in ticks, the checkpoint (1-based, in first-pass order) and the marble. */
export interface HudSplit {
  tick: number;
  checkpoint: number;
  marble: number;
}

export interface HudState {
  fps: number | null;
  tick: number | null;
  /** Playback (crates/parble-app/src/playback.rs); absent while running at 1x. */
  paused?: boolean;
  timeScale?: number;
  splits: HudSplit[];
  /** Checkpoint anchor cells in first-pass order this run, as "x,y,z". */
  checkpoints: string[];
}

export const emptyHud = (): HudState => ({ fps: null, tick: null, splits: [], checkpoints: [] });

/**
 * The state after `events`: the latest frame, and the run's checkpoint splits. A tick going
 * backwards (a run restart or a loaded track) clears the splits.
 */
export function applyEvents(state: HudState, events: readonly BridgeEvent[]): HudState {
  let next = state;
  const restarted = (tick: number): boolean => next.tick !== null && tick < next.tick;
  for (const e of events) {
    if (e.type === "frame") {
      next = restarted(e.tick)
        ? { fps: e.fps, tick: e.tick, splits: [], checkpoints: [] }
        : { fps: e.fps, tick: e.tick, splits: next.splits, checkpoints: next.checkpoints };
      if (e.paused) next.paused = true;
      if (e.timeScale !== undefined && e.timeScale !== 1) next.timeScale = e.timeScale;
    } else if (e.type === "split") {
      // A split is never later than the frame tick it arrives with, so only an earlier split marks a restart.
      const last = next.splits.at(-1);
      const base = last && e.tick < last.tick ? { ...next, splits: [], checkpoints: [] } : next;
      const key = e.instance.join(",");
      const checkpoints = base.checkpoints.includes(key) ? base.checkpoints : [...base.checkpoints, key];
      const split = { tick: e.tick, checkpoint: checkpoints.indexOf(key) + 1, marble: e.marble };
      next = { ...base, checkpoints, splits: [...base.splits, split].slice(-MAX_SPLITS) };
    }
  }
  return next;
}

/** Run time of `tick` as seconds with milliseconds, e.g. "12.345 s". */
export const formatSplitTime = (tick: number): string => `${(tick / TICK_HZ).toFixed(3)} s`;

/**
 * `fps N  tick N`, then `  paused` and the time scale (`  0.25x`) only when they apply, then one
 * line per checkpoint split.
 */
export function formatHud(state: HudState): string {
  const fps = state.fps === null ? "--" : Math.round(state.fps).toString();
  const tick = state.tick === null ? "--" : state.tick.toString();
  let text = `fps ${fps}  tick ${tick}`;
  if (state.paused) text += "  paused";
  if (state.timeScale !== undefined && state.timeScale !== 1) text += `  ${state.timeScale}x`;
  const lines = [text];
  for (const s of state.splits) {
    lines.push(`split cp${s.checkpoint}  ${formatSplitTime(s.tick)}  marble ${s.marble}`);
  }
  return lines.join("\n");
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

/** The controls line under the fly-only camera (`camera.flyOnly`, the mechanism showcase): always flying, no director to return to. */
export const FLY_ONLY_CONTROLS_HINT =
  "click the scene, then fly: WASD move  Q/E down/up  drag look  wheel speed  G sensors\n" +
  "P pause  , slower  . faster  N step (paused)";

/** The controls line for the app's camera: the fly-only line when it reports `flyOnly`. */
export function controlsHint(flyOnly: boolean): string {
  return flyOnly ? FLY_ONLY_CONTROLS_HINT : CONTROLS_HINT;
}

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
