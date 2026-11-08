"""P2-T3 mechanism pieces, built against docs/kit-contract.md (KIT_CONTRACT_VERSION 1).

Imports kitgeom and kitblocks and edits neither, so every block piece rebuilds byte-identically.
The static body follows kitblocks: an EXACT boolean of a sharp body box and channel cutters (plus
one union, the one-way gate's bridge), rounded to µm, with every side port ring asserted canonical.

Each moving part is a hinge (contract §11 `animated_parts`): a GLB empty `anim_<name>` at the pivot,
rotated to the rest angle, with render children `anim_<name>_lod{0,1,2}` in the part's frame. The
sidecar stores the part's own collision in the piece-local frame at 0 mdeg, as convex solids
(boxes and extruded polygons) in integer µm. Static collision never includes a part, so the
overhead and collider checks see only the piece body. Behaviour is P2-T5.

Parts per piece (kit-check enforces the names): hopper `anim_gate` (release gate), splitter
`anim_flap` and toggle `anim_rocker` (both a switch blade at the fork), one-way gate `anim_flap`, one-way hole `anim_trapdoor`. The
finish cup and the checkpoint arch have no moving part, only sensors.
"""
import math

import kitblocks as B
import kitgeom as K

PIECE_VERSION = 1
BUILD = ("mech_finish", "mech_hopper", "mech_splitter_l3", "mech_toggle_l3", "mech_checkpoint_l1",
         "mech_gate_oneway_l1", "mech_hole_oneway_l1")

TOP = K.CELL
COLLISION_SEGS = 16
COLLISION_PATH = 8
PART_BUDGET = {0: 1500, 1: 500, 2: 120}   # §12: all animated parts of a piece together

HOPPER_X = -10.0       # hopper bowl centre
HOPPER_BOWL = [(17.0, TOP + K.CUT_ABOVE), (17.0, 50.0), (14.0, 36.0)]   # (radius, height), to the floor
FLAP_PIVOT_Y = 51.0    # §11 example: L1 flap hinge 36 mm above the floor
FLAP_HALF = 12.0       # flap half-width (2 mm clear of each channel wall)
FLAP_T = 1.0           # flap half-thickness
BRIDGE_X = 4.0         # one-way gate bridge half-length; its underside stays above the pin
BRIDGE_Y = 53.0
FINISH_X = 6.0
FINISH_CUP = [(20.0, TOP + K.CUT_ABOVE), (20.0, 20.0), (14.0, 12.0)]
HOLE_BOWL_R = 18.0     # §4, as blk_drop
HOLE_R = 16.0
TRAP_R = 14.0


def handles(piece_id):
    return piece_id in BUILD


def _side(i, face, level, flow):
    return {"cell": [0, 0, 0], "class": "channel", "face": face, "flow": flow, "index": i, "inset_um": 0,
            "kind": "side", "level": level}


def _drop(i, face, flow):
    return {"cell": [0, 0, 0], "class": "drop", "face": face, "flow": flow, "index": i, "inset_um": 0,
            "kind": "drop"}


def _box_sensor(name, c, h):
    return {"center_um": list(c), "half_extents_um": list(h), "id": name, "shape": "box"}


def spec(piece_id):
    """Ports, shape, level and sensors of a P2-T3 piece (contract §10 ids, all 1 cell)."""
    if not handles(piece_id):
        raise ValueError(f"{piece_id}: not a P2-T3 mechanism piece")
    shape = piece_id[len("mech_"):].rsplit("_l", 1)[0] if piece_id[-3:-1] == "_l" else piece_id[len("mech_"):]
    if shape == "hopper":      # bowl holding the queue, release gate on the exit channel
        level, ports = 1, [_side(0, "+x", 1, "out")]
        sensors = [_box_sensor("release", (21000, 27500, 0), (5000, 12500, 14000))]
    elif shape == "finish":    # channel into a deep collector cup
        level, ports = 3, [_side(0, "-x", 3, "in")]
        sensors = [_box_sensor("cup", (6000, 26000, 0), (14000, 12000, 14000))]
    elif shape in ("splitter", "toggle"):   # fork: in at -x, out at +z and -z
        level = 3
        ports = [_side(0, "-x", 3, "in"), _side(1, "+z", 3, "out"), _side(2, "-z", 3, "out")]
        sensors = [_box_sensor("entry", (-22000, 52500, 0), (6000, 7500, 14000))]
    elif shape == "checkpoint":  # tunnel arch at L1 (apex 49 mm, under the cell top)
        level, ports = 1, [_side(0, "-x", 1, "both"), _side(1, "+x", 1, "both")]
        sensors = [_box_sensor("gate", (0, 27500, 0), (4000, 12500, 14000))]
    elif shape == "gate_oneway":
        level, ports = 1, [_side(0, "-x", 1, "in"), _side(1, "+x", 1, "out")]
        sensors = [_box_sensor("approach", (-20000, 27500, 0), (6000, 12500, 14000)),
                   _box_sensor("exit", (20000, 27500, 0), (6000, 12500, 14000))]
    elif shape == "hole_oneway":  # arriving from -x drops through the trapdoor; from +x rolls over
        level = 1
        ports = [_side(0, "-x", 1, "both"), _side(1, "+x", 1, "in"), _drop(2, "bottom", "out")]
        sensors = [_box_sensor("trap", (-22000, 27500, 0), (6000, 12500, 14000)),
                   _box_sensor("over", (22000, 27500, 0), (6000, 12500, 14000))]
    else:
        raise ValueError(piece_id)
    return {"id": piece_id, "shape": shape, "level": level, "ports": ports, "chiral": False, "family": "mech",
            "footprint": [[0, 0, 0]], "sensors": sensors}


def cells(piece_id):
    return 1


def bounds_mm(piece_id):
    return (-K.CELL / 2, 0.0, -K.CELL / 2), (K.CELL / 2, K.CELL, K.CELL / 2)


# ---------------------------------------------------------------------------------------------
# Static body
# ---------------------------------------------------------------------------------------------

def _box_builder(lo, hi):
    def build(bm):
        import bmesh
        bmesh.ops.create_cube(bm, size=1.0)
        bmesh.ops.scale(bm, vec=[hi[k] - lo[k] for k in range(3)], verts=bm.verts)
        bmesh.ops.translate(bm, vec=[(hi[k] + lo[k]) / 2 for k in range(3)], verts=bm.verts)
    return build


def ops(piece_id, q, collision):
    """[(boolean operation, bmesh builder)] in contract mm. `q` is a kitgeom LOD dict or None."""
    sp = spec(piece_id)
    shape = sp["shape"]
    f0 = K.LEVEL_STEP * sp["level"]
    # two arcs share the cell: 70% of a curve's path segments keeps LOD0 inside the block budget
    arc_n = COLLISION_PATH if collision else max(3, q["path"]["curve90"] * 7 // 10)
    circ = COLLISION_SEGS if collision else max(8, q["n_round"])
    oprof = B.open_profile(collision, q)
    ins = () if collision else (-K.CELL / 2 + K.INSET, K.CELL / 2 - K.INSET)
    top = TOP if collision else TOP - K.INSET
    out = []
    cut = lambda f: out.append(("DIFFERENCE", f))

    if shape == "hopper":
        sts = B.straight_x(HOPPER_X, 30.0, lambda x: f0, 1, extra_x=ins[1:])
        cut(lambda bm: B.sweep(bm, B.extend(sts, False, True), oprof))
        cut(lambda bm: B.lathe(bm, HOPPER_BOWL + [(14.0, f0)], circ, centre=(HOPPER_X, 0.0)))
    elif shape == "finish":
        sts = B.straight_x(-30.0, FINISH_X, lambda x: f0, 1, extra_x=ins[:1])
        cut(lambda bm: B.sweep(bm, B.extend(sts, True, False), oprof))
        cut(lambda bm: B.lathe(bm, FINISH_CUP, circ, centre=(FINISH_X, 0.0)))
    elif shape in ("splitter", "toggle"):
        a = math.degrees(math.asin(K.INSET / 30.0))
        e1 = () if collision else (90.0 - a, a)
        e2 = () if collision else (270.0 + a, 360.0 - a)
        s1 = B.arc((-30.0, -30.0), 30.0, 90.0, 0.0, arc_n, f0, extra=e1)
        s2 = B.arc((-30.0, 30.0), 30.0, 270.0, 360.0, arc_n, f0, extra=e2)
        cut(lambda bm: B.sweep(bm, B.extend(s1, True, True), oprof))
        cut(lambda bm: B.sweep(bm, B.extend(s2, True, True), oprof))
    elif shape == "checkpoint":
        sts = B.straight_x(-30.0, 30.0, lambda x: f0, 1, extra_x=ins)
        tprof = B.tunnel_profile(collision, q)
        cut(lambda bm: B.sweep(bm, B.extend(sts, True, True), tprof))
    elif shape == "gate_oneway":
        sts = B.straight_x(-30.0, 30.0, lambda x: f0, 1, extra_x=ins)
        cut(lambda bm: B.sweep(bm, B.extend(sts, True, True), oprof))
        # the arch: a bridge over the hinge pin, its underside 38 mm above the floor (§5.1)
        out.append(("UNION", _box_builder((-BRIDGE_X, BRIDGE_Y, -K.R - 2.0), (BRIDGE_X, top, K.R + 2.0))))
    elif shape == "hole_oneway":
        sts = B.straight_x(-30.0, 30.0, lambda x: f0, 1, extra_x=ins)
        cut(lambda bm: B.sweep(bm, B.extend(sts, True, True), oprof))
        cut(lambda bm: B.lathe(bm, [(HOLE_BOWL_R, TOP + K.CUT_ABOVE), (HOLE_BOWL_R, f0)], circ))
        cut(lambda bm: B.lathe(bm, [(HOLE_R, f0 + 1.0), (HOLE_R, -K.EXT)], circ))
    return out


def _boolean_mesh(piece_id, q, name, collection, collision):
    """Body box (bevelled for render) through every op (EXACT), contract mm, triangulated."""
    import bmesh
    import bpy
    lo, hi = bounds_mm(piece_id)
    if not collision:
        i = K.INSET
        lo, hi = tuple(c + i for c in lo), tuple(c - i for c in hi)
    bm = bmesh.new()
    _box_builder(lo, hi)(bm)
    if not collision and q["bevel_segs"]:
        bmesh.ops.bevel(bm, geom=list(bm.edges) + list(bm.verts), offset=K.BEVEL, offset_type="OFFSET",
                        profile_type="SUPERELLIPSE", segments=q["bevel_segs"], profile=0.5, affect="EDGES",
                        clamp_overlap=True)
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    body = bpy.data.objects.new(name, me)
    collection.objects.link(body)
    tools = []
    for i, (op, build) in enumerate(ops(piece_id, q, collision)):
        bm = bmesh.new()
        build(bm)
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        tme = bpy.data.meshes.new(f"{name}_op{i}")
        bm.to_mesh(tme)
        bm.free()
        ob = bpy.data.objects.new(f"{name}_op{i}", tme)
        collection.objects.link(ob)
        mod = body.modifiers.new(f"op{i}", "BOOLEAN")
        mod.operation = op
        mod.solver = "EXACT"
        mod.object = ob
        tools.append(ob)
    dg = bpy.context.evaluated_depsgraph_get()
    dg.update()
    result = bpy.data.meshes.new_from_object(body.evaluated_get(dg))
    for m in list(body.modifiers):
        body.modifiers.remove(m)
    for ob in tools:
        tme = ob.data
        bpy.data.objects.remove(ob)
        bpy.data.meshes.remove(tme)
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


_COLLISION_CACHE = {}


def collision_mesh(piece_id):
    """Static (vertices [(x, y, z) µm], triangles), every side port ring asserted canonical."""
    if piece_id in _COLLISION_CACHE:
        return _COLLISION_CACHE[piece_id]
    import bpy
    ob = _boolean_mesh(piece_id, None, f"{piece_id}_colwork", bpy.context.scene.collection, collision=True)
    me = ob.data
    idx_of, verts, tris, remap = {}, [], [], []
    for v in me.vertices:
        p = tuple(B.um(c) for c in v.co)
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
    for p in spec(piece_id)["ports"]:
        if p["kind"] != "side":
            continue
        ax, plane, open_ring, tunnel = B._expected_rings(p)
        other = 2 if ax == 0 else 0
        floor = 15000 * p["level"]
        on = [v for v in verts if v[ax] == plane and abs(v[other]) <= 14000]
        got_open = {(v[other], v[1]) for v in on if 0 <= v[1] <= 60000}
        got_tun = {(v[other], v[1]) for v in on if floor <= v[1] <= floor + 34000}
        if got_open != open_ring and got_tun != tunnel:
            raise RuntimeError(f"{piece_id}: port {p['index']} collision ring is not canonical: "
                               f"{sorted(got_open)[:6]} ...")
    _COLLISION_CACHE[piece_id] = (verts, tris)
    return verts, tris


# ---------------------------------------------------------------------------------------------
# Animated parts: convex solids in contract mm at 0 mdeg
# ---------------------------------------------------------------------------------------------

def _solid_box(lo, hi):
    v = [(x, y, z) for x in (lo[0], hi[0]) for y in (lo[1], hi[1]) for z in (lo[2], hi[2])]
    f = [(0, 1, 3, 2), (4, 6, 7, 5), (0, 4, 5, 1), (2, 3, 7, 6), (0, 2, 6, 4), (1, 5, 7, 3)]
    return v, f


def _solid_prism(poly, axis, a0, a1):
    """Convex polygon `poly` [(p, q)] extruded along `axis` from a0 to a1. Axis "x": poly is
    (z, y); axis "y": poly is (x, z); axis "z": poly is (x, y)."""
    def pt(p, q, a):
        return {"x": (a, q, p), "y": (p, a, q), "z": (p, q, a)}[axis]
    n = len(poly)
    v = [pt(p, q, a0) for p, q in poly] + [pt(p, q, a1) for p, q in poly]
    f = [tuple(range(n)), tuple(range(2 * n - 1, n - 1, -1))]
    f += [(i, (i + 1) % n, n + (i + 1) % n, n + i) for i in range(n)]
    return v, f


def _oriented(v, f):
    """Faces of a convex solid wound outward."""
    c = [sum(p[k] for p in v) / len(v) for k in range(3)]
    out = []
    for face in f:
        nx = ny = nz = 0.0
        for i in range(len(face)):
            a, b = v[face[i]], v[face[(i + 1) % len(face)]]
            nx += (a[1] - b[1]) * (a[2] + b[2])
            ny += (a[2] - b[2]) * (a[0] + b[0])
            nz += (a[0] - b[0]) * (a[1] + b[1])
        fc = [sum(v[i][k] for i in face) / len(face) for k in range(3)]
        d = nx * (fc[0] - c[0]) + ny * (fc[1] - c[1]) + nz * (fc[2] - c[2])
        out.append(face if d > 0 else tuple(reversed(face)))
    return out


def _u_plate(cy, segs):
    """Flap outline (z, y): the lower half-circle FLAP_HALF about (0, cy), then up to the pivot."""
    pts = []
    for i in range(segs + 1):
        th = math.radians(-90.0 + 180.0 * i / segs)
        pts.append((FLAP_HALF * math.sin(th), cy - FLAP_HALF * math.cos(th)))
    return pts + [(FLAP_HALF, FLAP_PIVOT_Y), (-FLAP_HALF, FLAP_PIVOT_Y)]


def _flap(px, f0, segs):
    plate = _solid_prism(_u_plate(f0 + K.R, segs), "x", px - FLAP_T, px + FLAP_T)
    pin = _solid_box((px - 1.5, FLAP_PIVOT_Y - 1.5, -K.R), (px + 1.5, FLAP_PIVOT_Y + 1.5, K.R))
    return [plate, pin]


# Fork switch blade (splitter and toggle): a vertical wall hinged about +Y at the fork, its free
# end BLADE_LEN upstream. At ±FORK_LIMIT the tip is 10.1 mm off the channel centre, past a standard
# marble's 8 mm radius, so the marble meets the blade's face and is steered to the open branch. The
# shallow 25° face keeps a slow or off-centre marble rolling instead of bouncing it back upstream.
# The underside (BLADE_UNDER: x at 0 mdeg, mm above the floor) follows the two troughs' floor and
# the ridge between them, clearing it everywhere in the sweep by at least 0.5 mm.
FORK_PIVOT_X = -2.0
BLADE_LEN = 24.0
BLADE_TOP = 14.0
BLADE_UNDER = [(-2.0, 6.0), (-10.0, 1.8), (-18.0, 1.0), (-26.0, 4.0)]
FORK_LIMIT = 25000


def _switch_blade(shape, f0, segs):
    """Splitter: a 2 mm blade on a slim post, resting at its +Z-tip limit. Toggle: the same blade on
    a round hub, resting at its -Z-tip limit (side 0, the first marble's default)."""
    half = FLAP_T
    tip = FORK_PIVOT_X - BLADE_LEN
    outline = [(x, f0 + y) for x, y in BLADE_UNDER] + [(tip, f0 + BLADE_TOP), (FORK_PIVOT_X, f0 + BLADE_TOP)]
    blade = _solid_prism(outline, "z", -half, half)
    y0, y1 = f0 + BLADE_UNDER[0][1], f0 + BLADE_TOP
    if shape == "splitter":
        hub = _solid_box((FORK_PIVOT_X - 1.5, y0, -1.5), (FORK_PIVOT_X + 1.5, y1, 1.5))
        name, rest = "anim_flap", FORK_LIMIT
    else:
        n = max(8, 2 * segs)
        ring = [(FORK_PIVOT_X + 2.0 * math.cos(2 * math.pi * k / n), 2.0 * math.sin(2 * math.pi * k / n))
                for k in range(n)]
        hub = _solid_prism(ring, "y", y0, y1)
        name, rest = "anim_rocker", -FORK_LIMIT
    return ({"name": name, "pivot_um": [B.um(FORK_PIVOT_X), B.um(f0), 0], "axis": [0, 1, 0],
             "rest_mdeg": rest, "limits_mdeg": [-FORK_LIMIT, FORK_LIMIT]}, [blade, hub])


def parts(piece_id, segs=None):
    """[(part dict without collision, [convex solids])], sorted by name. `segs` sets the curved
    outlines' segment count (collision: 8)."""
    sp = spec(piece_id)
    shape, f0 = sp["shape"], K.LEVEL_STEP * sp["level"]
    s = segs or 8
    if shape == "hopper":
        px = 12.0
        return [({"name": "anim_gate", "pivot_um": [12000, 51000, 0], "axis": [0, 0, 1], "rest_mdeg": 0,
                  "limits_mdeg": [0, 85000]}, _flap(px, f0, s))]
    if shape == "gate_oneway":
        return [({"name": "anim_flap", "pivot_um": [0, 51000, 0], "axis": [0, 0, 1], "rest_mdeg": 0,
                  "limits_mdeg": [0, 85000]}, _flap(0.0, f0, s))]
    if shape in ("splitter", "toggle"):
        return [_switch_blade(shape, f0, s)]
    if shape == "hole_oneway":  # trapdoor over the bore, hinged at its +x edge; drops the -x edge
        n = max(8, 2 * s)
        disc = [(TRAP_R * math.cos(2 * math.pi * k / n), TRAP_R * math.sin(2 * math.pi * k / n)) for k in range(n)]
        door = _solid_prism(disc, "y", f0 - 3.0, f0)
        return [({"name": "anim_trapdoor", "pivot_um": [14000, B.um(f0 - 1.5), 0], "axis": [0, 0, 1],
                  "rest_mdeg": 0, "limits_mdeg": [0, 60000]}, [door])]
    return []


def part_collision(solids):
    verts, idx, index_of = [], [], {}
    for v, f in solids:
        local = []
        for p in v:
            key = tuple(B.um(c) for c in p)
            if key not in index_of:
                index_of[key] = len(verts)
                verts.append(key)
            local.append(index_of[key])
        for face in _oriented(v, f):
            for k in range(1, len(face) - 1):
                idx += [local[face[0]], local[face[k]], local[face[k + 1]]]
    return [{"indices": idx, "surface": "wood_end_grain", "vertices_um": [c for p in verts for c in p]}]


# ---------------------------------------------------------------------------------------------
# Sidecar
# ---------------------------------------------------------------------------------------------

def sidecar(piece_id):
    sp = spec(piece_id)
    verts, tris = collision_mesh(piece_id)
    anim = []
    for part, solids in parts(piece_id):
        anim.append(dict(part, collision=part_collision(solids)))
    return {
        "animated_parts": anim,
        "chiral": sp["chiral"],
        "collision": [{"indices": [i for t in tris for i in t], "surface": "wood",
                       "vertices_um": [c for v in verts for c in v]}],
        "family": sp["family"],
        "footprint": sp["footprint"],
        "id": piece_id,
        "kit_contract": K.KIT_CONTRACT_VERSION,
        "piece_version": PIECE_VERSION,
        "ports": sp["ports"],
        "schema": K.SIDECAR_SCHEMA,
        "sensors": sp["sensors"],
    }


def sidecar_bytes(piece_id):
    import json
    return (json.dumps(sidecar(piece_id), indent=2, sort_keys=True, ensure_ascii=True) + "\n").encode()


# ---------------------------------------------------------------------------------------------
# Blender objects (same signatures as kitgeom's and kitblocks')
# ---------------------------------------------------------------------------------------------

TO_B = ((0.001, 0, 0, 0), (0, 0, -0.001, 0), (0, 0.001, 0, 0), (0, 0, 0, 1))


def build_render_object(piece_id, q, name, collection, material=None, depsgraph=None):
    from mathutils import Matrix
    ob = _boolean_mesh(piece_id, q, name, collection, collision=False)
    me = ob.data
    me.transform(Matrix(TO_B))
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


def build_socket_empties(piece_id, collection, parent=None, prefix=""):
    import bpy
    out = []
    for p in spec(piece_id)["ports"]:
        e = bpy.data.objects.new(f"{prefix}port_{p['index']}", None)
        e.empty_display_type = "SINGLE_ARROW"
        e.empty_display_size = 0.012
        e.location = K.to_blender(B.port_point_mm(p))
        if p["kind"] == "side":
            e.rotation_euler = (0.0, 0.0, math.radians(K.SOCKET_YAW[p["face"]]))
        else:
            e.rotation_euler = (0.0, math.radians(B.DROP_PITCH[p["face"]]), 0.0)
        for k in ("index", "face", "level", "class", "flow", "kind"):
            if k in p:
                e[k] = p[k]
        collection.objects.link(e)
        if parent is not None:
            e.parent = parent
        out.append(e)
    return out


def build_part_objects(piece_id, collection, parent, material=None):
    """Per part: empty `anim_<name>` at the pivot, rotated to rest about the axis, with mesh children
    `anim_<name>_lod{0,1,2}` (the part at 0 mdeg, in the empty's frame). Returns
    {lod: [mesh objects]} for the caller's budget and UV steps."""
    import bmesh
    import bpy
    from mathutils import Quaternion, Vector
    lod_objs = {lod: [] for lod in K.LODS}
    for part, _ in parts(piece_id):
        name = part["name"]
        pivot = [c / 1000.0 for c in part["pivot_um"]]
        e = bpy.data.objects.new(name, None)
        e.empty_display_type = "ARROWS"
        e.empty_display_size = 0.01
        e.location = K.to_blender(pivot)
        ax = Vector(K.to_blender(part["axis"])) * 1000.0
        e.rotation_mode = "QUATERNION"
        e.rotation_quaternion = Quaternion(ax, math.radians(part["rest_mdeg"] / 1000.0))
        e["axis"] = list(part["axis"])
        e["limits_mdeg"] = list(part["limits_mdeg"])
        e["rest_mdeg"] = part["rest_mdeg"]
        collection.objects.link(e)
        e.parent = parent
        for lod, q in K.LODS.items():
            segs = max(3, q["n_round"] // 2)
            solids = next(s for p, s in parts(piece_id, segs) if p["name"] == name)
            bm = bmesh.new()
            for v, f in solids:
                bv = [bm.verts.new(K.to_blender([p[k] - pivot[k] for k in range(3)])) for p in v]
                for face in _oriented(v, f):
                    bm.faces.new([bv[i] for i in face])
            bmesh.ops.triangulate(bm, faces=bm.faces, quad_method="FIXED", ngon_method="EAR_CLIP")
            me = bpy.data.meshes.new(f"{name}_lod{lod}")
            bm.to_mesh(me)
            bm.free()
            me.set_sharp_from_angle(angle=math.radians(35.0))
            if material is not None:
                me.materials.append(material)
            ob = bpy.data.objects.new(f"{name}_lod{lod}", me)
            collection.objects.link(ob)
            ob.parent = e
            lod_objs[lod].append(ob)
    return lod_objs
