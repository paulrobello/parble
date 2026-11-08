//! The mechanism showcase (P2-T6): two short lanes side by side behind the tray, each feeding one
//! cat's eye into a P2-T5 mechanism, so the renderer has parts that move on a fixed schedule.
//!
//! - Toggle lane: an L3 straight into `mech_toggle_l3` (the rig of the P2-T5 toggle test) with
//!   `first = 1`. The rocker sets side 1 at tick 0, the marble leaves by the −Z branch (away from
//!   the tray), and [`crate::behaviour::FORK_SETTLE_TICKS`] later the rocker swings to side 0.
//! - Gate lane: L1 straight, `mech_gate_oneway_l1`, L1 straight (the rig of the one-way gate test).
//!   The marble pushes the hinged flap open from the in port, rolls under it, and the flap falls
//!   shut behind it ([`crate::behaviour::GATE_FLAP`]).
//!
//! - Lift lane: an L3 straight into `mech_stepper_up1_l3`, and an L3 straight on its exit one
//!   layer up. The marble climbs the slats ([`crate::stepper`]) and the slats ease back to rest.
//!
//! All three marbles leave their lane onto the open floor ([`scene::floor_collider`]). The row
//! sits behind the tray (−Z), so the plain environment's tray never overlaps it.

use parble_core::{CellCoord, Mirror, PieceId, Placement, Yaw4};
use rapier3d::prelude::*;

use crate::marble::CATS_EYE;
use crate::{Sim, behaviour, scene};

/// Cell row of both lanes.
pub const ROW: i32 = -5;
/// First cell (x) of the toggle lane and of the gate lane.
pub const TOGGLE_X: i32 = 0;
pub const GATE_X: i32 = 3;
/// First cell (x) of the lift lane.
pub const LIFT_X: i32 = 7;

/// The showcase's pieces: id, anchor cell, yaw in quarter turns.
pub const LAYOUT: [(&str, [i32; 3], u8); 8] = [
    ("blk_straight_l3", [TOGGLE_X, 0, ROW], 0),
    (behaviour::TOGGLE, [TOGGLE_X + 1, 0, ROW], 0),
    ("blk_straight_l1", [GATE_X, 0, ROW], 0),
    (behaviour::GATE, [GATE_X + 1, 0, ROW], 0),
    ("blk_straight_l1", [GATE_X + 2, 0, ROW], 0),
    ("blk_straight_l3", [LIFT_X, 0, ROW], 0),
    (crate::stepper::STEPPER, [LIFT_X + 1, 0, ROW], 0),
    ("blk_straight_l3", [LIFT_X + 3, 1, ROW], 0),
];

/// The toggle's anchor cell (its mechanism instance id) and the gate's.
pub const TOGGLE_CELL: [i32; 3] = [TOGGLE_X + 1, 0, ROW];
pub const GATE_CELL: [i32; 3] = [GATE_X + 1, 0, ROW];
pub const LIFT_CELL: [i32; 3] = [LIFT_X + 1, 0, ROW];

/// Launch speeds along +X, m/s, those of the P2-T5 fork and one-way rigs.
pub const TOGGLE_LAUNCH_M_S: f32 = 0.6;
pub const GATE_LAUNCH_M_S: f32 = 0.3;
pub const LIFT_LAUNCH_M_S: f32 = 0.3;

/// Ticks in one showcase run before it restarts (10 s).
pub const SHOWCASE_TICKS: u32 = crate::golden::RAMP_10S_TICKS;

/// The `shots-test` capture ticks, multiples of the app's 4-tick lockstep frame: the toggle's
/// rocker part-way through its swing to side 0, and the gate's flap pushed well open (past 30°) with
/// the marble under it. The tests below pin both against the sim.
pub const TOGGLE_FLIP_SHOT_TICK: u64 = 60;
pub const GATE_OPEN_SHOT_TICK: u64 = 72;
/// The lift's slats part-way up their second stroke, carrying the marble (pinned below).
pub const LIFT_SHOT_TICK: u64 = 392;

/// The showcase as kit placements.
pub fn placements() -> Vec<Placement> {
    LAYOUT
        .iter()
        .map(|&(id, [x, y, z], yaw)| {
            let mut p = Placement::new(
                PieceId::new(id).expect("showcase ids follow the id grammar"),
                CellCoord::new(x, y, z),
                Yaw4::new(yaw),
                Mirror::Off,
            );
            if id == behaviour::TOGGLE {
                p.config.insert("first".into(), 1);
            }
            p
        })
        .collect()
}

/// Rest centre height of a cat's eye on a channel floor of `level` (layer 0), m.
fn rest_m(level: i32) -> f32 {
    0.015 * level as f32 + CATS_EYE.radius_m
}

impl Sim {
    /// The showcase: its pieces as chunk colliders, a kit behaviour on each mechanism (seed 0),
    /// and one cat's eye launched into each lane at tick 0.
    pub fn mech_showcase_scene() -> Self {
        let kit = parble_kit::builtin();
        let reg = behaviour::registry();
        let mut sim = Self::empty();
        sim.world.colliders.insert(scene::floor_collider());
        let layout = placements();
        for p in &layout {
            sim.place_piece(kit, p.clone())
                .expect("the showcase does not overlap");
        }
        sim.rebuild_chunks(kit)
            .expect("the showcase builds its chunk colliders");
        for p in &layout {
            sim.add_registered(&reg, kit, p, 0)
                .expect("the showcase mechanisms build");
        }
        let cell = scene::cell_m();
        for (x, level, speed) in [
            (TOGGLE_X, 3, TOGGLE_LAUNCH_M_S),
            (GATE_X, 1, GATE_LAUNCH_M_S),
            (LIFT_X, 3, LIFT_LAUNCH_M_S),
        ] {
            sim.add_marble(
                CATS_EYE,
                Vector::new(x as f32 * cell - 0.02, rest_m(level), ROW as f32 * cell),
            );
            let h = *sim.marbles.last().expect("just added");
            let body = &mut sim.world.bodies[h];
            body.set_linvel(Vector::new(speed, 0.0, 0.0), true);
            body.set_angvel(Vector::new(0.0, 0.0, -speed / CATS_EYE.radius_m), true);
        }
        sim.sample();
        sim
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(sim: &Sim, cell: [i32; 3]) -> i32 {
        let id = CellCoord::new(cell[0], cell[1], cell[2]);
        sim.mechanisms()
            .into_iter()
            .find(|m| m.id.0 == id)
            .expect("instance")
            .part_angles[0]
    }

    fn limits(piece: &str) -> [i32; 2] {
        parble_kit::builtin()
            .piece(&PieceId::new(piece).unwrap())
            .unwrap()
            .animated_parts[0]
            .limits_mdeg
    }

    #[test]
    fn shot_ticks_catch_the_toggle_mid_flip_and_the_gate_open() {
        assert_eq!(TOGGLE_FLIP_SHOT_TICK % 4, 0);
        assert_eq!(GATE_OPEN_SHOT_TICK % 4, 0);
        let [lo, hi] = limits(behaviour::TOGGLE);
        let mut sim = Sim::mech_showcase_scene();
        let (mut toggle, mut gate, mut marbles) = (Vec::new(), Vec::new(), Vec::new());
        for _ in 0..SHOWCASE_TICKS {
            toggle.push(part(&sim, TOGGLE_CELL));
            gate.push(part(&sim, GATE_CELL));
            marbles.push(sim.marbles());
            sim.tick(1);
        }
        // Side 1 by tick 24, then one swing to side 0 after the marble left.
        assert_eq!(toggle[24], hi);
        assert_eq!(*toggle.last().unwrap(), lo);
        let swings = toggle.windows(2).filter(|w| w[0] != w[1]).count();
        let t = TOGGLE_FLIP_SHOT_TICK as usize;
        let travel = (hi - toggle[t]) as f32 / (hi - lo) as f32;
        assert!(
            (0.25..=0.75).contains(&travel),
            "toggle at tick {t}: {} mdeg, {travel} of its swing",
            toggle[t]
        );
        assert!(toggle[t - 1] > toggle[t] && toggle[t] > toggle[t + 1]);
        assert!(swings < 24, "the rocker swung on {swings} ticks");
        // The toggle marble left by the −Z branch, away from the tray.
        let cell = scene::cell_m();
        let gone = marbles
            .iter()
            .position(|m| m[0].position[2] < (ROW as f32 - 0.5) * cell);
        assert!(gone.is_some_and(|g| g < t), "toggle marble: {gone:?}");

        let g = GATE_OPEN_SHOT_TICK as usize;
        assert_eq!(gate[0], 0, "the flap starts shut");
        assert!(gate[g] >= 30_000, "flap not open at {g}: {} mdeg", gate[g]);
        let flap_x = (GATE_X + 1) as f32 * cell;
        assert!(
            (marbles[g][1].position[0] - flap_x).abs() < 0.5 * cell,
            "gate marble at {:?}",
            marbles[g][1].position
        );
        // The flap falls shut again once the marble is through.
        assert!(
            libm::fabsf(*gate.last().unwrap() as f32) <= behaviour::GATE_CLOSED_MDEG as f32,
            "the flap closes again: {} mdeg",
            gate.last().unwrap()
        );
        assert!(
            marbles.last().unwrap()[1].position[0] > (GATE_X as f32 + 2.5) * cell,
            "the gate marble stayed in its lane"
        );
    }
    #[test]
    fn lift_shot_tick_catches_the_slats_carrying_the_marble_up() {
        assert_eq!(LIFT_SHOT_TICK % 4, 0);
        let stroke = parble_kit::builtin()
            .piece(&PieceId::new(crate::stepper::STEPPER).unwrap())
            .unwrap()
            .animated_parts[0]
            .limits_mdeg[1];
        let mut sim = Sim::mech_showcase_scene();
        let (mut slats, mut marbles) = (Vec::new(), Vec::new());
        for _ in 0..SHOWCASE_TICKS {
            slats.push(part(&sim, LIFT_CELL));
            marbles.push(sim.marbles()[2].position);
            sim.tick(1);
        }
        assert_eq!(slats[0], 0, "the slats start at rest");
        let t = LIFT_SHOT_TICK as usize;
        let travel = slats[t] as f32 / stroke as f32;
        assert!(
            (0.25..=0.75).contains(&travel),
            "slats at tick {t}: {} um, {travel} of the stroke",
            slats[t]
        );
        assert!(slats[t - 1] < slats[t] && slats[t] < slats[t + 1], "rising");
        let cell = scene::cell_m();
        let [x, y, _] = marbles[t];
        assert!(
            ((LIFT_CELL[0] as f32 - 0.5) * cell..(LIFT_CELL[0] as f32 + 1.5) * cell).contains(&x),
            "lift marble at {:?}",
            marbles[t]
        );
        assert!(y > rest_m(3) + 0.01, "the marble is already a step up: {y}");
        // The marble leaves by the high port and the slats ease back to rest.
        let out = marbles
            .iter()
            .position(|m| m[0] > (LIFT_X + 3) as f32 * cell && m[1] > rest_m(3) + 0.03);
        assert!(out.is_some(), "the lift marble reached the high straight");
        assert_eq!(*slats.last().unwrap(), 0, "the slats rest again");
    }
}
