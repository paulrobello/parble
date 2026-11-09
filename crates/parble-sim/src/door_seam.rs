//! Door seam contact correction: the one-way hole's trapdoor (`mech_hole_oneway_l1`) lies flush
//! with the channel floor around its bore, but it is a separate collider from the chunk trimesh,
//! so at both seams a marble rolling over the shut door meets contacts on the other body's edge.
//! At the hinge a door contact 0.37 mm ahead tilted 0.30 against the travel, at the free edge a
//! floor contact 0.69 mm ahead tilted 0.39; each took about five times the resting impulse and
//! threw the marble up 9–410 mm/s (measured 2026-11-09,
//! `behaviour::tests::one_way_hole_over_side_sweep_keeps_rolling_resistance_speed`). The door
//! trimesh also has no internal edge correction, so its own facet diagonals do the same.
//!
//! While the door rests through the step and a marble's centre is above the hinge and within
//! reach of the bore, every marble contact with the door or with the hole's chunk collider is
//! kept only when the marble's centre projects into that contact's triangle. Those are the face
//! contacts one seamless floor would give: the door's top and the floor around the bore tile the
//! channel's facets exactly, so the triangle under the marble always holds it. A contact whose
//! projection falls outside its triangle is an edge or vertex contact (the seam rims, the bore
//! wall, the door's sides and its internal edges) and is dropped. Concave creases keep both
//! facets' contacts, since the centre projects into each.
//!
//! Each manifold decides alone, from its own triangle and the marble's centre, so the result does
//! not depend on pair order. An open or moving door keeps all its contacts: a marble from the
//! trap side or from below meets the door exactly as before.

use rapier3d::prelude::*;

use crate::chunk::ChunkColliders;
use crate::mechanism::{InstanceId, Mechanisms};

/// Reach past the bore radius plus the marble radius, m, inside which contacts are corrected
/// (past the solver's 1.2 mm prediction distance).
const DOOR_SEAM_MARGIN_M: f32 = 0.003;

/// Distance outside its triangle, m, within which a contact's projected centre still counts as
/// inside (shared edges keep both sides' contacts).
const INSIDE_TOLERANCE_M: f32 = 1.0e-6;

/// A flush door's placed bore: its plan axis and radius, and the hinge height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DoorSeam {
    pub instance: InstanceId,
    pub part: usize,
    /// World `(x, z)` of the bore axis, m.
    pub axis: [f32; 2],
    /// Largest plan distance of a door vertex from the axis, m.
    pub radius: f32,
    /// World height of the door's hinge, m: a marble centred above it is on the over side.
    pub hinge_y: f32,
}

impl DoorSeam {
    /// The seam of a door part from its placed rest-pose collision vertices (world m).
    pub fn of(
        instance: InstanceId,
        part: usize,
        vertices: impl Iterator<Item = Vector> + Clone,
        hinge_y: f32,
    ) -> Option<Self> {
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for v in vertices.clone() {
            lo = [lo[0].min(v.x), lo[1].min(v.z)];
            hi = [hi[0].max(v.x), hi[1].max(v.z)];
        }
        if lo[0] > hi[0] {
            return None;
        }
        let axis = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
        let radius = libm::sqrtf(vertices.map(|v| plan_sq(v, axis)).fold(0.0, f32::max));
        Some(Self {
            instance,
            part,
            axis,
            radius,
            hinge_y,
        })
    }

    /// Whether a ball of centre `c` and radius `r` is on the over side and within reach.
    fn holds(&self, c: Vector, r: f32) -> bool {
        let reach = self.radius + r + DOOR_SEAM_MARGIN_M;
        c.y > self.hinge_y && plan_sq(c, self.axis) <= reach * reach
    }
}

/// Squared plan distance of `v` from `axis`, m² (mul and add only, so native = wasm; `hypot` is
/// the platform's `hypotf` natively).
fn plan_sq(v: Vector, axis: [f32; 2]) -> f32 {
    let (dx, dz) = (v.x - axis[0], v.z - axis[1]);
    dx * dx + dz * dz
}

/// Whether `p` projects into triangle `(a, b, c)` along its normal, within
/// [`INSIDE_TOLERANCE_M`]. A degenerate triangle counts as inside (its contact is left alone).
fn projects_into(p: Vector, a: Vector, b: Vector, c: Vector) -> bool {
    let n = (b - a).cross(c - a);
    let len = n.length();
    if len <= 0.0 {
        return true;
    }
    let n = n / len;
    [(a, b), (b, c), (c, a)].iter().all(|&(u, v)| {
        let e = v - u;
        e.cross(p - u).dot(n) >= -INSIDE_TOLERANCE_M * e.length()
    })
}

/// The step's contact hooks: the door seam correction, then the chunk colliders' own
/// ([`ChunkColliders`]) for everything it leaves.
pub(crate) struct StepHooks<'a> {
    pub pieces: &'a ChunkColliders,
    pub mechanisms: &'a Mechanisms,
    /// The tick the step starts from.
    pub tick: u64,
}

impl StepHooks<'_> {
    /// Apply the door seam rule to this manifold. Returns `true` when it dropped the contacts.
    fn door_seam(&self, ctx: &mut ContactModificationContext) -> bool {
        if !self.mechanisms.has_doors() {
            return false;
        }
        let colliders = ctx.colliders;
        let ball_of = |h: ColliderHandle| colliders[h].shape().as_ball().map(|b| b.radius);
        let (other, ball_body, first, radius) =
            match (ball_of(ctx.collider1), ball_of(ctx.collider2)) {
                (None, Some(r)) => (ctx.collider1, ctx.rigid_body2, true, r),
                (Some(r), None) => (ctx.collider2, ctx.rigid_body1, false, r),
                _ => return false,
            };
        let Some(ball_body) = ball_body else {
            return false;
        };
        let centre = ctx.bodies[ball_body].translation();
        let in_zone = if self.pieces.is_chunk(other) {
            self.mechanisms
                .shut_doors(self.tick)
                .any(|d| d.holds(centre, radius))
        } else {
            self.mechanisms
                .shut_door(other, self.tick)
                .is_some_and(|d| d.holds(centre, radius))
        };
        if !in_zone {
            return false;
        }
        let collider = &colliders[other];
        let Some(mesh) = collider.shape().as_trimesh() else {
            return false;
        };
        let Some(rigid) = ctx.rigid_mut() else {
            return false;
        };
        let tri = if first {
            rigid.manifold.subshape1
        } else {
            rigid.manifold.subshape2
        };
        if tri as usize >= mesh.num_triangles() {
            return false;
        }
        let t = mesh.triangle(tri);
        let pose = collider.position();
        if projects_into(centre, pose * t.a, pose * t.b, pose * t.c) {
            return false;
        }
        rigid.solver_contacts.clear();
        true
    }
}

impl PhysicsHooks for StepHooks<'_> {
    fn filter_contact_pair(&self, ctx: &PairFilterContext) -> Option<SolverFlags> {
        self.pieces.filter_contact_pair(ctx)
    }

    fn modify_solver_contacts(&self, ctx: &mut ContactModificationContext) {
        if !self.door_seam(ctx) {
            self.pieces.modify_solver_contacts(ctx);
        }
    }
}
