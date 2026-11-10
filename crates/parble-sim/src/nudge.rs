//! Nudge zones in the sim ([`parble_core::nudge_zone`], owner request 2026-11-09).
//!
//! [`Sim::tick`](crate::Sim::tick) calls [`apply`] before every physics step, after rolling
//! resistance. For each marble in spawn order and each zone in track order: when the marble's
//! centre, quantized to 1 µm like the trajectory hash, is inside the zone's box (an integer
//! test), and its speed along the zone's direction is below the zone's minimum, its linear
//! velocity gains `min(max_accel · dt, min_speed − speed along)` along the direction. A zone in
//! constant mode adds `max_accel · dt` along the direction to every marble inside, whatever its
//! speed. A sim with no zones skips the pass entirely, so every track without them keeps its
//! trajectory hash, and the top-up arithmetic is the same as before constant mode existed.
//!
//! Determinism: the direction comes from `libm` trig, the speeds from integer µm units, and the
//! iteration order is the marbles' spawn order and the zones' track order. No wall clock.
//!
//! [`track`] is the nudged test track: a hopper releases marbles into a run of rope arches that
//! stalls them short of the finish cup, and [`track_zone`] keeps them rolling to
//! it. Its trajectory hash is the committed golden `tests/golden/nudge_track.hash`, checked
//! natively and by the wasm parity check (`make determinism-wasm`). [`constant_track`] is the same
//! run with [`constant_zone`], a constant-mode zone, and its golden is
//! `tests/golden/nudge_constant_track.hash`.

use parble_core::nudge_zone::{NudgeMode, NudgeZone};
use parble_core::{
    CellCoord, HopperConfig, MarbleSize, Mirror, PieceId, Placement, ReleaseMode, Track, Yaw4,
};
use rapier3d::prelude::*;

use crate::hash::{POSITION_QUANTA_PER_M, quantize};
use crate::track_run::{TrackRun, TrackRunError};
use crate::{DT, behaviour, hash};

/// One zone as the sim applies it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Nudge {
    min_um: [i64; 3],
    max_um: [i64; 3],
    /// Unit direction.
    dir: Vector,
    /// Minimum speed along `dir`, m/s.
    min_speed: f32,
    /// Most velocity one tick adds, m/s (in constant mode, what every tick adds).
    dv_max: f32,
    /// Constant mode: push whatever the speed.
    constant: bool,
}

impl Nudge {
    pub fn new(z: &NudgeZone) -> Self {
        let d = direction(z.yaw_mdeg, z.pitch_mdeg);
        Self {
            min_um: z.min_um.map(i64::from),
            max_um: z.max_um.map(i64::from),
            dir: Vector::new(d[0], d[1], d[2]),
            min_speed: (f64::from(z.min_speed_um_s) * 1.0e-6) as f32,
            dv_max: (f64::from(z.max_accel_um_s2) * 1.0e-6 * f64::from(DT)) as f32,
            constant: z.mode == NudgeMode::Constant,
        }
    }

    fn contains(&self, p: [i64; 3]) -> bool {
        (0..3).all(|i| self.min_um[i] <= p[i] && p[i] <= self.max_um[i])
    }
}

/// The unit direction of a zone's yaw and pitch, millidegrees ([`parble_core::nudge_zone`]):
/// `(cos p · cos y, sin p, −cos p · sin y)`, through `libm`.
pub fn direction(yaw_mdeg: i32, pitch_mdeg: i32) -> [f32; 3] {
    let rad = |mdeg: i32| f64::from(mdeg) * 1.0e-3 * core::f64::consts::PI / 180.0;
    let (y, p) = (rad(yaw_mdeg), rad(pitch_mdeg));
    let (cp, sp) = (libm::cos(p), libm::sin(p));
    [
        (cp * libm::cos(y)) as f32,
        sp as f32,
        (-cp * libm::sin(y)) as f32,
    ]
}

/// One tick's push on every marble inside a zone (module docs).
pub(crate) fn apply(world: &mut PhysicsWorld, marbles: &[RigidBodyHandle], nudges: &[Nudge]) {
    for &h in marbles {
        let Some(body) = world.bodies.get_mut(h) else {
            continue;
        };
        let t = body.translation();
        let at = [t.x, t.y, t.z].map(|c| quantize(c, POSITION_QUANTA_PER_M));
        let mut v = body.linvel();
        let mut pushed = false;
        for n in nudges {
            if !n.contains(at) {
                continue;
            }
            if n.constant {
                v += n.dir * n.dv_max;
                pushed = true;
                continue;
            }
            let along = v.dot(n.dir);
            if along < n.min_speed {
                v += n.dir * n.dv_max.min(n.min_speed - along);
                pushed = true;
            }
        }
        if pushed {
            body.set_linvel(v, true);
        }
    }
}

/// Rope arches on [`track`]'s run: each one halves a marble's speed, so a marble from the hopper
/// stalls in them.
pub const ARCHES: i32 = 6;
/// Track seed.
pub const TRACK_SEED: u64 = 11;
/// Marbles in the hopper queue.
pub const TRACK_MARBLES: usize = 3;
/// Release interval, ticks (2 s).
pub const TRACK_INTERVAL_TICKS: u32 = 480;
/// Ticks in one run of [`track`] (20 s).
pub const TRACK_TICKS: u32 = 4_800;
/// Row of the track, cell z (behind the tray).
pub const TRACK_Z: i32 = -6;
/// Last cell x of the run (the last arch).
pub const RUN_END_X: i32 = 2 * ARCHES;

fn place(piece: &str, x: i32, y: i32, z: i32, yaw: u8) -> Placement {
    Placement::new(
        PieceId::new(piece).expect("track ids follow the id grammar"),
        CellCoord::new(x, y, z),
        Yaw4::new(yaw),
        Mirror::Off,
    )
}

/// The track without its zone: a hopper at L1, then [`ARCHES`] times an L1 straight and a rope
/// arch along +X, then a 30 mm drop into a finish cup one layer down.
pub fn flat_track() -> Track {
    let mut t = Track::new(TRACK_SEED);
    t.placements
        .push(place(behaviour::HOPPER, 0, 1, TRACK_Z, 0));
    for i in 0..ARCHES {
        t.placements
            .push(place("blk_straight_l1", 2 * i + 1, 1, TRACK_Z, 0));
        t.placements
            .push(place(behaviour::ROPE_ARCH, 2 * i + 2, 1, TRACK_Z, 0));
    }
    t.placements
        .push(place(behaviour::FINISH, RUN_END_X + 1, 0, TRACK_Z, 0));
    t.hopper = HopperConfig {
        placement: Some(0),
        queue: vec![MarbleSize::Standard; TRACK_MARBLES],
        release: ReleaseMode::Interval {
            ticks: TRACK_INTERVAL_TICKS,
        },
    };
    t
}

/// The zone of [`track`]: over the whole run of straights and arches, along +X, with the editor's defaults (0.2 m/s,
/// 1 m/s²).
pub fn track_zone() -> NudgeZone {
    NudgeZone {
        name: "flat run".into(),
        ..NudgeZone::over_cells(
            CellCoord::new(1, 1, TRACK_Z),
            CellCoord::new(RUN_END_X, 1, TRACK_Z),
            0,
        )
    }
}

/// [`flat_track`] with [`track_zone`].
pub fn track() -> Track {
    let mut t = flat_track();
    t.nudge_zones.push(track_zone());
    t
}

/// The constant-mode zone of [`constant_track`]: [`track_zone`]'s box and heading, pushing at
/// 0.25 m/s² whatever the speed (a quarter of the top-up default, so the run stays on the track).
pub fn constant_zone() -> NudgeZone {
    NudgeZone {
        name: "constant run".into(),
        mode: NudgeMode::Constant,
        max_accel_um_s2: 250_000,
        ..track_zone()
    }
}

/// [`flat_track`] with [`constant_zone`].
pub fn constant_track() -> Track {
    let mut t = flat_track();
    t.nudge_zones.push(constant_zone());
    t
}

/// Run [`constant_track`] for [`TRACK_TICKS`] and return its trajectory hash, the format of
/// `tests/golden/nudge_constant_track.hash` (without the newline).
pub fn constant_track_hash_hex() -> String {
    let mut run = TrackRun::new(&constant_track()).expect("the constant nudge track runs");
    run.tick(TRACK_TICKS);
    hash::format_hash(run.trajectory_hash())
}

/// A fresh run of [`track`] at tick 0.
pub fn track_run() -> Result<TrackRun, TrackRunError> {
    TrackRun::new(&track())
}

/// Run [`track`] for [`TRACK_TICKS`] and return its trajectory hash as 16 hex digits, the format
/// of `tests/golden/nudge_track.hash` (without the newline).
pub fn nudge_track_hash_hex() -> String {
    let mut run = track_run().expect("the nudge track runs");
    run.tick(TRACK_TICKS);
    hash::format_hash(run.trajectory_hash())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lift_run::finished;

    #[test]
    fn directions_follow_the_placement_turn() {
        let close = |a: [f32; 3], b: [f32; 3]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-6);
        assert!(close(direction(0, 0), [1.0, 0.0, 0.0]));
        assert!(close(direction(90_000, 0), [0.0, 0.0, -1.0]));
        assert!(close(direction(180_000, 0), [-1.0, 0.0, 0.0]));
        assert!(close(direction(270_000, 0), [0.0, 0.0, 1.0]));
        assert!(close(direction(0, 90_000), [0.0, 1.0, 0.0]));
        let d = direction(45_000, 30_000);
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        assert!((len - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_arches_stall_marbles_and_the_zone_finishes_them() {
        let mut bare = TrackRun::new(&flat_track()).unwrap();
        bare.tick(TRACK_TICKS);
        assert_eq!(bare.marbles().len(), TRACK_MARBLES);
        assert_eq!(finished(&bare), 0, "the arches stall every marble");
        let mut nudged = track_run().unwrap();
        nudged.tick(TRACK_TICKS);
        assert_eq!(
            finished(&nudged),
            TRACK_MARBLES,
            "the zone finishes them all"
        );
    }

    /// A marble inside the zone is topped up by at most one tick's acceleration, never past the
    /// minimum speed, and never slowed.
    #[test]
    fn the_push_tops_up_to_the_minimum_and_no_further() {
        let z = track_zone();
        let n = Nudge::new(&z);
        let mut world = PhysicsWorld::new();
        let inside = Vector::new(0.12, 0.08, -0.36);
        let mut add = |v: Vector| {
            let h = world
                .bodies
                .insert(RigidBodyBuilder::dynamic().translation(inside).linvel(v));
            (h, v)
        };
        let still = add(Vector::ZERO);
        let near = add(Vector::new(0.199, 0.0, 0.0));
        let fast = add(Vector::new(0.5, 0.0, 0.0));
        let back = add(Vector::new(-0.3, 0.0, 0.0));
        let side = add(Vector::new(0.0, 0.0, 0.4));
        let marbles = [still.0, near.0, fast.0, back.0, side.0];
        apply(&mut world, &marbles, &[n]);
        let dv = 1.0 / 240.0;
        let v = |h: RigidBodyHandle| world.bodies[h].linvel();
        assert!((v(still.0).x - dv).abs() < 1e-7, "{:?}", v(still.0));
        assert!((v(near.0).x - 0.2).abs() < 1e-6, "capped at the minimum");
        assert_eq!(v(fast.0), fast.1, "a fast marble is left alone");
        assert!((v(back.0).x - (-0.3 + dv)).abs() < 1e-6);
        assert!((v(side.0).x - dv).abs() < 1e-7 && v(side.0).z == 0.4);
        // Outside the box nothing happens.
        let mut world2 = PhysicsWorld::new();
        let out = world2.bodies.insert(
            RigidBodyBuilder::dynamic()
                .translation(Vector::new(0.12, 0.2, -0.36))
                .linvel(Vector::ZERO),
        );
        apply(&mut world2, &[out], &[n]);
        assert_eq!(world2.bodies[out].linvel(), Vector::ZERO);
    }

    /// From rest, a second inside a top-up zone ends at the minimum speed and stays there, while
    /// the same zone in constant mode keeps adding its acceleration every tick, past the minimum.
    #[test]
    fn a_constant_zone_keeps_accelerating_past_the_minimum_where_top_up_stops() {
        let top_up = track_zone();
        let constant = NudgeZone {
            mode: NudgeMode::Constant,
            ..top_up.clone()
        };
        let mut world = PhysicsWorld::new();
        let inside = Vector::new(0.12, 0.08, -0.36);
        let mut add = || {
            world.bodies.insert(
                RigidBodyBuilder::dynamic()
                    .translation(inside)
                    .linvel(Vector::ZERO),
            )
        };
        let (a, b) = (add(), add());
        let (na, nb) = ([Nudge::new(&top_up)], [Nudge::new(&constant)]);
        let dv = 1.0 / 240.0;
        let mut speeds = Vec::new();
        for _ in 0..240 {
            apply(&mut world, &[a], &na);
            apply(&mut world, &[b], &nb);
            speeds.push((world.bodies[a].linvel().x, world.bodies[b].linvel().x));
        }
        // Both start the same: one tick of 1 m/s².
        assert!((speeds[0].0 - dv).abs() < 1e-7 && (speeds[0].1 - dv).abs() < 1e-7);
        // Top-up reaches 0.2 m/s after 48 ticks and holds it.
        assert!(
            speeds[47..].iter().all(|s| (s.0 - 0.2).abs() < 1e-6),
            "{:?}",
            &speeds[40..60]
        );
        // Constant passes 0.2 m/s and keeps rising by one tick's push each tick, to 1 m/s after 1 s.
        assert!(speeds[60].1 > 0.25);
        assert!(
            speeds
                .windows(2)
                .all(|w| (w[1].1 - w[0].1 - dv).abs() < 1e-5)
        );
        assert!((speeds[239].1 - 1.0).abs() < 1e-3, "{}", speeds[239].1);
        // Constant mode pushes a fast marble too, and slows nothing sideways.
        let fast = world.bodies.insert(
            RigidBodyBuilder::dynamic()
                .translation(inside)
                .linvel(Vector::new(1.5, 0.0, 0.3)),
        );
        apply(&mut world, &[fast], &nb);
        let v = world.bodies[fast].linvel();
        assert!((v.x - (1.5 + dv)).abs() < 1e-6 && v.z == 0.3, "{v:?}");
        // Outside the box nothing happens.
        let out = world.bodies.insert(
            RigidBodyBuilder::dynamic()
                .translation(Vector::new(0.12, 0.2, -0.36))
                .linvel(Vector::ZERO),
        );
        apply(&mut world, &[out], &nb);
        assert_eq!(world.bodies[out].linvel(), Vector::ZERO);
    }

    /// The constant track runs every marble to the finish, and its zone moves the run away from
    /// the top-up track's.
    #[test]
    fn the_constant_track_differs_from_top_up_and_repeats() {
        let mut run = TrackRun::new(&constant_track()).unwrap();
        run.tick(TRACK_TICKS);
        assert_eq!(
            finished(&run),
            TRACK_MARBLES,
            "the constant zone finishes them all"
        );
        let c = constant_track_hash_hex();
        assert_eq!(c, constant_track_hash_hex());
        assert_ne!(c, nudge_track_hash_hex());
    }

    #[test]
    fn the_nudge_track_run_repeats() {
        assert_eq!(nudge_track_hash_hex(), nudge_track_hash_hex());
    }

    /// The marble check runs the same [`TrackRun`], so it sees the zone: every checked marble type
    /// stalls in the arches of the bare track, and the zone clears every stall warning.
    #[test]
    fn a_nudge_zone_clears_the_marble_check_stall_warning() {
        use crate::marble_check::{MarbleCheck, Outcome, check_kinds};
        for (kind, _) in check_kinds() {
            let bare = MarbleCheck::run(&flat_track(), kind).unwrap();
            assert_eq!(bare.marbles.len(), TRACK_MARBLES);
            let arches = 1..=(2 * ARCHES as usize);
            for m in &bare.marbles {
                assert!(
                    matches!(m.outcome, Outcome::Stalled { at: Some(p), .. } if arches.contains(&p)),
                    "{} marble {}: {:?}",
                    kind.name(),
                    m.marble,
                    m.outcome
                );
            }
            let nudged = MarbleCheck::run(&track(), kind).unwrap();
            assert_eq!(nudged.marbles.len(), TRACK_MARBLES);
            assert_eq!(
                nudged.problems().count(),
                0,
                "{}: {:?}",
                kind.name(),
                nudged.lines()
            );
        }
    }
}
