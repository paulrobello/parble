# Perf gate (P1-T8)

`make perf-gate` runs `e2e/perf-gate.mjs`: the paced p95 of 600 `requestAnimationFrame` deltas after READY, at 2560 × 1440 device pixels on the High tier (`?mute=1&quality=high&scenario=…`), on the `showcase` and `stress2000` scenes, in two browsers:

- **GPU Chromium**: Playwright's `chromium` channel with the platform GPU flags from `e2e/gpu-args.cjs` (`--use-angle=metal --enable-gpu --ignore-gpu-blocklist` on macOS), viewport 2560 × 1440 at device scale 1, headless unless `PARBLE_PERF_HEADED=1`. The run fails if the WebGPU adapter is missing or SwiftShader shows up; each sample records the adapter and the WebGL renderer string.
- **Safari**: real Safari through `safaridriver`'s W3C WebDriver (the pattern of `e2e/safari-smoke.cjs`), not Playwright WebKit. Safari has no device-scale override, so the window is sized from `devicePixelRatio`: a 1280 × 720 CSS viewport on a Retina screen. The gate checks the canvas really is 2560 × 1440 and reports a problem when it is not (a non-Retina screen smaller than 2560 × 1440 cannot give it).

The sampler waits for `window.__parble.ready`, skips 60 frames (`PARBLE_PERF_SETTLE_FRAMES`), then records 600 deltas. The page is served by `e2e/static-server.py` on the gate's own port, **5313** (reserved in `~/.claude/used_ports.md`), never the 5311 preview the Playwright suites share.

## Pass rule

The criterion is p95 ≤ 16.7 ms. A rAF delta is a whole number of display refreshes plus the browser's timestamp quantum: Chromium coarsens rAF times to 0.1 ms, so a frame on every 60 Hz vsync reads 16.6 to 16.8 ms, and Safari to 1 ms (16 or 17 ms). A missed refresh at 60 Hz reads 25 ms or more. Each sample therefore passes when its p95 is at most 16.7 ms plus the browser's quantum (`jitterMs`: Chromium 0.3 ms, Safari 1.0 ms), meaning the 95th-percentile frame made its vsync. The raw p95 is recorded unrounded next to the threshold it was judged against. This allowance is a reading of the criterion, not the criterion itself, so each sample (and the whole run) also records `passStrict`: p95 rounded to 0.1 ms at most 16.7 ms, with no allowance. A Safari run can only pass strictly when its p95 frame reads 16 ms. The gate's exit status follows `pass`; report both.

## Load guard

The host's 1-minute load average must be at most 15 both before and after every sample. Before each sample the gate waits, polling every 15 s for up to `PARBLE_LOAD_WAIT_MS` (20 min by default), and records the sample as `taken: false` if the load never drops. A sample whose post-run load exceeded 15 is discarded (kept in `discarded` with its loads and p95) and re-run, up to `PARBLE_PERF_ATTEMPTS` (3) times. Every sample records `loadBefore` and `loadAfter`. The gate passes only when every sample was taken, has no problems (canvas size, quality tier, page errors, GPU) and passes the rule above.

Output: `docs/perf/p1-gate.json`. `make perf-gate-smoke` (`PARBLE_PERF_SMOKE=1`) runs the same samples once each with no load wait, marks the file `gating: false`, and writes `target/perf/p1-gate-smoke.json` instead, so a smoke result never lands where a gate result belongs.

## What the scenes put on screen

- `showcase`: the all-pieces run (46 pieces) on the playroom rug with its lights, under the follow camera. Over the 10 s sampled after READY the hopper has released about 5 of its 10 marbles (one every 2 s).
- `stress2000`: 2,000 kit pieces in 40 chunks (the all-pieces track plus a 20-layer render-only tower) under one fixed set shot that keeps all of them in frame. The same hopper timing applies: about 5 marbles are moving during the sample, all 10 by 27 s.

## Hardware

The PRD criterion is "on M4 Max". gpu-host-a is the M4 Max but is usually far above load 15. gpu-host-b is an Apple **M4** (10-core GPU), not an M4 Max: a pass there is a stricter result than the criterion asks for, but a fail there does not disprove the M4 Max criterion. Record which host a result came from (`host.cpu` in the JSON).

## Running it on gpu-host-b

Book the window first (house load rules). gpu-host-b needs a logged-in console (GUI) session: Safari WebDriver and the Metal device need one, and Safari must stay frontmost, since a background window's rAF is throttled.

One-time setup on gpu-host-b, at the console:

1. Safari: Settings > Advanced > "Show features for web developers", then Develop > "Allow Remote Automation" (or `sudo safaridriver --enable`).
2. Node 24 or later on the PATH (`/opt/homebrew/bin/node`), and `python3`.

Ship the tree with the release build already in it (no Rust or Trunk needed on gpu-host-b), from this checkout on gpu-host-a:

```bash
make build-web
tar czf /tmp/parble-perf.tgz --exclude .git --exclude node_modules --exclude 'target/debug' \
  --exclude 'target/release' --exclude 'target/wasm32-unknown-unknown' --exclude 'target/tools' \
  --exclude 'target/shots' .
rsync -az -e "ssh -i <ssh-key>" /tmp/parble-perf.tgz <user>@gpu-host-b.local:/tmp/
# If "No route to host": add -o ProxyJump=<user>@gpu-host-a.local (infrastructure guide).
```

On gpu-host-b, in a par-mux pane so the run survives an ssh drop:

```bash
ssh -i <ssh-key> <user>@gpu-host-b.local
rm -rf ~/Repos/parble-perf && mkdir -p ~/Repos/parble-perf && tar xzf /tmp/parble-perf.tgz -C ~/Repos/parble-perf
par-mux --cmd "new-window -n parble-perf -- bash -lc 'cd ~/Repos/parble-perf && export PATH=/opt/homebrew/bin:\$PATH && npm ci --prefix e2e --no-audit --no-fund && (cd e2e && ./node_modules/.bin/playwright install chromium) && node e2e/perf-gate.mjs > /tmp/parble-perf.log 2>&1; echo EXIT=\$? >> /tmp/parble-perf.log'"
# (start the daemon first with `par-mux default --no-attach` if --cmd reports none)
tail -f /tmp/parble-perf.log   # or par-mux capture-pane
```

`node e2e/perf-gate.mjs` is what `make perf-gate` runs after `build-web`; calling it directly uses the shipped `target/web-dist`. Copy the result back and clean up:

```bash
scp -i <ssh-key> <user>@gpu-host-b.local:Repos/parble-perf/docs/perf/p1-gate.json docs/perf/p1-gate.json
ssh -i <ssh-key> <user>@gpu-host-b.local 'rm -rf ~/Repos/parble-perf /tmp/parble-perf.tgz /tmp/parble-perf.log'
```

Headless Chromium paces rAF from its own 60 Hz frame source, which may not show GPU back-pressure. Headed Chromium follows the display: on a 120 Hz ProMotion screen it paces at 8.3 ms (a 2026-11-08 non-gating smoke on gpu-host-a's M4 Max read p95 10.1 ms showcase and 9.9 ms stress2000 headed, against 16.7 / 16.8 ms headless), and the jitter allowance above assumes 60 Hz. Run a second, headed Chromium pass and keep it as a separate baseline (house rule: headed and headless timings are never mixed), written to its own file:

```bash
PARBLE_PERF_HEADED=1 PARBLE_PERF_BROWSERS=chromium PARBLE_PERF_OUT=docs/perf/p1-gate-chromium-headed.json node e2e/perf-gate.mjs
```

Useful overrides for a partial run: `PARBLE_PERF_BROWSERS=safari`, `PARBLE_PERF_SCENES=stress2000`, `PARBLE_PERF_HEADED=1`.
