//! The all-pieces test track (P2-T7): one run through all 16 PRD §6 P2 kit items, with
//! [`MARBLES`] standard marbles released from the hopper, all of them ending in a finish cup.
//!
//! The track runs through [`TrackRun`] like a loaded `.parble` track: the P0 tray (the track stands
//! behind it, rows z = −4…−7) plus the placements as chunk colliders and the kit behaviours, rooted
//! at [`SEED`].
//!
//! # Route
//!
//! Hopper (layer 7) → L3 straight → redirect → splitter (both branches rejoin at a joiner) →
//! redirect → toggle (rejoined) → steep slope → L1 straight → L3 straight → gentle slope → L2 rail
//! straight → S-rail → rail curve → L2 block curve → drop hole → tower socket → rail joint → L1 rail
//! straight → checkpoint → one-way gate → one-way hole (rolled over) → L3 straight → redirect →
//! toggle → one of two finish cups (layer 0). The support pillar stands under the hopper.
//!
//! # Free-fall transfers
//!
//! When the track was laid out no chain of mated ports led from the hopper to a finish: the hopper
//! leaves at `L1`, the finish takes `L3`, and no piece then raised a marble's port level (the
//! `L1`→`L3`-of-the-layer-below slope of contract §7 is not generated). The later
//! `mech_stepper_up1_l1` does (`crate::lift_run`), but this track keeps its golden. The track therefore drops
//! the marble from an open `L1` port 30 mm onto the open `L3` channel of the layer below three
//! times ([`TRANSFERS`]). Each is a controlled drop (owner ruling 2026-11-08, PRD §5.4): the
//! validator links it ([`parble_core::controlled_drops`]) and reports none of its ports. The track
//! validates with no issues: the support pillar is structural (no marble enters it, so its unlinked
//! ports and its unreachability are not issues), and the one-way hole's drop port is reached only
//! by its trap-side route, whose `-X` entry nothing feeds (that port is the drop's source).
//!
//! # Measured limits that shaped the layout
//!
//! - Hopper exit: the track was laid out when the closing gate caught a marble on flat `L1` 13 mm
//!   past the port (its `release` sensor now reaches past the gate's sweep, and flat runs are
//!   fine), so the hopper's exit is a transfer: the marble falls clear of the gate at once.
//! - Finish cup capacity: one cup holds and counts 10 standard marbles (the top ones stack with
//!   their centres 47–53 mm above the cup's cell floor, inside the `cup` sensor, which reaches the
//!   cell top). The last toggle still sends the marbles to two cups, 5 each.
//! - One-way hole from the trap side: the version 1 door (60°) hung about 11 mm into the cell
//!   below and wedged marbles against the bore. The version 2 door fills the bore, opens to 90°
//!   and delivers trap-side marbles into a drop port directly below (tower socket, or pillar then
//!   tower socket: 10 of 10). Into an empty cell below, the v1 door kept no falling marble on a
//!   redirect or a cup two layers down (0 of 10 and 0 of 5); that case was not re-measured with
//!   v2. The track still uses the hole from its other side, where marbles roll over the shut door.
//! - One-way hole from the over side (v4 door, cat's eye, glazed and steel at both sizes on a
//!   level `L1` test layout, measured 2026-11-09): the door's top is flush with the floor, but at
//!   the hinge and at the free edge the marble met contacts on the other body's edge, tilted
//!   against the travel, so a marble entering at 0.09 m/s left at 0.017–0.023 m/s, below about
//!   0.09 m/s it stopped on the door, and from about 0.3 m/s the hinge threw it up. The door seam
//!   hook (`crate::door_seam`) drops those edge contacts and the shut door is a fixed body, so it
//!   now costs what a straight in its place costs, 0.06–0.65 m/s
//!   (`behaviour::tests::one_way_hole_over_side_sweep_keeps_rolling_resistance_speed`). The
//!   layer-2 `L1` run delivers real marbles to it at 0.06–0.12 m/s, because the tower landing
//!   resets their speed to 0.14–0.20 m/s.
//! - A drop → pillar → tower fall (120 mm) gave tower exit speeds of 0.25–0.96 m/s and marbles
//!   leaving the rails past the rail joint, so the drop feeds the tower directly and the pillar is
//!   a support. The sideways speed comes from the tower, not the joint (re-measured 2026-11-08,
//!   24 runs, entry 0.25–0.525 m/s, centred or 2 mm off): the marble lands 1–5 mm off centre on
//!   the tunnel's half-round landing floor and bounces between its walls. On the faceted floor it
//!   left the socket at up to 0.64 m/s sideways and 2 of the 24 climbed off the rails; with the
//!   exact landing floor (`chunk::ChannelSlope::landing`) none leave the rails, but it still
//!   leaves at up to 0.54 m/s sideways (`block_seam_test::tower_drop_stays_on_the_rails`).
//! - The redirects stand before both forks: the landing in their tunnel sets the exit speed
//!   whatever the entry speed, to 0.31–0.70 m/s one cell past the exit on this track (measured
//!   2026-11-08 with the exact landing floor, 0.32–0.69 m/s on the faceted one), about the
//!   0.2–0.6 m/s the fork tests route.

use parble_core::{
    CellCoord, HopperConfig, MarbleSize, Mirror, PieceId, Placement, ReleaseMode, Track, Yaw4,
};

use crate::marble::MarbleKind;
use crate::track_run::{TrackRun, TrackRunError};
use crate::{Sim, behaviour, hash};

/// The track's pieces in marble order: id, anchor cell, yaw in quarter turns (no mirrors).
pub const LAYOUT: [(&str, [i32; 3], u8); 46] = [
    // Layer 7: hopper. Its L1 exit is transfer 1, onto layer 6.
    ("mech_hopper", [0, 7, -6], 0),
    ("blk_pillar", [0, 6, -6], 0),
    ("blk_straight_l3", [1, 6, -6], 0),
    ("blk_redirect_l3", [2, 6, -6], 0),
    // Layer 5: splitter, +Z branch (curve, straight) and −Z branch (curve, straight, curve,
    // straight) into the joiner's −X and −Z inlets.
    ("blk_straight_l3", [3, 5, -6], 0),
    ("mech_splitter_l3", [4, 5, -6], 0),
    ("blk_curve90_l3", [4, 5, -5], 3),
    ("blk_straight_l3", [5, 5, -5], 0),
    ("blk_curve90_l3", [4, 5, -7], 2),
    ("blk_straight_l3", [5, 5, -7], 0),
    ("blk_curve90_l3", [6, 5, -7], 1),
    ("blk_straight_l3", [6, 5, -6], 1),
    ("blk_join_l3", [6, 5, -5], 0),
    ("blk_straight_l3", [7, 5, -5], 0),
    ("blk_redirect_l3", [8, 5, -5], 0),
    // Layer 4: toggle, rejoined the same way; steep slope to L1, then transfer 2.
    ("blk_straight_l3", [9, 4, -5], 0),
    ("mech_toggle_l3", [10, 4, -5], 0),
    ("blk_curve90_l3", [10, 4, -4], 3),
    ("blk_straight_l3", [11, 4, -4], 0),
    ("blk_curve90_l3", [10, 4, -6], 2),
    ("blk_straight_l3", [11, 4, -6], 0),
    ("blk_curve90_l3", [12, 4, -6], 1),
    ("blk_straight_l3", [12, 4, -5], 1),
    ("blk_join_l3", [12, 4, -4], 0),
    ("blk_slope_steep_l3", [13, 4, -4], 0),
    ("blk_straight_l1", [14, 4, -4], 0),
    // Layer 3: gentle slope to L2, the rail family, an L2 curve, the drop hole.
    ("blk_straight_l3", [15, 3, -4], 0),
    ("blk_slope_gentle_l3", [16, 3, -4], 0),
    ("rail_straight_l2", [17, 3, -4], 0),
    ("rail_s_1x3_l2", [18, 3, -4], 0),
    ("rail_curve90_r90_l2", [21, 3, -5], 0),
    ("blk_curve90_l2", [22, 3, -7], 2),
    ("blk_straight_l2", [23, 3, -7], 0),
    ("blk_drop_l2", [24, 3, -7], 0),
    // Layer 2: tower socket, rail joint and an L1 run back along −X. The hole's −X port is
    // transfer 3.
    ("blk_tower_socket_l1", [24, 2, -7], 0),
    ("rail_joint_l1", [23, 2, -7], 0),
    ("rail_straight_l1", [22, 2, -7], 0),
    ("mech_checkpoint_l1", [21, 2, -7], 0),
    ("mech_gate_oneway_l1", [20, 2, -7], 2),
    ("mech_hole_oneway_l1", [19, 2, -7], 0),
    // Layers 1 and 0: redirect, toggle, two finish cups.
    ("blk_straight_l3", [18, 1, -7], 0),
    ("blk_redirect_l3", [17, 1, -7], 2),
    ("blk_straight_l3", [16, 0, -7], 0),
    ("mech_toggle_l3", [15, 0, -7], 2),
    ("mech_finish", [15, 0, -6], 3),
    ("mech_finish", [15, 0, -8], 1),
];

/// [`LAYOUT`] index of the support pillar, the one placement no marble enters.
pub const SUPPORT: usize = 1;

/// The free-fall transfers: (index of the placement the marble leaves by an open `L1` port,
/// index of the placement whose open `L3` channel it lands in, one layer down).
pub const TRANSFERS: [(usize, usize); 3] = [(0, 2), (25, 26), (39, 40)];

/// Each PRD §6 P2 kit item and the kit ids that stand for it in [`LAYOUT`].
pub const PRD_ITEMS: [(&str, &[&str]); 16] = [
    ("start hopper", &["mech_hopper"]),
    ("finish collector cup", &["mech_finish"]),
    (
        "straight",
        &["blk_straight_l3", "blk_straight_l2", "blk_straight_l1"],
    ),
    ("curve 90", &["blk_curve90_l3", "blk_curve90_l2"]),
    ("gentle slope", &["blk_slope_gentle_l3"]),
    ("steep slope", &["blk_slope_steep_l3"]),
    ("drop hole", &["blk_drop_l2"]),
    ("redirect drop", &["blk_redirect_l3"]),
    ("random splitter", &["mech_splitter_l3"]),
    ("joiner", &["blk_join_l3"]),
    ("L/R toggle", &["mech_toggle_l3"]),
    ("checkpoint arch", &["mech_checkpoint_l1"]),
    ("one-way gate", &["mech_gate_oneway_l1"]),
    ("one-way hole", &["mech_hole_oneway_l1"]),
    (
        "support pillar and tower block",
        &["blk_pillar", "blk_tower_socket_l1"],
    ),
    (
        "rail family proof",
        &[
            "rail_straight_l2",
            "rail_straight_l1",
            "rail_curve90_r90_l2",
            "rail_s_1x3_l2",
            "rail_joint_l1",
        ],
    ),
];

/// Track seed (the splitter's stream root).
pub const SEED: u64 = 2;
/// Marbles in the hopper queue.
pub const MARBLES: usize = 10;
/// Release interval, ticks (2 s): far enough apart that marbles do not meet at the forks,
/// joiners or the gate.
pub const INTERVAL_TICKS: u32 = 480;
/// Ticks in one run (30 s): the last marble reaches its cup at about 27 s.
pub const RUN_TICKS: u32 = 7_200;

/// [`LAYOUT`] as kit placements.
pub fn placements() -> Vec<Placement> {
    LAYOUT
        .iter()
        .map(|&(id, [x, y, z], yaw)| {
            Placement::new(
                PieceId::new(id).expect("layout ids follow the id grammar"),
                CellCoord::new(x, y, z),
                Yaw4::new(yaw),
                Mirror::Off,
            )
        })
        .collect()
}

/// The track: [`LAYOUT`], [`SEED`], and [`MARBLES`] standard marbles released from the hopper
/// every [`INTERVAL_TICKS`].
pub fn track() -> Track {
    track_sized(MarbleSize::Standard)
}

/// [`track`] with [`MARBLES`] marbles of `size` in the hopper queue (P5-T1).
pub fn track_sized(size: MarbleSize) -> Track {
    let mut t = Track::new(SEED);
    t.placements = placements();
    t.hopper = HopperConfig {
        placement: Some(0),
        queue: vec![size; MARBLES],
        release: ReleaseMode::Interval {
            ticks: INTERVAL_TICKS,
        },
    };
    t
}

/// A fresh run of [`track`] at tick 0.
pub fn run() -> Result<TrackRun, TrackRunError> {
    TrackRun::new(&track())
}

/// A fresh run of [`track_sized`] at tick 0 with every marble spawned as `kind` (P5-T1).
pub fn run_marble(kind: MarbleKind, size: MarbleSize) -> Result<TrackRun, TrackRunError> {
    TrackRun::with_marble(&track_sized(size), kind)
}

/// Marbles counted by each finish cup, in [`LAYOUT`] order.
pub fn finish_counts(sim: &Sim) -> Vec<usize> {
    let mechanisms = sim.mechanisms();
    placements()
        .into_iter()
        .filter(|p| p.piece.as_str() == behaviour::FINISH)
        .map(|p| {
            mechanisms
                .iter()
                .find(|m| m.id.0 == p.anchor)
                .map_or(0, |m| behaviour::Finish::arrivals(&m.state).len())
        })
        .collect()
}

/// Run [`track`] for [`RUN_TICKS`] and return its trajectory hash as 16 hex digits, the format of
/// `tests/golden/allpieces_30s.hash` (without the newline).
pub fn allpieces_hash_hex() -> String {
    let mut run = run().expect("the all-pieces track runs");
    run.tick(RUN_TICKS);
    hash::format_hash(run.trajectory_hash())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;
    use parble_core::{controlled_drops, validate_track};

    #[test]
    fn every_prd_item_is_in_the_layout_and_every_layout_id_is_an_item() {
        let kit = parble_kit::builtin();
        let ids: BTreeSet<&str> = LAYOUT.iter().map(|p| p.0).collect();
        let mut named = BTreeSet::new();
        for (item, pieces) in PRD_ITEMS {
            for id in pieces {
                assert!(ids.contains(id), "{item}: {id} is not placed");
                assert!(
                    kit.piece(&PieceId::new(id).unwrap()).is_some(),
                    "{id} not in the kit"
                );
                named.insert(*id);
            }
        }
        assert_eq!(ids, named, "a placed id belongs to no PRD item");
        assert_eq!(LAYOUT[SUPPORT].0, "blk_pillar");
    }

    /// The validator's view: every transfer is a controlled drop, so the port chain runs from
    /// the hopper to both cups, and the track has no issues (P3-T2). The support pillar is
    /// structural and no marble enters it; the one-way hole's drop port is reached only from its
    /// trap side, which nothing feeds.
    #[test]
    fn the_transfers_link_and_the_track_validates_clean() {
        let kit = parble_kit::builtin();
        let drops: Vec<(usize, usize)> = controlled_drops(&track(), kit)
            .iter()
            .map(|d| (d.from, d.to))
            .collect();
        assert_eq!(drops, TRANSFERS);
        let hole = 39;
        assert_eq!(LAYOUT[hole].0, "mech_hole_oneway_l1");
        assert_eq!(LAYOUT[SUPPORT].0, "blk_pillar");
        let issues = validate_track(&track(), kit);
        assert_eq!(issues, Vec::new(), "{issues:#?}");
    }

    /// [`LAYOUT`] index of the placement holding each world cell.
    fn cell_owners() -> BTreeMap<[i32; 3], usize> {
        let kit = parble_kit::builtin();
        let mut owners = BTreeMap::new();
        for (i, p) in placements().iter().enumerate() {
            for c in p.world_cells(&kit.piece(&p.piece).unwrap().def) {
                assert!(owners.insert([c.x, c.y, c.z], i).is_none(), "cells overlap");
            }
        }
        owners
    }

    fn cell_of(position: [f32; 3]) -> [i32; 3] {
        let cell = crate::scene::cell_m();
        [
            libm::floorf(position[0] / cell + 0.5) as i32,
            libm::floorf(position[1] / cell) as i32,
            libm::floorf(position[2] / cell + 0.5) as i32,
        ]
    }

    /// Why a run missed the track's success condition ([`check_run`]).
    #[derive(Debug, PartialEq)]
    enum Failure {
        /// `marble` fell off the track; `last` is the last placement its centre entered.
        FellOff { marble: usize, last: Option<usize> },
        /// The run ended with marbles outside the cups: the cup counts, and per marble no cup
        /// counted, the marble and the placement holding its centre.
        Short {
            cups: Vec<usize>,
            stuck: Vec<(usize, Option<usize>)>,
        },
        /// Every marble finished, but another part of the condition failed.
        Other(String),
    }

    /// Runs `run` for [`RUN_TICKS`] and checks the track's success condition: no marble falls off,
    /// the hopper releases all [`MARBLES`], every one ends in a finish cup with both cups used,
    /// every placement but the support is entered, the single-line pieces carry every marble,
    /// each fork sends marbles both ways and the mechanisms count every pass. On success returns
    /// the tick all marbles were in a cup and the cup counts.
    fn check_run(mut run: TrackRun) -> Result<(u32, Vec<usize>), Failure> {
        let owners = cell_owners();
        // entered[i][m]: marble m's centre was inside placement i.
        let mut entered = vec![vec![false; MARBLES]; LAYOUT.len()];
        let mut done_at = None;
        for t in 1..=RUN_TICKS {
            run.tick(1);
            for (m, s) in run.marbles().iter().enumerate() {
                if let Some(&i) = owners.get(&cell_of(s.position)) {
                    entered[i][m] = true;
                }
                if s.position[1] <= -0.06 {
                    eprintln!(
                        "marble {m} fell off the track at tick {t}: {:?}",
                        s.position
                    );
                    let last = (0..LAYOUT.len()).rev().find(|&i| entered[i][m]);
                    return Err(Failure::FellOff { marble: m, last });
                }
            }
            if done_at.is_none() && finish_counts(&run).iter().sum::<usize>() == MARBLES {
                done_at = Some(t);
            }
        }
        let other = |e: String| Err(Failure::Other(e));
        if run.marbles().len() != MARBLES {
            return other(format!("the hopper released {}", run.marbles().len()));
        }
        let counts = finish_counts(&run);
        if counts.iter().sum::<usize>() != MARBLES || !counts.iter().all(|&c| c > 0) {
            let counted: BTreeSet<usize> = run
                .mechanisms()
                .iter()
                .filter(|m| m.piece.as_str() == behaviour::FINISH)
                .flat_map(|m| behaviour::Finish::arrivals(&m.state))
                .map(|(m, _)| m)
                .collect();
            let stuck = run
                .marbles()
                .iter()
                .enumerate()
                .filter(|(m, _)| !counted.contains(m))
                .map(|(m, s)| (m, owners.get(&cell_of(s.position)).copied()))
                .collect();
            return Err(Failure::Short {
                cups: counts,
                stuck,
            });
        }
        for (i, (id, cell, _)) in LAYOUT.iter().enumerate() {
            let n = entered[i].iter().filter(|&&e| e).count();
            if i == SUPPORT {
                if n != 0 {
                    return other("a marble entered the support pillar".into());
                }
            } else if n == 0 {
                return other(format!(
                    "no marble entered placement {i} ({id} at {cell:?})"
                ));
            }
        }
        // The single-line pieces carry every marble, and each fork sent marbles both ways.
        for i in [0, 3, 5, 14, 16, 24, 29, 33, 34, 37, 38, 39, 41, 43] {
            if !entered[i].iter().all(|&e| e) {
                return other(format!("placement {i} ({}) missed a marble", LAYOUT[i].0));
            }
        }
        for i in [6, 8, 17, 19, 44, 45] {
            if !entered[i].iter().any(|&e| e) {
                return other(format!("branch {i} unused"));
            }
        }
        let mechanisms = run.mechanisms();
        let state = |i: usize| {
            let a = placements()[i].anchor;
            mechanisms
                .iter()
                .find(|m| m.id.0 == a)
                .unwrap()
                .state
                .clone()
        };
        let n = MARBLES as i64;
        for (i, key) in [
            (0, "released"),
            (5, "passes"),
            (16, "passes"),
            (37, "passes"),
            (38, "passes"),
            (39, "overs"),
        ] {
            if state(i)[key] != n {
                return other(format!("placement {i} {key} = {}, want {n}", state(i)[key]));
            }
        }
        Ok((done_at.expect("every marble is in a cup"), counts))
    }

    #[test]
    fn all_ten_marbles_reach_a_finish_cup_through_every_piece() {
        let (done_at, counts) = check_run(run().unwrap()).unwrap_or_else(|e| panic!("{e:?}"));
        eprintln!("all {MARBLES} marbles in a cup by tick {done_at}, cups {counts:?}");
    }

    /// P5-T1: every marble type at every size on the track, each with its physics table entry
    /// (the track's own test and golden run the P0 legacy marble). Each outcome is pinned: steel
    /// at the mini size and glazed at the standard size meet the success condition. Swirl,
    /// onionskin, opal and glow share the cat's eye spec, so they share its outcome at each size.
    /// Since the door seam correction (`crate::door_seam`, PHYSICS_VERSION 7) every marble that
    /// reaches the one-way hole rolls over it. The failures, per measurement on 2026-11-09 with
    /// the exact straight tube, the round fork junction (PHYSICS_VERSION 8), the forks' flat frog
    /// and the blade over its end wall (piece_version 4):
    ///
    /// - Shooter (every type): every shooter passes both forks, but each finish cup holds three,
    ///   so with both cups full four shooters stop in the layer-0 toggle [15, 0, −7] and on its
    ///   feed (placements 40 to 43). Steel: one of them at rest under the one-way hole (x 1.11 m,
    ///   in no placement), and marble 8 at rest on the straight after the hopper [1, 6, −6].
    /// - Standard steel: 9 of 10 in the cups, marble 5 at rest under the one-way hole (x 1.11 m,
    ///   y 0.11 m, in no placement).
    /// - Mini glazed: 9 of 10 in the cups, marble 8 at rest in the layer-5 redirect [8, 5, −5].
    ///
    /// The run is chaotic past the hole: a change there (the door seam hook, the shut door as a
    /// fixed body) or in the channel contacts moves which marbles collide later, and so these
    /// outcomes. Before the frog (2026-11-09) no shooter passed the splitter, the standard cat's
    /// eye family left marble 9 on the layer-0 toggle, standard steel lost marble 6 off the hole's
    /// transfer, mini cat's eye marble 7 left `rail_s_1x3_l2`, and mini glazed rolled back through
    /// the splitter (13 passes).
    fn every_size_of(kind: MarbleKind, want: [Result<(), Failure>; 3]) {
        let sizes = [MarbleSize::Mini, MarbleSize::Standard, MarbleSize::Shooter];
        for (size, want) in sizes.into_iter().zip(want) {
            let got = check_run(run_marble(kind, size).unwrap()).map(|_| ());
            assert_eq!(got, want, "{} {size:?}", kind.name());
        }
    }

    /// [`every_size_of`] for the cat's eye spec, which swirl, onionskin, opal and glow share.
    fn cats_eye_spec(kind: MarbleKind) {
        assert_eq!(kind.spec(), MarbleKind::CatsEye.spec());
        every_size_of(
            kind,
            [
                Ok(()),
                Ok(()),
                Err(Failure::Short {
                    cups: vec![3, 3],
                    stuck: vec![(6, Some(43)), (7, Some(43)), (8, Some(42)), (9, Some(40))],
                }),
            ],
        );
    }

    #[test]
    fn cats_eye_runs_the_track_at_every_size() {
        cats_eye_spec(MarbleKind::CatsEye);
    }

    #[test]
    fn swirl_runs_the_track_at_every_size() {
        cats_eye_spec(MarbleKind::Swirl);
    }

    #[test]
    fn onionskin_runs_the_track_at_every_size() {
        cats_eye_spec(MarbleKind::Onionskin);
    }

    #[test]
    fn opal_runs_the_track_at_every_size() {
        cats_eye_spec(MarbleKind::Opal);
    }

    #[test]
    fn glow_runs_the_track_at_every_size() {
        cats_eye_spec(MarbleKind::Glow);
    }

    #[test]
    fn glazed_runs_the_track_at_every_size() {
        every_size_of(
            MarbleKind::Glazed,
            [
                Err(Failure::Short {
                    cups: vec![4, 5],
                    stuck: vec![(8, Some(14))],
                }),
                Ok(()),
                Err(Failure::Short {
                    cups: vec![3, 3],
                    stuck: vec![(6, Some(43)), (7, Some(43)), (8, Some(40)), (9, Some(42))],
                }),
            ],
        );
    }

    #[test]
    fn steel_runs_the_track_at_every_size() {
        every_size_of(
            MarbleKind::Steel,
            [
                Ok(()),
                Err(Failure::Short {
                    cups: vec![4, 5],
                    stuck: vec![(5, None)],
                }),
                Err(Failure::Short {
                    cups: vec![3, 3],
                    stuck: vec![(6, Some(43)), (7, None), (8, Some(2)), (9, Some(43))],
                }),
            ],
        );
    }

    #[test]
    fn the_run_repeats() {
        assert_eq!(RUN_TICKS % 4, 0);
        assert_eq!(allpieces_hash_hex(), allpieces_hash_hex());
    }
}
