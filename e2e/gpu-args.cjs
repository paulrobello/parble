// Chromium flags shared by every local browser run: the platform GPU by default (Metal on macOS),
// SwiftShader only when PW_GPU=swiftshader is set explicitly.
"use strict";

const SWIFTSHADER = process.env.PW_GPU === "swiftshader";

const GPU_ARGS = SWIFTSHADER
  ? ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"]
  : process.platform === "darwin"
    ? ["--use-angle=metal", "--enable-gpu", "--ignore-gpu-blocklist"]
    : ["--enable-gpu", "--ignore-gpu-blocklist"];

// Reads the WebGPU adapter and the WebGL renderer from a page on a secure origin.
async function probeGpu(page) {
  return page.evaluate(async () => {
    const a = navigator.gpu ? await navigator.gpu.requestAdapter() : null;
    const adapter = a?.info ? `${a.info.vendor} ${a.info.architecture} ${a.info.description}` : null;
    const gl = document.createElement("canvas").getContext("webgl");
    const ext = gl?.getExtension("WEBGL_debug_renderer_info");
    const renderer = ext ? gl.getParameter(ext.UNMASKED_RENDERER_WEBGL) : null;
    return { adapter, renderer };
  });
}

// Problems with a probe result: no WebGPU adapter, or SwiftShader in a run that did not opt in.
function gpuProblems({ adapter, renderer }) {
  const out = [];
  if (!adapter) out.push("no WebGPU adapter");
  if (!SWIFTSHADER && /swiftshader/i.test(`${adapter} ${renderer}`)) {
    out.push(`SwiftShader in a GPU run (adapter: ${adapter}; renderer: ${renderer})`);
  }
  return out;
}

module.exports = { GPU_ARGS, SWIFTSHADER, probeGpu, gpuProblems };
