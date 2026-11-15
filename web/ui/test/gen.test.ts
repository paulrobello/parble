// The generator worker protocol (P6-T2): runJob against a fake engine (progress per slice, accepted,
// failed, thrown, cancelled), and GenClient against a fake worker that runs the same loop in process
// (results, progress, supersede, abort, bad seeds, a worker that fails to load). Real wasm parity is
// make determinism-wasm's job (scripts/determinism/run-golden.cjs), not Vitest's.
import { describe, expect, it } from "vitest";
import { GenClient, GenError, type WorkerLike } from "../src/gen/client";
import {
  type Engine,
  type EngineJob,
  type FromWorker,
  type JobStatus,
  type LoopHost,
  type StartMessage,
  type ToWorker,
  runJob,
} from "../src/gen/protocol";

/** A job that needs `units` work units; it rejects `rejects` candidates first, or fails if `fail`. */
function fakeEngine(units: number, opts: { rejects?: number; fail?: boolean; throwOnStart?: boolean } = {}) {
  const log = { budgets: [] as number[], freed: 0, params: [] as string[], segmented: [] as boolean[] };
  const engine: Engine = {
    start(seed, paramsJson, budgetMs, segmented) {
      if (opts.throwOnStart) throw new Error("params: unknown field `seed`");
      log.params.push(paramsJson);
      log.segmented.push(segmented === true);
      let done = 0;
      const status = (): JobStatus => {
        const end = done >= units;
        const rejected = Array.from({ length: opts.rejects ?? 0 }, (_, i) => ({
          attempt: i,
          seed: seed + i,
          reason: "a marble was lost",
        }));
        const state = end ? (opts.fail ? "failed" : "accepted") : "running";
        return {
          status: state,
          attempt: opts.rejects ?? 0,
          progress: Math.min(1, done / units),
          units: done,
          ...(state === "accepted" ? { seed: seed + (opts.rejects ?? 0) } : {}),
          rejected,
        };
      };
      const step = (ms: number) => {
        log.budgets.push(ms);
        done += 1;
      };
      step(budgetMs);
      const job: EngineJob = {
        resume(ms) {
          step(ms);
          return JSON.stringify(status());
        },
        statusJson: () => JSON.stringify(status()),
        fileJson: (name) => (done >= units && !opts.fail ? JSON.stringify({ name, seed }) : undefined),
        free: () => {
          log.freed += 1;
        },
      };
      return job;
    },
  };
  return { engine, log };
}

function start(over: Partial<StartMessage> = {}): StartMessage {
  return {
    type: "start",
    id: 1,
    seed: 7,
    params: { length: 14 },
    sliceMs: 12,
    name: "Generated track 7",
    urls: { glue: "g.js", wasm: "g.wasm" },
    ...over,
  };
}

function host(cancelAfterPauses = Number.POSITIVE_INFINITY) {
  const posted: FromWorker[] = [];
  let pauses = 0;
  const h: LoopHost = {
    post: (m) => posted.push(m),
    pause: async () => {
      pauses += 1;
    },
    cancelled: () => pauses >= cancelAfterPauses,
  };
  return { h, posted, pauses: () => pauses };
}

describe("runJob", () => {
  it("posts progress after every slice and the accepted file at the end", async () => {
    const { engine, log } = fakeEngine(4, { rejects: 1 });
    const { h, posted, pauses } = host();
    await runJob(engine, start(), h);
    const types = posted.map((m) => m.type);
    expect(types).toEqual(["progress", "progress", "progress", "progress", "accepted"]);
    expect(pauses()).toBe(3);
    expect(log.budgets).toEqual([12, 12, 12, 12]);
    expect(log.params).toEqual([JSON.stringify({ length: 14 })]);
    const last = posted.at(-1);
    expect(last?.type === "accepted" && last.seed).toBe(8);
    expect(last?.type === "accepted" && JSON.parse(last.file)).toEqual({ name: "Generated track 7", seed: 7 });
    expect(log.freed).toBe(1);
  });

  it("starts a segmented job only when the start message asks for one", async () => {
    const { engine, log } = fakeEngine(1);
    await runJob(engine, start(), host().h);
    await runJob(engine, start({ segmented: true }), host().h);
    expect(log.segmented).toEqual([false, true]);
  });

  it("posts failed with every rejection when no candidate passes", async () => {
    const { engine, log } = fakeEngine(2, { rejects: 32, fail: true });
    const { h, posted } = host();
    await runJob(engine, start(), h);
    const last = posted.at(-1);
    expect(last?.type).toBe("failed");
    expect(last?.type === "failed" && last.status.rejected).toHaveLength(32);
    expect(log.freed).toBe(1);
  });

  it("turns a throw into an error message", async () => {
    const { engine } = fakeEngine(2, { throwOnStart: true });
    const { h, posted } = host();
    await runJob(engine, start(), h);
    expect(posted).toEqual([{ type: "error", id: 1, message: "params: unknown field `seed`" }]);
  });

  it("stops at the first pause after a cancel and frees the job", async () => {
    const { engine, log } = fakeEngine(100);
    const { h, posted } = host(2);
    await runJob(engine, start(), h);
    expect(posted.map((m) => m.type)).toEqual(["progress", "progress", "cancelled"]);
    expect(log.budgets).toHaveLength(2);
    expect(log.freed).toBe(1);
  });
});

/** A worker that runs runJob in process on the fake engine, cancelling by id. */
function fakeWorker(units: number, opts: { loadError?: string } = {}) {
  const sent: ToWorker[] = [];
  let terminated = 0;
  const cancelled = new Set<number>();
  const worker: WorkerLike = {
    onmessage: null,
    onerror: null,
    terminate: () => {
      terminated += 1;
    },
    postMessage(msg) {
      sent.push(msg);
      if (msg.type === "cancel") {
        cancelled.add(msg.id);
        return;
      }
      if (opts.loadError) {
        const message = opts.loadError;
        queueMicrotask(() => worker.onerror?.({ message, preventDefault: () => {} } as ErrorEvent));
        return;
      }
      const { engine } = fakeEngine(units);
      void runJob(engine, msg, {
        post: (m) => worker.onmessage?.({ data: m } as MessageEvent<FromWorker>),
        pause: () => new Promise((r) => setTimeout(r, 0)),
        cancelled: (id) => cancelled.has(id),
      });
    },
  };
  return { worker, sent, terminated: () => terminated };
}

function client(units: number, opts: { loadError?: string } = {}) {
  const spawned: ReturnType<typeof fakeWorker>[] = [];
  let t = 0;
  const c = new GenClient({
    spawn: () => {
      const w = fakeWorker(units, opts);
      spawned.push(w);
      return w.worker;
    },
    urls: () => ({ glue: "https://x/web/ui/gen/parble_sim_wasm.js?v=1", wasm: "https://x/web/ui/gen/b.wasm?v=2" }),
    now: () => (t += 5),
  });
  return { c, spawned };
}

describe("GenClient", () => {
  it("resolves with the accepted file and relays progress", async () => {
    const { c, spawned } = client(3);
    const seen: number[] = [];
    const r = await c.generate(7, { height: 3 }, { onProgress: (s) => seen.push(s.units), sliceMs: 4 });
    expect(r.seed).toBe(7);
    expect(JSON.parse(r.file)).toEqual({ name: "Generated track 7", seed: 7 });
    expect(seen).toEqual([1, 2, 3]);
    expect(r.ms).toBeGreaterThan(0);
    const startMsg = spawned[0]?.sent[0];
    expect(startMsg?.type === "start" && startMsg.sliceMs).toBe(4);
    expect(startMsg?.type === "start" && startMsg.urls.glue).toContain("?v=1");
    expect(startMsg?.type === "start" && "segmented" in startMsg).toBe(false);
    expect(c.busy).toBe(false);
  });

  it("passes the segmented option to the worker", async () => {
    const { c, spawned } = client(2);
    await c.generate(7, { length: 500, height: 48 }, { segmented: true });
    const startMsg = spawned[0]?.sent[0];
    expect(startMsg?.type === "start" && startMsg.segmented).toBe(true);
  });

  it("spawns the worker once, on the first generate", async () => {
    const { c, spawned } = client(1);
    expect(spawned).toHaveLength(0);
    await c.generate(1);
    await c.generate(2);
    expect(spawned).toHaveLength(1);
  });

  it("cancels the running job when a newer one starts", async () => {
    const { c, spawned } = client(50);
    const first = c.generate(1);
    const second = c.generate(2);
    await expect(first).rejects.toMatchObject({ kind: "cancelled" });
    expect((await second).seed).toBe(2);
    expect(spawned[0]?.sent.map((m) => m.type)).toEqual(["start", "cancel", "start"]);
  });

  it("cancels on abort", async () => {
    const { c } = client(50);
    const ctl = new AbortController();
    const p = c.generate(1, {}, { signal: ctl.signal });
    ctl.abort();
    await expect(p).rejects.toBeInstanceOf(GenError);
    await expect(p).rejects.toMatchObject({ kind: "cancelled" });
  });

  it("refuses seeds a track file cannot hold", async () => {
    const { c, spawned } = client(1);
    for (const bad of [-1, 1.5, Number.NaN, 2 ** 53]) {
      await expect(c.generate(bad)).rejects.toMatchObject({ kind: "error" });
    }
    expect(spawned).toHaveLength(0);
  });

  it("rejects with the load error and spawns a fresh worker next time", async () => {
    const { c, spawned } = client(1, { loadError: "importScripts failed" });
    await expect(c.generate(1)).rejects.toMatchObject({ kind: "error", message: "importScripts failed" });
    expect(spawned[0]?.terminated()).toBe(1);
    await expect(c.generate(2)).rejects.toMatchObject({ kind: "error" });
    expect(spawned).toHaveLength(2);
  });
});
