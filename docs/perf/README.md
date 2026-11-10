# Perf gate (P1-T8)

`make perf-gate` runs `e2e/perf-gate.mjs`: the paced p95 of 600 `requestAnimationFrame` deltas after READY, at 2560 × 1440 device pixels on the High tier (`?mute=1&quality=high&scenario=…`), on the `showcase` and `stress2000` scenes, in two browsers:

- **GPU Chromium**: Playwright's `chromium` channel with the platform GPU flags from `e2e/gpu-args.cjs` (`--use-angle=metal --enable-gpu --ignore-gpu-blocklist` on macOS), viewport 2560 × 1440 at device scale 1, headless unless `PARBLE_PERF_HEADED=1`. The run fails if the WebGPU adapter is missing or SwiftShader shows up; each sample records the adapter and the WebGL renderer string.
- **Safari**: real Safari through `safaridriver`'s W3C WebDriver (the pattern of `e2e/safari-smoke.cjs`), not Playwright WebKit. Safari has no device-scale override, so the window is sized from `devicePixelRatio`: a 1280 × 720 CSS viewport on a Retina screen. The gate checks the canvas really is 2560 × 1440 and reports a problem when it is not (a non-Retina screen smaller than 2560 × 1440 cannot give it).

The sampler waits for `window.__parble.ready`, skips 60 frames (`PARBLE_PERF_SETTLE_FRAMES`), then records 600 deltas. The page is served by `e2e/static-server.py` on the gate's own port, **5313** (reserved in `~/.claude/used_ports.md`), never the 5311 preview the Playwright suites share.

## Pass rule

The criterion is p95 ≤ 16.7 ms. A rAF delta is a whole number of display refreshes plus the browser's timestamp quantum: Chromium coarsens rAF times to 0.1 ms, so a frame on every 60 Hz vsync reads 16.6 to 16.8 ms, and Safari to 1 ms (16 or 17 ms). A missed refresh at 60 Hz reads 25 ms or more. Each sample therefore passes when its p95 is at most 16.7 ms plus the browser's quantum (`jitterMs`: 0.3 ms for both, approved by Paul on 2026-11-08), meaning the 95th-percentile frame made its vsync. With Safari's 1 ms quantum this passes a p95 that reads 16 or 17 ms and fails one that reads 18 ms. The raw p95 is recorded unrounded next to the threshold it was judged against. This allowance is a reading of the criterion, not the criterion itself, so each sample (and the whole run) also records `passStrict`: p95 rounded to 0.1 ms at most 16.7 ms, with no allowance. A Safari run can only pass strictly when its p95 frame reads 16 ms. The gate's exit status follows `pass`; report both.

## Load guard

The host's 1-minute load average must be at most 15 both before and after every sample. Before each sample the gate waits, polling every 15 s for up to `PARBLE_LOAD_WAIT_MS` (20 min by default), and records the sample as `taken: false` if the load never drops. A sample whose post-run load exceeded 15 is discarded (kept in `discarded` with its loads and p95) and re-run, up to `PARBLE_PERF_ATTEMPTS` (3) times. Every sample records `loadBefore` and `loadAfter`. The gate passes only when every sample was taken, has no problems (canvas size, quality tier, page errors, GPU) and passes the rule above.

Output: `docs/perf/p1-gate.json`. `make perf-gate-smoke` (`PARBLE_PERF_SMOKE=1`) runs the same samples once each with no load wait, marks the file `gating: false`, and writes `target/perf/p1-gate-smoke.json` instead, so a smoke result never lands where a gate result belongs.

## What the scenes put on screen

- `showcase`: the all-pieces run (52 pieces) on the playroom rug with its lights, under the follow camera. Over the 10 s sampled after READY the hopper has released about 5 of its 10 marbles (one every 2 s).
- `stress2000`: 2,000 kit pieces in 41 chunks (the all-pieces track plus a 20-layer render-only tower) under one fixed set shot that keeps all of them in frame. The same hopper timing applies: about 5 marbles are moving during the sample, all 10 by 27 s.

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
  --exclude 'target/shots' --exclude .claude .
rsync -az -e "ssh -i <ssh-key>" /tmp/parble-perf.tgz <user>@gpu-host-b.local:/tmp/
# If "No route to host": add -o ProxyJump=<user>@gpu-host-a.local (infrastructure guide).
```

On gpu-host-b, launch the run in the console (GUI) session with `open`. A `safaridriver` started from an ssh shell cannot reach Safari: every session request times out with "Request creation of a new automation session" (measured 2026-11-09). gpu-host-b has no par-mux, and `open` also keeps the run alive through an ssh drop. Quit Safari first if it was running before remote automation was enabled. A Safari process that predates the setting times out the same way.

```bash
ssh -i <ssh-key> <user>@gpu-host-b.local
rm -rf ~/Repos/parble-perf && mkdir -p ~/Repos/parble-perf && tar xzf /tmp/parble-perf.tgz -C ~/Repos/parble-perf
cat > /tmp/parble-perf.command <<'CMD'
#!/bin/bash
cd ~/Repos/parble-perf && export PATH=/opt/homebrew/bin:$PATH
{ npm ci --prefix e2e --no-audit --no-fund && (cd e2e && ./node_modules/.bin/playwright install chromium) && node e2e/perf-gate.mjs; } > /tmp/parble-perf.log 2>&1
echo EXIT=$? >> /tmp/parble-perf.log
CMD
chmod +x /tmp/parble-perf.command && open -g /tmp/parble-perf.command
tail -f /tmp/parble-perf.log
```

`node e2e/perf-gate.mjs` is what `make perf-gate` runs after `build-web`; calling it directly uses the shipped `target/web-dist`. Copy the result back and clean up:

```bash
scp -i <ssh-key> <user>@gpu-host-b.local:Repos/parble-perf/docs/perf/p1-gate.json docs/perf/p1-gate.json
ssh -i <ssh-key> <user>@gpu-host-b.local 'rm -rf ~/Repos/parble-perf /tmp/parble-perf.tgz /tmp/parble-perf.log /tmp/parble-perf.command'
```

Headless Chromium paces rAF from its own 60 Hz frame source, which may not show GPU back-pressure. Headed Chromium follows the display: on a 120 Hz ProMotion screen it paces at 8.3 ms (a 2026-11-08 non-gating smoke on gpu-host-a's M4 Max read p95 10.1 ms showcase and 9.9 ms stress2000 headed, against 16.7 / 16.8 ms headless), and the jitter allowance above assumes 60 Hz. Run a second, headed Chromium pass and keep it as a separate baseline (house rule: headed and headless timings are never mixed), written to its own file:

```bash
PARBLE_PERF_HEADED=1 PARBLE_PERF_BROWSERS=chromium PARBLE_PERF_OUT=docs/perf/p1-gate-chromium-headed.json node e2e/perf-gate.mjs
```

Useful overrides for a partial run: `PARBLE_PERF_BROWSERS=safari`, `PARBLE_PERF_SCENES=stress2000`, `PARBLE_PERF_HEADED=1`.

## Results

2026-11-08 on gpu-host-b (Apple M4, 10-core GPU; the adapter read `apple metal-3`, the renderer ANGLE Metal on the Apple M4), at 2560 × 1440 on the High tier, under the load guard:

| Run | Scene | p95 | p50 | Dropped | Load before → after | pass | passStrict |
|---|---|---|---|---|---|---|---|
| Headless Chromium (gating, `p1-gate.json`) | showcase | 16.8 ms | 16.7 ms | 1/600 | 2.64 → 2.55 | true | false |
| Headless Chromium (gating) | stress2000 | 16.8 ms | 16.7 ms | 1/600 | 2.55 → 2.58 | true | false |
| Headed Chromium (baseline, `p1-gate-chromium-headed.json`) | showcase | 18.6 ms | 16.7 ms | 1/600 | 2.46 → 3.77 | false | false |
| Headed Chromium (baseline) | stress2000 | 18.3 ms | 16.7 ms | 1/600 | 3.77 → 3.92 | false | false |

The headless pass is within the 0.3 ms allowance: the p95 frame made its vsync, and only one frame in 600 missed one. The headed run also missed only one refresh per scene, but its rAF deltas jitter between 16.7 and about 18.6 ms around the display's vsync, so its p95 is over the allowance. That is the headed baseline, kept apart from the gate, not a frame miss.

2026-11-09 on gpu-host-b (same host, Safari 27.2), build c86a1ef, under the load guard:

| Run | Scene | p95 | p50 | Max | Dropped | Load before → after | pass | passStrict |
|---|---|---|---|---|---|---|---|---|
| Headless Chromium (gating, `p1-gate.json`) | showcase | 16.8 ms | 16.7 ms | 33.4 ms | 1/600 | 1.69 → 2.33 | true | false |
| Headless Chromium (gating) | stress2000 | 16.8 ms | 16.7 ms | 33.3 ms | 1/600 | 2.33 → 2.32 | true | false |
| Safari (gating, `p1-gate.json`) | stress2000 | 17.0 ms | 17.0 ms | 1103 ms | 1/600 | 1.72 → 2.03 | true | false |
| Safari (gating, `p1-gate-safari-showcase.json`) | showcase | 17.0 ms | 17.0 ms | 1115 ms | 1/600 | 1.74 → 1.87 | true | false |

All four samples pass within the allowance. In the full run the Safari showcase sample failed with a WebDriver `fetch failed` after the stress2000 sample, so `p1-gate.json` records `pass: false` for that one problem. A Safari-only showcase rerun in the same booked window passed and is in its own file. Each Safari sample has one dropped frame of about 1.1 s. It is a single stall in 600 frames, not a pacing miss.

### The first-scene stall (2026-11-09)

The stall was a render pipeline created mid-run. Bevy creates a material's pipelines only for meshes some view draws. The hopper's unreleased marbles were hidden, so the opal marble (the second one, released at tick 480 = 2 s) brought new pipelines when it first appeared. Its shell is the only look with diffuse transmission, so it needed the `pbr_opaque_mesh_pipeline` variant with `STANDARD_MATERIAL_DIFFUSE_TRANSMISSION`, plus a directional-shadow `pbr_prepass_pipeline` variant for its core. On wasm Bevy creates pipelines synchronously on the main thread, inside `PipelineCache::process_queue`. In a cold GPU Chromium trace (`e2e/first-frames.mjs` with `PARBLE_FF_TRACE=1`), Dawn's `DeviceBase::APICreateRenderPipeline` ran 7.1 ms at the opal's release and 0.3 ms about 1.2 s later, and the page's frames stayed at 16.7 ms. In Chromium the cost lands in the GPU process. Safari's 1.1 s frame, once per browser session and gone with 600 settle frames, is consistent with the same compile done in the page's own frame.

`window.__parble.pipelines` now lists the pipelines each render frame creates, and READY waits for a loading warm-up: every unreleased marble is drawn 50 m under the floor (with `NoFrustumCulling`, so the main, prepass and shadow views all specialize it) until the kit textures have loaded and the pipeline cache has been quiet for 10 render frames. The READY that the gate waits for now arrives after the last pipeline is created. On a cold GPU Chromium load the last pipeline is created on render frame 4. Over the 1,500 frames after READY, with all 10 marbles released, none is created in `showcase` or in `stress2000`. Before the change, the post-processing and atmosphere pipelines were created on the first frames after READY (frames of 150 to 180 ms that the 60 settle frames hid), and the opal's variants 107 to 183 frames after READY (when the opal came into the camera's view).

Headed Chromium on a Retina screen ignores the page's `deviceScaleFactor: 1` and draws a 5120 × 2880 canvas, so a headed run passes `--force-device-scale-factor=1`. The first headed run, before that flag, was rejected by the canvas check.

### Safari never reached READY (2026-11-09)

The gpu-host-b run at 11:52 PM PST on 2026-11-09 (Safari 27.2, 2560 × 1440 High, load 1.9 to 4.3) never saw READY on either scene within the sampler's 180 s, while Chromium read READY at 1722 ms (showcase) and 1021 ms (stress2000). Before the warm-up above, Safari reached READY. So in Safari the warm-up never ended: a texture stayed loading, or the pipeline cache never went quiet.

What the code showed, without Safari:

- The pending count was wrong for the case that matters. On wasm Bevy creates a pipeline synchronously, so `Creating` never happens. A pipeline whose shader or shader import has not loaded fails with a retryable error (`ShaderNotLoaded`, `ShaderImportNotYetAvailable`) and is reset to `Queued` on the next frame. It alternates between the two states every frame, and the old count called the error frames `failed`. One such pipeline let `pending` reach 0 on every other frame. Two that started waiting on frames of different parity kept it at 1 or more forever. The retryable errors now count as pending.
- Nothing bounded the warm-up. A pipeline waiting for a shader that never loads (in one browser only, through a shader def or import that browser's device selects), or a texture that never finishes, held READY forever.

The warm-up now has two bounds (`crates/parble-render/src/pipelines.rs`), counted in render frames so a native lockstep run ends it on the same frame whatever the host load:

- `STUCK_FRAMES` = 120. A pipeline still waiting after 120 render frames (2 s at 60 Hz) is marked `stuck`, logged with its shader error, and no longer holds the warm-up. A cold GPU Chromium load creates its last pipeline on render frame 4 or 5.
- `WARMUP_MAX_FRAMES` = 600, the hard ceiling. The warm-up ends on render frame 600 (10 s at 60 Hz) whatever is still loading or pending, with `warmup.endedBy: "ceiling"` and a warning naming what it waited on. A normal load ends quietly on render frame 13 to 17, so the ceiling is about 40 times that, and above `STUCK_FRAMES` + `QUIET_FRAMES` for a pipeline first queued late. The opal hitch the warm-up exists to prevent is still covered on any load that ends quietly.

READY and the parked warm-up marbles were the only things waiting on the warm-up: marble release and the sim do not read it, and the UI's bridge `ready` event is sent unconditionally (`announce_bridge` in `crates/parble-app/src/main.rs`). A player in Safari was therefore never stuck. The cost was the hopper's unreleased marbles drawn 50 m under the floor, in every view and shadow view, for the whole session.

After the fix, on gpu-host-a (M4 Max) in GPU Chromium, non-gating smoke at load 27 to 42: READY at 1340 ms (showcase) and 788 ms (stress2000), warm-up `endedBy: "quiet"` on render frames 15 and 17, p95 16.8 ms on both, and no pipeline created after READY (`createdTotal` equals `total`). Native `make run-diag`: `doneFrame` 13, `endedBy: "quiet"`.

#### Diagnosing it in Safari

`e2e/safari-diag.mjs` opens Safari (safaridriver, sized like the gate), loads `?scenario=<scene>&quality=high&mute=1` for `showcase` and `stress2000`, polls `window.__parble` once a second for 60 s, and writes the time series to `target/perf/safari-diag.json`, with a screenshot per scene next to it. Each poll records `ready`, `tick`, `frame`, `warmup` (`waitingOn`, `endedBy`, `stuck`), the pipeline counts with `waiting` (each pending pipeline's name, state, shader error and `sinceFrame`) and `failures`, the kit textures that are not loaded, and the page-side error count. `last` keeps the full final state with `recentCreated`. Safari's WebDriver has no console or log endpoint, so `window.__parbleErrors` (console.error, Bevy ERROR lines, uncaught errors, CSP blocks) is the only console output it can read; the warm-up's own warnings reach the JSON through `warmup` and `pipelines`, not the console. `PARBLE_DIAG_BROWSER=chromium` runs the same script in GPU Chromium. `e2e/perf-gate.mjs` now also writes this state (`diag`) for a sample whose READY times out, and records `warmup` and `pipelines` for every sample that reaches READY, so a Safari READY that came from the ceiling shows `endedBy: "ceiling"`.

Run on gpu-host-b in the booked window. Ship the build from gpu-host-a, from a checkout with this fix, exactly as above (`make build-web`, then the `tar` and `rsync`), then on gpu-host-b:

```bash
ssh -i <ssh-key> <user>@gpu-host-b.local
rm -rf ~/Repos/parble-perf && mkdir -p ~/Repos/parble-perf && tar xzf /tmp/parble-perf.tgz -C ~/Repos/parble-perf
killall Safari 2>/dev/null || true
cat > /tmp/parble-diag.command <<'CMD'
#!/bin/bash
cd ~/Repos/parble-perf && export PATH=/opt/homebrew/bin:$PATH
{ npm ci --prefix e2e --no-audit --no-fund && node e2e/safari-diag.mjs; } > /tmp/parble-diag.log 2>&1
echo EXIT=$? >> /tmp/parble-diag.log
CMD
chmod +x /tmp/parble-diag.command && rm -f /tmp/parble-diag.log && open -g /tmp/parble-diag.command
for i in $(seq 1 120); do grep -q '^EXIT=' /tmp/parble-diag.log 2>/dev/null && break; sleep 5; done; cat /tmp/parble-diag.log
```

The diag takes about 2.5 minutes after `npm ci`, and writes `safari-diag.json` after each scene, so a failed second session keeps the first scene's data. Then the gate, Safari only, into its own file so it never overwrites the gating `p1-gate.json`:

```bash
cat > /tmp/parble-perf.command <<'CMD'
#!/bin/bash
cd ~/Repos/parble-perf && export PATH=/opt/homebrew/bin:$PATH
{ npm ci --prefix e2e --no-audit --no-fund && PARBLE_PERF_BROWSERS=safari PARBLE_PERF_OUT=docs/perf/p1-gate-safari.json node e2e/perf-gate.mjs; } > /tmp/parble-perf.log 2>&1
echo EXIT=$? >> /tmp/parble-perf.log
CMD
chmod +x /tmp/parble-perf.command && rm -f /tmp/parble-perf.log && open -g /tmp/parble-perf.command
for i in $(seq 1 480); do grep -q '^EXIT=' /tmp/parble-perf.log 2>/dev/null && break; sleep 5; done; cat /tmp/parble-perf.log
exit
```

Back on gpu-host-a, copy the results and clean up:

```bash
mkdir -p target/perf
scp -i <ssh-key> '<user>@gpu-host-b.local:Repos/parble-perf/target/perf/safari-diag*' target/perf/
scp -i <ssh-key> <user>@gpu-host-b.local:Repos/parble-perf/docs/perf/p1-gate-safari.json docs/perf/p1-gate-safari.json
scp -i <ssh-key> <user>@gpu-host-b.local:/tmp/parble-diag.log <user>@gpu-host-b.local:/tmp/parble-perf.log target/perf/
ssh -i <ssh-key> <user>@gpu-host-b.local 'rm -rf ~/Repos/parble-perf /tmp/parble-perf.tgz /tmp/parble-perf.log /tmp/parble-perf.command /tmp/parble-diag.log /tmp/parble-diag.command'
```

Read `scenes[].last.warmup` in `safari-diag.json` first. `endedBy: "quiet"` with an empty `stuck` means nothing waited 120 frames in that run, so the hang did not reproduce. A non-empty `stuck`, or `waitingOn: "pending"` at the ceiling, names the pipeline and its shader error. `waitingOn: "textures"` with `texturesNotLoaded` names the texture. A `pipelines.createdTotal` still climbing between polls means pipelines keep being recreated. The gate log prints each sample's warm-up end (`endedBy`, render frame, stuck count) under its p95 line: a `ceiling` end can still hitch mid-run even when the p95 passes.
