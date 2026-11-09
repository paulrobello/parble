"""Editor palette thumbnails for the builtin kit pieces (web/ui/thumbs/<id>.png, 192x192, transparent).

Run through `make kit-thumbs`. Two modes in one file:
  python3 blender/kit/thumbnails.py [--check] [--sheet] [id ...]   orchestrator (needs Pillow)
  Blender -b ... -P blender/kit/thumbnails.py -- <raw_dir> <id ...>  render worker (EEVEE, 3/4 ortho view)

The worker renders each piece's LOD0 meshes (shipped GLB, baked LOD0 AO map multiplied into the base colour)
at 2x; the orchestrator downsamples, quantises and records provenance.
"""
import datetime
import hashlib
import json
import math
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
KIT = ROOT / "assets" / "kit"
OUT = ROOT / "web" / "ui" / "thumbs"
RAW = ROOT / "target" / "thumbs-raw"
SIZE, SS, FILL, AZ, EL, COLORS = 192, 2, 0.80, 45.0, 30.0, 192
BLENDER = "/Applications/Blender.app/Contents/MacOS/Blender"

try:
    import bpy
    from mathutils import Vector
except ImportError:
    bpy = None


def piece_ids():
    return sorted(p.name for p in KIT.iterdir() if (p / f"{p.name}.glb").is_file())


def sha(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()


# ---------------------------------------------------------------- Blender worker
def render_piece(pid, raw_dir):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=str(KIT / pid / f"{pid}.glb"))
    sc = bpy.context.scene
    ao_path = KIT / pid / f"{pid}_lod0_ao.png"
    ao = bpy.data.images.load(str(ao_path)) if ao_path.is_file() else None
    if ao:
        ao.colorspace_settings.name = "Non-Color"
    meshes = [o for o in sc.objects if o.type == "MESH" and o.name.endswith("_lod0")]
    for o in sc.objects:
        if o.type == "MESH" and o not in meshes:
            o.hide_render = True
    for o in meshes:
        for m in o.data.materials:
            nodes, links = m.node_tree.nodes, m.node_tree.links
            bsdf = next(n for n in nodes if n.type == "BSDF_PRINCIPLED")
            bsdf.inputs["Roughness"].default_value = 0.6
            if ao and o.data.uv_layers:
                tex = nodes.new("ShaderNodeTexImage")
                tex.image = ao
                mix = nodes.new("ShaderNodeMix")
                mix.data_type = "RGBA"
                mix.blend_type = "MULTIPLY"
                mix.inputs[0].default_value = 1.0
                mix.inputs[6].default_value = bsdf.inputs["Base Color"].default_value
                links.new(tex.outputs["Color"], mix.inputs[7])
                links.new(mix.outputs[2], bsdf.inputs["Base Color"])
    bpy.context.view_layer.update()

    corners = [o.matrix_world @ Vector(c) for o in meshes for c in o.bound_box]
    cen = sum(corners, Vector()) / len(corners)
    az, el = math.radians(AZ), math.radians(EL)
    d = Vector((math.cos(el) * math.sin(az), -math.cos(el) * math.cos(az), math.sin(el)))
    cam_data = bpy.data.cameras.new("cam")
    cam_data.type = "ORTHO"
    cam = bpy.data.objects.new("cam", cam_data)
    sc.collection.objects.link(cam)
    sc.camera = cam
    cam.rotation_euler = (-d).to_track_quat("-Z", "Y").to_euler()
    cam.location = cen + d * 10.0
    bpy.context.view_layer.update()
    inv = cam.matrix_world.inverted()
    pts = [inv @ c for c in corners]
    xs, ys = [p.x for p in pts], [p.y for p in pts]
    w, h = max(xs) - min(xs), max(ys) - min(ys)
    cam.location += cam.matrix_world.to_3x3() @ Vector(((max(xs) + min(xs)) / 2, (max(ys) + min(ys)) / 2, 0))
    cam_data.ortho_scale = max(w, h) / FILL
    cam_data.clip_start, cam_data.clip_end = 0.01, 100

    def light(name, rot, energy, color, angle):
        ld = bpy.data.lights.new(name, "SUN")
        ld.energy, ld.color, ld.angle = energy, color, angle
        lo = bpy.data.objects.new(name, ld)
        sc.collection.objects.link(lo)
        lo.rotation_euler = [math.radians(a) for a in rot]

    light("key", (50, 0, 20), 4.0, (1.0, 0.86, 0.68), math.radians(8))
    light("fill", (60, 0, 200), 1.2, (0.75, 0.82, 1.0), math.radians(30))
    world = bpy.data.worlds.new("w")
    world.use_nodes = True
    bg = next(n for n in world.node_tree.nodes if n.type == "BACKGROUND")
    bg.inputs["Color"].default_value = (0.55, 0.5, 0.45, 1)
    bg.inputs["Strength"].default_value = 0.6
    sc.world = world

    sc.render.engine = "BLENDER_EEVEE"
    sc.render.film_transparent = True
    sc.render.resolution_x = sc.render.resolution_y = SIZE * SS
    sc.render.resolution_percentage = 100
    sc.render.image_settings.file_format = "PNG"
    sc.render.image_settings.color_mode = "RGBA"
    sc.view_settings.view_transform = "Standard"
    sc.eevee.taa_render_samples = 64
    sc.render.filepath = str(pathlib.Path(raw_dir) / f"{pid}.png")
    bpy.ops.render.render(write_still=True)
    print(f"[thumbs] {pid}: {len(meshes)} lod0 mesh(es), ao={'yes' if ao else 'no'}")


# ---------------------------------------------------------------- orchestrator
def finish(pid):
    from PIL import Image

    im = Image.open(RAW / f"{pid}.png").convert("RGBA").resize((SIZE, SIZE), Image.LANCZOS)
    q = im.quantize(colors=COLORS, method=Image.Quantize.FASTOCTREE, dither=Image.Dither.NONE)
    q.save(OUT / f"{pid}.png", optimize=True)


def provenance():
    ver = subprocess.run([BLENDER, "--version"], capture_output=True, text=True).stdout.splitlines()[0]
    head = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True).stdout.strip()
    files = []
    for pid in piece_ids():
        glb, png = KIT / pid / f"{pid}.glb", OUT / f"{pid}.png"
        if not png.is_file():
            continue
        files.append({"id": pid, "source_glb": str(glb.relative_to(ROOT)), "source_glb_sha256": sha(glb),
                      "png": png.name, "png_sha256": sha(png), "bytes": png.stat().st_size})
    meta = {"generator": "blender/kit/thumbnails.py", "blender": ver, "git_head": head,
            "date": datetime.date.today().isoformat(),
            "render": {"engine": "BLENDER_EEVEE", "samples": 64, "supersample": SS, "size": SIZE, "fill": FILL,
                       "camera": f"orthographic, azimuth {AZ} deg, elevation {EL} deg, auto-framed to LOD0 bounds",
                       "lights": "warm sun key + cool sun fill + dim warm world", "view_transform": "Standard",
                       "background": "transparent", "material": "GLB wood_placeholder x baked LOD0 AO map",
                       "post": f"Pillow LANCZOS downsample, {COLORS}-colour FASTOCTREE quantise"},
            "files": files}
    (OUT / "provenance.json").write_text(json.dumps(meta, indent=2) + "\n")


def check(ids):
    meta = json.loads((OUT / "provenance.json").read_text())
    rec = {f["id"]: f for f in meta["files"]}
    bad = []
    for pid in ids:
        f, png, glb = rec.get(pid), OUT / f"{pid}.png", KIT / pid / f"{pid}.glb"
        if not f or not png.is_file():
            bad.append(f"{pid}: missing")
        elif sha(png) != f["png_sha256"]:
            bad.append(f"{pid}: png hash mismatch")
        elif sha(glb) != f["source_glb_sha256"]:
            bad.append(f"{pid}: source GLB changed since render")
    print("\n".join(bad) or f"thumbs ok: {len(ids)} pieces")
    return not bad


def sheet(ids):
    from PIL import Image, ImageDraw

    cols, cell, lab = 8, SIZE + 16, 18
    rows = -(-len(ids) // cols)
    img = Image.new("RGBA", (cols * cell, rows * (cell + lab)), "#22160f")
    d = ImageDraw.Draw(img)
    for i, pid in enumerate(ids):
        x, y = (i % cols) * cell + 8, (i // cols) * (cell + lab)
        img.alpha_composite(Image.open(OUT / f"{pid}.png").convert("RGBA"), (x, y + 4))
        d.text((x, y + SIZE + 6), pid, fill="#d9c2a0")
    dst = ROOT / "target" / "thumbs-sheet.png"
    dst.parent.mkdir(exist_ok=True)
    img.save(dst)
    print(dst)


def main():
    args = sys.argv[1:]
    ids = [a for a in args if not a.startswith("--")] or piece_ids()
    if "--check" in args:
        sys.exit(0 if check(piece_ids()) else 1)
    if "--sheet" in args:
        return sheet(piece_ids())
    RAW.mkdir(parents=True, exist_ok=True)
    OUT.mkdir(parents=True, exist_ok=True)
    subprocess.run([BLENDER, "-b", "--factory-startup", "--python-exit-code", "1", "-P", __file__, "--",
                    str(RAW), *ids], check=True)
    for pid in ids:
        finish(pid)
    provenance()
    if len(list(OUT.glob("*.png"))) == len(piece_ids()):
        sheet(piece_ids())


if bpy is not None:
    _a = sys.argv[sys.argv.index("--") + 1:]
    for _pid in _a[1:]:
        render_piece(_pid, _a[0])
elif __name__ == "__main__":
    main()
