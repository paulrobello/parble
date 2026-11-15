// The generator client on the page (P6-T2): one GenClient over web/ui/gen-worker.js, its wasm
// fetched by versioned URL (window.__PARBLE_HASHES), exposed as window.__parbleGen for drivers and
// the P6-T3 generator panel. Nothing loads until the first generate().
import { GenClient, type GenResult, type GenerateOptions } from "./client";
import type { GenParamsInput, JobStatus } from "./protocol";

const BASE = "web/ui/";

export interface GenHook {
  client: GenClient;
  /** Generate and keep the result in `last`; resolves to the result or rejects with a GenError. */
  generate(seed: number, params?: GenParamsInput, options?: GenerateOptions): Promise<GenResult>;
  /** The latest progress status of the running or last job. */
  progress: JobStatus | null;
  last: GenResult | null;
}

declare global {
  interface Window {
    __parbleGen?: GenHook;
  }
}

/** An absolute URL for site path `BASE + path`, with its content-hash version when the map has one. */
export function siteUrl(win: Window, path: string): string {
  const site = BASE + path;
  const v = win.__PARBLE_HASHES?.[site];
  return new URL(v ? `${site}?v=${v}` : site, win.document.baseURI).href;
}

export function mountGen(win: Window): GenHook {
  const client = new GenClient({
    spawn: () => new Worker(siteUrl(win, "gen-worker.js"), { name: "parble-gen" }),
    urls: () => ({ glue: siteUrl(win, "gen/parble_sim_wasm.js"), wasm: siteUrl(win, "gen/parble_sim_wasm_bg.wasm") }),
  });
  const hook: GenHook = {
    client,
    progress: null,
    last: null,
    async generate(seed, params = {}, options = {}) {
      const result = await client.generate(seed, params, {
        ...options,
        onProgress: (status) => {
          hook.progress = status;
          options.onProgress?.(status);
        },
      });
      hook.last = result;
      return result;
    },
  };
  win.__parbleGen = hook;
  return hook;
}
