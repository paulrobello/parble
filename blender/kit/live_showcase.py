"""Builds the P1-T2 pieces into the live Blender session for review (not part of any gate).

Run through the community MCP socket, under the shared lock:
    python3 ~/Repos/mux-xr/.local/tools/bl.py exec blender/kit/live_showcase.py

Touches only scene "PB_kit" and datablocks prefixed "PBK_". Never resets, purges, bakes or exports.
Set LOD below to show another LOD.
"""
import importlib
import math
import sys

import bpy

KIT_DIR = "__KIT_DIR__"
LOD = 0
SPACING = 0.09  # m between piece centres
PFX = "PBK_"

if KIT_DIR not in sys.path:
    sys.path.insert(0, KIT_DIR)
import kitgeom as K  # noqa: E402

importlib.reload(K)

scn = bpy.data.scenes.get("PB_kit") or bpy.data.scenes.new("PB_kit")
win = bpy.context.window or bpy.context.window_manager.windows[0]
win.scene = scn
scn.render.engine = "BLENDER_EEVEE"
scn.unit_settings.system = "METRIC"

# Remove our previous build: PBK_ objects in this scene, then orphaned PBK_ meshes.
for ob in [o for o in scn.objects if o.name.startswith(PFX)]:
    bpy.data.objects.remove(ob, do_unlink=True)
for me in [m for m in bpy.data.meshes if m.name.startswith(PFX) and m.users == 0]:
    bpy.data.meshes.remove(me)

coll = bpy.data.collections.get(PFX + "pieces") or bpy.data.collections.new(PFX + "pieces")
if coll.name not in scn.collection.children:
    scn.collection.children.link(coll)
dg = scn.view_layers[0].depsgraph
mat = K.placeholder_wood(PFX + "wood_placeholder")

for i, pid in enumerate(K.BUILD):
    root = bpy.data.objects.new(PFX + pid, None)
    root.empty_display_size = 0.005
    coll.objects.link(root)
    ob = K.build_render_object(pid, K.LODS[LOD], f"{PFX}{pid}_lod{LOD}", coll, mat, depsgraph=dg)
    ob.parent = root
    col = K.build_collider_object(pid, f"{PFX}{pid}_collider", coll)
    col.parent = root
    col.hide_set(True)
    col.hide_render = True
    K.build_socket_empties(pid, coll, parent=root, prefix=f"{PFX}{pid}_")
    root.location = (i * SPACING, 0.0, 0.0)
    print(pid, "LOD", LOD, sum(len(p.vertices) - 2 for p in ob.data.polygons), "tris")

# World and key / fill / rim lights, confined to PB_kit.
world = bpy.data.worlds.get(PFX + "world") or bpy.data.worlds.new(PFX + "world")
bg = world.node_tree.nodes.get("Background")
bg.inputs["Color"].default_value = (0.035, 0.05, 0.1, 1.0)
bg.inputs["Strength"].default_value = 0.6
scn.world = world
centre = ((len(K.BUILD) - 1) * SPACING / 2, 0.0, 0.03)
for name, kind, energy, loc, color in (
        ("key", "AREA", 3.5, (centre[0] - 0.12, -0.22, 0.26), (1.0, 0.86, 0.68)),
        ("fill", "AREA", 0.8, (centre[0] + 0.3, -0.18, 0.1), (0.7, 0.8, 1.0)),
        ("rim", "AREA", 3.0, (centre[0] + 0.05, 0.3, 0.2), (1.0, 0.92, 0.8))):
    ld = bpy.data.lights.get(PFX + name) or bpy.data.lights.new(PFX + name, kind)
    ld.energy, ld.color = energy, color
    ld.size = 0.12
    lo = bpy.data.objects.get(PFX + name) or bpy.data.objects.new(PFX + name, ld)
    if lo.name not in scn.collection.objects:
        scn.collection.objects.link(lo)
    lo.location = loc
    d = [centre[k] - loc[k] for k in range(3)]
    lo.rotation_euler = (math.atan2(math.hypot(d[0], d[1]), -d[2]), 0.0, math.atan2(d[1], d[0]) - math.pi / 2)

cam_d = bpy.data.cameras.get(PFX + "cam") or bpy.data.cameras.new(PFX + "cam")
cam_d.lens = 50
cam = bpy.data.objects.get(PFX + "cam") or bpy.data.objects.new(PFX + "cam", cam_d)
if cam.name not in scn.collection.objects:
    scn.collection.objects.link(cam)
cam.location = (centre[0] - 0.16, -0.40, 0.40)
d = [centre[k] - cam.location[k] for k in range(3)]
cam.rotation_euler = (math.atan2(math.hypot(d[0], d[1]), -d[2]), 0.0, math.atan2(d[1], d[0]) - math.pi / 2)
scn.camera = cam

for area in win.screen.areas:
    if area.type == "VIEW_3D":
        sp = area.spaces.active
        sp.shading.type = "RENDERED"
        sp.clip_start = 0.001
        r3d = sp.region_3d
        r3d.view_perspective = "CAMERA"
print("PB_kit built:", ", ".join(K.BUILD))
