"""parble block-family kit geometry, built against docs/kit-contract.md (KIT_CONTRACT_VERSION 1).

Importable by the headless generator (generate.py) and by a live Blender session (live_showcase.py).
Nothing here resets, purges, bakes or exports.

Frames: the contract frame is millimetres, +Y up, piece-local origin at the centre of the anchor
cell's bottom face (contract §1). Blender is metres, +Z up: contract (x, y, z) -> Blender
(x, -z, y) * 0.001, a proper rotation, so triangle winding is preserved. glTF export (Y-up) maps
it back to the contract frame in metres.

The collision mesh and the sidecar are pure-Python integers (µm), never read back from Blender,
so the canonical port ring (§5.4) is bit-exact.
"""
import math
import re

KIT_CONTRACT_VERSION = 1
SIDECAR_SCHEMA = 1
PIECE_VERSION = 1

CELL = 60.0          # mm (§2)
INSET = 0.5          # visual seam inset per face, render mesh only (§2)
R = 14.0             # channel half-round radius (§5.1)
LIP_FILLET = 3.0     # block lip fillet, render only (§5.2)
BEVEL = 1.5          # outer edge bevel, render only (§5.2)
LEVEL_STEP = 15.0    # port level step (§3)
END_AXIS_X = 6.0     # blk_end_*: the channel ends in a half-turn about the vertical axis x = 6 mm
EXT = 6.0            # render cutter runs this far past the cell faces
CUT_ABOVE = 4.0      # render cutter rises this far above the rendered top face

ID_RE = re.compile(r"^(blk|rail|mech|terr|music|deco)_[a-z0-9]+(_[a-z0-9]+)*$")

# The four P1-T2 pieces. Every 1-cell level variant below is buildable by id; only these ship now.
BUILD = ("blk_straight_l3", "blk_curve90_l3", "blk_slope_gentle_l3", "blk_end_l3")

# Render LODs (§12: half-round segments 32 / 12 / 6). "path" is segments along the centreline.
LODS = {
    0: {"n_round": 32, "n_fillet": 6, "bevel_segs": 3, "path": {"straight": 1, "slope_gentle": 16, "curve90": 20, "end": 16}},
    1: {"n_round": 12, "n_fillet": 2, "bevel_segs": 1, "path": {"straight": 1, "slope_gentle": 6, "curve90": 8, "end": 8}},
    2: {"n_round": 6, "n_fillet": 1, "bevel_segs": 0, "path": {"straight": 1, "slope_gentle": 3, "curve90": 4, "end": 4}},
}
HIGH = {"n_round": 64, "n_fillet": 12, "bevel_segs": 8, "path": {"straight": 1, "slope_gentle": 32, "curve90": 40, "end": 32}}
COLLISION_PATH = {"straight": 1, "slope_gentle": 8, "curve90": 8, "end": 8}

BUDGET_1CELL_BLOCK = {0: 3000, 1: 1000, 2: 250}   # §12
COLLISION_TRIS_PER_CELL = 600                      # §12


def round_half_away(x):
    """Integer rounding, half away from zero (§5.4). Python's round() is half-to-even."""
    return int(math.copysign(math.floor(abs(x) + 0.5), x))


def canonical_ring(floor_um, top_um=60000):
    """§5.4 port ring as 19 (u, v) µm pairs: left wall top, 17 half-round vertices (θ = -90..+90 in
    11.25° steps), right wall top."""
    pts = [(-14000, top_um)]
    for i in range(17):
        th = math.radians(-90.0 + 11.25 * i)
        pts.append((round_half_away(14000.0 * math.sin(th)),
                    floor_um + round_half_away(14000.0 - 14000.0 * math.cos(th))))
    pts.append((14000, top_um))
    return pts


# ---------------------------------------------------------------------------------------------
# Piece specs
# ---------------------------------------------------------------------------------------------

def spec(piece_id):
    """Ports, shape and level for a 1-cell block piece id (§7, §10)."""
    if not ID_RE.match(piece_id) or len(piece_id) > 48:
        raise ValueError(f"{piece_id}: not a §10 id")
    m = re.match(r"^blk_(straight|curve90|slope_gentle|end)_l([0-3])$", piece_id)
    if not m:
        raise ValueError(f"{piece_id}: not a 1-cell block piece this generator builds")
    shape, level = m.group(1), int(m.group(2))
    legal = {"straight": (1, 2, 3), "curve90": (1, 2, 3), "slope_gentle": (2, 3), "end": (1, 2, 3)}[shape]
    if level not in legal:
        raise ValueError(f"{piece_id}: level {level} is not a 1-cell variant of {shape} (§5.2, §7)")
    port = lambda i, face, lvl, flow: {"cell": [0, 0, 0], "class": "channel", "face": face, "flow": flow,
                                       "index": i, "inset_um": 0, "kind": "side", "level": lvl}
    if shape == "straight":
        ports = [port(0, "-x", level, "both"), port(1, "+x", level, "both")]
    elif shape == "curve90":
        ports = [port(0, "-x", level, "both"), port(1, "-z", level, "both")]
    elif shape == "slope_gentle":
        ports = [port(0, "-x", level, "in"), port(1, "+x", level - 1, "out")]
    else:
        ports = [port(0, "-x", level, "in")]
    return {"id": piece_id, "shape": shape, "level": level, "ports": ports,
            "chiral": shape == "curve90", "family": "blk", "footprint": [[0, 0, 0]]}


def floor_mm(shape, level, t):
    """Channel floor height at path parameter t in [0, 1]. Gentle slope: cubic smoothstep (§7)."""
    f0 = LEVEL_STEP * level
    if shape == "slope_gentle":
        return f0 - LEVEL_STEP * (3 * t * t - 2 * t ** 3)
    return f0


def stations(shape, level, n, inset_stations=False):
    """Centreline frames from port 0 inward: (cx, cz, lx, lz, floor) in mm. (lx, lz) is the lateral
    (+u) unit vector in plan. For "end" the path stops at the cap axis (x = END_AXIS_X).
    inset_stations adds frames exactly at the rendered faces (|x| or |z| = 29.5) on the curve and the
    slope, so the render floor at a port is not a chord interpolation."""
    out = []
    if shape in ("straight", "slope_gentle"):
        ts = [k / n for k in range(n + 1)]
        if inset_stations and shape == "slope_gentle":
            ts = sorted(set(ts) | {INSET / CELL, 1.0 - INSET / CELL})
        for t in ts:
            out.append((-30.0 + 60.0 * t, 0.0, 0.0, 1.0, floor_mm(shape, level, t)))
    elif shape == "end":  # one straight segment; n is the cap's segment count, used by the caller
        for k in range(2):
            t = float(k)
            out.append((-30.0 + (END_AXIS_X + 30.0) * t, 0.0, 0.0, 1.0, floor_mm(shape, level, t)))
    elif shape == "curve90":
        # Arc of radius 30 about the corner (-30, -30): phi 90° at the -x port, 0° at the -z port.
        phis = [90.0 - 90.0 * k / n for k in range(n + 1)]
        if inset_stations:
            a = math.degrees(math.asin(INSET / 30.0))
            phis = sorted(set(phis) | {90.0 - a, a}, reverse=True)
        for p in phis:
            r = math.radians(p)
            c, s = math.cos(r), math.sin(r)
            out.append((-30.0 + 30.0 * c, -30.0 + 30.0 * s, c, s, floor_mm(shape, level, 0.0)))
    return out


# ---------------------------------------------------------------------------------------------
# Collision mesh (integer µm, contract frame)
# ---------------------------------------------------------------------------------------------

class _Pool:
    def __init__(self):
        self.verts, self.index, self.tris = [], {}, []

    def v(self, p):
        p = tuple(int(c) for c in p)
        if p not in self.index:
            self.index[p] = len(self.verts)
            self.verts.append(p)
        return self.index[p]

    def tri(self, a, b, c, want):
        """Add triangle (a, b, c), wound so its normal points along `want` (a direction)."""
        ia, ib, ic = self.v(a), self.v(b), self.v(c)
        if len({ia, ib, ic}) < 3:
            return
        pa, pb, pc = self.verts[ia], self.verts[ib], self.verts[ic]
        e1 = [pb[i] - pa[i] for i in range(3)]
        e2 = [pc[i] - pa[i] for i in range(3)]
        n = (e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0])
        if n == (0, 0, 0):
            return
        if sum(n[i] * want[i] for i in range(3)) < 0:
            ib, ic = ic, ib
        self.tris.append((ia, ib, ic))

    def quad(self, a, b, c, d, want):
        self.tri(a, b, c, want)
        self.tri(a, c, d, want)


def _world_um(st, u_um, v_um):
    cx, cz, lx, lz, _ = st
    return (round_half_away(cx * 1000.0 + u_um * lx), v_um, round_half_away(cz * 1000.0 + u_um * lz))


def _fan(pool, apex, poly, want):
    for a, b in zip(poly, poly[1:]):
        pool.tri(apex, a, b, want)


def collision_mesh(piece_id):
    """(vertices [(x, y, z) µm], triangles [(i, j, k)]) for one piece: channel with the canonical
    ring at every station, wall tops at 60 mm, top faces, non-port outer sides and the bottom.
    Port faces are left open (they mate with a neighbour)."""
    sp = spec(piece_id)
    shape, level = sp["shape"], sp["level"]
    pool = _Pool()
    sts = stations(shape, level, COLLISION_PATH[shape])
    rings = []
    for st in sts:
        f_um = round_half_away(st[4] * 1000.0)
        rings.append([_world_um(st, u, v) for u, v in canonical_ring(f_um)])
    for s in range(len(sts) - 1):
        a, b = rings[s], rings[s + 1]
        mid = sts[s]
        axis = (round_half_away(mid[0] * 1000), round_half_away(mid[4] * 1000) + 14000, round_half_away(mid[1] * 1000))
        for j in range(18):
            cen = [(a[j][i] + a[j + 1][i] + b[j][i] + b[j + 1][i]) / 4 for i in range(3)]
            want = [axis[i] - cen[i] for i in range(3)]
            pool.quad(a[j], a[j + 1], b[j + 1], b[j], want)

    UP, DOWN = (0, 1, 0), (0, -1, 0)
    T = 60000
    sq = lambda x, z: (x, T, z)
    if shape in ("straight", "slope_gentle"):
        for sgn, j in ((1, 18), (-1, 0)):
            inner = [r[j] for r in rings]
            _fan(pool, sq(-30000, 30000 * sgn), inner + [sq(30000, 30000 * sgn)], UP)
        sides = [("+z",), ("-z",)]
    elif shape == "curve90":
        inner = [r[0] for r in rings]   # u = -14 -> radius 16 about the corner
        outer = [r[18] for r in rings]  # u = +14 -> radius 44
        _fan(pool, sq(-30000, -30000), inner, UP)
        _fan(pool, sq(30000, 30000), [sq(-30000, 30000)] + outer + [sq(30000, -30000)], UP)
        sides = [("+x",), ("+z",)]
    else:  # end
        xe = round_half_away(END_AXIS_X * 1000)
        k_cap = COLLISION_PATH["end"]
        cap = []  # cap[k] = half ring (θ = 0..90 and the wall top) rotated to ψ_k
        half = canonical_ring(round_half_away(LEVEL_STEP * level * 1000))[9:]
        for k in range(k_cap + 1):
            psi = math.radians(180.0 * k / k_cap)
            st = (END_AXIS_X, 0.0, math.sin(psi), math.cos(psi), 0.0)
            cap.append([_world_um(st, u, v) for u, v in half])
        cap[0] = rings[-1][9:]
        cap[-1] = [rings[-1][9 - j] for j in range(10)]
        axis = (xe, round_half_away(LEVEL_STEP * level * 1000) + 14000, 0)
        for k in range(k_cap):
            a, b = cap[k], cap[k + 1]
            for j in range(9):
                cen = [(a[j][i] + a[j + 1][i] + b[j][i] + b[j + 1][i]) / 4 for i in range(3)]
                pool.quad(a[j], a[j + 1], b[j + 1], b[j], [axis[i] - cen[i] for i in range(3)])
        cap_tops = [c[9] for c in cap]  # wall tops, from +z round to -z
        half_k = k_cap // 2
        _fan(pool, sq(-30000, 30000), [r[18] for r in rings] + [sq(xe, 30000)], UP)
        _fan(pool, sq(30000, 30000), [sq(xe, 30000)] + cap_tops[:half_k + 1], UP)
        pool.tri(sq(30000, 30000), cap_tops[half_k], sq(30000, -30000), UP)
        _fan(pool, sq(30000, -30000), cap_tops[half_k:] + [sq(xe, -30000)], UP)
        _fan(pool, sq(-30000, -30000), [r[0] for r in rings] + [sq(xe, -30000)], UP)
        sides = [("+x",), ("+z", xe), ("-z", xe)]

    for side in sides:
        face, extra = side[0], side[1:]
        axis_i = 0 if face[1] == "x" else 2
        sgn = 1 if face[0] == "+" else -1
        want = [0, 0, 0]
        want[axis_i] = sgn
        other = 2 if axis_i == 0 else 0

        def P(o, y):
            p = [0, y, 0]
            p[axis_i] = 30000 * sgn
            p[other] = o
            return tuple(p)

        top = [P(-30000, T)] + [P(e, T) for e in extra] + [P(30000, T)]
        _fan(pool, P(-30000, 0), [P(30000, 0)] + top[::-1], want)
    pool.quad((-30000, 0, -30000), (30000, 0, -30000), (30000, 0, 30000), (-30000, 0, 30000), DOWN)
    return pool.verts, pool.tris


def sidecar(piece_id):
    """The §11 sidecar as a dict (integers only)."""
    sp = spec(piece_id)
    verts, tris = collision_mesh(piece_id)
    return {
        "animated_parts": [],
        "chiral": sp["chiral"],
        "collision": [{"indices": [i for t in tris for i in t], "surface": "wood",
                       "vertices_um": [c for v in verts for c in v]}],
        "family": sp["family"],
        "footprint": sp["footprint"],
        "id": piece_id,
        "kit_contract": KIT_CONTRACT_VERSION,
        "piece_version": PIECE_VERSION,
        "ports": sp["ports"],
        "schema": SIDECAR_SCHEMA,
        "sensors": [],
    }


def sidecar_bytes(piece_id):
    """Canonical bytes: UTF-8, LF, 2-space indent, sorted keys, trailing newline (§11)."""
    import json
    return (json.dumps(sidecar(piece_id), indent=2, sort_keys=True, ensure_ascii=True) + "\n").encode()


def port_point_mm(port):
    """Port floor point on the cell face, contract frame mm, and the outward normal."""
    f = port["face"]
    n = {"+x": (1, 0, 0), "-x": (-1, 0, 0), "+z": (0, 0, 1), "-z": (0, 0, -1)}[f]
    return (30.0 * n[0], LEVEL_STEP * port["level"], 30.0 * n[2]), n


# ---------------------------------------------------------------------------------------------
# Render mesh (Blender)
# ---------------------------------------------------------------------------------------------

def render_half_profile(floor, n_round, n_fillet, top=CELL - INSET):
    """Right half (u >= 0) of the render cutter section, (u, v) mm, from the top axis point down to
    the floor point (0, floor). The R14 half-round meets the top face through the 3 mm lip fillet;
    where the half-round reaches above top - 3 mm (L3) the fillet blends into the round (§5.2)."""
    cy = floor + R
    yf = top - LIP_FILLET
    if yf >= cy:
        fu, th_max, a0 = R + LIP_FILLET, math.pi / 2, math.pi
    else:
        dy = cy - yf
        fu = math.sqrt((R + LIP_FILLET) ** 2 - dy * dy)
        th_max = math.atan2(fu, dy)
        a0 = math.atan2(dy, -fu)
    pts = [(0.0, top + CUT_ABOVE), (fu, top + CUT_ABOVE), (fu, top)]
    # The blend case has no wall segment, so it gets one more fillet segment: every station of a
    # slope (blend at L3, wall at L2) then has the same vertex count.
    nf = n_fillet if yf >= cy else n_fillet + 1
    for i in range(1, nf + 1):  # fillet from the top face (α = 90°) down to α0
        a = math.pi / 2 + (a0 - math.pi / 2) * i / nf
        pts.append((fu + LIP_FILLET * math.cos(a), yf + LIP_FILLET * math.sin(a)))
    h = n_round // 2
    for i in range(h, -1, -1):  # half-round from θ_max down to the floor
        th = th_max * i / h
        p = (R * math.sin(th), cy - R * math.cos(th))
        if math.dist(p, pts[-1]) > 1e-9:
            pts.append(p)
    pts[-1] = (0.0, floor)
    return pts


def full_profile(half):
    """Closed section: the half, then its mirror back up (axis points not repeated)."""
    return half + [(-u, v) for u, v in reversed(half[1:-1])]


def to_blender(p):
    x, y, z = p
    return (x * 0.001, -z * 0.001, y * 0.001)


def _cutter_bmesh(bm, piece_id, q):
    """Closed cutter solid for the channel, built into bm in contract-frame mm."""
    sp = spec(piece_id)
    shape, level = sp["shape"], sp["level"]
    sts = stations(shape, level, q["path"][shape], inset_stations=True)
    # extensions past the cell faces along the end tangents
    d0 = (1.0, 0.0)
    sts = [(sts[0][0] - EXT * d0[0], sts[0][1] - EXT * d0[1]) + sts[0][2:]] + sts
    if shape != "end":
        d1 = (1.0, 0.0) if shape != "curve90" else (0.0, -1.0)
        sts = sts + [(sts[-1][0] + EXT * d1[0], sts[-1][1] + EXT * d1[1]) + sts[-1][2:]]
    rings = []
    for cx, cz, lx, lz, fl in sts:
        prof = full_profile(render_half_profile(fl, q["n_round"], q["n_fillet"]))
        rings.append([bm.verts.new((cx + u * lx, v, cz + u * lz)) for u, v in prof])
    n = len(rings[0])
    if any(len(r) != n for r in rings):
        raise RuntimeError(f"{piece_id}: profile vertex count changes along the path")
    for a, b in zip(rings, rings[1:]):
        for j in range(n):
            bm.faces.new((a[j], a[(j + 1) % n], b[(j + 1) % n], b[j]))
    bm.faces.new(rings[0][::-1])
    if shape != "end":
        bm.faces.new(rings[-1])
        return
    # end: half-turn of the u >= 0 half about the vertical axis at x = END_AXIS_X
    end = rings[-1]
    h = len(render_half_profile(sts[-1][4], q["n_round"], q["n_fillet"])) - 1
    k_cap = q["path"]["end"]
    half = render_half_profile(sts[-1][4], q["n_round"], q["n_fillet"])
    cap = [end[:h + 1]]
    for k in range(1, k_cap):
        psi = math.radians(180.0 * k / k_cap)
        lx, lz = math.sin(psi), math.cos(psi)
        ring = [end[0]] + [bm.verts.new((END_AXIS_X + u * lx, v, u * lz)) for u, v in half[1:h]] + [end[h]]
        cap.append(ring)
    cap.append([end[0]] + [end[(n - j) % n] for j in range(1, h)] + [end[h]])
    for a, b in zip(cap, cap[1:]):
        for j in range(h):
            vs = [a[j], a[j + 1], b[j + 1], b[j]]
            uniq = []
            for v in vs:
                if v not in uniq:
                    uniq.append(v)
            if len(uniq) >= 3:
                bm.faces.new(uniq)


def build_render_object(piece_id, q, name, collection, material=None, depsgraph=None):
    """Render mesh for one LOD (or the high-poly) as a Blender object linked into `collection`:
    bevelled 59 mm body minus the channel cutter (EXACT boolean), triangulated, smooth with sharp
    edges above 35°. Returns the object."""
    import bmesh
    import bpy
    from mathutils import Matrix

    to_b = Matrix(((0.001, 0, 0, 0), (0, 0, -0.001, 0), (0, 0.001, 0, 0), (0, 0, 0, 1)))

    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=CELL - 2 * INSET)
    bmesh.ops.translate(bm, verts=bm.verts, vec=(0.0, CELL / 2, 0.0))
    if q["bevel_segs"]:
        bmesh.ops.bevel(bm, geom=list(bm.edges) + list(bm.verts), offset=BEVEL, offset_type="OFFSET",
                        profile_type="SUPERELLIPSE", segments=q["bevel_segs"], profile=0.5, affect="EDGES",
                        clamp_overlap=True)
    bmesh.ops.transform(bm, matrix=to_b, verts=bm.verts)
    body_me = bpy.data.meshes.new(name)
    bm.to_mesh(body_me)
    bm.free()

    bm = bmesh.new()
    _cutter_bmesh(bm, piece_id, q)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bmesh.ops.transform(bm, matrix=to_b, verts=bm.verts)
    cut_me = bpy.data.meshes.new(name + "_cutter")
    bm.to_mesh(cut_me)
    bm.free()

    body = bpy.data.objects.new(name, body_me)
    cutter = bpy.data.objects.new(name + "_cutter", cut_me)
    collection.objects.link(body)
    collection.objects.link(cutter)
    mod = body.modifiers.new("channel", "BOOLEAN")
    mod.operation = "DIFFERENCE"
    mod.solver = "EXACT"
    mod.object = cutter
    dg = depsgraph or bpy.context.evaluated_depsgraph_get()
    dg.update()
    result = bpy.data.meshes.new_from_object(body.evaluated_get(dg))
    body.modifiers.remove(mod)
    bpy.data.objects.remove(cutter)
    bpy.data.meshes.remove(cut_me)
    old = body.data
    body.data = result
    bpy.data.meshes.remove(old)
    result.name = name

    bm = bmesh.new()
    bm.from_mesh(result)
    bmesh.ops.dissolve_degenerate(bm, edges=bm.edges, dist=1e-7)
    bmesh.ops.triangulate(bm, faces=bm.faces, quad_method="FIXED", ngon_method="EAR_CLIP")
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.to_mesh(result)
    bm.free()
    for p in result.polygons:
        p.use_smooth = True
    result.set_sharp_from_angle(angle=math.radians(35.0))
    result.materials.clear()
    if material is not None:
        result.materials.append(material)
    return body


def build_collider_object(piece_id, name, collection):
    import bpy
    verts, tris = collision_mesh(piece_id)
    me = bpy.data.meshes.new(name)
    me.from_pydata([to_blender((x / 1000.0, y / 1000.0, z / 1000.0)) for x, y, z in verts], [], tris)
    me.update()
    ob = bpy.data.objects.new(name, me)
    collection.objects.link(ob)
    return ob


SOCKET_YAW = {"+x": 0.0, "-x": 180.0, "-z": 90.0, "+z": -90.0}  # Blender yaw so local +X faces out


def build_socket_empties(piece_id, collection, parent=None, prefix=""):
    """One empty per port, named port_<index>, at the port floor point on the cell face; local +X
    points out of the face. Extras carry the sidecar port fields."""
    import bpy
    out = []
    for p in spec(piece_id)["ports"]:
        pos, _ = port_point_mm(p)
        e = bpy.data.objects.new(f"{prefix}port_{p['index']}", None)
        e.empty_display_type = "SINGLE_ARROW"
        e.empty_display_size = 0.012
        e.location = to_blender(pos)
        e.rotation_euler = (0.0, 0.0, math.radians(SOCKET_YAW[p["face"]]))
        for k in ("index", "face", "level", "class", "flow", "kind"):
            e[k] = p[k]
        collection.objects.link(e)
        if parent is not None:
            e.parent = parent
        out.append(e)
    return out


def placeholder_wood(name="wood_placeholder"):
    """Placeholder until P1-T1 delivers the tileable wood. No emission (§5.6)."""
    import bpy
    m = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    bsdf = next(n for n in m.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
    bsdf.inputs["Base Color"].default_value = (0.58, 0.40, 0.22, 1.0)
    bsdf.inputs["Roughness"].default_value = 0.55
    bsdf.inputs["Emission Strength"].default_value = 0.0
    m.use_backface_culling = True
    return m
