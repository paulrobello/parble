//! Fork contact diagnosis (2026-11-09): per-tick marble contacts in the splitter and toggle.
//!
//! [`fork_contact_dump`] prints every manifold the marble holds after each tick for one case of
//! `behaviour::tests::shipped_fork_parts_route_every_marble_type_and_size` (`FORK_CASE`,
//! `FORK_TICKS`, `FORK_FROM`), with the manifold normal against its triangle's face normal and
//! whether the marble centre projects into that triangle. [`fork_outlet_dump`] runs the
//! `block_seam_test` outlet case and prints the per-tick kick terms in the outlet window.
//!
//! Measured on the D1 blade (wip/fork-blade-d1, 27abb6c; kept with a round-contact prototype on
//! wip/fork-round-contact, 281773b). No fork or blade contact is an edge artefact:
//! - The splitter's -z outlet kick (6.38, 4.87 on the shipped blade) is the marble rolling across
//!   a crease of the downstream straight's faceted section: two face contacts (normal equal to
//!   the face normal, centre inside the triangle), the bottom crease's ±5.6° facets at 27.7 mm/s
//!   lateral speed on D1, the 5.6°/16.9° crease on the shipped blade. A blade changes only how
//!   fast the marble swings across the straight.
//! - Every blade contact is a face contact on the blade's side, plus one genuine convex contact on
//!   its underside edge near the pivot.
//! - The routing failures are the commanded side opposite the side the marble drifts to at the
//!   fork mouth. The faceted junction contact reproduces the bottom crease, so where the two arcs
//!   part the floor is a 5.6° ridge: a marble a fraction of a millimetre off centre accelerates
//!   sideways at 5/7 g tan 5.6° (shooter vz -3 to -23 mm/s from x 30 to 37 mm, mini +13 to +56). A
//!   shooter and its mirror case run identically until the blade: toward the open side it passes,
//!   toward the toggle's blade it hits the tip head on (vx 191 to 40 mm/s) and then stalls where
//!   the blade at 36° stands 1.0 mm inside the open branch's tube, a 0.38 mm climb up the far wall.
//!
//! With the straights and the forks on a round exact contact (the prototype), both outlet kicks
//! drop to 0.30 and 0.12, but the acceptance test still fails 21 runs, all on the toggle sent to
//! +z: centred marbles take the -z trough by the junction's tie-break (`max_by` keeps the last of
//! equal gaps) and bounce off the blade, which reaches the marble only 11.6 mm past the crotch.
//! Every splitter run passes, but both acceptance seeds (3, 11) command -z, the tie-break's side.
//! The prototype also fails 8 sim tests D1 alone passes (stepper climb, straight_to_slope_join,
//! both tower drop tests, rail contacts, the bend-row rest control, the showcase toggle shot, the
//! playroom hash); D1 alone fails the 7 all-pieces runs.
//!
//! In the tree since 191b7c8: the straights take the exact tube (`chunk::ChannelStraight`) and the
//! forks a round junction whose two arcs are blended within 50 um of the crotch ridge
//! (`chunk::ChannelJunction::fork`), so a centred marble has no side bias. [`Blade`] and
//! `behaviour::tests::fork_blade_sweep` swap the fork blade's collider for a fan prism built like
//! kitmech's, to score blade designs without a kit build.

use crate::behaviour::{SPLITTER, TOGGLE, registry};
use crate::marble::{MarbleKind, MarbleSize, MarbleSpec};
use crate::{MarbleState, Sim};
use parble_core::{CellCoord, Mirror, PieceId, Placement, Yaw4};
use rapier3d::prelude::*;

fn place(piece: &str, x: i32, y: i32, z: i32, yaw: u8) -> Placement {
    Placement::new(
        PieceId::new(piece).unwrap(),
        CellCoord::new(x, y, z),
        Yaw4::new(yaw),
        Mirror::Off,
    )
}

pub(crate) fn fork_sim(piece: &str, first: i64, seed: u64) -> Sim {
    let kit = parble_kit::builtin();
    let reg = registry();
    let mut t = place(piece, 1, 0, 0, 0);
    t.config.insert("first".into(), first);
    let layout = [place("blk_straight_l3", 0, 0, 0, 0), t];
    let mut sim = Sim::empty();
    for p in &layout {
        sim.place_piece(kit, p.clone()).unwrap();
    }
    sim.rebuild_chunks(kit).unwrap();
    for p in &layout {
        if let Some(f) = reg.get(&p.piece) {
            sim.add_mechanism(kit.piece(&p.piece).unwrap(), p, seed, f(&p.config))
                .unwrap();
        }
    }
    sim
}

pub(crate) fn launch(sim: &mut Sim, spec: &MarbleSpec, at: Vector, vel: Vector) {
    let (mut body, collider) = spec.build(at);
    let speed = vel.length();
    body.set_linvel(vel, true);
    body.set_angvel(Vector::Y.cross(vel / speed) * (speed / spec.radius_m), true);
    let h = sim.world.bodies.insert(body);
    sim.world
        .colliders
        .insert_with_parent(collider, h, &mut sim.world.bodies);
    sim.marbles.push(h);
}

/// One manifold of the marble after a step.
#[derive(Debug, Clone)]
pub(crate) struct Touch {
    /// "chunk" or "part".
    pub kind: &'static str,
    pub tri: u32,
    /// Manifold normal, pointing into the marble.
    pub normal: Vector,
    /// The triangle's face normal, on the marble's side.
    pub face: Vector,
    /// Angle between them, degrees.
    pub dev_deg: f32,
    /// Whether the marble centre projects into the triangle.
    pub inside: bool,
    pub impulse: f32,
    pub solver: usize,
    pub dist: f32,
}

pub(crate) fn touches(sim: &Sim) -> Vec<Touch> {
    let w = &sim.world;
    let h = sim.marbles[0];
    let mc = w.bodies[h].colliders()[0];
    let centre = w.bodies[h].translation();
    let mut out = Vec::new();
    for pair in w.narrow_phase.contact_pairs_with(mc) {
        let (other, flip) = if pair.collider1 == mc {
            (pair.collider2, false)
        } else {
            (pair.collider1, true)
        };
        let col = &w.colliders[other];
        let kind = if sim.pieces.is_chunk(other) {
            "chunk"
        } else {
            "part"
        };
        for m in pair.manifolds() {
            if m.points.is_empty() {
                continue;
            }
            // data.normal points from collider1 to collider2.
            let n = if flip { m.data.normal } else { -m.data.normal };
            let tri = if flip { m.subshape1 } else { m.subshape2 };
            let (face, inside) = match col.shape().as_trimesh() {
                Some(mesh) if (tri as usize) < mesh.num_triangles() => {
                    let t = mesh.triangle(tri);
                    let p = col.position();
                    let (a, b, c) = (p * t.a, p * t.b, p * t.c);
                    let mut f = (b - a).cross(c - a).normalize();
                    if f.dot(centre - a) < 0.0 {
                        f = -f;
                    }
                    let inside = [(a, b), (b, c), (c, a)]
                        .iter()
                        .all(|&(u, v)| (v - u).cross(centre - u).dot(f) >= -1.0e-6);
                    (f, inside)
                }
                _ => (Vector::ZERO, false),
            };
            let dev = libm::acosf(n.dot(face).clamp(-1.0, 1.0)).to_degrees();
            out.push(Touch {
                kind,
                tri,
                normal: n,
                face,
                dev_deg: dev,
                inside,
                impulse: m.points.iter().map(|p| p.data.impulse).sum(),
                solver: m.data.solver_contacts.len(),
                dist: m.points.iter().map(|p| p.dist).fold(f32::MAX, f32::min),
            });
        }
    }
    out
}

pub(crate) struct Trace {
    pub states: Vec<MarbleState>,
    pub touches: Vec<Vec<Touch>>,
}

pub(crate) fn trace(
    piece: &str,
    first: i64,
    seed: u64,
    spec: &MarbleSpec,
    v: f32,
    ticks: u32,
) -> Trace {
    let mut sim = fork_sim(piece, first, seed);
    launch(
        &mut sim,
        spec,
        Vector::new(-0.02, 0.045 + spec.radius_m, 0.0),
        Vector::X * v,
    );
    let (mut states, mut ts) = (Vec::new(), Vec::new());
    for _ in 0..ticks {
        sim.tick(1);
        states.push(sim.marbles()[0]);
        ts.push(touches(&sim));
    }
    Trace {
        states,
        touches: ts,
    }
}

fn fmt_v(v: Vector) -> String {
    format!("({:+.3},{:+.3},{:+.3})", v.x, v.y, v.z)
}

pub(crate) fn dump(t: &Trace, from: usize, to: usize) {
    for i in from..to.min(t.states.len()) {
        let s = t.states[i];
        let p = s.position.map(|c| c * 1e3);
        let l = s.linvel.map(|c| c * 1e3);
        let dvy = if i > 0 {
            (s.linvel[1] - t.states[i - 1].linvel[1]) * 1e3
        } else {
            0.0
        };
        eprintln!(
            "t{i:4} p=({:6.2},{:6.2},{:+6.2}) v=({:+7.1},{:+7.1},{:+7.1}) dvy={dvy:+6.1}",
            p[0], p[1], p[2], l[0], l[1], l[2]
        );
        for c in &t.touches[i] {
            eprintln!(
                "      {:5} tri {:5} n={} face={} dev={:5.1} in={} imp={:.2e} sc={} d={:+.2e}",
                c.kind,
                c.tri,
                fmt_v(c.normal),
                fmt_v(c.face),
                c.dev_deg,
                c.inside,
                c.impulse,
                c.solver,
                c.dist
            );
        }
    }
}

/// Edge-like contacts (`dev` over `min_dev` degrees, carrying impulse) per kind and side.
pub(crate) fn edge_summary(t: &Trace, min_dev: f32) -> Vec<String> {
    let mut out = Vec::new();
    for (i, ts) in t.touches.iter().enumerate() {
        for c in ts {
            if c.dev_deg > min_dev && c.solver > 0 {
                let p = t.states[i].position.map(|c| c * 1e3);
                out.push(format!(
                    "t{i} {} tri {} dev {:.1} in {} imp {:.2e} at ({:.1},{:.1},{:+.1})",
                    c.kind, c.tri, c.dev_deg, c.inside, c.impulse, p[0], p[1], p[2]
                ));
            }
        }
    }
    out
}

fn spec(kind: MarbleKind, size: MarbleSize) -> MarbleSpec {
    kind.spec_sized(size)
}

#[test]
#[ignore]
fn fork_contact_dump() {
    let which = std::env::var("FORK_CASE").unwrap_or_default();
    let (piece, first, seed, sp, v) = match which.as_str() {
        "toggle_shooter" => (
            TOGGLE,
            0,
            3,
            spec(MarbleKind::CatsEye, MarbleSize::Shooter),
            0.2,
        ),
        "toggle_shooter_m" => (
            TOGGLE,
            1,
            11,
            spec(MarbleKind::CatsEye, MarbleSize::Shooter),
            0.2,
        ),
        "toggle_mini" => (
            TOGGLE,
            0,
            3,
            spec(MarbleKind::CatsEye, MarbleSize::Mini),
            0.2,
        ),
        "splitter_mini" => (
            SPLITTER,
            1,
            11,
            spec(MarbleKind::CatsEye, MarbleSize::Mini),
            0.2,
        ),
        "splitter_steel_mini" => (
            SPLITTER,
            1,
            11,
            spec(MarbleKind::Steel, MarbleSize::Mini),
            0.2,
        ),
        "splitter_std" => (
            SPLITTER,
            1,
            11,
            spec(MarbleKind::CatsEye, MarbleSize::Standard),
            0.3,
        ),
        _ => (
            TOGGLE,
            0,
            3,
            spec(MarbleKind::CatsEye, MarbleSize::Standard),
            0.3,
        ),
    };
    let ticks: u32 = std::env::var("FORK_TICKS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(600);
    let from: usize = std::env::var("FORK_FROM")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let t = trace(piece, first, seed, &sp, v, ticks);
    dump(&t, from, ticks as usize);
    for l in edge_summary(&t, 2.0) {
        eprintln!("EDGE {l}");
    }
}

/// The `block_seam_test` fork outlet layout: the fork at (1, 0, 0) with straights on all ports.
pub(crate) fn outlet_sim(piece: &str) -> Sim {
    let kit = parble_kit::builtin();
    let reg = registry();
    let layout = [
        place("blk_straight_l3", 0, 0, 0, 0),
        place(piece, 1, 0, 0, 0),
        place("blk_straight_l3", 1, 0, 1, 1),
        place("blk_straight_l3", 1, 0, -1, 1),
        place("blk_straight_l3", 1, 0, 2, 1),
        place("blk_straight_l3", 1, 0, -2, 1),
    ];
    let mut sim = Sim::empty();
    for p in &layout {
        sim.place_piece(kit, p.clone()).unwrap();
    }
    sim.rebuild_chunks(kit).unwrap();
    for p in &layout {
        sim.add_registered(&reg, kit, p, 0).unwrap();
    }
    sim
}

/// Per-tick kick terms (`seam_test::kick`): vertical-velocity and plan-speed second differences.
pub(crate) fn kicks(states: &[MarbleState]) -> Vec<(f32, f32)> {
    let mut out = vec![(0.0, 0.0); states.len()];
    for i in 1..states.len().saturating_sub(2) {
        let w = &states[i - 1..i + 3];
        let vel = (w[1].linvel[1] - 0.5 * (w[0].linvel[1] + w[2].linvel[1])).abs();
        let v = |a: &MarbleState, b: &MarbleState| {
            let d = [0, 2].map(|k| (b.position[k] - a.position[k]) / crate::DT);
            d[0].hypot(d[1])
        };
        let (va, vb, vc) = (v(&w[0], &w[1]), v(&w[1], &w[2]), v(&w[2], &w[3]));
        out[i] = (vel * 1e3, (vb - 0.5 * (va + vc)).abs() * 1e3);
    }
    out
}

fn env_usize(k: &str, d: usize) -> usize {
    std::env::var(k)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(d)
}

#[test]
#[ignore]
fn fork_outlet_dump() {
    let toggle = std::env::var("FORK_CASE").as_deref() == Ok("toggle");
    let piece = if toggle { TOGGLE } else { SPLITTER };
    let mut sim = outlet_sim(piece);
    let spec = crate::marble::CATS_EYE;
    launch(
        &mut sim,
        &spec,
        Vector::new(-0.02, 0.045 + spec.radius_m, 0.0),
        Vector::X * 0.3,
    );
    let (mut states, mut ts) = (Vec::new(), Vec::new());
    for _ in 0..900 {
        sim.tick(1);
        states.push(sim.marbles()[0]);
        ts.push(touches(&sim));
    }
    let k = kicks(&states);
    let zs = if toggle { 1.0 } else { -1.0 };
    for (i, s) in states.iter().enumerate() {
        let p = s.position;
        if (0.03..0.09).contains(&p[0]) && (0.03..0.045).contains(&(p[2] * zs)) {
            eprintln!("KICK t{i} vel {:.2} plan {:.2}", k[i].0, k[i].1);
        }
    }
    let t = Trace {
        states,
        touches: ts,
    };
    dump(&t, env_usize("FORK_FROM", 0), env_usize("FORK_TO", 900));
}

/// A straight fork blade for the blade sweep (`behaviour::tests::fork_blade_sweep`): hinged
/// about +Y at piece-local `(pivot_x_mm, floor, 0)`, its tip `len_mm` upstream, swinging to
/// `±limit_deg`, 2 mm thick and 14 mm tall, its underside 0.5 mm over the exact fork floor
/// everywhere in the sweep (as kitmech's `BLADE_UNDER`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Blade {
    pub pivot_x_mm: f32,
    pub len_mm: f32,
    pub limit_deg: f32,
    /// Underside clearance over the floor, mm (kitmech: 0.5).
    pub clear_mm: f32,
}

thread_local! {
    static BLADE: std::cell::Cell<Option<Blade>> = const { std::cell::Cell::new(None) };
}

/// Make [`blade_override`] replace the fork parts' blade with `b` on this thread (`None`: the
/// sidecar's).
pub(crate) fn set_blade(b: Option<Blade>) {
    BLADE.with(|c| c.set(b));
}

/// The fork piece with the blade set by [`set_blade`], for `Sim::add_mechanism`.
pub(crate) fn blade_override(piece: &parble_kit::KitPiece) -> Option<parble_kit::KitPiece> {
    let b = BLADE.with(|c| c.get())?;
    let id = piece.def.id.as_str();
    if id != SPLITTER && id != TOGGLE {
        return None;
    }
    let mut out = piece.clone();
    let part = &mut out.animated_parts[0];
    let lim = (b.limit_deg * 1000.0).round() as i32;
    part.pivot_um = [(b.pivot_x_mm * 1000.0).round() as i32, 45_000, 0];
    part.limits_mdeg = [-lim, lim];
    part.rest_mdeg = if id == SPLITTER { lim } else { -lim };
    part.collision = vec![blade_mesh(&b)];
    Some(out)
}

/// Height of the exact fork channel floor over the `L3` floor at piece-local plan point `(x, z)`
/// (mm): the lower of the two arcs' half-rounds (radius 14 mm about quarter circles of radius
/// 30 mm about `(-30, ±30)`), the inlet straight's for `x < -30`; 14 where neither holds it.
fn fork_floor_mm(x: f32, z: f32) -> f32 {
    let half_round = |rho: f32| {
        if rho.abs() >= 14.0 {
            14.0
        } else {
            14.0 - (196.0 - rho * rho).sqrt()
        }
    };
    if x < -30.0 {
        return half_round(z);
    }
    [30.0f32, -30.0]
        .iter()
        .map(|&cz| half_round(((x + 30.0).hypot(z - cz)) - 30.0))
        .fold(14.0, f32::min)
}

fn blade_mesh(b: &Blade) -> parble_kit::CollisionGroup {
    // kitmech's outline: underside knots from the pivot to the tip (at D1's 0, 2, 5, 9, 14, 19 and
    // 24 of 24 mm), then the tip's and the pivot's top, extruded ±1 mm and fanned from vertex 0.
    let mut poly = Vec::new();
    for f in [0.0f32, 2.0, 5.0, 9.0, 14.0, 19.0, 24.0] {
        let s = b.len_mm * f / 24.0;
        let mut under: f32 = 0.0;
        for k in -40..=40 {
            let th = (b.limit_deg * k as f32 / 40.0).to_radians();
            let (sn, cs) = th.sin_cos();
            for w in [-1.0f32, 0.0, 1.0] {
                let x = b.pivot_x_mm - s * cs + w * sn;
                let z = s * sn + w * cs;
                under = under.max(fork_floor_mm(x, z));
            }
        }
        let under = ((under + b.clear_mm) * 10.0).round() / 10.0;
        poly.push((b.pivot_x_mm - s, under.min(13.0)));
    }
    poly.push((b.pivot_x_mm - b.len_mm, 14.0));
    poly.push((b.pivot_x_mm, 14.0));
    let um = |v: f32| (v * 1000.0).round() as i32;
    let n = poly.len() as u32;
    let mut vertices = Vec::new();
    for z in [-1000, 1000] {
        vertices.extend(poly.iter().map(|&(x, y)| [um(x), 45_000 + um(y), z]));
    }
    let mut triangles = Vec::new();
    for i in 1..n - 1 {
        triangles.push([0, i, i + 1]);
        triangles.push([2 * n - 1, 2 * n - 1 - i, 2 * n - 2 - i]);
    }
    for i in 0..n {
        let j = (i + 1) % n;
        triangles.push([i, j, n + j]);
        triangles.push([i, n + j, n + i]);
    }
    parble_kit::CollisionGroup {
        surface: parble_kit::Surface::WoodEndGrain,
        vertices_um: vertices,
        triangles,
    }
}
