// Provenance reconciliation for the P5-T2 key art, adapted from
// assets-source/lookdev/check-provenance.mjs per the validate-generated-assets
// "Reconcile Provenance Records" procedure. Covers the immutable sources in this
// directory and the runtime derivatives in assets/keyart/.
// Usage: node assets-source/keyart/check-provenance.mjs   (exit 0 = pass, 1 = fail)
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const DIR = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(DIR, "../..");
const OUT = join(ROOT, "assets/keyart");
const sha256 = (p) => createHash("sha256").update(readFileSync(p)).digest("hex");
const errors = [];
const fail = (m) => errors.push(m);
const ok = (m) => console.log(`ok   ${m}`);

// PNG IHDR: width/height are big-endian u32 at bytes 16..24.
function pngSize(b) {
  if (b.subarray(0, 8).toString("hex") !== "89504e470d0a1a0a" || b.subarray(12, 16).toString("ascii") !== "IHDR") return null;
  return { width: b.readUInt32BE(16), height: b.readUInt32BE(20) };
}

// JPEG: walk markers to the first SOFn frame header.
function jpgSize(b) {
  if (b[0] !== 0xff || b[1] !== 0xd8) return null;
  let i = 2;
  while (i + 9 < b.length) {
    if (b[i] !== 0xff) return null;
    const m = b[i + 1];
    const len = b.readUInt16BE(i + 2);
    if (m >= 0xc0 && m <= 0xcf && m !== 0xc4 && m !== 0xc8 && m !== 0xcc)
      return { width: b.readUInt16BE(i + 7), height: b.readUInt16BE(i + 5) };
    i += 2 + len;
  }
  return null;
}

// WebP: RIFF container with a VP8, VP8L or VP8X first chunk.
function webpSize(b) {
  if (b.subarray(0, 4).toString("ascii") !== "RIFF" || b.subarray(8, 12).toString("ascii") !== "WEBP") return null;
  const kind = b.subarray(12, 16).toString("ascii");
  if (kind === "VP8 ") return { width: b.readUInt16LE(26) & 0x3fff, height: b.readUInt16LE(28) & 0x3fff };
  if (kind === "VP8L") {
    const v = b.readUInt32LE(21);
    return { width: (v & 0x3fff) + 1, height: ((v >> 14) & 0x3fff) + 1 };
  }
  if (kind === "VP8X") return { width: b.readUIntLE(24, 3) + 1, height: b.readUIntLE(27, 3) + 1 };
  return null;
}

// AVIF: ISO-BMFF with an "avif" brand; the primary item's size is in the ispe property.
function avifSize(b) {
  if (b.subarray(4, 8).toString("ascii") !== "ftyp" || !b.subarray(8, 32).toString("ascii").includes("avif")) return null;
  const at = b.indexOf(Buffer.from("ispe"));
  if (at < 0) return null;
  return { width: b.readUInt32BE(at + 8), height: b.readUInt32BE(at + 12) };
}

const SIZE = { png: pngSize, jpg: jpgSize, webp: webpSize, avif: avifSize };
const sizeOf = (p, fmt) => (SIZE[fmt] ? SIZE[fmt](readFileSync(p)) : null);

const prov = JSON.parse(readFileSync(join(DIR, "provenance.json"), "utf8"));
const TOP = ["schema", "purpose", "date", "route", "generationTotal", "generationCap", "creditsTotal", "frames", "derivatives", "rights", "gates", "rejectedJobs"];
for (const k of TOP) if (prov[k] === undefined) fail(`provenance missing top-level field ${k}`);
const FRAME = ["file", "subject", "role", "targetsReference", "provider", "model", "jobId", "createdAt", "seed", "prompt", "referenceImages", "generations", "credits", "format", "width", "height", "bytes", "sha256"];
const BANNED_REFS = /(05-glow-inlay|08-glow-inlay|09-glow-inlay|ref-09)/;

let gens = 0;
let credits = 0;
const frameByHash = new Map();
for (const f of prov.frames ?? []) {
  for (const k of FRAME) if (f[k] === undefined || f[k] === "") fail(`${f.file ?? "?"}: missing ${k}`);
  const p = join(DIR, f.file);
  if (!existsSync(p)) {
    fail(`${f.file}: not on disk`);
    continue;
  }
  const h = sha256(p);
  if (h !== f.sha256) fail(`${f.file}: sha256 ${h} != recorded ${f.sha256}`);
  if (statSync(p).size !== f.bytes) fail(`${f.file}: ${statSync(p).size} bytes != recorded ${f.bytes}`);
  const dim = sizeOf(p, f.format);
  if (!dim) fail(`${f.file}: not a decodable ${f.format} header`);
  else if (dim.width !== f.width || dim.height !== f.height) fail(`${f.file}: ${dim.width}x${dim.height} != recorded ${f.width}x${f.height}`);
  if (f.model !== "nano_banana_pro") fail(`${f.file}: receipt model ${f.model} is not the requested nano_banana_pro`);
  if (f.promptFile && readFileSync(join(DIR, f.promptFile), "utf8").trim() !== f.prompt) fail(`${f.file}: prompt differs from ${f.promptFile}`);
  if (f.receipt) {
    const r = JSON.parse(readFileSync(join(DIR, f.receipt), "utf8"))[0];
    if (r.id !== f.jobId || r.job_type !== f.model || r.params?.prompt !== f.prompt) fail(`${f.file}: receipt ${f.receipt} does not match the record`);
  }
  for (const r of f.referenceImages) {
    const rp = join(ROOT, r.file);
    if (BANNED_REFS.test(r.file)) fail(`${f.file}: uses excluded glow-inlay reference ${r.file}`);
    if (!existsSync(rp)) fail(`${f.file}: reference ${r.file} missing`);
    else if (sha256(rp) !== r.sha256) fail(`${f.file}: reference ${r.file} hash drifted`);
  }
  if (/\b(brio|haba|cuboro|hape|gravitrax|quercetti|lego)\b/i.test(f.prompt)) fail(`${f.file}: prompt names a toy brand`);
  gens += f.generations;
  credits += f.credits;
  frameByHash.set(f.sha256, f);
  ok(`source ${f.file} ${dim?.width}x${dim?.height} ${h.slice(0, 12)} job ${f.jobId}`);
}
gens += (prov.rejectedJobs ?? []).length;
if (gens !== prov.generationTotal) fail(`generation sum ${gens} != generationTotal ${prov.generationTotal}`);
if (prov.generationTotal > prov.generationCap) fail(`generationTotal ${prov.generationTotal} exceeds cap ${prov.generationCap}`);
if (credits !== prov.creditsTotal) fail(`credit sum ${credits} != creditsTotal ${prov.creditsTotal}`);
const n = prov.frames?.length ?? 0;
if (n < 3 || n > 12) fail(`${n} kept sources, expected 3-12`);

const DERIV = ["file", "surface", "source", "sourceSha256", "crop", "format", "width", "height", "bytes", "sha256", "tool", "safeZones", "zoneMeanLuma"];
const surfaces = new Set();
for (const d of prov.derivatives ?? []) {
  for (const k of DERIV) if (d[k] === undefined || d[k] === "") fail(`${d.file ?? "?"}: missing ${k}`);
  const p = join(ROOT, d.file);
  if (!existsSync(p)) {
    fail(`${d.file}: not on disk`);
    continue;
  }
  const src = frameByHash.get(d.sourceSha256);
  if (!src) fail(`${d.file}: sourceSha256 matches no recorded source`);
  else {
    if (`assets-source/keyart/${src.file}` !== d.source) fail(`${d.file}: source ${d.source} != hash owner ${src.file}`);
    const c = d.crop;
    if (c.x < 0 || c.y < 0 || c.x + c.width > src.width || c.y + c.height > src.height) fail(`${d.file}: crop exceeds source bounds`);
  }
  if (d.width > d.crop.width || d.height > d.crop.height) fail(`${d.file}: ${d.width}x${d.height} upscales a ${d.crop.width}x${d.crop.height} crop`);
  const ar = d.width / d.height;
  const car = d.crop.width / d.crop.height;
  if (Math.abs(ar - car) / car > 0.01) fail(`${d.file}: aspect ${ar.toFixed(4)} distorts crop aspect ${car.toFixed(4)}`);
  if (sha256(p) !== d.sha256) fail(`${d.file}: sha256 drifted`);
  if (statSync(p).size !== d.bytes) fail(`${d.file}: bytes drifted`);
  if (d.bytes > 1000 * 1024) fail(`${d.file}: ${d.bytes} bytes exceeds the repo's 1000 KB large-file limit`);
  const dim = sizeOf(p, d.format);
  if (!dim) fail(`${d.file}: not a decodable ${d.format} header`);
  else if (dim.width !== d.width || dim.height !== d.height) fail(`${d.file}: header ${dim.width}x${dim.height} != recorded ${d.width}x${d.height}`);
  if (d.surface === "social-card" && (d.width !== 1200 || d.height !== 630)) fail(`${d.file}: social card must be exactly 1200x630`);
  for (const [k, z] of Object.entries(d.safeZones ?? {})) {
    if (!(z[0] >= 0 && z[1] >= 0 && z[2] <= 1 && z[3] <= 1 && z[0] < z[2] && z[1] < z[3])) fail(`${d.file}: bad ${k} zone`);
  }
  if (!(d.zoneMeanLuma?.title <= 0.2)) fail(`${d.file}: title zone mean luma ${d.zoneMeanLuma?.title} > 0.2, a light wordmark would not read`);
  surfaces.add(`${d.surface}:${d.format}`);
  ok(`deriv  ${d.file.replace("assets/keyart/", "")} ${dim?.width}x${dim?.height} ${d.bytes}B title-luma ${d.zoneMeanLuma?.title}`);
}
for (const s of ["hero-21x9", "hero-16x9", "hero-portrait-9x16", "loading-16x9"])
  for (const fmt of ["avif", "webp", "png"]) if (!surfaces.has(`${s}:${fmt}`)) fail(`no ${fmt} derivative for ${s}`);
if (!surfaces.has("social-card:png")) fail("no PNG social card");

const listedSrc = new Set((prov.frames ?? []).map((f) => f.file));
for (const f of readdirSync(DIR).filter((x) => /\.(png|jpe?g|webp|avif)$/i.test(x)))
  if (!listedSrc.has(f)) fail(`${f}: image in assets-source/keyart not recorded in provenance`);
const listedOut = new Set((prov.derivatives ?? []).map((d) => d.file));
for (const f of existsSync(OUT) ? readdirSync(OUT) : [])
  if (!listedOut.has(`assets/keyart/${f}`)) fail(`assets/keyart/${f}: file not recorded in provenance`);

const readme = readFileSync(join(DIR, "README.md"), "utf8");
for (const f of listedSrc) if (!readme.includes(f)) fail(`README.md does not list ${f}`);
for (const k of ["generatedOn", "terms", "references", "status"]) if (!prov.rights?.[k]) fail(`rights note missing ${k}`);

console.log(`sources=${n} derivatives=${prov.derivatives?.length ?? 0} generations=${prov.generationTotal}/${prov.generationCap} credits=${prov.creditsTotal} rights=${prov.rights?.status}`);
if (errors.length) {
  for (const e of errors) console.log(`FAIL ${e}`);
  process.exit(1);
}
console.log("PASS provenance reconciliation");
