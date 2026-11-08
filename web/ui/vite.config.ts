import { defineConfig } from "vitest/config";

// One classic IIFE script at a fixed name (dist/hud.js): index.html loads it by URL, version-dist adds
// its ?v= hash, and the page CSP allows no inline script, so no HTML entry and no hashed file names.
export default defineConfig({
  build: {
    outDir: "dist",
    emptyOutDir: true,
    copyPublicDir: false,
    lib: {
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
