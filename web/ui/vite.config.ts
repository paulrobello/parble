import { defineConfig } from "vitest/config";

// PARBLE_GEN_WORKER=1 builds the generator Web Worker (src/gen/worker.ts) into dist/gen-worker.js
// instead, after the HUD build and without emptying dist: a classic worker script, so it can
// importScripts the wasm-bindgen no-modules glue (scripts/web/gen-wasm.sh).
const worker = process.env.PARBLE_GEN_WORKER === "1";

// One classic IIFE script at a fixed name (dist/hud.js): index.html loads it by URL, version-dist adds
// its ?v= hash, and the page CSP allows no inline script, so no HTML entry and no hashed file names.
export default defineConfig({
  build: {
    outDir: "dist",
    emptyOutDir: !worker,
    copyPublicDir: false,
    lib: worker
      ? {
          entry: "src/gen/worker.ts",
          formats: ["iife"],
          name: "ParbleGenWorker",
          fileName: () => "gen-worker.js",
        }
      : {
          entry: "src/main.ts",
          formats: ["iife"],
          name: "ParbleHud",
          fileName: () => "hud.js",
        },
  },
  test: {
    include: ["test/**/*.test.ts"],
    environment: "node",
  },
});
