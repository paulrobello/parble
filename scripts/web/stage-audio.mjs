// Stages the runtime SFX for the Web Audio engine (P4-T4, web/ui/src/audio). It reads Paul's approved
// picks (docs/audio/audition-picks.json) and the processed set (audition/sfx/manifest.json), copies the
// Ogg Opus and AAC files of every picked stem of a cue the engine plays into <out>/sfx/, checks each
// copy against the manifest's SHA-256, and writes <out>/sfx.json, the catalog the engine fetches.
// Only picked stems ship: the manifest also holds rejected takes. The rug cues are left out because
// no wire surface maps to them: the sim has no rug collider (the tray and the open floor are wood
// slabs and report `wood`), so the audio stream carries only kit surfaces and `marble`. The tunnel rumble
// ships as a loop layered over a roll while the marble is in a redirect tunnel (`TUNNEL_CUE`).
// Runs from `make ui-build` (Trunk's pre_build hook) after Vite, into web/ui/dist/audio.
// Usage: node scripts/web/stage-audio.mjs <out-dir>
import { createHash } from "node:crypto";
import { copyFileSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const SCHEMA = "parble.sfx-runtime/1";

/** The mechanism signal cues (web/ui/src/audio/mapping.ts `SIGNAL_CUE_IDS`). */
export const MECHANISM_CUES = [
  "checkpoint_tick",
  "drop_hole_glass",
  "drop_hole_solid",
  "drop_hole_steel",
  "finish_cup_chime",
  "hopper_release",
  "one_way_gate_swing",
  "splitter_tick",
  "toggle_flap_clack",
  "trapdoor_drop",
];

/** The redirect tunnel layer's loop (web/ui/src/audio/engine.ts `TUNNEL_CUE`). */
export const TUNNEL_CUE = "tunnel_rumble";

/** Cue ids the engine selects (web/ui/src/audio/mapping.ts, and `TUNNEL_CUE`). */
export function isRuntimeCue(id) {
  if (id.includes("_rug")) return false;
  return /^(roll|impact|clack)_/.test(id) || MECHANISM_CUES.includes(id) || id === TUNNEL_CUE;
}

/** The runtime catalog for `picks` over `manifest`: cue id -> { loop, variants }. Throws on a picked stem the manifest lacks. */
export function buildCatalog(picks, manifest) {
  const byStem = new Map(manifest.items.map((it) => [it.stem, it]));
  const cues = {};
  for (const id of Object.keys(picks).sort()) {
    const stems = picks[id];
    if (!Array.isArray(stems) || !isRuntimeCue(id)) continue;
    const variants = stems.map((stem) => {
      const it = byStem.get(stem);
      if (!it) throw new Error(`picked stem ${stem} (${id}) is not in the manifest`);
      if (it.cue_id !== id) throw new Error(`picked stem ${stem} belongs to ${it.cue_id}, not ${id}`);
      return {
        stem,
        duration: it.duration_s,
        ogg: `sfx/${it.files.ogg.path}`,
        m4a: `sfx/${it.files.m4a.path}`,
      };
    });
    const loops = new Set(stems.map((s) => byStem.get(s).loop));
    if (loops.size !== 1) throw new Error(`cue ${id} mixes loop and one-shot stems`);
    cues[id] = { loop: [...loops][0], variants };
  }
  return { schema: SCHEMA, cues };
}

/** The manifest's SHA-256 per runtime path (`sfx/<family>/<file>`). */
function expectedHashes(manifest) {
  const out = new Map();
  for (const it of manifest.items) {
    for (const f of Object.values(it.files)) out.set(`sfx/${f.path}`, f.sha256);
  }
  return out;
}

/** Copy the catalog's files from `sfxDir` into `out` and write `out`/sfx.json. Returns the file count. */
export function stage({ picks, manifest, sfxDir, out }) {
  const catalog = buildCatalog(picks, manifest);
  const hashes = expectedHashes(manifest);
  rmSync(out, { recursive: true, force: true });
  let files = 0;
  for (const cue of Object.values(catalog.cues)) {
    for (const v of cue.variants) {
      for (const rel of [v.ogg, v.m4a]) {
        const src = join(sfxDir, rel.slice("sfx/".length));
        const sha = createHash("sha256").update(readFileSync(src)).digest("hex");
        if (sha !== hashes.get(rel)) throw new Error(`${src}: sha256 ${sha} does not match the manifest`);
        const dst = join(out, rel);
        mkdirSync(dirname(dst), { recursive: true });
        copyFileSync(src, dst);
        files++;
      }
    }
  }
  writeFileSync(join(out, "sfx.json"), `${JSON.stringify(catalog)}\n`);
  return files;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const out = process.argv[2];
  if (!out) {
    console.error("usage: node scripts/web/stage-audio.mjs <out-dir>");
    process.exit(2);
  }
  const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
  const picks = JSON.parse(readFileSync(join(root, "docs/audio/audition-picks.json"), "utf8"));
  const manifest = JSON.parse(readFileSync(join(root, "audition/sfx/manifest.json"), "utf8"));
  const n = stage({ picks, manifest, sfxDir: join(root, "audition/sfx"), out });
  console.log(`stage-audio: ${n} files into ${out}`);
}
