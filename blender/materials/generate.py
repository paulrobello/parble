"""P1-T1 tileable wood set: procedural Blender nodes -> Cycles EMIT bakes -> numpy-derived PBR maps.

Run headless (see `make materials-build`):
    Blender -b --factory-startup --python-exit-code 1 -P blender/materials/generate.py -- <stage_dir> <runtime_dir>

Every texture is a function of the UV torus (cos/sin of 2*pi*u and 2*pi*v fed to 4D noise), and every
ring count is an integer per tile, so all maps wrap exactly. Wave/Magic/3D noise are not used: they do
not wrap. Normal maps are OpenGL convention (+Y = +V = image up), as glTF and Bevy expect.
ORM packs AO in R, roughness in G and metallic in B (glTF metallicRoughness + occlusion).
"""

from __future__ import annotations

import hashlib
import json
import math
import os
import sys
import time

import bpy
import numpy as np

TILE_M = 0.25  # kit-contract.md §12: one repeat = 250 mm
BAKE_PX = 2048  # Ultra: 8,192 texels/m
RUNTIME_PX = 1024  # High: 4,096 texels/m. Low (2,048 texels/m) is mip 1 of the High file.
SAMPLES = 16

# Linear-light colours. Rich warm wood calibrated to the approved look-dev frames (walnut-leaning), owner
# ruling 2026-11-07: hue from assets-source/lookdev 01/04/06/07 wood crops (light-compensated against the
# review render), darker than the old birch set. a = look-dev tone, b = darker walnut heartwood, c = lighter warm.
VARIANTS: dict[str, dict] = {
    "wood_warm_a": {
        "seed": 11,
        "rings": 18,  # latewood lines per 250 mm tile (integer -> periodic)
        "warp": 6.5,  # large-scale swirl amplitude, in ring periods
        "warpScale": 0.85,
        "warp2": 0.55,
        "warp2Scale": 2.4,
        "light": [0.48, 0.295, 0.115],
        "dark": [0.22, 0.118, 0.042],
        "lineSharpness": 2.5,
        "lineContrast": 0.70,
        "toneVariation": 0.12,
        "pores": 0.30,  # albedo darkening inside pores
        "roughness": [0.38, 0.48],
    },
    "wood_walnut_b": {
        "seed": 23,
        "rings": 14,
        "warp": 9.0,
        "warpScale": 0.8,
        "warp2": 0.8,
        "warp2Scale": 2.0,
        "light": [0.21, 0.106, 0.052],
        "dark": [0.066, 0.031, 0.015],
        "lineSharpness": 2.0,
        "lineContrast": 0.78,
        "toneVariation": 0.14,
        "pores": 0.40,
        "roughness": [0.36, 0.46],
    },
    "wood_warm_c": {
        "seed": 37,
        "rings": 22,
        "warp": 5.0,
        "warpScale": 0.95,
        "warp2": 0.45,
        "warp2Scale": 2.8,
        "light": [0.62, 0.40, 0.165],
        "dark": [0.33, 0.19, 0.07],
        "lineSharpness": 3.0,
        "lineContrast": 0.62,
        "toneVariation": 0.10,
        "pores": 0.25,
        "roughness": [0.40, 0.50],
    },
}
HEIGHT_AMPLITUDE_M = 0.00005  # 50 um peak-to-peak grain and pore relief under a satin finish
AO_STRENGTH = 0.10  # faint latewood cavity term; per-piece AO is baked separately (P1-T2, §12)


def die(msg: str) -> None:
    print(f"generate.py: ERROR {msg}", file=sys.stderr)
    sys.exit(1)


def args() -> tuple[str, str]:
    a = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    if len(a) != 2:
        die("usage: -- <stage_dir> <runtime_dir>")
    return os.path.abspath(a[0]), os.path.abspath(a[1])


def sha256(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        h.update(f.read())
    return h.hexdigest()


def seed_offsets(seed: int, n: int) -> list[float]:
    rng = np.random.default_rng(seed)
    return [float(x) for x in rng.uniform(-50.0, 50.0, n)]


class Graph:
    """Small helper for building math-node graphs."""

    def __init__(self, mat: bpy.types.Material) -> None:
        self.nt = mat.node_tree
        self.nt.nodes.clear()
        self.x = 0

    def node(self, kind: str, **props) -> bpy.types.Node:
        n = self.nt.nodes.new(kind)
        for k, v in props.items():
            setattr(n, k, v)
        n.location = (self.x, 0)
        self.x += 40
        return n

    def link(self, src, dst) -> None:
        self.nt.links.new(src, dst)

    def math(self, op: str, a, b=None) -> bpy.types.NodeSocket:
        n = self.node("ShaderNodeMath", operation=op)
        for i, v in enumerate((a, b)):
            if v is None:
                continue
            if isinstance(v, (int, float)):
                n.inputs[i].default_value = float(v)
            else:
                self.link(v, n.inputs[i])
        return n.outputs[0]

    def combine(self, x, y, z) -> bpy.types.NodeSocket:
        n = self.node("ShaderNodeCombineXYZ")
        for i, v in enumerate((x, y, z)):
            if isinstance(v, (int, float)):
                n.inputs[i].default_value = float(v)
            else:
                self.link(v, n.inputs[i])
        return n.outputs[0]

    def noise4d(self, vec, w, scale: float, detail: float, rough: float = 0.5) -> bpy.types.NodeSocket:
        n = self.node("ShaderNodeTexNoise", noise_dimensions="4D")
        self.link(vec, n.inputs["Vector"])
        self.link(w, n.inputs["W"])
        n.inputs["Scale"].default_value = scale
        n.inputs["Detail"].default_value = detail
        n.inputs["Roughness"].default_value = rough
        return n.outputs["Fac"]

    def torus(self, u2, v2, ru: float, rv: float, off: list[float]):
        """Torus point for UV angles u2, v2 (radians). Offsets translate the torus, keeping it periodic."""
        x = self.math("ADD", self.math("MULTIPLY", self.math("COSINE", u2), ru), off[0])
        y = self.math("ADD", self.math("MULTIPLY", self.math("SINE", u2), ru), off[1])
        z = self.math("ADD", self.math("MULTIPLY", self.math("COSINE", v2), rv), off[2])
        w = self.math("ADD", self.math("MULTIPLY", self.math("SINE", v2), rv), off[3])
        return self.combine(x, y, z), w


def build_material(name: str, p: dict) -> tuple[bpy.types.Material, dict]:
    """Returns the material and the emission sockets {albedo, data} to bake."""
    mat = bpy.data.materials.new(f"PBM_{name}")
    mat.use_fake_user = True
    g = Graph(mat)
    tc = g.node("ShaderNodeTexCoord")
    sep = g.node("ShaderNodeSeparateXYZ")
    g.link(tc.outputs["UV"], sep.inputs[0])
    tau = 2.0 * math.pi
    u2 = g.math("MULTIPLY", sep.outputs[0], tau)
    v2 = g.math("MULTIPLY", sep.outputs[1], tau)
    off = seed_offsets(p["seed"], 24)

    # Grain runs along U. Warp fields: broad swirl plus a finer wobble.
    pa, wa = g.torus(u2, v2, 1.0, 1.0, off[0:4])
    warp_a = g.noise4d(pa, wa, p["warpScale"], 2.0, 0.45)
    pb, wb = g.torus(u2, v2, 1.0, 1.0, off[4:8])
    warp_b = g.noise4d(pb, wb, p["warp2Scale"], 3.0, 0.5)
    # phase = rings*v + warp*(nA-0.5) + warp2*(nB-0.5): integer rings keep it periodic in v.
    phase = g.math("MULTIPLY", sep.outputs[1], float(p["rings"]))
    phase = g.math("ADD", phase, g.math("MULTIPLY", g.math("SUBTRACT", warp_a, 0.5), p["warp"] * 2.0))
    phase = g.math("ADD", phase, g.math("MULTIPLY", g.math("SUBTRACT", warp_b, 0.5), p["warp2"] * 2.0))
    # Thin dark latewood lines: ((1 - sin(2*pi*phase))/2)^sharpness.
    s = g.math("SINE", g.math("MULTIPLY", phase, tau))
    line = g.math("POWER", g.math("MULTIPLY", g.math("SUBTRACT", 1.0, s), 0.5), p["lineSharpness"])
    # Soft earlywood band around each line widens the figure.
    band = g.math("MULTIPLY", g.math("SUBTRACT", 1.0, s), 0.5)
    late = g.math("MINIMUM", g.math("ADD", g.math("MULTIPLY", line, 0.8), g.math("MULTIPLY", band, 0.25)), 1.0)
    # Lines fade in and out along the figure, as on a rotary-cut veneer.
    pf, wf = g.torus(u2, v2, 1.0, 1.0, off[16:20])
    fade = g.noise4d(pf, wf, 1.3, 2.0, 0.5)
    fade = g.math("MAXIMUM", g.math("MINIMUM", g.math("MULTIPLY", g.math("SUBTRACT", fade, 0.3), 2.5), 1.0), 0.25)
    late = g.math("MULTIPLY", late, fade)

    # Fine fibre streaks: stretched along U (small U radius, large V radius).
    pc, wc = g.torus(u2, v2, 0.6, 9.0, off[8:12])
    streak = g.noise4d(pc, wc, 3.0, 4.0, 0.6)
    # Broad tone variation (sheet-to-sheet colour drift).
    pd, wd = g.torus(u2, v2, 1.0, 1.0, off[12:16])
    tone = g.noise4d(pd, wd, 0.9, 1.0, 0.4)

    # Open pores: short dashes along U, about 3 bake px across V (slightly exaggerated so mip 1 keeps them).
    pp, wp = g.torus(u2, v2, 6.0, 40.0, off[20:24])
    pore = g.noise4d(pp, wp, 3.0, 1.0, 0.5)
    pore = g.math("MAXIMUM", g.math("MINIMUM", g.math("MULTIPLY", g.math("SUBTRACT", pore, 0.60), 6.0), 1.0), 0.0)

    t = g.math("MULTIPLY", late, p["lineContrast"])
    t = g.math("ADD", t, g.math("MULTIPLY", g.math("SUBTRACT", streak, 0.5), 0.22))
    t = g.math("ADD", t, g.math("MULTIPLY", pore, p["pores"]))
    t = g.math("MAXIMUM", g.math("MINIMUM", t, 1.0), 0.0)
    shade = g.math("ADD", 1.0, g.math("MULTIPLY", g.math("SUBTRACT", tone, 0.5), p["toneVariation"] * 2.0))

    mix = g.node("ShaderNodeMix", data_type="RGBA", blend_type="MIX")
    g.link(t, mix.inputs["Factor"])
    mix.inputs[6].default_value = (*p["light"], 1.0)
    mix.inputs[7].default_value = (*p["dark"], 1.0)
    scale = g.node("ShaderNodeVectorMath", operation="SCALE")
    g.link(mix.outputs[2], scale.inputs[0])
    g.link(shade, scale.inputs["Scale"])
    albedo = scale.outputs[0]

    # Data: R height (0..1, latewood recessed, fibres raised), G roughness, B latewood mask.
    h = g.math("SUBTRACT", 1.0, g.math("MULTIPLY", late, 0.7))
    h = g.math("ADD", g.math("MULTIPLY", h, 0.8), g.math("MULTIPLY", streak, 0.2))
    h = g.math("SUBTRACT", h, g.math("MULTIPLY", pore, 0.25))
    r0, r1 = p["roughness"]
    rough = g.math("ADD", r0, g.math("MULTIPLY", g.math("ADD", g.math("MULTIPLY", late, 0.7), g.math("MULTIPLY", streak, 0.3)), r1 - r0))
    rough = g.math("ADD", rough, g.math("MULTIPLY", pore, 0.08))
    data = g.combine(h, rough, late)

    emit = g.node("ShaderNodeEmission")
    emit.inputs["Strength"].default_value = 1.0
    out = g.node("ShaderNodeOutputMaterial")
    g.link(emit.outputs[0], out.inputs["Surface"])
    return mat, {"albedo": albedo, "data": data, "emit": emit}


def setup_scene() -> bpy.types.Object:
    for o in list(bpy.data.objects):
        bpy.data.objects.remove(o, do_unlink=True)
    sc = bpy.context.scene
    sc.name = "PBM_generator"
    sc.render.engine = "CYCLES"
    device = os.environ.get("PARBLE_BAKE_DEVICE", "GPU")
    if device == "GPU":
        prefs = bpy.context.preferences.addons["cycles"].preferences
        prefs.compute_device_type = "METAL"
        prefs.refresh_devices()
        gpus = [d for d in prefs.devices if d.type == "METAL"]
        if not gpus:
            die("no Metal device; set PARBLE_BAKE_DEVICE=CPU explicitly to bake on CPU")
        for d in prefs.devices:
            d.use = d.type == "METAL"
        sc.cycles.device = "GPU"
    else:
        sc.cycles.device = "CPU"
    sc.cycles.samples = SAMPLES
    sc.cycles.seed = 0
    sc.cycles.use_denoising = False
    bpy.ops.mesh.primitive_plane_add(size=1.0)
    plane = bpy.context.active_object
    plane.name = "PBM_bake_plane"
    return plane


def bake(plane: bpy.types.Object, mat: bpy.types.Material, socks: dict, which: str, name: str) -> np.ndarray:
    img = bpy.data.images.new(f"PBM_{name}_{which}", BAKE_PX, BAKE_PX, alpha=False, float_buffer=True)
    img.colorspace_settings.name = "Non-Color"
    nt = mat.node_tree
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = img
    nt.nodes.active = tex
    for l in list(socks["emit"].inputs["Color"].links):
        nt.links.remove(l)
    nt.links.new(socks[which], socks["emit"].inputs["Color"])
    plane.data.materials.clear()
    plane.data.materials.append(mat)
    bpy.ops.object.select_all(action="DESELECT")
    plane.select_set(True)
    bpy.context.view_layer.objects.active = plane
    bpy.ops.object.bake(type="EMIT", margin=0, use_clear=True)
    px = np.empty(BAKE_PX * BAKE_PX * 4, dtype=np.float32)
    img.pixels.foreach_get(px)
    nt.nodes.remove(tex)
    # Row 0 is v = 0 (Blender stores images bottom-up).
    return px.reshape(BAKE_PX, BAKE_PX, 4)[:, :, :3].astype(np.float64)


def to_srgb(lin: np.ndarray) -> np.ndarray:
    lin = np.clip(lin, 0.0, 1.0)
    return np.where(lin <= 0.0031308, lin * 12.92, 1.055 * np.power(lin, 1.0 / 2.4) - 0.055)


def down2(a: np.ndarray) -> np.ndarray:
    """Periodic 2x2 box average (pixel-aligned, so no boundary handling is needed)."""
    n = a.shape[0] // 2
    return a.reshape(n, 2, n, 2, -1).mean(axis=(1, 3))


def normal_from_height(h_m: np.ndarray, px_m: float) -> np.ndarray:
    gx = (np.roll(h_m, -1, axis=1) - np.roll(h_m, 1, axis=1)) / (2.0 * px_m)
    gy = (np.roll(h_m, -1, axis=0) - np.roll(h_m, 1, axis=0)) / (2.0 * px_m)  # +y = +v (row 0 is v=0)
    n = np.stack([-gx, -gy, np.ones_like(h_m)], axis=-1)
    return n / np.linalg.norm(n, axis=-1, keepdims=True)


def renorm(n: np.ndarray) -> np.ndarray:
    return n / np.linalg.norm(n, axis=-1, keepdims=True)


def write_png(path: str, rgb01: np.ndarray, name: str) -> dict:
    """Writes 8-bit RGBA PNG; values are quantized here so Blender's float->byte step is exact."""
    hgt, wid = rgb01.shape[:2]
    q = np.round(np.clip(rgb01, 0.0, 1.0) * 255.0) / 255.0
    rgba = np.concatenate([q, np.ones((hgt, wid, 1))], axis=-1).astype(np.float32)
    img = bpy.data.images.new(f"PBM_out_{name}", wid, hgt, alpha=True, float_buffer=False)
    img.colorspace_settings.name = "Non-Color"
    img.pixels.foreach_set(rgba.ravel())
    img.filepath_raw = path
    img.file_format = "PNG"
    sc = bpy.context.scene
    sc.render.image_settings.color_depth = "8"
    sc.render.image_settings.compression = 90
    img.save()
    bpy.data.images.remove(img)
    return {"path": path, "size": wid, "sha256": sha256(path), "bytes": os.path.getsize(path)}


def main() -> None:
    stage, runtime = args()
    os.makedirs(stage, exist_ok=True)
    t0 = time.time()
    plane = setup_scene()
    report: dict = {
        "blender": bpy.app.version_string,
        "blenderHash": bpy.app.build_hash.decode() if isinstance(bpy.app.build_hash, bytes) else str(bpy.app.build_hash),
        "bake": {
            "type": "EMIT",
            "device": bpy.context.scene.cycles.device,
            "samples": SAMPLES,
            "seed": 0,
            "marginPx": 0,
            "sizePx": BAKE_PX,
        },
        "tileM": TILE_M,
        "runtimePx": RUNTIME_PX,
        "texelDensityPxPerM": {"ultra": BAKE_PX / TILE_M, "high": RUNTIME_PX / TILE_M, "low": RUNTIME_PX / 2 / TILE_M},
        "heightAmplitudeM": HEIGHT_AMPLITUDE_M,
        "aoStrength": AO_STRENGTH,
        "normalConvention": "OpenGL (+Y = +V = image up), tangent space, glTF/Bevy",
        "ormPacking": "R=ambient occlusion, G=roughness, B=metallic (0, dielectric)",
        "variants": {},
    }
    for name, p in VARIANTS.items():
        tv = time.time()
        mat, socks = build_material(name, p)
        lin = bake(plane, mat, socks, "albedo", name)
        data = bake(plane, mat, socks, "data", name)
        hraw, rough, late = data[..., 0], data[..., 1], data[..., 2]
        h_m = (hraw - hraw.mean()) * HEIGHT_AMPLITUDE_M
        ao = 1.0 - AO_STRENGTH * np.clip(late, 0.0, 1.0)
        nrm = normal_from_height(h_m, TILE_M / BAKE_PX)
        orm = np.stack([ao, rough, np.zeros_like(ao)], axis=-1)

        vstage = os.path.join(stage, name)
        vrun = os.path.join(runtime, name)
        os.makedirs(vstage, exist_ok=True)
        os.makedirs(vrun, exist_ok=True)
        files: dict = {"stage": {}, "runtime": {}}
        files["stage"]["base_color"] = write_png(os.path.join(vstage, f"{name}.base_color.png"), to_srgb(lin), f"{name}_bc_hi")
        files["stage"]["normal"] = write_png(os.path.join(vstage, f"{name}.normal.png"), nrm * 0.5 + 0.5, f"{name}_n_hi")
        files["stage"]["orm"] = write_png(os.path.join(vstage, f"{name}.orm.png"), orm, f"{name}_orm_hi")
        files["stage"]["height"] = write_png(os.path.join(vstage, f"{name}.height.png"), np.repeat(np.clip(hraw, 0, 1)[..., None], 3, -1), f"{name}_h_hi")
        files["runtime"]["base_color"] = write_png(os.path.join(vrun, f"{name}.base_color.png"), to_srgb(down2(lin)), f"{name}_bc")
        files["runtime"]["normal"] = write_png(os.path.join(vrun, f"{name}.normal.png"), renorm(down2(nrm)) * 0.5 + 0.5, f"{name}_n")
        files["runtime"]["orm"] = write_png(os.path.join(vrun, f"{name}.orm.png"), down2(orm), f"{name}_orm")
        stats = {
            "albedoLinearMean": [round(float(x), 4) for x in lin.reshape(-1, 3).mean(0)],
            "roughness": [round(float(rough.min()), 4), round(float(rough.mean()), 4), round(float(rough.max()), 4)],
            "ao": [round(float(ao.min()), 4), round(float(ao.mean()), 4)],
            "normalMaxTiltDeg": round(float(np.degrees(np.arccos(nrm[..., 2].min()))), 3),
        }
        for kind in files.values():
            for f in kind.values():
                f["path"] = os.path.relpath(f["path"], os.getcwd())
        report["variants"][name] = {"params": p, "files": files, "stats": stats, "seconds": round(time.time() - tv, 1)}
        print(f"generate.py: {name} {stats} {time.time() - tv:.1f}s")

    # Keep the editable source: node graphs, without the baked buffers.
    for img in list(bpy.data.images):
        bpy.data.images.remove(img)
    blend = os.path.join(os.path.dirname(os.path.abspath(__file__)), "wood_materials.blend")
    bpy.context.preferences.filepaths.save_version = 0  # no .blend1 backup beside the source
    bpy.ops.wm.save_as_mainfile(filepath=blend, compress=True)
    report["blend"] = {"path": os.path.relpath(blend, os.getcwd()), "note": "node source; rebuilt by every run"}
    report["seconds"] = round(time.time() - t0, 1)
    with open(os.path.join(stage, "bake-report.json"), "w") as f:
        json.dump(report, f, indent=2)
    print(f"generate.py: done in {report['seconds']}s")


main()
