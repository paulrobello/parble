#!/usr/bin/env node
// P0-T5 Safari smoke over safaridriver's W3C WebDriver (local only; needs Safari's "Allow Remote
// Automation"). Safari WebDriver cannot read the console, so this reads index.html's page-side log
// (window.__parbleErrors) and the WGSL shim stats, and saves a WebDriver screenshot.
// Usage: node e2e/safari-smoke.cjs [url] [screenshot.png]
//   url defaults to serving ./target/web-dist (make build-web) on a random loopback port.
// Limit: errors the browser reports itself (native WebGPU validation warnings) never reach the page
// log; console.error, Bevy's ERROR lines, uncaught errors and CSP violations do.
"use strict";
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

let url = process.argv[2];
const shot = process.argv[3];
const port = 20000 + Math.floor(Math.random() * 20000);
const base = `http://127.0.0.1:${port}`;

async function wd(method, p, body) {
  const r = await fetch(base + p, { method, headers: { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(60_000) });
  const j = await r.json();
  if (!r.ok) throw new Error(`${method} ${p}: ${JSON.stringify(j.value)}`);
  return j.value;
}

(async () => {
  const driver = spawn("safaridriver", ["-p", String(port)], { stdio: "ignore" });
  let server = null;
  if (!url) {
    const sitePort = port + 1;
    server = spawn("python3", ["-m", "http.server", String(sitePort), "--bind", "127.0.0.1",
      "--directory", path.resolve(__dirname, "../target/web-dist")], { stdio: "ignore" });
    url = `http://127.0.0.1:${sitePort}/`;
  }
  let session = null, code = 1;
  try {
    await new Promise((r) => setTimeout(r, 1500));
    session = (await wd("POST", "/session", { capabilities: { alwaysMatch: { browserName: "safari" } } })).sessionId;
    const s = `/session/${session}`;
    await wd("POST", `${s}/url`, { url });
    await new Promise((r) => setTimeout(r, 12_000));
    const state = await wd("POST", `${s}/execute/sync`, { args: [], script:
      "var c=document.querySelector('canvas#parble');return {href:location.href,errors:window.__parbleErrors||null," +
      "compat:window.__wgslCompat||null,canvas:c?[c.width,c.height]:null,gpu:!!navigator.gpu};" });
    console.log("safari:", JSON.stringify(state));
    if (shot) {
      const b64 = await wd("GET", `${s}/screenshot`);
      fs.mkdirSync(path.dirname(shot), { recursive: true });
      fs.writeFileSync(shot, Buffer.from(b64, "base64"));
      console.log("screenshot:", shot);
    }
    const fails = [];
    if (state.href.includes("unsupported.html")) fails.push("redirected to unsupported.html");
    if (!Array.isArray(state.errors)) fails.push("window.__parbleErrors missing");
    else if (state.errors.length) fails.push(`${state.errors.length} page-logged error(s): ${state.errors.join(" | ")}`);
    for (const f of fails) console.error("FAIL:", f);
    code = fails.length ? 1 : 0;
    if (!code) console.log("safari-smoke: PASS (pixels judged from the screenshot)");
  } catch (e) {
    console.error("FAIL:", e && e.message ? e.message : e);
  } finally {
    if (session) await wd("DELETE", `/session/${session}`).catch(() => {});
    driver.kill();
    if (server) server.kill();
    process.exit(code);
  }
})();
