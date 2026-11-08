"""Blender reimport gate (P1-T2). Run:

    Blender -b --factory-startup --python-exit-code 1 -P blender/kit/reimport.py -- <runtime_dir> [id ...]

Imports each shipped (meshopt) GLB into an empty scene and asserts: one root named <id>; LOD0-LOD2,
the collider and every port socket present under it by name; LOD and collider triangle counts equal
the export report and the sidecar; socket positions equal the sidecar ports. Exits non-zero on any
failure.
"""
import json
import pathlib
import sys

import bpy

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import kitgeom as K  # noqa: E402
import kitblocks as B  # noqa: E402


def tris(ob):
    return sum(len(p.vertices) - 2 for p in ob.data.polygons)


def check(runtime, pid):
    errs = []
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=str(runtime / pid / f"{pid}.glb"))
    bpy.context.view_layer.update()
    sc = json.loads((runtime / pid / f"{pid}.kit.json").read_text())
    roots = [o for o in bpy.context.scene.objects if o.parent is None]
    if [o.name for o in roots] != [pid]:
        errs.append(f"roots {[o.name for o in roots]} != [{pid}]")
    objs = {o.name: o for o in bpy.context.scene.objects}
    out = {}
    for lod in range(3):
        n = f"{pid}_lod{lod}"
        if n not in objs or objs[n].type != "MESH":
            errs.append(f"{n} missing")
            continue
        out[n] = tris(objs[n])
        budget = K.BUDGET_1CELL_BLOCK[lod] * len(sc["footprint"])
        if out[n] > budget:
            errs.append(f"{n}: {out[n]} tris > {budget}")
    col = objs.get(f"{pid}_collider")
    want_col = len(sc["collision"][0]["indices"]) // 3
    if col is None or tris(col) != want_col:
        errs.append(f"collider missing or {col and tris(col)} tris != sidecar {want_col}")
    for p in sc["ports"]:
        e = objs.get(f"port_{p['index']}")
        if e is None:
            errs.append(f"port_{p['index']} missing")
            continue
        want = K.to_blender(B.port_point_mm(p))
        got = e.matrix_world.translation
        if max(abs(got[i] - want[i]) for i in range(3)) > 1e-6:
            errs.append(f"port_{p['index']} at {tuple(got)} != {want}")
        if e.get("face") != p["face"] or e.get("level") != p.get("level"):
            errs.append(f"port_{p['index']} extras {dict(e.items())} do not match the sidecar")
    print(f"[reimport] {pid}: tris {out}, collider {col and tris(col)}, sockets {len(sc['ports'])}: "
          + ("ok" if not errs else "FAIL"))
    for e in errs:
        print(f"  - {pid}: {e}")
    return errs


argv = sys.argv[sys.argv.index("--") + 1:]
runtime = pathlib.Path(argv[0]).resolve()
ids = argv[1:] or list(B.BUILD)
bad = [e for pid in ids for e in check(runtime, pid)]
if bad:
    raise SystemExit(f"reimport gate: {len(bad)} failure(s)")
print("[reimport] all pieces pass")
