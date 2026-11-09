.PHONY: build test lint lint-native lint-wasm fmt fmt-check typecheck ui-static ui-test ui-build checkall checkall-ci \
	wasm-build-check secrets-scan pre-commit pre-commit-update dev dev-start dev-restart dev-stop build-web web-size \
	audio-process audition

DEV_PORT := 5310
AUDITION_PORT := 5312
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

# web/ui (Vite + TS + Biome + Vitest): the HUD overlay and its typed wasm bridge. npm ci reruns only
# when the lockfile changes; ui-build is also Trunk's pre_build hook (Trunk.toml).
UI_NPM := $(UI)/node_modules/.npm-ci.stamp

$(UI_NPM): $(UI)/package.json $(UI)/package-lock.json
	npm ci --prefix $(UI) --no-audit --no-fund
	touch $@

ui-static: $(UI_NPM)
	cd $(UI) && ./node_modules/.bin/biome check . && ./node_modules/.bin/tsc --noEmit -p .

ui-test: $(UI_NPM)
	cd $(UI) && ./node_modules/.bin/vitest run

# The runtime SFX (scripts/web/stage-audio.mjs) are staged after Vite, which empties dist.
ui-build: $(UI_NPM)
	cd $(UI) && ./node_modules/.bin/vite build
	node scripts/web/stage-audio.mjs $(UI)/dist/audio

# Fast gate, fail-fast order: fmt → clippy (native + wasm32) → Biome/tsc → cargo test → Vitest → audio tests + provenance.
checkall:
	@$(MAKE) --no-print-directory fmt-check
	@$(MAKE) --no-print-directory lint
	@$(MAKE) --no-print-directory ui-static
	@$(MAKE) --no-print-directory test
	@$(MAKE) --no-print-directory ui-test
	@$(MAKE) --no-print-directory audio-test

# Audio pipeline unit tests and provenance reconciliation (needs numpy + ffmpeg; local only, not in CI).
.PHONY: audio-test
audio-test:
	python3 -m unittest discover -s scripts/audio -p 'test_*.py'
	python3 scripts/audio/validate_provenance.py

# Push gate: actionlint (cheap, first) + checkall + the CI-parity legs.
# web-size is a full Trunk release build: ~3.5 min cold, ~15 s warm.
checkall-ci: actionlint checkall wasm-build-check secrets-scan web-test determinism-wasm web-size

# P0-T8: lint .github/workflows with the pinned actionlint (scripts/fetch-actionlint.sh, never PATH).
.PHONY: actionlint
actionlint:
	@bash scripts/fetch-actionlint.sh
	target/tools/actionlint-1.7.12/actionlint

# P0-T4: the wasm build of the golden ramp run must hash the same as the native golden.
.PHONY: determinism-wasm
determinism-wasm:
	bash scripts/determinism/wasm.sh

wasm-build-check:
	cargo build --workspace --locked --target $(WASM)

secrets-scan:
	gitleaks git --no-banner --redact .

pre-commit:
	pre-commit run --all-files

pre-commit-update:
	pre-commit autoupdate

# Web shell (P0-T5). Trunk is pinned to 0.21.14 in target/tools (scripts/web/fetch-trunk.sh); a trunk
# on PATH is never used. Override the port for a second server: make dev DEV_PORT=5313.
.PHONY: fetch-trunk web-test web-smoke
TRUNK_VERSION := 0.21.14
TRUNK := target/tools/trunk-$(TRUNK_VERSION)/trunk

$(TRUNK):
	bash scripts/web/fetch-trunk.sh

fetch-trunk: $(TRUNK)

# node:test suites for the cache-busting and CSP scripts.
web-test:
	node --test scripts/web/*.test.mjs

# Browser suites (local only, never CI) use e2e/'s own pinned @playwright/test.
.PHONY: e2e-deps web-e2e run-shot run-diag playroom-shots
E2E_STAMP := e2e/node_modules/.npm-ci.stamp

$(E2E_STAMP): e2e/package.json e2e/package-lock.json
	npm ci --prefix e2e --no-audit --no-fund
	touch $@

e2e-deps: $(E2E_STAMP)

# GPU Chromium smoke of the release build (P0-T5).
web-smoke: build-web $(E2E_STAMP)
	node e2e/web-smoke.cjs "" docs/shots/p0-t5-chromium.png

# P0-T6 Playwright smoke against the 5311 static preview of target/web-dist (Playwright starts and stops it).
web-e2e: build-web $(E2E_STAMP)
	cd e2e && ./node_modules/.bin/playwright test

# P4-T5 manual music soak (local only, not in any gate): one muted loop for MUSIC_SOAK_S seconds in GPU
# Chromium, checked for gaps, restarts and errors (e2e/tests/music.spec.mjs). MUSIC_SOAK_STATE picks the cue.
MUSIC_SOAK_S ?= 600
MUSIC_SOAK_STATE ?= run
.PHONY: music-soak
music-soak: build-web $(E2E_STAMP)
	cd e2e && MUSIC_SOAK_S=$(MUSIC_SOAK_S) MUSIC_SOAK_STATE=$(MUSIC_SOAK_STATE) ./node_modules/.bin/playwright test tests/music.spec.mjs -g soak

# P0-T6 agent switches on the native build (docs/agent-operability.md). Both exit by themselves;
# run-shot exits non-zero unless the PNG shows a lit scene.
SHOT ?= target/shots/run-shot.png
run-shot:
	cargo run -p parble-app -- --screenshot $(SHOT) --screenshot-after 3 --exit-after 4

run-diag:
	cargo run -p parble-app -- --exit-after 3 --dump-state

# P1-T7 shots test (local only, needs numpy + Pillow): one lockstep frame per quality tier, the TAA
# ghost check on a max-speed marble, and the DoF side-by-side with ref-01. Writes docs/shots/p1-t7-*.
.PHONY: shots-p1t7
shots-p1t7:
	cargo build -p parble-app
	python3 scripts/shots/p1t7.py

# Look-dev shots test (local only, native). P2-T6: the mechanism showcase in lockstep, a toggle
# mid-flip and a one-way gate open, each checked against the part poses drawn on the shot frame.
# Writes target/shots/mechanisms; docs/shots/p2-t6-*.png are reviewed copies. P1-T8: the showcase
# and stress2000 at one fixed lockstep shot per tier, compared with docs/shots/p1-t8-*.png and
# across tiers; P5-T1 adds the marble lineup the same way, against docs/shots/p5-t1-lineup-<tier>.png
# (scripts/shots/p1t8.py; `--update [scene ...]` rewrites those references).
.PHONY: shots-test
shots-test:
	cargo build -p parble-app
	python3 scripts/shots/mechanisms.py
	python3 scripts/shots/p1t8.py

# P1-T6: the marble spec alone (GPU Chromium, every type on Low and High) against an existing target/web-dist.
.PHONY: web-editor-perf
web-editor-perf: $(E2E_STAMP)
	cd e2e && PARBLE_PERF=1 ./node_modules/.bin/playwright test tests/editor-perf.spec.mjs

# P1-T8 perf gate (local only, never CI): paced p95 over 600 frames after READY at 1440p High, showcase
# and stress2000, in GPU Chromium and Safari (safaridriver), under the host-load guard (1-min load
# <= 15 before and after each sample). Serves target/web-dist on its own port 5313. Writes
# docs/perf/p1-gate.json. `make perf-gate-smoke` is the non-gating smoke (no load wait) into target/perf.
.PHONY: perf-gate perf-gate-smoke
perf-gate: build-web $(E2E_STAMP)
	node e2e/perf-gate.mjs

perf-gate-smoke: build-web $(E2E_STAMP)
	PARBLE_PERF_SMOKE=1 node e2e/perf-gate.mjs

.PHONY: web-marbles
web-marbles: $(E2E_STAMP)
	cd e2e && ./node_modules/.bin/playwright test tests/marbles.spec.mjs

# P1-T6 / P5-T1: native close-up shots of every marble type at every tier, two sizes, the lineup
# scenario per tier and the glow bloom check (needs numpy + Pillow) into target/shots/marbles.
# shots-test checks the lineup against its per-tier references.
.PHONY: marble-shots
marble-shots:
	bash scripts/marble-shots.sh

# P1-T5: both playroom framings on High (native). Non-zero unless each shot is lit.
playroom-shots:
	cargo build -p parble-app
	./target/debug/parble --scenario playroom --view rug --quality high --screenshot target/shots/playroom-rug.png --screenshot-after 5 --exit-after 6
	./target/debug/parble --scenario playroom --view blocks --quality high --screenshot target/shots/playroom-blocks.png --screenshot-after 5 --exit-after 6

# Trunk dev server on 127.0.0.1:$(DEV_PORT). No autoreload: Trunk injects its reload script after the
# post_build hook hashed the inline scripts, so the page's CSP would block it (a console error).
# ui-build first: Trunk refuses to serve while a [watch] ignore path (web/ui/dist) is missing.
dev: $(TRUNK) ui-build
	$(TRUNK) serve --address 127.0.0.1 --port $(DEV_PORT) --no-autoreload

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

# Release build into its own dir, so it never races `make dev` (trunk serve writes dist/).
WEB_DIST := target/web-dist
build-web: $(TRUNK)
	$(TRUNK) build --release --dist $(WEB_DIST)

# PRD §6 P0 budget: wasm <= 15 MB gzip.
web-size:
	@rm -rf $(WEB_DIST)
	@$(MAKE) --no-print-directory build-web
	@wasm=$$(ls $(WEB_DIST)/*.wasm); raw=$$(wc -c < $$wasm | tr -d ' '); gz=$$(gzip -9 -c $$wasm | wc -c | tr -d ' '); \
	echo "wasm raw=$$raw gzip=$$gz"; \
	test $$gz -le 15000000 || { echo "FAIL: wasm gzip $$gz > 15 MB budget"; exit 1; }

# P4-T2: process assets-source/audio originals into audition/ (originals are never modified).
audio-process:
	python3 scripts/audio/process.py

# P4-T5: seamless loop derivatives of the Suno music into audition/music-loops (and the provenance record).
.PHONY: music-loops
music-loops:
	python3 scripts/audio/music_loops.py

# Audition page on the reserved parble port (Trunk dev server is not built yet).
audition:
	@test -f audition/sfx/manifest.json || { echo "ERROR: run make audio-process first"; exit 1; }
	@echo "audition page: http://127.0.0.1:$(AUDITION_PORT)/"
	python3 -m http.server $(AUDITION_PORT) --bind 127.0.0.1 --directory audition

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

# --- P1-T5 playroom assets (blender/playroom). Local only: needs Blender 5.2, ImageMagick, numpy and
# Pillow, and the pinned KTX-Software (fetch-tools). Not part of checkall; outputs are committed.
.PHONY: playroom-build
playroom-build: fetch-tools
	python3 blender/playroom/rug.py assets-source/playroom/rug
	blender/playroom/encode.sh
	$(BLENDER_RUN) -P blender/playroom/blocks.py -- assets/playroom
	$(BLENDER_RUN) -P blender/playroom/probe.py -- .
	python3 blender/playroom/provenance.py

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

# KIT_IDS limits kit-build to those pieces (re-baking an approved piece replaces its maps, so
# rebuild only what changed, e.g. make kit-build KIT_IDS="blk_drop_l3 blk_pillar").
KIT_IDS ?=
kit-build: $(KIT_STAMP)
	rm -rf target/kit-stage
	mkdir -p target/kit-stage
	$(KIT_RUN) -P blender/kit/generate.py -- target/kit-stage $(KIT_IDS)
	node $(KIT_TOOLS)/kit.mjs compress target/kit-stage assets/kit assets-source/kit $(KIT_IDS)
	$(KIT_RUN) -P blender/kit/reimport.py -- assets/kit $(KIT_IDS)
	node $(KIT_TOOLS)/kit.mjs provenance assets/kit assets-source/kit \
	  --checks "kit-check contract 14.1-14.10,gltf-validator reimport (meshopt GLB),blender reimport (meshopt GLB)" $(KIT_IDS)

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
