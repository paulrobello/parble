//! The mechanism showcase (P2-T6): short lanes behind the tray, each feeding one cat's eye into a
//! kit mechanism, so the renderer has parts that move on a fixed schedule.
//!
//! - Toggle lane: an L3 straight into `mech_toggle_l3` (the rig of the P2-T5 toggle test) with
//!   `first = 1`. The rocker sets side 1 at tick 0, the marble leaves by the −Z branch (away from
//!   the tray), and [`crate::behaviour::FORK_SETTLE_TICKS`] later the rocker swings to side 0.
//! - Gate lane: L1 straight, `mech_gate_oneway_l1`, L1 straight (the rig of the one-way gate test).
//!   The marble pushes the hinged flap open from the in port, rolls under it, and the flap falls
//!   shut behind it ([`crate::behaviour::GATE_FLAP`]).
//! - Rope lane: three L1 straights, `mech_arch_rope_l1`, two L1 straights, running along −Z in
//!   column [`ROPE_X`], left of the toggle lane. The marble rolls away from the camera's rope
//!   close-up (`Cues::mechanisms` cuts to it at [`ROPE_CUT_TICK`]), loses half its speed under
//!   the arch, and both rope rows swing away from it. The column is in the chunk left of the other
//!   lanes' (cells x = −8..−1), so their chunk collider is unchanged.
//!
//! - Lift lane: an L3 straight into `mech_stepper_up1_l3`, and an L3 straight on its exit one
//!   layer up. The marble climbs the slats ([`crate::stepper`]) and the slats ease back to rest.
//!
//! - Checkpoint lane (eight rows behind the toggle and gate row, out of its set shot): L1
//!   straight, `mech_checkpoint_l1`, L1 straight. The marble rolls through the arch and lights
//!   its glow inlay ([`crate::inlay`]).
//!
//! Every marble leaves its lane onto the open floor ([`scene::floor_collider`]). The rows sit
//! behind the tray (−Z), so the plain environment's tray never overlaps them.

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

/// Column (x) of the rope lane, its first cell (z) and its arch's cell (z). It runs along −Z.
pub const ROPE_X: i32 = -3;
pub const ROPE_Z: i32 = -1;
pub const ROPE_ARCH_Z: i32 = -4;

/// Row and first cell (x) of the checkpoint lane: eight rows further from the tray, outside the
/// set shot of the other two (`--camera checkpoint` frames it).
pub const CHECKPOINT_ROW: i32 = ROW - 8;
pub const CHECKPOINT_X: i32 = 3;

/// The showcase's pieces: id, anchor cell, yaw in quarter turns.
pub const LAYOUT: [(&str, [i32; 3], u8); 17] = [
    ("blk_straight_l3", [TOGGLE_X, 0, ROW], 0),
    (behaviour::TOGGLE, [TOGGLE_X + 1, 0, ROW], 0),
    ("blk_straight_l1", [GATE_X, 0, ROW], 0),
    (behaviour::GATE, [GATE_X + 1, 0, ROW], 0),
    ("blk_straight_l1", [GATE_X + 2, 0, ROW], 0),
    ("blk_straight_l3", [LIFT_X, 0, ROW], 0),
    (crate::stepper::STEPPER, [LIFT_X + 1, 0, ROW], 0),
    ("blk_straight_l3", [LIFT_X + 3, 1, ROW], 0),
    ("blk_straight_l1", [ROPE_X, 0, ROPE_Z], 1),
    ("blk_straight_l1", [ROPE_X, 0, ROPE_Z - 1], 1),
    ("blk_straight_l1", [ROPE_X, 0, ROPE_Z - 2], 1),
    (behaviour::ROPE_ARCH, [ROPE_X, 0, ROPE_ARCH_Z], 1),
    ("blk_straight_l1", [ROPE_X, 0, ROPE_ARCH_Z - 1], 1),
    ("blk_straight_l1", [ROPE_X, 0, ROPE_ARCH_Z - 2], 1),
    ("blk_straight_l1", [CHECKPOINT_X, 0, CHECKPOINT_ROW], 0),
    (
        behaviour::CHECKPOINT,
        [CHECKPOINT_X + 1, 0, CHECKPOINT_ROW],
        0,
    ),
    ("blk_straight_l1", [CHECKPOINT_X + 2, 0, CHECKPOINT_ROW], 0),
];

/// The toggle's anchor cell (its mechanism instance id) and the gate's.
pub const TOGGLE_CELL: [i32; 3] = [TOGGLE_X + 1, 0, ROW];
pub const GATE_CELL: [i32; 3] = [GATE_X + 1, 0, ROW];
pub const LIFT_CELL: [i32; 3] = [LIFT_X + 1, 0, ROW];
pub const ROPE_CELL: [i32; 3] = [ROPE_X, 0, ROPE_ARCH_Z];
pub const CHECKPOINT_CELL: [i32; 3] = [CHECKPOINT_X + 1, 0, CHECKPOINT_ROW];

/// Launch speeds, m/s: along +X those of the P2-T5 fork and one-way rigs, the rope lane's along −Z.
pub const TOGGLE_LAUNCH_M_S: f32 = 0.6;
pub const GATE_LAUNCH_M_S: f32 = 0.3;
pub const LIFT_LAUNCH_M_S: f32 = 0.3;
pub const ROPE_LAUNCH_M_S: f32 = 0.4;
pub const CHECKPOINT_LAUNCH_M_S: f32 = 0.3;

/// Ticks in one showcase run before it restarts (10 s).
pub const SHOWCASE_TICKS: u32 = crate::golden::RAMP_10S_TICKS;

/// The `shots-test` capture ticks, multiples of the app's 4-tick lockstep frame: the toggle's
/// rocker part-way through its swing to side 0, the gate's flap pushed well open (past 30°) with
/// the marble under it, and the rope rows swung away from the marble in the curtain (after the
/// camera's cut to the rope close-up at [`ROPE_CUT_TICK`]), and the lift's slats carrying the marble
/// up (after the cut back at [`ROPE_RETURN_TICK`]). The tests below pin each against the sim.
pub const TOGGLE_FLIP_SHOT_TICK: u64 = 60;
pub const GATE_OPEN_SHOT_TICK: u64 = 72;
/// The lift's slats part-way up their second stroke, carrying the marble (pinned below).
pub const LIFT_SHOT_TICK: u64 = 392;
pub const ROPE_SWING_SHOT_TICK: u64 = 128;
/// The director cuts from the overview to the rope close-up here, after the gate shot and before
/// the rope marble reaches the curtain.
pub const ROPE_CUT_TICK: u64 = 96;
/// The director cuts back from the rope close-up to the overview here, after the rope shot
/// and before the lift shot.
pub const ROPE_RETURN_TICK: u64 = 192;
/// Checkpoint inlay shots: dark before its marble reaches the arch, and fully lit (past the
/// render fade) once it has passed.
pub const CHECKPOINT_DARK_SHOT_TICK: u64 = 32;
pub const CHECKPOINT_LIT_SHOT_TICK: u64 = 160;

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
        for (at, level, v) in [
            (
                Vector::new(TOGGLE_X as f32 * cell - 0.02, 0.0, ROW as f32 * cell),
                3,
                Vector::X * TOGGLE_LAUNCH_M_S,
            ),
            (
                Vector::new(GATE_X as f32 * cell - 0.02, 0.0, ROW as f32 * cell),
                1,
                Vector::X * GATE_LAUNCH_M_S,
            ),
            (
                Vector::new(LIFT_X as f32 * cell - 0.02, 0.0, ROW as f32 * cell),
                3,
                Vector::X * LIFT_LAUNCH_M_S,
            ),
            (
                Vector::new(ROPE_X as f32 * cell, 0.0, ROPE_Z as f32 * cell + 0.02),
                1,
                -Vector::Z * ROPE_LAUNCH_M_S,
            ),
            (
                Vector::new(
                    CHECKPOINT_X as f32 * cell - 0.02,
                    0.0,
                    CHECKPOINT_ROW as f32 * cell,
                ),
                1,
                Vector::X * CHECKPOINT_LAUNCH_M_S,
            ),
        ] {
            sim.add_marble(CATS_EYE, at + Vector::Y * rest_m(level));
            let h = *sim.marbles.last().expect("just added");
            let body = &mut sim.world.bodies[h];
            body.set_linvel(v, true);
            // Rolling without slip: ω = ŷ × v / r.
            body.set_angvel(Vector::Y.cross(v) / CATS_EYE.radius_m, true);
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

    #[test]
    fn shot_tick_catches_the_ropes_swung_away_from_the_marble() {
        assert_eq!(ROPE_SWING_SHOT_TICK % 4, 0);
        let id = CellCoord::new(ROPE_CELL[0], ROPE_CELL[1], ROPE_CELL[2]);
        let mut sim = Sim::mech_showcase_scene();
        let (mut ropes, mut marbles) = (Vec::new(), Vec::new());
        for _ in 0..SHOWCASE_TICKS {
            let v = sim.mechanisms().into_iter().find(|m| m.id.0 == id).unwrap();
            ropes.push(v.part_angles);
            marbles.push(sim.marbles()[3]);
            sim.tick(1);
        }
        let t = ROPE_SWING_SHOT_TICK as usize;
        // The near row (ropes 0-3) swung toward +X past 10° at its inner ropes, and the marble is
        // under the arch.
        assert!(
            ropes[t][1] >= 10_000 && ropes[t][2] >= 10_000,
            "ropes at tick {t}: {:?}",
            ropes[t]
        );
        const {
            assert!(ROPE_CUT_TICK.is_multiple_of(4) && ROPE_CUT_TICK > GATE_OPEN_SHOT_TICK);
            assert!(ROPE_RETURN_TICK.is_multiple_of(4));
            assert!(ROPE_SWING_SHOT_TICK < ROPE_RETURN_TICK && ROPE_RETURN_TICK < LIFT_SHOT_TICK);
        };
        let cell = scene::cell_m();
        let arch_z = ROPE_ARCH_Z as f32 * cell;
        assert!(
            (marbles[t].position[2] - arch_z).abs() < 0.02,
            "rope marble at {:?}",
            marbles[t].position
        );
        // Still on its way to the curtain at the cut, so the close-up sees it arrive.
        let c = ROPE_CUT_TICK as usize;
        assert!(
            marbles[c].position[2] - arch_z > 0.02 + 0.012 + 0.008,
            "{:?}",
            marbles[c].position
        );
        assert!(
            ropes[c].iter().all(|&a| a == 0),
            "ropes at the cut: {:?}",
            ropes[c]
        );
        assert!(
            ropes.last().unwrap().iter().all(|&a| a == 0),
            "ropes settle"
        );
        assert!(
            marbles.last().unwrap().position[2] < (ROPE_ARCH_Z as f32 - 2.5) * cell,
            "the rope marble left its lane"
        );
    }

    #[test]
    fn the_checkpoint_inlay_is_dark_then_lit_at_its_shot_ticks() {
        assert_eq!(CHECKPOINT_DARK_SHOT_TICK % 4, 0);
        assert_eq!(CHECKPOINT_LIT_SHOT_TICK % 4, 0);
        let id = CellCoord::new(CHECKPOINT_CELL[0], CHECKPOINT_CELL[1], CHECKPOINT_CELL[2]);
        let inlay = |sim: &Sim| {
            let v = sim.mechanisms().into_iter().find(|m| m.id.0 == id).unwrap();
            crate::inlay::inlays(&v.state)[0]
        };
        let mut sim = Sim::mech_showcase_scene();
        sim.tick(CHECKPOINT_DARK_SHOT_TICK as u32);
        assert!(!inlay(&sim).on);
        sim.tick((CHECKPOINT_LIT_SHOT_TICK - CHECKPOINT_DARK_SHOT_TICK) as u32);
        let lit = inlay(&sim);
        let since = lit.since.expect("lit by a pass");
        assert!(lit.on && since > CHECKPOINT_DARK_SHOT_TICK, "{lit:?}");
        // The render fade (60 ticks) is over by the lit shot.
        assert!(CHECKPOINT_LIT_SHOT_TICK >= since + 60, "lit at {since}");
        // It holds for the rest of the run.
        sim.tick(SHOWCASE_TICKS - CHECKPOINT_LIT_SHOT_TICK as u32);
        assert_eq!(inlay(&sim), lit);
        let m = &sim.marbles()[4];
        assert!(
            m.position[0] > (CHECKPOINT_X as f32 + 2.5) * scene::cell_m(),
            "the checkpoint marble stayed in its lane: {:?}",
            m.position
        );
    }
}
