//! Chunk collider merge (P1-T3, PRD §5.4, kit contract §2, §3.1, §8, §9).
//!
//! Every placed piece's static collision mesh is transformed into the world in integer µm (mirror
//! first, then yaw, then the anchor offset), and all pieces anchored in one 8×8×8-cell chunk merge
//! into one trimesh with [`TriMeshFlags::FIX_INTERNAL_EDGES`]. Pieces are appended in sorted
//! `(anchor cell, piece id)` order and vertices are welded on exact integer-µm equality (the
//! contract's 1 µm weld epsilon on an integer grid), so mating port rings share vertices and the
//! marble sees no internal edge at a seam. The build is a pure function of the placements, so it
//! is identical natively and in wasm.
//!
//! Shell faces that touch: block bodies are closed shells, so two pieces side by side (or
//! stacked) would put coincident, opposite-facing triangles on the shared cell face. A triangle
//! lying in a cell-face plane is dropped when the cell across that face belongs to another
//! placement that also has triangles on the same face. A closed face against an open port keeps
//! its triangles, so a mismatched joint stays a wall.
//!
//! `FIX_INTERNAL_EDGES` corrects contact normals at internal edges but not at vertices, so two
//! more steps keep a seam from bumping the marble (measured in `seam_test`):
//! - [`ChunkMesh::simplify_flat`] removes the vertices a straight run of seams leaves on flat
//!   strips and straight creases, so a welded run is the same surface as one seamless piece;
//! - at a chunk boundary, where the next chunk is a separate collider, the [`PhysicsHooks`] impl
//!   replaces a contact on a stitched boundary edge or vertex with the triangle's plane contact.
//!
//! In a 90° bend the same hook replaces facet contacts with one contact against the exact channel
//! tube ([`ChannelBend`]), since the faceted tube chatters under the marble's outward load. On a
//! slope it replaces them with one contact against the exact slope channel
//! ([`ChannelSlope`]), on a curved rail against the exact rail tube ([`ChannelArc`]), on a
//! junction against the exact union of its paths ([`ChannelJunction`]), and on a straight block
//! against the exact straight tube ([`ChannelStraight`]).
//!
//! [`ChunkColliders`] is the incremental API: insert or remove placements, then
//! [`ChunkColliders::rebuild`] replaces the collider of each dirty chunk, in chunk order.

use std::collections::{BTreeMap, BTreeSet};

use parble_core::contract::{CELL_UM, CHANNEL_RADIUS_UM, CHUNK_CELLS, CHUNK_UM, HALF_CELL_UM};
use parble_core::{
    CellCoord, Mirror, Orientation, PieceId, Placement, PortClass, PortFace, PortFlow,
};
use parble_kit::{KitCatalog, KitPiece};
use rapier3d::prelude::*;

use crate::rail_arc::{self, ChannelArc};
use crate::scene;

/// The chunk holding a cell (floor division, so negative cells land in negative chunks).
pub fn chunk_of(cell: CellCoord) -> CellCoord {
    CellCoord::new(
        cell.x.div_euclid(CHUNK_CELLS),
        cell.y.div_euclid(CHUNK_CELLS),
        cell.z.div_euclid(CHUNK_CELLS),
    )
}

/// World µm of a chunk's origin: the centre of its first cell in X and Z, its bottom in Y.
pub fn chunk_origin_um(chunk: CellCoord) -> [i64; 3] {
    let c = i64::from(CHUNK_UM);
    [
        c * i64::from(chunk.x),
        c * i64::from(chunk.y),
        c * i64::from(chunk.z),
    ]
}

/// A piece-local µm point placed with orientation `o` at `anchor` (contract §8: mirror x first,
/// then yaw `(x, z) → (z, −x)` per quarter turn, then the anchor's cell offset).
pub fn place_point_um(o: Orientation, anchor: CellCoord, p: [i32; 3]) -> [i64; 3] {
    let mut x = i64::from(p[0]);
    let y = i64::from(p[1]);
    let mut z = i64::from(p[2]);
    if o.mirror == Mirror::On {
        x = -x;
    }
    for _ in 0..o.yaw.quarter_turns() {
        let nx = z;
        z = -x;
        x = nx;
    }
    let cell = i64::from(CELL_UM);
    [
        x + cell * i64::from(anchor.x),
        y + cell * i64::from(anchor.y),
        z + cell * i64::from(anchor.z),
    ]
}

/// World triangles (µm) of `piece` placed by `p`, in sidecar order. Mirrored placements are
/// re-wound so every triangle keeps facing out of the wood.
pub fn placed_triangles(piece: &KitPiece, p: &Placement) -> Vec<[[i64; 3]; 3]> {
    let o = p.orientation();
    let mut out = Vec::new();
    for group in &piece.collision {
        let world: Vec<[i64; 3]> = group
            .vertices_um
            .iter()
            .map(|&v| place_point_um(o, p.anchor, v))
            .collect();
        for t in &group.triangles {
            let [a, b, c] = t.map(|i| world[i as usize]);
            out.push(if o.mirror == Mirror::On {
                [a, c, b]
            } else {
                [a, b, c]
            });
        }
    }
    out
}

/// The cell-face plane a triangle lies in, as `(cell in `cells`, face of that cell)`, if any.
fn grid_face(t: &[[i64; 3]; 3], cells: &[CellCoord]) -> Option<(CellCoord, PortFace)> {
    let cell = i64::from(CELL_UM);
    let half = i64::from(HALF_CELL_UM);
    // Cell index along a horizontal axis / the vertical axis from three summed coordinates.
    let horiz = |s: i64| (s + 3 * half).div_euclid(3 * cell) as i32;
    let vert = |s: i64| s.div_euclid(3 * cell) as i32;
    let sum = |axis: usize| t[0][axis] + t[1][axis] + t[2][axis];
    let flat = |axis: usize| t[0][axis] == t[1][axis] && t[1][axis] == t[2][axis];

    let (neg, pos, neg_face) = if flat(0) && (t[0][0] - half).rem_euclid(cell) == 0 {
        let k = (t[0][0] - half).div_euclid(cell) as i32;
        let (y, z) = (vert(sum(1)), horiz(sum(2)));
        (
            CellCoord::new(k, y, z),
            CellCoord::new(k + 1, y, z),
            PortFace::PosX,
        )
    } else if flat(2) && (t[0][2] - half).rem_euclid(cell) == 0 {
        let k = (t[0][2] - half).div_euclid(cell) as i32;
        let (x, y) = (horiz(sum(0)), vert(sum(1)));
        (
            CellCoord::new(x, y, k),
            CellCoord::new(x, y, k + 1),
            PortFace::PosZ,
        )
    } else if flat(1) && t[0][1].rem_euclid(cell) == 0 {
        let k = t[0][1].div_euclid(cell) as i32;
        let (x, z) = (horiz(sum(0)), horiz(sum(2)));
        (
            CellCoord::new(x, k - 1, z),
            CellCoord::new(x, k, z),
            PortFace::Top,
        )
    } else {
        return None;
    };
    match (cells.contains(&neg), cells.contains(&pos)) {
        (true, false) => Some((neg, neg_face)),
        (false, true) => Some((pos, neg_face.opposite())),
        _ => None,
    }
}

fn sub(a: [i128; 3], b: [i128; 3]) -> [i128; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [i128; 3], b: [i128; 3]) -> [i128; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [i128; 3], b: [i128; 3]) -> i128 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// A merged chunk mesh: welded world vertices (µm) and triangles.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChunkMesh {
    pub vertices_um: Vec<[i64; 3]>,
    pub triangles: Vec<[u32; 3]>,
    /// Per triangle, bit `i` set when edge `(t[i], t[i + 1])` is an open edge stitched to a
    /// piece in another chunk, and bit `3 + i` when vertex `t[i]` ends such an edge (see
    /// [`ChunkColliders`]). Empty for meshes built outside a chunk.
    pub stitched: Vec<u8>,
    /// Channel bends of the chunk's placements (see [`ChannelBend`]).
    pub bends: Vec<ChannelBend>,
    /// Per triangle, `k + 1` when it lies on the channel tube of `bends[k]` (or its straight
    /// continuation), with [`IN_BEND_CELL`] set when it also lies in the bend's cell, else 0.
    /// Empty for meshes built outside a chunk.
    pub bend_of: Vec<u32>,
    /// Channel slopes of the chunk's placements (see [`ChannelSlope`]).
    pub slopes: Vec<ChannelSlope>,
    /// Per triangle, the indices `k` of the [`Self::slopes`] whose channel (or its flat
    /// continuation past either port) the triangle lies on. Empty for meshes built outside a chunk.
    pub slope_of: Vec<Vec<u32>>,
    /// Curved rail centrelines of the chunk's placements (see [`ChannelArc`]).
    pub arcs: Vec<ChannelArc>,
    /// Per triangle, `k + 1` when it lies on the tube of `arcs[k]` (or its straight continuation),
    /// else 0. Empty for meshes built outside a chunk or without curved rails.
    pub arc_of: Vec<u32>,
    /// Channel junctions of the chunk's placements (see [`ChannelJunction`]).
    pub junctions: Vec<ChannelJunction>,
    /// Per triangle, `k + 1` when it lies on the union of `junctions[k]`'s paths, else 0. Empty
    /// for meshes built outside a chunk.
    pub junction_of: Vec<u32>,
    /// The chunk holds a piece with a trapdoor over a bottom drop port (the one-way hole), so its
    /// collider takes the door seam contact correction ([`crate::door_seam`]).
    pub trapdoor: bool,
    /// Straight channel blocks of the chunk's placements (see [`ChannelStraight`]).
    pub straights: Vec<ChannelStraight>,
    /// Cells of the chunk's forks ([`ChannelJunction::fork`]).
    pub forks: Vec<CellCoord>,
}

/// [`ChunkMesh::bend_of`] flag: the triangle lies in its bend's cell.
pub const IN_BEND_CELL: u32 = 1 << 31;

/// A 90° channel bend in one cell: the channel half-round (radius
/// [`CHANNEL_RADIUS_UM`]) swept about a horizontal quarter circle of radius half a cell
/// (contract §9, §10: block curve centreline radius 30 mm about the cell corner).
///
/// The sidecar samples that tube with flat facets (8 path × 16 round segments). A marble in a bend
/// presses outward and touches several facets at once whose normals lean up to 20° from the
/// surface normal; the solver answers with a per-tick vertical chatter of 50–65 mm/s against
/// 30–40 mm/s on the smooth tube, and finer sampling does not help (16, 32 and 64 path segments
/// peak at 81, 117 and 109 mm/s, measured 2026-11-07 in `seam_test`). The contact hook therefore
/// replaces a contact on a bend's tube triangle with the contact against the exact tube.
///
/// That contact follows the slope contact's rules ([`ChannelSlope`]): evaluated at the ball's
/// end-of-step position, put on one manifold per pair, with no restitution while penetrating, and
/// on a straight only within a ball diameter of the bend's cell. Evaluated at the start of the
/// step it pumped energy into slow marbles rocking across the channel (measured 2026-11-07, one
/// marble at rest 4 mm off centre on a straight in a bend's row): the solver holds the contact
/// normal for the whole step, so on the 6 mm tube-minus-ball radius the ball sank 250–440 µm
/// into the wall, past Rapier's 300 µm allowed error, and the depth correction pushed it back
/// out with energy gained on 147 of 240 ticks. The rocking grew to 5.1 mm and 0.19 m/s and
/// held. Evaluated at the end of the step the depth stays at 57–69 µm and the marble settles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelBend {
    /// World µm `(x, z)` of the quarter circle's centre (the cell corner).
    pub centre_um: [i64; 2],
    /// World µm height of the tube axis (channel floor + channel radius).
    pub axis_y_um: i64,
    /// World µm `(min x, min z)` of the bend's cell.
    pub cell_min_um: [i64; 2],
}

impl ChannelBend {
    /// The bend of a 1-cell piece whose two channel ports sit on adjacent side faces at one level.
    fn of(piece: &KitPiece, p: &Placement) -> Option<Self> {
        let def = &piece.def;
        let [a, b] = def.ports.as_slice() else {
            return None;
        };
        let level = a.level?;
        let side = |f: PortFace| !matches!(f, PortFace::Top | PortFace::Bottom);
        let (na, nb) = (a.face.normal(), b.face.normal());
        if def.footprint.len() != 1
            || a.class != PortClass::Channel
            || b.class != PortClass::Channel
            || b.level != Some(level)
            || !side(a.face)
            || !side(b.face)
            || na.x * nb.x + na.z * nb.z != 0
        {
            return None;
        }
        let half = HALF_CELL_UM;
        let local = [
            (na.x + nb.x) * half,
            level.floor_um() + CHANNEL_RADIUS_UM,
            (na.z + nb.z) * half,
        ];
        let c = place_point_um(p.orientation(), p.anchor, local);
        let cell = i64::from(CELL_UM);
        Some(Self {
            centre_um: [c[0], c[2]],
            axis_y_um: c[1],
            cell_min_um: [
                cell * i64::from(p.anchor.x) - i64::from(half),
                cell * i64::from(p.anchor.z) - i64::from(half),
            ],
        })
    }

    /// The side `(sx, sz)` of the centre the bend's cell lies on.
    fn sides(&self) -> (i64, i64) {
        let half = i64::from(HALF_CELL_UM);
        let s = |min: i64, centre: i64| if min + half > centre { 1 } else { -1 };
        (
            s(self.cell_min_um[0], self.centre_um[0]),
            s(self.cell_min_um[1], self.centre_um[1]),
        )
    }

    /// The world axis (0 for x, 2 for z) of the straight continuation a ball centre lies beside,
    /// or `None` inside the bend's quadrant.
    fn continuation_axis(&self, c: Vector) -> Option<usize> {
        let (sx, sz) = self.sides();
        let m = |v: i64| v as f32 * 1.0e-6;
        if (c.x - m(self.centre_um[0])) * (sx as f32) < 0.0 {
            Some(0)
        } else if (c.z - m(self.centre_um[1])) * (sz as f32) < 0.0 {
            Some(2)
        } else {
            None
        }
    }

    /// Whether a ball centre lies over the bend's cell grown by `margin` metres on every side.
    fn reaches(&self, c: Vector, margin: f32) -> bool {
        let m = |v: i64| v as f32 * 1.0e-6;
        let cell = m(i64::from(CELL_UM));
        let (x0, z0) = (m(self.cell_min_um[0]), m(self.cell_min_um[1]));
        (x0 - margin..=x0 + cell + margin).contains(&c.x)
            && (z0 - margin..=z0 + cell + margin).contains(&c.z)
    }

    /// Whether a world µm plan point lies in the bend's cell.
    fn contains(&self, x: i64, z: i64) -> bool {
        let cell = i64::from(CELL_UM);
        (self.cell_min_um[0]..=self.cell_min_um[0] + cell).contains(&x)
            && (self.cell_min_um[1]..=self.cell_min_um[1] + cell).contains(&z)
    }

    /// Whether a world µm vertex lies on the bend's tube or on its straight continuation past
    /// either port (the half-round of a mating straight). The sidecar rounds every coordinate to
    /// 1 µm, so a tube vertex is within 2 µm of the exact tube.
    fn holds(&self, v: [i64; 3]) -> bool {
        self.tube_error_um(v) <= 2.0
    }

    /// How far a world µm vertex lies off the bend's tube (or its straight continuation), µm.
    fn tube_error_um(&self, v: [i64; 3]) -> f64 {
        let (sx, sz) = self.sides();
        let (dx, dz) = (v[0] - self.centre_um[0], v[2] - self.centre_um[1]);
        let half = f64::from(HALF_CELL_UM);
        let rho = if dx * sx < 0 {
            (dz * sz) as f64 - half
        } else if dz * sz < 0 {
            (dx * sx) as f64 - half
        } else {
            libm::sqrt((dx * dx + dz * dz) as f64) - half
        };
        let dy = (v[1] - self.axis_y_um) as f64;
        (libm::sqrt(rho * rho + dy * dy) - f64::from(CHANNEL_RADIUS_UM)).abs()
    }

    /// The contact of a ball (centre `c`, radius `r`, metres) with the exact tube: the normal from
    /// the tube wall toward the ball and the gap (negative when penetrating). Past a port the
    /// tube continues straight, as the mating straight does, so a ball leaving the bend while
    /// still touching a bend triangle sees no torus curving away under it.
    fn contact(&self, c: Vector, r: f32) -> Option<(Vector, f32)> {
        let axis = self.axis_point(c)?;
        let off = (axis - c).length();
        if off <= 0.0 {
            return None;
        }
        Some(((axis - c) / off, channel_tube_radius(r) - r - off))
    }

    /// The point of the bend's axis (or its straight continuation past a port) nearest the ball
    /// centre `c`, metres.
    fn axis_point(&self, c: Vector) -> Option<Vector> {
        let m = |v: i64| v as f32 * 1.0e-6;
        let (cx, cz, ay) = (
            m(self.centre_um[0]),
            m(self.centre_um[1]),
            m(self.axis_y_um),
        );
        let bend_r = m(i64::from(HALF_CELL_UM));
        let (sx, sz) = self.sides();
        let (sx, sz) = (sx as f32, sz as f32);
        let plan = Vector::new(c.x - cx, 0.0, c.z - cz);
        let axis = if plan.x * sx < 0.0 {
            Vector::new(c.x, ay, cz + sz * bend_r)
        } else if plan.z * sz < 0.0 {
            Vector::new(cx + sx * bend_r, ay, c.z)
        } else {
            let d = plan.length();
            if d <= 0.0 {
                return None;
            }
            Vector::new(cx, ay, cz) + plan * (bend_r / d)
        };
        Some(axis)
    }
}

/// Distance from the channel axis to the midline of a collision facet, metres: 14 mm ×
/// cos(5.625°) for the contract's 16 half-round segments (§5.4). The rail arcs' exact tube
/// ([`crate::rail_arc::ChannelArc`]) takes it as its radius.
pub(crate) const FACET_TUBE_RADIUS_M: f32 = 0.013_932_586;

/// How much deeper the solver sinks a ball held by one contact than one held by the two facets
/// beside a straight's bottom crease, metres.
///
/// Rapier's contacts with a fixed body are soft springs (`static_contact_softness`, 60 Hz), so a
/// ball at rest on one contact sinks its static deflection `g / ω²` = 69.0 µm whatever its mass.
/// On the crease each facet carries half the weight along a normal tilted α = 5.625°, so the ball
/// sinks `g / (2 ω² cos² α)` = 34.8 µm. An exact contact replaces the two facet contacts with
/// one, so with the rigid rest radius alone a centred ball sat 34.2 µm lower on it than on the
/// facets it hands over to (measured 2026-11-09: 68.9 against 34.7 µm under the rigid rest at
/// every marble size, `rest_height_test`), a step a slow marble rolling onto the facets had to
/// climb. Pinned to Rapier's default softness and [`crate::marble::G`] by
/// `tests::the_crease_sink_compensation_follows_the_static_contact_softness`.
pub(crate) const CREASE_SINK_COMPENSATION_M: f32 = 3.417_8e-5;

/// A straight channel block (`blk_straight_*`): the channel half-round along one horizontal axis
/// through its cell.
///
/// The sidecar samples the half-round with 16 flat facets 11.25° apart, so a marble swinging
/// across the channel meets a facet edge whose normal turns by a whole facet in one tick: its
/// vertical speed jumped by 4–42 mm/s at each crossing (measured 2026-11-08). The contact hook
/// replaces the contacts on the channel's triangles with one contact against the exact tube.
///
/// Its radius is every exact channel contact's ([`channel_tube_radius`]), so a centred ball at
/// rest sits at the height the two crease facets hold it at.
///
/// The straight owns the ball only while its centre is over the straight's cell and no other
/// piece's cell lies within a ball diameter along the channel ([`ChunkColliders::straight_owner`]).
/// Every seam with a slope, bend, junction, rail or mechanism keeps the contact it had, so the
/// hand-over to and from the exact tube happens between straights, where the two surfaces meet
/// the ball at the same height and the same normal for a centred ball.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelStraight {
    /// The straight's cell.
    pub cell: CellCoord,
    /// The world axis the channel runs along (0 for x, 2 for z).
    pub axis: usize,
    /// World µm of the centreline on the other horizontal axis.
    pub at_um: i64,
    /// World µm height of the tube axis (channel floor + channel radius).
    pub axis_y_um: i64,
}

impl ChannelStraight {
    /// The straight of a `blk_straight_*` placement: two channel ports on opposite side faces at
    /// one level.
    fn of(piece: &KitPiece, p: &Placement) -> Option<Self> {
        let def = &piece.def;
        let [a, b] = def.ports.as_slice() else {
            return None;
        };
        let level = a.level?;
        let side = |f: PortFace| !matches!(f, PortFace::Top | PortFace::Bottom);
        let (na, nb) = (a.face.normal(), b.face.normal());
        if !p.piece.as_str().starts_with("blk_straight_")
            || def.footprint.len() != 1
            || a.class != PortClass::Channel
            || b.class != PortClass::Channel
            || b.level != Some(level)
            || !side(a.face)
            || !side(b.face)
            || na.x != -nb.x
            || na.z != -nb.z
        {
            return None;
        }
        let half = HALF_CELL_UM;
        let axis_y = level.floor_um() + CHANNEL_RADIUS_UM;
        let world = |n: CellCoord| {
            place_point_um(p.orientation(), p.anchor, [n.x * half, axis_y, n.z * half])
        };
        let (u, v) = (world(na), world(nb));
        let (axis, at_um) = if u[0] != v[0] { (0, u[2]) } else { (2, u[0]) };
        Some(Self {
            cell: p.anchor,
            axis,
            at_um,
            axis_y_um: u[1],
        })
    }

    /// Whether two straights share one tube line.
    fn same_tube(&self, o: &Self) -> bool {
        self.axis == o.axis && self.at_um == o.at_um && self.axis_y_um == o.axis_y_um
    }

    /// Whether a world point (metres) lies on the tube line's half-round, within 3 µm (the
    /// sidecar's 1 µm rounding plus the collider's `f32` vertices).
    fn on_tube(&self, v: Vector) -> bool {
        let m = |x: i64| x as f64 * 1.0e-6;
        let d = f64::from(v[2 - self.axis]) - m(self.at_um);
        let dy = f64::from(v.y) - m(self.axis_y_um);
        (libm::sqrt(d * d + dy * dy) - m(i64::from(CHANNEL_RADIUS_UM))).abs() <= 3.0e-6
    }

    /// The contact of a ball (centre `c`, radius `r`, metres) with the exact tube: the normal from
    /// the tube wall toward the ball and the gap (negative when penetrating).
    fn contact(&self, c: Vector, r: f32) -> Option<(Vector, f32)> {
        let m = |x: i64| x as f32 * 1.0e-6;
        let axis = if self.axis == 0 {
            Vector::new(c.x, m(self.axis_y_um), m(self.at_um))
        } else {
            Vector::new(m(self.at_um), m(self.axis_y_um), c.z)
        };
        let off = (axis - c).length();
        (off > 0.0).then(|| ((axis - c) / off, channel_tube_radius(r) - r - off))
    }
}

/// A one-cell channel junction: three or more channel side ports at one level, as on the joiner
/// (in at −x and −z, out at +x) and the forks (in at −x, out at ±z). Its channel is the union of
/// one path per in-to-out port pair: a straight tube between opposite faces, a [`ChannelBend`]
/// between adjacent ones, all of the exact channel radius ([`channel_tube_radius`]).
///
/// The sidecar samples each path with flat facets and the boolean union meets them in a faceted
/// ridge, so a marble crossing the joiner peaked at 45 mm/s upward from its −x inlet and 88 mm/s
/// from its −z inlet (measured 2026-11-08, `block_seam_test`). The contact hook replaces a contact on a junction
/// triangle with one contact against the exact union: free space is the union of the paths'
/// tubes, so the path the ball sits deepest inside (largest gap) owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelJunction {
    /// Paths between adjacent in and out ports.
    pub bends: Vec<ChannelBend>,
    /// Paths between opposite in and out ports: the world axis they run along (0 for x, 2 for
    /// z) and their centreline's world µm coordinate on the other horizontal axis.
    pub straights: Vec<(usize, i64)>,
    /// World µm height of the tube axes (channel floor + channel radius).
    pub axis_y_um: i64,
    /// World µm `(min x, min z)` of the junction's cell.
    pub cell_min_um: [i64; 2],
    /// Whether the junction is a fork (a moving blade at the crotch): the ridge between its arcs
    /// is blended ([`Self::contact`]). Every junction's paths take the exact channel radius
    /// ([`channel_tube_radius`]).
    pub fork: bool,
    /// A fork's frog: world µm points, at axis height, of the inlet port and of the middle of
    /// the frog's end line ([`FORK_FROG_UM`]). The frog is the plan region between the two arcs'
    /// centrelines from the port to the end line. Over it the floor is flat at the channel floor
    /// (a ball's centre lies within the tube radius of the frog's plane at axis height), and past
    /// the end line it rises as a tube about that line ([`Self::frog_axis_point`]).
    pub frog: Option<[[i64; 3]; 2]>,
}

/// Where a fork's frog ends, µm from the cell centre toward the inlet port: at the middle
/// station of the collision arcs (30 mm × (1 − cos 45°) past the inlet port), kitmech
/// `FORK_FROG_X`. On the two arcs alone the floor along the fork's centreline was a ridge that
/// rose 2.3 mm before the blade, and a centred mini at 0.2 m/s stalled on it and rolled back
/// (measured 2026-11-09, `behaviour_test::fork_blade_sweep`).
pub const FORK_FROG_UM: i64 = 8_787;

/// Radius of a fork's arcs' centrelines, metres.
const FORK_ARC_M: f32 = 0.03;

/// Tolerance of [`ChannelJunction::holds`], µm: over the 144 µm sag of a bend's 8 path facets
/// (30 mm × (1 − cos 5.625°)), where the ridge between two paths can sit.
const JUNCTION_RIDGE_UM: f64 = 200.0;

impl ChannelJunction {
    fn of(piece: &KitPiece, p: &Placement) -> Option<Self> {
        let def = &piece.def;
        let ports = def.ports.as_slice();
        let level = ports.first()?.level?;
        let side = |f: PortFace| !matches!(f, PortFace::Top | PortFace::Bottom);
        // The forks (a moving blade at the crotch) take the round tube of the exact straight
        // ([`ChannelJunction::fork`]); the blade is untouched (the ball-blade pair is not a chunk
        // pair, so this hook never sees it). With the forks on the faceted section until
        // 2026-11-09, the outlet kicks were the downstream straight's facet crease and the
        // section's 5.6° ridge where the arcs part drifted off-centre marbles to a side
        // (`fork_contact_test`). On the round tube, with the exact straights, the frog
        // ([`Self::frog`]) and the shipped blade, the inlet reads upward 0.50 / kick 0.11 mm/s,
        // the splitter's -z outlet 1.27 / 0.10 and the toggle's +z outlet 0.99 / 0.17 (measured
        // 2026-11-09 in `block_seam_test`).
        if def.footprint.len() != 1
            || ports.len() < 3
            || ports
                .iter()
                .any(|q| q.class != PortClass::Channel || q.level != Some(level) || !side(q.face))
        {
            return None;
        }
        let half = HALF_CELL_UM;
        let axis_y = level.floor_um() + CHANNEL_RADIUS_UM;
        let world = |local: [i32; 3]| place_point_um(p.orientation(), p.anchor, local);
        let cell = i64::from(CELL_UM);
        let cell_min_um = [
            cell * i64::from(p.anchor.x) - i64::from(half),
            cell * i64::from(p.anchor.z) - i64::from(half),
        ];
        let (mut bends, mut straights) = (Vec::new(), Vec::new());
        let ins = ports.iter().filter(|q| q.flow != PortFlow::Out);
        for a in ins {
            for b in ports
                .iter()
                .filter(|q| q.flow != PortFlow::In && q.face != a.face)
            {
                let (na, nb) = (a.face.normal(), b.face.normal());
                if na.x * nb.x + na.z * nb.z == 0 {
                    let c = world([(na.x + nb.x) * half, axis_y, (na.z + nb.z) * half]);
                    let bend = ChannelBend {
                        centre_um: [c[0], c[2]],
                        axis_y_um: c[1],
                        cell_min_um,
                    };
                    if !bends.contains(&bend) {
                        bends.push(bend);
                    }
                } else {
                    let (u, v) = (
                        world([na.x * half, axis_y, na.z * half]),
                        world([nb.x * half, axis_y, nb.z * half]),
                    );
                    let path = if u[0] != v[0] { (0, u[2]) } else { (2, u[0]) };
                    if !straights.contains(&path) {
                        straights.push(path);
                    }
                }
            }
        }
        let fork = !piece.animated_parts.is_empty();
        let frog = ports
            .iter()
            .find(|q| q.flow == PortFlow::In)
            .filter(|_| fork)
            .map(|q| {
                let n = q.face.normal();
                let to = |d: i64| {
                    world([
                        (i64::from(n.x) * d) as i32,
                        axis_y,
                        (i64::from(n.z) * d) as i32,
                    ])
                };
                [to(i64::from(half)), to(FORK_FROG_UM)]
            });
        Some(Self {
            bends,
            straights,
            axis_y_um: world([0, axis_y, 0])[1],
            cell_min_um,
            fork,
            frog,
        })
    }

    /// Whether a world µm vertex lies on one of the paths' tubes (or their straight
    /// continuations past the ports), within [`JUNCTION_RIDGE_UM`]: the boolean's ridge vertices
    /// lie where two paths' facets cross, off both exact tubes.
    fn holds(&self, v: [i64; 3]) -> bool {
        let p = [v[0] as f32, v[1] as f32, v[2] as f32].map(|c| c * 1.0e-6);
        let on_frog = self
            .frog_axis_point(Vector::new(p[0], p[1], p[2]))
            .is_some_and(|a| {
                let d = (a - Vector::new(p[0], p[1], p[2])).length() as f64 * 1.0e6;
                (d - f64::from(CHANNEL_RADIUS_UM)).abs() <= JUNCTION_RIDGE_UM
            });
        on_frog
            || self
                .bends
                .iter()
                .any(|b| b.tube_error_um(v) <= JUNCTION_RIDGE_UM)
            || self.straights.iter().any(|&(axis, at)| {
                let d = (v[2 - axis] - at) as f64;
                let dy = (v[1] - self.axis_y_um) as f64;
                (libm::sqrt(d * d + dy * dy) - f64::from(CHANNEL_RADIUS_UM)).abs()
                    <= JUNCTION_RIDGE_UM
            })
    }

    /// The point of the frog's floor plane at axis height nearest `c` (metres), where the frog
    /// owns it: over the frog, the point above or below `c`; past its end line, the nearest point
    /// of that line. `None` upstream of the inlet port and beside the frog (the arcs own those).
    fn frog_axis_point(&self, c: Vector) -> Option<Vector> {
        let [a, b] = self.frog?;
        let m = |v: [i64; 3]| Vector::new(v[0] as f32, v[1] as f32, v[2] as f32) * 1.0e-6;
        let (a, b) = (m(a), m(b));
        let s_end = (b - a).length();
        let d = (b - a) / s_end;
        let l = Vector::new(d.z, 0.0, -d.x);
        let rel = c - a;
        let (s, t) = (rel.dot(d), rel.dot(l));
        let arc = |s: f32| FORK_ARC_M - libm::sqrtf((FORK_ARC_M * FORK_ARC_M - s * s).max(0.0));
        if s < 0.0 {
            None
        } else if s <= s_end {
            (t.abs() <= arc(s)).then(|| Vector::new(c.x, a.y, c.z))
        } else {
            let w = arc(s_end);
            Some(a + d * s_end + l * t.clamp(-w, w))
        }
    }

    /// Whether a ball centre lies over the junction's cell grown by `margin` metres on every side.
    fn reaches(&self, c: Vector, margin: f32) -> bool {
        let m = |v: i64| v as f32 * 1.0e-6;
        let cell = m(i64::from(CELL_UM));
        let (x0, z0) = (m(self.cell_min_um[0]), m(self.cell_min_um[1]));
        (x0 - margin..=x0 + cell + margin).contains(&c.x)
            && (z0 - margin..=z0 + cell + margin).contains(&c.z)
    }

    /// The contact of a ball (centre `c`, radius `r`, metres) with the exact union of the paths:
    /// that of the path with the largest gap.
    ///
    /// On a fork ([`Self::fork`]) the paths are round tubes, and where
    /// the two deepest gaps lie within [`FORK_RIDGE_BLEND_M`] of each other the normal blends the
    /// two paths' normals by their gap difference, equally at a tie ([`blend_ridge`]). The contact
    /// then depends on the set of paths, not their order: a centred marble feels a vertical
    /// normal on the ridge where the arcs part, so the blade, not the order the paths were listed
    /// in, picks its branch.
    fn contact(&self, c: Vector, r: f32) -> Option<(Vector, f32)> {
        let m = |v: i64| v as f32 * 1.0e-6;
        let ay = m(self.axis_y_um);
        let big_r = channel_tube_radius(r);
        let tube = |axis_point: Vector| {
            let off = (axis_point - c).length();
            (off > 0.0).then(|| ((axis_point - c) / off, big_r - r - off))
        };
        let straight = self.straights.iter().filter_map(|&(axis, at)| {
            tube(if axis == 0 {
                Vector::new(c.x, ay, m(at))
            } else {
                Vector::new(m(at), ay, c.z)
            })
        });
        let frog = self.frog_axis_point(c).and_then(tube);
        let paths = self
            .bends
            .iter()
            .filter_map(|b| tube(b.axis_point(c)?))
            .chain(straight)
            .chain(frog);
        if !self.fork {
            return paths.max_by(|a, b| a.1.total_cmp(&b.1));
        }
        let mut best: Option<(Vector, f32)> = None;
        let mut second: Option<(Vector, f32)> = None;
        for p in paths {
            if best.is_none_or(|b| p.1 > b.1) {
                second = best;
                best = Some(p);
            } else if second.is_none_or(|s| p.1 > s.1) {
                second = Some(p);
            }
        }
        match (best, second) {
            (Some(a), Some(b)) => Some(blend_ridge(a, b)),
            (a, _) => a,
        }
    }
}

/// Gap difference over which a fork's contact blends its two deepest paths' normals, metres
/// ([`ChannelJunction::contact`]).
const FORK_RIDGE_BLEND_M: f32 = 5.0e-5;

/// The contact on the ridge between two paths `a` and `b` (normal, gap), `a.1 >= b.1`: the deeper
/// path's gap, and a normal that turns from the mean of the two at a tie to `a`'s at a gap
/// difference of [`FORK_RIDGE_BLEND_M`]. Symmetric in `a` and `b` at a tie, and the sums are
/// taken in a fixed order, so mirrored marbles take mirrored contacts on every platform.
fn blend_ridge(a: (Vector, f32), b: (Vector, f32)) -> (Vector, f32) {
    let d = a.1 - b.1;
    if d >= FORK_RIDGE_BLEND_M {
        return a;
    }
    let w = 0.5 + 0.5 * d / FORK_RIDGE_BLEND_M;
    let n = a.0 * w + b.0 * (1.0 - w);
    let len = n.length();
    if len <= 0.0 {
        return a;
    }
    (n / len, a.1)
}

/// A one-cell channel slope: the channel half-round, in vertical cross-sections across the path,
/// lowered along the path from the entry floor to the exit floor by the contract's profile
/// (contract §7): the quintic smootherstep on both slopes and on a landing floor (the generator's
/// `kitgeom.floor_mm` and `kitblocks.smoother`). Its gradient and curvature are zero at both ports,
/// so past either port the tube continues flat at that port's level, as the mating straight does,
/// and the marble's normal acceleration does not step at the seams.
///
/// The same surface, shorter, is the landing floor a dropped marble lands on in the tower socket
/// block and the redirect ([`ChannelSlope::landing`]).
///
/// The sidecar samples the slope with 8 flat path segments. Two things bump a marble on them
/// (measured 2026-11-07 in `seam_test`, 0.3 m/s centred into `blk_slope_gentle_l3`):
/// - at the entry seam the ball, still on the straight, touches the slope's first bottom vertex
///   0.4 mm ahead. Rapier does not correct vertex normals, so that contact leans 3° back against
///   the motion and the ball bounces off it: vertical speed goes from −11 to +32 mm/s in one tick;
/// - down the slope the ball touches 4–10 facets at once and chatters, bouncing off on alternate
///   ticks (kick 134 mm/s). Finer sampling does not converge: a rebuilt slope channel with 8,
///   16, 32, 64 and 128 path segments kicks 112, 90, 104, 175 and 114 mm/s inside the slope.
///
/// The contact hook therefore replaces contacts on the slope's channel triangles, and on the flat
/// continuation triangles of the straights at either port while the ball is at the slope, with
/// the contact against this exact surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelSlope {
    /// World µm `(x, z)` of the entry (upper) port centre, on the entry cell face.
    pub entry_um: [i64; 2],
    /// Plan unit direction `(x, z)` from the entry port to the exit port.
    pub dir: [i64; 2],
    /// World µm height of the tube axis at the entry port (entry floor + channel radius).
    pub axis_y_um: i64,
    /// Floor drop from the entry port to the exit port, µm.
    pub drop_um: i64,
    /// Plan length of the smootherstep from the entry to the exit, µm: a cell on a slope piece,
    /// shorter on a landing floor ([`ChannelSlope::landing`]).
    pub len_um: i64,
    /// Plan length of a constant-gradient pad from the entry before the descent, µm: the tower
    /// socket's landing pad, 0 elsewhere.
    pub pad_len_um: i64,
    /// Floor fall over the pad, µm (0 without a pad).
    pub pad_drop_um: i64,
}

impl ChannelSlope {
    /// The slope of a 1-cell piece whose two channel ports sit on opposite side faces at
    /// different levels.
    fn of(piece: &KitPiece, p: &Placement) -> Option<Self> {
        let def = &piece.def;
        let [a, b] = def.ports.as_slice() else {
            return None;
        };
        let (Some(la), Some(lb)) = (a.level, b.level) else {
            return None;
        };
        let side = |f: PortFace| !matches!(f, PortFace::Top | PortFace::Bottom);
        let (na, nb) = (a.face.normal(), b.face.normal());
        if def.footprint.len() != 1
            || a.class != PortClass::Channel
            || b.class != PortClass::Channel
            || !side(a.face)
            || !side(b.face)
            || na.x != -nb.x
            || na.z != -nb.z
            || la == lb
        {
            return None;
        }
        let ((hi, nh), (lo, nl)) = if la.floor_um() > lb.floor_um() {
            ((la, na), (lb, nb))
        } else {
            ((lb, nb), (la, na))
        };
        let half = HALF_CELL_UM;
        let port = |n: CellCoord, floor: i32| {
            place_point_um(
                p.orientation(),
                p.anchor,
                [n.x * half, floor + CHANNEL_RADIUS_UM, n.z * half],
            )
        };
        let (e, x) = (port(nh, hi.floor_um()), port(nl, lo.floor_um()));
        let cell = i64::from(CELL_UM);
        Some(Self {
            entry_um: [e[0], e[2]],
            dir: [(x[0] - e[0]) / cell, (x[2] - e[2]) / cell],
            axis_y_um: e[1],
            drop_um: e[1] - x[1],
            len_um: cell,
            pad_len_um: 0,
            pad_drop_um: 0,
        })
    }

    /// The landing floor of a piece a marble falls into and leaves by a side port
    /// (blender/kit/kitblocks.py), from 1 mm past the shaft's far wall to the exit. On the
    /// redirect (in on a side port, out one cell lower) the tunnel floor rises
    /// [`LANDING_RISE_UM`] over the exit floor there and falls by the smootherstep to the exit
    /// port face. On the tower socket block (in by the top drop port, out by the socket) the
    /// marble falls straight down the shaft onto a pad falling [`TOWER_PAD_FALL_UM_PER_MM`] µm/mm
    /// to [`TOWER_PAD_UM`] past the axis, then a quintic Hermite descent of
    /// [`TOWER_DESCENT_UM`] (gradient continuous with the pad, zero gradient and curvature at both
    /// its ends) to the socket's collision port plane, [`Port::inset_um`] inside the face.
    ///
    /// [`Port::inset_um`]: parble_core::Port::inset_um
    fn landing(piece: &KitPiece, p: &Placement) -> Option<Self> {
        let ports = piece.def.ports.as_slice();
        let side = |f: PortFace| !matches!(f, PortFace::Top | PortFace::Bottom);
        let [out] = ports
            .iter()
            .filter(|q| q.flow == PortFlow::Out && side(q.face))
            .collect::<Vec<_>>()[..]
        else {
            return None;
        };
        let level = out.level?;
        // The drop catch (`blk_catch_*`, blender/kit/kitfunnel.py) is the tower's landing with a
        // channel port on the face (inset 0) instead of the socket.
        let tower = matches!(out.class, PortClass::Socket | PortClass::Channel)
            && ports.len() == 2
            && ports.iter().any(|q| q.face == PortFace::Top);
        let redirect = out.class == PortClass::Channel
            && ports.len() == 2
            && ports
                .iter()
                .any(|q| q.flow == PortFlow::In && side(q.face) && q.cell.y == out.cell.y + 1);
        if !(tower || redirect) {
            return None;
        }
        let n = out.face.normal();
        let cell = CELL_UM;
        let base = [out.cell.x * cell, out.cell.y * cell, out.cell.z * cell];
        let axis = level.floor_um() + CHANNEL_RADIUS_UM;
        let at = |d: i32, y: i32| {
            place_point_um(
                p.orientation(),
                p.anchor,
                [base[0] + n.x * d, base[1] + y, base[2] + n.z * d],
            )
        };
        let pad_len = LANDING_REACH_UM + TOWER_PAD_UM;
        let pad_drop = pad_len / 1_000 * TOWER_PAD_FALL_UM_PER_MM;
        let (rise, pad_len, pad_drop) = if tower {
            (pad_drop + TOWER_DESCENT_UM, pad_len, pad_drop)
        } else {
            (LANDING_RISE_UM, 0, 0)
        };
        let (e, x) = (
            at(-LANDING_REACH_UM, axis + rise),
            at(HALF_CELL_UM - out.inset_um, axis),
        );
        let len = i64::from(LANDING_REACH_UM + HALF_CELL_UM - out.inset_um);
        Some(Self {
            entry_um: [e[0], e[2]],
            dir: [(x[0] - e[0]) / len, (x[2] - e[2]) / len],
            axis_y_um: e[1],
            drop_um: e[1] - x[1],
            len_um: len,
            pad_len_um: i64::from(pad_len),
            pad_drop_um: i64::from(pad_drop),
        })
    }

    /// The floor's drop below the entry at `t` in [0, 1] along the path, for a total `drop` over
    /// the plan length `len`, and its gradient along the path. Plain multiplies and adds only, so
    /// native and wasm round alike.
    ///
    /// With a pad ([`Self::pad_len_um`]) the floor falls at the pad's constant gradient first,
    /// then by the quintic Hermite that starts at that gradient with zero curvature and ends flat
    /// with zero curvature: `pad + D s(u) + g L h1(u)`, `s` the smootherstep, `h1(u) = u - 6u^3 +
    /// 8u^4 - 3u^5`, `D` the descent's own drop and `g L` the pad gradient over its length.
    fn profile(&self, drop: f32, len: f32, t: f32) -> (f32, f32) {
        if self.pad_len_um == 0 {
            let u = t * (1.0 - t);
            return (
                drop * (t * t * t * (10.0 + t * (-15.0 + 6.0 * t))),
                drop * (30.0 * u * u) / len,
            );
        }
        // The pad's share of `drop` (which carries the caller's sign) and of `len`.
        let k = drop / (self.drop_um as f32 * 1.0e-6);
        let pl = self.pad_len_um as f32 * 1.0e-6 * (len / (self.len_um as f32 * 1.0e-6));
        let pd = self.pad_drop_um as f32 * 1.0e-6 * k;
        let s = t * len;
        let g = pd / pl;
        if s <= pl {
            return (g * s, g);
        }
        let l = len - pl;
        let u = (s - pl) / l;
        let d = drop - pd;
        let w = u * (1.0 - u);
        let h1 = u * (1.0 + u * u * (-6.0 + u * (8.0 - 3.0 * u)));
        let dh1 = 1.0 + u * u * (-18.0 + u * (32.0 - 15.0 * u));
        (
            pd + d * (u * u * u * (10.0 + u * (-15.0 + 6.0 * u))) + g * l * h1,
            (d * (30.0 * w * w) + g * l * dh1) / l,
        )
    }

    /// [`Self::profile`]'s drop in `f64`, for the µm vertex test ([`Self::holds`]).
    fn profile_f64(&self, drop: f64, t: f64) -> f64 {
        if self.pad_len_um == 0 {
            return drop * (t * t * t * (10.0 + t * (-15.0 + 6.0 * t)));
        }
        let k = drop / self.drop_um as f64;
        let (len, pl) = (self.len_um as f64, self.pad_len_um as f64);
        let pd = self.pad_drop_um as f64 * k;
        let s = t * len;
        let g = pd / pl;
        if s <= pl {
            return g * s;
        }
        let l = len - pl;
        let u = (s - pl) / l;
        let h1 = u * (1.0 + u * u * (-6.0 + u * (8.0 - 3.0 * u)));
        pd + (drop - pd) * (u * u * u * (10.0 + u * (-15.0 + 6.0 * u))) + g * l * h1
    }

    /// Path coordinate from the entry port and lateral offset of a world plan point, metres.
    fn plan(&self, x: f32, z: f32) -> (f32, f32) {
        let m = |v: i64| v as f32 * 1.0e-6;
        let (dx, dz) = (self.dir[0] as f32, self.dir[1] as f32);
        let (px, pz) = (x - m(self.entry_um[0]), z - m(self.entry_um[1]));
        (px * dx + pz * dz, pz * dx - px * dz)
    }

    /// Whether a world µm vertex lies on the slope's channel or its flat continuation past either
    /// port. The sidecar rounds every coordinate to 1 µm, so a channel vertex is within 2 µm.
    fn holds(&self, v: [i64; 3]) -> bool {
        let (dx, dz) = (self.dir[0], self.dir[1]);
        let (px, pz) = (v[0] - self.entry_um[0], v[2] - self.entry_um[1]);
        let (s, w) = (px * dx + pz * dz, pz * dx - px * dz);
        let t = (s as f64 / self.len_um as f64).clamp(0.0, 1.0);
        let axis = self.axis_y_um as f64 - self.profile_f64(self.drop_um as f64, t);
        let dy = v[1] as f64 - axis;
        let w = w as f64;
        (libm::sqrt(dy * dy + w * w) - f64::from(CHANNEL_RADIUS_UM)).abs() <= 2.0
    }

    /// Whether a ball centre lies over the slope's cell, or within `margin` metres of it along
    /// the path.
    fn covers(&self, c: Vector, margin: f32) -> bool {
        let (s, w) = self.plan(c.x, c.z);
        let (cell, len) = (CELL_UM as f32 * 1.0e-6, self.len_um as f32 * 1.0e-6);
        (-margin..=len + margin).contains(&s) && w.abs() <= 0.5 * cell
    }

    /// The contact of a ball (centre `c`, radius `r`, metres) with the exact slope channel: the
    /// normal from the channel wall toward the ball and the gap (negative when penetrating).
    ///
    /// The wall is the family of vertical circles about the axis curve, so the nearest wall point
    /// is found along the path by a fixed four Gauss–Newton steps from the centre's own section
    /// (the axis curve's radius is at least 40 mm on the gentle slope and 20 mm on the steep one,
    /// the contact under 3 mm along the path from the centre). The circle radius is
    /// [`channel_tube_radius`].
    fn contact(&self, c: Vector, r: f32) -> Option<(Vector, f32)> {
        let m = |v: i64| v as f32 * 1.0e-6;
        let (sc, w) = self.plan(c.x, c.z);
        let (dx, dz) = (self.dir[0] as f32, self.dir[1] as f32);
        let (len, ay, drop) = (m(self.len_um), m(self.axis_y_um), m(self.drop_um));
        // Axis height and its slope along the path at `s`.
        let axis = |s: f32| {
            let t = (s / len).clamp(0.0, 1.0);
            let (f, df) = self.profile(-drop, len, t);
            (ay + f, df)
        };
        let big_r = channel_tube_radius(r);
        let mut s = sc;
        for _ in 0..4 {
            let (a, da) = axis(s);
            let dy = c.y - a;
            let q = (dy * dy + w * w).sqrt();
            if q <= 0.0 {
                return None;
            }
            let j = dy * da / q;
            s -= ((s - sc) + (big_r - q) * j) / (1.0 + j * j);
        }
        let (a, _) = axis(s);
        let dy = c.y - a;
        let q = (dy * dy + w * w).sqrt();
        let (ds, d) = (sc - s, big_r - q);
        let dist = (ds * ds + d * d).sqrt();
        if q <= 0.0 || dist <= 0.0 {
            return None;
        }
        // From the wall point to the centre: along the path, then inward across the section.
        let sd = if d >= 0.0 { dist } else { -dist };
        let (ns, ny, nw) = (ds / sd, -dy * d / (q * sd), -w * d / (q * sd));
        Some((
            Vector::new(ns * dx - nw * dz, ny, ns * dz + nw * dx),
            sd - r,
        ))
    }
}

/// Height of a landing floor's high end over its exit floor, µm (kitblocks `LANDING_RISE`).
const LANDING_RISE_UM: i32 = 15_000;

/// Plan distance from the shaft axis to a landing floor's high end, µm: 1 mm past the 32 mm
/// shaft's wall (kitblocks `HOLE_R + 1`).
const LANDING_REACH_UM: i32 = 17_000;

/// Plan distance past the shaft axis to the end of the tower socket's landing pad, µm (kitblocks
/// `TOWER_PAD`): every 8 mm marble centre in the 32 mm shaft is over the pad.
const TOWER_PAD_UM: i32 = 8_000;

/// The tower socket's landing pad's fall toward the socket, µm per mm (kitblocks
/// `TOWER_PAD_FALL`): 3 %, enough that a marble landing on it rolls off.
const TOWER_PAD_FALL_UM_PER_MM: i32 = 30;

/// The tower socket's descent after the pad, µm (kitblocks `TOWER_DESCENT`): the most the 12 mm
/// to the socket allows with its concave curvature radius at or above 20 mm.
const TOWER_DESCENT_UM: i32 = 1_300;

/// Approach speed along the normal, m/s, above which a ball meeting a landing floor bounces even
/// when its contact penetrates: a rolling ball meets a fresh triangle at up to 64 mm/s (see the
/// slope's hand-over rule), a falling one lands at about 1 m/s.
const LANDING_ROLLING_M_S: f32 = 0.25;

/// Largest current gap, metres, at which a ball on an exact surface counts as rolling on it
/// ([`ChunkColliders::rolling`]): over the float noise of a resting ball's gap, under the 42 µm a
/// ball falling at 10 mm/s closes in one tick, so a slowly dropped ball still bounces.
const ROLLING_GAP_M: f32 = 1.0e-5;

/// Section radius of every exact channel contact (straight, bend, slope, landing floor,
/// junction) for a ball of radius `r`, metres: the radius at which a centred ball comes to rest
/// at the height the two facets beside a straight's bottom crease hold it at, so the contacts hand
/// over to one another and to the facets without a step.
///
/// On the rigid facets (midline [`FACET_TUBE_RADIUS_M`], tilted α = 5.625°, cos α = midline /
/// 14 mm) the ball's centre lies `(FACET_TUBE_RADIUS_M - r) / cos α` below the axis. The exact
/// contact is one soft contact where the crease is two, so its radius is
/// [`CREASE_SINK_COMPENSATION_M`] smaller to leave the resting ball at the same height.
fn channel_tube_radius(r: f32) -> f32 {
    let channel = CHANNEL_RADIUS_UM as f32 * 1.0e-6;
    r + (FACET_TUBE_RADIUS_M - r) * channel / FACET_TUBE_RADIUS_M - CREASE_SINK_COMPENSATION_M
}

impl ChunkMesh {
    /// Append triangles, welding vertices on exact µm equality. Triangles that collapse are skipped.
    fn push_welded(&mut self, weld: &mut BTreeMap<[i64; 3], u32>, tris: &[[[i64; 3]; 3]]) {
        for t in tris {
            let idx = t.map(|v| {
                *weld.entry(v).or_insert_with(|| {
                    self.vertices_um.push(v);
                    (self.vertices_um.len() - 1) as u32
                })
            });
            if idx[0] != idx[1] && idx[1] != idx[2] && idx[0] != idx[2] {
                self.triangles.push(idx);
            }
        }
    }

    /// A welded mesh from loose triangles.
    pub fn from_triangles(tris: &[[[i64; 3]; 3]]) -> Self {
        let mut mesh = Self::default();
        mesh.push_welded(&mut BTreeMap::new(), tris);
        mesh
    }

    /// Remove every interior vertex that lies inside one plane or on a straight crease between two
    /// planes, by exact half-edge collapse, then drop unused vertices.
    ///
    /// Rapier's `FIX_INTERNAL_EDGES` corrects contact normals at internal *edges* but not at
    /// vertices: a marble rolling over a vertex picks up a contact whose normal leans toward the
    /// vertex, which kicks it upward (measured in the seam test). A straight run of pieces welds
    /// into long flat strips with a ring of such vertices at every seam; removing them gives the
    /// same surface as one seamless piece. The surface is unchanged: a vertex is removed only when
    /// every rewritten triangle keeps its plane and orientation (integer µm, exact arithmetic), and
    /// vertices on an open boundary or a corner of three or more planes are kept.
    pub fn simplify_flat(&mut self) {
        let mut s = FlatSimplify::new(self);
        while !s.step(self, usize::MAX) {}
        s.finish(self);
    }

    /// Tag every triangle whose three vertices lie on one of [`Self::bends`]' tubes.
    fn mark_bends(&mut self) {
        self.bend_of = self
            .triangles
            .iter()
            .map(|t| {
                let vs = t.map(|v| self.vertices_um[v as usize]);
                self.bends
                    .iter()
                    .position(|b| vs.iter().all(|&v| b.holds(v)))
                    .map_or(0, |k| {
                        let inside = vs.iter().all(|v| self.bends[k].contains(v[0], v[2]));
                        (k as u32 + 1) | if inside { IN_BEND_CELL } else { 0 }
                    })
            })
            .collect();
        self.junction_of = self
            .triangles
            .iter()
            .map(|t| {
                let vs = t.map(|v| self.vertices_um[v as usize]);
                self.junctions
                    .iter()
                    .position(|j| vs.iter().all(|&v| j.holds(v)))
                    .map_or(0, |k| k as u32 + 1)
            })
            .collect();
    }

    /// Tag every triangle whose three vertices lie on one of [`Self::slopes`]' channels.
    fn mark_slopes(&mut self) {
        self.slope_of = self
            .triangles
            .iter()
            .map(|t| {
                let vs = t.map(|v| self.vertices_um[v as usize]);
                (0..self.slopes.len() as u32)
                    .filter(|&k| vs.iter().all(|&v| self.slopes[k as usize].holds(v)))
                    .collect()
            })
            .collect();
    }

    /// Tag every triangle on one of [`Self::arcs`]' rail tubes.
    fn mark_arcs(&mut self) {
        self.arc_of = rail_arc::tag(&self.arcs, &self.vertices_um, &self.triangles);
    }

    /// The static wood collider: vertices relative to `origin_um`, collider placed at the origin.
    pub fn collider(&self, origin_um: [i64; 3]) -> Result<Collider, TriMeshBuilderError> {
        Ok(Self::collider_of(self.trimesh(origin_um)?, origin_um))
    }

    /// [`Self::collider`]'s first half: the trimesh and its BVH, without the `FIX_INTERNAL_EDGES`
    /// data. Parry builds the BVH last in `TriMesh::with_flags`, from the triangles, which its
    /// duplicate-vertex merge leaves in place, so building it first gives the same trimesh. The
    /// editor's staged rebuild runs the halves in separate steps (P3-T4).
    fn trimesh(&self, origin_um: [i64; 3]) -> Result<TriMesh, TriMeshBuilderError> {
        let m = |v: i64| v as f32 * 1.0e-6;
        let vertices = self
            .vertices_um
            .iter()
            .map(|v| {
                Vector::new(
                    m(v[0] - origin_um[0]),
                    m(v[1] - origin_um[1]),
                    m(v[2] - origin_um[2]),
                )
            })
            .collect();
        TriMesh::with_flags(vertices, self.triangles.clone(), TriMeshFlags::empty())
    }

    /// [`Self::collider`]'s second half: the `FIX_INTERNAL_EDGES` data, then the collider.
    fn collider_of(mut trimesh: TriMesh, origin_um: [i64; 3]) -> Collider {
        let m = |v: i64| v as f32 * 1.0e-6;
        // `TriMesh::with_flags` ignores this result too: no topology is requested.
        let _ = trimesh.set_flags(TriMeshFlags::FIX_INTERNAL_EDGES);
        ColliderBuilder::new(SharedShape::new(trimesh))
            .translation(Vector::new(
                m(origin_um[0]),
                m(origin_um[1]),
                m(origin_um[2]),
            ))
            .friction(scene::WOOD_FRICTION)
            .restitution(scene::WOOD_RESTITUTION)
            .collision_groups(crate::mechanism::fixed_groups())
            .build()
    }
}

/// [`ChunkMesh::simplify_flat`] as resumable work: [`Self::step`] visits vertices in the same
/// order and passes as one uninterrupted run, so the result does not depend on how the work is
/// split. The editor's staged rebuild ([`ChunkColliders::rebuild_step`]) bounds each step, since
/// a dense chunk's full simplify took 15 ms in wasm (P3-T4).
#[derive(Debug)]
pub struct FlatSimplify {
    tris: Vec<Option<[u32; 3]>>,
    incident: Vec<Vec<usize>>,
    /// The next vertex of the current pass.
    next: u32,
    /// Whether the current pass collapsed a vertex, so another pass follows.
    changed: bool,
}

impl FlatSimplify {
    pub fn new(mesh: &ChunkMesh) -> Self {
        let mut incident: Vec<Vec<usize>> = vec![Vec::new(); mesh.vertices_um.len()];
        for (i, t) in mesh.triangles.iter().enumerate() {
            for &v in t {
                incident[v as usize].push(i);
            }
        }
        Self {
            tris: mesh.triangles.iter().copied().map(Some).collect(),
            incident,
            next: 0,
            changed: false,
        }
    }

    /// Visit vertices until about `work` units (a vertex plus its incident triangles) are spent or
    /// the simplify is done. Returns whether it is done. `mesh` is the one passed to [`Self::new`].
    pub fn step(&mut self, mesh: &ChunkMesh, work: usize) -> bool {
        let nv = mesh.vertices_um.len() as u32;
        let mut spent = 0usize;
        loop {
            if self.next == nv {
                if !self.changed {
                    return true;
                }
                self.next = 0;
                self.changed = false;
            }
            if spent >= work {
                return false;
            }
            let v = self.next;
            self.next += 1;
            spent = spent.saturating_add(1 + self.incident[v as usize].len());
            if self.visit(&mesh.vertices_um, v) {
                self.changed = true;
            }
        }
    }

    /// Collapse `v` into a neighbour when that keeps the surface; returns whether it did.
    fn visit(&mut self, vertices_um: &[[i64; 3]], v: u32) -> bool {
        let tris = &mut self.tris;
        let incident = &mut self.incident;
        let pos = |v: u32| vertices_um[v as usize].map(i128::from);
        let normal = |t: [u32; 3]| {
            let [a, b, c] = t.map(pos);
            cross(sub(b, a), sub(c, a))
        };
        let live: Vec<usize> = incident[v as usize]
            .iter()
            .copied()
            .filter(|&i| tris[i].is_some())
            .collect();
        if live.is_empty() {
            return false;
        }
        // Edge (v, x) → triangles using it. Interior vertices have exactly two per edge.
        let mut edges: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        for &i in &live {
            for &x in tris[i].as_ref().unwrap() {
                if x != v {
                    edges.entry(x).or_default().push(i);
                }
            }
        }
        if edges.values().any(|ts| ts.len() != 2) {
            return false;
        }
        let same_plane = |tris: &[Option<[u32; 3]>], i: usize, j: usize| {
            let (a, b) = (normal(tris[i].unwrap()), normal(tris[j].unwrap()));
            cross(a, b) == [0; 3] && dot(a, b) > 0
        };
        let mut planes: Vec<usize> = Vec::new();
        for &i in &live {
            if !planes.iter().any(|&p| same_plane(tris, p, i)) {
                planes.push(i);
            }
        }
        let candidates: Vec<u32> = match planes.len() {
            1 => edges.keys().copied().collect(),
            2 => {
                let crease: Vec<u32> = edges
                    .iter()
                    .filter(|(_, ts)| !same_plane(tris, ts[0], ts[1]))
                    .map(|(&x, _)| x)
                    .collect();
                let pv = pos(v);
                match crease[..] {
                    [a, b]
                        if cross(sub(pos(a), pv), sub(pos(b), pv)) == [0; 3]
                            && dot(sub(pos(a), pv), sub(pos(b), pv)) < 0 =>
                    {
                        crease
                    }
                    _ => return false,
                }
            }
            _ => return false,
        };
        for u in candidates {
            // Link condition: v and u share exactly the two vertices opposite edge (v, u).
            let opposite: BTreeSet<u32> = edges[&u]
                .iter()
                .flat_map(|&i| tris[i].unwrap())
                .filter(|&x| x != u && x != v)
                .collect();
            let u_nbrs: BTreeSet<u32> = incident[u as usize]
                .iter()
                .filter_map(|&i| tris[i])
                .flatten()
                .collect();
            let shared = edges
                .keys()
                .filter(|&&x| x != u && u_nbrs.contains(&x))
                .count();
            if opposite.len() != 2 || shared != 2 {
                continue;
            }
            let rewritten: Vec<(usize, [u32; 3])> = live
                .iter()
                .filter(|&&i| !tris[i].unwrap().contains(&u))
                .map(|&i| (i, tris[i].unwrap().map(|x| if x == v { u } else { x })))
                .collect();
            let keeps_plane = rewritten.iter().all(|&(i, t)| {
                let (old, new) = (normal(tris[i].unwrap()), normal(t));
                new != [0; 3] && cross(old, new) == [0; 3] && dot(old, new) > 0
            });
            if !keeps_plane {
                continue;
            }
            for &i in &edges[&u] {
                tris[i] = None;
            }
            for (i, t) in rewritten {
                tris[i] = Some(t);
                incident[u as usize].push(i);
            }
            incident[v as usize].clear();
            return true;
        }
        false
    }

    /// Write the simplified mesh into `mesh` once [`Self::step`] is done: triangle order kept,
    /// vertices renumbered by first use, per-triangle tags cleared.
    pub fn finish(self, mesh: &mut ChunkMesh) {
        let mut remap = vec![u32::MAX; mesh.vertices_um.len()];
        let mut vertices = Vec::new();
        let mut triangles = Vec::new();
        for t in self.tris.into_iter().flatten() {
            triangles.push(t.map(|v| {
                if remap[v as usize] == u32::MAX {
                    remap[v as usize] = vertices.len() as u32;
                    vertices.push(mesh.vertices_um[v as usize]);
                }
                remap[v as usize]
            }));
        }
        mesh.vertices_um = vertices;
        mesh.triangles = triangles;
        mesh.stitched.clear();
        mesh.bend_of.clear();
        mesh.junction_of.clear();
        mesh.slope_of.clear();
        mesh.arc_of.clear();
    }
}

/// Why an edit or a rebuild was refused.
#[derive(Debug, Clone, PartialEq)]
pub enum ChunkError {
    UnknownPiece(PieceId),
    /// The cell is already covered by another placement (contract §9 rule 6).
    Overlap(CellCoord),
    /// No placement is anchored at this cell.
    NotPlaced(CellCoord),
    /// Rapier refused the merged trimesh of this chunk.
    TriMesh(CellCoord, String),
}

impl core::fmt::Display for ChunkError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ChunkError {}

#[derive(Debug, Clone)]
struct Placed {
    placement: Placement,
    cells: Vec<CellCoord>,
    /// Cell faces this piece has collision triangles on.
    closed: BTreeSet<(CellCoord, PortFace)>,
}

/// A chunk collider's slopes and per-triangle slope tags ([`ChunkMesh::slope_of`]).
pub type SlopeTags = (Vec<ChannelSlope>, Vec<Vec<u32>>);

/// A chunk collider's bends and per-triangle bend tags ([`ChunkMesh::bend_of`]).
pub type BendTags = (Vec<ChannelBend>, Vec<u32>);

/// A chunk collider's curved rails and per-triangle arc tags ([`ChunkMesh::arc_of`]).
pub type ArcTags = (Vec<ChannelArc>, Vec<u32>);

/// Placements grouped by chunk, with one merged collider per non-empty chunk.
#[derive(Debug, Default)]
pub struct ChunkColliders {
    /// By anchor cell.
    placed: BTreeMap<CellCoord, Placed>,
    /// Covered cell → anchor of the placement covering it.
    occupancy: BTreeMap<CellCoord, CellCoord>,
    /// Chunk → `(anchor, piece)` of the placements anchored in it, in merge order.
    by_chunk: BTreeMap<CellCoord, BTreeSet<(CellCoord, PieceId)>>,
    handles: BTreeMap<CellCoord, ColliderHandle>,
    /// Stitched-edge masks ([`ChunkMesh::stitched`]) of each chunk collider, for the contact hook.
    stitched: BTreeMap<(u32, u32), Vec<u8>>,
    /// Bends and per-triangle bend tags ([`ChunkMesh::bend_of`]) of each chunk collider.
    bends: BTreeMap<(u32, u32), (Vec<ChannelBend>, Vec<u32>)>,
    /// Slopes and per-triangle slope tags ([`ChunkMesh::slope_of`]) of each chunk collider.
    slopes: BTreeMap<(u32, u32), SlopeTags>,
    /// Junctions and per-triangle junction tags ([`ChunkMesh::junction_of`]) of each chunk
    /// collider.
    junctions: BTreeMap<(u32, u32), (Vec<ChannelJunction>, Vec<u32>)>,
    /// Whether the contact pair being updated has received its exact junction contact, reset
    /// with [`Self::slope_contact_claimed`].
    junction_contact_claimed: std::sync::Mutex<bool>,
    /// Test switch: leave junction triangles to the mesh contact, for the seam gates' control.
    #[cfg(test)]
    pub(crate) no_junction_contact: bool,
    /// Straight channel blocks of every installed chunk collider by cell (see
    /// [`ChannelStraight`]), for the straight contact's ownership across chunk seams.
    straight_cells: BTreeMap<CellCoord, ChannelStraight>,
    /// Fork cells of every installed chunk collider ([`ChunkMesh::forks`]).
    fork_cells: BTreeSet<CellCoord>,
    /// Whether the contact pair being updated has received its exact straight contact, reset
    /// with [`Self::slope_contact_claimed`].
    straight_contact_claimed: std::sync::Mutex<bool>,
    /// Test switch: leave straight channel triangles to the mesh contact, for the straight
    /// gates' control.
    #[cfg(test)]
    pub(crate) no_straight_contact: bool,
    /// Test switch: leave landing floors ([`ChannelSlope::landing`]) to the mesh contact, for the
    /// landing gates' control.
    #[cfg(test)]
    pub(crate) no_landing_contact: bool,
    /// Curved rails and per-triangle arc tags ([`ChunkMesh::arc_of`]) of each chunk collider.
    arcs: BTreeMap<(u32, u32), (Vec<ChannelArc>, Vec<u32>)>,
    /// Whether the contact pair being updated has received its exact rail arc contact, reset with
    /// [`Self::slope_contact_claimed`].
    arc_contact_claimed: std::sync::Mutex<bool>,
    /// Whether the contact pair being updated has received its exact slope contact. Reset by
    /// [`PhysicsHooks::filter_contact_pair`], which the narrow phase calls before it builds and
    /// modifies the pair's manifolds, one pair at a time.
    slope_contact_claimed: std::sync::Mutex<bool>,
    /// Test switch: keep the marble's restitution on a rolling ball's exact slope contact (the
    /// bounce at every triangle hand-over that the hook removes), for the seam test's control.
    #[cfg(test)]
    pub(crate) slope_hand_over_bounces: bool,
    /// Whether the contact pair being updated has received its exact bend contact, reset with
    /// [`Self::slope_contact_claimed`].
    bend_contact_claimed: std::sync::Mutex<bool>,
    /// Test switch: the bend contact as it was before 2026-11-07, evaluated at the start of the
    /// step, copied onto every touching manifold, bouncing at triangle hand-overs and reaching
    /// along the whole straight continuation (the energy gain the seam test's control shows).
    #[cfg(test)]
    pub(crate) bend_contact_at_step_start: bool,
    /// Test switch: drop the bounce on an exact slope or bend contact only when its end-of-step
    /// gap penetrates, as before 2026-11-08 (see [`Self::rolling`]), for the seam gates' control.
    #[cfg(test)]
    pub(crate) end_gap_hand_over: bool,
    /// Test triangles (world µm) merged into chunk (0, 0, 0) with its placements, for the seam
    /// gates' control (a port face cap put back).
    #[cfg(test)]
    pub(crate) extra_triangles: Vec<[[i64; 3]; 3]>,
    /// Test switch: [`FlatSimplify::step`] work per staged simplify step in place of
    /// [`SIMPLIFY_STEP_WORK`], to split the simplify at every vertex or every few.
    #[cfg(test)]
    pub(crate) simplify_step_work: Option<usize>,
    /// Test count of staged rebuilds that reused [`Self::simplified`].
    #[cfg(test)]
    pub(crate) simplify_reused: usize,
    dirty: BTreeSet<CellCoord>,
    /// The chunk [`Self::rebuild_step`] is part way through, and its mesh so far.
    in_flight: Option<InFlight>,
    /// Per chunk, the gathered geometry of its last staged build and that geometry simplified
    /// ([`ChunkMesh::simplify_flat`] is a pure function of it). A chunk dirtied only because a
    /// neighbour's stitched-edge tags may change gathers the same geometry and skips the simplify.
    simplified: BTreeMap<CellCoord, (Geometry, Geometry)>,
}

/// A staged chunk rebuild ([`ChunkColliders::rebuild_step`]).
#[derive(Debug)]
struct InFlight {
    chunk: CellCoord,
    stage: Stage,
    mesh: ChunkMesh,
    /// The gathered mesh's welded geometry, the key of [`ChunkColliders::simplified`].
    gathered: Option<Geometry>,
    /// The weld map while gathering.
    weld: BTreeMap<[i64; 3], u32>,
    /// The built trimesh (none for an empty chunk), between [`Stage::Build`] and [`Stage::Hook`].
    trimesh: Option<TriMesh>,
    /// The built collider (none for an empty chunk), between [`Stage::Hook`] and
    /// [`Stage::Install`].
    collider: Option<Collider>,
}

/// The stages of [`ChunkColliders::rebuild_step`], each a part of [`ChunkColliders::chunk_mesh`]
/// and [`ChunkColliders::install`] in their order.
#[derive(Debug)]
enum Stage {
    /// Gather the chunk's placements from this index on, [`GATHER_STEP_PIECES`] per step.
    Gather(usize),
    Simplify(Option<Box<FlatSimplify>>),
    MarkStitched,
    MarkBends,
    MarkSlopes,
    MarkArcs,
    /// The trimesh and its BVH ([`ChunkMesh::trimesh`]).
    Build,
    /// Its `FIX_INTERNAL_EDGES` data, then the collider with its active hooks.
    Hook,
    Install,
}

/// Placements gathered per staged gather step (a chunk holds up to 64 on one layer).
const GATHER_STEP_PIECES: usize = 16;

/// A mesh's welded vertices (µm) and triangles.
type Geometry = (Vec<[i64; 3]>, Vec<[u32; 3]>);

/// Work units ([`FlatSimplify::step`]) per staged simplify step: at most 0.6 ms natively on the
/// 2,000-piece fill's chunks (measured 2026-11-08 on a loaded host, P3-T4).
const SIMPLIFY_STEP_WORK: usize = 4_000;

impl ChunkColliders {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a placement and mark its chunk (and the chunks of touching neighbours) dirty.
    pub fn insert(&mut self, kit: &KitCatalog, p: Placement) -> Result<(), ChunkError> {
        let piece = kit
            .piece(&p.piece)
            .ok_or_else(|| ChunkError::UnknownPiece(p.piece.clone()))?;
        let cells = p.world_cells(&piece.def);
        if let Some(&c) = cells.iter().find(|c| self.occupancy.contains_key(c)) {
            return Err(ChunkError::Overlap(c));
        }
        let closed = placed_triangles(piece, &p)
            .iter()
            .filter_map(|t| grid_face(t, &cells))
            .collect();
        for &c in &cells {
            self.occupancy.insert(c, p.anchor);
        }
        self.by_chunk
            .entry(chunk_of(p.anchor))
            .or_default()
            .insert((p.anchor, p.piece.clone()));
        let anchor = p.anchor;
        self.placed.insert(
            anchor,
            Placed {
                placement: p,
                cells,
                closed,
            },
        );
        self.mark_dirty(anchor);
        Ok(())
    }

    /// Remove the placement anchored at `anchor` and mark the affected chunks dirty.
    pub fn remove(&mut self, anchor: CellCoord) -> Result<Placement, ChunkError> {
        self.mark_dirty(anchor);
        let placed = self
            .placed
            .remove(&anchor)
            .ok_or(ChunkError::NotPlaced(anchor))?;
        for c in &placed.cells {
            self.occupancy.remove(c);
        }
        let chunk = chunk_of(anchor);
        if let Some(set) = self.by_chunk.get_mut(&chunk) {
            set.remove(&(anchor, placed.placement.piece.clone()));
            if set.is_empty() {
                self.by_chunk.remove(&chunk);
            }
        }
        Ok(placed.placement)
    }

    /// Dirty the chunk of `anchor` and of every placement adjacent to it across a face, edge or
    /// corner: their shared-face culling and stitched-edge tags may change ([`Self::mark_stitched`]
    /// finds the cell across an edge on a cell corner diagonally).
    fn mark_dirty(&mut self, anchor: CellCoord) {
        let Some(placed) = self.placed.get(&anchor) else {
            return;
        };
        let mut touched = vec![chunk_of(anchor)];
        for &c in &placed.cells {
            for i in 0..27 {
                let d = CellCoord::new(i % 3 - 1, i / 3 % 3 - 1, i / 9 - 1);
                if let Some(&other) = self.occupancy.get(&(c + d)) {
                    touched.push(chunk_of(other));
                }
            }
        }
        for chunk in touched {
            self.dirty.insert(chunk);
            // A staged rebuild of a chunk that changed again starts over.
            if self.in_flight.as_ref().is_some_and(|f| f.chunk == chunk) {
                self.in_flight = None;
            }
        }
    }

    /// Chunks waiting for [`Self::rebuild`], in order.
    pub fn dirty_chunks(&self) -> impl Iterator<Item = CellCoord> + '_ {
        self.dirty.iter().copied()
    }

    /// Chunks that currently hold a collider, in order.
    /// The placement covering `cell`, if any.
    pub fn placement_at(&self, cell: CellCoord) -> Option<&Placement> {
        let anchor = self.occupancy.get(&cell)?;
        self.placed.get(anchor).map(|p| &p.placement)
    }

    pub fn chunks(&self) -> impl Iterator<Item = (CellCoord, ColliderHandle)> + '_ {
        self.handles.iter().map(|(&c, &h)| (c, h))
    }

    /// The contact-hook tags installed with a chunk collider: stitched-edge masks, then the
    /// bends, slopes and rail arcs with their per-triangle tags. For tests that compare an edited
    /// world with a fresh one.
    pub fn hook_tags(&self, h: ColliderHandle) -> Option<(&[u8], &BendTags, &SlopeTags, &ArcTags)> {
        let k = h.into_raw_parts();
        Some((
            self.stitched.get(&k)?,
            self.bends.get(&k)?,
            self.slopes.get(&k)?,
            self.arcs.get(&k)?,
        ))
    }

    /// The merged mesh of one chunk from the current placements.
    pub fn chunk_mesh(&self, kit: &KitCatalog, chunk: CellCoord) -> ChunkMesh {
        let mut mesh = self.gathered(kit, chunk);
        mesh.simplify_flat();
        self.marked(&mut mesh, chunk);
        mesh
    }

    /// [`Self::chunk_mesh`]'s first stage: the welded, culled triangles, bends and slopes.
    fn gathered(&self, kit: &KitCatalog, chunk: CellCoord) -> ChunkMesh {
        let mut mesh = ChunkMesh::default();
        self.gather(kit, chunk, &mut mesh, &mut BTreeMap::new(), 0, usize::MAX);
        mesh
    }

    /// Gather up to `count` of `chunk`'s placements from index `from` on into `mesh`, then (once
    /// every placement is in) the test triangles. Returns whether every placement is in.
    fn gather(
        &self,
        kit: &KitCatalog,
        chunk: CellCoord,
        mesh: &mut ChunkMesh,
        weld: &mut BTreeMap<[i64; 3], u32>,
        from: usize,
        count: usize,
    ) -> bool {
        let Some(members) = self.by_chunk.get(&chunk) else {
            return true;
        };
        for (anchor, id) in members.iter().skip(from).take(count) {
            let placed = &self.placed[anchor];
            let Some(piece) = kit.piece(id) else { continue };
            let tris: Vec<[[i64; 3]; 3]> = placed_triangles(piece, &placed.placement)
                .into_iter()
                .filter(|t| !self.culled(placed, t))
                .collect();
            mesh.push_welded(weld, &tris);
            mesh.bends.extend(ChannelBend::of(piece, &placed.placement));
            let junction = ChannelJunction::of(piece, &placed.placement);
            if junction.as_ref().is_some_and(|j| j.fork) {
                mesh.forks.push(placed.placement.anchor);
            }
            mesh.junctions.extend(junction);
            mesh.slopes
                .extend(ChannelSlope::of(piece, &placed.placement).or_else(|| {
                    #[cfg(test)]
                    if self.no_landing_contact {
                        return None;
                    }
                    ChannelSlope::landing(piece, &placed.placement)
                }));
            mesh.arcs.extend(ChannelArc::of(piece, &placed.placement));
            mesh.trapdoor |= !piece.animated_parts.is_empty()
                && piece
                    .def
                    .ports
                    .iter()
                    .any(|q| q.face == PortFace::Bottom && q.class == PortClass::Drop);
            mesh.straights
                .extend(ChannelStraight::of(piece, &placed.placement));
        }
        if from.saturating_add(count) < members.len() {
            return false;
        }
        #[cfg(test)]
        if chunk == CellCoord::new(0, 0, 0) {
            mesh.push_welded(weld, &self.extra_triangles);
        }
        true
    }

    /// [`Self::chunk_mesh`]'s last stage, after [`ChunkMesh::simplify_flat`]: the stitched-edge,
    /// bend and slope tags.
    fn marked(&self, mesh: &mut ChunkMesh, chunk: CellCoord) {
        self.mark_stitched(mesh, chunk);
        mesh.mark_bends();
        mesh.mark_slopes();
        mesh.mark_arcs();
    }

    /// Mark open edges that lie in a cell-face plane with a placement from another chunk across.
    fn mark_stitched(&self, mesh: &mut ChunkMesh, chunk: CellCoord) {
        let mut uses: BTreeMap<(u32, u32), u32> = BTreeMap::new();
        for t in &mesh.triangles {
            for e in 0..3 {
                let (a, b) = (t[e], t[(e + 1) % 3]);
                *uses.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
        let cell = i64::from(CELL_UM);
        let half = i64::from(HALF_CELL_UM);
        // Cell index from two summed coordinates (the edge midpoint).
        let horiz = |s: i64| (s + 2 * half).div_euclid(2 * cell) as i32;
        let vert = |s: i64| s.div_euclid(2 * cell) as i32;
        let foreign = |c: CellCoord| {
            self.occupancy
                .get(&c)
                .is_some_and(|&a| chunk_of(a) != chunk)
        };
        let stitched_edge = |p: [i64; 3], q: [i64; 3]| {
            let s = [p[0] + q[0], p[1] + q[1], p[2] + q[2]];
            let x_plane = p[0] == q[0] && (p[0] - half).rem_euclid(cell) == 0;
            let z_plane = p[2] == q[2] && (p[2] - half).rem_euclid(cell) == 0;
            let y_plane = p[1] == q[1] && p[1].rem_euclid(cell) == 0;
            (x_plane && {
                let k = (p[0] - half).div_euclid(cell) as i32;
                let (y, z) = (vert(s[1]), horiz(s[2]));
                foreign(CellCoord::new(k, y, z)) || foreign(CellCoord::new(k + 1, y, z))
            }) || (z_plane && {
                let k = (p[2] - half).div_euclid(cell) as i32;
                let (x, y) = (horiz(s[0]), vert(s[1]));
                foreign(CellCoord::new(x, y, k)) || foreign(CellCoord::new(x, y, k + 1))
            }) || (y_plane && {
                let k = p[1].div_euclid(cell) as i32;
                let (x, z) = (horiz(s[0]), horiz(s[2]));
                foreign(CellCoord::new(x, k - 1, z)) || foreign(CellCoord::new(x, k, z))
            })
        };
        let edge_bits: Vec<u8> = mesh
            .triangles
            .iter()
            .map(|t| {
                (0..3).fold(0u8, |bits, e| {
                    let (a, b) = (t[e], t[(e + 1) % 3]);
                    let open = uses[&(a.min(b), a.max(b))] == 1;
                    let (p, q) = (mesh.vertices_um[a as usize], mesh.vertices_um[b as usize]);
                    if open && stitched_edge(p, q) {
                        bits | (1 << e)
                    } else {
                        bits
                    }
                })
            })
            .collect();
        let mut stitched_vertex = vec![false; mesh.vertices_um.len()];
        for (t, &bits) in mesh.triangles.iter().zip(&edge_bits) {
            for e in 0..3 {
                if bits & (1 << e) != 0 {
                    stitched_vertex[t[e] as usize] = true;
                    stitched_vertex[t[(e + 1) % 3] as usize] = true;
                }
            }
        }
        mesh.stitched = mesh
            .triangles
            .iter()
            .zip(edge_bits)
            .map(|(t, bits)| {
                (0..3).fold(bits, |bits, i| {
                    if stitched_vertex[t[i] as usize] {
                        bits | (1 << (3 + i))
                    } else {
                        bits
                    }
                })
            })
            .collect();
    }

    /// Whether triangle `t` of `placed` sits on a face shared with another closed face.
    fn culled(&self, placed: &Placed, t: &[[i64; 3]; 3]) -> bool {
        let Some((cell, face)) = grid_face(t, &placed.cells) else {
            return false;
        };
        let across = cell + face.normal();
        self.occupancy
            .get(&across)
            .and_then(|a| self.placed.get(a))
            .is_some_and(|other| {
                other.placement.anchor != placed.placement.anchor
                    && other.closed.contains(&(across, face.opposite()))
            })
    }

    /// Rebuild every dirty chunk in chunk order: drop its old collider and insert the new merged
    /// trimesh (none for an empty chunk). Stops at the first chunk Rapier refuses; that chunk and
    /// the ones after it stay dirty.
    pub fn rebuild(
        &mut self,
        kit: &KitCatalog,
        world: &mut PhysicsWorld,
    ) -> Result<(), ChunkError> {
        self.in_flight = None;
        while self.rebuild_next(kit, world)?.is_some() {}
        Ok(())
    }

    /// One stage of a staged rebuild of the first dirty chunk: gather, simplify, tag, install.
    /// Each stage is a part of [`Self::chunk_mesh`] in its order, so the installed collider is the
    /// one [`Self::rebuild_next`] builds; a chunk dirtied again mid-way starts over. Returns the
    /// chunk and whether this step installed it, or `None` when nothing is dirty. The editor
    /// (P3-T4) spreads a dense chunk's rebuild over frames with it.
    pub fn rebuild_step(
        &mut self,
        kit: &KitCatalog,
        world: &mut PhysicsWorld,
    ) -> Result<Option<(CellCoord, bool)>, ChunkError> {
        let Some(mut f) = self.in_flight.take() else {
            let Some(chunk) = self.dirty.first().copied() else {
                return Ok(None);
            };
            self.in_flight = Some(InFlight {
                chunk,
                stage: Stage::Gather(0),
                mesh: ChunkMesh::default(),
                gathered: None,
                weld: BTreeMap::new(),
                trimesh: None,
                collider: None,
            });
            return self.rebuild_step(kit, world);
        };
        let chunk = f.chunk;
        match &mut f.stage {
            Stage::Gather(from) => {
                let all = self.gather(
                    kit,
                    chunk,
                    &mut f.mesh,
                    &mut f.weld,
                    *from,
                    GATHER_STEP_PIECES,
                );
                *from += GATHER_STEP_PIECES;
                if all {
                    f.weld = BTreeMap::new();
                    f.stage = match self.simplified.get(&chunk) {
                        Some((key, done))
                            if key.0 == f.mesh.vertices_um && key.1 == f.mesh.triangles =>
                        {
                            f.mesh.vertices_um.clone_from(&done.0);
                            f.mesh.triangles.clone_from(&done.1);
                            #[cfg(test)]
                            {
                                self.simplify_reused += 1;
                            }
                            Stage::MarkStitched
                        }
                        _ => {
                            f.gathered =
                                Some((f.mesh.vertices_um.clone(), f.mesh.triangles.clone()));
                            Stage::Simplify(None)
                        }
                    };
                }
            }
            Stage::Simplify(state) => {
                let s = state.get_or_insert_with(|| Box::new(FlatSimplify::new(&f.mesh)));
                #[cfg(test)]
                let work = self.simplify_step_work.unwrap_or(SIMPLIFY_STEP_WORK);
                #[cfg(not(test))]
                let work = SIMPLIFY_STEP_WORK;
                if s.step(&f.mesh, work) {
                    state.take().unwrap().finish(&mut f.mesh);
                    if let Some(key) = f.gathered.take() {
                        let done = (f.mesh.vertices_um.clone(), f.mesh.triangles.clone());
                        self.simplified.insert(chunk, (key, done));
                    }
                    f.stage = Stage::MarkStitched;
                }
            }
            Stage::MarkStitched => {
                self.mark_stitched(&mut f.mesh, chunk);
                f.stage = Stage::MarkBends;
            }
            Stage::MarkBends => {
                f.mesh.mark_bends();
                f.stage = Stage::MarkSlopes;
            }
            Stage::MarkSlopes => {
                f.mesh.mark_slopes();
                f.stage = Stage::MarkArcs;
            }
            Stage::MarkArcs => {
                f.mesh.mark_arcs();
                f.stage = Stage::Build;
            }
            Stage::Build => {
                match Self::trimesh_of(&f.mesh, chunk) {
                    Ok(t) => f.trimesh = t,
                    Err(e) => {
                        // As in [`Self::install`]: the old collider is gone, the chunk stays dirty.
                        self.remove_installed(world, chunk);
                        return Err(e);
                    }
                }
                f.stage = Stage::Hook;
            }
            Stage::Hook => {
                f.collider = f.trimesh.take().map(|t| Self::hooked(&f.mesh, chunk, t));
                f.stage = Stage::Install;
            }
            Stage::Install => {
                self.swap_in(world, chunk, f.mesh, f.collider);
                return Ok(Some((chunk, true)));
            }
        }
        self.in_flight = Some(f);
        Ok(Some((chunk, false)))
    }

    /// Rebuild the first dirty chunk only, as [`Self::rebuild`] does for each, and return it
    /// (`None` when nothing is dirty). The editor (P3-T4) spreads a large edit's chunks over
    /// frames with it.
    pub fn rebuild_next(
        &mut self,
        kit: &KitCatalog,
        world: &mut PhysicsWorld,
    ) -> Result<Option<CellCoord>, ChunkError> {
        let Some(chunk) = self.dirty.first().copied() else {
            return Ok(None);
        };
        if self.in_flight.as_ref().is_some_and(|f| f.chunk == chunk) {
            self.in_flight = None;
        }
        let mesh = self.chunk_mesh(kit, chunk);
        self.install(world, chunk, mesh)?;
        Ok(Some(chunk))
    }

    /// Whether the ball rolls on an exact slope or bend surface, so its contact takes no bounce:
    /// the contact's end-of-step `gap` penetrates, or the surface at the ball's current centre
    /// (`now`) does. On a tight concave turn the end-of-step contact, carried back along its
    /// normal, reads a positive gap while the ball is still pressed into the floor, and the bounce
    /// kept there threw it off: 78 mm/s upward at the steep slope's exit seam (on its former cubic
    /// profile) and contact on
    /// alternate ticks in a `blk_curve90_l2` taken at 0.53 m/s (kick 62.55), measured 2026-11-08.
    /// A ball falling slowly onto the surface has a positive gap at both and still bounces. One
    /// falling fast meets the contact already penetrating and loses its bounce here, so a landing
    /// floor also checks the approach speed ([`LANDING_ROLLING_M_S`]).
    ///
    /// The ball's current gap counts as touching up to [`ROLLING_GAP_M`]: a rolling ball sits at
    /// a gap of a few µm either side of zero, and a centred marble leaving the gentle slope read
    /// +4 nm and +6 nm on two hand-over ticks, kept the bounce and kicked 20.48 mm/s at the exit
    /// (measured 2026-11-09, 0.3 m/s off seven exact straights).
    fn rolling(&self, gap: f32, now: impl FnOnce() -> Option<(Vector, f32)>) -> bool {
        #[cfg(test)]
        if self.end_gap_hand_over {
            return gap <= 0.0;
        }
        gap <= 0.0 || now().is_some_and(|(_, g)| g <= ROLLING_GAP_M)
    }

    /// The straight that owns a ball of centre `c` and radius `r` (metres), if any: the straight
    /// whose cell holds the centre, when each cell along the channel within a ball diameter of
    /// the centre is empty or a straight on the same tube line. Next to any other piece the
    /// contact stays as it was, so every seam with a slope, bend, junction, rail or mechanism
    /// keeps its own hand-over.
    fn straight_owner(&self, c: Vector, r: f32) -> Option<&ChannelStraight> {
        #[cfg(test)]
        if self.no_straight_contact {
            return None;
        }
        if self.straight_cells.is_empty() {
            return None;
        }
        let cell = CELL_UM as f32 * 1.0e-6;
        let half = 0.5 * cell;
        let at = CellCoord::new(
            ((c.x + half) / cell).floor() as i32,
            (c.y / cell).floor() as i32,
            ((c.z + half) / cell).floor() as i32,
        );
        let s = self.straight_cells.get(&at)?;
        let (index, step) = if s.axis == 0 {
            (at.x, CellCoord::new(1, 0, 0))
        } else {
            (at.z, CellCoord::new(0, 0, 1))
        };
        let lo = index as f32 * cell - half;
        let along = c[s.axis];
        let reach = 2.0 * r;
        let clear = |n: CellCoord| {
            !self.occupancy.contains_key(&n)
                || self.straight_cells.get(&n).is_some_and(|o| o.same_tube(s))
                || (FORK_HAND_OVER && self.fork_cells.contains(&n))
        };
        if along - lo < reach && !clear(at + CellCoord::new(-step.x, 0, -step.z)) {
            return None;
        }
        if lo + cell - along < reach && !clear(at + step) {
            return None;
        }
        Some(s)
    }

    /// Replace `chunk`'s collider with `mesh` (none when empty) and clear its dirty mark.
    fn install(
        &mut self,
        world: &mut PhysicsWorld,
        chunk: CellCoord,
        mesh: ChunkMesh,
    ) -> Result<(), ChunkError> {
        self.remove_installed(world, chunk);
        let collider = Self::built(&mesh, chunk)?;
        self.swap_in(world, chunk, mesh, collider);
        Ok(())
    }

    /// Drop `chunk`'s collider and its contact-hook tags, if it has one.
    fn remove_installed(&mut self, world: &mut PhysicsWorld, chunk: CellCoord) {
        self.straight_cells.retain(|&c, _| chunk_of(c) != chunk);
        self.fork_cells.retain(|&c| chunk_of(c) != chunk);
        if let Some(h) = self.handles.remove(&chunk) {
            world.remove_collider(h);
            self.stitched.remove(&h.into_raw_parts());
            self.bends.remove(&h.into_raw_parts());
            self.junctions.remove(&h.into_raw_parts());
            self.slopes.remove(&h.into_raw_parts());
            self.arcs.remove(&h.into_raw_parts());
        }
    }

    /// The collider of a tagged chunk mesh with its active hooks (none when empty).
    fn built(mesh: &ChunkMesh, chunk: CellCoord) -> Result<Option<Collider>, ChunkError> {
        Ok(Self::trimesh_of(mesh, chunk)?.map(|t| Self::hooked(mesh, chunk, t)))
    }

    /// [`Self::built`]'s first half ([`ChunkMesh::trimesh`]), none for an empty chunk.
    fn trimesh_of(mesh: &ChunkMesh, chunk: CellCoord) -> Result<Option<TriMesh>, ChunkError> {
        if mesh.triangles.is_empty() {
            return Ok(None);
        }
        mesh.trimesh(chunk_origin_um(chunk))
            .map(Some)
            .map_err(|e| ChunkError::TriMesh(chunk, format!("{e:?}")))
    }

    /// [`Self::built`]'s second half: the collider of `trimesh` with `mesh`'s active hooks.
    fn hooked(mesh: &ChunkMesh, chunk: CellCoord, trimesh: TriMesh) -> Collider {
        let mut collider = ChunkMesh::collider_of(trimesh, chunk_origin_um(chunk));
        if mesh.slope_of.iter().any(|k| !k.is_empty())
            || mesh.bend_of.iter().any(|&b| b != 0)
            || mesh.junction_of.iter().any(|&j| j != 0)
            || mesh.arc_of.iter().any(|&a| a != 0)
            || !mesh.straights.is_empty()
        {
            collider.set_active_hooks(
                ActiveHooks::MODIFY_SOLVER_CONTACTS | ActiveHooks::FILTER_CONTACT_PAIRS,
            );
        } else if mesh.stitched.iter().any(|&b| b != 0)
            || mesh.bend_of.iter().any(|&b| b != 0)
            || mesh.trapdoor
        {
            collider.set_active_hooks(ActiveHooks::MODIFY_SOLVER_CONTACTS);
        }
        collider
    }

    /// Install `collider` (built from `mesh`) as `chunk`'s, replacing the old one, with `mesh`'s
    /// contact-hook tags, and clear the chunk's dirty mark.
    fn swap_in(
        &mut self,
        world: &mut PhysicsWorld,
        chunk: CellCoord,
        mesh: ChunkMesh,
        collider: Option<Collider>,
    ) {
        self.remove_installed(world, chunk);
        if let Some(collider) = collider {
            let h = world.colliders.insert(collider);
            self.handles.insert(chunk, h);
            self.stitched.insert(h.into_raw_parts(), mesh.stitched);
            self.bends
                .insert(h.into_raw_parts(), (mesh.bends, mesh.bend_of));
            self.junctions
                .insert(h.into_raw_parts(), (mesh.junctions, mesh.junction_of));
            self.slopes
                .insert(h.into_raw_parts(), (mesh.slopes, mesh.slope_of));
            self.arcs
                .insert(h.into_raw_parts(), (mesh.arcs, mesh.arc_of));
            self.straight_cells
                .extend(mesh.straights.iter().map(|s| (s.cell, *s)));
            self.fork_cells.extend(mesh.forks.iter().copied());
        }
        self.dirty.remove(&chunk);
    }
}

/// Contact fix at chunk boundaries. `FIX_INTERNAL_EDGES` only sees one trimesh, so where a run
/// crosses from one chunk collider into the next, the open edges and vertices on the chunk face
/// produce contacts whose normal leans toward the edge (a measured 60+ mm/s vertical kick). For a
/// contact that lies on a stitched edge, the manifold normal is replaced by that triangle's face
/// normal, which is what one merged mesh would have produced.
impl ChunkColliders {
    /// Whether `h` is one of the chunk colliders.
    pub(crate) fn is_chunk(&self, h: ColliderHandle) -> bool {
        self.stitched.contains_key(&h.into_raw_parts())
    }
}

impl PhysicsHooks for ChunkColliders {
    fn filter_contact_pair(&self, _: &PairFilterContext) -> Option<SolverFlags> {
        *self
            .slope_contact_claimed
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = false;
        *self
            .bend_contact_claimed
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = false;
        *self
            .arc_contact_claimed
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = false;
        *self
            .junction_contact_claimed
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = false;
        *self
            .straight_contact_claimed
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = false;
        Some(SolverFlags::default())
    }

    fn modify_solver_contacts(&self, ctx: &mut ContactModificationContext) {
        let (handle, other, ball_body, first) =
            if self.stitched.contains_key(&ctx.collider1.into_raw_parts()) {
                (ctx.collider1, ctx.collider2, ctx.rigid_body2, true)
            } else if self.stitched.contains_key(&ctx.collider2.into_raw_parts()) {
                (ctx.collider2, ctx.collider1, ctx.rigid_body1, false)
            } else {
                return;
            };
        let colliders = ctx.colliders;
        let bodies = ctx.bodies;
        let (Some(ball), Some(ball_body)) = (colliders[other].shape().as_ball(), ball_body) else {
            return;
        };
        let centre = bodies[ball_body].translation();
        let Some(rigid) = ctx.rigid_mut() else {
            return;
        };
        let tri = if first {
            rigid.manifold.subshape1
        } else {
            rigid.manifold.subshape2
        };
        if let Some(s) = self.straight_owner(centre, ball.radius) {
            let on_tube = colliders[handle]
                .shape()
                .as_trimesh()
                .map(|m| m.triangle(tri))
                .is_some_and(|t| {
                    let pose = colliders[handle].position();
                    [t.a, t.b, t.c].iter().all(|&p| s.on_tube(pose * p))
                });
            if on_tube {
                // One contact against the exact tube (see [`ChannelStraight`]), on the first
                // manifold of the pair whose chunk holds the straight, evaluated at the end of
                // the step and without a bounce at the triangle hand-overs, as on a bend. The
                // tube's triangles in a neighbouring chunk lose their contacts: the straight's
                // own chunk carries the one contact.
                if rigid.solver_contacts.is_empty() {
                    return;
                }
                if self.handles.get(&chunk_of(s.cell)) != Some(&handle) {
                    rigid.solver_contacts.clear();
                    return;
                }
                let v = bodies[ball_body].linvel();
                let now = s.contact(centre, ball.radius);
                let Some((n, gap)) = s
                    .contact(centre + v * crate::DT, ball.radius)
                    .map(|(n, g)| (n, g - v.dot(n) * crate::DT))
                else {
                    return;
                };
                let mut claimed = self
                    .straight_contact_claimed
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if *claimed {
                    rigid.solver_contacts.clear();
                    return;
                }
                *claimed = true;
                set_one_contact(rigid, first, centre, ball.radius, n, gap);
                // A marble dropped onto a straight (a controlled drop, PRD §5.4) meets it already
                // penetrating, so as on a landing floor it keeps its bounce while it approaches
                // faster than LANDING_ROLLING_M_S.
                let approach = -v.dot(n);
                if self.rolling(gap, || now) && approach <= LANDING_ROLLING_M_S {
                    *rigid.restitution = 0.0;
                }
                return;
            }
            if self
                .junctions
                .get(&handle.into_raw_parts())
                .is_some_and(|(_, tags)| tags.get(tri as usize).is_some_and(|&t| t != 0))
            {
                // A fork's channel next to the straight: the straight's contact holds the ball
                // once its centre has left the fork's cell.
                rigid.solver_contacts.clear();
                return;
            }
        }
        if let Some((slopes, slope_of)) = self.slopes.get(&handle.into_raw_parts())
            && let Some(tagged) = slope_of.get(tri as usize)
            && !tagged.is_empty()
            && let Some(k) = slopes
                .iter()
                .position(|s| s.covers(centre, 0.0))
                .or_else(|| {
                    slopes
                        .iter()
                        .position(|s| s.covers(centre, 2.0 * ball.radius))
                })
            && tagged.contains(&(k as u32))
            && let Some((n, gap)) = {
                // Evaluated where the ball will be at the end of the step and carried back along
                // the normal: the solver holds a contact's normal for the whole step, and on a
                // curved floor a start-of-step contact lags the surface, so the ball sinks and
                // chatters on alternate ticks (measured 2026-11-07: kick 29.19 at the entry and
                // 29.06 inside the slope).
                let v = bodies[ball_body].linvel();
                slopes[k]
                    .contact(centre + v * crate::DT, ball.radius)
                    .map(|(n, g)| (n, g - v.dot(n) * crate::DT))
            }
        {
            // One contact against the exact slope channel (see [`ChannelSlope`]). The slope whose
            // cell holds the ball owns it, else the slope it is about to enter or has just left
            // (within a ball diameter, past the reach of any speculative contact with the slope's
            // triangles), and only on triangles of that slope's channel or flat continuation.
            //
            // The contact goes on the pair's first such manifold only. Every manifold would carry
            // the same contact, and the solver's soft contacts get stiffer with each copy, so a
            // copy gained or lost as the ball crosses a triangle edge kicks it: at the slope seam
            // vertical speed went −12.4, +8.2, −24.7 mm/s over three ticks (kick 27), measured
            // 2026-11-07.
            if rigid.solver_contacts.is_empty() {
                return;
            }
            let mut claimed = self
                .slope_contact_claimed
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if *claimed {
                rigid.solver_contacts.clear();
            } else {
                *claimed = true;
                set_one_contact(rigid, first, centre, ball.radius, n, gap);
                // No bounce while the ball rolls on the surface. Rapier applies restitution to a
                // contact whose manifold point holds no warm-start impulse, and the claim moves to
                // a fresh manifold whenever the ball crosses a triangle edge. In the concave lower
                // half the floor turns into the motion, so the ball approaches the new contact
                // at 11–64 mm/s along its normal and bounced off it at 0.66 of that: kicks 15–73
                // mm/s on exactly the hand-over ticks, 48.55 over 405–445 mm (measured
                // 2026-11-07; in the convex upper half the approach is +2–6 mm/s, separating, and
                // the hand-overs did nothing). A rolling ball's contact is penetrating (−35 to
                // −200 µm here), while a ball dropped slowly onto the slope first meets it as a
                // speculative contact with a positive gap and still bounces. One falling at about
                // 1 m/s or more meets it already penetrating, so the rule drops its bounce too:
                // only a landing floor guards against that (below).
                #[cfg(test)]
                let keep = self.slope_hand_over_bounces;
                #[cfg(not(test))]
                let keep = false;
                // A marble falling onto a landing floor ([`ChannelSlope::landing`]) moves 5–7 mm
                // a tick and first meets it already penetrating (−11 to −250 µm at the start of
                // the step, measured 2026-11-08 in the tower), so there the bounce is dropped only
                // while the ball also approaches no faster than LANDING_ROLLING_M_S.
                let landing = slopes[k].len_um != i64::from(CELL_UM);
                let approach = -bodies[ball_body].linvel().dot(n);
                if self.rolling(gap, || slopes[k].contact(centre, ball.radius))
                    && !keep
                    && !(landing && approach > LANDING_ROLLING_M_S)
                {
                    *rigid.restitution = 0.0;
                }
            }
            return;
        }
        if let Some((junctions, junction_of)) = self.junctions.get(&handle.into_raw_parts())
            && let Some(&tag @ 1..) = junction_of.get(tri as usize)
            && {
                #[cfg(test)]
                let on = !self.no_junction_contact;
                #[cfg(not(test))]
                let on = true;
                on
            }
            && let Some((n, gap, now)) = {
                // The junction whose cell holds the ball owns the contact, else the tagged one
                // within a ball diameter of its cell, evaluated at the end of the step as on a
                // bend.
                let v = bodies[ball_body].linvel();
                junctions
                    .iter()
                    .find(|j| j.reaches(centre, 0.0))
                    .or_else(|| {
                        let j = &junctions[tag as usize - 1];
                        j.reaches(centre, 2.0 * ball.radius).then_some(j)
                    })
                    .and_then(|j| {
                        let now = j.contact(centre, ball.radius);
                        j.contact(centre + v * crate::DT, ball.radius)
                            .map(|(n, g)| (n, g - v.dot(n) * crate::DT, now))
                    })
            }
        {
            // One contact against the exact union of the junction's paths (see
            // [`ChannelJunction`]), on the pair's first junction manifold only and without a
            // bounce at the triangle hand-overs, as on a bend.
            if rigid.solver_contacts.is_empty() {
                return;
            }
            let mut claimed = self
                .junction_contact_claimed
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if *claimed {
                rigid.solver_contacts.clear();
                return;
            }
            *claimed = true;
            set_one_contact(rigid, first, centre, ball.radius, n, gap);
            if self.rolling(gap, || now) {
                *rigid.restitution = 0.0;
            }
            return;
        }
        if let Some((bends, bend_of)) = self.bends.get(&handle.into_raw_parts())
            && bend_of.get(tri as usize).is_some_and(|&t| t != 0)
            && !bends.iter().any(|b| {
                b.contains(
                    (centre.x * 1.0e6).round() as i64,
                    (centre.z * 1.0e6).round() as i64,
                )
            })
            && self
                .slopes
                .get(&handle.into_raw_parts())
                .is_some_and(|(slopes, _)| slopes.iter().any(|s| s.covers(centre, 0.0)))
        {
            // A ball centred over a slope's cell belongs to the slope's contact. The bend tube's
            // continuation past its cell is level, so a bend triangle still in reach would hold
            // the ball up while the slope floor falls away under it: off the mirrored curve90_l2
            // onto the gentle slope the ball then dropped clear for one tick (kick 26.03, 7.76
            // with this rule, measured 2026-11-08 in
            // `curve90_l2_seams_and_interiors_stay_under_the_kick_threshold`).
            rigid.solver_contacts.clear();
            return;
        }
        if let Some((bends, bend_of)) = self.bends.get(&handle.into_raw_parts())
            && let Some(&tag @ 1..) = bend_of.get(tri as usize)
            && let Some((n, gap, now)) = {
                // A straight triangle can lie on the continuation of several bends: the bend
                // whose cell holds the ball owns the contact. Off every bend cell a straight
                // triangle's tube holds only along its own span, so a ball rolling past the next
                // seam (onto a slope, say) keeps the mesh contact there, and only within a ball
                // diameter of the bend's cell: the flat-vertex collapse welds a straight run into
                // strips as long as the run, so without that reach every straight in a bend's
                // row took the exact tube contact.
                let um = |v: f32| (v * 1.0e6).round() as i64;
                let t = colliders[handle]
                    .shape()
                    .as_trimesh()
                    .map(|m| m.triangle(tri));
                let pose = colliders[handle].position();
                #[cfg(test)]
                let legacy = self.bend_contact_at_step_start;
                #[cfg(not(test))]
                let legacy = false;
                // Evaluated where the ball will be at the end of the step and carried back along
                // the normal, as on the slope: the solver holds the normal for the whole step, so
                // a start-of-step contact on the tube lags the wall and pumps energy into a
                // marble rocking across the channel (see [`ChannelBend`]).
                let v = bodies[ball_body].linvel();
                let ahead = if legacy { 0.0 } else { crate::DT };
                let eval = |b: &ChannelBend| {
                    let now = b.contact(centre, ball.radius);
                    b.contact(centre + v * ahead, ball.radius)
                        .map(|(n, g)| (n, g - v.dot(n) * ahead, now))
                };
                match bends
                    .iter()
                    .find(|b| b.contains(um(centre.x), um(centre.z)))
                {
                    Some(b) => eval(b),
                    None => {
                        let b = &bends[(tag & !IN_BEND_CELL) as usize - 1];
                        let along = b.continuation_axis(centre);
                        let spans = |i: usize| {
                            tag & IN_BEND_CELL != 0
                                || t.is_some_and(|t| {
                                    let v = [pose * t.a, pose * t.b, pose * t.c].map(|p| p[i]);
                                    (v[0].min(v[1]).min(v[2])..=v[0].max(v[1]).max(v[2]))
                                        .contains(&centre[i])
                                })
                        };
                        along
                            .filter(|&i| {
                                spans(i) && (legacy || b.reaches(centre, 2.0 * ball.radius))
                            })
                            .and_then(|_| eval(b))
                    }
                }
            }
        {
            // One contact against the exact tube (see [`ChannelBend`]), on the pair's first bend
            // manifold only and without a bounce at the triangle hand-overs, as on the slope.
            #[cfg(test)]
            let legacy = self.bend_contact_at_step_start;
            #[cfg(not(test))]
            let legacy = false;
            if !legacy {
                if rigid.solver_contacts.is_empty() {
                    return;
                }
                let mut claimed = self
                    .bend_contact_claimed
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if *claimed {
                    rigid.solver_contacts.clear();
                    return;
                }
                *claimed = true;
            }
            set_one_contact(rigid, first, centre, ball.radius, n, gap);
            if !legacy && self.rolling(gap, || now) {
                *rigid.restitution = 0.0;
            }
            return;
        }
        if let Some((arcs, arc_of)) = self.arcs.get(&handle.into_raw_parts())
            && let Some(&tag @ 1..) = arc_of.get(tri as usize)
            && let Some((n, gap)) = {
                // One contact against the exact rail tube (see [`ChannelArc`]): the rail whose
                // footprint holds the ball owns it, else the tagged rail within a ball diameter,
                // evaluated at the end of the step, on the first manifold, without the bounce.
                let v = bodies[ball_body].linvel();
                arcs.iter()
                    .find(|a| a.reaches(centre, 0.0))
                    .or_else(|| {
                        let a = &arcs[tag as usize - 1];
                        a.reaches(centre, 2.0 * ball.radius).then_some(a)
                    })
                    .and_then(|a| a.contact(centre + v * crate::DT, ball.radius))
                    .map(|(n, g)| (n, g - v.dot(n) * crate::DT))
            }
        {
            if rigid.solver_contacts.is_empty() {
                return;
            }
            let mut claimed = self
                .arc_contact_claimed
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if *claimed {
                rigid.solver_contacts.clear();
                return;
            }
            *claimed = true;
            set_one_contact(rigid, first, centre, ball.radius, n, gap);
            if gap <= 0.0 {
                *rigid.restitution = 0.0;
            }
            return;
        }
        let bits = self.stitched[&handle.into_raw_parts()]
            .get(tri as usize)
            .copied()
            .unwrap_or(0);
        if bits == 0 {
            return;
        }
        let collider = &colliders[handle];
        let Some(mesh) = collider.shape().as_trimesh() else {
            return;
        };
        let t = mesh.triangle(tri);
        let corners = [t.a, t.b, t.c];
        let on_stitched = rigid.manifold.points.iter().any(|p| {
            let lp = if first { p.local_p1 } else { p.local_p2 };
            (0..3).any(|i| {
                (bits & (1 << i) != 0
                    && point_segment_distance(lp, corners[i], corners[(i + 1) % 3]) < 1.0e-6)
                    || (bits & (1 << (3 + i)) != 0 && (lp - corners[i]).length() < 1.0e-6)
            })
        });
        if !on_stitched {
            return;
        }
        let Some(face) = t.normal() else {
            return;
        };
        let pose = collider.position();
        let face = pose.rotation * face;
        let on_plane = pose * t.a;
        // Contact against the triangle's plane, as one merged mesh would report it: the ball's
        // nearest point along the face normal and its gap to the plane.
        let gap = (centre - on_plane).dot(face) - ball.radius;
        let ball_point = centre - face * ball.radius;
        let mesh_point = ball_point - face * gap;
        *rigid.normal = if first { face } else { -face };
        for c in rigid.solver_contacts.iter_mut() {
            c.dist = gap;
            if first {
                c.anchor1 = mesh_point;
                c.anchor2 = ball_point;
            } else {
                c.anchor1 = ball_point;
                c.anchor2 = mesh_point;
            }
        }
    }
}

/// Whether a straight next to a fork takes the ball once its centre leaves the fork cell.
const FORK_HAND_OVER: bool = true;

/// Replace a manifold's solver contacts with one contact of normal `n` (from the mesh toward the
/// ball) and gap `gap` for the ball of centre `centre` and radius `radius`.
fn set_one_contact(
    rigid: &mut ModifiableManifold<'_>,
    first: bool,
    centre: Vector,
    radius: f32,
    n: Vector,
    gap: f32,
) {
    let ball_point = centre - n * radius;
    let mesh_point = ball_point - n * gap;
    *rigid.normal = if first { n } else { -n };
    rigid.solver_contacts.truncate(1);
    for c in rigid.solver_contacts.iter_mut() {
        c.dist = gap;
        if first {
            c.anchor1 = mesh_point;
            c.anchor2 = ball_point;
        } else {
            c.anchor1 = ball_point;
            c.anchor2 = mesh_point;
        }
    }
}

fn point_segment_distance(p: Vector, a: Vector, b: Vector) -> f32 {
    let ab = b - a;
    let len2 = ab.dot(ab);
    let s = if len2 > 0.0 {
        ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p - (a + ab * s)).length()
}

#[cfg(test)]
mod tests {
    use super::*;
    use parble_core::Yaw4;

    fn id(s: &str) -> PieceId {
        PieceId::new(s).unwrap()
    }

    /// [`CREASE_SINK_COMPENSATION_M`] is the static deflection of one soft contact with a fixed
    /// body less that of the two crease facets, from the softness the sim runs with.
    #[test]
    fn the_crease_sink_compensation_follows_the_static_contact_softness() {
        let sim = crate::Sim::empty();
        let soft = sim.world.integration_parameters.static_contact_softness;
        assert_eq!(
            soft,
            IntegrationParameters::default().static_contact_softness
        );
        let w = f64::from(soft.angular_frequency());
        let cos = f64::from(FACET_TUBE_RADIUS_M) / f64::from(CHANNEL_RADIUS_UM) * 1.0e6;
        let one = f64::from(crate::marble::G) / (w * w);
        let crease = one / (2.0 * cos * cos);
        let want = one - crease;
        assert!(
            (f64::from(CREASE_SINK_COMPENSATION_M) - want).abs() < 1.0e-9,
            "{CREASE_SINK_COMPENSATION_M} against {want}"
        );
    }

    /// Every channel triangle of the forks (below the cell top), placed turned and mirrored, lies
    /// on the union the junction contact models: the two arcs and the frog. A triangle off it
    /// would keep its mesh contact beside the exact one.
    #[test]
    fn fork_channel_triangles_all_lie_on_the_junction() {
        let kit = parble_kit::builtin();
        for piece in ["mech_splitter_l3", "mech_toggle_l3"] {
            for (yaw, mirror) in [(0, false), (1, true), (2, false), (3, true)] {
                let p = place(piece, 2, 1, -1, yaw, mirror);
                let kp = kit.piece(&p.piece).unwrap();
                let j = ChannelJunction::of(kp, &p).unwrap();
                assert!(j.frog.is_some(), "{piece}: no frog");
                let top = i64::from(CELL_UM) * 2 - 100;
                let off: Vec<_> = placed_triangles(kp, &p)
                    .into_iter()
                    .filter(|t| {
                        t.iter().all(|v| (top - 58_000..top).contains(&v[1]))
                            && !t.iter().all(|&v| j.holds(v))
                    })
                    .collect();
                assert!(
                    off.is_empty(),
                    "{piece} yaw {yaw}: {} off: {:?}",
                    off.len(),
                    &off[..off.len().min(4)]
                );
            }
        }
    }

    /// The landing floors of the tower socket block and the redirects, placed turned and mirrored:
    /// every floor vertex of the tunnel under the shaft lies on the exact landing, and the contact
    /// matches a brute-force nearest point on the exact surface.
    #[test]
    fn landing_floors_follow_the_sidecar_and_the_contact_finds_the_nearest_point() {
        let kit = parble_kit::builtin();
        for (piece, yaw, mirror) in [
            ("blk_tower_socket_l1", 0, false),
            ("blk_tower_socket_l1", 1, true),
            ("blk_catch_l1", 0, false),
            ("blk_catch_l1", 3, true),
            ("blk_redirect_l3", 0, false),
            ("blk_redirect_l2", 3, true),
        ] {
            let p = place(piece, 2, 1, -1, yaw, mirror);
            let kp = kit.piece(&p.piece).unwrap();
            let l = ChannelSlope::landing(kp, &p).unwrap();
            assert_eq!(ChannelSlope::of(kp, &p), None);
            let len = l.len_um;
            let held = placed_triangles(kp, &p)
                .into_iter()
                .flatten()
                .filter(|&v| {
                    let (s, _) = l.plan(v[0] as f32 * 1.0e-6, v[2] as f32 * 1.0e-6);
                    (0.0..=len as f32 * 1.0e-6).contains(&s) && l.holds(v)
                })
                .count();
            // 17 floor-ring vertices per station (4 path segments on the tower, 8 on the redirect).
            assert!(
                held >= 17 * 5,
                "{piece} yaw {yaw}: {held} floor vertices held"
            );
            let m = |v: i64| v as f32 * 1.0e-6;
            let r = 0.008;
            let big_r = channel_tube_radius(r);
            let surface = |s: f32, a: f32| {
                let t = (s / m(len)).clamp(0.0, 1.0);
                let ay = m(l.axis_y_um) - l.profile(m(l.drop_um), m(len), t).0;
                (s, ay - big_r * a.cos(), big_r * a.sin())
            };
            for i in 0..=20 {
                for (lat, up) in [
                    (0.0, 0.0),
                    (0.004, 0.0005),
                    (-0.005, 0.002),
                    (0.002, -0.001),
                ] {
                    let s = m(len) * i as f32 / 20.0;
                    let t = (s / m(len)).clamp(0.0, 1.0);
                    let ay = m(l.axis_y_um) - l.profile(m(l.drop_um), m(len), t).0;
                    let y = ay - 0.006 + up;
                    let (dx, dz) = (l.dir[0] as f32, l.dir[1] as f32);
                    let c = Vector::new(
                        m(l.entry_um[0]) + s * dx - lat * dz,
                        y,
                        m(l.entry_um[1]) + s * dz + lat * dx,
                    );
                    let (_, gap) = l.contact(c, r).unwrap();
                    let mut best = f32::MAX;
                    for k in -150..=150 {
                        for j in -100..=100 {
                            let (ss, yy, ww) = surface(
                                s + k as f32 * 4.0e-5,
                                j as f32 * core::f32::consts::PI / 200.0,
                            );
                            let d =
                                ((ss - s).powi(2) + (yy - y).powi(2) + (ww - lat).powi(2)).sqrt();
                            best = best.min(d);
                        }
                    }
                    assert!(
                        (gap - (best - r)).abs() < 3.0e-5,
                        "{piece} s {s:.4} lat {lat} up {up}: gap {gap} brute {}",
                        best - r
                    );
                }
            }
        }
    }

    fn place(piece: &str, x: i32, y: i32, z: i32, yaw: u8, mirror: bool) -> Placement {
        Placement::new(
            id(piece),
            CellCoord::new(x, y, z),
            Yaw4::new(yaw),
            mirror.into(),
        )
    }

    fn build(placements: &[Placement]) -> (ChunkColliders, PhysicsWorld) {
        let kit = parble_kit::builtin();
        let mut cc = ChunkColliders::new();
        for p in placements {
            cc.insert(kit, p.clone()).unwrap();
        }
        let mut world = PhysicsWorld::new();
        cc.rebuild(kit, &mut world).unwrap();
        (cc, world)
    }

    #[test]
    fn chunks_floor_toward_negative() {
        assert_eq!(chunk_of(CellCoord::new(-1, 0, 8)), CellCoord::new(-1, 0, 1));
        assert_eq!(
            chunk_of(CellCoord::new(-8, 7, -9)),
            CellCoord::new(-1, 0, -2)
        );
        assert_eq!(chunk_of(CellCoord::new(-3, 0, 5)), CellCoord::new(-1, 0, 0));
    }

    #[test]
    fn placement_transform_follows_the_contract() {
        let a = CellCoord::new(1, 2, -1);
        let p = [10, 20, 3];
        let o = |k: u8, m: bool| Orientation::new(Yaw4::new(k), m.into());
        // Yaw 1 maps +X to −Z (§8), then the anchor offset of 60 mm per cell.
        assert_eq!(
            place_point_um(o(1, false), a, [1, 0, 0]),
            [60_000, 120_000, -60_001]
        );
        // Mirror is applied first: mirror + yaw 1 sends +X to +Z.
        assert_eq!(
            place_point_um(o(1, true), CellCoord::ORIGIN, [1, 0, 0]),
            [0, 0, 1]
        );
        assert_eq!(
            place_point_um(o(0, false), CellCoord::ORIGIN, p),
            [10, 20, 3]
        );
        // Points transform like cells.
        for k in 0..4 {
            for m in [false, true] {
                let c = o(k, m).apply_cell(CellCoord::new(1, 0, 2));
                let pt = place_point_um(o(k, m), CellCoord::ORIGIN, [60_000, 0, 120_000]);
                assert_eq!(pt, [60_000 * c.x as i64, 0, 60_000 * c.z as i64]);
            }
        }
    }

    #[test]
    fn mating_rings_weld_into_one_shared_seam() {
        let kit = parble_kit::builtin();
        let straight = kit.piece(&id("blk_straight_l3")).unwrap();
        let a = place("blk_straight_l3", 0, 0, 0, 0, false);
        let b = place("blk_straight_l3", 1, 0, 0, 0, false);
        let one = ChunkMesh::from_triangles(&placed_triangles(straight, &a));
        let mut loose = placed_triangles(straight, &a);
        loose.extend(placed_triangles(straight, &b));
        let welded = ChunkMesh::from_triangles(&loose);
        assert_eq!(welded.triangles.len(), 2 * one.triangles.len());
        // The x = 30 mm port outline is shared: 23 vertices (ring + shell outline) weld away.
        let shared = 2 * one.vertices_um.len() - welded.vertices_um.len();
        assert_eq!(
            shared,
            one.vertices_um.iter().filter(|v| v[0] == 30_000).count()
        );
        assert_eq!(shared, 23);

        // After simplification the pair is the same surface as one seamless 2-cell straight.
        let (cc, _) = build(&[a, b]);
        let two = cc.chunk_mesh(kit, CellCoord::ORIGIN);
        let stretched: Vec<[[i64; 3]; 3]> =
            placed_triangles(straight, &place("blk_straight_l3", 0, 0, 0, 0, false))
                .into_iter()
                .map(|t| t.map(|v| [(v[0] + 30_000) * 2 - 30_000, v[1], v[2]]))
                .collect();
        let mut seamless = ChunkMesh::from_triangles(&stretched);
        seamless.simplify_flat();
        assert_eq!(two.vertices_um.len(), one.vertices_um.len());
        assert_eq!(two.triangles.len(), seamless.triangles.len());
        assert!(
            two.vertices_um.iter().all(|v| v[0] != 30_000),
            "seam ring removed"
        );
    }

    #[test]
    fn insertion_order_does_not_change_the_mesh() {
        let kit = parble_kit::builtin();
        let layout = [
            place("blk_straight_l3", 0, 0, 0, 0, false),
            place("blk_straight_l3", 1, 0, 0, 0, false),
            place("blk_curve90_l3", 2, 0, 0, 0, false),
            place("blk_straight_l3", 2, 0, -1, 1, false),
            place("blk_slope_gentle_l3", 2, 0, -2, 1, false),
            place("blk_end_l3", -1, 0, 0, 2, false),
        ];
        let mut fwd = ChunkColliders::new();
        let mut rev = ChunkColliders::new();
        for p in &layout {
            fwd.insert(kit, p.clone()).unwrap();
        }
        for p in layout.iter().rev() {
            rev.insert(kit, p.clone()).unwrap();
        }
        for c in [
            CellCoord::new(-1, 0, -1),
            CellCoord::new(-1, 0, 0),
            CellCoord::new(0, 0, -1),
            CellCoord::ORIGIN,
        ] {
            assert_eq!(
                fwd.chunk_mesh(kit, c),
                rev.chunk_mesh(kit, c),
                "chunk {c:?}"
            );
        }
    }

    #[test]
    fn mirrored_pieces_keep_their_triangles_facing_out() {
        // A mirrored curve is the reflection of the unmirrored one, with winding restored: the
        // signed volume (sum over triangles of a · (b × c)) keeps its sign.
        let kit = parble_kit::builtin();
        let curve = kit.piece(&id("blk_curve90_l3")).unwrap();
        let volume = |p: &Placement| -> i128 {
            placed_triangles(curve, p)
                .iter()
                .map(|[a, b, c]| {
                    let [a, b, c] = [a, b, c].map(|v| v.map(i128::from));
                    a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                        + a[2] * (b[0] * c[1] - b[1] * c[0])
                })
                .sum()
        };
        let plain = volume(&place("blk_curve90_l3", 0, 0, 0, 0, false));
        let mirrored = volume(&place("blk_curve90_l3", 0, 0, 0, 0, true));
        assert_ne!(plain, 0);
        assert_eq!(plain, mirrored);
    }

    #[test]
    fn neighbours_side_by_side_and_stacked_build() {
        // Parallel straights and a stacked pair share closed shell faces; those faces are culled
        // so the merged trimesh stays manifold and Rapier accepts it.
        let kit = parble_kit::builtin();
        let lone = ChunkMesh::from_triangles(&placed_triangles(
            kit.piece(&id("blk_straight_l3")).unwrap(),
            &place("blk_straight_l3", 0, 0, 0, 0, false),
        ));
        let (cc, world) = build(&[
            place("blk_straight_l3", 0, 0, 0, 0, false),
            place("blk_straight_l3", 0, 0, 1, 0, false),
            place("blk_straight_l3", 0, 1, 0, 0, false),
            place("blk_curve90_l3", 1, 0, 1, 2, true),
        ]);
        assert_eq!(world.colliders.len(), 1);
        let mesh = cc.chunk_mesh(kit, CellCoord::ORIGIN);
        // Triangles on the z = 30 mm face of cell (0, 0, 0).
        let walls = |m: &ChunkMesh, z: i64| {
            m.triangles
                .iter()
                .filter(|t| {
                    t.iter().all(|&i| {
                        let v = m.vertices_um[i as usize];
                        v[2] == z && v[0] <= 30_000 && v[1] <= 60_000
                    })
                })
                .count()
        };
        assert!(walls(&lone, 30_000) > 0);
        assert_eq!(walls(&mesh, 30_000), 0, "shared z = 30 mm face is culled");
    }

    #[test]
    fn incremental_rebuild_matches_a_fresh_build() {
        let kit = parble_kit::builtin();
        let layout = [
            place("blk_straight_l3", 6, 0, 0, 0, false),
            place("blk_straight_l3", 7, 0, 0, 0, false),
            place("blk_straight_l3", 8, 0, 0, 0, false),
            place("blk_curve90_l3", 9, 0, 0, 0, false),
            place("blk_end_l3", -1, 0, 0, 2, false),
        ];
        let (mut cc, mut world) = build(&layout[..2]);
        for p in &layout[2..] {
            cc.insert(kit, p.clone()).unwrap();
        }
        cc.insert(kit, place("blk_slope_gentle_l3", 7, 0, 1, 0, false))
            .unwrap();
        cc.rebuild(kit, &mut world).unwrap();
        cc.remove(CellCoord::new(7, 0, 1)).unwrap();
        // Removing the slope dirties its own chunk and the chunk of the diagonal straight at
        // (8, 0, 0).
        assert_eq!(
            cc.dirty_chunks().collect::<Vec<_>>(),
            [CellCoord::ORIGIN, CellCoord::new(1, 0, 0)]
        );
        cc.rebuild(kit, &mut world).unwrap();

        let (fresh, fresh_world) = build(&layout);
        let chunks: Vec<CellCoord> = cc.chunks().map(|(c, _)| c).collect();
        let fresh_chunks: Vec<CellCoord> = fresh.chunks().map(|(c, _)| c).collect();
        assert_eq!(
            chunks,
            [
                CellCoord::new(-1, 0, 0),
                CellCoord::ORIGIN,
                CellCoord::new(1, 0, 0)
            ]
        );
        assert_eq!(chunks, fresh_chunks);
        assert_eq!(world.colliders.len(), fresh_world.colliders.len());
        for c in chunks {
            assert_eq!(
                cc.chunk_mesh(kit, c),
                fresh.chunk_mesh(kit, c),
                "chunk {c:?}"
            );
            let tri = |w: &PhysicsWorld, h: ColliderHandle| {
                let m = w.colliders[h].shape().as_trimesh().unwrap();
                (m.vertices().to_vec(), m.indices().to_vec())
            };
            let h = cc.handles[&c];
            let fh = fresh.handles[&c];
            assert_eq!(tri(&world, h), tri(&fresh_world, fh));
        }
    }

    /// The staged rebuild, with its simplify split at every vertex or every few, and with a chunk
    /// that reuses its cached simplify after a diagonal neighbour's edit, installs what a fresh
    /// build does.
    #[test]
    fn split_staged_rebuild_matches_a_fresh_build() {
        let kit = parble_kit::builtin();
        let ids = [
            "blk_straight_l3",
            "blk_curve90_l3",
            "blk_slope_gentle_l3",
            "blk_end_l3",
        ];
        // A dense 12 x 12 cell slab across four chunks.
        let layout: Vec<Placement> = (0..144)
            .map(|i: i32| {
                let (x, z) = (i % 12 + 2, i / 12 + 2);
                place(ids[(i * 7 % 4) as usize], x, 0, z, (i * 5 % 4) as u8, false)
            })
            .collect();
        let cut = CellCoord::new(7, 0, 7);
        let rest: Vec<Placement> = layout.iter().filter(|p| p.anchor != cut).cloned().collect();
        let (fresh, fresh_world) = build(&rest);
        let tags = |cc: &ChunkColliders| {
            cc.chunks()
                .map(|(c, h)| {
                    let (st, b, s, a) = cc.hook_tags(h).unwrap();
                    (c, st.to_vec(), b.clone(), s.clone(), a.clone())
                })
                .collect::<Vec<_>>()
        };
        let tri = |w: &PhysicsWorld, cc: &ChunkColliders| {
            cc.chunks()
                .map(|(_, h)| {
                    let c = &w.colliders[h];
                    let m = c.shape().as_trimesh().unwrap();
                    (
                        m.vertices().to_vec(),
                        m.indices().to_vec(),
                        c.active_hooks(),
                    )
                })
                .collect::<Vec<_>>()
        };
        for work in [1, 37] {
            let mut cc = ChunkColliders::new();
            cc.simplify_step_work = Some(work);
            let mut world = PhysicsWorld::new();
            for p in &layout {
                cc.insert(kit, p.clone()).unwrap();
            }
            let mut steps = 0;
            while cc.rebuild_step(kit, &mut world).unwrap().is_some() {
                steps += 1;
            }
            assert!(steps > 100, "work {work}: {steps} steps");
            // The piece at a chunk corner: chunk (1, 0, 1) is dirtied only for its stitched tags
            // and reuses its simplified geometry.
            cc.remove(cut).unwrap();
            assert_eq!(cc.dirty_chunks().count(), 4);
            while cc.rebuild_step(kit, &mut world).unwrap().is_some() {}
            assert!(cc.simplify_reused >= 1, "work {work}: no reuse");
            for (c, _) in fresh.chunks() {
                assert_eq!(
                    cc.chunk_mesh(kit, c),
                    fresh.chunk_mesh(kit, c),
                    "chunk {c:?}"
                );
            }
            assert_eq!(tri(&world, &cc), tri(&fresh_world, &fresh), "work {work}");
            assert_eq!(tags(&cc), tags(&fresh), "work {work}");
        }
    }

    #[test]
    fn edits_are_checked() {
        let kit = parble_kit::builtin();
        let mut cc = ChunkColliders::new();
        cc.insert(kit, place("blk_straight_l3", 0, 0, 0, 0, false))
            .unwrap();
        assert_eq!(
            cc.insert(kit, place("blk_end_l3", 0, 0, 0, 1, false)),
            Err(ChunkError::Overlap(CellCoord::ORIGIN))
        );
        assert_eq!(
            cc.insert(kit, place("blk_straight", 3, 0, 0, 0, false)),
            Err(ChunkError::UnknownPiece(id("blk_straight")))
        );
        assert_eq!(
            cc.remove(CellCoord::new(5, 0, 0)),
            Err(ChunkError::NotPlaced(CellCoord::new(5, 0, 0)))
        );
    }
}
