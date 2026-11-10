// First-run GPU quality detection (P5-T3): the adapter's info plus a short fill benchmark. The test
// pass draws one full-screen triangle at 1920x1080 through a fragment loop of trig, so its time tracks
// the per-pixel shading budget the post stack spends. A few passes are timed by queue completion and
// the median picks the tier. The whole check stays under about a second, and a slow first pass ends
// it early.

import type { AdapterInfo, Tier } from "./store";

export const BENCH_W = 1920;
export const BENCH_H = 1080;
/** Draws per timed submit, and timed submits after one warm-up. */
export const PASSES_PER_SUBMIT = 4;
export const SUBMITS = 5;
/** A warm-up submit slower than this ends the check at `low`, ms. */
export const SLOW_WARMUP_MS = 400;

/**
 * Upper bounds of the median ms per test pass for each tier. Calibrated on an Apple M-series GPU in
 * GPU Chromium (PRODUCT.md: 60 fps at High on M-series at 1440p), which measured 0.75 ms per pass
 * there (docs/design/title/README.md). Faster than `ultra` is ultra, slower than `medium` is low.
 */
export const TIER_MS: Readonly<Record<Exclude<Tier, "low">, number>> = { ultra: 0.3, high: 2.5, medium: 6 };

export interface Detection {
  tier: Tier;
  adapter: AdapterInfo | null;
  benchMs: number | null;
  /** Why the tier was chosen, for the settings note. */
  reason: "bench" | "fallback-adapter" | "slow" | "no-adapter" | "error";
}

/** The tier for a median pass time and an adapter that may be a software fallback. */
export function tierFor(benchMs: number, fallbackAdapter: boolean): Tier {
  if (fallbackAdapter) return "low";
  if (benchMs <= TIER_MS.ultra) return "ultra";
  if (benchMs <= TIER_MS.high) return "high";
  if (benchMs <= TIER_MS.medium) return "medium";
  return "low";
}

export function median(xs: readonly number[]): number {
  const s = [...xs].sort((a, b) => a - b);
  const m = s.length >> 1;
  return s.length % 2 ? (s[m] as number) : ((s[m - 1] as number) + (s[m] as number)) / 2;
}

/** A readable adapter name: the description, else vendor and architecture. */
export function adapterName(a: AdapterInfo | null): string {
  if (!a) return "unknown GPU";
  if (a.description) return a.description;
  const vendor = a.vendor ? a.vendor[0]?.toUpperCase() + a.vendor.slice(1) : "";
  const v = [vendor, a.architecture].filter(Boolean).join(" ");
  return v || "unknown GPU";
}

const SHADER = /* wgsl */ `
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
  var p = array(vec2f(-1.0, -3.0), vec2f(-1.0, 1.0), vec2f(3.0, 1.0));
  return vec4f(p[i], 0.0, 1.0);
}
@fragment fn fs(@builtin(position) c: vec4f) -> @location(0) vec4f {
  var v = c.xy * 0.01;
  var a = 0.0;
  for (var k = 0; k < 48; k++) {
    v = vec2f(sin(v.y * 1.7 + a), cos(v.x * 1.3 - a));
    a = a + dot(v, v) * 0.37;
  }
  return vec4f(fract(a), v * 0.5 + 0.5, 1.0);
}`;

interface GpuLike {
  requestAdapter(o?: { powerPreference?: string }): Promise<unknown>;
}

/** Run the detection. Never throws: a missing adapter or a failed device is `low`. */
export async function detectQuality(gpu: GpuLike | undefined, now: () => number): Promise<Detection> {
  if (!gpu) return { tier: "low", adapter: null, benchMs: null, reason: "no-adapter" };
  let device: GPUDevice | null = null;
  try {
    const adapter = (await gpu.requestAdapter({ powerPreference: "high-performance" })) as GPUAdapter | null;
    if (!adapter) return { tier: "low", adapter: null, benchMs: null, reason: "no-adapter" };
    const raw = (adapter as unknown as { info?: Partial<AdapterInfo> & { isFallbackAdapter?: boolean } }).info ?? {};
    const info: AdapterInfo = {
      vendor: raw.vendor ?? "",
      architecture: raw.architecture ?? "",
      device: raw.device ?? "",
      description: raw.description ?? "",
    };
    const fallback =
      raw.isFallbackAdapter === true ||
      (adapter as unknown as { isFallbackAdapter?: boolean }).isFallbackAdapter === true;
    if (fallback) return { tier: "low", adapter: info, benchMs: null, reason: "fallback-adapter" };
    device = await adapter.requestDevice();
    const d = device;
    const module = d.createShaderModule({ code: SHADER });
    const format: GPUTextureFormat = "rgba8unorm";
    const pipeline = await d.createRenderPipelineAsync({
      layout: "auto",
      vertex: { module, entryPoint: "vs" },
      fragment: { module, entryPoint: "fs", targets: [{ format }] },
      primitive: { topology: "triangle-list" },
    });
    const target = d.createTexture({
      size: [BENCH_W, BENCH_H],
      format,
      usage: 0x10, // GPUTextureUsage.RENDER_ATTACHMENT (the constant is not in the DOM lib)
    });
    const view = target.createView();
    const submit = async (): Promise<number> => {
      const t0 = now();
      const enc = d.createCommandEncoder();
      for (let i = 0; i < PASSES_PER_SUBMIT; i++) {
        const pass = enc.beginRenderPass({
          colorAttachments: [{ view, loadOp: "clear", storeOp: "store", clearValue: [0, 0, 0, 1] }],
        });
        pass.setPipeline(pipeline);
        pass.draw(3);
        pass.end();
      }
      d.queue.submit([enc.finish()]);
      await d.queue.onSubmittedWorkDone();
      return (now() - t0) / PASSES_PER_SUBMIT;
    };
    const warm = await submit();
    if (warm * PASSES_PER_SUBMIT > SLOW_WARMUP_MS) {
      target.destroy();
      return { tier: "low", adapter: info, benchMs: warm, reason: "slow" };
    }
    const times: number[] = [];
    for (let i = 0; i < SUBMITS; i++) times.push(await submit());
    target.destroy();
    const ms = median(times);
    return { tier: tierFor(ms, false), adapter: info, benchMs: Math.round(ms * 1000) / 1000, reason: "bench" };
  } catch (err) {
    console.warn("parble: GPU quality check failed", err);
    return { tier: "low", adapter: null, benchMs: null, reason: "error" };
  } finally {
    device?.destroy();
  }
}
