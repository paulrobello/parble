//! Exact channel contact for curved rails (P2-T4, kit contract §9): `rail_curve90_r90_*` (one
//! quarter circle of radius 90 mm) and `rail_s_1x3_*` (two opposed arcs of radius 150 mm).
//!
//! The sidecar samples a rail arc with flat facets (16 and 24 path segments). A marble entering
//! one touches several facets whose normals lean off the surface normal and chatters exactly as
//! on the block bend before [`crate::chunk::ChannelBend`] (measured 2026-11-07 at 0.3 m/s at the
//! rail to S-rail seam: upward 57.19 mm/s and kick 54.17 mm/s against limits of 15.15 and 24.24).
//! The contact hook therefore replaces contacts on an arc's tube triangles, and on the straight
//! continuation past either port, with one contact against the exact tube, under the bend
//! contact's rules: evaluated at the ball's end-of-step position, on the pair's first manifold
//! only, with no restitution while penetrating.
//!
//! The plan centreline is a chain of tangent arcs at one axis height, continued straight past
//! both ports. Every centre and endpoint is an integer µm point placed with [`place_point_um`],
//! so mirror and yaw need no trig, and an arc is the sector between its endpoints (each arc turns
//! under 180°), tested with cross products.

use parble_core::contract::{CELL_UM, CHANNEL_RADIUS_UM, HALF_CELL_UM};
use parble_core::{CellCoord, Placement, PortClass};
use parble_kit::KitPiece;
use rapier3d::prelude::*;

use crate::chunk::{FACET_TUBE_RADIUS_M, place_point_um};

/// One arc of a rail centreline, world µm in plan `(x, z)`: centre, radius and the two endpoints
/// in path order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arc {
    pub centre_um: [i64; 2],
    pub radius_um: i64,
    pub from_um: [i64; 2],
    pub to_um: [i64; 2],
}

/// A curved rail's centreline (see the module docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelArc {
    /// Arcs in path order, each starting where the previous ends.
    pub arcs: Vec<Arc>,
    /// Outward plan unit normals `(x, z)` of the entry and exit ports, along which the tube
    /// continues straight.
    pub entry_out: [i64; 2],
    pub exit_out: [i64; 2],
    /// World µm height of the tube axis (channel floor + channel radius).
    pub axis_y_um: i64,
    /// World µm `(min x, min z)` of each footprint cell.
    pub cells_min_um: Vec<[i64; 2]>,
}

/// Piece-local arcs `(centre, radius, from, to)` in µm `(x, z)` per curved rail shape
/// (blender/kit/kitrails.py).
fn local_arcs(id: &str) -> Option<&'static [([i32; 2], i32, [i32; 2], [i32; 2])]> {
    const CURVE: [([i32; 2], i32, [i32; 2], [i32; 2]); 1] =
        [([-30_000, -90_000], 90_000, [-30_000, 0], [60_000, -90_000])];
    const S: [([i32; 2], i32, [i32; 2], [i32; 2]); 2] = [
        (
            [-30_000, -150_000],
            150_000,
            [-30_000, 0],
            [60_000, -30_000],
        ),
        (
            [150_000, 90_000],
            150_000,
            [60_000, -30_000],
            [150_000, -60_000],
        ),
    ];
    if id.starts_with("rail_curve90_r90_") {
        Some(&CURVE)
    } else if id.starts_with("rail_s_1x3_") {
        Some(&S)
    } else {
        None
    }
}

impl ChannelArc {
    /// The centreline of a curved rail placement, `None` for any other piece.
    pub fn of(piece: &KitPiece, p: &Placement) -> Option<Self> {
        let def = &piece.def;
        let local = local_arcs(def.id.as_str())?;
        let [a, b] = def.ports.as_slice() else {
            return None;
        };
        let level = a.level?;
        if piece.family != "rail"
            || a.class != PortClass::Channel
            || b.class != PortClass::Channel
            || b.level != Some(level)
        {
            return None;
        }
        let o = p.orientation();
        let y = level.floor_um() + CHANNEL_RADIUS_UM;
        let at = |q: [i32; 2]| {
            let w = place_point_um(o, p.anchor, [q[0], y, q[1]]);
            [w[0], w[2]]
        };
        let out = |port: &parble_core::Port| {
            let n = port.transformed(o).face.normal();
            [i64::from(n.x), i64::from(n.z)]
        };
        let cell = i64::from(CELL_UM);
        let half = i64::from(HALF_CELL_UM);
        Some(Self {
            arcs: local
                .iter()
                .map(|&(c, r, f, t)| Arc {
                    centre_um: at(c),
                    radius_um: i64::from(r),
                    from_um: at(f),
                    to_um: at(t),
                })
                .collect(),
            entry_out: out(a),
            exit_out: out(b),
            axis_y_um: place_point_um(o, p.anchor, [0, y, 0])[1],
            cells_min_um: p
                .world_cells(def)
                .iter()
                .map(|c: &CellCoord| [cell * i64::from(c.x) - half, cell * i64::from(c.z) - half])
                .collect(),
        })
    }

    /// The nearest plan point of the centreline (with its straight continuations) to `q`, in the
    /// units of `q` (µm as f64, or metres via `scale` = 1e-6).
    fn nearest(&self, q: [f64; 2], scale: f64) -> [f64; 2] {
        let s = |v: [i64; 2]| [v[0] as f64 * scale, v[1] as f64 * scale];
        let mut best = [f64::NAN; 2];
        let mut best_d = f64::INFINITY;
        let mut take = |p: [f64; 2]| {
            let d = (p[0] - q[0]) * (p[0] - q[0]) + (p[1] - q[1]) * (p[1] - q[1]);
            if d < best_d {
                best_d = d;
                best = p;
            }
        };
        let first = self.arcs[0].from_um;
        let last = self.arcs[self.arcs.len() - 1].to_um;
        for (end, n) in [(first, self.entry_out), (last, self.exit_out)] {
            let e = s(end);
            let along = (q[0] - e[0]) * n[0] as f64 + (q[1] - e[1]) * n[1] as f64;
            take(if along > 0.0 {
                [e[0] + along * n[0] as f64, e[1] + along * n[1] as f64]
            } else {
                e
            });
        }
        for a in &self.arcs {
            let c = s(a.centre_um);
            let (f, t) = (s(a.from_um), s(a.to_um));
            take(t);
            let v = [q[0] - c[0], q[1] - c[1]];
            let d = libm::sqrt(v[0] * v[0] + v[1] * v[1]);
            if d <= 0.0 {
                continue;
            }
            let (fa, ta) = ([f[0] - c[0], f[1] - c[1]], [t[0] - c[0], t[1] - c[1]]);
            let cross = |x: [f64; 2], y: [f64; 2]| x[0] * y[1] - x[1] * y[0];
            let turn = cross(fa, ta);
            if cross(fa, v) * turn >= 0.0 && cross(v, ta) * turn >= 0.0 {
                let r = a.radius_um as f64 * scale;
                take([c[0] + v[0] * r / d, c[1] + v[1] * r / d]);
            }
        }
        best
    }

    /// Whether a world µm vertex lies on the tube (within 2 µm, the sidecar's rounding).
    pub fn holds(&self, v: [i64; 3]) -> bool {
        let q = [v[0] as f64, v[2] as f64];
        let p = self.nearest(q, 1.0);
        let (dx, dz, dy) = (q[0] - p[0], q[1] - p[1], (v[1] - self.axis_y_um) as f64);
        (libm::sqrt(dx * dx + dz * dz + dy * dy) - f64::from(CHANNEL_RADIUS_UM)).abs() <= 2.0
    }

    /// Whether a ball centre lies over one of the footprint cells grown by `margin` metres.
    pub fn reaches(&self, c: Vector, margin: f32) -> bool {
        let m = |v: i64| v as f32 * 1.0e-6;
        let cell = m(i64::from(CELL_UM));
        self.cells_min_um.iter().any(|&[x0, z0]| {
            (m(x0) - margin..=m(x0) + cell + margin).contains(&c.x)
                && (m(z0) - margin..=m(z0) + cell + margin).contains(&c.z)
        })
    }

    /// The contact of a ball (centre `c`, radius `r`, metres) with the exact tube: the normal from
    /// the tube wall toward the ball and the gap (negative when penetrating), as
    /// [`crate::chunk::ChannelBend`]'s.
    pub fn contact(&self, c: Vector, r: f32) -> Option<(Vector, f32)> {
        let p = self.nearest([f64::from(c.x), f64::from(c.z)], 1.0e-6);
        let axis = Vector::new(p[0] as f32, self.axis_y_um as f32 * 1.0e-6, p[1] as f32);
        let off = (axis - c).length();
        if off <= 0.0 {
            return None;
        }
        Some(((axis - c) / off, FACET_TUBE_RADIUS_M - r - off))
    }
}

/// Per triangle, `k + 1` when its three vertices lie on the tube of `arcs[k]`, else 0.
pub fn tag(arcs: &[ChannelArc], vertices_um: &[[i64; 3]], triangles: &[[u32; 3]]) -> Vec<u32> {
    if arcs.is_empty() {
        return Vec::new();
    }
    triangles
        .iter()
        .map(|t| {
            let vs = t.map(|v| vertices_um[v as usize]);
            arcs.iter()
                .position(|a| vs.iter().all(|&v| a.holds(v)))
                .map_or(0, |k| k as u32 + 1)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use parble_core::{PieceId, Yaw4};

    fn arc_of(id: &str, yaw: u8, mirror: bool) -> ChannelArc {
        let kit = parble_kit::builtin();
        let p = Placement::new(
            PieceId::new(id).unwrap(),
            CellCoord::new(2, 0, 3),
            Yaw4::new(yaw),
            mirror.into(),
        );
        ChannelArc::of(kit.piece(&p.piece).unwrap(), &p).unwrap()
    }

    #[test]
    fn every_inner_ring_vertex_of_a_curved_rail_lies_on_its_tube() {
        // The 17 half-round vertices of every collision station sit on the exact tube, in every
        // orientation; the outer shell (18 mm) and the lip tops do not.
        let kit = parble_kit::builtin();
        for id in ["rail_curve90_r90_l2", "rail_s_1x3_l2"] {
            for (yaw, mirror) in [(0, false), (1, false), (3, true)] {
                let arc = arc_of(id, yaw, mirror);
                let p = Placement::new(
                    PieceId::new(id).unwrap(),
                    CellCoord::new(2, 0, 3),
                    Yaw4::new(yaw),
                    mirror.into(),
                );
                let verts: std::collections::BTreeSet<[i64; 3]> =
                    crate::chunk::placed_triangles(kit.piece(&p.piece).unwrap(), &p)
                        .into_iter()
                        .flatten()
                        .collect();
                let on = verts.iter().filter(|&&v| arc.holds(v)).count();
                // one floor vertex per collision station
                let floor = verts
                    .iter()
                    .filter(|v| v[1] == arc.axis_y_um - i64::from(CHANNEL_RADIUS_UM))
                    .count();
                assert!(
                    floor > 0 && on >= 17 * floor,
                    "{id} yaw {yaw}: {on} on tube, {floor}"
                );
                assert!(
                    !verts
                        .iter()
                        .any(|&v| v[1] > arc.axis_y_um + 3_000 && arc.holds(v))
                );
            }
        }
    }

    #[test]
    fn a_centred_ball_rests_at_the_facet_gap_along_the_whole_path() {
        // On the centreline at rest height the exact contact gives the gap a straight's two floor
        // facets give (FACET_TUBE_RADIUS_M), with a vertical normal, at both ports, the S-rail's
        // inflection and past either port.
        let arc = arc_of("rail_s_1x3_l2", 0, false);
        let r = 0.008;
        let y = arc.axis_y_um as f32 * 1.0e-6 - (FACET_TUBE_RADIUS_M - r);
        let anchor = [2.0 * 0.06, 3.0 * 0.06];
        for (x, z) in [
            (-0.04, 0.0),
            (-0.03, 0.0),
            (0.06, -0.03),
            (0.15, -0.06),
            (0.17, -0.06),
        ] {
            let c = Vector::new(anchor[0] + x, y, anchor[1] + z);
            let (n, gap) = arc.contact(c, r).unwrap();
            assert!(
                gap.abs() < 2.0e-6 && n.y > 0.9999,
                "({x}, {z}): gap {gap}, n {n}"
            );
        }
    }
}
