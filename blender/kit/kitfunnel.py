"""Funnel catcher and drop catch pieces (owner request 2026-11-08), built against
docs/kit-contract.md (KIT_CONTRACT_VERSION 1).

Imports kitgeom and kitblocks and edits neither, so every other piece rebuilds byte-identically.

`blk_funnel_3x3_l2` is a 3 x 3 x 2 block (plan 180 mm square, 120 mm tall) holding a bowl of
revolution about the centre cell's axis. Two lead channels enter at L2 on opposite corner cells
(a pinwheel, so the piece is chiral: mirror spins the other way). Each runs straight from its port
to the point where it is tangent to the rim circle of radius RIM_R about the axis, and ends there.
Along the rim runs a gutter: the R14 channel's outer half revolved about the axis, centreline at
RIM_R, so at the tangent point the lead's section and the gutter's coincide and the marble rolls on
along the rim with no step. Inside the rim the floor falls from zero slope at RIM_R by a quadratic
to BOWL_R1, then a circular fillet turns it vertical into the 32 mm drop bore (contract §4) down to
the bottom drop port of the centre cell in the lower layer. Port levels at L2 take a block or a
rail (§5.3, §5.5) directly.

`blk_catch_l1` is the drop catch under the funnel: a top drop port with a straight 32 mm bore of
the funnel's segment count and phase (so the two bores meet ring to ring), the tower socket's
landing pad and descent (kitblocks `tower_floor`, chunk.rs `ChannelSlope::landing`), and an L1
tunnel exit on its -x face instead of the tower's socket.
"""
import math

import kitblocks as B
import kitgeom as K

PIECE_VERSION = 1
FUNNEL = "blk_funnel_3x3_l2"
CATCH = "blk_catch_l1"
BUILD = (CATCH, FUNNEL)

TOP = K.CELL
LEVEL = 2
F0 = K.LEVEL_STEP * LEVEL          # rim floor, anchor (upper) layer
AXIS = (60.0, -60.0)               # bowl axis (x, z): the centre cell of the 3 x 3
RIM_R = 60.0                       # gutter centreline: the lead channels are tangent to it
BOWL_R1 = 22.0                     # the quadratic ends here ...
BOWL_D = 40.0                      # ... this far below the rim floor
BOTTOM = -K.CELL                   # the funnel's lower layer bottom face
HOLE_R = B.HOLE_R                  # §4: 32 mm drop bore
LEAD_CLEAN = 0.0                   # lead station (x, anchor frame) before the bowl reaches it
COLLISION_N = 48                   # segments around the axis; divisible by 4, so the tangent points
                                   # and every yaw/mirror image land on lathe vertices
RENDER_N = {0: 96, 1: 48, 2: 24}
CATCH_RENDER_N = {0: 48, 1: 24, 2: 12}
QUAD_SEGS = {None: 8, 0: 24, 1: 10, 2: 5}
FILLET_SEGS = {None: 6, 0: 12, 1: 5, 2: 2}


def handles(piece_id):
    return piece_id in BUILD


def piece_version(piece_id):
    return PIECE_VERSION


def spec(piece_id):
    if piece_id == FUNNEL:
        ports = [B._side(0, (0, 0, 0), "-x", LEVEL, "in"), B._side(1, (2, 0, -2), "+x", LEVEL, "in"),
                 B._drop(2, (1, -1, -1), "bottom", "out")]
        fp = sorted([x, y, z] for x in range(3) for y in (-1, 0) for z in (-2, -1, 0))
        return {"id": piece_id, "shape": "funnel", "level": LEVEL, "ports": ports, "chiral": True,
                "family": "blk", "footprint": fp}
    if piece_id == CATCH:
        ports = [B._side(0, (0, 0, 0), "-x", 1, "out"), B._drop(1, (0, 0, 0), "top", "in")]
        return {"id": piece_id, "shape": "catch", "level": 1, "ports": ports, "chiral": False,
                "family": "blk", "footprint": [[0, 0, 0]]}
    raise ValueError(f"{piece_id}: not a funnel piece")


def cells(piece_id):
    return len(spec(piece_id)["footprint"])


def bounds_mm(piece_id):
    fp = spec(piece_id)["footprint"]
    lo = [min(c[k] for c in fp) for k in range(3)]
    hi = [max(c[k] for c in fp) for k in range(3)]
    return ((lo[0] * K.CELL - K.CELL / 2, lo[1] * K.CELL, lo[2] * K.CELL - K.CELL / 2),
            (hi[0] * K.CELL + K.CELL / 2, hi[1] * K.CELL + K.CELL, hi[2] * K.CELL + K.CELL / 2))


# ---------------------------------------------------------------------------------------------
# Bowl profile
# ---------------------------------------------------------------------------------------------

def bowl_floor():
    """[(r, h)] of the floor inside the rim, from (RIM_R, F0) exclusive to the bore: the quadratic
    (zero slope at the rim) then the fillet to vertical at HOLE_R, as functions of the sample counts.
    Returns (quad(n), fillet(n), fillet foot height)."""
    span = RIM_R - BOWL_R1
    slope = 2.0 * BOWL_D / span                   # dy/dr at BOWL_R1
    t1 = math.atan(1.0 / slope)                   # fillet angle swept to vertical
    rho = (BOWL_R1 - HOLE_R) / (1.0 - math.cos(t1))
    y1 = F0 - BOWL_D
    y_v = y1 - rho * math.sin(t1)
    assert y_v > BOTTOM + 10.0, y_v

    def quad(n):
        return [(BOWL_R1 + span * s, F0 - BOWL_D * (1.0 - s) ** 2)
                for s in (1.0 - k / n for k in range(1, n + 1))]

    def fillet(n):
        return [(HOLE_R + rho * (1.0 - math.cos(t)), y_v + rho * math.sin(t))
                for t in (t1 * (1.0 - k / n) for k in range(1, n + 1))]
    return quad, fillet, y_v


def funnel_profile(q, lod):
    """Lathe profile [(r, h)] top cap to bottom cap: the gutter (the channel's outer half about
    RIM_R, as the lead channel's own section), the bowl floor, the bore."""
    if q is None:
        ring = K.canonical_ring(B.um(F0))
        gutter = [(RIM_R + 14.0, TOP + K.CUT_ABOVE)] + [(RIM_R + u / 1000.0, v / 1000.0) for u, v in reversed(ring[9:])]
    else:
        half = K.render_half_profile(F0, q["n_round"], q["n_fillet"])
        gutter = [(RIM_R + u, v) for u, v in half[1:]]
    quad, fillet, _ = bowl_floor()
    return gutter + quad(QUAD_SEGS[lod]) + fillet(FILLET_SEGS[lod]) + [(HOLE_R, BOTTOM - K.EXT)]


def lead_stations(collision):
    """Both lead channels' stations (cx, cz, lx, lz, floor): port face to the tangent point."""
    # A station at LEAD_CLEAN keeps the lead's first stretch a plain straight channel (its facet
    # strips whole quads, as on a straight), so the chunk merge's flat simplification removes the
    # seam's ring vertices; past it the bowl cuts the lead's inner half.
    ins = (LEAD_CLEAN,) if collision else (-K.CELL / 2 + K.INSET, LEAD_CLEAN)
    s0 = B.extend(B.straight_x(-30.0, AXIS[0], lambda x: F0, 1, extra_x=ins), True, False)
    s1 = [(2 * AXIS[0] - x, 2 * AXIS[1] - z, -lx, -lz, f) for x, z, lx, lz, f in s0]
    return s0, s1


def cutters(piece_id, q, collision, lod):
    out = []
    if piece_id == FUNNEL:
        prof = B.open_profile(collision, q)
        for sts in lead_stations(collision):
            out.append(lambda bm, sts=sts: B.sweep(bm, sts, prof))
        n = COLLISION_N if collision else RENDER_N[lod]
        out.append(lambda bm: B.lathe(bm, funnel_profile(q, lod), n, centre=AXIS))
        return out
    # catch: the tower socket's landing (kitblocks tower_floor) to an L1 tunnel at the -x face
    f0 = K.LEVEL_STEP
    x_end, x0, x_pad = HOLE_R + 1.0, -30.0, -B.TOWER_PAD
    fl, rise = B.tower_floor(f0, x_end, x_pad, x0)
    tprof = B.tunnel_profile(collision, q)
    if collision:
        sts = B.straight_x(x0, x_pad, fl, 3, extra_x=(x_end,))
    else:
        sts = B.straight_x(x0, x_pad, fl, max(2, q["path"]["slope_gentle"] - 1), extra_x=(x_end, x0 + K.INSET))
    out.append(lambda bm: B.sweep(bm, B.extend(sts, True, False), tprof))
    n = COLLISION_N if collision else CATCH_RENDER_N[lod]
    out.append(lambda bm: B.lathe(bm, [(HOLE_R, TOP + K.CUT_ABOVE), (HOLE_R, f0 + rise + 10.0)], n))
    return out


# ---------------------------------------------------------------------------------------------
# Boolean, collision, sidecar
# ---------------------------------------------------------------------------------------------

def _boolean_mesh(piece_id, q, name, collection, collision, lod=None):
    """Body box (bevelled for render) minus every cutter (EXACT), contract mm, triangulated."""
    import bmesh
    import bpy
    lo, hi = bounds_mm(piece_id)
    if not collision:
        lo, hi = tuple(c + K.INSET for c in lo), tuple(c - K.INSET for c in hi)
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    bmesh.ops.scale(bm, vec=[hi[k] - lo[k] for k in range(3)], verts=bm.verts)
    bmesh.ops.translate(bm, vec=[(hi[k] + lo[k]) / 2 for k in range(3)], verts=bm.verts)
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
    for i, build in enumerate(cutters(piece_id, q, collision, lod)):
        bm = bmesh.new()
        build(bm)
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        tme = bpy.data.meshes.new(f"{name}_cut{i}")
        bm.to_mesh(tme)
        bm.free()
        ob = bpy.data.objects.new(f"{name}_cut{i}", tme)
        collection.objects.link(ob)
        mod = body.modifiers.new(f"cut{i}", "BOOLEAN")
        mod.operation = "DIFFERENCE"
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
    if not collision:  # slivers (zero UV area, no tangent: a glTF validator error)
        slivers = [f for f in bm.faces if f.calc_area() < B.SLIVER_MM2]
        bmesh.ops.delete(bm, geom=slivers, context="FACES_ONLY")
        bmesh.ops.delete(bm, geom=[v for v in bm.verts if not v.link_faces], context="VERTS")
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.to_mesh(result)
    bm.free()
    return body


def _open_port_faces(piece_id, verts, tris):
    """Open each channel side port's cell face (§5.4, as kitblocks). On the multi-cell funnel the
    boolean fans the whole body side from far corners through the port ring, so a cap triangle
    reaches outside the port's cell square: every triangle in the side's plane is dropped and the
    side is rebuilt (ear clipping, integer µm) as its rectangle minus the port's cell square, which
    sits on the side's top edge. New vertices follow the kept ones."""
    lo, hi = bounds_mm(piece_id)
    lo, hi = [B.um(c) for c in lo], [B.um(c) for c in hi]
    for p in spec(piece_id)["ports"]:
        if not (p["kind"] == "side" and p["class"] == "channel"):
            continue
        ax, plane, _, _ = B._expected_rings(p)
        other = 2 if ax == 0 else 0
        on_plane = [t for t in tris if all(verts[i][ax] == plane for i in t)]
        if not on_plane:
            continue
        a, b, c = (verts[i] for i in on_plane[0])
        e1, e2 = [b[k] - a[k] for k in range(3)], [c[k] - a[k] for k in range(3)]
        n_ax = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]][ax]
        u0, y0 = p["cell"][other] * 60000, p["cell"][1] * 60000
        s0, s1, sy = u0 - 30000, u0 + 30000, y0
        r0, r1, top, bot = lo[other], hi[other], hi[1], lo[1]
        if y0 + 60000 != top:
            raise RuntimeError(f"{piece_id}: port {p['index']} is not in the top layer")
        if s0 <= r0 and s1 >= r1 and sy <= bot:  # the side is the port's cell face: open it all
            kept = [t for t in tris if t not in on_plane]
            used = sorted({i for t in kept for i in t})
            new = {old: k for k, old in enumerate(used)}
            verts, tris = [verts[i] for i in used], [tuple(new[i] for i in t) for t in kept]
            continue
        poly = [(r0, bot), (r1, bot), (r1, top)]
        poly += [(s1, top), (s1, sy), (s0, sy), (s0, top)]
        poly += [(r0, top)]
        clean = []
        for q in poly:
            if not clean or clean[-1] != q:
                clean.append(q)
        if clean[0] == clean[-1]:
            clean.pop()
        changed = True
        while changed:  # drop collinear corners (the square may share a corner with the side)
            changed = False
            for k in range(len(clean)):
                o, m, n = clean[k - 1], clean[k], clean[(k + 1) % len(clean)]
                if (m[0] - o[0]) * (n[1] - o[1]) - (m[1] - o[1]) * (n[0] - o[0]) == 0:
                    clean.pop(k)
                    changed = True
                    break
        kept = [t for t in tris if t not in on_plane]
        used = sorted({i for t in kept for i in t})
        new = {old: k for k, old in enumerate(used)}
        nv = [verts[i] for i in used]
        at = {}
        for k, v in enumerate(nv):
            if v[ax] == plane:
                at[(v[other], v[1])] = k
        idx = []
        for u, y in clean:
            if (u, y) not in at:
                at[(u, y)] = len(nv)
                pt = [0, y, 0]
                pt[ax], pt[other] = plane, u
                nv.append(tuple(pt))
            idx.append(at[(u, y)])
        nt = [tuple(new[i] for i in t) for t in kept]
        for i, j, k in B._ear_clip(clean):
            t = (idx[i], idx[j], idx[k])
            va, vb, vc = (nv[q] for q in t)
            f1, f2 = [vb[m] - va[m] for m in range(3)], [vc[m] - va[m] for m in range(3)]
            nn = [f1[1] * f2[2] - f1[2] * f2[1], f1[2] * f2[0] - f1[0] * f2[2], f1[0] * f2[1] - f1[1] * f2[0]][ax]
            nt.append(t if (nn > 0) == (n_ax > 0) else (t[0], t[2], t[1]))
        verts, tris = nv, nt
    return verts, tris


_COLLISION_CACHE = {}


def collision_mesh(piece_id):
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
    verts, tris = _open_port_faces(piece_id, verts, tris)
    for p in spec(piece_id)["ports"]:
        if p["kind"] != "side":
            continue
        ax, plane, open_ring, tunnel = B._expected_rings(p)
        other = 2 if ax == 0 else 0
        u0 = p["cell"][other] * 60000
        floor = p["cell"][1] * 60000 + 15000 * p["level"]
        on = [v for v in verts if v[ax] == plane and abs(v[other] - u0) <= 14000]
        got_open = {(v[other], v[1]) for v in on if p["cell"][1] * 60000 <= v[1] <= p["cell"][1] * 60000 + 60000}
        got_tun = {(v[other], v[1]) for v in on if floor <= v[1] <= floor + 34000}
        if got_open != open_ring and got_tun != tunnel:
            raise RuntimeError(f"{piece_id}: port {p['index']} collision ring is not canonical: "
                               f"{sorted(got_open)[:6]} ...")
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
        "footprint": sp["footprint"],
        "id": piece_id,
        "kit_contract": K.KIT_CONTRACT_VERSION,
        "piece_version": piece_version(piece_id),
        "ports": sp["ports"],
        "schema": K.SIDECAR_SCHEMA,
        "sensors": [],
    }


def sidecar_bytes(piece_id):
    import json
    return (json.dumps(sidecar(piece_id), indent=2, sort_keys=True, ensure_ascii=True) + "\n").encode()


# ---------------------------------------------------------------------------------------------
# Blender objects (same signatures as kitblocks')
# ---------------------------------------------------------------------------------------------

def _lod_of(q):
    for lod, v in K.LODS.items():
        if v is q:
            return lod
    return 0  # the high-poly bake source uses LOD0's ring counts


def build_render_object(piece_id, q, name, collection, material=None, depsgraph=None):
    from mathutils import Matrix
    ob = _boolean_mesh(piece_id, q, name, collection, collision=False, lod=_lod_of(q))
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
