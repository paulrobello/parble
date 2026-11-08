"""P2-T2 passive block pieces, built against docs/kit-contract.md (KIT_CONTRACT_VERSION 1).

Imports kitgeom (the P1-T2 helper the four approved pieces hash) and never edits it. kitgeom
still builds the straight, curve, gentle slope and end shapes at every legal level; this module
adds the shapes kitgeom has no code for: steep slope, drop hole, redirect drop, joiner, support
pillar and tower socket block.

Collision differs from kitgeom on purpose. kitgeom writes each collision mesh triangle by triangle
in integer µm; for bowls, bores, tunnels and a merging branch that is days of hand geometry. Here
the collision mesh is the EXACT boolean of a sharp, uninset body and collision cutters (16-segment
half-rounds, vertical walls, 16-segment bores) evaluated in contract-frame millimetres and rounded
to µm. Every cutter vertex is placed at an integer µm, and the port rings are where straight
cutter extensions cross the face plane, so they come out at the canonical integer values;
`collision_mesh` asserts every side port ring is exactly canonical (open or tunnel ring) before
it returns. The boolean is deterministic for one Blender build (kit-determinism checks it).
"""
import math
import re

import kitgeom as K

PIECE_VERSION = 1

# kitgeom shapes at other levels: the L2 set follows blk_slope_gentle_l3's L2 exit, and
# blk_straight_l1 follows the L1 exits of blk_slope_gentle_l2 and blk_slope_steep_l3.
KITGEOM_VARIANTS = ("blk_straight_l2", "blk_curve90_l2", "blk_slope_gentle_l2", "blk_straight_l1")
NEW_SHAPES = ("blk_slope_steep_l3", "blk_drop_l3", "blk_drop_l2", "blk_redirect_l3", "blk_redirect_l2",
              "blk_join_l3", "blk_join_l2", "blk_pillar", "blk_tower_socket_l1")
P2_BUILD = KITGEOM_VARIANTS + NEW_SHAPES
BUILD = K.BUILD + P2_BUILD

SHAPE_RE = re.compile(r"^blk_(slope_steep|drop|redirect|join|tower_socket)_l([0-3])$")
LEGAL = {"slope_steep": (3,), "drop": (1, 2, 3), "redirect": (1, 2, 3), "join": (1, 2, 3), "tower_socket": (1,)}

TOP = K.CELL               # top of the anchor cell (every piece here carves from the anchor's top)
BOWL_R = 18.0              # §4: 36 mm bowl around the drop hole
HOLE_R = 16.0              # §4: 32 mm drop hole
SOCKET_R = 20.0            # §5.5: 40 mm socket bore
SOCKET_DEPTH = 10.5        # §5.5 render bore depth
SOCKET_INSET = 10.0        # §5.5 collision port plane inside the face
TUNNEL_WALL = 6.0          # §5.1: 34 mm tunnel = R14 floor + 6 mm wall + R14 crown
LANDING_RISE = 15.0        # a dropped marble lands on a floor this much above the tunnel exit floor
COLLISION_SEGS = 16        # §5.4 half-round and bore segments
COLLISION_PATH = 8         # path segments for collision arcs and slopes (kitgeom COLLISION_PATH)


def handles(piece_id):
    return piece_id == "blk_pillar" or bool(SHAPE_RE.match(piece_id))


def _side(i, cell, face, level, flow, cls="channel", inset=0):
    return {"cell": list(cell), "class": cls, "face": face, "flow": flow, "index": i, "inset_um": inset,
            "kind": "side", "level": level}


def _drop(i, cell, face, flow):
    return {"cell": list(cell), "class": "drop", "face": face, "flow": flow, "index": i, "inset_um": 0,
            "kind": "drop"}


def spec(piece_id):
    """Ports, shape, level and footprint for a P2-T2 piece id (§4, §5.5, §7, §9, §10)."""
    if not K.ID_RE.match(piece_id) or len(piece_id) > 48:
        raise ValueError(f"{piece_id}: not a §10 id")
    if piece_id == "blk_pillar":
        return {"id": piece_id, "shape": "pillar", "level": None, "chiral": False, "family": "blk",
                "footprint": [[0, 0, 0]],
                "ports": [_drop(0, (0, 0, 0), "top", "in"), _drop(1, (0, 0, 0), "bottom", "out")]}
    m = SHAPE_RE.match(piece_id)
    if not m:
        raise ValueError(f"{piece_id}: not a P2-T2 block piece")
    shape, level = m.group(1), int(m.group(2))
    if level not in LEGAL[shape]:
        raise ValueError(f"{piece_id}: level {level} is not a variant of {shape}")
    o, below = (0, 0, 0), (0, -1, 0)
    fp = [[0, 0, 0]]
    chiral = False
    if shape == "slope_steep":
        ports = [_side(0, o, "-x", level, "in"), _side(1, o, "+x", level - 2, "out")]
    elif shape == "drop":
        ports = [_side(0, o, "-x", level, "in"), _drop(1, o, "bottom", "out")]
    elif shape == "redirect":  # §9 rule 7, §16 Q8: one cell layer lower, same level
        ports = [_side(0, o, "-x", level, "in"), _side(1, below, "+x", level, "out")]
        fp = [[0, -1, 0], [0, 0, 0]]
    elif shape == "join":
        ports = [_side(0, o, "-x", level, "in"), _side(1, o, "-z", level, "in"), _side(2, o, "+x", level, "out")]
        chiral = True
    else:  # tower_socket: a dropped marble leaves through the socket (§5.5, ref-06)
        ports = [_side(0, o, "-x", level, "out", cls="socket", inset=10000), _drop(1, o, "top", "in")]
    return {"id": piece_id, "shape": shape, "level": level, "ports": ports, "chiral": chiral,
            "family": "blk", "footprint": fp}


def footprint(piece_id):
    return (spec(piece_id) if handles(piece_id) else K.spec(piece_id))["footprint"]


def cells(piece_id):
    return len(footprint(piece_id))


def bounds_mm(piece_id):
    """(lo, hi) contract-frame mm of the footprint's bounding box."""
    fp = footprint(piece_id)
    lo = [min(c[k] for c in fp) for k in range(3)]
    hi = [max(c[k] for c in fp) for k in range(3)]
    return ((lo[0] * K.CELL - K.CELL / 2, lo[1] * K.CELL, lo[2] * K.CELL - K.CELL / 2),
            (hi[0] * K.CELL + K.CELL / 2, hi[1] * K.CELL + K.CELL, hi[2] * K.CELL + K.CELL / 2))


def port_point_mm(port):
    """Socket position, contract-frame mm: side ports on the face at the floor, drop ports at the
    face centre."""
    c = port["cell"]
    base = [c[0] * K.CELL, c[1] * K.CELL, c[2] * K.CELL]
    if port["kind"] == "drop":
        base[1] += K.CELL if port["face"] == "top" else 0.0
        return tuple(base)
    (x, y, z), _ = K.port_point_mm(port)
    return (base[0] + x, base[1] + y, base[2] + z)


# ---------------------------------------------------------------------------------------------
# Paths and profiles (contract-frame mm). A station is (cx, cz, lx, lz, floor): centre, lateral
# unit vector (+u) and floor height.
# ---------------------------------------------------------------------------------------------

def smooth(t):
    return 3 * t * t - 2 * t ** 3


def straight_x(x0, x1, floor_fn, n, extra_x=()):
    xs = sorted({x0 + (x1 - x0) * k / n for k in range(n + 1)} | set(extra_x))
    return [(x, 0.0, 0.0, 1.0, floor_fn(x)) for x in xs]


def arc(centre, r, phi0, phi1, n, floor, extra=()):
    """Arc about `centre` (x, z) from angle phi0 to phi1 (degrees), lateral = radial."""
    phis = sorted({phi0 + (phi1 - phi0) * k / n for k in range(n + 1)} | set(extra),
                  reverse=phi1 < phi0)
    out = []
    for p in phis:
        a = math.radians(p)
        c, s = math.cos(a), math.sin(a)
        out.append((centre[0] + r * c, centre[1] + r * s, c, s, floor))
    return out


def extend(sts, start, end):
    """Add a station EXT mm past each listed end, along the end tangent (port faces)."""
    def ext(a, b):
        d = (a[0] - b[0], a[1] - b[1])
        n = math.hypot(*d)
        return (a[0] + K.EXT * d[0] / n, a[1] + K.EXT * d[1] / n) + a[2:]
    out = list(sts)
    if start:
        out.insert(0, ext(sts[0], sts[1]))
    if end:
        out.append(ext(sts[-1], sts[-2]))
    return out


def um(x):
    return K.round_half_away(x * 1000.0)


def open_profile(collision, q):
    if collision:
        def prof(fl):
            ring = K.canonical_ring(um(fl))
            return [(-14.0, TOP + K.CUT_ABOVE)] + [(u / 1000.0, v / 1000.0) for u, v in ring[1:-1]] + \
                   [(14.0, TOP + K.CUT_ABOVE)]
        return prof
    return lambda fl: K.full_profile(K.render_half_profile(fl, q["n_round"], q["n_fillet"]))


def tunnel_ring_um(floor_um):
    """Collision tunnel section (§5.1 bore) as integer µm (u, v): the 17 canonical half-round floor
    vertices (θ -90..+90), then the 17 crown vertices from u = +14 to u = -14 (crown centre at
    floor + 20 mm, apex at floor + 34 mm)."""
    pts = [p for p in K.canonical_ring(floor_um)[1:-1]]
    for i in range(17):
        ph = math.radians(90.0 - 11.25 * i)
        pts.append((K.round_half_away(14000.0 * math.sin(ph)),
                    floor_um + 20000 + K.round_half_away(14000.0 * math.cos(ph))))
    return pts


def tunnel_profile(collision, q):
    if collision:
        return lambda fl: [(u / 1000.0, v / 1000.0) for u, v in tunnel_ring_um(um(fl))]

    def prof(fl):
        h = q["n_round"] // 2
        pts = []
        for i in range(2 * h + 1):  # floor, θ -90..+90
            th = math.radians(-90.0 + 90.0 * i / h)
            pts.append((K.R * math.sin(th), fl + K.R - K.R * math.cos(th)))
        for i in range(2 * h + 1):  # crown, +90..-90
            ph = math.radians(90.0 - 90.0 * i / h)
            pts.append((K.R * math.sin(ph), fl + K.R + TUNNEL_WALL + K.R * math.cos(ph)))
        return pts
    return prof


def sweep(bm, sts, prof):
    """Closed solid: `prof(floor)` swept through the stations, both ends capped."""
    rings = []
    for cx, cz, lx, lz, fl in sts:
        rings.append([bm.verts.new((cx + u * lx, v, cz + u * lz)) for u, v in prof(fl)])
    n = len(rings[0])
    if any(len(r) != n for r in rings):
        raise RuntimeError("profile vertex count changes along the path")
    for a, b in zip(rings, rings[1:]):
        for j in range(n):
            bm.faces.new((a[j], a[(j + 1) % n], b[(j + 1) % n], b[j]))
    bm.faces.new(rings[0][::-1])
    bm.faces.new(rings[-1])


def lathe(bm, profile, n, axis="y", centre=(0.0, 0.0)):
    """Closed solid of revolution: `profile` is [(radius, height)] from one cap to the other.
    axis "y": vertical, centre (x, z). axis "x": horizontal along x, centre (y, z). Vertex 0 of
    every ring sits at angle -90° (straight down for axis "x"), so a bore's lowest point is a vertex."""
    rings = []
    for r, h in profile:
        ring = []
        for k in range(n):
            a = math.radians(-90.0 + 360.0 * k / n)
            if axis == "y":
                ring.append(bm.verts.new((centre[0] + r * math.cos(a), h, centre[1] + r * math.sin(a))))
            else:
                ring.append(bm.verts.new((h, centre[0] + r * math.sin(a), centre[1] + r * math.cos(a))))
        rings.append(ring)
    for a, b in zip(rings, rings[1:]):
        for j in range(n):
            bm.faces.new((a[j], a[(j + 1) % n], b[(j + 1) % n], b[j]))
    bm.faces.new(rings[0][::-1])
    bm.faces.new(rings[-1])


# ---------------------------------------------------------------------------------------------
# Cutters per shape
# ---------------------------------------------------------------------------------------------

def cutters(piece_id, q, collision):
    """List of builder callables, each adding one closed cutter solid to a bmesh (contract mm).
    `q` is a kitgeom LOD dict (render) or None (collision)."""
    sp = spec(piece_id)
    shape, level = sp["shape"], sp["level"]
    f0 = (K.LEVEL_STEP * level) if level is not None else 0.0
    path_n = COLLISION_PATH if collision else q["path"]["slope_gentle"]
    arc_n = COLLISION_PATH if collision else q["path"]["curve90"]
    circ = COLLISION_SEGS if collision else max(8, q["n_round"])
    oprof = open_profile(collision, q)
    tprof = tunnel_profile(collision, q)
    inset_x = () if collision else (-K.CELL / 2 + K.INSET, K.CELL / 2 - K.INSET)
    out = []

    def bowl_and_bore(bottom):
        # bowl: 36 mm pocket from above the top down to the floor; hole: 32 mm bore below it
        out.append(lambda bm: lathe(bm, [(BOWL_R, TOP + K.CUT_ABOVE), (BOWL_R, f0)], circ))
        out.append(lambda bm: lathe(bm, [(HOLE_R, f0 + 1.0), (HOLE_R, bottom)], circ))

    if shape == "slope_steep":
        drop = 2 * K.LEVEL_STEP
        fl = lambda x: f0 - drop * smooth((x + 30.0) / 60.0)
        sts = straight_x(-30.0, 30.0, fl, path_n, extra_x=inset_x)
        out.append(lambda bm: sweep(bm, extend(sts, True, True), oprof))
    elif shape == "drop":
        sts = straight_x(-30.0, 0.0, lambda x: f0, 1, extra_x=())
        out.append(lambda bm: sweep(bm, extend(sts, True, False), oprof))
        bowl_and_bore(-K.EXT)
    elif shape == "redirect":
        fe = f0 - K.CELL                      # exit floor in the lower cell (same level)
        x_land = -HOLE_R - 1.0                # tunnel reaches 1 mm past the shaft's far wall
        fl = lambda x: fe + LANDING_RISE * (1.0 - smooth((x - x_land) / (30.0 - x_land)))
        sts_in = straight_x(-30.0, 0.0, lambda x: f0, 1)
        out.append(lambda bm: sweep(bm, extend(sts_in, True, False), oprof))
        bowl_and_bore(fe + 30.0)
        sts_t = straight_x(x_land, 30.0, fl, path_n, extra_x=() if collision else (30.0 - K.INSET,))
        out.append(lambda bm: sweep(bm, extend(sts_t, False, True), tprof))
    elif shape == "join":
        sts_s = straight_x(-30.0, 30.0, lambda x: f0, 1)
        out.append(lambda bm: sweep(bm, extend(sts_s, True, True), oprof))
        a = math.degrees(math.asin(K.INSET / 30.0))
        extra = () if collision else (180.0 - a, 90.0 + a)
        sts_a = arc((30.0, -30.0), 30.0, 180.0, 90.0, arc_n, f0, extra=extra)
        out.append(lambda bm: sweep(bm, extend(sts_a, True, True), oprof))
    elif shape == "tower_socket":
        x_end = HOLE_R + 1.0
        fl = lambda x: f0 + LANDING_RISE * smooth((x + 20.0) / (x_end + 20.0))
        x0 = -30.0 + SOCKET_INSET
        if collision:  # 6 path segments: the 34-vertex tunnel ring at 8 would pass 600 tris (§12)
            sts_t = straight_x(x0, x_end, fl, 6)
            out.append(lambda bm: sweep(bm, extend(sts_t, True, False), tprof))
        else:
            sts_t = straight_x(x0, x_end, fl, path_n)
            sts_t = [(x0 - 1.5,) + sts_t[0][1:]] + sts_t   # runs 1.5 mm into the socket bore
            out.append(lambda bm: sweep(bm, sts_t, tprof))
            yc = f0 + K.R
            out.append(lambda bm: lathe(bm, [(SOCKET_R, -30.0 - K.EXT), (SOCKET_R, -30.0 + SOCKET_DEPTH)],
                                        max(24, 2 * q["n_round"]) if q["n_round"] > 6 else 8,
                                        axis="x", centre=(yc, 0.0)))
        # funnel mouth 36 mm narrowing to the 32 mm shaft (§4), down into the tunnel
        mouth = [(BOWL_R, TOP + K.CUT_ABOVE), (BOWL_R, TOP - 4.0), (HOLE_R, TOP - 8.0)]
        if not collision and q["n_round"] <= 6:  # LOD2 budget: plain bore, no mouth
            mouth = [(HOLE_R, TOP + K.CUT_ABOVE)]
        out.append(lambda bm: lathe(bm, mouth + [(HOLE_R, f0 + LANDING_RISE + 10.0)], circ))
    elif shape == "pillar":
        out.append(lambda bm: lathe(bm, [(HOLE_R, TOP + K.CUT_ABOVE), (HOLE_R, -K.EXT)], circ))
    return out


def body_box(piece_id, collision):
    lo, hi = bounds_mm(piece_id)
    if sp_shape(piece_id) == "tower_socket" and collision:
        lo = (lo[0] + SOCKET_INSET, lo[1], lo[2])  # the rail spigot owns the socket bore (§5.5)
    if collision:
        return lo, hi
    i = K.INSET
    return (lo[0] + i, lo[1] + i, lo[2] + i), (hi[0] - i, hi[1] - i, hi[2] - i)


def sp_shape(piece_id):
    return spec(piece_id)["shape"]


def _boolean_mesh(piece_id, q, name, collection, collision):
    """Body minus every cutter (EXACT), in contract-frame mm, as a triangulated Blender mesh."""
    import bmesh
    import bpy
    lo, hi = body_box(piece_id, collision)
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    size = [hi[k] - lo[k] for k in range(3)]
    mid = [(hi[k] + lo[k]) / 2 for k in range(3)]
    bmesh.ops.scale(bm, vec=size, verts=bm.verts)
    bmesh.ops.translate(bm, vec=mid, verts=bm.verts)
    if not collision and q["bevel_segs"]:
        bmesh.ops.bevel(bm, geom=list(bm.edges) + list(bm.verts), offset=K.BEVEL, offset_type="OFFSET",
                        profile_type="SUPERELLIPSE", segments=q["bevel_segs"], profile=0.5, affect="EDGES",
                        clamp_overlap=True)
    body_me = bpy.data.meshes.new(name)
    bm.to_mesh(body_me)
    bm.free()
    body = bpy.data.objects.new(name, body_me)
    collection.objects.link(body)
    cut_objs = []
    for i, build in enumerate(cutters(piece_id, q, collision)):
        bm = bmesh.new()
        build(bm)
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        me = bpy.data.meshes.new(f"{name}_cut{i}")
        bm.to_mesh(me)
        bm.free()
        ob = bpy.data.objects.new(f"{name}_cut{i}", me)
        collection.objects.link(ob)
        mod = body.modifiers.new(f"cut{i}", "BOOLEAN")
        mod.operation = "DIFFERENCE"
        mod.solver = "EXACT"
        mod.object = ob
        cut_objs.append(ob)
    dg = bpy.context.evaluated_depsgraph_get()
    dg.update()
    result = bpy.data.meshes.new_from_object(body.evaluated_get(dg))
    for m in list(body.modifiers):
        body.modifiers.remove(m)
    for ob in cut_objs:
        me = ob.data
        bpy.data.objects.remove(ob)
        bpy.data.meshes.remove(me)
    old = body.data
    body.data = result
    bpy.data.meshes.remove(old)
    result.name = name
    bm = bmesh.new()
    bm.from_mesh(result)
    bmesh.ops.dissolve_degenerate(bm, edges=bm.edges, dist=1e-4)
    bmesh.ops.triangulate(bm, faces=bm.faces, quad_method="FIXED", ngon_method="EAR_CLIP")
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.to_mesh(result)
    bm.free()
    return body


# ---------------------------------------------------------------------------------------------
# Collision (integer µm) and sidecar
# ---------------------------------------------------------------------------------------------

_COLLISION_CACHE = {}


def _expected_rings(port):
    """Port plane axis, plane coordinate and the allowed rings [(u, v) µm sets] for a side port."""
    face = port["face"]
    ax = 0 if face[1] == "x" else 2
    sgn = 1 if face[0] == "+" else -1
    c = port["cell"]
    plane = c[ax] * 60000 + sgn * (30000 - port["inset_um"])
    u0 = c[2 if ax == 0 else 0] * 60000
    floor = c[1] * 60000 + 15000 * port["level"]
    open_ring = {(u + u0, c[1] * 60000 + v) for u, v in K.canonical_ring(15000 * port["level"])}
    tunnel = {(u + u0, v) for u, v in tunnel_ring_um(floor)}
    return ax, plane, open_ring, tunnel


def collision_mesh(piece_id):
    """(vertices [(x, y, z) µm], triangles [(i, j, k)]) from the EXACT boolean (see module doc).
    Needs Blender; cached per id so the sidecar and the collider object agree."""
    if piece_id in _COLLISION_CACHE:
        return _COLLISION_CACHE[piece_id]
    import bpy
    ob = _boolean_mesh(piece_id, None, f"{piece_id}_colwork", bpy.context.scene.collection, collision=True)
    me = ob.data
    idx_of, verts, tris = {}, [], []
    remap = []
    for v in me.vertices:
        p = tuple(um(c) for c in v.co)
        if p not in idx_of:
            idx_of[p] = len(verts)
            verts.append(p)
        remap.append(idx_of[p])
    for poly in me.polygons:
        t = tuple(remap[i] for i in poly.vertices)
        if len(set(t)) == 3:
            tris.append(t)
    bpy.data.objects.remove(ob)
    bpy.data.meshes.remove(me)
    bpy.context.view_layer.update()
    # every side port ring must be exactly canonical (open channel or tunnel)
    for p in spec(piece_id)["ports"]:
        if p["kind"] != "side":
            continue
        ax, plane, open_ring, tunnel = _expected_rings(p)
        other = 2 if ax == 0 else 0
        u0 = p["cell"][other] * 60000
        floor = p["cell"][1] * 60000 + 15000 * p["level"]
        on = [v for v in verts if v[ax] == plane and abs(v[other] - u0) <= 14000]
        got_open = {(v[other], v[1]) for v in on if p["cell"][1] * 60000 <= v[1] <= p["cell"][1] * 60000 + 60000}
        got_tun = {(v[other], v[1]) for v in on if floor <= v[1] <= floor + 34000}
        if got_open != open_ring and got_tun != tunnel:
            raise RuntimeError(f"{piece_id}: port {p['index']} collision ring is not canonical: "
                               f"{sorted(got_tun)[:6]} ...")
    _COLLISION_CACHE[piece_id] = (verts, tris)
    return verts, tris


def sidecar(piece_id):
    sp = spec(piece_id)
    verts, tris = collision_mesh(piece_id)
    return {
        "animated_parts": [],
        "chiral": sp["chiral"],
        "collision": [{"indices": [i for t in tris for i in t], "surface": "wood",
                       "vertices_um": [c for v in verts for c in v]}],
        "family": sp["family"],
        "footprint": sorted(sp["footprint"]),
        "id": piece_id,
        "kit_contract": K.KIT_CONTRACT_VERSION,
        "piece_version": PIECE_VERSION,
        "ports": sp["ports"],
        "schema": K.SIDECAR_SCHEMA,
        "sensors": [],
    }


def sidecar_bytes(piece_id):
    import json
    return (json.dumps(sidecar(piece_id), indent=2, sort_keys=True, ensure_ascii=True) + "\n").encode()


# ---------------------------------------------------------------------------------------------
# Blender objects (same signatures as kitgeom's)
# ---------------------------------------------------------------------------------------------

def build_render_object(piece_id, q, name, collection, material=None, depsgraph=None):
    from mathutils import Matrix
    ob = _boolean_mesh(piece_id, q, name, collection, collision=False)
    to_b = Matrix(((0.001, 0, 0, 0), (0, 0, -0.001, 0), (0, 0.001, 0, 0), (0, 0, 0, 1)))
    me = ob.data
    me.transform(to_b)
    me.update()
    for p in me.polygons:
        p.use_smooth = True
    me.set_sharp_from_angle(angle=math.radians(35.0))
    me.materials.clear()
    if material is not None:
        me.materials.append(material)
    return ob


def build_collider_object(piece_id, name, collection):
    import bpy
    verts, tris = collision_mesh(piece_id)
    me = bpy.data.meshes.new(name)
    me.from_pydata([K.to_blender((x / 1000.0, y / 1000.0, z / 1000.0)) for x, y, z in verts], [], tris)
    me.update()
    ob = bpy.data.objects.new(name, me)
    collection.objects.link(ob)
    return ob


DROP_PITCH = {"top": -90.0, "bottom": 90.0}  # Blender Y rotation so local +X points out of the face


def build_socket_empties(piece_id, collection, parent=None, prefix=""):
    import bpy
    out = []
    for p in spec(piece_id)["ports"]:
        e = bpy.data.objects.new(f"{prefix}port_{p['index']}", None)
        e.empty_display_type = "SINGLE_ARROW"
        e.empty_display_size = 0.012
        e.location = K.to_blender(port_point_mm(p))
        if p["kind"] == "side":
            e.rotation_euler = (0.0, 0.0, math.radians(K.SOCKET_YAW[p["face"]]))
        else:
            e.rotation_euler = (0.0, math.radians(DROP_PITCH[p["face"]]), 0.0)
        for k in ("index", "face", "level", "class", "flow", "kind"):
            if k in p:
                e[k] = p[k]
        collection.objects.link(e)
        if parent is not None:
            e.parent = parent
        out.append(e)
    return out
