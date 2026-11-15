// The generator Web Worker (P6-T2), built by Vite into dist/gen-worker.js (PARBLE_GEN_WORKER=1). A
// classic worker: the first start message names the wasm-bindgen no-modules glue and module
// (scripts/web/gen-wasm.sh), loaded once with importScripts. Jobs run one at a time through
// runJob; a start while one runs cancels it.
import { type Engine, type EngineJob, type FromWorker, type StartMessage, type ToWorker, runJob } from "./protocol";

interface Bindgen {
  (init: { module_or_path: string }): Promise<unknown>;
  generate_and_validate(seed: number, paramsJson: string, budgetMs: number): EngineJob;
}

interface WorkerScope {
  importScripts(...urls: string[]): void;
  postMessage(message: FromWorker): void;
  onmessage: ((event: MessageEvent<ToWorker>) => void) | null;
}

// The glue declares `let wasm_bindgen` at its top level: a global binding every classic script of
// the worker sees, but not a property of globalThis, so it is read by name after importScripts.
declare const wasm_bindgen: Bindgen | undefined;

const scope = globalThis as unknown as WorkerScope;
let engine: Promise<Engine> | null = null;
let current = -1;
const cancelled = new Set<number>();
const queue: StartMessage[] = [];
let running = false;

function load(msg: StartMessage): Promise<Engine> {
  engine ??= (async () => {
    scope.importScripts(msg.urls.glue);
    const bindgen = typeof wasm_bindgen === "undefined" ? undefined : wasm_bindgen;
    if (!bindgen) throw new Error("the generator glue defined no wasm_bindgen");
    await bindgen({ module_or_path: msg.urls.wasm });
    return { start: (seed, params, budget) => bindgen.generate_and_validate(seed, params, budget) };
  })();
  return engine;
}

/** A macrotask turn, so a cancel or a newer start already posted is handled first. */
function pause(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

async function drain(): Promise<void> {
  if (running) return;
  running = true;
  try {
    for (let msg = queue.shift(); msg; msg = queue.shift()) {
      current = msg.id;
      if (cancelled.delete(msg.id)) {
        scope.postMessage({ type: "cancelled", id: msg.id });
        continue;
      }
      let ready: Engine;
      try {
        ready = await load(msg);
      } catch (e) {
        engine = null;
        scope.postMessage({ type: "error", id: msg.id, message: e instanceof Error ? e.message : String(e) });
        continue;
      }
      await runJob(ready, msg, {
        post: (m) => scope.postMessage(m),
        pause,
        cancelled: (id) => cancelled.has(id),
      });
      cancelled.delete(msg.id);
    }
  } finally {
    running = false;
  }
}

scope.onmessage = (event) => {
  const msg = event.data;
  if (msg.type === "cancel") {
    cancelled.add(msg.id);
    return;
  }
  if (running) cancelled.add(current);
  for (const queued of queue.splice(0)) scope.postMessage({ type: "cancelled", id: queued.id });
  queue.push(msg);
  void drain();
};
