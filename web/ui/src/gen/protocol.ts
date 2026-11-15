// The generator worker's message protocol (P6-T2) and the job loop it runs, kept free of worker
// globals so Vitest drives it against a fake engine. The engine is parble-sim-wasm's
// generate_and_validate (crates/parble-sim-wasm/src/generate.rs): every resume(budgetMs) does at
// least one deterministic work unit and returns the status JSON, so the accepted track never
// depends on how the time budget sliced the work.

/** Generator sliders (parble_procgen::GenParams); every field optional, defaults in Rust. */
export interface GenParamsInput {
  footprint?: number;
  height?: number;
  length?: number;
  specialPerMille?: number;
  difficulty?: number;
  families?: { blk: boolean; rail: boolean; mech: boolean };
  marbles?: number;
}

export interface Rejected {
  attempt: number;
  seed: number;
  reason: string;
}

/** The status JSON of GenerateJob.resume. */
export interface JobStatus {
  status: "running" | "accepted" | "failed";
  attempt: number;
  progress: number;
  units: number;
  seed?: number;
  rejected: Rejected[];
}

/** Where the worker loads the wasm-bindgen glue and module from (absolute, versioned URLs). */
export interface EngineUrls {
  glue: string;
  wasm: string;
}

export interface StartMessage {
  type: "start";
  id: number;
  seed: number;
  params: GenParamsInput;
  /** Work per slice before the worker yields to its message queue, ms. */
  sliceMs: number;
  /** The accepted file's metadata name. */
  name: string;
  urls: EngineUrls;
}

export interface CancelMessage {
  type: "cancel";
  id: number;
}

export type ToWorker = StartMessage | CancelMessage;

export type FromWorker =
  | { type: "progress"; id: number; status: JobStatus }
  | { type: "accepted"; id: number; status: JobStatus; file: string; seed: number }
  | { type: "failed"; id: number; status: JobStatus }
  | { type: "cancelled"; id: number }
  | { type: "error"; id: number; message: string };

/** One running job, as parble-sim-wasm's GenerateJob exposes it. */
export interface EngineJob {
  resume(budgetMs: number): string;
  statusJson(): string;
  fileJson(name: string): string | undefined;
  free(): void;
}

export interface Engine {
  /** generate_and_validate(seed, paramsJson, budgetMs): starts the job and runs its first slice. */
  start(seed: number, paramsJson: string, budgetMs: number): EngineJob;
}

/** What the job loop needs from its host: posting, yielding and the cancel flag. */
export interface LoopHost {
  post(message: FromWorker): void;
  /** Resolves once queued messages (a cancel) have been handled. */
  pause(): Promise<void>;
  cancelled(id: number): boolean;
}

/** Default slice, ms: short enough that a cancel lands quickly, long enough to amortize posting. */
export const SLICE_MS = 12;

/**
 * Run one job to its end: a progress message after every slice, then accepted, failed, cancelled
 * or error. The first slice runs inside start(); each later one after a pause.
 */
export async function runJob(engine: Engine, msg: StartMessage, host: LoopHost): Promise<void> {
  const { id } = msg;
  let job: EngineJob | null = null;
  try {
    job = engine.start(msg.seed, JSON.stringify(msg.params), msg.sliceMs);
    let status = JSON.parse(job.statusJson()) as JobStatus;
    for (;;) {
      host.post({ type: "progress", id, status });
      if (status.status !== "running") break;
      await host.pause();
      if (host.cancelled(id)) {
        host.post({ type: "cancelled", id });
        return;
      }
      status = JSON.parse(job.resume(msg.sliceMs)) as JobStatus;
    }
    if (status.status === "accepted") {
      const file = job.fileJson(msg.name);
      if (file === undefined || status.seed === undefined) throw new Error("accepted job has no file");
      host.post({ type: "accepted", id, status, file, seed: status.seed });
    } else {
      host.post({ type: "failed", id, status });
    }
  } catch (e) {
    host.post({ type: "error", id, message: e instanceof Error ? e.message : String(e) });
  } finally {
    job?.free();
  }
}
