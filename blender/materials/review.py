"""Owner-review render: three L3 block-channel pieces (one per wood variant) beside ref-02.

Headless: Blender -b --factory-startup --python-exit-code 1 -P blender/materials/review.py
Writes blender/materials/review/side-by-side.png (left: EEVEE render of the shipped 1024 PNG maps,
middle: docs/reference/ref-02.webp, right: a crop of the approved look-dev frame 07 the colours are calibrated to). The blocks are approximate kit-contract §5.2 bodies (59 mm
rendered cube, 28 mm slot, R14 half-round centred at 59 mm, 1.5 mm bevel); P1-T2 owns the real kit.
`build_showcase(scene)` is also run inside Paul's live Blender session (scene PB_materials).
"""

from __future__ import annotations

import math
import os
import sys

import bpy
import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
VARIANTS = ["wood_warm_a", "wood_warm_b", "wood_warm_c"]
TILE_M = 0.25
CELL = 0.060
BODY = 0.059
PFX = "PBM_"
LOOKDEV = "assets-source/lookdev/07-wave-channel-opal-marble.png"
LOOKDEV_CROP = (0, 0, 1400, 1050)  # x0, y0, x1, y1 in image px, top-left origin


def wood_material(name: str) -> bpy.types.Material:
    mat = bpy.data.materials.new(f"{PFX}{name}_runtime")
    nt = mat.node_tree
    nt.nodes.clear()
    d = os.path.join(ROOT, "assets/materials", name)

    def tex(ch: str, colorspace: str, y: int) -> bpy.types.Node:
        n = nt.nodes.new("ShaderNodeTexImage")
        n.image = bpy.data.images.load(os.path.join(d, f"{name}.{ch}.png"), check_existing=True)
        n.image.name = f"{PFX}{name}.{ch}"
        n.image.colorspace_settings.name = colorspace
        n.location = (-700, y)
        return n

    bsdf = nt.nodes.new("ShaderNodeBsdfPrincipled")
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    out.location = (300, 0)
    nt.links.new(bsdf.outputs[0], out.inputs["Surface"])
    bc = tex("base_color", "sRGB", 300)
    nt.links.new(bc.outputs["Color"], bsdf.inputs["Base Color"])
    orm = tex("orm", "Non-Color", 0)
    sep = nt.nodes.new("ShaderNodeSeparateColor")
    sep.location = (-400, 0)
    nt.links.new(orm.outputs["Color"], sep.inputs[0])
    nt.links.new(sep.outputs[1], bsdf.inputs["Roughness"])
    nt.links.new(sep.outputs[2], bsdf.inputs["Metallic"])
    nrm = tex("normal", "Non-Color", -300)
    nm = nt.nodes.new("ShaderNodeNormalMap")  # tangent space, OpenGL (+Y) convention
    nm.location = (-300, -300)
    nt.links.new(nrm.outputs["Color"], nm.inputs["Color"])
    nt.links.new(nm.outputs["Normal"], bsdf.inputs["Normal"])
    return mat


def block(scene: bpy.types.Scene, name: str, x: float, mat: bpy.types.Material) -> bpy.types.Object:
    import bmesh

    # Section in XZ (metres): 59 mm body inset 0.5 mm in the cell, R14 half-round centred at z = 59 mm.
    h = BODY / 2
    pts = [(-h, 0.0005), (h, 0.0005), (h, 0.0595), (0.014, 0.0595)]
    for k in range(33):
        t = -math.pi * k / 32
        pts.append((0.014 * math.cos(t), 0.059 + 0.014 * math.sin(t)))
    pts += [(-0.014, 0.0595), (-h, 0.0595)]
    bm = bmesh.new()
    verts = [bm.verts.new((px, -h, pz)) for px, pz in pts]
    face = bm.faces.new(verts)
    ext = bmesh.ops.extrude_face_region(bm, geom=[face])
    bmesh.ops.translate(bm, vec=(0, BODY, 0), verts=[e for e in ext["geom"] if isinstance(e, bmesh.types.BMVert)])
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bmesh.ops.triangulate(bm, faces=[f for f in bm.faces if len(f.verts) > 4])
    mesh = bpy.data.meshes.new(f"{PFX}{name}_mesh")
    bm.to_mesh(mesh)
    bm.free()
    obj = bpy.data.objects.new(f"{PFX}{name}", mesh)
    scene.collection.objects.link(obj)
    obj.location.x = x
    bev = obj.modifiers.new("bevel", "BEVEL")
    bev.width = 0.0015
    bev.segments = 3
    bev.limit_method = "ANGLE"
    bev.angle_limit = math.radians(50)
    obj.data.materials.append(mat)

    # Apply modifiers so UVs cover the carved channel, then project 1 UV unit = 0.25 m.
    dg = bpy.context.evaluated_depsgraph_get()
    ev = obj.evaluated_get(dg)
    baked = bpy.data.meshes.new_from_object(ev)
    obj.modifiers.clear()
    old = obj.data
    obj.data = baked
    bpy.data.meshes.remove(old)
    uv = baked.uv_layers.new(name="UVMap")
    for poly in baked.polygons:
        n = poly.normal
        poly.use_smooth = abs(n[0]) < 0.999 and abs(n[1]) < 0.999 and abs(n[2]) < 0.999
        ax = max(range(3), key=lambda i: abs(n[i]))
        for li in poly.loop_indices:
            co = baked.vertices[baked.loops[li].vertex_index].co
            p = (co.x + x, co.y, co.z)
            # Grain runs along the block's length (Y) on top/side faces.
            if ax == 2:
                u, v = p[1], p[0]
            elif ax == 0:
                u, v = p[1], p[2]
            else:
                u, v = p[0], p[2]
            uv.data[li].uv = (u / TILE_M + 0.37 * (ax + 1), v / TILE_M + 0.21 * (ax + 1))
    return obj


def build_showcase(scene: bpy.types.Scene) -> bpy.types.Object:
    scene.render.engine = "BLENDER_EEVEE"
    world = bpy.data.worlds.new(f"{PFX}world")
    bg = world.node_tree.nodes.get("Background")
    bg.inputs["Color"].default_value = (0.004, 0.005, 0.012, 1.0)
    bg.inputs["Strength"].default_value = 1.0
    scene.world = world
    for i, v in enumerate(VARIANTS):
        block(scene, v, (i - 1) * CELL, wood_material(v))

    def light(name: str, kind: str, energy: float, color, loc, rot, size: float = 0.1):
        ld = bpy.data.lights.new(f"{PFX}{name}", kind)
        ld.energy = energy
        ld.color = color
        if kind == "AREA":
            ld.size = size
        o = bpy.data.objects.new(f"{PFX}{name}", ld)
        scene.collection.objects.link(o)
        o.location = loc
        o.rotation_euler = rot
        return o

    light("key", "AREA", 2.2, (1.0, 0.78, 0.52), (-0.18, -0.20, 0.30), (math.radians(45), 0, math.radians(-40)), 0.12)
    light("fill", "AREA", 0.25, (0.55, 0.65, 1.0), (0.25, -0.15, 0.12), (math.radians(70), 0, math.radians(55)), 0.3)
    light("rim", "AREA", 1.2, (1.0, 0.85, 0.65), (0.05, 0.25, 0.20), (math.radians(-55), 0, math.radians(170)), 0.08)

    cam_d = bpy.data.cameras.new(f"{PFX}camera")
    cam_d.lens = 50
    cam = bpy.data.objects.new(f"{PFX}camera", cam_d)
    scene.collection.objects.link(cam)
    cam.location = (0.02, -0.26, 0.19)
    aim = bpy.data.objects.new(f"{PFX}camera_target", None)
    scene.collection.objects.link(aim)
    aim.location = (0.0, 0.0, 0.045)
    track = cam.constraints.new("TRACK_TO")
    track.target = aim
    scene.camera = cam
    scene.render.resolution_x = 1280
    scene.render.resolution_y = 720
    return cam


def load_rgb(path: str) -> np.ndarray:
    img = bpy.data.images.load(path, check_existing=False)
    img.colorspace_settings.name = "Non-Color"
    w, h = img.size
    px = np.empty(w * h * 4, dtype=np.float32)
    img.pixels.foreach_get(px)
    bpy.data.images.remove(img)
    return px.reshape(h, w, 4)[:, :, :3]


def resize_h(a: np.ndarray, h: int) -> np.ndarray:
    ys = (np.arange(h) + 0.5) * a.shape[0] / h
    w = round(a.shape[1] * h / a.shape[0])
    xs = (np.arange(w) + 0.5) * a.shape[1] / w
    return a[np.clip(ys.astype(int), 0, a.shape[0] - 1)][:, np.clip(xs.astype(int), 0, a.shape[1] - 1)]


def main() -> None:
    for o in list(bpy.data.objects):
        bpy.data.objects.remove(o, do_unlink=True)
    scene = bpy.context.scene
    build_showcase(scene)
    out_dir = os.path.join(HERE, "review")
    os.makedirs(out_dir, exist_ok=True)
    render_path = os.path.join(ROOT, "target/materials-stage/review-render.png")
    scene.render.filepath = render_path
    scene.render.image_settings.file_format = "PNG"
    bpy.ops.render.render(write_still=True)
    left = load_rgb(render_path)
    if left.mean() < 0.01:
        sys.exit("review.py: render is black")
    right = resize_h(load_rgb(os.path.join(ROOT, "docs/reference/ref-02.webp")), left.shape[0])
    lookdev = load_rgb(os.path.join(ROOT, LOOKDEV))[::-1]  # top-down rows for the crop box
    x0, y0, x1, y1 = LOOKDEV_CROP
    lookdev = resize_h(lookdev[y0:y1, x0:x1][::-1], left.shape[0])
    gap = np.zeros((left.shape[0], 12, 3), np.float32)
    sheet = np.concatenate([left, gap, right, gap, lookdev], axis=1)
    h, w = sheet.shape[:2]
    rgba = np.concatenate([np.round(np.clip(sheet, 0, 1) * 255) / 255, np.ones((h, w, 1))], axis=-1).astype(np.float32)
    img = bpy.data.images.new(f"{PFX}sheet", w, h, alpha=True, float_buffer=False)
    img.colorspace_settings.name = "Non-Color"
    img.pixels.foreach_set(rgba.ravel())
    img.filepath_raw = os.path.join(out_dir, "side-by-side.png")
    img.file_format = "PNG"
    img.save()
    print(f"review.py: wrote {img.filepath_raw} {w}x{h}, render mean {left.mean():.3f}")


if __name__ == "__main__":
    main()
