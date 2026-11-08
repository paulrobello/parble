"""Stepper elevator pieces (PRD P2 item 17, `ref-09`), built against docs/kit-contract.md.

kitmech owns the Blender side (boolean body, collision, sidecar, part objects) and calls into this
module for the stepper's spec, cutters and moving part. Nothing here imports bpy at module level.

The lift runs along +x from a side port at entry level L<level> in the anchor cell's layer to a side
port at the same level `rise` cell layers higher, `length` cells along. Between two short channel
stubs sits a row of 2n - 1 vertical slats of width w: odd slats (1, 3, ...) are the moving part
`anim_slats` (one rigid slide along +Y), even slats are fixed and part of the static body. Every slat
top falls by SLANT toward +x, so a marble on it rolls to its +x edge against the next slat.

At rest (offset 0, the entry-ready pose) each moving slat's high edge sits STEP below the low edge
before it; at the top of its stroke its low edge sits STEP above the high edge after it. One stroke
lifts a marble from one fixed slat (or the entry stub) onto the next, so a marble climbs one moving
slat per up-down cycle. Solving those two conditions for every moving slat with one shared stroke:

    G = (H + (n - 1) * SLANT) / n         rise from the low edge before a moving slat to the high
                                          edge after it, H = exit stub high edge - entry stub low edge
    stroke = G + SLANT + 2 * STEP

Heights are parameters: STEPPERS maps each piece id to (level, rise, length, moving slats), and
`layout` derives every height from those, asserting the slats still fit the footprint.
"""
import math

import kitgeom as K

# id -> (entry level, rise in cell layers, length in cells along +x, moving slats)
STEPPERS = {"mech_stepper_up1_l3": (3, 1, 2, 4)}

SLANT = 2.5        # mm a slat top falls across its width, toward +x
STEP = 1.0         # mm step down at each hand-off (onto a lowered slat, onto a fixed one at the top)
STUB_IN = 20.0     # mm entry channel stub, port face to the slat row: long enough that a marble meeting
                   # a raised first slat is already down in the stub's dip, so it cannot bounce back out
STUB_OUT = 10.5    # mm exit channel stub, slat row to the port face
STUB_FALL = 3.0    # mm the entry stub floor falls (smoothstep, level at both ends) toward the row: a
                   # marble bouncing back off the first slat cannot climb back out of the entry
EXIT_FALL = 1.0    # mm the exit stub floor falls toward the exit port, so the marble rolls out. More
                   # would lift the stub's half-round over the cell top (a sliver in the boolean)
STUB_STATIONS = 6  # path stations along each stub
CLEAR = 0.5        # mm gap between a moving slat and its slot, along x and z
HOLD = 6.0         # mm a raised moving slat still reaches below its rest low edge
SLOT_GAP = 0.5     # mm under a resting moving slat
WALL = 16.0        # mm side wall above the highest marble bottom on each slat column
HALF_W = K.R       # slat row channel half-width (the open channel's 28 mm)
PART = "anim_slats"
SLIVER_MM2 = 0.05  # render triangles smaller than this are dropped after the boolean (kitmech)


def handles(piece_id):
    return piece_id in STEPPERS


def cells(piece_id):
    _, rise, length, _ = STEPPERS[piece_id]
    return (rise + 1) * length


def footprint(piece_id):
    _, rise, length, _ = STEPPERS[piece_id]
    return sorted([x, y, 0] for x in range(length) for y in range(rise + 1))


def bounds_mm(piece_id):
    _, rise, length, _ = STEPPERS[piece_id]
    return (-K.CELL / 2, 0.0, -K.CELL / 2), (K.CELL * length - K.CELL / 2, K.CELL * (rise + 1), K.CELL / 2)


def layout(piece_id):
    """Every derived dimension, contract mm. `cols` lists the slat columns in order along +x as
    dicts: x0, x1, moving, top0 (high edge at x0, rest pose) and for moving slats the slot bottom."""
    level, rise, length, n = STEPPERS[piece_id]
    entry_floor = K.LEVEL_STEP * level
    exit_floor = entry_floor + K.CELL * rise
    x_in, x_out = -K.CELL / 2, K.CELL * length - K.CELL / 2
    xa, xb = x_in + STUB_IN, x_out - STUB_OUT
    count = 2 * n - 1
    w = (xb - xa) / count
    low_in = entry_floor - STUB_FALL     # entry stub floor at the row
    high_out = exit_floor + EXIT_FALL    # exit stub floor at the row
    g = (high_out - low_in + (n - 1) * SLANT) / n
    stroke = g + SLANT + 2 * STEP
    cols = []
    low_prev = low_in
    for i in range(count):
        x0 = xa + i * w
        if i % 2 == 0:   # moving slat j = i // 2
            top0 = low_prev - STEP
            low_rest = top0 - SLANT
            bottom = low_rest - (stroke + HOLD)
            cols.append({"x0": x0, "x1": x0 + w, "moving": True, "top0": top0, "bottom": bottom,
                         "slot": bottom - SLOT_GAP})
            low_prev = None
        else:            # fixed slat: high edge g above the low edge before the moving slat before it
            prev = cols[-1]
            top = prev["top0"] + STEP + g
            cols.append({"x0": x0, "x1": x0 + w, "moving": False, "top0": top})
            low_prev = top - SLANT
    # the last moving slat's raised low edge sits STEP above the exit stub's floor at the row
    last = cols[-1]
    assert abs(last["top0"] - SLANT + stroke - (high_out + STEP)) < 1e-9, piece_id
    for c in cols:
        if c["moving"]:
            assert c["slot"] >= 2.0, f"{piece_id}: slot bottom {c['slot']} mm under the piece"
        # the highest marble bottom on the column, plus the wall, capped at the piece top
        peak = c["top0"] + (stroke if c["moving"] else 0.0)
        c["roof"] = min(K.CELL * (rise + 1), max(K.CELL, peak + WALL))
    # walls never step down along the climb
    for a, b in zip(cols, cols[1:]):
        b["roof"] = max(b["roof"], a["roof"])
    cols[-1]["roof"] = K.CELL * (rise + 1)
    return {"level": level, "rise": rise, "length": length, "n": n, "w": w, "xa": xa, "xb": xb,
            "x_in": x_in, "x_out": x_out, "entry_floor": entry_floor, "exit_floor": exit_floor,
            "low_in": low_in, "high_out": high_out, "g": g, "stroke": stroke, "cols": cols}


def _um(x):
    return K.round_half_away(x * 1000.0)


def stroke_um(piece_id):
    return _um(layout(piece_id)["stroke"])


def _side(i, cell, face, level, flow):
    return {"cell": list(cell), "class": "channel", "face": face, "flow": flow, "index": i, "inset_um": 0,
            "kind": "side", "level": level}


def spec(piece_id):
    lay = layout(piece_id)
    level, rise, length = lay["level"], lay["rise"], lay["length"]
    ports = [_side(0, (0, 0, 0), "-x", level, "in"), _side(1, (length - 1, rise, 0), "+x", level, "out")]
    # `lift`: the slat row, from just before the first slat to the last, entry floor to the top.
    # A marble touching it keeps the slats cycling.
    x0, x1 = lay["xa"] - 2.0, lay["xb"]
    y0, y1 = lay["low_in"] - 6.0, K.CELL * (rise + 1)
    sensor = {"center_um": [_um((x0 + x1) / 2), _um((y0 + y1) / 2), 0],
              "half_extents_um": [_um((x1 - x0) / 2), _um((y1 - y0) / 2), _um(HALF_W)],
              "id": "lift", "shape": "box"}
    return {"id": piece_id, "shape": "stepper", "level": level, "ports": ports, "chiral": False, "family": "mech",
            "footprint": footprint(piece_id), "sensors": [sensor]}


def _prism_xy(poly, z0, z1):
    """Convex polygon [(x, y)] extruded along z: (vertices, faces)."""
    n = len(poly)
    v = [(x, y, z0) for x, y in poly] + [(x, y, z1) for x, y in poly]
    f = [tuple(range(n)), tuple(range(2 * n - 1, n - 1, -1))]
    f += [(i, (i + 1) % n, n + (i + 1) % n, n + i) for i in range(n)]
    return v, f


def _prism_builder(poly, z0, z1):
    def build(bm):
        v, f = _prism_xy(poly, z0, z1)
        bv = [bm.verts.new(p) for p in v]
        for face in f:
            bm.faces.new([bv[i] for i in face])
    return build


def cutters(piece_id, collision, B, q):
    """[("DIFFERENCE", builder)] for the static body: the two channel stubs, the slat row (a cut
    above each fixed slat's slanted top, a slot under each moving slat) and the stepped wall tops."""
    lay = layout(piece_id)
    rise = lay["rise"]
    top_hi = K.CELL * (rise + 1)
    out = []
    cut = lambda f: out.append(("DIFFERENCE", f))
    inset = 0.0 if collision else K.INSET
    up = K.CELL * rise
    oprof = B.open_profile(collision, q)
    oprof_up = lambda fl: [(u, v + up) for u, v in oprof(fl - up)]
    xa, xb, x_in, x_out = lay["xa"], lay["xb"], lay["x_in"], lay["x_out"]
    ins_in = () if collision else (x_in + K.INSET,)
    ins_out = () if collision else (x_out - K.INSET,)
    fin = lambda x: lay["entry_floor"] - STUB_FALL * B.smooth((x - x_in) / (xa - x_in))
    fout = lambda x: lay["high_out"] - EXIT_FALL * B.smooth((x - xb) / (x_out - xb))
    s_in = B.straight_x(x_in, xa, fin, STUB_STATIONS, extra_x=ins_in)
    s_out = B.straight_x(xb, x_out, fout, STUB_STATIONS, extra_x=ins_out)
    cut(lambda bm: B.sweep(bm, B.extend(s_in, True, True), oprof))
    # render: the stub starts flush with the last slat column; reaching back into it, its lip fillet
    # notched the wall top beside the column with zero-area UV triangles
    cut(lambda bm: B.sweep(bm, B.extend(s_out, collision, True), oprof_up))
    over = top_hi + K.CUT_ABOVE + 1.0
    zc = HALF_W
    zo = K.CELL / 2 + K.EXT
    for c in lay["cols"]:
        x0, x1 = c["x0"], c["x1"]
        if c["moving"]:
            poly = [(x0, c["slot"]), (x1, c["slot"]), (x1, over), (x0, over)]
        else:
            poly = [(x0, c["top0"]), (x1, c["top0"] - SLANT), (x1, over), (x0, over)]
        cut(_prism_builder(poly, -zc, zc))
    # stepped wall tops: one cutter per run of columns with the same roof, so no wall-top edge is
    # split by a lone vertex (a degenerate sliver with no tangent)
    runs = []
    for c in lay["cols"]:
        if runs and runs[-1][2] == c["roof"]:
            runs[-1][1] = c["x1"]
        else:
            runs.append([c["x0"], c["x1"], c["roof"]])
    for x0, x1, roof in runs:
        if roof < top_hi:
            r = roof - (inset if roof == K.CELL else 0.0)
            cut(_prism_builder([(x0, r), (x1, r), (x1, over), (x0, over)], -zo, zo))
    # the entry stub's walls stop at the anchor cell's top
    cut(_prism_builder([(x_in - K.EXT, K.CELL - inset), (xa, K.CELL - inset), (xa, over), (x_in - K.EXT, over)],
                       -zo, zo))
    return out


def parts(piece_id):
    """[(part dict without collision, [convex solids])]: one slide part, every moving slat at rest
    offset 0 as a prism with its slanted top, CLEAR inside its slot."""
    lay = layout(piece_id)
    solids = []
    zc = HALF_W - CLEAR
    for c in lay["cols"]:
        if not c["moving"]:
            continue
        x0, x1 = c["x0"] + CLEAR, c["x1"] - CLEAR
        top = lambda x: c["top0"] - SLANT * (x - c["x0"]) / lay["w"]
        poly = [(x0, c["bottom"]), (x1, c["bottom"]), (x1, top(x1)), (x0, top(x0))]
        solids.append(_prism_xy(poly, -zc, zc))
    part = {"axis": [0, 1, 0], "joint": "slide", "limits_um": [0, stroke_um(piece_id)], "name": PART,
            "pivot_um": [0, 0, 0], "rest_um": 0}
    return [(part, solids)]
