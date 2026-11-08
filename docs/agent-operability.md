# Agent operability

parble can be launched, observed and stopped from a script, on native and on the web, without anyone at the keyboard (P0-T6). The switches are the same on both platforms. Natively they are flags. On the web they are URL query parameters.

## Switches

| Flag (native) | Query (web) | Default | Effect |
|---|---|---|---|
| `--screenshot <path>` | `screenshot=<name>` | off | Capture the primary window once. Natively the PNG is written to `<path>` and parent directories are created. On the web the browser downloads `<name>`. |
| `--screenshot-after <s>` | `screenshot-after=<s>` | `3` | Seconds of real time after startup before the capture. The first frames show only the backdrop while pipelines compile, so stay at 3 s or more. |
| `--exit-after <s>` | `exit-after=<s>` | off | Exit after `<s>` seconds of real time. If a screenshot is still pending, the app waits for it, up to 10 s longer. |
| `--scenario <name>` | `scenario=<name>` | `ramp` | Scene to load. P0 has only `ramp`, the tray ramp with one standard marble, which restarts every 10 s. |
| `--seed <u64>` | `seed=<u64>` | `0` | Recorded and reported. No P0 scenario uses it yet. Procedural tracks consume it from P3. |
| `--quality <tier>` | `quality=<tier>` | `medium` | `low`: no shadows and no MSAA. `medium`: a 2048² shadow map and 4× MSAA. `high`: a 4096² shadow map and 4× MSAA. |
| `--dump-state` | `dump-state` (or `=1`) | off | At auto-exit, print the state JSON. Natively it goes to stdout as one line. On the web it is logged to the console as `parble state: {...}`. |

Native flags accept both `--key value` and `--key=value`. An unknown flag or a bad value prints an error and exits with code 2. Unknown query keys are ignored on the web. A bad query value logs an error and the app falls back to the defaults.

When `--screenshot` or `--exit-after` is set, the app redraws continuously even if its window never gets focus.

## Exit codes (native)

- `0`: clean exit. If a screenshot was requested, it was written and shows a lit scene.
- `2`: bad flags.
- `3`: the screenshot failed. Either it never arrived, writing it failed, or it is not a lit scene.

The lit-scene check uses the same thresholds as the web smoke. More than 50 % of pixels must be non-black (max channel above 8), more than 2 % must be lit (max channel above 60), and the frame must hold more than 16 distinct 4-bit colours. The backdrop alone passes the first test and fails the other two.

## State JSON and `window.__parble`

The web build replaces `window.__parble` every frame. `--dump-state` prints the same object. This one is from `make run-diag`:

```json
{
  "ready": true,
  "tick": 682,
  "run": 0,
  "frame": 249,
  "fps": 124.9248,
  "hash": "3f6e35c78c10f020",
  "lastRunHash": null,
  "marbles": [{"position": [0.34963298, 0.007915289, -1.3966178e-07], "rotation": [1.4082439e-06, 3.885606e-08, 0.36115447, -0.932506], "linvel": [0.25444952, 0.0, -1.9141713e-07]}],
  "scenario": "ramp",
  "track": null,
  "trackRun": null,
  "seed": 0,
  "quality": "medium",
  "screenshot": {
    "state": "none"
  },
  "exited": true
}
```

- `ready`: the sim has ticked and at least 3 frames have run. Pixels can lag behind it while pipelines compile, so a pixel check still needs to poll.
- `tick`: sim ticks in the current run (240 Hz).
- `run`: completed 10 s runs.
- `hash`: the trajectory hash of the current run so far, as 16 hex digits. It is a string because a `u64` does not fit a JS number.
- `lastRunHash`: the hash of the last completed run. A run always stops at exactly 2,400 ticks, so for `ramp` this equals the golden hash in `crates/parble-sim/tests/golden/ramp_10s.hash`.
- `marbles`: one entry per marble in spawn order. Positions are in metres, rotation is a quaternion `[x, y, z, w]`, and `linvel` is in m/s.
- `track`: the metadata name of the `.parble` track loaded over the bridge (`loadTrack`), or `null`. A loaded track replaces the scenario in the live sim and its 10 s restarts.
- `trackRun`: the last bridge `runTrack`, as `{ "ticks": 2400, "hash": "…" }`, or `null`. The run is headless from tick 0, so its hash does not depend on frame timing. `web/ui/src/parble-fixtures.json` records the expected `runHash` of every valid fixture track; `make web-e2e` checks it in the browser (`e2e/tests/persistence.spec.mjs`).
- `screenshot`: `state` is one of `none`, `pending`, `capturing`, `done` (with `lit`, `colors` and `ok`), or `failed` (with `error`).
- `exited`: set when auto-exit fires.

All timing (screenshot, exit, fps) is real time in the app layer. The sim never receives wall-clock input, so none of these switches can change a trajectory hash.

## Make targets

```bash
make run-shot                        # native: PNG to target/shots/run-shot.png, then exit (non-zero unless lit)
make run-shot SHOT=docs/shots/x.png  # another path
make run-diag                        # native: run 3 s, print the state JSON, exit
make web-e2e                         # release build, then the Playwright suites (smoke, persistence) in GPU Chromium on 5311
make web-smoke                       # the P0-T5 single-script smoke (random port)
```

The browser suites are local only and never run in CI. `e2e/` has its own package, pinning `@playwright/test` 1.64.1. Install it with `make e2e-deps`. Chromium launches with `channel: 'chromium'` and the platform GPU flags (`--use-angle=metal --enable-gpu --ignore-gpu-blocklist` on macOS). `PW_GPU=swiftshader` opts into the CPU renderer.

`make web-e2e` has Playwright start `python3 -m http.server` on 127.0.0.1:5311 over `dist/` and stop it afterwards. The URL switches can be tried by hand on any served build, for example `http://127.0.0.1:5310/?quality=low&dump-state&exit-after=5`.
