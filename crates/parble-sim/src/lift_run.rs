//! The stepper elevators' runs.
//!
//! - [`rig`]: an L3 straight into `mech_stepper_up1_l3` and an L3 straight on its exit one cell
//!   layer up, with scripted launches ([`ten_marbles`]).
//! - [`track`]: a hopper-to-finish track whose only ways up are the two steppers, every port mated:
//!   hopper (`L1`) → L1 straight → `mech_stepper_up1_l1` (`L1` in, `L3` out one layer up) → L3
//!   straight → `mech_stepper_up1_l3` → L3 straight → toggle → two finish cups, climbing two
//!   layers. Its
//!   trajectory hash is the committed golden `tests/golden/lift_track.hash`, checked natively and
//!   by the wasm parity check (`make determinism-wasm`).

use parble_core::{
    CellCoord, HopperConfig, MarbleSize, Mirror, PieceId, Placement, ReleaseMode, Track, Yaw4,
};
use rapier3d::prelude::*;

use crate::marble::{MarbleKind, MarbleSpec};
use crate::stepper::{STEPPER, STEPPER_L1};
use crate::track_run::{TrackRun, TrackRunError};
use crate::{Sim, behaviour, hash};

const CELL: f32 = 0.06;
/// Marbles in [`ten_marbles`].
pub const MARBLES: usize = 10;
/// Ticks between launches (1.5 s): later marbles arrive while earlier ones still climb.
pub const INTERVAL_TICKS: u32 = 360;
/// Ticks in the run: the last launch plus 10 s.
pub const RUN_TICKS: u32 = MARBLES as u32 * INTERVAL_TICKS + 2_400;
/// Entry speed of every launch, m/s.
pub const LAUNCH_M_S: f32 = 0.3;

fn place(piece: &str, x: i32, y: i32, z: i32) -> Placement {
    place_yaw(piece, x, y, z, 0)
}

fn place_yaw(piece: &str, x: i32, y: i32, z: i32, yaw: u8) -> Placement {
    Placement::new(
        PieceId::new(piece).expect("rig ids follow the id grammar"),
        CellCoord::new(x, y, z),
        Yaw4::new(yaw),
        Mirror::Off,
    )
}

/// An L3 straight into the stepper at the origin, and an L3 straight on its exit one layer up.
pub fn layout() -> Vec<Placement> {
    vec![
        place("blk_straight_l3", -1, 0, 0),
        place(STEPPER, 0, 0, 0),
        place("blk_straight_l3", 2, 1, 0),
    ]
}

/// [`layout`] as chunk colliders with the stepper's kit behaviour (seed 0), events recorded.
pub fn rig() -> Sim {
    let kit = parble_kit::builtin();
    let reg = behaviour::registry();
    let mut sim = Sim::empty();
    let layout = layout();
    for p in &layout {
        sim.place_piece(kit, p.clone())
            .expect("the rig does not overlap");
    }
    sim.rebuild_chunks(kit)
        .expect("the rig builds its chunk colliders");
    for p in &layout {
        if let Some(f) = reg.get(&p.piece) {
            sim.add_mechanism(kit.piece(&p.piece).expect("kit piece"), p, 0, f(&p.config))
                .expect("the stepper behaviour builds");
        }
    }
    sim.set_event_recording(true);
    sim
}

/// A marble rolling along +X on the entry straight at `speed`, `dz` off the centreline.
pub fn launch(sim: &mut Sim, spec: MarbleSpec, speed: f32, dz: f32) -> RigidBodyHandle {
    let at = Vector::new(-CELL - 0.015, 0.045 + spec.radius_m, dz);
    let (mut body, collider) = spec.build(at);
    body.set_linvel(Vector::X * speed, true);
    body.set_angvel(Vector::new(0.0, 0.0, -speed / spec.radius_m), true);
    let h = sim.world.bodies.insert(body);
    sim.world
        .colliders
        .insert_with_parent(collider, h, &mut sim.world.bodies);
    sim.marbles.push(h);
    h
}

/// One tick of the 10-marble run at `t` (0-based): launches marble `t / INTERVAL_TICKS` of every
/// standard kind in turn on each interval, then ticks the sim.
pub fn ten_marbles_tick(sim: &mut Sim, t: u32) {
    if t.is_multiple_of(INTERVAL_TICKS) && (t / INTERVAL_TICKS) < MARBLES as u32 {
        let kind = MarbleKind::ALL[(t / INTERVAL_TICKS) as usize % MarbleKind::ALL.len()];
        launch(sim, kind.spec(), LAUNCH_M_S, 0.0);
    }
    sim.tick(1);
}

/// [`rig`] after the whole 10-marble run, [`RUN_TICKS`] ticks.
pub fn ten_marbles() -> Sim {
    let mut sim = rig();
    for t in 0..RUN_TICKS {
        ten_marbles_tick(&mut sim, t);
    }
    sim
}

/// [`track`]'s pieces in marble order: id, anchor cell (row z = −6, behind the tray), no turns.
pub const TRACK_LAYOUT: [(&str, [i32; 3], u8); 9] = [
    (behaviour::HOPPER, [0, 1, -6], 0),
    ("blk_straight_l1", [1, 1, -6], 0),
    // Footprint x 2..3, y 0..2: in at L1 in layer 1, out at L3 in layer 2.
    (STEPPER_L1, [2, 1, -6], 0),
    ("blk_straight_l3", [4, 2, -6], 0),
    // Footprint x 5..6, y 2..3: in at L3 in layer 2, out at L3 in layer 3.
    (STEPPER, [5, 2, -6], 0),
    ("blk_straight_l3", [7, 3, -6], 0),
    // The toggle alternates the marbles between two cups, five each, as the all-pieces track
    // ends: one cup piles ten too high, and the last two queue at its lip outside the sensor.
    ("mech_toggle_l3", [8, 3, -6], 0),
    (behaviour::FINISH, [8, 3, -5], 3),
    (behaviour::FINISH, [8, 3, -7], 1),
];
/// Track seed.
pub const TRACK_SEED: u64 = 3;
/// Marbles in the hopper queue: the two finish cups count 5 each.
pub const TRACK_MARBLES: usize = 10;
/// Release interval, ticks (2 s).
pub const TRACK_INTERVAL_TICKS: u32 = 480;
/// Ticks in one run of [`track`] (30 s): the tenth marble reaches the cup at about 27 s.
pub const TRACK_TICKS: u32 = 7_200;

/// [`TRACK_LAYOUT`] as kit placements.
pub fn track_placements() -> Vec<Placement> {
    TRACK_LAYOUT
        .iter()
        .map(|&(id, [x, y, z], yaw)| place_yaw(id, x, y, z, yaw))
        .collect()
}

/// The lift track: [`TRACK_LAYOUT`], [`TRACK_SEED`], and [`TRACK_MARBLES`] standard marbles
/// released from the hopper every [`TRACK_INTERVAL_TICKS`].
pub fn track() -> Track {
    let mut t = Track::new(TRACK_SEED);
    t.placements = track_placements();
    t.hopper = HopperConfig {
        placement: Some(0),
        queue: vec![MarbleSize::Standard; TRACK_MARBLES],
        release: ReleaseMode::Interval {
            ticks: TRACK_INTERVAL_TICKS,
        },
    };
    t
}

/// A fresh run of [`track`] at tick 0.
pub fn track_run() -> Result<TrackRun, TrackRunError> {
    TrackRun::new(&track())
}

/// Marbles the finish cups have counted.
pub fn finished(sim: &Sim) -> usize {
    sim.mechanisms()
        .iter()
        .filter(|m| m.piece.as_str() == behaviour::FINISH)
        .map(|m| behaviour::Finish::arrivals(&m.state).len())
        .sum()
}

/// Run [`track`] for [`TRACK_TICKS`] and return its trajectory hash as 16 hex digits, the format
/// of `tests/golden/lift_track.hash` (without the newline).
pub fn lift_track_hash_hex() -> String {
    let mut run = track_run().expect("the lift track runs");
    run.tick(TRACK_TICKS);
    hash::format_hash(run.trajectory_hash())
}

#[cfg(test)]
mod tests {
    use super::*;
    use parble_core::{IssueKind, validate_track};

    /// Every port of the track is mated, hopper to finish: the steppers are real port links.
    #[test]
    fn the_lift_track_validates_with_every_port_mated() {
        let issues = validate_track(&track(), parble_kit::builtin());
        assert!(
            !issues.iter().any(|i| matches!(
                i.kind,
                IssueKind::OpenPort { .. }
                    | IssueKind::BrokenLink { .. }
                    | IssueKind::FinishUnreachable
            )),
            "{issues:#?}"
        );
        assert!(issues.is_empty(), "{issues:#?}");
    }

    /// All ten hopper marbles climb both steppers, two layers in all, into the finish cups.
    #[test]
    fn every_hopper_marble_climbs_both_steppers_into_the_finish() {
        let mut run = track_run().unwrap();
        let mut done_at = None;
        for t in 1..=TRACK_TICKS {
            run.tick(1);
            for (m, s) in run.marbles().iter().enumerate() {
                assert!(
                    s.position[1] > 0.0,
                    "marble {m} fell at tick {t}: {:?}",
                    s.position
                );
            }
            if done_at.is_none() && finished(&run) == TRACK_MARBLES {
                done_at = Some(t);
            }
        }
        assert_eq!(
            run.marbles().len(),
            TRACK_MARBLES,
            "the hopper released them all"
        );
        assert_eq!(finished(&run), TRACK_MARBLES);
        let cup_floor = 3.0 * CELL;
        for (m, s) in run.marbles().iter().enumerate() {
            assert!(s.position[1] > cup_floor, "marble {m} at {:?}", s.position);
        }
        eprintln!(
            "all {TRACK_MARBLES} marbles in the cups by tick {}",
            done_at.unwrap()
        );
    }

    #[test]
    fn the_lift_track_run_repeats() {
        assert_eq!(TRACK_TICKS % 4, 0);
        assert_eq!(lift_track_hash_hex(), lift_track_hash_hex());
    }
}
