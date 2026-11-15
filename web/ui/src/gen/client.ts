// The main-thread side of the generator worker (P6-T2): starts jobs, relays progress and settles
// each generate() with the accepted file or a GenError. All the work runs in the worker, so the
// render loop never waits on generation. One job at a time: a newer generate() cancels the last.
import {
  type EngineUrls,
  type FromWorker,
  type GenParamsInput,
  type JobStatus,
  SLICE_MS,
  type ToWorker,
} from "./protocol";

/** The part of a Worker the client uses (a fake in Vitest). */
export interface WorkerLike {
  postMessage(message: ToWorker): void;
  terminate(): void;
  onmessage: ((event: MessageEvent<FromWorker>) => void) | null;
  onerror: ((event: ErrorEvent) => void) | null;
}

export interface GenResult {
  /** The accepted track as a `.parble` file. */
  file: string;
  /**
   * The file's `seed`: the accepted sub-seed, or for a segmented job the seed it was started
   * with (the segment seeds are in status.segmentSeeds and the file's metadata.generator).
   */
  seed: number;
  status: JobStatus;
  ms: number;
}

export type GenErrorKind = "failed" | "cancelled" | "error";

export class GenError extends Error {
  constructor(
    readonly kind: GenErrorKind,
    message: string,
    readonly status?: JobStatus,
  ) {
    super(message);
    this.name = "GenError";
  }
}

export interface GenerateOptions {
  onProgress?: (status: JobStatus) => void;
  signal?: AbortSignal;
  sliceMs?: number;
  name?: string;
  /** Generate and retry segment by segment (StartMessage.segmented). */
  segmented?: boolean;
}

interface Pending {
  resolve(r: GenResult): void;
  reject(e: GenError): void;
  onProgress?: ((status: JobStatus) => void) | undefined;
  started: number;
}

export interface GenClientOptions {
  spawn: () => WorkerLike;
  urls: () => EngineUrls;
  now?: () => number;
}

export class GenClient {
  private worker: WorkerLike | null = null;
  private next = 1;
  private readonly pending = new Map<number, Pending>();
  private readonly now: () => number;

  constructor(private readonly opts: GenClientOptions) {
    this.now = opts.now ?? (() => performance.now());
  }

  /** Whether a job is running. */
  get busy(): boolean {
    return this.pending.size > 0;
  }

  generate(seed: number, params: GenParamsInput = {}, options: GenerateOptions = {}): Promise<GenResult> {
    if (!Number.isSafeInteger(seed) || seed < 0) {
      return Promise.reject(new GenError("error", `seed ${seed} is not a non-negative safe integer`));
    }
    for (const id of [...this.pending.keys()]) this.cancel(id, "superseded by a newer generate");
    const id = this.next++;
    const worker = this.ensure();
    return new Promise<GenResult>((resolve, reject) => {
      this.pending.set(id, { resolve, reject, onProgress: options.onProgress, started: this.now() });
      const signal = options.signal;
      if (signal) {
        if (signal.aborted) {
          this.cancel(id, "aborted");
          return;
        }
        signal.addEventListener("abort", () => this.cancel(id, "aborted"), { once: true });
      }
      worker.postMessage({
        type: "start",
        id,
        seed,
        params,
        sliceMs: options.sliceMs ?? SLICE_MS,
        name: options.name ?? `Generated track ${seed}`,
        ...(options.segmented ? { segmented: true } : {}),
        urls: this.opts.urls(),
      });
    });
  }

  /** Stop the worker; every pending job rejects as cancelled. */
  dispose(): void {
    for (const id of [...this.pending.keys()]) this.settleError(id, new GenError("cancelled", "disposed"));
    this.worker?.terminate();
    this.worker = null;
  }

  private cancel(id: number, why: string): void {
    if (!this.pending.has(id)) return;
    this.worker?.postMessage({ type: "cancel", id });
    this.settleError(id, new GenError("cancelled", why));
  }

  private settleError(id: number, e: GenError): void {
    const p = this.pending.get(id);
    if (!p) return;
    this.pending.delete(id);
    p.reject(e);
  }

  private ensure(): WorkerLike {
    if (this.worker) return this.worker;
    const worker = this.opts.spawn();
    worker.onmessage = (event) => this.receive(event.data);
    worker.onerror = (event) => {
      // A worker that failed to load (or threw outside a job) cannot run anything: start afresh next time.
      event.preventDefault?.();
      const message = event.message || "the generator worker failed";
      for (const id of [...this.pending.keys()]) this.settleError(id, new GenError("error", message));
      worker.terminate();
      if (this.worker === worker) this.worker = null;
    };
    this.worker = worker;
    return worker;
  }

  private receive(msg: FromWorker): void {
    const p = this.pending.get(msg.id);
    if (!p) return;
    switch (msg.type) {
      case "progress":
        p.onProgress?.(msg.status);
        return;
      case "accepted":
        this.pending.delete(msg.id);
        p.resolve({ file: msg.file, seed: msg.seed, status: msg.status, ms: this.now() - p.started });
        return;
      case "failed": {
        const last = msg.status.rejected.at(-1);
        const why = last ? `: last ${last.reason}` : "";
        this.settleError(
          msg.id,
          new GenError("failed", `no track passed after ${msg.status.rejected.length} candidates${why}`, msg.status),
        );
        return;
      }
      case "cancelled":
        this.settleError(msg.id, new GenError("cancelled", "cancelled"));
        return;
      case "error":
        this.settleError(msg.id, new GenError("error", msg.message));
        return;
    }
  }
}
