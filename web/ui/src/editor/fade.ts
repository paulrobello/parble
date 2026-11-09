// The run fade (docs/design/editor/README.md, Run): during a run, 1.2 s without pointer movement
// fades the chrome to cinematic; once faded, the pointer within 48 px of an edge brings that edge's
// chrome back, and leaving the edge starts the 1.2 s timer again. Time is passed in, so tests drive it.

export const IDLE_MS = 1_200;
export const EDGE_PX = 48;

export type Edge = "top" | "bottom" | "right";
const EDGES: readonly Edge[] = ["top", "bottom", "right"];

export interface FadeView {
  /** The chrome is faded (cinematic). */
  faded: boolean;
  /** Edges whose chrome is back while faded. */
  shown: ReadonlySet<Edge>;
}

export function edgesNear(x: number, y: number, w: number, h: number): Set<Edge> {
  const near = new Set<Edge>();
  if (y <= EDGE_PX) near.add("top");
  if (y >= h - EDGE_PX) near.add("bottom");
  if (x >= w - EDGE_PX) near.add("right");
  return near;
}

export class RunFade {
  #running = false;
  #last = 0;
  #faded = false;
  #near = new Set<Edge>();
  /** When the pointer left each edge it had brought back. */
  #leftAt = new Map<Edge, number>();

  /** Start or end a run. Either way everything comes back at once. */
  setRunning(on: boolean, now: number): void {
    if (on === this.#running) return;
    this.#running = on;
    this.#last = now;
    this.#faded = false;
    this.#near.clear();
    this.#leftAt.clear();
  }

  get running(): boolean {
    return this.#running;
  }

  /** A pointer move to (x, y) in a w × h viewport. */
  move(x: number, y: number, w: number, h: number, now: number): void {
    if (!this.#running) return;
    if (!this.#faded) {
      this.#last = now;
      return;
    }
    const near = edgesNear(x, y, w, h);
    for (const e of this.#near) if (!near.has(e)) this.#leftAt.set(e, now);
    for (const e of near) this.#leftAt.delete(e);
    this.#near = near;
  }

  /** The view at `now`. */
  view(now: number): FadeView {
    if (!this.#running) return { faded: false, shown: new Set() };
    if (!this.#faded && now - this.#last >= IDLE_MS) this.#faded = true;
    const shown = new Set<Edge>();
    for (const e of EDGES) {
      const left = this.#leftAt.get(e);
      if (this.#near.has(e) || (left !== undefined && now - left < IDLE_MS)) shown.add(e);
      else if (left !== undefined) this.#leftAt.delete(e);
    }
    return { faded: this.#faded, shown };
  }
}
