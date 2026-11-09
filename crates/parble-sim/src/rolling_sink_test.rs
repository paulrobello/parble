//! Rolling-marble floor sink on a flat trimesh (card 1).
//!
//! Rapier re-measures each contact's separation per solver substep from body-fixed anchors. For a
//! spinning ball the body-fixed anchor rotates away from the true contact point (centre − r·n) and
//! lifts by r(1 − cos θ), so the solver lets the ball sink until that phantom lift is cancelled.
//! The vendored Rapier (`third_party/rapier3d-0.36.1`) tracks ball contact points translation-only.

use crate::Sim;
use crate::marble::CATS_EYE;
use rapier3d::prelude::*;

/// A 40 × 40 grid of 60 mm cells at y = 0, two triangles per cell.
fn flat_floor() -> Collider {
    let n = 40u32;
    let s = 0.06f32;
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for i in 0..=n {
        for k in 0..=n {
            vertices.push(Vector::new(i as f32 * s - 0.3, 0.0, k as f32 * s - 1.2));
        }
    }
    let id = |i: u32, k: u32| i * (n + 1) + k;
    for i in 0..n {
        for k in 0..n {
            triangles.push([id(i, k), id(i, k + 1), id(i + 1, k)]);
            triangles.push([id(i + 1, k), id(i, k + 1), id(i + 1, k + 1)]);
        }
    }
    ColliderBuilder::trimesh_with_flags(vertices, triangles, TriMeshFlags::FIX_INTERNAL_EDGES)
        .unwrap()
        .friction(0.5)
        .restitution(0.4)
        .build()
}

/// Sink statistics over ticks 40..240 of a marble launched along +x on the flat floor.
pub(crate) struct Sink {
    /// Penetration below the floor (r − y), mm.
    pub max_mm: f32,
    pub mean_mm: f32,
    /// End-of-tick vertical velocity, mm/s.
    pub vy_mean: f32,
    pub vy_max_abs: f32,
}

/// `spin`: start rolling without slipping; otherwise slide frictionless.
pub(crate) fn roll_flat(v: f32, spin: bool) -> Sink {
    let mut sim = Sim::empty();
    sim.world.colliders.insert(flat_floor());
    let r = CATS_EYE.radius_m;
    let (mut body, mut collider) = CATS_EYE.build(Vector::new(0.0, r, 0.0));
    body.set_linvel(Vector::X * v, true);
    if spin {
        body.set_angvel(Vector::Y.cross(Vector::X) * (v / r), true);
    } else {
        collider.set_friction(0.0);
    }
    let h = sim.world.bodies.insert(body);
    sim.world
        .colliders
        .insert_with_parent(collider, h, &mut sim.world.bodies);
    sim.marbles.push(h);
    let (mut max_pen, mut sum_pen, mut sum_vy, mut max_vy, mut n) = (0.0f32, 0.0, 0.0, 0.0f32, 0);
    for t in 0..240 {
        sim.tick(1);
        if t >= 40 {
            let b = &sim.world.bodies[h];
            let pen = r - b.translation().y;
            let vy = b.linvel().y * 1e3;
            max_pen = max_pen.max(pen);
            sum_pen += pen;
            sum_vy += vy;
            max_vy = max_vy.max(vy.abs());
            n += 1;
        }
    }
    Sink {
        max_mm: max_pen * 1e3,
        mean_mm: sum_pen / n as f32 * 1e3,
        vy_mean: sum_vy / n as f32,
        vy_max_abs: max_vy,
    }
}

#[test]
fn rolling_marble_does_not_sink_into_a_flat_trimesh() {
    let slide = roll_flat(1.17, false);
    eprintln!(
        "sliding v 1.17: pen max {:.3} mean {:.3} mm",
        slide.max_mm, slide.mean_mm
    );
    let runs: Vec<(f32, Sink)> = [0.3f32, 1.17, 2.0]
        .into_iter()
        .map(|v| (v, roll_flat(v, true)))
        .collect();
    for (v, s) in &runs {
        eprintln!(
            "rolling v {v}: pen max {:.3} mean {:.3} mm, end-of-tick vy mean {:.2} max|vy| {:.2} mm/s",
            s.max_mm, s.mean_mm, s.vy_mean, s.vy_max_abs
        );
    }
    for (v, s) in &runs {
        assert!(
            s.max_mm < 0.1,
            "v {v}: rolling sink {:.3} mm >= 0.1 mm",
            s.max_mm
        );
        // Unpatched Rapier ends every tick at about -20 mm/s while rolling at 1.17 m/s: the
        // contact releases the ball each tick and catches it again.
        assert!(
            s.vy_max_abs < 2.0,
            "v {v}: end-of-tick |vy| {:.2} mm/s, the contact alternates",
            s.vy_max_abs
        );
    }
}
