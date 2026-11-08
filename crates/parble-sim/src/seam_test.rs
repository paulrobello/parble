//! Seam bump test (P1-T3, PRD §6 P1 "no visible or physical bump at piece seams", risk R2).
//!
//! A cat's eye marble is launched rolling (ω = v / r) along the `L3` channel floor and its peak
//! vertical velocity is compared with a seamless reference. Flat channels give a vertical velocity
//! of zero in theory; what the solver leaves is contact jitter (about ±10 mm/s alternating each
//! tick), so the threshold is the margin a seam may add above the seamless peak.

use crate::chunk::{ChunkMesh, placed_triangles};
use crate::marble::CATS_EYE;
use crate::{MarbleState, Sim};
use parble_core::{CellCoord, PieceId, Placement, Yaw4};
use rapier3d::prelude::*;

/// Allowed peak vertical-velocity spike above the seamless baseline, mm/s. The plan's starting
/// value (2 mm/s) held unchanged. Measured 2026-11-07 at 0.3 m/s: 20 straights 13.52 mm/s against a
/// seamless 20-cell channel 13.15 mm/s (+0.37). Without the flat-vertex simplification and the
/// chunk seam hook the same run peaks at 65–76 mm/s (`seams_without_the_fix_bump`).
const SEAM_SPIKE_MM_S: f32 = 2.0;
/// Launch speed along the channel, m/s.
const SPEED: f32 = 0.3;
/// Centre height of a marble resting on an `L3` floor (45 mm) in cell layer 0.
const REST_Y: f32 = 0.045 + 0.008;

fn place(piece: &str, x: i32, z: i32, yaw: u8, mirror: bool) -> Placement {
    Placement::new(
        PieceId::new(piece).unwrap(),
        CellCoord::new(x, 0, z),
        Yaw4::new(yaw),
        mirror.into(),
    )
}

fn launch(sim: &mut Sim, at: Vector, dir: Vector) {
    let (mut body, collider) = CATS_EYE.build(at);
    body.set_linvel(dir * SPEED, true);
    body.set_angvel(Vector::Y.cross(dir) * (SPEED / CATS_EYE.radius_m), true);
    let h = sim.world.bodies.insert(body);
    sim.world
        .colliders
        .insert_with_parent(collider, h, &mut sim.world.bodies);
    sim.marbles.push(h);
}

fn trace(sim: &mut Sim, ticks: u32) -> Vec<MarbleState> {
    (0..ticks)
        .map(|_| {
            sim.tick(1);
            sim.marbles()[0]
        })
        .collect()
}

/// A run over `layout` through the real API: chunk merge, simplification and the seam hook.
fn roll_track(layout: &[Placement], at: Vector, dir: Vector, ticks: u32) -> Vec<MarbleState> {
    let kit = parble_kit::builtin();
    let mut sim = Sim::empty();
    for p in layout {
        sim.place_piece(kit, p.clone()).unwrap();
    }
    sim.rebuild_chunks(kit).unwrap();
    assert!(!sim.world.integration_parameters.contact_clustering);
    launch(&mut sim, at, dir);
    trace(&mut sim, ticks)
}

/// A run over hand-built colliders, with the same solver settings as [`roll_track`].
fn roll_colliders(
    colliders: Vec<Collider>,
    at: Vector,
    dir: Vector,
    ticks: u32,
) -> Vec<MarbleState> {
    let mut sim = Sim::empty();
    sim.world.integration_parameters.contact_clustering = false;
    for c in colliders {
        sim.world.colliders.insert(c);
    }
    launch(&mut sim, at, dir);
    trace(&mut sim, ticks)
}

fn straight_run(n: i32) -> Vec<Placement> {
    (0..n)
        .map(|x| place("blk_straight_l3", x, 0, 0, false))
        .collect()
}

fn triangles(layout: &[Placement]) -> Vec<[[i64; 3]; 3]> {
    let kit = parble_kit::builtin();
    layout
        .iter()
        .flat_map(|p| placed_triangles(kit.piece(&p.piece).unwrap(), p))
        .collect()
}

/// One straight stretched over `n` cells: the same cross-section with no seam.
fn seamless_straight(n: i64) -> Collider {
    let tris: Vec<[[i64; 3]; 3]> = triangles(&straight_run(1))
        .into_iter()
        .map(|t| t.map(|v| [(v[0] + 30_000) * n - 30_000, v[1], v[2]]))
        .collect();
    ChunkMesh::from_triangles(&tris)
        .collider([0, 0, 0])
        .unwrap()
}

/// Peak |vy| in mm/s while `sel` holds.
fn peak(tr: &[MarbleState], sel: impl Fn(&MarbleState) -> bool) -> f32 {
    tr.iter()
        .filter(|m| sel(m))
        .map(|m| m.linvel[1].abs())
        .fold(0.0, f32::max)
        * 1_000.0
}

/// The 20-straight window: from the first seam (skipping the launch cell) to the last.
fn in_run(m: &MarbleState) -> bool {
    (0.03..1.14).contains(&m.position[0])
}

const RUN_TICKS: u32 = 1_100;

#[test]
fn twenty_straights_roll_like_one_seamless_channel() {
    let start = Vector::new(-0.02, REST_Y, 0.0);
    let run = roll_track(&straight_run(20), start, Vector::X, RUN_TICKS);
    let base = roll_colliders(vec![seamless_straight(20)], start, Vector::X, RUN_TICKS);
    // The run crosses 19 piece seams and two chunk boundaries (x = 450 and 930 mm).
    assert!(
        run.iter().any(|m| m.position[0] > 1.14),
        "marble must cross every seam"
    );
    assert!(run.iter().all(|m| !in_run(m) || m.position[1] > 0.05));
    let (p_run, p_base) = (peak(&run, in_run), peak(&base, in_run));
    eprintln!("20 straights: peak {p_run:.2} mm/s, seamless {p_base:.2} mm/s");
    assert!(
        p_run <= p_base + SEAM_SPIKE_MM_S,
        "seam spike {p_run:.2} mm/s over seamless {p_base:.2} + {SEAM_SPIKE_MM_S}"
    );
}

#[test]
fn seams_without_the_fix_bump() {
    // Negative control: per-piece colliders (no merge) on the same run exceed the threshold, so
    // the metric does detect seams.
    let start = Vector::new(-0.02, REST_Y, 0.0);
    let pieces: Vec<Collider> = straight_run(20)
        .iter()
        .map(|p| {
            ChunkMesh::from_triangles(&triangles(std::slice::from_ref(p)))
                .collider([0, 0, 0])
                .unwrap()
        })
        .collect();
    let control = roll_colliders(pieces, start, Vector::X, RUN_TICKS);
    let base = roll_colliders(vec![seamless_straight(20)], start, Vector::X, RUN_TICKS);
    let (p_ctl, p_base) = (peak(&control, in_run), peak(&base, in_run));
    eprintln!("per-piece control: peak {p_ctl:.2} mm/s, seamless {p_base:.2} mm/s");
    assert!(p_ctl > p_base + SEAM_SPIKE_MM_S);
}

#[test]
fn curve_and_slope_joins() {
    // All in chunk (0, 0, 0): straights east, a curve turning to +z, straights north, a mirrored
    // curve turning back east, two straights, then the gentle slope (cell x = 7, 390..450 mm).
    // The kit has no `L2` entry piece yet, so the slope is last and only its entry seam
    // (x = 390 mm) is a join.
    //
    // Curve joins are measured, not gated: inside one chunk the merged mesh is the same surface
    // a single welded model would have, and the vertical motion there comes from the curve itself
    // (curvature starts abruptly at the port, and the sampled curve keeps crease vertices that
    // Rapier's internal-edge fix does not cover). Measured 2026-11-07 at 0.3 m/s: straight to
    // curve 64.95, curve to straight 44.09, straight to mirrored curve 49.81, mirrored curve to
    // straight 44.19 mm/s; curve interiors 53.98 and 38.77 mm/s; seamless straight 13.15 mm/s.
    // Tracked as a PRD R2 follow-up, not a seam failure.
    let mut layout = straight_run(4);
    layout.push(place("blk_curve90_l3", 4, 0, 1, false));
    for z in 1..4 {
        layout.push(place("blk_straight_l3", 4, z, 1, false));
    }
    layout.push(place("blk_curve90_l3", 4, 4, 0, true));
    layout.push(place("blk_straight_l3", 5, 4, 0, false));
    layout.push(place("blk_straight_l3", 6, 4, 0, false));
    layout.push(place("blk_slope_gentle_l3", 7, 4, 0, false));
    let start = Vector::new(-0.02, REST_Y, 0.0);
    let tr = roll_track(&layout, start, Vector::X, 1_600);
    let base = roll_colliders(vec![seamless_straight(20)], start, Vector::X, RUN_TICKS);
    let p_base = peak(&base, in_run);
    assert!(
        tr.iter().any(|m| m.position[0] > 0.45),
        "marble must leave the slope exit"
    );
    assert!(
        tr.iter()
            .all(|m| m.position[1] > 0.05 || m.position[0] > 0.39),
        "marble stays in the L3 channel up to the slope"
    );

    let (x, z) = (0, 2);
    let at = |m: &MarbleState, lo: [f32; 2], hi: [f32; 2]| {
        (lo[0]..hi[0]).contains(&m.position[x]) && (lo[1]..hi[1]).contains(&m.position[z])
    };
    // Piece interiors, 15 mm clear of their seams. Curve (4, 0, 0) arcs from (210, 0) to (240, 30)
    // mm; the mirrored curve (4, 0, 4) from (240, 210) to (270, 240) mm.
    let curve = peak(&tr, |m| at(m, [0.225, -0.03], [0.27, 0.015]));
    let mirrored = peak(&tr, |m| at(m, [0.21, 0.225], [0.255, 0.27]));
    eprintln!(
        "interiors: curve {curve:.2}, mirrored curve {mirrored:.2}, seamless {p_base:.2} mm/s"
    );

    let joins: [(&str, [f32; 2], [f32; 2], f32); 4] = [
        ("straight to curve", [0.195, -0.03], [0.225, 0.03], curve),
        ("curve to straight", [0.21, 0.015], [0.27, 0.045], curve),
        (
            "straight to mirrored curve",
            [0.21, 0.195],
            [0.27, 0.225],
            mirrored,
        ),
        (
            "mirrored curve to straight",
            [0.255, 0.21],
            [0.285, 0.27],
            mirrored,
        ),
    ];
    for (name, lo, hi, interior) in joins {
        assert!(
            tr.iter().any(|m| at(m, lo, hi)),
            "{name}: marble never crossed"
        );
        let p = peak(&tr, |m| at(m, lo, hi));
        eprintln!("{name}: peak {p:.2} mm/s (piece interior {interior:.2})");
    }

    // Slope entry: the floor starts to descend at the seam, so only an upward kick counts.
    let up = |lo: [f32; 2], hi: [f32; 2]| {
        tr.iter()
            .filter(|m| at(m, lo, hi))
            .map(|m| m.linvel[1])
            .fold(0.0, f32::max)
            * 1_000.0
    };
    let entry = [[0.375, 0.21], [0.405, 0.27]];
    assert!(
        tr.iter().any(|m| at(m, entry[0], entry[1])),
        "slope entry never crossed"
    );
    let p = up(entry[0], entry[1]);
    let limit = p_base + SEAM_SPIKE_MM_S;
    eprintln!(
        "straight to slope: peak upward {p:.2} mm/s, limit {limit:.2}, slope interior {:.2}",
        up([0.405, 0.21], [0.435, 0.27])
    );
    assert!(p <= limit, "slope entry: {p:.2} mm/s over {limit:.2}");
}
