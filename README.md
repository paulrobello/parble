# parble

A browser 3D wooden marble-run sandbox: build or generate modular wooden tracks, drop marbles, and watch a cinematic follow camera. Rust + Bevy → WebGPU.

Status: Phase 0 (foundations). The Cargo workspace and gates exist; the sim and web shell are in progress. See [PRD.md](PRD.md) for the product plan, [plans/](plans/) for per-phase implementation plans, and [docs/reference/](docs/reference/) for the visual references.

## Development

Requires the Rust stable toolchain (pinned by `rust-toolchain.toml`, with the `wasm32-unknown-unknown` target) and, for the web build, Trunk.

```bash
make checkall      # fast gate
make checkall-ci   # CI-parity gate
make dev           # Trunk dev server on http://127.0.0.1:5310
```

## License

MIT, see [LICENSE](LICENSE).
