"""P1-T5 baked environment probe for the night playroom.

Headless: Blender -b --factory-startup --python-exit-code 1 -P blender/playroom/probe.py -- <repo>
Rebuilds the room from assets/playroom/layout.json (the same file the renderer reads), renders the
six cube faces from the probe position with Cycles, then prefilters on the CPU (numpy):

- specular: SPEC_SIZE^2 faces with a full mip chain; mip k is a GGX-like (Phong-lobe) convolution
  at perceptual roughness k / (levels - 1), mip 0 is the mirror image.
- diffuse: DIFF_SIZE^2 faces, cosine-convolved radiance (irradiance / pi).

Output, raw little-endian RGBA16F in wgpu's layer-major order (face 0 mips 0..n, then face 1...):
assets/playroom/env_specular.rgba16f, assets/playroom/env_diffuse.rgba16f, plus env_probe.json.
Face order and texel directions follow the wgpu/Vulkan cube convention; Bevy samples the cube with
(x, y, -z) of the world direction, so face texel direction c maps to world direction (c.x, c.y, -c.z).
Radiance is in relative units: the renderer scales it with EnvironmentMapLight::intensity.
"""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path

import bpy
import numpy as np
from mathutils import Matrix, Vector

RENDER_SIZE = 128
SPEC_SIZE = 64
DIFF_SIZE = 16
SAMPLES = 64
SEED = 0


def repo() -> Path:
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    if not argv:
        raise SystemExit("usage: ... -- <repo>")
    return Path(argv[0]).resolve()


def b(v) -> Vector:
    """Bevy (x, y, z), +Y up, to Blender (x, -z, y), +Z up."""
    return Vector((v[0], -v[2], v[1]))


def emission_mat(name: str, rgb, strength: float, image: str | None = None, scale=(1.0, 1.0)):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    nt.nodes.clear()
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    em = nt.nodes.new("ShaderNodeEmission")
    em.inputs["Strength"].default_value = strength
    em.inputs["Color"].default_value = (*rgb, 1.0)
    if image:
        tex = nt.nodes.new("ShaderNodeTexImage")
        tex.image = bpy.data.images.load(image)
        nt.links.new(tex.outputs["Color"], em.inputs["Color"])
    nt.links.new(em.outputs["Emission"], out.inputs["Surface"])
    return m


def diffuse_mat(name: str, rgb, image: str | None = None, tile=(1.0, 1.0)):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    bsdf = nt.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*rgb, 1.0)
    bsdf.inputs["Roughness"].default_value = 0.95
    if image:
        tex = nt.nodes.new("ShaderNodeTexImage")
        tex.image = bpy.data.images.load(image)
        tex.extension = "REPEAT"
        coord = nt.nodes.new("ShaderNodeTexCoord")
        mapping = nt.nodes.new("ShaderNodeMapping")
        mapping.inputs["Scale"].default_value = (tile[0], tile[1], 1.0)
        nt.links.new(coord.outputs["UV"], mapping.inputs["Vector"])
        nt.links.new(mapping.outputs["Vector"], tex.inputs["Vector"])
        nt.links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    return m


def plane(name: str, center, sx: float, sy: float, mat, rot=(0.0, 0.0, 0.0)):
    bpy.ops.mesh.primitive_plane_add(size=1.0, location=center, rotation=rot)
    o = bpy.context.active_object
    o.name = name
    o.scale = (sx, sy, 1.0)
    o.data.materials.append(mat)
    return o


def build_room(root: Path, lay: dict) -> None:
    for ob in list(bpy.data.objects):
        bpy.data.objects.remove(ob, do_unlink=True)
    room = lay["room"]
    lo, hi = Vector(room["min"]), Vector(room["max"])
    size = hi - lo
    centre = (lo + hi) / 2
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=b(centre))
    shell = bpy.context.active_object
    shell.name = "room_shell"
    shell.scale = (size.x, size.z, size.y)
    shell.data.materials.append(diffuse_mat("wall", room["color"]))
    floor_c = Vector((centre.x, 0.0005, centre.z))
    plane("floor", b(floor_c), size.x, size.z, diffuse_mat("floor", lay["floor"]["color"]))
    rug = lay["rug"]
    tile = rug["tileM"]
    plane(
        "rug",
        b((rug["center"][0], 0.001, rug["center"][1])),
        rug["size"][0],
        rug["size"][1],
        diffuse_mat(
            "rug",
            (1, 1, 1),
            str(root / "assets-source/playroom/rug/rug.base_color.png"),
            (rug["size"][0] / tile, rug["size"][1] / tile),
        ),
    )
    bd = lay["backdrop"]
    plane(
        "backdrop",
        b(bd["center"]),
        bd["size"][0],
        bd["size"][1],
        emission_mat("backdrop", (1, 1, 1), 0.35, str(root / "assets-source/playroom/backdrop/backdrop-01.png")),
        rot=(math.pi / 2, 0.0, 0.0),
    )
    warm = (1.0, 0.72, 0.42)
    for i, lan in enumerate(lay["lanterns"]):
        bpy.ops.mesh.primitive_uv_sphere_add(radius=lan["radius"], location=b(lan["center"]))
        o = bpy.context.active_object
        o.name = f"lantern_{i}"
        o.data.materials.append(emission_mat(f"lantern_{i}", warm, lan["lumens"] / 1200.0))
    lamp = lay["lamp"]
    top = Vector(lamp["base"]) + Vector((0.0, lamp["height"], 0.0))
    bpy.ops.mesh.primitive_cone_add(
        radius1=lamp["shadeRadius"], radius2=lamp["shadeRadius"] * 0.6, depth=0.18, location=b(top)
    )
    o = bpy.context.active_object
    o.name = "lamp_shade"
    o.data.materials.append(emission_mat("lamp", warm, lamp["lumens"] / 4000.0))
    moon = lay["moon"]
    bpy.ops.object.light_add(type="SUN", location=b(moon["from"]))
    sun = bpy.context.active_object
    sun.data.energy = 0.05
    sun.data.color = (0.55, 0.65, 1.0)
    direction = (Vector((0, 0, 0)) - b(moon["from"])).normalized()
    sun.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()


# wgpu/Vulkan cube faces: texel (s, t) in [-1, 1], s right, t down.
FACES = [
    lambda s, t: (np.ones_like(s), -t, -s),  # +X
    lambda s, t: (-np.ones_like(s), -t, s),  # -X
    lambda s, t: (s, np.ones_like(s), t),  # +Y
    lambda s, t: (s, -np.ones_like(s), -t),  # -Y
    lambda s, t: (s, -t, np.ones_like(s)),  # +Z
    lambda s, t: (-s, -t, -np.ones_like(s)),  # -Z
]


def face_dirs(face: int, n: int) -> np.ndarray:
    c = (np.arange(n) + 0.5) / n * 2 - 1
    t, s = np.meshgrid(c, c, indexing="ij")  # rows = t (down), cols = s (right)
    x, y, z = FACES[face](s, t)
    d = np.stack([x, y, -z], axis=-1)  # cube direction -> Bevy world direction
    return d / np.linalg.norm(d, axis=-1, keepdims=True)


def render_faces(root: Path, pos) -> np.ndarray:
    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.samples = SAMPLES
    scene.cycles.seed = SEED
    scene.cycles.use_denoising = False
    scene.render.resolution_x = scene.render.resolution_y = RENDER_SIZE
    scene.render.image_settings.file_format = "OPEN_EXR"
    scene.render.image_settings.color_depth = "32"
    scene.view_settings.view_transform = "Standard"
    scene.world = bpy.data.worlds.new("black")
    scene.world.color = (0.0, 0.0, 0.0)
    cam_data = bpy.data.cameras.new("probe")
    cam_data.angle = math.pi / 2
    cam_data.clip_start = 0.001
    cam = bpy.data.objects.new("probe", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam
    cam.location = b(pos)
    tmp = root / "target/playroom-stage/probe"
    tmp.mkdir(parents=True, exist_ok=True)
    faces = np.zeros((6, RENDER_SIZE, RENDER_SIZE, 3), dtype=np.float32)
    for f in range(6):
        # Direction derivatives in Bevy world space: right = d/ds, up = -d/dt, forward = centre.
        fwd = face_dirs(f, 3)[1, 1]
        right = face_dirs(f, 3)[1, 2] - face_dirs(f, 3)[1, 0]
        up = face_dirs(f, 3)[0, 1] - face_dirs(f, 3)[2, 1]
        r, u, w = (b(v / np.linalg.norm(v)) for v in (right, up, -fwd))
        cam.matrix_world = Matrix(
            ((r.x, u.x, w.x, cam.location.x), (r.y, u.y, w.y, cam.location.y), (r.z, u.z, w.z, cam.location.z), (0, 0, 0, 1))
        )
        path = tmp / f"face{f}.exr"
        scene.render.filepath = str(path)
        bpy.ops.render.render(write_still=True)
        img = bpy.data.images.load(str(path))
        px = np.array(img.pixels[:], dtype=np.float32).reshape(RENDER_SIZE, RENDER_SIZE, 4)
        faces[f] = px[::-1, :, :3]  # Blender rows are bottom-up; cube rows are top-down
        bpy.data.images.remove(img)
    return faces


def solid_angle(n: int) -> np.ndarray:
    c = (np.arange(n) + 0.5) / n * 2 - 1
    t, s = np.meshgrid(c, c, indexing="ij")
    return (2.0 / n) ** 2 / (1 + s * s + t * t) ** 1.5


def convolve(faces: np.ndarray, out_size: int, exponent: float | None) -> np.ndarray:
    n = faces.shape[1]
    src_d = np.concatenate([face_dirs(f, n).reshape(-1, 3) for f in range(6)])
    src_l = faces.reshape(-1, 3).astype(np.float64)
    src_w = np.tile(solid_angle(n).reshape(-1), 6)
    out = np.zeros((6, out_size, out_size, 3))
    for f in range(6):
        d = face_dirs(f, out_size).reshape(-1, 3)
        acc = np.zeros((d.shape[0], 3))
        for i in range(0, d.shape[0], 256):
            cos = d[i : i + 256] @ src_d.T
            cos = np.maximum(cos, 0.0)
            k = cos if exponent is None else cos**exponent
            k = k * src_w
            acc[i : i + 256] = (k @ src_l) / np.maximum(k.sum(axis=1, keepdims=True), 1e-12)
        out[f] = acc.reshape(out_size, out_size, 3)
    return out


def downsample(faces: np.ndarray, size: int) -> np.ndarray:
    f = faces.shape[1] // size
    return faces.reshape(6, size, f, size, f, 3).mean(axis=(2, 4))


def to_rgba16f(face: np.ndarray) -> bytes:
    rgba = np.concatenate([face, np.ones(face.shape[:-1] + (1,))], axis=-1)
    return rgba.astype("<f2").tobytes()


def main() -> None:
    root = repo()
    lay = json.loads((root / "assets/playroom/layout.json").read_text())
    build_room(root, lay)
    faces = render_faces(root, lay["probe"]["position"])
    small = downsample(faces, 32)
    levels = int(math.log2(SPEC_SIZE)) + 1
    spec = [downsample(faces, SPEC_SIZE)]
    for k in range(1, levels):
        size = SPEC_SIZE >> k
        rough = k / (levels - 1)
        alpha = max(rough * rough, 1e-3)
        exponent = max(2.0 / (alpha * alpha) - 2.0, 1.0)
        src = faces if exponent > 200 else small
        spec.append(convolve(src, size, exponent))
    diffuse = convolve(small, DIFF_SIZE, None)
    out = root / "assets/playroom"
    with open(out / "env_specular.rgba16f", "wb") as fh:
        for f in range(6):
            for m in spec:
                fh.write(to_rgba16f(m[f]))
    with open(out / "env_diffuse.rgba16f", "wb") as fh:
        for f in range(6):
            fh.write(to_rgba16f(diffuse[f]))
    meta = {
        "specularSize": SPEC_SIZE,
        "specularLevels": levels,
        "diffuseSize": DIFF_SIZE,
        "format": "rgba16float, layer-major (face, then mip)",
        "renderSize": RENDER_SIZE,
        "samples": SAMPLES,
        "meanRadiance": [round(float(x), 5) for x in faces.reshape(-1, 3).mean(axis=0)],
        "maxRadiance": round(float(faces.max()), 3),
    }
    (out / "env_probe.json").write_text(json.dumps(meta, indent=1) + "\n")
    # Review strip: the six mirror faces, tone-mapped.
    review = root / "assets-source/playroom/probe"
    review.mkdir(parents=True, exist_ok=True)
    strip = np.concatenate(list(faces), axis=1)
    strip = np.clip((strip / (1 + strip)) ** (1 / 2.2), 0, 1)
    img = bpy.data.images.new("strip", strip.shape[1], strip.shape[0])
    img.pixels = np.concatenate([strip[::-1], np.ones(strip.shape[:2] + (1,))[::-1]], axis=-1).ravel()
    img.filepath_raw = str(review / "probe-faces.png")
    img.file_format = "PNG"
    img.save()
    print(json.dumps(meta))


main()
