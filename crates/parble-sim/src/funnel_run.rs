//! The funnel look-dev track: `blk_funnel_3x3_l2` with both rim inlets fed and its centre exit
//! dropping into `blk_catch_l1` and on to an `L1` run-out, for judging how a marble enters and
//! spirals down the bowl (`--scenario funnel` and `funnel-rail`, `docs/design/funnel/`).
//!
//! - Inlet 0 (the funnel's −x port, cell (2, 2, −2)) is fed by two `L2` block straights from −x.
//! - Inlet 1 (its +x port, cell (4, 2, −4)) is fed by two `L2` rails from +x.
//! - The catch stands on the floor under the bore at (3, 0, −3), its `L1` exit facing −x on to three
//!   `L1` straights; the marble leaves the last one on to the open floor.
//!
//! Every cell is in chunk (0, 0, −1), so the bore crossing is inside one trimesh (as the funnel
//! tests shift their layout to have it), and behind the plain environment's tray.

use parble_core::{CellCoord, Mirror, PieceId, Placement, Yaw4};
use rapier3d::prelude::*;

use crate::marble::MarbleSpec;
use crate::{Sim, scene};

/// The funnel's anchor cell: its cells are x 2..4, layers 1..2, z −4..−2.
pub const FUNNEL_CELL: [i32; 3] = [2, 2, -2];
/// The bowl's axis in plan (world x, z), m: the centre of the funnel's middle cell.
pub const AXIS_M: [f32; 2] = [0.18, -0.18];
/// The rim inlets' floor height (world y), m: layer 2's `L2` floor.
pub const RIM_FLOOR_M: f32 = 0.15;

/// The track's pieces: id, anchor cell, yaw in quarter turns.
pub const LAYOUT: [(&str, [i32; 3], u8); 9] = [
    ("blk_straight_l2", [0, 2, -2], 0),
    ("blk_straight_l2", [1, 2, -2], 0),
    ("blk_funnel_3x3_l2", FUNNEL_CELL, 0),
    ("rail_straight_l2", [5, 2, -4], 0),
    ("rail_straight_l2", [6, 2, -4], 0),
    ("blk_catch_l1", [3, 0, -3], 0),
    ("blk_straight_l1", [2, 0, -3], 0),
    ("blk_straight_l1", [1, 0, -3], 0),
    ("blk_straight_l1", [0, 0, -3], 0),
];

/// Launch speed along the feeder, m/s (the funnel roll-in test's).
pub const LAUNCH_M_S: f32 = 0.3;

/// Ticks in one run before it restarts (20 s): the slowest marble has left the run-out by then.
pub const RUN_TICKS: u32 = 4_800;

/// The track as kit placements.
pub fn placements() -> Vec<Placement> {
    LAYOUT
        .iter()
        .map(|&(id, [x, y, z], yaw)| {
            Placement::new(
                PieceId::new(id).expect("funnel track ids follow the id grammar"),
                CellCoord::new(x, y, z),
                Yaw4::new(yaw),
                Mirror::Off,
            )
        })
        .collect()
}

/// Start position and velocity of a marble of radius `r` fed to `inlet` (0: the block channel
/// from −x, 1: the rail from +x), on the feeder's first piece, 20 mm in from its far end.
pub fn launch(inlet: u8, r: f32) -> (Vector, Vector) {
    let cell = scene::cell_m();
    let y = RIM_FLOOR_M + r + 0.001;
    if inlet == 0 {
        (Vector::new(-0.02, y, -2.0 * cell), Vector::X * LAUNCH_M_S)
    } else {
        (
            Vector::new(6.0 * cell + 0.02, y, -4.0 * cell),
            -Vector::X * LAUNCH_M_S,
        )
    }
}

impl Sim {
    /// The funnel track on the floor, one marble of `spec` rolling into `inlet` at tick 0.
    pub fn funnel_scene(inlet: u8, spec: MarbleSpec) -> Self {
        let kit = parble_kit::builtin();
        let mut sim = Self::empty();
        sim.world.colliders.insert(scene::floor_collider());
        for p in placements() {
            sim.place_piece(kit, p)
                .expect("the funnel track does not overlap");
        }
        sim.rebuild_chunks(kit)
            .expect("the funnel track builds its chunk colliders");
        let (at, v) = launch(inlet, spec.radius_m);
        sim.add_marble(spec, at);
        let h = *sim.marbles.last().expect("just added");
        let body = &mut sim.world.bodies[h];
        body.set_linvel(v, true);
        // Rolling without slip: ω = ŷ × v / r.
        body.set_angvel(Vector::Y.cross(v) / spec.radius_m, true);
        sim.sample();
        sim
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marble::{MarbleKind, MarbleSize};
    use parble_core::{IssueKind, Track, validate_track};

    #[test]
    fn the_funnel_track_links_every_inlet_and_the_exit() {
        let mut t = Track::new(0);
        t.placements = placements();
        let open: Vec<_> = validate_track(&t, parble_kit::builtin())
            .into_iter()
            .filter(|i| {
                matches!(
                    i.kind,
                    IssueKind::OpenPort { .. } | IssueKind::BrokenLink { .. }
                ) || matches!(i.kind, IssueKind::Overlap { .. })
            })
            .collect();
        // Only the three far ends are open: both feeders' starts and the run-out's end.
        assert_eq!(open.len(), 3, "{open:#?}");
    }

    /// Writes every tick of each (inlet, size) run to `target/funnel/paths.json` for the design
    /// overlay (`scripts/design/funnel_overlay.py`). Not a gate: `cargo test -p parble-sim
    /// funnel_paths -- --ignored`.
    #[test]
    #[ignore]
    fn funnel_paths() {
        use std::fmt::Write;
        let mut json = String::from("[");
        for inlet in [0u8, 1] {
            for size in [MarbleSize::Mini, MarbleSize::Standard, MarbleSize::Shooter] {
                let spec = MarbleKind::CatsEye.spec_sized(size);
                let mut sim = Sim::funnel_scene(inlet, spec);
                let mut pts = Vec::new();
                pts.push(sim.marbles()[0]);
                for _ in 0..RUN_TICKS {
                    sim.tick(1);
                    pts.push(sim.marbles()[0]);
                }
                if json.len() > 1 {
                    json.push(',');
                }
                write!(
                    json,
                    "{{\"inlet\":{inlet},\"size\":\"{}\",\"radius\":{},\"pos\":[",
                    size.name(),
                    spec.radius_m
                )
                .unwrap();
                for (i, m) in pts.iter().enumerate() {
                    let [x, y, z] = m.position;
                    let [vx, vy, vz] = m.linvel;
                    if i > 0 {
                        json.push(',');
                    }
                    write!(json, "[{x},{y},{z},{vx},{vy},{vz}]").unwrap();
                }
                json.push_str("]}");
            }
        }
        json.push(']');
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/funnel");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("paths.json"), json).unwrap();
    }
}
