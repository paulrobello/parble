"""P2-T4 rail family pieces, built against docs/kit-contract.md (KIT_CONTRACT_VERSION 1).

Imports kitgeom and kitblocks and edits neither, so every block and mechanism piece rebuilds
byte-identically.

A rail is the §5.3 round trough (the §5.1 channel inside a 4 mm wall, 2 mm lip round on top, R18
outer bottom) swept along a plan centreline at one floor height. Unlike the block family there is no
boolean: the render mesh is the closed section swept directly, and the collision mesh is built
ring by ring in integer µm, kitgeom style, so every port ring is exact by construction (§5.4: the
17 canonical half-round vertices plus the wall tops at floor + 18 mm). Port ends stay open, as on
the blocks. The collision keeps the outer shell, so a marble dropped onto a rail does not fall
through it.

Pieces (§9, §10):
- `rail_straight_l1`, `rail_straight_l2`, `rail_straight_l3`: 1 cell, port to port.
- Straight, curve and S-rail each come at L1, L2 and L3 (the editor shows them as one tile with a
  height picker). An L3 rail's lip is 63 mm above its cell bottom, so its footprint adds the cell
  layer above every cell (§5.3); L1 and L2 fit one layer.
- `rail_curve90_r90_l<n>`: 2 × 2, a quarter circle of radius 90 mm about the footprint corner
  (-30, -90) mm, from the -x face of the anchor cell to the -z face of cell (1, 0, -1).
- `rail_s_1x3_l<n>`: two opposed arcs of radius 150 mm turning 36.87° each (sin 0.6, cos 0.8),
  60 mm lateral offset over 180 mm, from the -x face of the anchor to the +x face of cell (2, 0, -1).
  §9 lists the footprint as 2 × 3 (lateral × length); in the piece frame it is 3 cells in x by 2
  in z.
- `rail_joint_l1`: a straight rail whose +x end is a 10 mm spigot (§5.5) that enters the socket
  of `blk_tower_socket_l1`. Its collision runs to the port plane 10 mm past its own face, where
  the tower's channel starts, and the sidecar declares the spigot cylinder.

Rails carry no posts (§16 Q18, decided in P2-T4): a rail is held up by the blocks it joins and by
the support pieces under it.
"""
import math

import kitblocks as B
import kitgeom as K

PIECE_VERSION = 1
# Each rail shape at every level a side port can use (§5.3): L1 and L2 fit one cell layer, L3 adds
# the layer above to the footprint. The first five ids are the P2-T4 proof pieces.
LEVELS = (1, 2, 3)
BUILD = ("rail_straight_l1", "rail_straight_l2", "rail_curve90_r90_l2", "rail_s_1x3_l2", "rail_joint_l1",
         "rail_straight_l3", "rail_curve90_r90_l1", "rail_curve90_r90_l3", "rail_s_1x3_l1", "rail_s_1x3_l3")

WALL = 4.0            # §5.3 wall thickness
LIP_H = 18.0          # §5.3 lip height above the floor
LIP_R = 2.0           # §5.3 lip full round
OUTER_R = K.R + WALL  # 18 mm outer bottom radius
CURVE_R = 90.0        # §9 rail curve centreline radius
S_R = 150.0           # §9 S-rail arc radius
S_DEG = math.degrees(math.atan2(0.6, 0.8))   # 36.87° (sin 0.6, cos 0.8)
SPIGOT_LEN = 10.0     # §5.5 spigot past the rail's own face
SOCKET_INSET_UM = 10000
# §5.5 spigot volume radius about the bore axis (floor + 14 mm). The render section reaches
# sqrt(18² + 2²) = 18.111 mm where the lip round meets the outer wall (1.889 mm clear of the 20 mm
# bore); the collision section keeps a sharp lip corner at (18, 18) mm, sqrt(18² + 4²) = 18.439 mm
# (1.561 mm clear). The volume covers both, rounded up to whole µm.
SPIGOT_RADIUS_UM = math.ceil(1000.0 * math.hypot(OUTER_R, LIP_H - K.R))

# §12: rail budget per cell of centreline length; centreline lengths in mm.
BUDGET_PER_CELL = {0: 1500, 1: 500, 2: 120}
CENTRELINE_MM = {"straight": K.CELL, "curve90_r90": CURVE_R * math.pi / 2,
                 "s_1x3": 2 * S_R * math.radians(S_DEG), "joint": K.CELL + SPIGOT_LEN}

# Render LODs: half-round segments (§12: 32 / 12 / 6), lip-round segments, path segments per shape.
LODS = {
    0: {"n_round": 32, "n_lip": 6, "path": {"straight": 1, "joint": 1, "curve90_r90": 20, "s_1x3": 28}},
    1: {"n_round": 12, "n_lip": 2, "path": {"straight": 1, "joint": 1, "curve90_r90": 10, "s_1x3": 16}},
    2: {"n_round": 6, "n_lip": 1, "path": {"straight": 1, "joint": 1, "curve90_r90": 6, "s_1x3": 8}},
}
HIGH = {"n_round": 64, "n_lip": 12, "path": {"straight": 1, "joint": 1, "curve90_r90": 40, "s_1x3": 56}}
COLLISION_PATH = {"straight": 1, "joint": 1, "curve90_r90": 16, "s_1x3": 24}
COLLISION_SEGS = 16   # §5.4 half-round segments, inner and outer


def handles(piece_id):
    return piece_id.startswith("rail_")


def _side(i, cell, face, level, cls="channel", inset=0):
    return {"cell": list(cell), "class": cls, "face": face, "flow": "both", "index": i, "inset_um": inset,
            "kind": "side", "level": level}


def spec(piece_id):
    """Ports, shape, level and footprint for a rail piece id (§5.3, §5.5, §9, §10)."""
    if not K.ID_RE.match(piece_id) or len(piece_id) > 48:
        raise ValueError(f"{piece_id}: not a §10 id")
    legal = {f"rail_{shape}_l{lv}": (shape, lv) for shape in ("straight", "curve90_r90", "s_1x3") for lv in LEVELS}
    legal["rail_joint_l1"] = ("joint", 1)
    if piece_id not in legal:
        raise ValueError(f"{piece_id}: not a rail piece this generator builds")
    shape, level = legal[piece_id]
    o = (0, 0, 0)
    if shape == "straight":
        ports, fp = [_side(0, o, "-x", level), _side(1, o, "+x", level)], [[0, 0, 0]]
    elif shape == "joint":
        ports = [_side(0, o, "-x", level), _side(1, o, "+x", level, cls="spigot", inset=SOCKET_INSET_UM)]
        fp = [[0, 0, 0]]
    elif shape == "curve90_r90":
        ports = [_side(0, o, "-x", level), _side(1, (1, 0, -1), "-z", level)]
        fp = [[x, 0, z] for x in (0, 1) for z in (-1, 0)]
    else:
        ports = [_side(0, o, "-x", level), _side(1, (2, 0, -1), "+x", level)]
        fp = [[x, 0, z] for x in (0, 1, 2) for z in (-1, 0)]
    if level == 3:   # §5.3: the L3 lip (63 mm) rises into the cell layer above
        fp += [[x, 1, z] for x, _, z in fp]
    return {"id": piece_id, "shape": shape, "level": level, "ports": ports,
            "chiral": shape in ("curve90_r90", "s_1x3"), "family": "rail", "footprint": sorted(fp)}


def cells(piece_id):
    return len(spec(piece_id)["footprint"])


def bounds_mm(piece_id):
    """(lo, hi) contract-frame mm of the footprint's bounding box; the joint's spigot reaches
    SPIGOT_LEN past its +x face (hi grows by the seam inset too, which the render check removes)."""
    fp = spec(piece_id)["footprint"]
    lo = [min(c[k] for c in fp) for k in range(3)]
    hi = [max(c[k] for c in fp) for k in range(3)]
    lo = (lo[0] * K.CELL - K.CELL / 2, lo[1] * K.CELL, lo[2] * K.CELL - K.CELL / 2)
    hi = (hi[0] * K.CELL + K.CELL / 2, hi[1] * K.CELL + K.CELL, hi[2] * K.CELL + K.CELL / 2)
    if spec(piece_id)["shape"] == "joint":
        hi = (hi[0] + SPIGOT_LEN + K.INSET, hi[1], hi[2])
    return lo, hi


def budget(piece_id, lod):
    """§12 render triangle budget: per cell of centreline length."""
    return int(BUDGET_PER_CELL[lod] * CENTRELINE_MM[spec(piece_id)["shape"]] / K.CELL)


def spigot(piece_id):
    """§11 spigot cylinder (integer µm, piece-local): axis, the centre of its base on the rail's
    own face, length past the face and radius. None for pieces without a spigot."""
    sp = spec(piece_id)
    if sp["shape"] != "joint":
        return None
    return {"axis": [1, 0, 0], "center_um": [30000, 15000 * sp["level"] + 14000, 0],
            "length_um": int(SPIGOT_LEN * 1000), "radius_um": SPIGOT_RADIUS_UM}


# ---------------------------------------------------------------------------------------------
# Centreline. A station is (cx, cz, lx, lz): plan point and lateral (+u) unit vector, which is the
# direction of travel turned +90° in plan: (lx, lz) = (-dz, dx).
# ---------------------------------------------------------------------------------------------

def _arc(centre, r, a0, a1, n):
    s = 1.0 if a1 > a0 else -1.0
    out = []
    for k in range(n + 1):
        a = math.radians(a0 + (a1 - a0) * k / n)
        c, si = math.cos(a), math.sin(a)
        dx, dz = -si * s, c * s
        out.append((centre[0] + r * c, centre[1] + r * si, -dz, dx))
    return out


def stations(shape, n):
    """Centreline from port 0 to the far end with `n` path segments (per arc for the S-rail, total
    otherwise). The joint runs on to the spigot end. Ends sit exactly on the port planes."""
    if shape == "straight":
        return [(-30.0 + 60.0 * k / n, 0.0, 0.0, 1.0) for k in range(n + 1)]
    if shape == "joint":
        return [(-30.0, 0.0, 0.0, 1.0), (30.0, 0.0, 0.0, 1.0), (30.0 + SPIGOT_LEN, 0.0, 0.0, 1.0)]
    if shape == "curve90_r90":
        sts = _arc((-30.0, -CURVE_R), CURVE_R, 90.0, 0.0, n)
        sts[0], sts[-1] = (-30.0, 0.0, 0.0, 1.0), (60.0, -90.0, 1.0, 0.0)
        return sts
    half = max(1, n // 2)
    a = _arc((-30.0, -S_R), S_R, 90.0, 90.0 - S_DEG, half)
    b = _arc((150.0, 90.0), S_R, 270.0 - S_DEG, 270.0, half)
    a[0], b[-1] = (-30.0, 0.0, 0.0, 1.0), (150.0, -60.0, 0.0, 1.0)
    return a + b[1:]


# ---------------------------------------------------------------------------------------------
# Collision (integer µm, contract frame)
# ---------------------------------------------------------------------------------------------

def collision_ring(floor_um):
    """Closed collision section as (u, v) µm: the §5.4 rail port ring (left wall top, 17 canonical
    half-round vertices, right wall top at floor + 18 mm), then the right lip's outer corner, the
    outer R18 bottom from u = +18 to u = -18 in 11.25° steps, and the left lip's outer corner."""
    inner = K.canonical_ring(floor_um, top_um=floor_um + 18000)
    outer = []
    for i in range(COLLISION_SEGS + 1):
        th = math.radians(90.0 - 11.25 * i)
        outer.append((K.round_half_away(18000.0 * math.sin(th)),
                      floor_um + 14000 - K.round_half_away(18000.0 * math.cos(th))))
    return inner + [(18000, floor_um + 18000)] + outer + [(-18000, floor_um + 18000)]


def _world_um(st, u, v):
    cx, cz, lx, lz = st
    return (K.round_half_away(cx * 1000.0 + u * lx), v, K.round_half_away(cz * 1000.0 + u * lz))


def collision_mesh(piece_id):
    """(vertices [(x, y, z) µm], triangles [(i, j, k)]): the section ring swept through the
    collision stations, ends open. Inner faces point toward the channel axis, the lip tops up and
    the outer shell away from the axis."""
    sp = spec(piece_id)
    floor = 15000 * sp["level"]
    ring = collision_ring(floor)
    n_inner = 19
    pool = K._Pool()
    sts = stations(sp["shape"], COLLISION_PATH[sp["shape"]])
    rings = [[_world_um(st, u, v) for u, v in ring] for st in sts]
    m = len(ring)
    for s in range(len(sts) - 1):
        a, b = rings[s], rings[s + 1]
        axis = (sts[s][0] * 1000.0, floor + 14000, sts[s][1] * 1000.0)
        for j in range(m):
            k = (j + 1) % m
            cen = [(a[j][i] + a[k][i] + b[j][i] + b[k][i]) / 4 for i in range(3)]
            to_axis = [axis[i] - cen[i] for i in range(3)]
            if j < n_inner - 1:          # inner trough and walls: face the axis
                want = to_axis
            elif j == n_inner - 1 or j == m - 1:   # lip tops
                want = (0, 1, 0)
            else:                        # outer walls and bottom: face away from the axis
                want = [-c for c in to_axis]
            pool.quad(a[j], a[k], b[k], b[j], want)
    return pool.verts, pool.tris


def sidecar(piece_id):
    sp = spec(piece_id)
    verts, tris = collision_mesh(piece_id)
    out = {
        "animated_parts": [],
        "chiral": sp["chiral"],
        "collision": [{"indices": [i for t in tris for i in t], "surface": "rail",
                       "vertices_um": [c for v in verts for c in v]}],
        "family": sp["family"],
        "footprint": sp["footprint"],
        "id": piece_id,
        "kit_contract": K.KIT_CONTRACT_VERSION,
        "piece_version": PIECE_VERSION,
        "ports": sp["ports"],
        "schema": K.SIDECAR_SCHEMA,
        "sensors": [],
    }
    if spigot(piece_id):
        out["spigot"] = spigot(piece_id)
    return out


def sidecar_bytes(piece_id):
    import json
    return (json.dumps(sidecar(piece_id), indent=2, sort_keys=True, ensure_ascii=True) + "\n").encode()


# ---------------------------------------------------------------------------------------------
# Render mesh (Blender)
# ---------------------------------------------------------------------------------------------

def render_profile(q):
    """Closed render section as (u, v) mm relative to the floor: inner R14 half-round from u = -14
    to +14, right wall, right lip round (2 mm about (16, 16)), outer wall, outer R18 bottom back
    to u = -18, left wall, left lip round. The walls are the straight edges between rounds."""
    n, nl = q["n_round"], q["n_lip"]
    pts = []
    for i in range(n + 1):
        th = math.radians(-90.0 + 180.0 * i / n)
        pts.append((K.R * math.sin(th), K.R - K.R * math.cos(th)))
    yc = LIP_H - LIP_R
    for i in range(nl + 1):
        a = math.radians(180.0 - 180.0 * i / nl)
        pts.append((K.R + LIP_R + LIP_R * math.cos(a), yc + LIP_R * math.sin(a)))
    for i in range(n + 1):
        th = math.radians(90.0 - 180.0 * i / n)
        pts.append((OUTER_R * math.sin(th), K.R - OUTER_R * math.cos(th)))
    for i in range(nl + 1):
        a = math.radians(180.0 * i / nl)
        pts.append((-(K.R + LIP_R) - LIP_R * math.cos(a), yc + LIP_R * math.sin(a)))
    return pts


def render_stations(piece_id, q):
    """Render centreline: the collision path at the LOD's density, with each port end moved
    K.INSET inward along its face normal so the end section lies exactly on the seam-inset plane
    (the centreline moves under 1 µm off the arc there). The spigot end stays at its full length."""
    sp = spec(piece_id)
    sts = list(stations(sp["shape"], q["path"][sp["shape"]]))
    if sp["shape"] == "joint":
        return [(sts[0][0] + K.INSET,) + sts[0][1:], sts[2]]
    sts[0] = (sts[0][0] + K.INSET,) + sts[0][1:]
    cx, cz, lx, lz = sts[-1]
    nx, nz = lz, -lx   # direction of travel at the end = the exit face normal
    sts[-1] = (cx - K.INSET * nx, cz - K.INSET * nz, lx, lz)
    return sts


def build_render_object(piece_id, q, name, collection, material=None, depsgraph=None):
    """The closed section swept along the render stations, both ends capped, triangulated, smooth
    with sharp edges above 35°."""
    import bmesh
    import bpy
    from mathutils import Matrix

    sp = spec(piece_id)
    floor = K.LEVEL_STEP * sp["level"]
    prof = render_profile(q)
    bm = bmesh.new()
    rings = []
    for cx, cz, lx, lz in render_stations(piece_id, q):
        rings.append([bm.verts.new((cx + u * lx, floor + v, cz + u * lz)) for u, v in prof])
    m = len(prof)
    for a, b in zip(rings, rings[1:]):
        for j in range(m):
            bm.faces.new((a[j], a[(j + 1) % m], b[(j + 1) % m], b[j]))
    bm.faces.new(rings[0][::-1])
    bm.faces.new(rings[-1])
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bmesh.ops.triangulate(bm, faces=bm.faces, quad_method="FIXED", ngon_method="EAR_CLIP")
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    to_b = Matrix(((0.001, 0, 0, 0), (0, 0, -0.001, 0), (0, 0.001, 0, 0), (0, 0, 0, 1)))
    bmesh.ops.transform(bm, matrix=to_b, verts=bm.verts)
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    for p in me.polygons:
        p.use_smooth = True
    me.set_sharp_from_angle(angle=math.radians(35.0))
    if material is not None:
        me.materials.append(material)
    ob = bpy.data.objects.new(name, me)
    collection.objects.link(ob)
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
    """One empty per port, named port_<index>, at the port floor point on the cell face; local +X
    points out of the face (the spigot port's empty sits on the rail's own face too)."""
    import bpy
    out = []
    for p in spec(piece_id)["ports"]:
        e = bpy.data.objects.new(f"{prefix}port_{p['index']}", None)
        e.empty_display_type = "SINGLE_ARROW"
        e.empty_display_size = 0.012
        e.location = K.to_blender(B.port_point_mm(p))
        e.rotation_euler = (0.0, 0.0, math.radians(K.SOCKET_YAW[p["face"]]))
        for k in ("index", "face", "level", "class", "flow", "kind"):
            e[k] = p[k]
        collection.objects.link(e)
        if parent is not None:
            e.parent = parent
        out.append(e)
    return out
