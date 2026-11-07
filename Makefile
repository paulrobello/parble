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

# P1-T1 wood material set. Local only (macOS arm64): needs Blender 5.2 and the pinned KTX-Software 4.4.2
# in target/tools (make fetch-tools). Not part of checkall/checkall-ci, which run on CI's Linux host.
.PHONY: fetch-tools materials-build materials-check
BLENDER ?= /Applications/Blender.app/Contents/MacOS/Blender
BLENDER_RUN = "$(BLENDER)" -b --factory-startup --python-exit-code 1

fetch-tools:
	bash scripts/fetch-ktx.sh

materials-build: fetch-tools
	rm -rf target/materials-stage
	$(BLENDER_RUN) -P blender/materials/generate.py -- target/materials-stage assets/materials
	python3 blender/materials/compress.py
	$(BLENDER_RUN) -P blender/materials/check.py
	$(BLENDER_RUN) -P blender/materials/review.py
	python3 blender/materials/provenance.py assemble

# Seam/mirror gate (needs the mip extracts from materials-build) + provenance hashes + ktx validate.
materials-check: fetch-tools
	$(BLENDER_RUN) -P blender/materials/check.py
	python3 blender/materials/provenance.py check

# --- P1-T2 kit pipeline (blender/kit). Local only: needs Blender 5.2 and Node; not part of checkall.
# kit-build: headless generator + Cycles bakes -> meshopt GLB + sidecar + maps in assets/kit/<id>/,
# .blend + build reports in assets-source/kit/<id>/, then the Blender reimport gate and provenance.
.PHONY: kit-build kit-check kit-determinism kit-tools
KIT_BLENDER ?= /Applications/Blender.app/Contents/MacOS/Blender
KIT_RUN = "$(KIT_BLENDER)" -b --factory-startup --python-exit-code 1
KIT_TOOLS = blender/kit/tools
KIT_STAMP = $(KIT_TOOLS)/node_modules/.npm-ci.stamp

$(KIT_STAMP): $(KIT_TOOLS)/package.json $(KIT_TOOLS)/package-lock.json
	npm ci --prefix $(KIT_TOOLS) --no-audit --no-fund
	touch $@

kit-tools: $(KIT_STAMP)

kit-build: $(KIT_STAMP)
	rm -rf target/kit-stage
	mkdir -p target/kit-stage
	$(KIT_RUN) -P blender/kit/generate.py -- target/kit-stage
	node $(KIT_TOOLS)/kit.mjs compress target/kit-stage assets/kit assets-source/kit
	$(KIT_RUN) -P blender/kit/reimport.py -- assets/kit
	node $(KIT_TOOLS)/kit.mjs provenance assets/kit assets-source/kit \
	  --checks "kit-check contract 14.1-14.10,gltf-validator reimport (meshopt GLB),blender reimport (meshopt GLB)"

kit-check: $(KIT_STAMP)
	node $(KIT_TOOLS)/kit.mjs check assets/kit assets-source/kit

# Two fresh generator runs, one at a time, into separate dirs (bakes skipped: the GLB is image-free,
# so the maps are not in it). Stage GLBs, sidecars and meshopt GLBs must be byte-identical.
kit-determinism: $(KIT_STAMP)
	rm -rf target/kit-det
	mkdir -p target/kit-det/a target/kit-det/b
	PARBLE_SKIP_BAKE=1 $(KIT_RUN) -P blender/kit/generate.py -- target/kit-det/a
	PARBLE_SKIP_BAKE=1 $(KIT_RUN) -P blender/kit/generate.py -- target/kit-det/b
	node $(KIT_TOOLS)/kit.mjs compress target/kit-det/a target/kit-det/a-rt target/kit-det/a-src
	node $(KIT_TOOLS)/kit.mjs compress target/kit-det/b target/kit-det/b-rt target/kit-det/b-src
	@fail=0; for d in target/kit-det/a/*/; do id=$$(basename $$d); \
	  for f in $$id/$$id.glb $$id/$$id.kit.json; do \
	    cmp -s target/kit-det/a/$$f target/kit-det/b/$$f || { echo "FAIL: stage $$f differs"; fail=1; }; done; \
	  cmp -s target/kit-det/a-rt/$$id/$$id.glb target/kit-det/b-rt/$$id/$$id.glb || { echo "FAIL: meshopt $$id.glb differs"; fail=1; }; \
	  echo "$$id glb $$(shasum -a 256 target/kit-det/a-rt/$$id/$$id.glb | cut -c1-16) sidecar $$(shasum -a 256 target/kit-det/a/$$id/$$id.kit.json | cut -c1-16)"; \
	done; test $$fail = 0 && echo "kit-determinism: byte-identical"
