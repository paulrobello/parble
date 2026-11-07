// version-dist.mjs: deterministic content hashes, a changed file changes only its own version (and
// the versions of files that reference it), references are rewritten, and a rerun is a no-op.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { contentHash, versionDist } from "./version-dist.mjs";

const SITE = {
  "index.html":
    '<!doctype html><html><head><meta charset="utf-8" />\n    <link rel="icon" href="/web/brand/icon.png" />\n' +
    '    <script src="web/shim.js"></script>\n    <link rel="stylesheet" href="web/ui/hud.css" />\n' +
    '    <script>if (!navigator.gpu) location.replace("web/unsupported.html");</script>\n' +
    "<script type=\"module\">import init from '/parble-0123456789abcdef.js';</script></head></html>\n",
  "parble-0123456789abcdef.js": "export default function init() {}\n",
  "parble-0123456789abcdef_bg.wasm": "\0asm",
  CNAME: "example.net\n",
  "web/shim.js": "// shim\n",
  "web/ui/hud.css": '@font-face{src:url(fonts/f.woff2) format("woff2")}body{background:url(data:image/png;base64,AA)}',
  "web/ui/fonts/f.woff2": "font-v1",
  "web/unsupported.html": '<link rel="stylesheet" href="ui/hud.css" /><img src="unsupported.jpg" /><a href="../">Retry</a>',
  "web/unsupported.jpg": "jpg",
  "web/brand/icon.png": "png",
  "web/brand/site.webmanifest": '{"icons":[{"src":"/web/brand/icon.png"}]}',
  "assets/kit/p1/a/a.glb": "glb-a",
  "assets/kit/p1/b/b.glb": "glb-b",
  "assets/audio/music/m.webm": "webm",
};

function site() {
  const dir = mkdtempSync(join(tmpdir(), "version-dist-"));
  for (const [p, body] of Object.entries(SITE)) {
    mkdirSync(dirname(join(dir, p)), { recursive: true });
    writeFileSync(join(dir, p), body);
  }
  return dir;
}

const read = (dir, p) => readFileSync(join(dir, p), "utf8");
const pageMap = (dir) => JSON.parse(/window\.__PARBLE_HASHES=(\{.*?\});/.exec(read(dir, "index.html"))[1]);
const pageBuild = (dir) => /window\.__PARBLE_BUILD="([0-9a-f]+)";/.exec(read(dir, "index.html"))?.[1];
const fileBuild = (dir) => JSON.parse(read(dir, "version.json")).id;

test("CSP hashes cover final inline bytes including Trunk's loader and the generated map", () => {
  const dir = site();
  try {
    const policy = "default-src 'self'; script-src 'self' 'sha256-stale=' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; base-uri 'none'; form-action 'none'; object-src 'none'";
    writeFileSync(join(dir, "index.html"), read(dir, "index.html").replace("<head>",
      `<head><meta http-equiv="Content-Security-Policy" content="${policy}" />`));
    versionDist(dir);
    const page = read(dir, "index.html");
    const directive = /script-src ([^;"]+)/.exec(page)[1];
    const scripts = [...page.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/g)]
      .filter(([, attrs]) => !/\bsrc\s*=/.test(attrs));
    assert.equal(scripts.length, 3);
    assert.deepEqual([...directive.matchAll(/'sha256-[^']+'/g)].map(([hash]) => hash),
      scripts.map(([, , body]) => `'sha256-${createHash("sha256").update(body).digest("base64")}'`));
    assert.ok(directive.includes("'wasm-unsafe-eval'") && !directive.includes("'unsafe-inline'"));
    assert.ok(page.includes("style-src 'self' 'unsafe-inline'; base-uri 'none'; form-action 'none'; object-src 'none'"));
    assert.ok(page.indexOf('http-equiv="Content-Security-Policy"') < page.indexOf("<script"));
    versionDist(dir);
    assert.equal(read(dir, "index.html"), page, "CSP hashes do not make the build id recursive");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("every site file but the page, CNAME and Trunk's hashed outputs gets its content hash", () => {
  const dir = site();
  try {
    const map = versionDist(dir);
    assert.deepEqual(Object.keys(map), [
      "assets/audio/music/m.webm",
      "assets/kit/p1/a/a.glb",
      "assets/kit/p1/b/b.glb",
      "web/brand/icon.png",
      "web/brand/site.webmanifest",
      "web/shim.js",
      "web/ui/fonts/f.woff2",
      "web/ui/hud.css",
      "web/unsupported.html",
      "web/unsupported.jpg",
    ]);
    assert.equal(map["assets/kit/p1/a/a.glb"], contentHash(Buffer.from("glb-a")));
    assert.match(map["assets/kit/p1/a/a.glb"], /^[0-9a-f]{10}$/);
    assert.deepEqual(pageMap(dir), map);
    // References are versioned; Trunk's own hashed glue, external and data: URLs are not.
    const css = read(dir, "web/ui/hud.css");
    assert.ok(css.includes(`url(fonts/f.woff2?v=${map["web/ui/fonts/f.woff2"]})`), css);
    assert.ok(css.includes("url(data:image/png;base64,AA)"));
    assert.equal(map["web/ui/hud.css"], contentHash(Buffer.from(css)), "hud.css is hashed after its rewrite");
    const html = read(dir, "index.html");
    for (const ref of ["/web/brand/icon.png", "web/shim.js", "web/ui/hud.css", "web/unsupported.html"])
      assert.ok(html.includes(`${ref}?v=${map[ref.replace(/^\//, "")]}`), `${ref} in ${html}`);
    assert.ok(html.includes("'/parble-0123456789abcdef.js'"));
    assert.ok(html.indexOf('<script id="parble-hashes">') < html.indexOf('<script src="web/shim.js'), "map first");
    const page = read(dir, "web/unsupported.html");
    assert.ok(page.includes(`ui/hud.css?v=${map["web/ui/hud.css"]}`) && page.includes(`unsupported.jpg?v=`), page);
    assert.ok(page.includes('href="../"'));
    assert.ok(read(dir, "web/brand/site.webmanifest").includes(`/web/brand/icon.png?v=${map["web/brand/icon.png"]}`));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a rerun writes the same bytes, and an unchanged file keeps its version across builds", () => {
  const a = site();
  const b = site();
  try {
    const first = versionDist(a);
    const once = Object.fromEntries(Object.keys(SITE).map((p) => [p, read(a, p)]));
    assert.deepEqual(versionDist(a), first);
    for (const p of Object.keys(SITE)) assert.equal(read(a, p), once[p], `${p} changed on a rerun`);
    // A second, separate build of the same sources.
    assert.deepEqual(versionDist(b), first);
    assert.equal(read(b, "index.html"), once["index.html"]);
  } finally {
    rmSync(a, { recursive: true, force: true });
    rmSync(b, { recursive: true, force: true });
  }
});

test("changing one file's bytes changes only that file's version (and those that reference it)", () => {
  const dir = site();
  const next = site();
  try {
    const before = versionDist(dir);
    writeFileSync(join(next, "assets/kit/p1/a/a.glb"), "glb-a, rebuilt");
    const after = versionDist(next);
    const changed = Object.keys(before).filter((k) => before[k] !== after[k]);
    assert.deepEqual(changed, ["assets/kit/p1/a/a.glb"]);

    // A font is referenced by hud.css, which unsupported.html references in turn: all three move.
    const fonts = site();
    try {
      writeFileSync(join(fonts, "web/ui/fonts/f.woff2"), "font-v2");
      const moved = versionDist(fonts);
      assert.deepEqual(
        Object.keys(before).filter((k) => before[k] !== moved[k]),
        ["web/ui/fonts/f.woff2", "web/ui/hud.css", "web/unsupported.html"],
      );
    } finally {
      rmSync(fonts, { recursive: true, force: true });
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
    rmSync(next, { recursive: true, force: true });
  }
});

test("rerunning on a copy of a versioned site is a no-op", () => {
  const dir = site();
  const copy = mkdtempSync(join(tmpdir(), "version-dist-copy-"));
  try {
    const map = versionDist(dir);
    cpSync(dir, copy, { recursive: true });
    assert.deepEqual(versionDist(copy), map);
    assert.equal(read(copy, "index.html"), read(dir, "index.html"));
    assert.equal((read(copy, "index.html").match(/parble-hashes/g) ?? []).length, 1);
  } finally {
    rmSync(dir, { recursive: true, force: true });
    rmSync(copy, { recursive: true, force: true });
  }
});

test("the build id is inlined beside the map and written to version.json, which is never in the map", () => {
  const a = site();
  const b = site();
  try {
    const map = versionDist(a);
    const id = fileBuild(a);
    assert.match(id, /^[0-9a-f]{10}$/);
    assert.equal(pageBuild(a), id);
    assert.equal(read(a, "version.json"), `{"id":"${id}"}\n`);
    assert.ok(!("version.json" in map) && !("version.json" in pageMap(a)));
    // A rerun sees the version.json the first run wrote and writes the same bytes everywhere.
    const page = read(a, "index.html");
    assert.deepEqual(versionDist(a), map);
    assert.equal(read(a, "index.html"), page);
    assert.equal(fileBuild(a), id);
    assert.equal((read(a, "index.html").match(/__PARBLE_BUILD/g) ?? []).length, 1);
    // A separate build of the same sources gets the same id.
    versionDist(b);
    assert.equal(fileBuild(b), id);
  } finally {
    rmSync(a, { recursive: true, force: true });
    rmSync(b, { recursive: true, force: true });
  }
});

test("the build id changes when any site file, Trunk's output or the page changes", () => {
  const base = site();
  try {
    versionDist(base);
    const id = fileBuild(base);
    const edits = {
      asset: (d) => writeFileSync(join(d, "assets/kit/p1/b/b.glb"), "glb-b, rebuilt"),
      trunk: (d) => {
        rmSync(join(d, "parble-0123456789abcdef_bg.wasm"));
        writeFileSync(join(d, "parble-fedcba9876543210_bg.wasm"), "\0asm2");
      },
      page: (d) => writeFileSync(join(d, "index.html"), read(d, "index.html").replace("<head>", "<head><title>t</title>")),
    };
    for (const [what, edit] of Object.entries(edits)) {
      const d = site();
      try {
        edit(d);
        versionDist(d);
        assert.notEqual(fileBuild(d), id, `${what} edit kept the id`);
        assert.equal(pageBuild(d), fileBuild(d));
      } finally {
        rmSync(d, { recursive: true, force: true });
      }
    }
  } finally {
    rmSync(base, { recursive: true, force: true });
  }
});
