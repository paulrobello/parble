// P0-T6 Playwright smoke against the 5311 static preview of dist/ (make build-web first).
// Local done-gate only, never CI. GPU Chromium by default; PW_GPU=swiftshader opts into the CPU renderer.
import { defineConfig } from "@playwright/test";

const GPU_ARGS = process.env.PW_GPU === "swiftshader"
  ? ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"]
  : process.platform === "darwin"
    ? ["--use-angle=metal", "--enable-gpu", "--ignore-gpu-blocklist"]
    : ["--enable-gpu", "--ignore-gpu-blocklist"];

export default defineConfig({
  testDir: "./tests",
  outputDir: "./test-results",
  timeout: 90_000,
  workers: 1,
  reporter: [["list"]],
  use: {
    baseURL: "http://127.0.0.1:5311",
    channel: "chromium",
    headless: true,
    viewport: { width: 1280, height: 720 },
    acceptDownloads: true,
    launchOptions: { args: GPU_ARGS },
  },
  webServer: {
    command: "python3 -m http.server 5311 --bind 127.0.0.1 --directory ../dist",
    url: "http://127.0.0.1:5311/",
    reuseExistingServer: false,
    timeout: 15_000,
  },
});
