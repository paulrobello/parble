# CLAUDE.md

Guidance for coding agents working in this repository.

parble is a browser 3D wooden marble-run sandbox: a Rust workspace compiled to wasm32, Bevy `=0.20.1` (`default-features = false`) rendering through WebGPU (no WebGL fallback), served by Trunk. A native build of `parble-app` is kept working for fast iteration, screenshots and determinism tests.

## Authoritative docs

- `PRD.md` — product plan. §6 lists the phases and their acceptance boxes.
- `plans/phase-*.md` — per-phase task plans, including the version Pins table in `plans/phase-0-foundations.md`.
- `docs/reference/` — visual references for look-dev.

## Commands

```bash
make build          # cargo build, native and wasm32-unknown-unknown
make test           # cargo test --workspace
make fmt            # cargo fmt --all
make lint           # clippy -D warnings, native (--all-targets) and wasm32
make typecheck      # cargo check native + wasm32, plus web/ui Biome/tsc once it exists

make checkall       # fast gate: fmt-check → clippy (native + wasm32) → Biome/tsc → cargo test → Vitest
make checkall-ci    # push gate: actionlint (pinned, target/tools) + checkall + wasm32 --locked build + gitleaks history scan + web-test + determinism-wasm + web-size
make pre-commit     # pre-commit run --all-files

make dev            # trunk serve on 127.0.0.1:5310 (needs index.html, added in P0-T5)
make dev-stop       # stop the 5310 listener: TERM, poll ≤ 2 s, then SIGKILL
make build-web      # trunk build --release into dist/
make web-size       # build-web + wasm ≤ 15 MB gzip budget

make run-shot       # native screenshot to target/shots/run-shot.png, then exits (non-zero unless lit)
make run-diag       # native: run 3 s, print the state JSON, exit
make web-e2e        # build-web + Playwright smoke in GPU Chromium on the 5311 preview (local only)
```

- Agent switches (native flags, URL query on web, `window.__parble`): `docs/agent-operability.md`.

- `checkall` legs run in fail-fast order. The web/ui legs (Biome, tsc, Vitest) print `SKIP` until `web/ui/package.json` exists (P0-T7).
- `checkall-ci` grows as Phase 0 lands: P0-T4 added `determinism-wasm`, P0-T5 `web-size`, P0-T8 `actionlint` (first leg). Keep it a superset of what `.github/workflows/ci.yml` runs.
- Capture real exit codes when running a gate: `make checkall > /tmp/x.log 2>&1; echo EXIT=$?`. Never trust a piped exit status.

## Ports

- 5310 — Trunk dev server (`make dev`).
- 5311 — static preview of `dist/` for Playwright smoke, shots and the perf gate.
- 5312 — standalone audio audition page (`make audition`).

All three are reserved in `~/.claude/used_ports.md`.

## Architecture

Workspace crates (`crates/`):

- `parble-core` — cell grid, ports, piece definitions, the track model, seeded RNG. No Bevy, no Rapier.
- `parble-kit` — kit piece catalogue and collision sidecars.
- `parble-sim` — deterministic Rapier sim at a fixed 240 Hz, trajectory hash.
- `parble-procgen` — procedural track generation.
- `parble-sim-wasm` — wasm-bindgen build of the sim for the Node determinism check.
- `parble-render` — Bevy rendering.
- `parble-app` — the Bevy app (`parble` binary), entry for native and web.

## Conventions

- Dependency versions are pinned once in the root `[workspace.dependencies]`; crates use `{ workspace = true }`. Change a pin only with a recorded reason in the commit message.
- Every crate must compile natively and for `wasm32-unknown-unknown`.
- Determinism: the sim takes no wall-clock input, uses `libm` for trig, and Rapier with `enhanced-determinism` (no `parallel`, no `simd-*`). One golden hash for every platform, never per-platform goldens.
- Browser/e2e suites (Playwright) run locally on GPU Chromium only, never in CI.
- Commit `Cargo.lock`. Commit after each verified unit of work.
- **Paul must approve every push.** Queue commits locally until he does. The GitHub repo is created in P5-T5, not before.
