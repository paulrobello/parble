"""Headless kit generator (P1-T2). Run:

    Blender -b --factory-startup --python-exit-code 1 -P blender/kit/generate.py -- <stage_dir> [piece_id ...]

Per piece, into <stage_dir>/<id>/: <id>.glb (image-free, uncompressed; tools/kit.mjs compresses it),
<id>.kit.json (§11 sidecar), <id>_lod{0,1,2}_{normal,ao}.png (Cycles bakes), <id>_high.blend,
<id>_low.blend, bake-report.json, export-report.json.

Env: PARBLE_SKIP_BAKE=1 skips bakes and .blend saves (kit-determinism); PARBLE_BAKE_DEVICE=CPU|GPU
(default GPU, Metal on macOS).
"""
import hashlib
import json
import math
import os
import pathlib
import statistics
import sys

import bpy

HERE = pathlib.Path(__file__).resolve().parent
REPO = HERE.parents[1]
sys.path.insert(0, str(HERE))
import kitgeom as K  # noqa: E402
import kitblocks as B  # noqa: E402
import kitmech as M  # noqa: E402
import kitstepper as S  # noqa: E402
import kitrails as R  # noqa: E402

SKIP_BAKE = os.environ.get("PARBLE_SKIP_BAKE") == "1"
DEVICE = os.environ.get("PARBLE_BAKE_DEVICE", "GPU")
MAP_SIZE = {0: 512, 1: 256, 2: 128}  # §12: 2,048 texels/m -> 512² for a 1-cell block at LOD0
BAKE = {"samples": 16, "cageExtrusionM": 0.0005, "maxRayDistanceM": 0.002, "marginPx": 8, "aoDistanceM": 0.02}


def sha256(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.context.preferences.filepaths.save_version = 0
    s = bpy.context.scene
    s.unit_settings.system = "METRIC"
    s.unit_settings.scale_length = 1.0
    return s


def select_only(objs, active=None):
    for o in bpy.context.view_layer.objects:
        o.select_set(False)
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = active or objs[0]


def tri_count(ob):
    return sum(len(p.vertices) - 2 for p in ob.data.polygons)


def unwrap(ob):
    ob.data.uv_layers.new(name="UVMap")
    select_only([ob])
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.uv.smart_project(angle_limit=math.radians(66.0), island_margin=0.01, area_weight=0.0,
                             correct_aspect=True, scale_to_bounds=False)
    bpy.ops.object.mode_set(mode="OBJECT")


def texel_density(ob, px):
    uvl = ob.data.uv_layers["UVMap"]
    vals = []
    for p in ob.data.polygons:
        if p.area <= 1e-12:
            continue
        uv = [uvl.data[i].uv for i in p.loop_indices]
        a = sum(abs((uv[i] - uv[0]).cross(uv[i + 1] - uv[0])) / 2 for i in range(1, len(uv) - 1))
        if a > 0:
            vals.append((a * px * px / p.area) ** 0.5)
    return {"min": round(min(vals), 1), "median": round(statistics.median(vals), 1), "max": round(max(vals), 1)}


def family(pid):
    """The generator module that owns a piece id (kitgeom pieces go through kitblocks' helpers)."""
    return R if R.handles(pid) else M if M.handles(pid) else B


def check_render_bounds(ob, pid):
    lo, hi = family(pid).bounds_mm(pid)  # contract frame mm; Blender (x, y, z) = contract (x, -z, y)
    lo = [(c + K.INSET) * 0.001 - 1e-6 for c in lo]
    hi = [(c - K.INSET) * 0.001 + 1e-6 for c in hi]
    for v in ob.data.vertices:
        x, y, z = v.co
        p = (x, z, -y)
        if any(p[k] < lo[k] or p[k] > hi[k] for k in range(3)):
            raise RuntimeError(f"{ob.name}: vertex {tuple(v.co)} leaves the inset footprint")


def map_size(pid, lod):
    """§12: 512² per 1-cell piece at LOD0; a multi-cell piece's atlas scales with its footprint
    (next power of two of 512 × sqrt(cells)), capped at 2,048²."""
    n = family(pid).cells(pid)
    s = MAP_SIZE[lod]
    while s * s < MAP_SIZE[lod] ** 2 * n:
        s *= 2
    return min(s, 2048)


def setup_cycles():
    s = bpy.context.scene
    s.render.engine = "CYCLES"
    s.cycles.samples = BAKE["samples"]
    s.cycles.seed = 0
    if DEVICE != "CPU":
        prefs = bpy.context.preferences.addons["cycles"].preferences
        prefs.compute_device_type = "METAL"
        prefs.get_devices()
        for d in prefs.devices:
            d.use = d.type == "METAL"
        if not any(d.use for d in prefs.devices):
            raise RuntimeError("PARBLE_BAKE_DEVICE=GPU but no Metal device; set PARBLE_BAKE_DEVICE=CPU explicitly")
        s.cycles.device = "GPU"
    else:
        s.cycles.device = "CPU"
    b = s.render.bake
    b.use_pass_direct = False
    b.use_pass_indirect = False
    b.normal_space = "TANGENT"
    if s.world is None:
        s.world = bpy.data.worlds.new("bake_world")
    s.world.light_settings.distance = BAKE["aoDistanceM"]


def bake(low, kind, image, highs=()):
    mat = low.data.materials[0]
    nt = mat.node_tree
    node = nt.nodes.get("BAKE_TARGET") or nt.nodes.new("ShaderNodeTexImage")
    node.name = "BAKE_TARGET"
    node.image = image
    nt.nodes.active = node
    b = bpy.context.scene.render.bake
    b.use_selected_to_active = bool(highs)
    b.cage_extrusion = BAKE["cageExtrusionM"]
    b.max_ray_distance = BAKE["maxRayDistanceM"]
    b.margin = BAKE["marginPx"]
    b.use_clear = True
    keep = {low.name, *(h.name for h in highs)}
    hidden = [o for o in bpy.context.scene.objects if o.type == "MESH" and o.name not in keep and not o.hide_render]
    for o in hidden:
        o.hide_render = True
    try:
        select_only(list(highs) + [low], low)
        bpy.ops.object.bake(type=kind)
    finally:
        for o in hidden:
            o.hide_render = False


def tree(root):
    out = [root]
    for c in sorted(root.children, key=lambda o: o.name):
        out += tree(c)
    return out


def build_piece(pid, out):
    reset()
    coll = bpy.context.scene.collection
    mat = K.placeholder_wood()
    root = bpy.data.objects.new(pid, None)
    coll.objects.link(root)

    lods = {}
    G = R if R.handles(pid) else M if M.handles(pid) else B if B.handles(pid) else K
    ncells = family(pid).cells(pid)
    lod_params, high_params = (R.LODS, R.HIGH) if G is R else (K.LODS, K.HIGH)
    for lod, q in lod_params.items():
        ob = G.build_render_object(pid, q, f"{pid}_lod{lod}", coll, mat)
        check_render_bounds(ob, pid)
        tris = tri_count(ob)
        budget = R.budget(pid, lod) if G is R else K.BUDGET_1CELL_BLOCK[lod] * ncells
        print(f"[kit] {pid} LOD{lod}: {tris} tris (budget {budget})")
        if tris > budget:
            raise RuntimeError(f"{pid} LOD{lod}: {tris} tris > budget {budget}")
        ob.parent = root
        lods[lod] = ob
    col = G.build_collider_object(pid, f"{pid}_collider", coll)
    col.parent = root
    ctris = tri_count(col)
    print(f"[kit] {pid} collider: {ctris} tris (budget {K.COLLISION_TRIS_PER_CELL * ncells})")
    if ctris > K.COLLISION_TRIS_PER_CELL * ncells:
        raise RuntimeError(f"{pid} collider: {ctris} tris > {K.COLLISION_TRIS_PER_CELL * ncells}")
    G.build_socket_empties(pid, coll, parent=root)
    part_tris = {}
    if G is M:  # animated parts (P2-T3): §12 budget for all parts together, per LOD
        for lod, obs in M.build_part_objects(pid, coll, root, mat).items():
            part_tris[f"lod{lod}"] = {o.name: tri_count(o) for o in obs}
            total = sum(part_tris[f"lod{lod}"].values())
            print(f"[kit] {pid} parts LOD{lod}: {total} tris (budget {M.PART_BUDGET[lod]})")
            if total > M.PART_BUDGET[lod]:
                raise RuntimeError(f"{pid} parts LOD{lod}: {total} tris > {M.PART_BUDGET[lod]}")
            for o in obs:
                unwrap(o)
    for ob in lods.values():
        unwrap(ob)
    density = {f"lod{lod}": texel_density(ob, map_size(pid, lod)) for lod, ob in lods.items()}

    bake_rep = {"skipped": True}
    if not SKIP_BAKE:
        high = G.build_render_object(pid, high_params, f"{pid}_high", coll, mat)
        select_only([high])
        bpy.ops.wm.save_as_mainfile(filepath=str(out / f"{pid}_high.blend"), compress=True, copy=True)
        setup_cycles()
        channels = {}
        for lod, ob in lods.items():
            s = map_size(pid, lod)
            for kind, highs in (("NORMAL", [high]), ("AO", [])):
                name = f"{pid}_lod{lod}_{kind.lower()}"
                img = bpy.data.images.new(name, s, s, alpha=False, float_buffer=False)
                img.colorspace_settings.name = "Non-Color"
                bake(ob, kind, img, highs)
                path = out / f"{name}.png"
                img.filepath_raw = str(path)
                img.file_format = "PNG"
                img.save()
                channels[name] = {"size": s, "colorspace": "Non-Color", "sha256": sha256(path)}
        mat.node_tree.nodes.remove(mat.node_tree.nodes["BAKE_TARGET"])
        bpy.data.objects.remove(high)
        bake_rep = dict(BAKE, device=DEVICE, channels=channels, highPoly=high_params, skipped=False,
                        blender=bpy.app.version_string)
        (out / "bake-report.json").write_text(json.dumps(bake_rep, indent=2, sort_keys=True) + "\n")

    objs = tree(root)
    for o in objs:
        o.hide_render = False
    bpy.context.view_layer.update()
    select_only(objs, root)
    bpy.ops.export_scene.gltf(
        filepath=str(out / f"{pid}.glb"), export_format="GLB", use_selection=True, use_active_scene=True,
        export_yup=True, export_apply=True, export_extras=True, export_tangents=True, export_normals=True,
        export_texcoords=True, export_cameras=False, export_lights=False, export_image_format="NONE",
        export_materials="EXPORT", export_animations=False)
    (out / f"{pid}.kit.json").write_bytes(G.sidecar_bytes(pid))

    rel = lambda p: pathlib.Path(p).resolve().relative_to(REPO).as_posix()
    rep = {
        "pieceId": pid,
        "nodes": [o.name for o in objs],
        "lodTris": {f"{pid}_lod{lod}": tri_count(ob) for lod, ob in lods.items()},
        "colliderTris": ctris,
        "texelDensityPxPerM": density,
        "mapSize": {f"lod{k}": map_size(pid, k) for k in MAP_SIZE},
        "generator": {"path": rel(__file__), "sha256": sha256(__file__)},
        "generatorDeps": [{"path": rel(m.__file__), "sha256": sha256(m.__file__)}
                          for m in ((K, B, R) if G is R else (K, B, M, S) if G is M else (K, B))],
        "blender": bpy.app.version_string,
        "lodParams": {f"lod{k}": v for k, v in lod_params.items()},
    }
    if part_tris:
        rep["partTris"] = part_tris
    (out / "export-report.json").write_text(json.dumps(rep, indent=2, sort_keys=True) + "\n")
    if not SKIP_BAKE:
        bpy.ops.wm.save_as_mainfile(filepath=str(out / f"{pid}_low.blend"), compress=True)
    print(f"[kit] {pid}: wrote {out}")


def main():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    if not argv:
        raise SystemExit("usage: generate.py -- <stage_dir> [piece_id ...]")
    stage = pathlib.Path(argv[0]).resolve()
    for pid in argv[1:] or B.BUILD + M.BUILD + R.BUILD:
        out = stage / pid
        out.mkdir(parents=True, exist_ok=True)
        build_piece(pid, out)


main()
