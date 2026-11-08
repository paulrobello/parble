// Provenance reconciliation for the marble textures (P1-T6 vanes here, P5-T1 patterns in p5-t1/), per
// the validate-generated-assets "Reconcile Provenance Records" procedure (the skill ships no script).
// Usage: node assets-source/marbles/check-provenance.mjs [p5-t1]   (exit 0 = pass, 1 = fail)
// The optional argument is a subdirectory with its own provenance.json, README.md and sources.
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const DIR = process.argv[2] ? join(HERE, process.argv[2]) : HERE;
const ROOT = resolve(HERE, "../..");
const sha256 = (p) => createHash("sha256").update(readFileSync(p)).digest("hex");
const errors = [];
const fail = (m) => errors.push(m);
const ok = (m) => console.log(`ok   ${m}`);

// PNG IHDR: width/height are big-endian u32 at bytes 16..24 after the 8-byte signature.
function pngSize(p) {
  const b = readFileSync(p);
  const sig = b.subarray(0, 8).toString("hex");
  if (sig !== "89504e470d0a1a0a" || b.subarray(12, 16).toString("ascii") !== "IHDR") return null;
  return { width: b.readUInt32BE(16), height: b.readUInt32BE(20) };
}

const prov = JSON.parse(readFileSync(join(DIR, "provenance.json"), "utf8"));
const TOP = ["schema", "purpose", "date", "route", "generationTotal", "generationCap", "creditsTotal", "frames", "rights", "gates", "rejectedJobs"];
for (const k of TOP) if (prov[k] === undefined) fail(`provenance missing top-level field ${k}`);
const FRAME = ["file", "subject", "targetsReference", "provider", "model", "jobId", "createdAt", "seed", "prompt", "referenceImages", "generations", "credits", "format", "width", "height", "bytes", "sha256"];

let gens = 0;
let credits = 0;
for (const f of prov.frames ?? []) {
  for (const k of FRAME) if (f[k] === undefined || f[k] === "") fail(`${f.file ?? "?"}: missing ${k}`);
  const p = join(DIR, f.file);
  if (!existsSync(p)) {
    fail(`${f.file}: not on disk`);
    continue;
  }
  const h = sha256(p);
  if (h !== f.sha256) fail(`${f.file}: sha256 ${h} != recorded ${f.sha256}`);
  const bytes = statSync(p).size;
  if (bytes !== f.bytes) fail(`${f.file}: ${bytes} bytes != recorded ${f.bytes}`);
  const dim = f.format === "png" ? pngSize(p) : null;
  if (!dim) fail(`${f.file}: not a decodable ${f.format} header`);
  else if (dim.width !== f.width || dim.height !== f.height)
    fail(`${f.file}: ${dim.width}x${dim.height} != recorded ${f.width}x${f.height}`);
  if (f.promptFile && readFileSync(join(DIR, f.promptFile), "utf8").trim() !== f.prompt) fail(`${f.file}: prompt differs from ${f.promptFile}`);
  for (const r of f.referenceImages) {
    const rp = join(ROOT, r.file);
    if (!existsSync(rp)) fail(`${f.file}: reference ${r.file} missing`);
    else if (sha256(rp) !== r.sha256) fail(`${f.file}: reference ${r.file} hash drifted`);
  }
  if (/\b(brio|haba|cuboro|hape|gravitrax|quercetti|lego)\b/i.test(f.prompt)) fail(`${f.file}: prompt names a toy brand`);
  gens += f.generations;
  credits += f.credits;
  ok(`${f.file} ${dim?.width}x${dim?.height} ${h.slice(0, 12)} job ${f.jobId}`);
}
gens += (prov.rejectedJobs ?? []).length;
if (gens !== prov.generationTotal) fail(`generation sum ${gens} != generationTotal ${prov.generationTotal}`);
if (prov.generationTotal > prov.generationCap) fail(`generationTotal ${prov.generationTotal} exceeds cap ${prov.generationCap}`);
if (credits !== prov.creditsTotal) fail(`credit sum ${credits} != creditsTotal ${prov.creditsTotal}`);
const n = prov.frames?.length ?? 0;
if (n < 1) fail("no kept frames");
for (const f of prov.frames ?? []) {
  const d = f.runtimeDerivative;
  if (!d) { fail(`${f.file}: no runtimeDerivative`); continue; }
  const rp = join(ROOT, d.file);
  if (!existsSync(rp)) { fail(`${d.file}: not on disk`); continue; }
  if (sha256(rp) !== d.sha256) fail(`${d.file}: sha256 drifted`);
  const dim = pngSize(rp);
  if (!dim || dim.width !== d.width || dim.height !== d.height) fail(`${d.file}: size != ${d.width}x${d.height}`);
  else ok(`${d.file} ${dim.width}x${dim.height} runtime derivative`);
}

const listed = new Set((prov.frames ?? []).map((f) => f.file));
for (const f of readdirSync(DIR).filter((x) => /\.(png|jpe?g|webp)$/i.test(x)))
  if (!listed.has(f)) fail(`${f}: image on disk not recorded in provenance`);

const readme = readFileSync(join(DIR, "README.md"), "utf8");
for (const f of listed) if (!readme.includes(f)) fail(`README.md does not list ${f}`);
for (const k of ["generatedOn", "terms", "references", "status"]) if (!prov.rights?.[k]) fail(`rights note missing ${k}`);

console.log(`frames=${n} generations=${prov.generationTotal}/${prov.generationCap} credits=${prov.creditsTotal} rights=${prov.rights?.status}`);
if (errors.length) {
  for (const e of errors) console.log(`FAIL ${e}`);
  process.exit(1);
}
console.log("PASS provenance reconciliation");
