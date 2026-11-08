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
//! No chain of mated ports leads from the hopper to a finish with the shipped kit: the hopper
//! leaves at `L1`, the finish takes `L3`, and no shipped piece raises a marble's port level (the
//! `L1`→`L3`-of-the-layer-below slope of contract §7 is not generated). The track therefore drops
//! the marble from an open `L1` port 30 mm onto the open `L3` channel of the layer below three
//! times ([`TRANSFERS`]). The validator reports those ports as open and everything after the first
//! transfer as unreachable; the test pins that list.
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
//! - A drop → pillar → tower fall (120 mm) gave tower exit speeds of 0.25–0.96 m/s and marbles
//!   leaving the rails past the rail joint, so the drop feeds the tower directly and the pillar is
//!   a support. The sideways speed comes from the tower, not the joint (re-measured 2026-11-08,
//!   24 runs, entry 0.25–0.525 m/s, centred or 2 mm off): the marble lands 1–5 mm off centre on
//!   the tunnel's half-round landing floor and bounces between its walls. On the faceted floor it
//!   left the socket at up to 0.64 m/s sideways and 2 of the 24 climbed off the rails; with the
//!   exact landing floor (`chunk::ChannelSlope::landing`) none leave the rails, but it still
//!   leaves at up to 0.54 m/s sideways (`block_seam_test::tower_drop_stays_on_the_rails`).
//! - The redirects stand before both forks: the landing in their tunnel sets the exit speed to
//!   about 0.55–0.65 m/s whatever the entry speed, inside the 0.6 m/s the fork tests route.

use parble_core::{
    CellCoord, HopperConfig, MarbleSize, Mirror, PieceId, Placement, ReleaseMode, Track, Yaw4,
};

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
    let mut t = Track::new(SEED);
    t.placements = placements();
    t.hopper = HopperConfig {
        placement: Some(0),
        queue: vec![MarbleSize::Standard; MARBLES],
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
    use parble_core::{IssueKind, validate_track};

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

    /// The validator's view: no overlaps, no unknown ids, and open ports or broken links only at
    /// the transfers and at the support pillar. Everything after the hopper is unreachable to
    /// it, because a free fall is not a port link.
    #[test]
    fn only_the_transfers_and_the_support_break_the_port_chain() {
        let issues = validate_track(&track(), parble_kit::builtin());
        let mut port_issues = Vec::new();
        for i in &issues {
            match &i.kind {
                IssueKind::Unreachable | IssueKind::FinishUnreachable => {}
                IssueKind::OpenPort { .. } | IssueKind::BrokenLink { .. } => {
                    port_issues.push(i.placement.expect("a port issue has a placement"));
                }
                other => panic!("unexpected issue {other:?} at {:?}", i.placement),
            }
        }
        let mut allowed: Vec<usize> = TRANSFERS.iter().flat_map(|&(a, b)| [a, b]).collect();
        allowed.push(SUPPORT);
        allowed.sort_unstable();
        port_issues.sort_unstable();
        port_issues.dedup();
        assert_eq!(port_issues, allowed, "{issues:#?}");
        let unreachable: BTreeSet<usize> = issues
            .iter()
            .filter(|i| {
                matches!(
                    i.kind,
                    IssueKind::Unreachable | IssueKind::FinishUnreachable
                )
            })
            .filter_map(|i| i.placement)
            .collect();
        let expected: BTreeSet<usize> = (1..LAYOUT.len()).collect();
        assert_eq!(unreachable, expected);
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

    #[test]
    fn all_ten_marbles_reach_a_finish_cup_through_every_piece() {
        let owners = cell_owners();
        let mut run = run().unwrap();
        // entered[i][m]: marble m's centre was inside placement i.
        let mut entered = vec![vec![false; MARBLES]; LAYOUT.len()];
        let mut done_at = None;
        for t in 1..=RUN_TICKS {
            run.tick(1);
            for (m, s) in run.marbles().iter().enumerate() {
                if let Some(&i) = owners.get(&cell_of(s.position)) {
                    entered[i][m] = true;
                }
                assert!(
                    s.position[1] > -0.06,
                    "marble {m} fell off the track at tick {t}: {:?}",
                    s.position
                );
            }
            if done_at.is_none() && finish_counts(&run).iter().sum::<usize>() == MARBLES {
                done_at = Some(t);
            }
        }
        assert_eq!(run.marbles().len(), MARBLES, "the hopper released them all");
        let counts = finish_counts(&run);
        assert_eq!(counts.iter().sum::<usize>(), MARBLES, "cups {counts:?}");
        assert!(counts.iter().all(|&c| c > 0), "cups {counts:?}");
        eprintln!(
            "all {MARBLES} marbles in a cup by tick {}, cups {counts:?}",
            done_at.unwrap()
        );
        for (i, (id, cell, _)) in LAYOUT.iter().enumerate() {
            let n = entered[i].iter().filter(|&&e| e).count();
            if i == SUPPORT {
                assert_eq!(n, 0, "a marble entered the support pillar");
            } else {
                assert!(n > 0, "no marble entered placement {i} ({id} at {cell:?})");
            }
        }
        // The single-line pieces carry every marble, and each fork sent marbles both ways.
        for i in [0, 3, 5, 14, 16, 24, 29, 33, 34, 37, 38, 39, 41, 43] {
            assert!(
                entered[i].iter().all(|&e| e),
                "placement {i} ({}) missed a marble",
                LAYOUT[i].0
            );
        }
        for i in [6, 8, 17, 19, 44, 45] {
            assert!(entered[i].iter().any(|&e| e), "branch {i} unused");
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
        assert_eq!(state(0)["released"], n);
        assert_eq!(state(5)["passes"], n);
        assert_eq!(state(16)["passes"], n);
        assert_eq!(state(37)["passes"], n);
        assert_eq!(state(38)["passes"], n);
        assert_eq!(state(39)["overs"], n);
    }

    #[test]
    fn the_run_repeats() {
        assert_eq!(RUN_TICKS % 4, 0);
        assert_eq!(allpieces_hash_hex(), allpieces_hash_hex());
    }
}
