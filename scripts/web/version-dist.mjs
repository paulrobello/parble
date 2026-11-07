// Cache busting for the web build, ported from hex-world (scripts/web/version-dist.mjs). Every file
// in the built site gets a content hash (the first 10 hex of its SHA-256), and every URL the page fetches carries it as `?v=<hash>`, so browsers and CDNs can cache freely and
// a URL changes exactly when its file's bytes do. Files keep their names: Bevy asset paths, the kit
// manifest and `kit://` identities are unchanged, only the fetched URL is versioned.
//
// - Text files that reference other files (*.html, *.css, *.webmanifest) have each reference to a
//   site file rewritten to `<ref>?v=<hash>`, and are hashed after that, so a font change also changes
//   hud.css's version. Trunk's own hashed outputs (`<name>-<16 hex>.js`, `_bg.wasm`) are left alone.
// - The map (site path -> hash, every file but index.html, CNAME and Trunk's outputs) is inlined into
//   index.html as `window.__PARBLE_HASHES`, the first script in <head>, so it is as fresh as the HTML and
//   costs no request. Asset fetches from the wasm and the HUD (web/ui) are to version from it once
//   they exist. index.html itself is never versioned and must not be long-cached.
// - The build id hashes the map, Trunk's hashed names and index.html (with the map script left out),
//   so it changes exactly when the site's bytes do. It is inlined beside the map as
//   `window.__PARBLE_BUILD` and written to version.json (`{"id":"<id>"}`), for a HUD to poll and offer
//   a reload once a newer build is live. version.json is never versioned and never in the map.
//
// Idempotent: existing `?v=` suffixes and the map script are replaced, so a second run on the same
// files writes the same bytes. Runs as Trunk's post_build hook (Trunk.toml) on the staging dir, so
// every Trunk build (make build-web, make web-size and the CI deploy) produces a versioned site.
// Usage: node scripts/web/version-dist.mjs [dir]   (default: $TRUNK_STAGING_DIR, else dist)
import { createHash } from "node:crypto";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, posix } from "node:path";
import { fileURLToPath } from "node:url";
import { assertCsp } from "./csp-check.mjs";

export const HASH_LEN = 10;
/** Files never put in the map: the page and the build id (both fetched fresh) and the Pages domain file. */
const UNVERSIONED = new Set(["index.html", "CNAME", "version.json"]);
/** Trunk's filehash outputs at the site root, already content-addressed by name. */
export const TRUNK_HASHED = /^[^/]+-[0-9a-f]{16}(_bg\.wasm|\.js|\.css)$/;
/** Text files whose references to site files are versioned. */
const REFERRING = /\.(html|css|webmanifest)$/;
/** A quoted or url() reference, with any version it already carries. */
const REF = /(["'(])([^"'()\s?#<>]+)(\?v=[0-9a-f]*)?(?=["')])/g;
const MAP_SCRIPT = /<script id="parble-hashes">[^<]*<\/script>\n?\s*/;

export function contentHash(bytes) {
  return createHash("sha256").update(bytes).digest("hex").slice(0, HASH_LEN);
}

function listFiles(dir, rel = "") {
  const out = [];
  for (const e of readdirSync(join(dir, rel), { withFileTypes: true })) {
    const p = rel ? `${rel}/${e.name}` : e.name;
    if (e.isDirectory()) out.push(...listFiles(dir, p));
    else if (e.isFile()) out.push(p);
  }
  return out.sort();
}

/** The site file `ref` names from inside `from`, or null (external, data:, outside the site). */
function resolveRef(from, ref) {
  if (/^[a-z][a-z0-9+.-]*:/i.test(ref) || ref.startsWith("//")) return null;
  const p = ref.startsWith("/") ? ref.slice(1) : posix.normalize(posix.join(posix.dirname(from), ref));
  return p.startsWith("..") || p === "." ? null : p;
}

/** The build id: the map, Trunk's hashed names and the page without its map script. */
export function buildId(map, trunkFiles, html) {
  // Generated CSP hashes include the map's build id, so exclude them from its own input.
  const stableHtml = html.replace(/script-src\s+[^;"<>]+/g, (directive) =>
    directive.replace(/\s+'sha256-[A-Za-z0-9+/=]+'/g, ""),
  );
  return contentHash(Buffer.from(JSON.stringify([map, [...trunkFiles].sort(), stableHtml]), "utf8"));
}

/**
 * `page` with every CSP script-src directive listing exactly the sha256 of each inline script it holds.
 * Any page whose inline script bytes change after this (a rewrite, a swapped build id) must pass
 * through it again, or the browser blocks that script.
 */
export function hashInlineScripts(page) {
  const scriptHashes = [...page.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/gi)]
    .filter(([, attrs]) => !/\bsrc\s*=/i.test(attrs))
    .map(([, , body]) => `'sha256-${createHash("sha256").update(body).digest("base64")}'`);
  return page.replace(/script-src\s+[^;"<>]+/g, (directive) => {
    const sources = directive.replace(/\s+'(?:sha256-[A-Za-z0-9+/=]+|unsafe-inline)'/g, "");
    return `${sources} ${[...new Set(scriptHashes)].join(" ")}`;
  });
}

/** Versions `dir` in place and returns the map it inlined into index.html (sorted keys). */
export function versionDist(dir) {
  const files = listFiles(dir);
  const versioned = new Set(files.filter((f) => !UNVERSIONED.has(f) && !TRUNK_HASHED.test(f)));
  const hashes = new Map();
  const visiting = new Set();

  const rewrite = (file) => {
    const text = readFileSync(join(dir, file), "utf8");
    const out = text.replace(REF, (all, open, ref) => {
      const target = resolveRef(file, ref);
      return target && versioned.has(target) && target !== file ? `${open}${ref}?v=${hash(target)}` : all;
    });
    if (out !== text) writeFileSync(join(dir, file), out);
    return out;
  };

  function hash(file) {
    if (hashes.has(file)) return hashes.get(file);
    if (visiting.has(file)) throw new Error(`version-dist: reference cycle through ${file}`);
    visiting.add(file);
    const bytes = REFERRING.test(file) ? Buffer.from(rewrite(file), "utf8") : readFileSync(join(dir, file));
    visiting.delete(file);
    const h = contentHash(bytes);
    hashes.set(file, h);
    return h;
  }

  for (const f of versioned) hash(f);
  const map = Object.fromEntries([...hashes].sort(([a], [b]) => (a < b ? -1 : 1)));
  if (files.includes("index.html")) {
    const html = rewrite("index.html").replace(MAP_SCRIPT, "");
    const at = html.indexOf("<script");
    if (at < 0) throw new Error("version-dist: index.html has no <script> to put the hash map before");
    const id = buildId(map, files.filter((f) => TRUNK_HASHED.test(f)), html);
    const script =
      `<script id="parble-hashes">window.__PARBLE_HASHES=${JSON.stringify(map)};` +
      `window.__PARBLE_BUILD=${JSON.stringify(id)};</script>\n    `;
    const page = html.slice(0, at) + script + html.slice(at);
    // Trunk's loader, rewritten URLs and the generated map must be hashed after their final bytes exist.
    writeFileSync(join(dir, "index.html"), hashInlineScripts(page));
    writeFileSync(join(dir, "version.json"), `${JSON.stringify({ id })}\n`);
  }
  return map;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const dir = process.argv[2] ?? process.env.TRUNK_STAGING_DIR ?? "dist";
  const map = versionDist(dir);
  const page = join(dir, "index.html");
  assertCsp(readFileSync(page, "utf8"), page);
  console.log(`version-dist: ${Object.keys(map).length} files versioned in ${dir}, CSP covers every inline script`);
}
