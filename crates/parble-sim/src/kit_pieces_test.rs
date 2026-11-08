//! P2-T2 passive block pieces: a cat's eye marble is run through each new piece in a small
//! layout through the real API (chunk merge, simplification, contact hooks), and the test asserts
//! the port it leaves by and its speed there.
//!
//! The exit speed is bounded above by energy: with the launch rolling (ω = v / r), kinetic
//! energy is 0.7 m v0², so at the exit `v² ≤ 1.4 v0² + 2 g Δh` whatever mix of rolling,
//! sliding and falling happened on the way. The lower bound is half the speed of the ideal
//! loss-free roll for the same drop (`v² = v0² + (10/7) g Δh`): it catches a marble that stalls,
//! jams or crawls out, not normal rolling and impact losses.

use crate::marble::{CATS_EYE, G};
use crate::{MarbleState, Sim};
use parble_core::{CellCoord, PieceId, Placement, Yaw4};
use rapier3d::prelude::*;

const R: f32 = 0.008;
const V0: f32 = 0.3;

fn place(piece: &str, x: i32, y: i32, z: i32, yaw: u8, mirror: bool) -> Placement {
    Placement::new(
        PieceId::new(piece).unwrap(),
        CellCoord::new(x, y, z),
        Yaw4::new(yaw),
        mirror.into(),
    )
}

/// `ticks` of a marble launched rolling at `at` with velocity `vel` over `layout`.
fn run(layout: &[Placement], at: Vector, vel: Vector, ticks: u32) -> Vec<MarbleState> {
    let kit = parble_kit::builtin();
    let mut sim = Sim::empty();
    for p in layout {
        sim.place_piece(kit, p.clone()).unwrap();
    }
    sim.rebuild_chunks(kit).unwrap();
    let (mut body, collider) = CATS_EYE.build(at);
    let speed = vel.length();
    body.set_linvel(vel, true);
    body.set_angvel(
        Vector::Y.cross(vel / speed) * (speed / CATS_EYE.radius_m),
        true,
    );
    let h = sim.world.bodies.insert(body);
    sim.world
        .colliders
        .insert_with_parent(collider, h, &mut sim.world.bodies);
    sim.marbles.push(h);
    (0..ticks)
        .map(|_| {
            sim.tick(1);
            sim.marbles()[0]
        })
        .collect()
}

/// A port as a plane the marble centre crosses: `axis` (0 x, 1 y, 2 z), plane coordinate and
/// travel sign, and where the centre must be on the plane (`centre`, metres; the plane axis entry
/// is ignored) within `lateral` across the port and, for side ports, `rise` above the rest height.
struct Exit {
    name: &'static str,
    axis: usize,
    plane: f32,
    sign: f32,
    centre: [f32; 3],
    lateral: f32,
    rise: f32,
    /// For a piece the marble falls through and lands in: the drop from the landing floor to this
    /// port, metres. A landing absorbs the fall, so the lower bound counts only this drop.
    landing: Option<f32>,
}

/// A side port on an X or Z face: the marble rests on the floor at `floor` (world metres).
fn side(name: &'static str, axis: usize, plane: f32, sign: f32, u: f32, floor: f32) -> Exit {
    let mut centre = [0.0; 3];
    centre[2 - axis] = u;
    centre[1] = floor + R;
    Exit {
        name,
        axis,
        plane,
        sign,
        centre,
        lateral: 0.0061, // §6: a standard marble's 6 mm play in the 28 mm channel, + 0.1 mm
        rise: 0.015,
        landing: None,
    }
}

/// `e` reached after a fall onto a landing floor (redirect tunnel, tower socket block): the
/// landing floor sits `LANDING_RISE` above the exit floor (blender/kit/kitblocks.py).
fn landed(e: Exit) -> Exit {
    Exit {
        landing: Some(0.015),
        ..e
    }
}

/// A bottom drop port at plan centre `(x, z)`, face height `y`.
fn bottom(name: &'static str, x: f32, y: f32, z: f32) -> Exit {
    Exit {
        name,
        axis: 1,
        plane: y,
        sign: -1.0,
        centre: [x, y, z],
        lateral: 0.008, // §4: 32 mm hole, 16 mm marble
        rise: 0.0,
        landing: None,
    }
}

/// Checks the first crossing of `exit` after tick `after` and returns (tick, speed).
fn check_exit(tr: &[MarbleState], exit: &Exit, start_y: f32, after: usize) -> (usize, f32) {
    let ax = exit.axis;
    let past = |m: &MarbleState| (m.position[ax] - exit.plane) * exit.sign >= 0.0;
    let i = (after.max(1)..tr.len())
        .find(|&i| past(&tr[i]) && !past(&tr[i - 1]))
        .unwrap_or_else(|| {
            let last = tr.last().unwrap().position;
            panic!(
                "{}: marble never left by this port (last {last:?})",
                exit.name
            )
        });
    let m = &tr[i];
    for k in (0..3).filter(|&k| k != ax) {
        let d = m.position[k] - exit.centre[k];
        if ax != 1 && k == 1 {
            assert!(
                (-0.001..=exit.rise).contains(&d),
                "{}: centre {:.4} m at the port, rest height {:.4}",
                exit.name,
                m.position[1],
                exit.centre[1]
            );
        } else {
            assert!(
                d.abs() <= exit.lateral,
                "{}: centre {:?} is {:.4} m off the port centre on axis {k}",
                exit.name,
                m.position,
                d
            );
        }
    }
    let v = Vector::from(m.linvel);
    assert!(
        v[ax] * exit.sign > 0.0,
        "{}: velocity {:?} does not leave through the port",
        exit.name,
        m.linvel
    );
    let dh = start_y - m.position[1];
    let hi = (1.4 * V0 * V0 + 2.0 * G * dh).sqrt();
    let lo = match exit.landing {
        Some(h) => 0.5 * (10.0 / 7.0 * G * h).sqrt(),
        None => 0.5 * (V0 * V0 + 10.0 / 7.0 * G * dh).max(0.0).sqrt(),
    };
    let s = v.length();
    eprintln!(
        "{}: tick {i}, centre {:?}, speed {s:.3} m/s (range {lo:.3}..{hi:.3}, drop {:.1} mm)",
        exit.name,
        m.position,
        dh * 1000.0
    );
    assert!(
        (lo..=hi).contains(&s),
        "{}: exit speed {s:.3} m/s outside {lo:.3}..{hi:.3}",
        exit.name
    );
    (i, s)
}

/// Checks each exit in order along one run.
fn check_exits(tr: &[MarbleState], exits: &[Exit], start_y: f32) {
    let mut after = 0;
    for e in exits {
        after = check_exit(tr, e, start_y, after).0;
    }
}

/// Rest centre height on a floor `level` in cell layer `layer`.
fn rest(layer: i32, level: i32) -> f32 {
    0.06 * layer as f32 + 0.015 * level as f32 + R
}

#[test]
fn steep_slope_drops_two_levels_onto_l1() {
    let mut layout: Vec<Placement> = (0..2)
        .map(|x| place("blk_straight_l3", x, 0, 0, 0, false))
        .collect();
    layout.push(place("blk_slope_steep_l3", 2, 0, 0, 0, false));
    for x in 3..6 {
        layout.push(place("blk_straight_l1", x, 0, 0, 0, false));
    }
    let start = Vector::new(-0.02, rest(0, 3), 0.0);
    let tr = run(&layout, start, Vector::X * V0, 1_200);
    check_exits(
        &tr,
        &[
            side("blk_slope_steep_l3 +x (L1)", 0, 0.15, 1.0, 0.0, 0.015),
            side("blk_straight_l1 run-out", 0, 0.33, 1.0, 0.0, 0.015),
        ],
        start.y,
    );
}

/// Straight, gentle slope (L3 -> L2), then the L2 pieces that now follow it: straight, curve,
/// mirrored curve, gentle slope (L2 -> L1), then the L1 straight.
fn gentle_chain() -> Vec<Placement> {
    vec![
        place("blk_straight_l3", 0, 0, 0, 0, false),
        place("blk_slope_gentle_l3", 1, 0, 0, 0, false),
        place("blk_straight_l2", 2, 0, 0, 0, false),
        place("blk_curve90_l2", 3, 0, 0, 1, false),
        place("blk_straight_l2", 3, 0, 1, 1, false),
        place("blk_curve90_l2", 3, 0, 2, 0, true),
        place("blk_slope_gentle_l2", 4, 0, 2, 0, false),
        place("blk_straight_l1", 5, 0, 2, 0, false),
        place("blk_straight_l1", 6, 0, 2, 0, false),
    ]
}

#[test]
fn gentle_slope_exit_composes_with_the_l2_pieces() {
    let start = Vector::new(-0.02, rest(0, 3), 0.0);
    let tr = run(&gentle_chain(), start, Vector::X * V0, 2_000);
    check_exits(
        &tr,
        &[
            side("blk_slope_gentle_l3 +x (L2)", 0, 0.09, 1.0, 0.0, 0.030),
            side("blk_straight_l2 +x", 0, 0.15, 1.0, 0.0, 0.030),
            side("blk_curve90_l2 +z", 2, 0.03, 1.0, 0.18, 0.030),
            side("blk_straight_l2 (yaw 1) +z", 2, 0.09, 1.0, 0.18, 0.030),
            side("blk_curve90_l2 mirrored +x", 0, 0.21, 1.0, 0.12, 0.030),
            side("blk_slope_gentle_l2 +x (L1)", 0, 0.27, 1.0, 0.12, 0.015),
            side("blk_straight_l1 run-out", 0, 0.39, 1.0, 0.12, 0.015),
        ],
        start.y,
    );
}

#[test]
fn drop_pillar_and_tower_pass_a_marble_down() {
    // Drop hole on layer 2, support pillar on layer 1, tower socket block on layer 0: the marble
    // falls through the drop's bottom port, the pillar's bore and the tower's funnel, and leaves by
    // the tower's socket port (collision plane 10 mm inside the -x face, x = 40 mm).
    let layout = vec![
        place("blk_straight_l3", 0, 2, 0, 0, false),
        place("blk_drop_l3", 1, 2, 0, 0, false),
        place("blk_pillar", 1, 1, 0, 0, false),
        place("blk_tower_socket_l1", 1, 0, 0, 0, false),
    ];
    let start = Vector::new(-0.02, rest(2, 3), 0.0);
    let tr = run(&layout, start, Vector::X * V0, 1_200);
    check_exits(
        &tr,
        &[
            bottom("blk_drop_l3 bottom", 0.06, 0.12, 0.0),
            bottom("blk_pillar bottom", 0.06, 0.06, 0.0),
            landed(side(
                "blk_tower_socket_l1 socket (-x)",
                0,
                0.04,
                -1.0,
                0.0,
                0.015,
            )),
        ],
        start.y,
    );
}

#[test]
fn drop_l2_follows_the_gentle_slope() {
    let layout = vec![
        place("blk_straight_l3", 0, 1, 0, 0, false),
        place("blk_slope_gentle_l3", 1, 1, 0, 0, false),
        place("blk_drop_l2", 2, 1, 0, 0, false),
    ];
    let start = Vector::new(-0.02, rest(1, 3), 0.0);
    let tr = run(&layout, start, Vector::X * V0, 900);
    check_exits(
        &tr,
        &[bottom("blk_drop_l2 bottom", 0.12, 0.06, 0.0)],
        start.y,
    );
}

#[test]
fn redirect_drops_one_layer_and_exits_sideways() {
    // L3: straight in on layer 1, out on layer 0 at L3 into a straight.
    let layout = vec![
        place("blk_straight_l3", 0, 1, 0, 0, false),
        place("blk_redirect_l3", 1, 1, 0, 0, false),
        place("blk_straight_l3", 2, 0, 0, 0, false),
        place("blk_straight_l3", 3, 0, 0, 0, false),
    ];
    let start = Vector::new(-0.02, rest(1, 3), 0.0);
    let tr = run(&layout, start, Vector::X * V0, 1_200);
    check_exits(
        &tr,
        &[
            landed(side(
                "blk_redirect_l3 +x (layer 0, L3)",
                0,
                0.09,
                1.0,
                0.0,
                0.045,
            )),
            landed(side("blk_straight_l3 run-out", 0, 0.21, 1.0, 0.0, 0.045)),
        ],
        start.y,
    );
    // L2, after the gentle slope's L2 exit.
    let layout = vec![
        place("blk_straight_l3", 0, 1, 0, 0, false),
        place("blk_slope_gentle_l3", 1, 1, 0, 0, false),
        place("blk_redirect_l2", 2, 1, 0, 0, false),
        place("blk_straight_l2", 3, 0, 0, 0, false),
        place("blk_straight_l2", 4, 0, 0, 0, false),
    ];
    let tr = run(&layout, start, Vector::X * V0, 1_200);
    check_exits(
        &tr,
        &[
            landed(side(
                "blk_redirect_l2 +x (layer 0, L2)",
                0,
                0.15,
                1.0,
                0.0,
                0.030,
            )),
            landed(side("blk_straight_l2 run-out", 0, 0.27, 1.0, 0.0, 0.030)),
        ],
        start.y,
    );
}

/// Joiner at (1, 0, 0): run-out straights east of it, a straight into port 0 (-x) and one into
/// port 1 (-z) along +z.
fn joiner(level: u8) -> Vec<Placement> {
    let (j, s) = match level {
        3 => ("blk_join_l3", "blk_straight_l3"),
        _ => ("blk_join_l2", "blk_straight_l2"),
    };
    vec![
        place(s, 0, 0, 0, 0, false),
        place(j, 1, 0, 0, 0, false),
        place(s, 1, 0, -1, 1, false),
        place(s, 2, 0, 0, 0, false),
        place(s, 3, 0, 0, 0, false),
    ]
}

#[test]
fn joiner_merges_both_inlets_into_its_outlet() {
    for level in [3_u8, 2] {
        let floor = 0.015 * f32::from(level);
        let layout = joiner(level);
        for (inlet, at, vel) in [
            (
                "port 0 (-x)",
                Vector::new(-0.02, floor + R, 0.0),
                Vector::X * V0,
            ),
            (
                "port 1 (-z)",
                Vector::new(0.06, floor + R, -0.08),
                Vector::Z * V0,
            ),
        ] {
            eprintln!("blk_join_l{level} from {inlet}:");
            let tr = run(&layout, at, vel, 1_000);
            check_exits(
                &tr,
                &[
                    side("joiner +x", 0, 0.09, 1.0, 0.0, floor),
                    side("run-out", 0, 0.21, 1.0, 0.0, floor),
                ],
                at.y,
            );
        }
    }
}
