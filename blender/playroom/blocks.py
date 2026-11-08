"""P1-T5 toy-block set dressing: simple rigid wooden pieces, one GLB each.

Headless: Blender -b --factory-startup --python-exit-code 1 -P blender/playroom/blocks.py -- <out_dir>
Builds each piece in a headless --factory-startup process (never a user's open file), bevels the edges,
assigns one pale-beech Principled material and exports a plain GLB (no Draco, no meshopt, no
lights or cameras). Origin at the bottom centre, Blender +Z up (glTF +Y up after export).
Render-only: the pieces never become colliders.
"""

from __future__ import annotations

import math
import sys
from pathlib import Path

import bpy

U = 0.03  # block unit, metres
BEVEL = 0.0016
BEECH = (0.70, 0.52, 0.34, 1.0)  # linear base colour


def out_dir() -> Path:
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    if not argv:
        raise SystemExit("usage: ... -- <out_dir>")
    p = Path(argv[0])
    p.mkdir(parents=True, exist_ok=True)
    return p


def material() -> bpy.types.Material:
    mat = bpy.data.materials.get("toy_beech") or bpy.data.materials.new("toy_beech")
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = BEECH
    bsdf.inputs["Roughness"].default_value = 0.55
    return mat


def box(name: str, sx: float, sy: float, sz: float, z0: float = 0.0) -> bpy.types.Object:
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=(0, 0, z0 + sz / 2))
    o = bpy.context.active_object
    o.name = name
    o.scale = (sx, sy, sz)
    bpy.ops.object.transform_apply(scale=True)
    return o


def cut(target: bpy.types.Object, tool: bpy.types.Object) -> None:
    m = target.modifiers.new("cut", "BOOLEAN")
    m.operation = "DIFFERENCE"
    m.solver = "EXACT"
    m.object = tool
    bpy.context.view_layer.objects.active = target
    bpy.ops.object.modifier_apply(modifier=m.name)
    bpy.data.objects.remove(tool, do_unlink=True)


def piece(name: str) -> bpy.types.Object:
    if name == "cube":
        return box(name, U, U, U)
    if name == "brick":
        return box(name, 2 * U, U, U)
    if name == "plank":
        return box(name, 4 * U, U, U)
    if name == "pillar":
        bpy.ops.mesh.primitive_cylinder_add(vertices=24, radius=U / 2, depth=2 * U, location=(0, 0, U))
        o = bpy.context.active_object
        o.name = name
        return o
    if name == "roof":
        # Triangular prism: ridge along +X, 2U long, U deep, U tall.
        verts = []
        for x in (-U, U):
            verts += [(x, -U / 2, 0), (x, U / 2, 0), (x, 0, U)]
        faces = [(0, 2, 1), (3, 4, 5), (0, 1, 4, 3), (1, 2, 5, 4), (2, 0, 3, 5)]
        me = bpy.data.meshes.new(name)
        me.from_pydata(verts, [], faces)
        me.update()
        o = bpy.data.objects.new(name, me)
        bpy.context.collection.objects.link(o)
        bpy.context.view_layer.objects.active = o
        return o
    if name == "arch":
        # 3U wide, 2U tall, U deep, with a round-topped doorway.
        o = box(name, 3 * U, U, 2 * U)
        r = 0.75 * U
        door = box("door", 2 * r, 2 * U, 2 * U - r - 0.2 * U, -0.01)
        cut(o, door)
        bpy.ops.mesh.primitive_cylinder_add(
            vertices=48, radius=r, depth=2 * U, location=(0, 0, 2 * U - r - 0.2 * U - 0.01),
            rotation=(math.pi / 2, 0, 0),
        )
        cut(o, bpy.context.active_object)
        return o
    raise ValueError(name)


PIECES = ["cube", "brick", "plank", "pillar", "roof", "arch"]


def build(name: str, out: Path) -> None:
    # A separate headless --factory-startup process: its scene is never a user's open file.
    for ob in list(bpy.data.objects):
        bpy.data.objects.remove(ob, do_unlink=True)
    o = piece(name)
    bv = o.modifiers.new("bevel", "BEVEL")
    bv.width = BEVEL
    bv.segments = 2
    bv.limit_method = "ANGLE"
    bv.harden_normals = False
    o.data.materials.clear()
    o.data.materials.append(material())
    for p in o.data.polygons:
        p.use_smooth = False
    o.name = f"toy_{name}"
    path = out / f"toy_{name}.glb"
    bpy.ops.object.select_all(action="DESELECT")
    o.select_set(True)
    bpy.context.view_layer.objects.active = o
    bpy.ops.export_scene.gltf(
        filepath=str(path),
        export_format="GLB",
        use_selection=True,
        use_active_scene=True,
        export_apply=True,
        export_lights=False,
        export_cameras=False,
        export_yup=True,
        export_texcoords=False,
        export_materials="EXPORT",
    )
    print(f"exported {path} tris={sum(len(p.vertices) - 2 for p in o.data.polygons)}")


def main() -> None:
    out = out_dir()
    for name in PIECES:
        build(name, out)


main()
