// parble kit tool (P1-T2). Node, pinned deps in ./package.json.
//   node kit.mjs compress <stage_dir> <runtime_dir> <source_dir> [id ...]
//       stage GLB -> meshopt runtime GLB (FLOAT attributes kept), copies sidecar + baked maps to
//       <runtime_dir>/<id>/, .blend + reports to <source_dir>/<id>/.
//   node kit.mjs check <runtime_dir> <source_dir> [--no-provenance] [id ...]
//       kit-check (docs/kit-contract.md §14) plus the validator reimport gate on the shipped GLB.
//   node kit.mjs provenance <runtime_dir> <source_dir> --checks "a,b" [id ...]
//       runs check, then writes <runtime_dir>/<id>/<id>.provenance.json with validation.pass.
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { NodeIO } from "@gltf-transform/core";
import { EXTMeshoptCompression } from "@gltf-transform/extensions";
import { reorder } from "@gltf-transform/functions";
import { MeshoptDecoder, MeshoptEncoder } from "meshoptimizer";
import validator from "gltf-validator";

const REPO = path.resolve(path.dirname(new URL(import.meta.url).pathname), "../../..");
const CONTRACT_VERSION = 1;
const SCHEMA = 1;
const ID_RE = /^(blk|rail|mech|terr|music|deco)_[a-z0-9]+(_[a-z0-9]+)*$/;
const BUDGET_1CELL_BLOCK = [3000, 1000, 250];
const COLLISION_TRIS_PER_CELL = 600;
const CELL = 0.06, INSET = 0.0005;
const TOL = 0.0001; // ±0.1 mm (§3.1)

await MeshoptDecoder.ready;
await MeshoptEncoder.ready;
const io = () => new NodeIO().registerExtensions([EXTMeshoptCompression])
  .registerDependencies({ "meshopt.decoder": MeshoptDecoder, "meshopt.encoder": MeshoptEncoder });

const sha256 = (p) => createHash("sha256").update(fs.readFileSync(p)).digest("hex");
const rel = (p) => path.relative(REPO, path.resolve(p)).split(path.sep).join("/");

function roundHalfAway(x) { return Math.sign(x) * Math.floor(Math.abs(x) + 0.5); }
function canonicalRing(floorUm) {
  const pts = [[-14000, 60000]];
  for (let i = 0; i < 17; i++) {
    const th = (-90 + 11.25 * i) * Math.PI / 180;
    pts.push([roundHalfAway(14000 * Math.sin(th)), floorUm + roundHalfAway(14000 - 14000 * Math.cos(th))]);
  }
  pts.push([14000, 60000]);
  return pts;
}
function sortKeys(v) {
  if (Array.isArray(v)) return v.map(sortKeys);
  if (v && typeof v === "object") return Object.fromEntries(Object.keys(v).sort().map((k) => [k, sortKeys(v[k])]));
  return v;
}
function allIntegers(v, where, errs) {
  if (typeof v === "number") { if (!Number.isInteger(v)) errs.push(`non-integer number ${v} at ${where}`); return; }
  if (Array.isArray(v)) v.forEach((x, i) => allIntegers(x, `${where}[${i}]`, errs));
  else if (v && typeof v === "object") for (const [k, x] of Object.entries(v)) allIntegers(x, `${where}.${k}`, errs);
}

const FACE = { "+x": [0, 1], "-x": [0, -1], "+z": [2, 1], "-z": [2, -1] };

function pieces(dir, ids) {
  return ids.length ? ids : fs.readdirSync(dir).filter((d) => ID_RE.test(d)).sort();
}

// ------------------------------------------------------------------------------------ compress
async function compress(stage, runtime, source, ids) {
  for (const id of pieces(stage, ids)) {
    const s = path.join(stage, id), r = path.join(runtime, id), src = path.join(source, id);
    fs.rmSync(r, { recursive: true, force: true });
    fs.mkdirSync(r, { recursive: true });
    fs.mkdirSync(src, { recursive: true });
    const stageGlb = path.join(s, `${id}.glb`);
    const rep = await validator.validateBytes(new Uint8Array(fs.readFileSync(stageGlb)));
    if (rep.issues.numErrors) throw new Error(`${id}: stage GLB has ${rep.issues.numErrors} validator errors`);
    const doc = await io().read(stageGlb);
    await doc.transform(reorder({ encoder: MeshoptEncoder }));
    doc.createExtension(EXTMeshoptCompression).setRequired(true)
      .setEncoderOptions({ method: EXTMeshoptCompression.EncoderMethod.QUANTIZE });
    fs.writeFileSync(path.join(r, `${id}.glb`), await io().writeBinary(doc));
    for (const f of fs.readdirSync(s).sort()) {
      if (f.endsWith(".kit.json") || f.endsWith(".png")) fs.copyFileSync(path.join(s, f), path.join(r, f));
      else if (f.endsWith(".blend") || f.endsWith("-report.json")) fs.copyFileSync(path.join(s, f), path.join(src, f));
    }
    console.log(`compress ${id}: ${fs.statSync(stageGlb).size} -> ${fs.statSync(path.join(r, `${id}.glb`)).size} bytes`);
  }
}

// ------------------------------------------------------------------------------------ geometry
function meshData(doc) {
  // world-space positions + triangle indices per named mesh node
  const out = {};
  for (const node of doc.getRoot().listNodes()) {
    const mesh = node.getMesh();
    if (!mesh) continue;
    const m = node.getWorldMatrix();
    const pos = [], tris = [];
    for (const prim of mesh.listPrimitives()) {
      const a = prim.getAttribute("POSITION"), idx = prim.getIndices();
      const base = pos.length;
      for (let i = 0; i < a.getCount(); i++) {
        const [x, y, z] = a.getElement(i, []);
        pos.push([m[0] * x + m[4] * y + m[8] * z + m[12], m[1] * x + m[5] * y + m[9] * z + m[13],
          m[2] * x + m[6] * y + m[10] * z + m[14]]);
      }
      for (let i = 0; i < idx.getCount(); i += 3) tris.push([base + idx.getScalar(i), base + idx.getScalar(i + 1), base + idx.getScalar(i + 2)]);
    }
    out[node.getName()] = { pos, tris };
  }
  return out;
}

function cellOf(p, unit) {
  // p in contract frame (unit = 1 for µm-scaled input via CELL size). Returns cell offsets.
  return [Math.floor((p[0] + unit / 2) / unit), Math.floor(p[1] / unit), Math.floor((p[2] + unit / 2) / unit)];
}

// Overhead clearance (§5.1, §14.8): every up-facing collision hit needs >= 34 mm free above it.
function overhead(verts, tris, footprint, errs, id) {
  const T = tris.map(([a, b, c]) => {
    const A = verts[a], B = verts[b], C = verts[c];
    const ny = (B[2] - A[2]) * (C[0] - A[0]) - (B[0] - A[0]) * (C[2] - A[2]);
    return { A, B, C, ny };
  });
  let worst = Infinity;
  for (const [cx, cy, cz] of footprint) {
    for (let i = 0; i < 60; i++) for (let k = 0; k < 60; k++) {
      const x = cx * 60000 - 30000 + i * 1000 + 371, z = cz * 60000 - 30000 + k * 1000 + 613;
      const hits = [];
      for (const t of T) {
        if (t.ny === 0) continue;
        const { A, B, C } = t;
        const d = (B[2] - C[2]) * (A[0] - C[0]) + (C[0] - B[0]) * (A[2] - C[2]);
        const l1 = ((B[2] - C[2]) * (x - C[0]) + (C[0] - B[0]) * (z - C[2])) / d;
        const l2 = ((C[2] - A[2]) * (x - C[0]) + (A[0] - C[0]) * (z - C[2])) / d;
        const l3 = 1 - l1 - l2;
        if (l1 < 0 || l2 < 0 || l3 < 0) continue;
        hits.push({ y: l1 * A[1] + l2 * B[1] + l3 * C[1], up: t.ny > 0 });
      }
      hits.sort((a, b) => a.y - b.y);
      for (let h = 0; h + 1 < hits.length; h++) {
        if (hits[h].up && hits[h + 1].y - hits[h].y > 1) worst = Math.min(worst, hits[h + 1].y - hits[h].y);
      }
    }
  }
  if (worst === Infinity) worst = "open (no static geometry above any floor)";
  else if (worst < 34000) errs.push(`${id}: overhead clearance ${worst} µm < 34000 µm (§5.1)`);
  return worst;
}

// ------------------------------------------------------------------------------------ check
async function checkPiece(runtime, source, id, opts) {
  const errs = [], info = {};
  const dir = path.join(runtime, id);
  const scPath = path.join(dir, `${id}.kit.json`), glbPath = path.join(dir, `${id}.glb`);
  // 1. sidecar parses, versions, canonical bytes, integers only
  const raw = fs.readFileSync(scPath, "utf8");
  const sc = JSON.parse(raw);
  if (JSON.stringify(sortKeys(sc), null, 2) + "\n" !== raw) errs.push(`${id}: sidecar is not canonical (sorted keys, 2-space, LF, trailing newline)`);
  if (raw.includes("\r")) errs.push(`${id}: CR in sidecar`);
  allIntegers(sc, "sidecar", errs);
  if (sc.schema !== SCHEMA) errs.push(`${id}: schema ${sc.schema} != ${SCHEMA}`);
  if (sc.kit_contract !== CONTRACT_VERSION) errs.push(`${id}: kit_contract ${sc.kit_contract} != ${CONTRACT_VERSION}`);
  if (!Number.isInteger(sc.piece_version) || sc.piece_version < 1) errs.push(`${id}: bad piece_version`);
  if (raw.includes(REPO) || /"\/(Users|home)\//.test(raw)) errs.push(`${id}: absolute path in sidecar`);
  // 2. id grammar and level suffix
  if (sc.id !== id || !ID_RE.test(id) || id.length > 48) errs.push(`${id}: id fails §10 grammar or differs from sidecar id ${sc.id}`);
  const lm = id.match(/_l([0-3])$/);
  if (lm && sc.ports[0]?.level !== Number(lm[1])) errs.push(`${id}: level suffix l${lm[1]} != port 0 level ${sc.ports[0]?.level}`);
  sc.ports.forEach((p, i) => { if (p.index !== i) errs.push(`${id}: port ${i} has index ${p.index}`); });
  // collision mesh (integer µm)
  const fp = sc.footprint, fpKey = new Set(fp.map((c) => c.join(",")));
  const colVerts = [], colTris = [];
  for (const g of sc.collision) {
    const base = colVerts.length;
    for (let i = 0; i < g.vertices_um.length; i += 3) colVerts.push(g.vertices_um.slice(i, i + 3));
    for (let i = 0; i < g.indices.length; i += 3) colTris.push(g.indices.slice(i, i + 3).map((x) => x + base));
    if (g.indices.some((x) => x < 0 || x * 3 >= g.vertices_um.length)) errs.push(`${id}: collision index out of range`);
  }
  // 3. canonical ring at every side port, on an outer footprint face
  for (const p of sc.ports.filter((p) => p.kind === "side")) {
    const [ax, sgn] = FACE[p.face];
    const other = ax === 0 ? 2 : 0;
    const nb = [...p.cell]; nb[ax] += sgn;
    if (!fpKey.has(p.cell.join(","))) errs.push(`${id}: port ${p.index} cell not in footprint`);
    if (fpKey.has(nb.join(","))) errs.push(`${id}: port ${p.index} faces a footprint cell (§9 rule 5)`);
    const plane = p.cell[ax] * 60000 + sgn * 30000, uC = p.cell[other] * 60000, vB = p.cell[1] * 60000;
    const got = new Set(colVerts.filter((v) => v[ax] === plane && Math.abs(v[other] - uC) <= 14000 && v[1] >= vB && v[1] <= vB + 60000)
      .map((v) => `${v[other] - uC},${v[1] - vB}`));
    const want = new Set(canonicalRing(15000 * p.level).map(([u, v]) => `${u},${v}`));
    const missing = [...want].filter((k) => !got.has(k)), extra = [...got].filter((k) => !want.has(k));
    if (missing.length || extra.length) errs.push(`${id}: port ${p.index} ring != canonical (missing ${missing.slice(0, 3)}, extra ${extra.slice(0, 3)})`);
  }
  // 5. collision footprint
  for (const [a, b, c] of colTris) {
    // centroid nudged 1 µm against the face normal, so a face on a cell boundary counts for the
    // cell whose solid it bounds
    const A = colVerts[a], B = colVerts[b], C = colVerts[c];
    const e1 = [0, 1, 2].map((k) => B[k] - A[k]), e2 = [0, 1, 2].map((k) => C[k] - A[k]);
    const n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
    const len = Math.hypot(...n) || 1;
    const cen = [0, 1, 2].map((k) => (A[k] + B[k] + C[k]) / 3 - n[k] / len);
    if (!fpKey.has(cellOf(cen, 60000).join(","))) { errs.push(`${id}: collision triangle outside footprint`); break; }
  }
  // 6. collision budget
  if (colTris.length === 0) errs.push(`${id}: no collision mesh`);
  if (colTris.length > COLLISION_TRIS_PER_CELL * fp.length) errs.push(`${id}: collision ${colTris.length} tris > ${COLLISION_TRIS_PER_CELL * fp.length}`);
  info.colliderTris = colTris.length;
  // 8. overhead clearance
  info.overheadMinUm = overhead(colVerts, colTris, fp, errs, id);
  // 10. no emissive / inlay on non-mechanical pieces (sidecar side)
  if (sc.family !== "mech" && /inlay/i.test(raw)) errs.push(`${id}: inlay slot on a non-mechanical piece (§5.6)`);

  // GLB: validator reimport gate
  const bytes = fs.readFileSync(glbPath);
  const vrep = await validator.validateBytes(new Uint8Array(bytes));
  if (vrep.issues.numErrors) errs.push(`${id}: gltf-validator ${vrep.issues.numErrors} errors: ${vrep.issues.messages.filter((m) => m.severity === 0).slice(0, 3).map((m) => m.code + " " + m.pointer).join("; ")}`);
  info.validator = { errors: vrep.issues.numErrors, warnings: vrep.issues.numWarnings };
  const doc = await io().read(glbPath);
  const json = JSON.parse(Buffer.from(bytes.subarray(20, 20 + bytes.readUInt32LE(12))).toString());
  const ext = json.extensionsUsed ?? [];
  if (!ext.includes("EXT_meshopt_compression")) errs.push(`${id}: GLB is not meshopt-compressed`);
  if (ext.some((e) => e !== "EXT_meshopt_compression")) errs.push(`${id}: unexpected extensions ${ext}`);
  const scene = doc.getRoot().listScenes()[0];
  const roots = scene.listChildren();
  if (roots.length !== 1 || roots[0].getName() !== id) errs.push(`${id}: GLB must have one root node named ${id}, got ${roots.map((n) => n.getName())}`);
  const names = new Set(doc.getRoot().listNodes().map((n) => n.getName()));
  const lodNames = [0, 1, 2].map((l) => `${id}_lod${l}`);
  for (const n of [...lodNames, `${id}_collider`, ...sc.ports.map((p) => `port_${p.index}`)]) if (!names.has(n)) errs.push(`${id}: GLB node ${n} missing`);
  // 7. animated parts
  for (const a of sc.animated_parts) {
    if (!names.has(a.name)) errs.push(`${id}: animated part node ${a.name} missing`);
    if (!a.pivot_um || !a.axis) errs.push(`${id}: animated part ${a.name} lacks pivot or axis`);
  }
  // 10. emissive (GLB side)
  for (const m of json.materials ?? []) {
    if ((m.emissiveFactor ?? [0, 0, 0]).some((x) => x !== 0) || m.emissiveTexture || m.extensions?.KHR_materials_emissive_strength)
      if (sc.family !== "mech") errs.push(`${id}: emissive material ${m.name} on a non-mechanical piece (§5.6)`);
  }
  const md = meshData(doc);
  // 6. LODs present, within budget, decreasing; tri counts match the export report
  info.lodTris = lodNames.map((n) => md[n]?.tris.length ?? -1);
  const report = JSON.parse(fs.readFileSync(path.join(source, id, "export-report.json"), "utf8"));
  info.lodTris.forEach((t, l) => {
    if (t < 0) return;
    if (fp.length === 1 && sc.family === "blk" && t > BUDGET_1CELL_BLOCK[l]) errs.push(`${id}: LOD${l} ${t} tris > ${BUDGET_1CELL_BLOCK[l]}`);
    if (l > 0 && t >= info.lodTris[l - 1]) errs.push(`${id}: LOD${l} (${t}) not below LOD${l - 1}`);
    if (report.lodTris[lodNames[l]] !== t) errs.push(`${id}: LOD${l} ${t} tris != export report ${report.lodTris[lodNames[l]]}`);
  });
  const glbCol = md[`${id}_collider`];
  if (!glbCol || glbCol.tris.length !== colTris.length) errs.push(`${id}: GLB collider tris ${glbCol?.tris.length} != sidecar ${colTris.length}`);
  // 5. render footprint: every render vertex inside the inset cells of the footprint
  for (const n of lodNames) {
    for (const p of md[n]?.pos ?? []) {
      const c = cellOf(p, CELL);
      const lo = [c[0] * CELL - CELL / 2 + INSET, c[1] * CELL + INSET, c[2] * CELL - CELL / 2 + INSET];
      const inset = [0, 1, 2].every((k) => p[k] >= lo[k] - 1e-6 && p[k] <= lo[k] + CELL - 2 * INSET + 1e-6);
      if (!fpKey.has(c.join(",")) || !inset) { errs.push(`${id}: ${n} vertex ${p.map((x) => x.toFixed(5))} outside the inset footprint`); break; }
    }
  }
  // 4. render-mesh port position and width, every LOD
  info.ports = [];
  for (const p of sc.ports.filter((p) => p.kind === "side")) {
    const [ax, sgn] = FACE[p.face];
    const other = ax === 0 ? 2 : 0;
    const plane = p.cell[ax] * CELL + sgn * (CELL / 2 - INSET), uC = p.cell[other] * CELL;
    const floor = p.cell[1] * CELL + 0.015 * p.level, cy = floor + 0.014;
    for (const n of lodNames) {
      const vs = (md[n]?.pos ?? []).filter((v) => Math.abs(v[ax] - plane) < 1e-6 && Math.abs(v[other] - uC) < 0.0145 && v[1] < cy + 1e-6);
      if (!vs.length) { errs.push(`${id}: ${n} port ${p.index} has no render channel vertices on the face`); continue; }
      const low = vs.reduce((a, b) => (b[1] < a[1] ? b : a));
      const fErr = low[1] - floor, uErr = low[other] - uC;
      const lowerHalf = vs.filter((v) => v[1] <= floor + 0.007 + 1e-7);
      const r = lowerHalf.map((v) => Math.hypot(v[other] - uC, v[1] - cy));
      const width = 2 * r.reduce((a, b) => a + b, 0) / r.length;
      const rDev = Math.max(...r.map((x) => Math.abs(x - 0.014)));
      if (Math.abs(fErr) > TOL || Math.abs(uErr) > TOL) errs.push(`${id}: ${n} port ${p.index} floor/centre off by ${(fErr * 1000).toFixed(3)}/${(uErr * 1000).toFixed(3)} mm`);
      if (Math.abs(width - 0.028) > TOL || rDev > TOL) errs.push(`${id}: ${n} port ${p.index} width ${(width * 1000).toFixed(3)} mm (max radius dev ${(rDev * 1000).toFixed(3)})`);
      info.ports.push({ lod: n.slice(-4), port: p.index, floorErrMm: +(fErr * 1000).toFixed(4), centreErrMm: +(uErr * 1000).toFixed(4), widthMm: +(width * 1000).toFixed(4) });
    }
  }
  // sockets: world position at the port floor point on the cell face; extras round-trip
  for (const p of sc.ports) {
    const node = doc.getRoot().listNodes().find((n) => n.getName() === `port_${p.index}`);
    if (!node) continue;
    const t = node.getWorldTranslation();
    const want = [p.cell[0] * CELL, p.cell[1] * CELL + 0.015 * (p.level ?? 0), p.cell[2] * CELL];
    if (p.kind === "side") { const [ax, sgn] = FACE[p.face]; want[ax] += sgn * CELL / 2; }
    if (t.some((x, k) => Math.abs(x - want[k]) > 1e-6)) errs.push(`${id}: socket port_${p.index} at ${t} != ${want}`);
    const ex = node.getExtras();
    for (const k of ["index", "face", "level", "class", "flow", "kind"]) if (p[k] !== undefined && ex[k] !== p[k]) errs.push(`${id}: socket port_${p.index} extras.${k} ${ex[k]} != ${p[k]}`);
  }
  // 9. provenance
  if (opts.provenance) {
    const pp = path.join(dir, `${id}.provenance.json`);
    if (!fs.existsSync(pp)) errs.push(`${id}: provenance missing`);
    else {
      const pv = JSON.parse(fs.readFileSync(pp, "utf8"));
      if (pv.validation?.pass !== true) errs.push(`${id}: provenance validation.pass != true`);
      for (const f of pv.derivedFiles ?? []) if (!fs.existsSync(path.join(REPO, f.path)) || sha256(path.join(REPO, f.path)) !== f.sha256) errs.push(`${id}: provenance hash stale for ${f.path}`);
      for (const g of [pv.generator, ...(pv.generatorDeps ?? [])]) if (g && sha256(path.join(REPO, g.path)) !== g.sha256) console.log(`warning: ${id}: ${g.path} changed since provenance`);
    }
  }
  info.glbBytes = bytes.length;
  return { errs, info };
}

async function check(runtime, source, ids, opts) {
  let bad = 0;
  const results = {};
  for (const id of pieces(runtime, ids)) {
    const { errs, info } = await checkPiece(runtime, source, id, opts);
    results[id] = info;
    const ports = info.ports.map((p) => `${p.lod}/p${p.port} floor ${p.floorErrMm} centre ${p.centreErrMm} width ${p.widthMm}`).join("; ");
    console.log(`${errs.length ? "FAIL" : "ok  "} ${id}: LOD tris ${info.lodTris.join("/")}, collider ${info.colliderTris}, overhead min ${info.overheadMinUm} µm, glb ${info.glbBytes} B, validator ${info.validator.errors}E/${info.validator.warnings}W`);
    console.log(`     render ports (mm): ${ports}`);
    for (const e of errs) console.log(`  - ${e}`);
    bad += errs.length;
  }
  return { bad, results };
}

// ------------------------------------------------------------------------------------ provenance
async function provenance(runtime, source, ids, checks) {
  const { bad, results } = await check(runtime, source, ids, { provenance: false });
  if (bad) throw new Error(`kit-check failed (${bad}); provenance not written`);
  const pkg = JSON.parse(fs.readFileSync(path.join(path.dirname(new URL(import.meta.url).pathname), "package.json"), "utf8")).dependencies;
  for (const id of pieces(runtime, ids)) {
    const dir = path.join(runtime, id), src = path.join(source, id);
    const exp = JSON.parse(fs.readFileSync(path.join(src, "export-report.json"), "utf8"));
    const bakeP = path.join(src, "bake-report.json");
    const bake = fs.existsSync(bakeP) ? JSON.parse(fs.readFileSync(bakeP, "utf8")) : null;
    const files = (d, pred) => fs.readdirSync(d).filter(pred).sort().map((f) => path.join(d, f));
    const pv = {
      schema: "parble-kit-provenance/1",
      pieceId: id,
      pieceVersion: JSON.parse(fs.readFileSync(path.join(dir, `${id}.kit.json`), "utf8")).piece_version,
      route: "scripted",
      card: "P1-T2",
      contract: { path: "docs/kit-contract.md", version: CONTRACT_VERSION, sha256: sha256(path.join(REPO, "docs/kit-contract.md")), approved: "Paul, 2026-11-07 (contract status line)" },
      generator: exp.generator,
      generatorDeps: exp.generatorDeps,
      authoringAgent: { harness: "claude-code", model: "claude-opus-5-5" },
      blender: exp.blender,
      toolVersions: { gltfTransform: pkg["@gltf-transform/core"], meshoptimizer: pkg.meshoptimizer, gltfValidator: pkg["gltf-validator"], node: process.version },
      material: "placeholder (wood_placeholder, no maps, no emission); final tileable wood comes from P1-T1",
      bake: bake && { device: bake.device, samples: bake.samples, cageExtrusionM: bake.cageExtrusionM, maxRayDistanceM: bake.maxRayDistanceM, marginPx: bake.marginPx, aoDistanceM: bake.aoDistanceM, channels: bake.channels },
      lods: Object.entries(exp.lodTris).map(([n, t]) => ({ lod: n, tris: t })),
      collider: { node: `${id}_collider`, tris: results[id].colliderTris },
      sockets: "port_<index> nodes at the port floor point on the cell face; extras carry index, face, level, class, flow, kind",
      texelDensityPxPerM: exp.texelDensityPxPerM,
      compression: { meshopt: "EXT_meshopt_compression, QUANTIZE method, FLOAT attributes (no KHR_mesh_quantization)", glbBytes: results[id].glbBytes, ktx2: "not produced in P1-T2 (maps ship as PNG)" },
      derivedFiles: files(dir, (f) => f.endsWith(".glb") || f.endsWith(".png") || f.endsWith(".kit.json"))
        .concat(files(src, (f) => f.endsWith(".blend")))
        .map((p) => ({ path: rel(p), sha256: sha256(p), bytes: fs.statSync(p).size })),
      references: ["docs/reference/ref-02.webp", "docs/reference/ref-01.webp", "docs/reference/ref-06-rail-into-block-tower.png"]
        .map((p) => ({ path: p, sha256: sha256(path.join(REPO, p)), role: "look reference (owner-supplied)" })),
      license: "MIT (repository licence); geometry is generated by the repository's own generator",
      validation: { pass: true, checks },
      approvals: [{ who: "Paul", gate: "piece look", status: "approved", notes: "Paul approved the four-piece look 2026-11-07 (blender/kit/review/pieces.png; also seen textured in the kit2000 and playroom scenes)" }],
    };
    fs.writeFileSync(path.join(dir, `${id}.provenance.json`), JSON.stringify(pv, null, 2) + "\n");
    console.log(`provenance ${id}`);
  }
}

// ------------------------------------------------------------------------------------ main
const [cmd, ...rest] = process.argv.slice(2);
const flags = new Set(rest.filter((a) => a.startsWith("--no-")));
const ci = rest.indexOf("--checks");
const checksArg = ci >= 0 ? rest.splice(ci, 2)[1] : "";
const pos = rest.filter((a) => !a.startsWith("--no-"));
if (cmd === "compress") await compress(pos[0], pos[1], pos[2], pos.slice(3));
else if (cmd === "check") {
  const { bad } = await check(pos[0], pos[1], pos.slice(2), { provenance: !flags.has("--no-provenance") });
  if (bad) { console.log(`kit-check: ${bad} failure(s)`); process.exit(1); }
  console.log("kit-check: all pieces pass");
} else if (cmd === "provenance") await provenance(pos[0], pos[1], pos.slice(2), checksArg.split(",").filter(Boolean));
else { console.error("usage: kit.mjs compress|check|provenance ..."); process.exit(2); }
