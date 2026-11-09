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
`anim_flap` and toggle `anim_rocker` (both a switch blade at the fork), one-way gate `anim_flap`, one-way hole `anim_trapdoor`,
rope-curtain arch `anim_rope_0` .. `anim_rope_7`. The finish cup and the checkpoint arch have no
moving part, only sensors. The stepper elevators (kitstepper) have one slide part, `anim_slats`.
"""
import math

import kitblocks as B
import kitgeom as K
import kitstepper as S

PIECE_VERSION = 1
# Pieces whose geometry changed since version 1 (contract §10: the id stays, the version bumps).
# 2026-11-08: mech_hole_oneway_l1 2, bore-filling 90° trapdoor and open port faces;
# mech_gate_oneway_l1 2, open port faces.
# 2026-11-08: mech_hole_oneway_l1 3, trapdoor top dished to the channel's half-round and no bowl
# over the bore, so the floor stays the channel section across the door.
# 2026-11-08: mech_finish, mech_hopper, mech_splitter_l3, mech_toggle_l3, mech_checkpoint_l1 2,
# open port faces.
PIECE_VERSIONS = {"mech_gate_oneway_l1": 3, "mech_hole_oneway_l1": 3, "mech_finish": 2, "mech_hopper": 3,
                  "mech_splitter_l3": 2, "mech_toggle_l3": 2, "mech_checkpoint_l1": 2}
BUILD = ("mech_finish", "mech_hopper", "mech_splitter_l3", "mech_toggle_l3", "mech_checkpoint_l1",
         "mech_gate_oneway_l1", "mech_hole_oneway_l1", "mech_arch_rope_l1") + tuple(S.STEPPERS)

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
HOLE_R = 16.0
# Trapdoor: fills the 32 mm bore, so a marble rolling over it meets no gap at the bore edge, and
# opens to 90 degrees, hanging along the bore's +x wall with 30.5 mm clear for a falling marble
# (a 60 degree door left 16 mm between its tip and the far wall, and the marble wedged there).
TRAP_R = HOLE_R
TRAP_T = 3.0
# The bore's top above the floor. The door's top follows the half-round up to it, and the
# half-round is 21.4 mm wide there, so a 16 mm marble drops through the open door (1 mm, where it
# is 10.4 mm wide, held the marble up on the walls).
TRAP_CAP = 5.0
TRAP_OPEN_MDEG = 90000
# Rope-curtain arch (PRD P2 item 18): a bridge over the middle 2 * ARCH_X mm of the cell's length,
# its underside 35 mm above the L1 floor (the §5.1 overhead rule asks 34), so the piece stays 1 cell
# tall (an L2 or L3 arch needs 2 cells, §16 Q13). Two rows of four ropes hang from it into the
# channel, each hinged about +Z at its top. A rope's sidecar collision is its whole strand, ready
# for physical ropes; the cosmetic sim model gives the ropes no collider (behaviour.rs).
ARCH_X = 12.0
ARCH_Y = 50.0
ROPE_ROWS_X = (-6.0, 6.0)
ROPE_Z = (-10.5, -4.5, 4.5, 10.5)
ROPE_R = 1.0
ROPE_BOTTOM = 22.0      # 2.3 mm above the round floor under the outer ropes
KNOT_R = 1.6
KNOT_H = 3.0
ROPE_LIMIT_MDEG = 45000  # at 45 degrees a rope's tip stays inside the inset cell


def piece_version(piece_id):
    return PIECE_VERSIONS.get(piece_id, PIECE_VERSION)


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
    if S.handles(piece_id):
        return S.spec(piece_id)
    shape = piece_id[len("mech_"):].rsplit("_l", 1)[0] if piece_id[-3:-1] == "_l" else piece_id[len("mech_"):]
    if shape == "hopper":      # bowl holding the queue, release gate on the exit channel
        level, ports = 1, [_side(0, "+x", 1, "out")]
        # The gate's 34 mm sweep reaches 16 mm past the +x face, so `release` does too: a marble
        # leaves it (centre 8 mm past the box, at x >= 48 mm) only once it is clear of the closing gate.
        sensors = [_box_sensor("release", (28000, 27500, 0), (12000, 12500, 14000))]
    elif shape == "finish":    # channel into a deep collector cup
        level, ports = 3, [_side(0, "-x", 3, "in")]
        # up to the cell top: the cup holds 10 standard marbles, the top ones with centres 47-53 mm up
        sensors = [_box_sensor("cup", (6000, 37000, 0), (14000, 23000, 14000))]
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
    elif shape == "arch_rope":  # one sensor per rope row; together they span the arch
        level, ports = 1, [_side(0, "-x", 1, "both"), _side(1, "+x", 1, "both")]
        h = int(ARCH_X * 500)
        sensors = [_box_sensor("curtain_nx", (-h, 27500, 0), (h, 12500, 14000)),
                   _box_sensor("curtain_px", (h, 27500, 0), (h, 12500, 14000))]
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
    return S.cells(piece_id) if S.handles(piece_id) else 1


def bounds_mm(piece_id):
    if S.handles(piece_id):
        return S.bounds_mm(piece_id)
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
    if S.handles(piece_id):
        return S.cutters(piece_id, collision, B, q)
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
    elif shape == "arch_rope":
        sts = B.straight_x(-30.0, 30.0, lambda x: f0, 1, extra_x=ins)
        cut(lambda bm: B.sweep(bm, B.extend(sts, True, True), oprof))
        out.append(("UNION", _box_builder((-ARCH_X, ARCH_Y, -K.R - 2.0), (ARCH_X, top, K.R + 2.0))))
    elif shape == "hole_oneway":
        # No bowl over the bore: the channel's half-round runs on across the cell and the trapdoor
        # fills the floor the bore cuts out of it. With a flat 18 mm bowl floor and a flat door, a
        # marble rolled over from +x 3.6-3.8 mm off centre drifted across the door and struck the
        # bowl's rim where the half-round wall resumed (vx -311 to -45 mm/s), and stalled
        # (2 of 120 crossings, measured 2026-11-08).
        sts = B.straight_x(-30.0, 30.0, lambda x: f0, 1, extra_x=ins)
        cut(lambda bm: B.sweep(bm, B.extend(sts, True, True), oprof))
        cut(lambda bm: B.lathe(bm, [(HOLE_R, f0 + TRAP_CAP), (HOLE_R, -K.EXT)], circ))
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
    if S.handles(piece_id) and not collision:
        # The stepper's stepped wall tops and stub lips leave sliver triangles (zero UV area, so no
        # tangent, a glTF validator error). Dropping them leaves no visible gap.
        slivers = [f for f in bm.faces if f.calc_area() < S.SLIVER_MM2]
        bmesh.ops.delete(bm, geom=slivers, context="FACES_ONLY")
        bmesh.ops.delete(bm, geom=[v for v in bm.verts if not v.link_faces], context="VERTS")
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.to_mesh(result)
    bm.free()
    return body


_COLLISION_CACHE = {}


def _flat(a, b, c):
    """Whether three µm points are collinear: a zero-area triangle the boolean can leave across the
    stepper's evenly stepped wall tops, which has no normal (kit-check cannot place it in a cell)."""
    e1, e2 = [b[k] - a[k] for k in range(3)], [c[k] - a[k] for k in range(3)]
    return (e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]) == (0, 0, 0)


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
        if len(set(t)) == 3 and not (S.handles(piece_id) and _flat(*(verts[i] for i in t))):
            tris.append(t)
    bpy.data.objects.remove(ob)
    bpy.data.meshes.remove(me)
    bpy.context.view_layer.update()
    for p in spec(piece_id)["ports"]:
        if p["kind"] != "side":
            continue
        ax, plane, open_ring, tunnel = B._expected_rings(p)
        other = 2 if ax == 0 else 0
        base = 60000 * p["cell"][1]
        floor = base + 15000 * p["level"]
        on = [v for v in verts if v[ax] == plane and abs(v[other]) <= 14000]
        got_open = {(v[other], v[1]) for v in on if base <= v[1] <= base + 60000}
        got_tun = {(v[other], v[1]) for v in on if floor <= v[1] <= floor + 34000}
        if got_open != open_ring and got_tun != tunnel:
            raise RuntimeError(f"{piece_id}: port {p['index']} collision ring is not canonical: "
                               f"{sorted(got_open)[:6]} ...")
    if piece_id in OPEN_PORT_FACES:
        verts, tris = open_port_faces(piece_id, verts, tris)
    _COLLISION_CACHE[piece_id] = (verts, tris)
    return verts, tris


# Pieces whose channel port faces are left open in the collision mesh (§5.4), as kitblocks does for
# the P2-T2 blocks. The boolean's cap on a port face meets the mating straight's floor at the port
# ring as a convex 90° edge: crossing a straight -> mech_hole seam threw a 0.42 m/s marble 0.24 m/s
# up, and the hops that followed stalled it on the closed trapdoor (measured 2026-11-08). At 0.3 m/s
# the caps threw marbles 115-147 mm/s up at the finish, hopper, fork and checkpoint seams.
OPEN_PORT_FACES = BUILD


def open_port_faces(piece_id, verts, tris):
    """Drop the cap triangles on each channel side port's cell face. Unused vertices are dropped,
    the rest keep their order."""
    faces = []
    for p in spec(piece_id)["ports"]:
        if p["kind"] == "side" and p["class"] == "channel":
            ax, plane, _, _ = B._expected_rings(p)
            faces.append((ax, plane, 2 if ax == 0 else 0))

    def on_face(t):
        return any(all(verts[i][ax] == plane and abs(verts[i][other]) <= 30000 for i in t)
                   for ax, plane, other in faces)

    kept = [t for t in tris if not on_face(t)]
    used = sorted({i for t in kept for i in t})
    new = {old: k for k, old in enumerate(used)}
    return [verts[i] for i in used], [tuple(new[i] for i in t) for t in kept]


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
BLADE_UNDER = [(-2.0, 6.0), (-10.0, 1.9), (-18.0, 1.0), (-26.0, 4.0)]
HUB_BASE = 8.2   # the ridge between the troughs reaches 7.7 mm within the hub radius
FORK_LIMIT = 25000


def _switch_blade(shape, f0, segs):
    """Splitter: a 2 mm blade on a slim post, resting at its +Z-tip limit. Toggle: the same blade on
    a round hub, resting at its -Z-tip limit (side 0, the first marble's default)."""
    half = FLAP_T
    tip = FORK_PIVOT_X - BLADE_LEN
    outline = [(x, f0 + y) for x, y in BLADE_UNDER] + [(tip, f0 + BLADE_TOP), (FORK_PIVOT_X, f0 + BLADE_TOP)]
    blade = _solid_prism(outline, "z", -half, half)
    y0, y1 = f0 + HUB_BASE, f0 + BLADE_TOP
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


def _trapdoor(f0, n):
    """The trapdoor: the `n`-gon disc of TRAP_R from TRAP_T under the floor up to the channel's
    canonical half-round section (the collision ring, contract §5.4), capped at the bore's top
    TRAP_CAP above the floor. Convex slices between the breakpoints across z (the disc's vertices,
    the ring's vertices under the cap and the ring's crossings of the cap), so each slice's top and
    sides are planar."""
    disc = [(TRAP_R * math.cos(2 * math.pi * k / n), TRAP_R * math.sin(2 * math.pi * k / n)) for k in range(n)]
    ring = [(u / 1000.0, v / 1000.0 - f0) for u, v in K.canonical_ring(B.um(f0))[1:-1]]
    if n < 16:  # render LODs under the collision's 16-gon: every other ring vertex, for the budget
        ring = ring[::2]

    def top(z):  # height of the dished top over the floor at z
        for (z0, y0), (z1, y1) in zip(ring, ring[1:]):
            if z0 <= z <= z1:
                y = y0 if z1 == z0 else y0 + (y1 - y0) * (z - z0) / (z1 - z0)
                return min(y, TRAP_CAP)
        return TRAP_CAP

    def half(z):  # the disc's half-width along x at z
        best = 0.0
        for (x0, z0), (x1, z1) in zip(disc, disc[1:] + disc[:1]):
            if min(z0, z1) <= z <= max(z0, z1) and z0 != z1:
                best = max(best, abs(x0 + (x1 - x0) * (z - z0) / (z1 - z0)))
        return best

    cuts = {round(z, 6) for _, z in disc}
    cuts |= {round(z, 6) for z, y in ring if y < TRAP_CAP}
    for (z0, y0), (z1, y1) in zip(ring, ring[1:]):
        if (y0 - TRAP_CAP) * (y1 - TRAP_CAP) < 0:
            cuts.add(round(z0 + (z1 - z0) * (TRAP_CAP - y0) / (y1 - y0), 6))
    zs = sorted(z for z in cuts if -TRAP_R <= z <= TRAP_R)
    solids = []
    for za, zb in zip(zs, zs[1:]):
        v, index = [], {}

        def at(x, y, z):
            key = (round(x, 6), round(y, 6), round(z, 6))
            if key not in index:
                index[key] = len(v)
                v.append(key)
            return index[key]

        sec = []
        for z in (za, zb):
            w, t, b = half(z), f0 + top(z), f0 - TRAP_T
            sec.append([at(-w, b, z), at(w, b, z), at(w, t, z), at(-w, t, z)])
        a, c = sec
        faces = [tuple(a), tuple(reversed(c)), (a[0], a[1], c[1], c[0]), (a[3], a[2], c[2], c[3]),
                 (a[1], a[2], c[2], c[1]), (a[0], a[3], c[3], c[0])]
        clean = []
        for f in faces:
            g = []
            for i in f:
                if i not in g:
                    g.append(i)
            if len(g) >= 3:
                clean.append(tuple(g))
        solids.append((v, clean))
    return solids


def parts(piece_id, segs=None):
    """[(part dict without collision, [convex solids])], sorted by name. `segs` sets the curved
    outlines' segment count (collision: 8)."""
    if S.handles(piece_id):
        return S.parts(piece_id)
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
    if shape == "arch_rope":  # each rope: a strand from the arch underside and a knot at its end
        n = max(3, s)
        out = []
        for i, (x, z) in enumerate((x, z) for x in ROPE_ROWS_X for z in ROPE_Z):
            def ring(r, x=x, z=z):
                return [(x + r * math.cos(2 * math.pi * k / n), z + r * math.sin(2 * math.pi * k / n))
                        for k in range(n)]
            if n == 3:  # LOD2: one plain strand keeps all eight ropes inside the 120-tri part budget
                solids = [_solid_prism(ring(ROPE_R), "y", ROPE_BOTTOM, ARCH_Y)]
            else:
                solids = [_solid_prism(ring(ROPE_R), "y", ROPE_BOTTOM + KNOT_H, ARCH_Y),
                          _solid_prism(ring(KNOT_R), "y", ROPE_BOTTOM, ROPE_BOTTOM + KNOT_H)]
            out.append(({"name": f"anim_rope_{i}", "pivot_um": [B.um(x), B.um(ARCH_Y), B.um(z)],
                         "axis": [0, 0, 1], "rest_mdeg": 0,
                         "limits_mdeg": [-ROPE_LIMIT_MDEG, ROPE_LIMIT_MDEG]}, solids))
        return out
    if shape == "hole_oneway":  # trapdoor over the bore, hinged at its +x edge; drops the -x edge
        return [({"name": "anim_trapdoor", "pivot_um": [B.um(TRAP_R), B.um(f0 - TRAP_T / 2), 0], "axis": [0, 0, 1],
                  "rest_mdeg": 0, "limits_mdeg": [0, TRAP_OPEN_MDEG]}, _trapdoor(f0, max(8, 2 * s)))]
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
        "piece_version": piece_version(piece_id),
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
        ax = Vector(K.to_blender(part["axis"])) * 1000.0
        e.rotation_mode = "QUATERNION"
        e["axis"] = list(part["axis"])
        if part.get("joint") == "slide":  # at the pivot moved rest_um along the axis, unrotated
            e.location = K.to_blender([pivot[k] + part["axis"][k] * part["rest_um"] * 1e-6 for k in range(3)])
            e.rotation_quaternion = Quaternion((1.0, 0.0, 0.0, 0.0))
            e["joint"] = "slide"
            e["limits_um"] = list(part["limits_um"])
            e["rest_um"] = part["rest_um"]
        else:
            e.location = K.to_blender(pivot)
            e.rotation_quaternion = Quaternion(ax, math.radians(part["rest_mdeg"] / 1000.0))
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
