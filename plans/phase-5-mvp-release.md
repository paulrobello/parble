# Phase 5: MVP polish and release

PRD: `PRD.md` §6 P5. Phase goal: a polished sandbox live at https://parble.pardev.net.

## Tasks

### P5-T1 — Full sandbox marble set
- Add swirl, onionskin, opal (`ref-04` blue) and glow (emissive) marbles, plus mini 10 mm and shooter 25 mm sizes.
- Physics table entries and materials, with Higgsfield pattern textures and provenance.
- Verify: sim tests per type for radius and mass sanity, and every type × size runs the P2-T7 track. A `shots-test` marble lineup shot is added.
- Depends on: P1-T6.

### P5-T2 — Key art
- Run `create-game-key-art` (Higgsfield) to produce the title-screen hero, loading art and a 1200×630 social card.
- Crop-safe responsive derivatives and provenance.
- Verify: `validate-generated-assets` passes and Paul approves.
- Depends on: P0-T9.

### P5-T3 — Title, settings, marble picker
- Title screen with key art and music.
- Settings: quality tier, audio mixer, camera options.
- First-run GPU quality detection (adapter info plus a short benchmark).
- Marble picker for the hopper queue.
- Polish the unsupported-browser page.
- Favicon via the `favicon-generator` skill. OG/meta tags.
- `impeccable` skill for the UI pass.
- Verify: Playwright UI smoke and Vitest pass.
- Depends on: P5-T1, P5-T2, P3-T5, P4-T5.

### P5-T4 — Release gate
- Full `make checkall-ci`, the local browser done-gate (Chrome + Safari smoke, `shots-test`, `perf-gate` under the load guard), and a `web-size` check.
- Re-check all P0–P4 acceptance boxes.
- Verify: evidence recorded in `docs/perf/p5-release.json` and the card notes.
- Depends on: P5-T3, P3-T6, P4-T6.

### P5-T5 — Go live (needs Paul's push approval)
1. Create the GitHub repo `paulrobello/parble` and push after Paul approves.
2. Watch CI to green (`gh run watch` with the full SHA).
3. Cloudflare DNS-only CNAME `parble.pardev.net` → `paulrobello.github.io` (token from parvault).
4. Only after DNS exists, commit `public/CNAME` (or the Trunk dist equivalent).
5. `PUT /repos/paulrobello/parble/pages` with the custom domain.
6. Wait out Let's Encrypt `authorization_pending`.

- Verify: a live render of https://parble.pardev.net in Chrome and Safari via Playwright/agentchrome, with canvas pixels non-black and no console errors. An HTTP 200 alone does not count.
- Depends on: P5-T4.

### P5-T6 — MVP sign-off
- Paul plays the live build. Findings are filed as cards.
- Verify: Paul's sign-off is recorded. README status updated.
- Depends on: P5-T5.

## Exit criteria
Every acceptance box under PRD §6 P5 checked with evidence.
