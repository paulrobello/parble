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
//! [`ChunkColliders`] is the incremental API: insert or remove placements, then
//! [`ChunkColliders::rebuild`] replaces the collider of each dirty chunk, in chunk order.

use std::collections::{BTreeMap, BTreeSet};

use parble_core::contract::{CELL_UM, CHUNK_CELLS, CHUNK_UM, HALF_CELL_UM};
use parble_core::{CellCoord, Mirror, Orientation, PieceId, Placement, PortFace};
use parble_kit::{KitCatalog, KitPiece};
use rapier3d::prelude::*;

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
        let nv = self.vertices_um.len();
        let mut tris: Vec<Option<[u32; 3]>> = self.triangles.iter().copied().map(Some).collect();
        let mut incident: Vec<Vec<usize>> = vec![Vec::new(); nv];
        for (i, t) in self.triangles.iter().enumerate() {
            for &v in t {
                incident[v as usize].push(i);
            }
        }
        let pos = |v: u32| self.vertices_um[v as usize].map(i128::from);
        let normal = |t: [u32; 3]| {
            let [a, b, c] = t.map(pos);
            cross(sub(b, a), sub(c, a))
        };

        let mut changed = true;
        while changed {
            changed = false;
            for v in 0..nv as u32 {
                let live: Vec<usize> = incident[v as usize]
                    .iter()
                    .copied()
                    .filter(|&i| tris[i].is_some())
                    .collect();
                if live.is_empty() {
                    continue;
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
                    continue;
                }
                let same_plane = |i: usize, j: usize| {
                    let (a, b) = (normal(tris[i].unwrap()), normal(tris[j].unwrap()));
                    cross(a, b) == [0; 3] && dot(a, b) > 0
                };
                let mut planes: Vec<usize> = Vec::new();
                for &i in &live {
                    if !planes.iter().any(|&p| same_plane(p, i)) {
                        planes.push(i);
                    }
                }
                let candidates: Vec<u32> = match planes.len() {
                    1 => edges.keys().copied().collect(),
                    2 => {
                        let crease: Vec<u32> = edges
                            .iter()
                            .filter(|(_, ts)| !same_plane(ts[0], ts[1]))
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
                            _ => continue,
                        }
                    }
                    _ => continue,
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
                    changed = true;
                    break;
                }
            }
        }

        // Compact: keep triangle order, renumber vertices by first use.
        let mut remap = vec![u32::MAX; nv];
        let mut vertices = Vec::new();
        let mut triangles = Vec::new();
        for t in tris.into_iter().flatten() {
            triangles.push(t.map(|v| {
                if remap[v as usize] == u32::MAX {
                    remap[v as usize] = vertices.len() as u32;
                    vertices.push(self.vertices_um[v as usize]);
                }
                remap[v as usize]
            }));
        }
        self.vertices_um = vertices;
        self.triangles = triangles;
        self.stitched.clear();
    }

    /// The static wood collider: vertices relative to `origin_um`, collider placed at the origin.
    pub fn collider(&self, origin_um: [i64; 3]) -> Result<Collider, TriMeshBuilderError> {
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
        Ok(ColliderBuilder::trimesh_with_flags(
            vertices,
            self.triangles.clone(),
            TriMeshFlags::FIX_INTERNAL_EDGES,
        )?
        .translation(Vector::new(
            m(origin_um[0]),
            m(origin_um[1]),
            m(origin_um[2]),
        ))
        .friction(scene::WOOD_FRICTION)
        .restitution(scene::WOOD_RESTITUTION)
        .build())
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
    dirty: BTreeSet<CellCoord>,
}

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

    /// Dirty the chunk of `anchor` and of every placement face-adjacent to it (their shared-face
    /// culling may change).
    fn mark_dirty(&mut self, anchor: CellCoord) {
        let Some(placed) = self.placed.get(&anchor) else {
            return;
        };
        self.dirty.insert(chunk_of(anchor));
        for &c in &placed.cells {
            for face in PortFace::ALL {
                if let Some(&other) = self.occupancy.get(&(c + face.normal())) {
                    self.dirty.insert(chunk_of(other));
                }
            }
        }
    }

    /// Chunks waiting for [`Self::rebuild`], in order.
    pub fn dirty_chunks(&self) -> impl Iterator<Item = CellCoord> + '_ {
        self.dirty.iter().copied()
    }

    /// Chunks that currently hold a collider, in order.
    pub fn chunks(&self) -> impl Iterator<Item = (CellCoord, ColliderHandle)> + '_ {
        self.handles.iter().map(|(&c, &h)| (c, h))
    }

    /// The merged mesh of one chunk from the current placements.
    pub fn chunk_mesh(&self, kit: &KitCatalog, chunk: CellCoord) -> ChunkMesh {
        let mut mesh = ChunkMesh::default();
        let mut weld = BTreeMap::new();
        let Some(members) = self.by_chunk.get(&chunk) else {
            return mesh;
        };
        for (anchor, id) in members {
            let placed = &self.placed[anchor];
            let Some(piece) = kit.piece(id) else { continue };
            let tris: Vec<[[i64; 3]; 3]> = placed_triangles(piece, &placed.placement)
                .into_iter()
                .filter(|t| !self.culled(placed, t))
                .collect();
            mesh.push_welded(&mut weld, &tris);
        }
        mesh.simplify_flat();
        self.mark_stitched(&mut mesh, chunk);
        mesh
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
        while let Some(chunk) = self.dirty.first().copied() {
            if let Some(h) = self.handles.remove(&chunk) {
                world.remove_collider(h);
                self.stitched.remove(&h.into_raw_parts());
            }
            let mesh = self.chunk_mesh(kit, chunk);
            if !mesh.triangles.is_empty() {
                let mut collider = mesh
                    .collider(chunk_origin_um(chunk))
                    .map_err(|e| ChunkError::TriMesh(chunk, format!("{e:?}")))?;
                if mesh.stitched.iter().any(|&b| b != 0) {
                    collider.set_active_hooks(ActiveHooks::MODIFY_SOLVER_CONTACTS);
                }
                let h = world.colliders.insert(collider);
                self.handles.insert(chunk, h);
                self.stitched.insert(h.into_raw_parts(), mesh.stitched);
            }
            self.dirty.remove(&chunk);
        }
        Ok(())
    }
}

/// Contact fix at chunk boundaries. `FIX_INTERNAL_EDGES` only sees one trimesh, so where a run
/// crosses from one chunk collider into the next, the open edges and vertices on the chunk face
/// produce contacts whose normal leans toward the edge (a measured 60+ mm/s vertical kick). For a
/// contact that lies on a stitched edge, the manifold normal is replaced by that triangle's face
/// normal, which is what one merged mesh would have produced.
impl PhysicsHooks for ChunkColliders {
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
        // Removing the slope dirties its own chunk and the chunk of the touching straight.
        assert_eq!(cc.dirty_chunks().collect::<Vec<_>>(), [CellCoord::ORIGIN]);
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
