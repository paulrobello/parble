//! The playroom marble run: a U-shaped run of shipped kit pieces standing on the rug, with a few
//! P5-T1 marbles rolling through it on the P1-T3 chunk colliders and coming to rest at the far end.
//!
//! The run uses `blk_straight_l3`, `blk_curve90_l3` and `blk_end_l3`, every port at `L3` of layer 0,
//! so every link mates (contract §3 to §5). `blk_slope_gentle_l3` is not in it: its exit is `L2`
//! (absolute floor 30 mm) and every other shipped port is `L3` (45 mm), so no shipped piece can
//! take a marble from a slope without a level mismatch (the `blk_*_l2` pieces of contract §7 are
//! not generated yet). With no slope there is no gravity drive, so the marbles start rolling at
//! [`LAUNCH_SPEED_M_S`], the stand-in for the hopper kick that arrives with mechanisms (P2). Both
//! ends are `blk_end_l3` stops, so a rebound cannot leave the run.
//!
//! The world has no tray: the run, plus the open floor ([`scene::floor_collider`]) under it.

use parble_core::{CellCoord, Mirror, PieceId, Placement, Track, Yaw4};
use rapier3d::prelude::*;

use crate::marble::{MarbleKind, MarbleSize};
use crate::{Sim, hash, scene};

/// The run's pieces in marble order: id, anchor cell, yaw in quarter turns. Layer 0 stands on the
/// floor (y = 0). The run goes +X along row z = [`ROW_NEAR`], turns twice through the far row and
/// comes back −X to the end stop.
pub const RUN: [(&str, [i32; 3], u8); 18] = [
    // Start cap: closed end on −X, open port facing the run (+X).
    ("blk_end_l3", [0, 0, ROW_NEAR], 2),
    ("blk_straight_l3", [1, 0, ROW_NEAR], 0),
    ("blk_straight_l3", [2, 0, ROW_NEAR], 0),
    ("blk_straight_l3", [3, 0, ROW_NEAR], 0),
    ("blk_straight_l3", [4, 0, ROW_NEAR], 0),
    ("blk_straight_l3", [5, 0, ROW_NEAR], 0),
    ("blk_straight_l3", [6, 0, ROW_NEAR], 0),
    // −X in, −Z out.
    ("blk_curve90_l3", [7, 0, ROW_NEAR], 0),
    ("blk_straight_l3", [7, 0, ROW_NEAR - 1], 1),
    ("blk_straight_l3", [7, 0, ROW_NEAR - 2], 1),
    // +Z in, −X out.
    ("blk_curve90_l3", [7, 0, ROW_FAR], 1),
    ("blk_straight_l3", [6, 0, ROW_FAR], 0),
    ("blk_straight_l3", [5, 0, ROW_FAR], 0),
    ("blk_straight_l3", [4, 0, ROW_FAR], 0),
    ("blk_straight_l3", [3, 0, ROW_FAR], 0),
    ("blk_straight_l3", [2, 0, ROW_FAR], 0),
    ("blk_straight_l3", [1, 0, ROW_FAR], 0),
    // Finish stop: its open port faces the incoming marbles (+X).
    ("blk_end_l3", [0, 0, ROW_FAR], 2),
];

/// Cell rows of the run's two long legs.
pub const ROW_NEAR: i32 = 0;
pub const ROW_FAR: i32 = -3;

/// The marbles, in spawn order: a few P5-T1 types at the standard size.
pub const MARBLES: [(MarbleKind, MarbleSize); 4] = [
    (MarbleKind::Swirl, MarbleSize::Standard),
    (MarbleKind::Opal, MarbleSize::Standard),
    (MarbleKind::Onionskin, MarbleSize::Standard),
    (MarbleKind::Glow, MarbleSize::Standard),
];

/// Starting speed along +X, m/s, rolling without slip.
pub const LAUNCH_SPEED_M_S: f32 = 0.34;
/// Channel floor of the run (`L3` of layer 0), m.
pub const FLOOR_M: f32 = 0.045;

/// The run as kit placements.
pub fn placements() -> Vec<Placement> {
    RUN.iter()
        .map(|&(id, [x, y, z], yaw)| {
            Placement::new(
                PieceId::new(id).expect("run ids follow the id grammar"),
                CellCoord::new(x, y, z),
                Yaw4::new(yaw),
                Mirror::Off,
            )
        })
        .collect()
}

/// The run as a track (seed 0, the default hopper config), for [`parble_core::validate_track`].
pub fn track() -> Track {
    let mut t = Track::new(0);
    t.placements = placements();
    t
}

/// Start position of marble `i`: on the channel floor of straight `i + 1` of the near leg.
pub fn marble_start_m(i: usize, radius: f32) -> Vector {
    let cell = scene::cell_m();
    Vector::new(
        (i as f32 + 1.0) * cell,
        FLOOR_M + radius + 0.0005,
        ROW_NEAR as f32 * cell,
    )
}

impl Sim {
    /// The playroom run with its [`MARBLES`] launched along the near leg at tick 0.
    pub fn playroom_scene() -> Self {
        let kit = parble_kit::builtin();
        let mut sim = Self::empty();
        sim.world.colliders.insert(scene::floor_collider());
        for p in placements() {
            sim.place_piece(kit, p)
                .expect("the playroom run does not overlap");
        }
        sim.rebuild_chunks(kit)
            .expect("the playroom run builds its chunk colliders");
        for (i, (kind, size)) in MARBLES.into_iter().enumerate() {
            let spec = kind.spec_sized(size);
            sim.add_marble(spec, marble_start_m(i, spec.radius_m));
            let h = *sim.marbles.last().expect("just added");
            let body = &mut sim.world.bodies[h];
            body.set_linvel(Vector::new(LAUNCH_SPEED_M_S, 0.0, 0.0), true);
            body.set_angvel(
                Vector::new(0.0, 0.0, -LAUNCH_SPEED_M_S / spec.radius_m),
                true,
            );
        }
        sim.sample();
        sim
    }
}

/// Ticks in one playroom run (4 s): the marbles reach the finish at about 2.75 s and the run
/// restarts while the train rests there. A train left longer drifts back up the far leg: the
/// marbles rebound off the stop and off each other, and two of them leave at 4–9 mm/s, slowing
/// only slowly because the rolling-resistance damping is matched at 0.5 m/s ([`crate::marble`]).
/// (Before 2026-11-07 they also gained energy from the bend contact, see
/// [`crate::chunk::ChannelBend`].)
pub const PLAYROOM_TICKS: u32 = 960;

/// The committed hash of a [`PLAYROOM_TICKS`] playroom run (`tests/golden/playroom_4s.hash`).
pub fn playroom_4s_hash() -> u64 {
    u64::from_str_radix(include_str!("../tests/golden/playroom_4s.hash").trim(), 16)
        .expect("playroom_4s.hash is 16 hex digits")
}

/// Run the playroom scene for [`PLAYROOM_TICKS`] and return its hash as 16 hex digits.
pub fn playroom_4s_hash_hex() -> String {
    let mut sim = Sim::playroom_scene();
    sim.tick(PLAYROOM_TICKS);
    hash::format_hash(sim.trajectory_hash())
}

#[cfg(test)]
mod tests {
    use super::*;
    use parble_core::{IssueKind, validate_track};

    #[test]
    fn every_link_mates_and_only_the_missing_mechanisms_are_reported() {
        let issues = validate_track(&track(), parble_kit::builtin());
        let kinds: Vec<_> = issues.iter().map(|i| &i.kind).collect();
        assert_eq!(
            kinds,
            [&IssueKind::MissingHopper, &IssueKind::MissingFinish],
            "{issues:#?}"
        );
    }

    #[test]
    fn the_shipped_slope_cannot_feed_any_shipped_piece() {
        for next in ["blk_straight_l3", "blk_curve90_l3", "blk_end_l3"] {
            let mut t = Track::new(0);
            for (id, x) in [("blk_slope_gentle_l3", 0), (next, 1)] {
                t.placements.push(Placement::new(
                    PieceId::new(id).unwrap(),
                    CellCoord::new(x, 0, 0),
                    Yaw4::new(0),
                    Mirror::Off,
                ));
            }
            let issues = validate_track(&t, parble_kit::builtin());
            assert!(
                issues
                    .iter()
                    .any(|i| matches!(i.kind, IssueKind::BrokenLink { .. })),
                "slope → {next}: {issues:#?}"
            );
        }
    }

    #[test]
    fn marbles_run_the_whole_track_and_gather_at_the_finish() {
        let mut sim = Sim::playroom_scene();
        let n = MARBLES.len();
        let cell = scene::cell_m();
        let mut reached_far = vec![false; n];
        let mut last_second = Vec::new();
        for t in 1..=PLAYROOM_TICKS {
            sim.tick(1);
            let ms = sim.marbles();
            for (i, m) in ms.iter().enumerate() {
                reached_far[i] |= m.position[2] < (ROW_FAR as f32 + 0.5) * cell;
                assert!(
                    m.position[1] > FLOOR_M,
                    "marble {i} left the channel at tick {t}: {:?}",
                    m.position
                );
            }
            if t + crate::TICK_HZ > PLAYROOM_TICKS {
                last_second.push(ms);
            }
        }
        for i in 0..n {
            assert!(reached_far[i], "marble {i} never reached the far leg");
            let first = last_second[0][i].position;
            let [x, _, z] = sim.marbles()[i].position;
            // The train rests against the finish stop: every marble within its first two cells.
            assert!(
                x < 1.5 * cell && (z - ROW_FAR as f32 * cell).abs() < 0.01,
                "marble {i} is not at the finish: {:?}",
                sim.marbles()[i].position
            );
            let drift = (x - first[0]).abs() + (z - first[2]).abs();
            assert!(
                drift < 0.03,
                "marble {i} moved {drift} m in the last second"
            );
        }
    }

    #[test]
    fn playroom_run_matches_its_committed_hash() {
        // Any change to the chunk colliders or marble physics moves this hash. Regenerate with
        // `cargo test -p parble-sim --lib print_playroom_hash -- --ignored --nocapture` and write
        // the printed value to crates/parble-sim/tests/golden/playroom_4s.hash.
        assert_eq!(
            playroom_4s_hash_hex(),
            hash::format_hash(playroom_4s_hash()),
            "playroom run hash moved; see the comment above to regenerate"
        );
    }

    #[test]
    #[ignore = "prints the playroom hash for regenerating the committed file"]
    fn print_playroom_hash() {
        println!("{}", playroom_4s_hash_hex());
    }
}
