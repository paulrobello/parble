.PHONY: build test lint lint-native lint-wasm fmt fmt-check typecheck ui-static ui-test checkall checkall-ci \
	wasm-build-check secrets-scan pre-commit pre-commit-update dev dev-start dev-restart dev-stop build-web web-size \
	audio-process audition

DEV_PORT := 5310
WASM := wasm32-unknown-unknown
UI := web/ui

build:
	cargo build --workspace
	cargo build --workspace --target $(WASM)

test:
	cargo test --workspace

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

# The wasm leg omits --all-targets: dev-dependencies (proptest) need not build for wasm32.
lint: lint-native lint-wasm

lint-native:
	cargo clippy --workspace --all-targets -- -D warnings

lint-wasm:
	cargo clippy --workspace --target $(WASM) -- -D warnings

typecheck:
	cargo check --workspace --all-targets
	cargo check --workspace --target $(WASM)
	@$(MAKE) --no-print-directory ui-static

# web/ui (Vite + TS + Biome + Vitest) arrives in P0-T7; until then these legs skip loudly.
ui-static:
	@if [ -f $(UI)/package.json ]; then \
	  cd $(UI) && npm ci --no-audit --no-fund && ./node_modules/.bin/biome check . && ./node_modules/.bin/tsc --noEmit -p .; \
	else echo "SKIP ui-static: $(UI)/package.json absent (added by P0-T7)"; fi

ui-test:
	@if [ -f $(UI)/package.json ]; then \
	  cd $(UI) && ./node_modules/.bin/vitest run; \
	else echo "SKIP ui-test: $(UI)/package.json absent (added by P0-T7)"; fi

# Fast gate, fail-fast order: fmt → clippy (native + wasm32) → Biome/tsc → cargo test → Vitest.
checkall:
	@$(MAKE) --no-print-directory fmt-check
	@$(MAKE) --no-print-directory lint
	@$(MAKE) --no-print-directory ui-static
	@$(MAKE) --no-print-directory test
	@$(MAKE) --no-print-directory ui-test

# Push gate: checkall + the CI-parity legs that exist today. P0-T4 adds determinism-wasm,
# P0-T5 adds web-size (needs index.html), P0-T8 adds actionlint.
checkall-ci: checkall wasm-build-check secrets-scan

wasm-build-check:
	cargo build --workspace --locked --target $(WASM)

secrets-scan:
	gitleaks git --no-banner --redact .

pre-commit:
	pre-commit run --all-files

pre-commit-update:
	pre-commit autoupdate

# Trunk dev server on 127.0.0.1:$(DEV_PORT). index.html and Trunk.toml land in P0-T5.
dev:
	@test -f index.html || { echo "ERROR: index.html missing (added by P0-T5); cannot run trunk serve"; exit 1; }
	trunk serve --address 127.0.0.1 --port $(DEV_PORT)

dev-start: dev

dev-restart:
	@$(MAKE) --no-print-directory dev-stop
	@$(MAKE) --no-print-directory dev

# TERM the listener, poll up to 2 s for the port to free, then SIGKILL.
dev-stop:
	@pids=$$(lsof -ti tcp:$(DEV_PORT) -sTCP:LISTEN); \
	if [ -z "$$pids" ]; then echo "port $(DEV_PORT) already free"; exit 0; fi; \
	kill $$pids 2>/dev/null; \
	for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do \
	  lsof -ti tcp:$(DEV_PORT) -sTCP:LISTEN >/dev/null || { echo "port $(DEV_PORT) free"; exit 0; }; \
	  sleep 0.1; \
	done; \
	pids=$$(lsof -ti tcp:$(DEV_PORT) -sTCP:LISTEN); \
	if [ -n "$$pids" ]; then echo "SIGKILL $$pids"; kill -9 $$pids 2>/dev/null; fi; \
	sleep 0.2; \
	if lsof -ti tcp:$(DEV_PORT) -sTCP:LISTEN >/dev/null; then echo "ERROR: port $(DEV_PORT) still busy"; exit 1; fi; \
	echo "port $(DEV_PORT) free"

build-web:
	@test -f index.html || { echo "ERROR: index.html missing (added by P0-T5); cannot run trunk build"; exit 1; }
	trunk build --release

# PRD §6 P0 budget: wasm <= 15 MB gzip.
web-size:
	@rm -rf dist
	@$(MAKE) --no-print-directory build-web
	@wasm=$$(ls dist/*.wasm); raw=$$(wc -c < $$wasm | tr -d ' '); gz=$$(gzip -9 -c $$wasm | wc -c | tr -d ' '); \
	echo "wasm raw=$$raw gzip=$$gz"; \
	test $$gz -le 15000000 || { echo "FAIL: wasm gzip $$gz > 15 MB budget"; exit 1; }

# P4-T2: process assets-source/audio originals into audition/ (originals are never modified).
audio-process:
	python3 scripts/audio/process.py

# Audition page on the reserved parble port (Trunk dev server is not built yet).
audition:
	@test -f audition/sfx/manifest.json || { echo "ERROR: run make audio-process first"; exit 1; }
	@echo "audition page: http://127.0.0.1:$(DEV_PORT)/"
	python3 -m http.server $(DEV_PORT) --bind 127.0.0.1 --directory audition
