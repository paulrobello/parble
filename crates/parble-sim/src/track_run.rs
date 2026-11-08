//! Running a loaded [`Track`] (P3-T3, P2-T5).
//!
//! A track runs in the P0 tray with its placements merged in as kit chunk colliders
//! ([`crate::chunk`], [`Sim::place_piece`]). Every placement id goes through [`kit_binding`]: a
//! [`parble_kit::builtin`] id places that piece, an id in [`KIT_ALIASES`] places its kit piece, an
//! id in [`NO_COLLIDER`] is a piece without a static collider yet, and any other id refuses the
//! run. Each placement whose piece has a behaviour in the registry ([`behaviour::registry`] for
//! [`TrackRun::new`]) also gets a [`crate::mechanism`] instance rooted at the track seed: its
//! sensors and its kinematic parts at the sidecar pivots.
//!
//! Marbles spawn in the bowl of the hopper placement ([`parble_core::HopperConfig::placement`])
//! when it is a `mech_hopper`, rolling toward its exit at [`HOPPER_KICK_M_S`], and the hopper's
//! gate opens for each one on its release tick. Without a hopper they spawn on the tray ramp.
//!
//! Releases happen between steps, on whole ticks, so a run is a pure function of the track and the
//! tick count.

use crate::behaviour::{self, release_ticks};
use crate::chunk::{ChunkError, place_point_um};
use crate::mechanism::{MechanismError, MechanismRegistry};
use crate::{Sim, scene};
use parble_core::contract::{
    MARBLE_MINI_DIAMETER_UM, MARBLE_SHOOTER_DIAMETER_UM, MARBLE_STANDARD_DIAMETER_UM,
};
use parble_core::{CellCoord, MarbleSize, Orientation, PieceId, Placement, Track};
use parble_kit::KitCatalog;
use rapier3d::prelude::*;

/// `.parble` piece ids written before the kit naming of contract §10, and the kit piece each one
/// runs as. Saved tracks keep these ids (contract §10: ids are stable forever).
pub const KIT_ALIASES: &[(&str, &str)] = &[
    ("blk_straight", "blk_straight_l3"),
    // The block-family run terminal. The contract's finish mechanism is `mech_finish`.
    ("blk_finish", "blk_end_l3"),
    ("mech_toggle", "mech_toggle_l3"),
];

/// Piece ids that are valid in a track but take no part in the run yet, with the reason. This list
/// wins over the kit.
pub const NO_COLLIDER: &[(&str, &str)] =
    &[("rail_curve_90", "no rail-family sidecar is shipped yet")];

/// How a `.parble` placement reaches the sim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KitBinding {
    /// Placed as this kit piece's chunk collider.
    Kit(PieceId),
    /// A known piece without a static collider ([`NO_COLLIDER`]).
    NoCollider { reason: &'static str },
}

/// Resolves a placement id against `kit`: [`NO_COLLIDER`], then a kit id, then [`KIT_ALIASES`].
/// Any other id is `None`.
pub fn kit_binding(kit: &KitCatalog, id: &PieceId) -> Option<KitBinding> {
    let name = id.as_str();
    if let Some(&(_, reason)) = NO_COLLIDER.iter().find(|(from, _)| *from == name) {
        return Some(KitBinding::NoCollider { reason });
    }
    if kit.piece(id).is_some() {
        return Some(KitBinding::Kit(id.clone()));
    }
    KIT_ALIASES
        .iter()
        .find(|(from, _)| *from == name)
        .map(|&(_, to)| {
            KitBinding::Kit(PieceId::new(to).expect("alias targets follow the id grammar"))
        })
}

/// Why a track cannot run.
#[derive(Debug, Clone, PartialEq)]
pub enum TrackRunError {
    /// The placement at this index uses an id that is not a kit piece, an alias or a known piece
    /// without a collider.
    UnknownPiece { placement: usize, piece: PieceId },
    /// The chunk colliders refused the placement at this index (overlap), or the rebuild failed
    /// (`placement: None`).
    Chunk {
        placement: Option<usize>,
        error: ChunkError,
    },
    /// The mechanism of the placement at this index could not be added.
    Mechanism {
        placement: usize,
        error: MechanismError,
    },
}

impl core::fmt::Display for TrackRunError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownPiece { placement, piece } => write!(
                f,
                "placement {placement}: piece {piece} is not a kit piece or a known alias"
            ),
            Self::Chunk {
                placement: Some(i),
                error,
            } => write!(f, "placement {i}: {error}"),
            Self::Chunk {
                placement: None,
                error,
            } => write!(f, "chunk rebuild: {error}"),
            Self::Mechanism { placement, error } => {
                write!(f, "placement {placement}: mechanism: {error}")
            }
        }
    }
}

impl std::error::Error for TrackRunError {}

pub use crate::behaviour::ALL_STAGGER_TICKS;

/// Speed a hopper marble starts rolling toward the hopper exit with, m/s: the bowl floor is flat.
pub const HOPPER_KICK_M_S: f32 = 0.3;
/// Hopper bowl centre on its floor in the piece frame, µm (`blender/kit/kitmech.py` `HOPPER_X`,
/// floor at L1).
pub const HOPPER_BOWL_UM: [i32; 3] = [-10_000, 15_000, 0];

/// Radius in metres of a marble size (contract §6).
pub fn marble_radius_m(size: MarbleSize) -> f32 {
    let d = match size {
        MarbleSize::Mini => MARBLE_MINI_DIAMETER_UM,
        MarbleSize::Standard => MARBLE_STANDARD_DIAMETER_UM,
        MarbleSize::Shooter => MARBLE_SHOOTER_DIAMETER_UM,
    };
    scene::um_to_m(d / 2)
}

/// Release ticks for the hopper queue, in queue order ([`release_ticks`]).
pub fn release_schedule(track: &Track) -> Vec<(u64, MarbleSize)> {
    let queue = &track.hopper.queue;
    release_ticks(track.hopper.release, queue.len())
        .into_iter()
        .zip(queue.iter().copied())
        .collect()
}

/// A [`Sim`] plus the release schedule that feeds it. Reads go through `Deref` to the sim;
/// [`TrackRun::tick`] releases due marbles before each step.
pub struct TrackRun {
    sim: Sim,
    releases: Vec<(u64, MarbleSize)>,
    next: usize,
    /// The hopper placement marbles spawn in, if the track has one.
    hopper: Option<Placement>,
}

impl TrackRun {
    /// A run with no releases (the P0 scenarios).
    pub fn from_sim(sim: Sim) -> Self {
        Self {
            sim,
            releases: Vec::new(),
            next: 0,
            hopper: None,
        }
    }

    /// `track` in the tray with its placements as [`parble_kit::builtin`] chunk colliders and the
    /// kit behaviours ([`behaviour::registry`]), at tick 0 with its tick-0 releases in and sampled.
    pub fn new(track: &Track) -> Result<Self, TrackRunError> {
        Self::with_kit(track, parble_kit::builtin())
    }

    /// [`TrackRun::new`] against any kit catalogue.
    pub fn with_kit(track: &Track, kit: &KitCatalog) -> Result<Self, TrackRunError> {
        Self::with_mechanisms(track, kit, &behaviour::registry())
    }

    /// `track` with a mechanism instance ([`Sim::add_registered`], rooted at the track seed) for
    /// every kit placement whose piece `registry` has a behaviour for. The hopper placement's
    /// instance gets the track's queue and release mode ([`behaviour::hopper_config`]).
    pub fn with_mechanisms(
        track: &Track,
        kit: &KitCatalog,
        registry: &MechanismRegistry,
    ) -> Result<Self, TrackRunError> {
        let mut sim = Sim::empty();
        let mut mechanisms = Vec::new();
        sim.world.colliders.insert(scene::tray_collider());
        let mut placed = 0usize;
        let mut hopper = None;
        for (i, p) in track.placements.iter().enumerate() {
            match kit_binding(kit, &p.piece) {
                None => {
                    return Err(TrackRunError::UnknownPiece {
                        placement: i,
                        piece: p.piece.clone(),
                    });
                }
                Some(KitBinding::NoCollider { .. }) => {}
                Some(KitBinding::Kit(piece)) => {
                    let kp = Placement { piece, ..p.clone() };
                    let is_hopper = track.hopper.placement == u32::try_from(i).ok()
                        && kp.piece.as_str() == behaviour::HOPPER;
                    if is_hopper {
                        hopper = Some(kp.clone());
                    }
                    if registry.get(&kp.piece).is_some() {
                        let mut mp = kp.clone();
                        if is_hopper {
                            mp.config.extend(behaviour::hopper_config(
                                track.hopper.release,
                                track.hopper.queue.len(),
                            ));
                        }
                        mechanisms.push((i, mp));
                    }
                    sim.place_piece(kit, kp)
                        .map_err(|error| TrackRunError::Chunk {
                            placement: Some(i),
                            error,
                        })?;
                    placed += 1;
                }
            }
        }
        // A piece-free world never rebuilds: rebuilding switches off contact clustering, which
        // would move tray-only runs off the ramp golden.
        if placed > 0 {
            sim.rebuild_chunks(kit)
                .map_err(|error| TrackRunError::Chunk {
                    placement: None,
                    error,
                })?;
        }
        for (i, p) in mechanisms {
            sim.add_registered(registry, kit, &p, track.seed)
                .map_err(|error| TrackRunError::Mechanism {
                    placement: i,
                    error,
                })?;
        }
        let mut run = Self {
            sim,
            releases: release_schedule(track),
            next: 0,
            hopper,
        };
        run.release_due();
        run.sim.sample();
        Ok(run)
    }

    /// Advance `n` fixed steps, releasing marbles whose tick has come before each step.
    pub fn tick(&mut self, n: u32) {
        for _ in 0..n {
            self.release_due();
            self.sim.tick(1);
        }
    }

    /// [`Sim::set_event_recording`]: read-only telemetry, so the run is unchanged.
    pub fn set_event_recording(&mut self, on: bool) {
        self.sim.set_event_recording(on);
    }

    /// [`Sim::drain_events`].
    pub fn drain_events(&mut self) -> Vec<crate::mechanism::SimEvent> {
        self.sim.drain_events()
    }

    fn release_due(&mut self) {
        while let Some(&(at, size)) = self.releases.get(self.next) {
            if at > self.sim.tick {
                break;
            }
            self.spawn(size);
            self.next += 1;
        }
    }

    fn spawn(&mut self, size: MarbleSize) {
        let r = marble_radius_m(size);
        let (at, vel) = match &self.hopper {
            Some(h) => {
                let o: Orientation = h.orientation();
                let [x, y, z] = HOPPER_BOWL_UM;
                let r_um = match size {
                    MarbleSize::Mini => MARBLE_MINI_DIAMETER_UM,
                    MarbleSize::Standard => MARBLE_STANDARD_DIAMETER_UM,
                    MarbleSize::Shooter => MARBLE_SHOOTER_DIAMETER_UM,
                } / 2;
                let p = place_point_um(o, h.anchor, [x, y + r_um, z]);
                let d = place_point_um(o, CellCoord::new(0, 0, 0), [1, 0, 0]);
                let m = |v: i64| (v as f64 * 1.0e-6) as f32;
                let dir = Vector::new(d[0] as f32, d[1] as f32, d[2] as f32);
                (
                    Vector::new(m(p[0]), m(p[1]), m(p[2])),
                    dir * HOPPER_KICK_M_S,
                )
            }
            None => {
                let x = r + 0.004;
                let surface_y = scene::ramp_height_m() * (1.0 - x / scene::ramp_run_m());
                (Vector::new(x, surface_y + 1.25 * r, 0.0), Vector::ZERO)
            }
        };
        let mut body = RigidBodyBuilder::dynamic()
            .translation(at)
            .ccd_enabled(true)
            .can_sleep(false)
            .build();
        if self.hopper.is_some() {
            body.set_linvel(vel, true);
            body.set_angvel(
                Vector::Y.cross(vel / HOPPER_KICK_M_S) * (HOPPER_KICK_M_S / r),
                true,
            );
        }
        let collider = ColliderBuilder::ball(r)
            .density(scene::GLASS_DENSITY)
            .friction(scene::GLASS_FRICTION)
            .restitution(scene::GLASS_RESTITUTION)
            .build();
        let w = &mut self.sim.world;
        let handle = w.bodies.insert(body);
        w.colliders
            .insert_with_parent(collider, handle, &mut w.bodies);
        self.sim.marbles.push(handle);
    }
}

/// Editing a run's pieces in place (P3-T4): the editor keeps the live colliders in step with its
/// track one changed placement at a time and rebuilds only the touched chunks. Placements go
/// straight to the chunk colliders, so ids must already be kit ids ([`kit_binding`]).
impl TrackRun {
    pub fn place_piece(&mut self, kit: &KitCatalog, p: Placement) -> Result<(), ChunkError> {
        self.sim.place_piece(kit, p)
    }

    pub fn remove_piece(
        &mut self,
        anchor: parble_core::CellCoord,
    ) -> Result<Placement, ChunkError> {
        self.sim.remove_piece(anchor)
    }

    /// [`Sim::rebuild_next_chunk`].
    pub fn rebuild_next_chunk(
        &mut self,
        kit: &KitCatalog,
    ) -> Result<Option<parble_core::CellCoord>, ChunkError> {
        self.sim.rebuild_next_chunk(kit)
    }

    /// [`Sim::rebuild_step_chunk`].
    pub fn rebuild_step_chunk(
        &mut self,
        kit: &KitCatalog,
    ) -> Result<Option<(parble_core::CellCoord, bool)>, ChunkError> {
        self.sim.rebuild_step_chunk(kit)
    }
}

impl core::ops::Deref for TrackRun {
    type Target = Sim;

    fn deref(&self) -> &Sim {
        &self.sim
    }
}

/// Run `track` for `ticks` and return its trajectory hash.
pub fn run_hash(track: &Track, ticks: u32) -> Result<u64, TrackRunError> {
    let mut run = TrackRun::new(track)?;
    run.tick(ticks);
    Ok(run.trajectory_hash())
}

#[cfg(test)]
mod tests {
    use super::*;
    use parble_core::{HopperConfig, ReleaseMode};

    fn track(queue: Vec<MarbleSize>, release: ReleaseMode) -> Track {
        let mut t = Track::new(1);
        t.hopper = HopperConfig {
            placement: None,
            queue,
            release,
        };
        t
    }

    #[test]
    fn schedules_follow_the_release_mode() {
        use MarbleSize::*;
        let q = vec![Standard, Mini, Shooter];
        assert_eq!(
            release_schedule(&track(q.clone(), ReleaseMode::One)),
            [(0, Standard)]
        );
        assert_eq!(
            release_schedule(&track(q.clone(), ReleaseMode::All)),
            [(0, Standard), (48, Mini), (96, Shooter)]
        );
        assert_eq!(
            release_schedule(&track(q, ReleaseMode::Interval { ticks: 10 })),
            [(0, Standard), (10, Mini), (20, Shooter)]
        );
    }

    #[test]
    fn marbles_enter_on_their_tick_and_runs_repeat() {
        let t = track(
            vec![MarbleSize::Standard, MarbleSize::Mini],
            ReleaseMode::Interval { ticks: 5 },
        );
        let mut run = TrackRun::new(&t).unwrap();
        assert_eq!(run.marbles().len(), 1);
        run.tick(5);
        assert_eq!(run.marbles().len(), 1);
        run.tick(1);
        assert_eq!(run.marbles().len(), 2);
        assert_eq!(run_hash(&t, 600), run_hash(&t, 600));
        assert_ne!(
            run_hash(&t, 600),
            run_hash(&track(vec![MarbleSize::Standard], ReleaseMode::One), 600)
        );
    }

    #[test]
    fn one_standard_marble_matches_the_ramp_golden() {
        let t = track(vec![MarbleSize::Standard], ReleaseMode::One);
        assert_eq!(
            run_hash(&t, crate::golden::RAMP_10S_TICKS),
            Ok(crate::golden::ramp_10s_hash())
        );
    }

    fn id(s: &str) -> PieceId {
        PieceId::new(s).unwrap()
    }

    fn with_pieces(pieces: &[(&str, [i32; 3], u8)]) -> Track {
        let mut t = track(vec![MarbleSize::Standard], ReleaseMode::One);
        t.placements = pieces
            .iter()
            .map(|&(p, [x, y, z], yaw)| {
                Placement::new(
                    id(p),
                    parble_core::CellCoord::new(x, y, z),
                    parble_core::Yaw4::new(yaw),
                    parble_core::Mirror::Off,
                )
            })
            .collect();
        t
    }

    #[test]
    fn every_alias_names_a_shipped_kit_piece_and_no_collider_ids_are_not_kit_ids() {
        let kit = parble_kit::builtin();
        for &(from, to) in KIT_ALIASES {
            assert!(kit.piece(&id(from)).is_none(), "{from} is itself a kit id");
            assert_eq!(kit_binding(kit, &id(from)), Some(KitBinding::Kit(id(to))));
        }
        for &(from, _) in NO_COLLIDER {
            assert!(kit.piece(&id(from)).is_none(), "{from} has a kit piece");
            assert!(matches!(
                kit_binding(kit, &id(from)),
                Some(KitBinding::NoCollider { .. })
            ));
        }
        for p in kit
            .iter()
            .filter(|p| !NO_COLLIDER.iter().any(|(f, _)| *f == p.def.id.as_str()))
        {
            assert_eq!(
                kit_binding(kit, &p.def.id),
                Some(KitBinding::Kit(p.def.id.clone()))
            );
        }
    }

    #[test]
    fn unknown_ids_and_overlaps_refuse_the_run() {
        let t = with_pieces(&[
            ("blk_straight_l3", [4, 0, 0], 0),
            ("blk_spiral_l1", [5, 0, 0], 0),
        ]);
        assert_eq!(
            TrackRun::new(&t).err(),
            Some(TrackRunError::UnknownPiece {
                placement: 1,
                piece: id("blk_spiral_l1")
            })
        );
        let t = with_pieces(&[("blk_straight", [4, 0, 0], 0), ("blk_end_l3", [4, 0, 0], 0)]);
        assert!(matches!(
            TrackRun::new(&t).err(),
            Some(TrackRunError::Chunk {
                placement: Some(1),
                error: ChunkError::Overlap(_)
            })
        ));
    }

    #[test]
    fn placed_pieces_become_chunk_colliders_the_marble_meets() {
        let bare = with_pieces(&[("rail_curve_90", [0, 4, 0], 0)]);
        let run = TrackRun::new(&bare).unwrap();
        assert_eq!(run.sim.pieces.chunks().count(), 0);
        assert_eq!(
            run_hash(&bare, crate::golden::RAMP_10S_TICKS),
            Ok(crate::golden::ramp_10s_hash())
        );

        // An end stop on the tray floor with its closed end (yaw 2: port on +X) facing the ramp.
        // Cell 6 spans x 330..390 mm.
        let stop = with_pieces(&[
            ("rail_curve_90", [0, 4, 0], 0),
            ("blk_finish", [6, 0, 0], 2),
        ]);
        let mut run = TrackRun::new(&stop).unwrap();
        assert_eq!(run.sim.pieces.chunks().count(), 1);
        assert_ne!(run_hash(&stop, 2_400), run_hash(&bare, 2_400));
        let mut free = TrackRun::new(&bare).unwrap();
        let mut furthest = [0.0f32; 2];
        for _ in 0..2_400 {
            run.tick(1);
            free.tick(1);
            furthest[0] = furthest[0].max(run.marbles()[0].position[0]);
            furthest[1] = furthest[1].max(free.marbles()[0].position[0]);
        }
        assert!(
            furthest[0] < 0.330,
            "marble passed the end stop: x = {}",
            furthest[0]
        );
        assert!(
            furthest[1] > 0.390,
            "tray-only marble stopped early: x = {}",
            furthest[1]
        );
    }
}
