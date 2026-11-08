// Playwright globalSetup: refuse the whole run unless Chromium is on the real GPU.
import { chromium } from "@playwright/test";
import gpu from "./gpu-args.cjs";

export default async function gpuSetup() {
  const browser = await chromium.launch({ channel: "chromium", headless: true, args: [...gpu.GPU_ARGS, "--mute-audio"] });
  try {
    const page = await browser.newPage();
    // Any path on the preview origin gives a secure context; the 404 page avoids booting the app.
    await page.goto("http://127.0.0.1:5311/__gpu-probe");
    const probe = await gpu.probeGpu(page);
    console.log(`gpu: adapter=${probe.adapter} renderer=${probe.renderer}`);
    const problems = gpu.gpuProblems(probe);
    if (problems.length) throw new Error(`GPU check failed: ${problems.join("; ")}`);
  } finally {
    await browser.close();
  }
}
