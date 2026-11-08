"""Review still of the shipped pieces (P1-T2). Run:

    Blender -b --factory-startup --python-exit-code 1 -P blender/kit/render_review.py -- <runtime_dir> <out.png>

Imports each runtime GLB, shows LOD0 with its baked normal and AO maps bound to the placeholder
wood, marks every port socket with a small cyan bead, and renders one EEVEE still (no stamps).
Evidence only: Blender's view transform is not the engine's tonemapper.
"""
import json
import math
import pathlib
import sys

import bpy

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import kitgeom as K  # noqa: E402

argv = sys.argv[sys.argv.index("--") + 1:]
runtime, out = pathlib.Path(argv[0]).resolve(), pathlib.Path(argv[1]).resolve()
SPACING = 0.085
# Optional piece ids after the output path, laid out in rows of COLS; "<id>:m" adds a mirrored
# copy (contract §8 mirror, x -> -x). Without ids: the four P1-T2 pieces in one row, as before.
IDS = argv[2:] or list(K.BUILD)
COLS = 7 if argv[2:] else len(IDS)
ROW = 0.12

bpy.ops.wm.read_factory_settings(use_empty=True)
scn = bpy.context.scene
bead_mat = bpy.data.materials.new("review_port")
bsdf = bead_mat.node_tree.nodes["Principled BSDF"]
bsdf.inputs["Base Color"].default_value = (0.1, 0.8, 1.0, 1.0)

for i, entry in enumerate(IDS):
    pid, mirrored = entry.split(":")[0], entry.endswith(":m")
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=str(runtime / pid / f"{pid}.glb"))
    new = [o for o in bpy.data.objects if o not in before]
    root = next(o for o in new if o.parent is None)
    root.location.x += (i % COLS) * SPACING
    root.location.y += (i // COLS) * ROW
    if mirrored:
        root.scale.x = -1.0
    fp = json.loads((runtime / pid / f"{pid}.kit.json").read_text())["footprint"]
    root.location.z -= min(c[1] for c in fp) * 0.06  # stand 2-tall pieces on the floor
    lod0 = next(o for o in new if o.name.startswith(f"{pid}_lod0"))
    for o in new:
        if o.type == "MESH" and o is not lod0:
            o.hide_render = True
    m = lod0.data.materials[0].copy()
    lod0.data.materials[0] = m
    nt = m.node_tree
    p = next(n for n in nt.nodes if n.type == "BSDF_PRINCIPLED")
    nimg = nt.nodes.new("ShaderNodeTexImage")
    nimg.image = bpy.data.images.load(str(runtime / pid / f"{pid}_lod0_normal.png"))
    nimg.image.colorspace_settings.name = "Non-Color"
    nmap = nt.nodes.new("ShaderNodeNormalMap")
    nt.links.new(nimg.outputs["Color"], nmap.inputs["Color"])
    nt.links.new(nmap.outputs["Normal"], p.inputs["Normal"])
    aimg = nt.nodes.new("ShaderNodeTexImage")
    aimg.image = bpy.data.images.load(str(runtime / pid / f"{pid}_lod0_ao.png"))
    aimg.image.colorspace_settings.name = "Non-Color"
    mix = nt.nodes.new("ShaderNodeMix")
    mix.data_type = "RGBA"
    mix.blend_type = "MULTIPLY"
    mix.inputs["Factor"].default_value = 1.0
    mix.inputs["A"].default_value = p.inputs["Base Color"].default_value
    nt.links.new(aimg.outputs["Color"], mix.inputs["B"])
    nt.links.new(mix.outputs["Result"], p.inputs["Base Color"])
    bpy.context.view_layer.update()
    for o in new:
        if o.name.startswith("port_"):
            bpy.ops.mesh.primitive_uv_sphere_add(radius=0.0025, location=o.matrix_world.translation)
            bpy.context.active_object.data.materials.append(bead_mat)

bpy.ops.mesh.primitive_plane_add(size=2.0, location=(0, 0, 0))
floor_mat = bpy.data.materials.new("review_floor")
floor_mat.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.05, 0.07, 0.12, 1.0)
bpy.context.active_object.data.materials.append(floor_mat)

world = bpy.data.worlds.new("review_world")
world.node_tree.nodes["Background"].inputs["Color"].default_value = (0.03, 0.045, 0.09, 1.0)
world.node_tree.nodes["Background"].inputs["Strength"].default_value = 0.8
scn.world = world
cx = (min(COLS, len(IDS)) - 1) * SPACING / 2
cy = ((len(IDS) - 1) // COLS) * ROW / 2
target = (cx, cy, 0.03)


def aim(ob, loc):
    ob.location = loc
    d = [target[k] - loc[k] for k in range(3)]
    ob.rotation_euler = (math.atan2(math.hypot(d[0], d[1]), -d[2]), 0.0, math.atan2(d[1], d[0]) - math.pi / 2)


for name, energy, loc, color in (("key", 3.5, (cx - 0.15, -0.2, 0.3), (1.0, 0.85, 0.65)),
                                 ("fill", 0.8, (cx + 0.3, -0.15, 0.12), (0.7, 0.8, 1.0)),
                                 ("rim", 3.0, (cx + 0.05, 0.3, 0.22), (1.0, 0.9, 0.75))):
    ld = bpy.data.lights.new(name, "AREA")
    ld.energy, ld.color, ld.size = energy, color, 0.12
    lo = bpy.data.objects.new(name, ld)
    scn.collection.objects.link(lo)
    aim(lo, loc)

cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
cam.data.lens = 50
scn.collection.objects.link(cam)
if argv[2:]:
    aim(cam, (cx - 0.12, cy - 0.70, 0.55))
else:
    aim(cam, (cx - 0.16, -0.40, 0.40))
scn.camera = cam

scn.render.engine = "BLENDER_EEVEE"
scn.render.resolution_x, scn.render.resolution_y = (1800, 1000) if argv[2:] else (1400, 640)
scn.render.use_stamp = False
scn.render.use_stamp_date = False
scn.render.use_stamp_render_time = False
scn.render.image_settings.file_format = "PNG"
scn.render.image_settings.color_mode = "RGB"
scn.render.image_settings.compression = 100
scn.view_settings.view_transform = "AgX"
scn.render.filepath = str(out)
bpy.ops.render.render(write_still=True)
print(f"[review] wrote {out}")
