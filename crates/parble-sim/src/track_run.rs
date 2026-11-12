//! Running a loaded [`Track`] (P3-T3, P2-T5).
//!
//! A track runs on the open track floor ([`scene::track_floor_collider`]) with its placements
//! merged in as kit chunk colliders
//! ([`crate::chunk`], [`Sim::place_piece`]). Every placement id goes through [`kit_binding`]: a
//! [`parble_kit::builtin`] id places that piece, an id in [`KIT_ALIASES`] places its kit piece, an
//! id in [`NO_COLLIDER`] is a piece without a static collider yet, and any other id refuses the
//! run. Each placement whose piece has a behaviour in the registry ([`behaviour::registry`] for
//! [`TrackRun::new`]) also gets a [`crate::mechanism`] instance rooted at the track seed: its
//! sensors and its kinematic parts at the sidecar pivots.
//!
//! Marbles spawn in the bowl of the hopper placement ([`parble_core::HopperConfig::placement`])
//! when it is a `mech_hopper`, rolling toward its exit at [`HOPPER_KICK_M_S`], and the hopper's
//! gate opens for each one on its release tick. Without a hopper they spawn in the air where the P0
//! tray ramp's top was and drop onto the floor.
//!
//! In [`ReleaseMode::Sequential`] (the default) the next marble spawns only once no released
//! marble is still moving ([`MarbleMotion`]): each one is counted at a finish, lost below
//! [`LOST_Y_UM`], off the track on the floor, or slower than [`SETTLE_SPEED_UM_S`] for
//! [`SETTLE_HOLD_TICKS`] ticks in a row.
//!
//! Releases happen between steps, on whole ticks, so a run is a pure function of the track and the
//! tick count.

use crate::behaviour::{self, release_ticks};
use crate::chunk::{ChunkError, place_point_um};
use crate::hash::{POSITION_QUANTA_PER_M, VELOCITY_QUANTA_PER_M_PER_S, quantize};
use crate::marble::MarbleKind;
use crate::mechanism::{MechanismError, MechanismRegistry};
use crate::{Sim, scene};
use parble_core::contract::{
    MARBLE_MINI_DIAMETER_UM, MARBLE_SHOOTER_DIAMETER_UM, MARBLE_STANDARD_DIAMETER_UM,
};
use parble_core::{
    CellCoord, MarbleSize, MechConfig, Orientation, PieceId, Placement, ReleaseMode, Track,
};
use parble_kit::KitCatalog;
use rapier3d::prelude::*;

/// `.parble` piece ids written before the kit naming of contract §10, and the kit piece each one
/// runs as. Saved tracks keep these ids (contract §10: ids are stable forever).
pub const KIT_ALIASES: &[(&str, &str)] = &[
    ("blk_straight", "blk_straight_l3"),
    ("blk_finish", "mech_finish"),
    ("mech_toggle", "mech_toggle_l3"),
    ("rail_curve_90", "rail_curve90_r90_l2"),
];

/// Piece ids that are valid in a track but take no part in the run yet, with the reason. This list
/// wins over the kit.
pub const NO_COLLIDER: &[(&str, &str)] = &[];

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

/// Sequential release: a marble slower than this, µm/s (5 mm/s), is still. Compared as the squared
/// speed of the velocity quantized like the trajectory hash, so the test is integer-exact.
pub const SETTLE_SPEED_UM_S: i64 = 5_000;
/// Sequential release: ticks in a row a marble must stay still to count as at rest (0.5 s), long
/// enough that the top of a bounce or a rocker's turn does not count.
pub const SETTLE_HOLD_TICKS: u32 = 120;
/// Sequential release: a marble whose centre is below this height, µm (1 m under the floor),
/// has fallen off the world and is lost.
pub const LOST_Y_UM: i64 = -1_000_000;
/// Sequential release: a marble whose centre is within this distance of its resting height on the
/// floor (one radius), µm, and over no placement's footprint, has left the track onto the floor.
pub const OFF_TRACK_CLEARANCE_UM: i64 = 1_000;

/// Whether a released marble still holds back a sequential release, and if not, why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarbleMotion {
    /// Moving: `still` ticks in a row below [`SETTLE_SPEED_UM_S`] so far, fewer than
    /// [`SETTLE_HOLD_TICKS`].
    Moving { still: u32 },
    /// Below [`SETTLE_SPEED_UM_S`] for [`SETTLE_HOLD_TICKS`] ticks. Moves again if knocked.
    AtRest,
    /// On the floor ([`OFF_TRACK_CLEARANCE_UM`]) over no placement's footprint: it has left
    /// the track, rolling or not. A marble on the open floor need not stop for the run to go on
    /// (a marble without rolling resistance rolls on for tens of seconds). Moves again if it
    /// climbs back onto a piece.
    OffTrack,
    /// Counted by a finish cup. Final.
    Finished,
    /// Fell below [`LOST_Y_UM`]. Final.
    Lost,
}

impl MarbleMotion {
    pub fn is_moving(self) -> bool {
        matches!(self, Self::Moving { .. })
    }
}

/// Radius in µm of a marble size (contract §6).
fn marble_radius_um(size: MarbleSize) -> i32 {
    let d = match size {
        MarbleSize::Mini => MARBLE_MINI_DIAMETER_UM,
        MarbleSize::Standard => MARBLE_STANDARD_DIAMETER_UM,
        MarbleSize::Shooter => MARBLE_SHOOTER_DIAMETER_UM,
    };
    d / 2
}

/// Radius in metres of a marble size (contract §6).
pub fn marble_radius_m(size: MarbleSize) -> f32 {
    scene::um_to_m(marble_radius_um(size))
}

/// The sim's [`crate::marble::MarbleSize`] for a track's queued size.
fn sim_size(size: MarbleSize) -> crate::marble::MarbleSize {
    match size {
        MarbleSize::Mini => crate::marble::MarbleSize::Mini,
        MarbleSize::Standard => crate::marble::MarbleSize::Standard,
        MarbleSize::Shooter => crate::marble::MarbleSize::Shooter,
    }
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
    /// The whole queue in [`ReleaseMode::Sequential`] runs, released from `next`; empty otherwise.
    sequential: Vec<MarbleSize>,
    /// Per released marble in [`ReleaseMode::Sequential`] runs; empty otherwise.
    motion: Vec<MarbleMotion>,
    /// The hopper placement marbles spawn in, if the track has one.
    hopper: Option<Placement>,
    /// Plan cell columns `(x, z)` under every placement's footprint.
    footprint: std::collections::BTreeSet<(i32, i32)>,
    /// The marble type every release spawns as ([`TrackRun::with_marble`]); `None` spawns the
    /// legacy glass marble the track goldens were recorded with.
    kind: Option<MarbleKind>,
    /// The track seed mechanism instances are rooted at, for live edits.
    seed: u64,
    /// The hopper's release mode and queue length, when the run releases from a hopper.
    hopper_queue: Option<(ReleaseMode, usize)>,
    /// The state of the run's hopper since a live edit removed it ([`TrackRun::remove_mechanism`]):
    /// no marble spawns until a hopper is placed again and takes this state over.
    hopper_gone: Option<MechConfig>,
}

impl TrackRun {
    /// A run with no releases (the P0 scenarios).
    pub fn from_sim(sim: Sim) -> Self {
        Self {
            sim,
            releases: Vec::new(),
            next: 0,
            sequential: Vec::new(),
            motion: Vec::new(),
            hopper: None,
            footprint: Default::default(),
            kind: None,
            seed: 0,
            hopper_queue: None,
            hopper_gone: None,
        }
    }

    /// `track` on the track floor with its placements as [`parble_kit::builtin`] chunk colliders and the
    /// kit behaviours ([`behaviour::registry`]), at tick 0 with its tick-0 releases in and sampled.
    pub fn new(track: &Track) -> Result<Self, TrackRunError> {
        Self::with_kit(track, parble_kit::builtin())
    }

    /// [`TrackRun::new`] with every queued marble spawned as `kind` at its queued size (P5-T1), with
    /// the type's physics table entry ([`MarbleKind::spec_sized`]).
    pub fn with_marble(track: &Track, kind: MarbleKind) -> Result<Self, TrackRunError> {
        Self::build(
            track,
            parble_kit::builtin(),
            &behaviour::registry(),
            Some(kind),
        )
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
        Self::build(track, kit, registry, None)
    }

    fn build(
        track: &Track,
        kit: &KitCatalog,
        registry: &MechanismRegistry,
        kind: Option<MarbleKind>,
    ) -> Result<Self, TrackRunError> {
        let mut sim = Sim::empty();
        sim.world.colliders.insert(scene::track_floor_collider());
        let mut plan = Plan::default();
        for i in 0..track.placements.len() {
            plan.place(&mut sim, kit, registry, track, i)?;
        }
        // A piece-free world never rebuilds: rebuilding switches off contact clustering, which
        // would change piece-free runs for nothing.
        if plan.placed > 0 {
            sim.rebuild_chunks(kit)
                .map_err(|error| TrackRunError::Chunk {
                    placement: None,
                    error,
                })?;
        }
        for (i, p) in &plan.mechanisms {
            add_mechanism(&mut sim, kit, registry, *i, p, track.seed)?;
        }
        Ok(Self::finish(sim, track, plan, kind))
    }

    /// The run of a world with every collider and mechanism in: the release schedule, then the
    /// tick-0 releases and sample.
    fn finish(mut sim: Sim, track: &Track, plan: Plan, kind: Option<MarbleKind>) -> Self {
        sim.set_nudge_zones(&track.nudge_zones);
        let sequential = track.hopper.release == ReleaseMode::Sequential;
        let has_queue = plan.hopper.is_some() && !track.hopper.queue.is_empty();
        let mut run = Self {
            sim,
            releases: if sequential {
                Vec::new()
            } else {
                release_schedule(track)
            },
            next: 0,
            sequential: if sequential {
                track.hopper.queue.clone()
            } else {
                Vec::new()
            },
            motion: Vec::new(),
            hopper: plan.hopper,
            footprint: plan.footprint,
            kind,
            seed: track.seed,
            hopper_queue: has_queue.then_some((track.hopper.release, track.hopper.queue.len())),
            hopper_gone: None,
        };
        run.release_due();
        run.sim.sample();
        run
    }

    /// Advance `n` fixed steps, releasing marbles whose tick has come before each step.
    pub fn tick(&mut self, n: u32) {
        for _ in 0..n {
            self.release_due();
            self.sim.tick(1);
            self.update_motion();
        }
    }

    /// [`MarbleMotion`] of each released marble in spawn order, in [`ReleaseMode::Sequential`]
    /// runs (empty in the other modes).
    pub fn motion(&self) -> &[MarbleMotion] {
        &self.motion
    }

    /// [`Sim::set_event_recording`]: read-only telemetry, so the run is unchanged.
    pub fn set_event_recording(&mut self, on: bool) {
        self.sim.set_event_recording(on);
    }

    /// [`Sim::drain_events`].
    pub fn drain_events(&mut self) -> Vec<crate::mechanism::SimEvent> {
        self.sim.drain_events()
    }

    fn update_motion(&mut self) {
        if self.motion.is_empty() {
            return;
        }
        let mut finished = vec![false; self.motion.len()];
        for v in self.sim.mechanisms() {
            if v.piece.as_str() == behaviour::FINISH {
                for (m, _) in behaviour::Finish::arrivals(&v.state) {
                    if let Some(f) = finished.get_mut(m) {
                        *f = true;
                    }
                }
            }
        }
        let marbles = self.sim.marbles();
        let cell = f64::from(parble_core::contract::CELL_UM) * 1.0e-6;
        let off_track = |s: &crate::MarbleState, size: MarbleSize| {
            let rest = i64::from(marble_radius_um(size));
            let column = |v: f32| libm::floor(f64::from(v) / cell + 0.5) as i32;
            (quantize(s.position[1], POSITION_QUANTA_PER_M) - rest).abs() <= OFF_TRACK_CLEARANCE_UM
                && !self
                    .footprint
                    .contains(&(column(s.position[0]), column(s.position[2])))
        };
        for (((m, s), f), &size) in self
            .motion
            .iter_mut()
            .zip(&marbles)
            .zip(finished)
            .zip(&self.sequential)
        {
            if matches!(m, MarbleMotion::Finished | MarbleMotion::Lost) {
                continue;
            }
            let speed2 = s
                .linvel
                .iter()
                .map(|&c| quantize(c, VELOCITY_QUANTA_PER_M_PER_S))
                .fold(0i64, |a, c| a.saturating_add(c.saturating_mul(c)));
            *m = if f {
                MarbleMotion::Finished
            } else if quantize(s.position[1], POSITION_QUANTA_PER_M) < LOST_Y_UM {
                MarbleMotion::Lost
            } else if off_track(s, size) {
                MarbleMotion::OffTrack
            } else if speed2 >= SETTLE_SPEED_UM_S * SETTLE_SPEED_UM_S {
                MarbleMotion::Moving { still: 0 }
            } else {
                match *m {
                    MarbleMotion::Moving { still } if still + 1 < SETTLE_HOLD_TICKS => {
                        MarbleMotion::Moving { still: still + 1 }
                    }
                    _ => MarbleMotion::AtRest,
                }
            };
        }
    }

    fn release_due(&mut self) {
        if self.hopper_gone.is_some() {
            return;
        }
        if let Some(&size) = self.sequential.get(self.next) {
            if self.motion.iter().all(|m| !m.is_moving()) {
                self.spawn(size);
                self.next += 1;
                self.motion.push(MarbleMotion::Moving { still: 0 });
            }
            return;
        }
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
        let (mut body, collider) = match self.kind {
            Some(kind) => kind.spec_sized(sim_size(size)).build(at),
            None => (
                RigidBodyBuilder::dynamic()
                    .translation(at)
                    .ccd_enabled(true)
                    .can_sleep(false)
                    .build(),
                ColliderBuilder::ball(r)
                    .density(scene::GLASS_DENSITY)
                    .friction(scene::GLASS_FRICTION)
                    .restitution(scene::GLASS_RESTITUTION)
                    .build(),
            ),
        };
        if self.hopper.is_some() {
            body.set_linvel(vel, true);
            body.set_angvel(
                Vector::Y.cross(vel / HOPPER_KICK_M_S) * (HOPPER_KICK_M_S / r),
                true,
            );
        }
        let w = &mut self.sim.world;
        let handle = w.bodies.insert(body);
        w.colliders
            .insert_with_parent(collider, handle, &mut w.bodies);
        self.sim.marbles.push(handle);
    }
}

/// What a build gathered from the placements so far: the mechanisms to add, the hopper, the
/// footprint and how many pieces have colliders.
#[derive(Default)]
struct Plan {
    mechanisms: Vec<(usize, Placement)>,
    hopper: Option<Placement>,
    footprint: std::collections::BTreeSet<(i32, i32)>,
    placed: usize,
}

impl Plan {
    /// Bind placement `i` of `track` and place its chunk collider.
    fn place(
        &mut self,
        sim: &mut Sim,
        kit: &KitCatalog,
        registry: &MechanismRegistry,
        track: &Track,
        i: usize,
    ) -> Result<(), TrackRunError> {
        let p = &track.placements[i];
        match kit_binding(kit, &p.piece) {
            None => Err(TrackRunError::UnknownPiece {
                placement: i,
                piece: p.piece.clone(),
            }),
            Some(KitBinding::NoCollider { .. }) => Ok(()),
            Some(KitBinding::Kit(piece)) => {
                let kp = Placement { piece, ..p.clone() };
                if let Some(k) = kit.piece(&kp.piece) {
                    self.footprint
                        .extend(kp.world_cells(&k.def).iter().map(|c| (c.x, c.z)));
                }
                let is_hopper = track.hopper.placement == u32::try_from(i).ok()
                    && kp.piece.as_str() == behaviour::HOPPER;
                if is_hopper {
                    self.hopper = Some(kp.clone());
                }
                if registry.get(&kp.piece).is_some() {
                    let mut mp = kp.clone();
                    if is_hopper {
                        mp.config.extend(behaviour::hopper_config(
                            track.hopper.release,
                            track.hopper.queue.len(),
                        ));
                    }
                    self.mechanisms.push((i, mp));
                }
                sim.place_piece(kit, kp)
                    .map_err(|error| TrackRunError::Chunk {
                        placement: Some(i),
                        error,
                    })?;
                self.placed += 1;
                Ok(())
            }
        }
    }
}

fn add_mechanism(
    sim: &mut Sim,
    kit: &KitCatalog,
    registry: &MechanismRegistry,
    i: usize,
    p: &Placement,
    seed: u64,
) -> Result<(), TrackRunError> {
    sim.add_registered(registry, kit, p, seed)
        .map(drop)
        .map_err(|error| TrackRunError::Mechanism {
            placement: i,
            error,
        })
}

/// Placements bound and placed per [`TrackRunBuild::step`] unit.
pub const BUILD_PLACE_PIECES: usize = 64;

/// Where a [`TrackRunBuild`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildStage {
    /// Binding and placing placements from this index on, [`BUILD_PLACE_PIECES`] per unit.
    Place(usize),
    /// One stage of one chunk collider per unit ([`Sim::rebuild_step_chunk`]).
    Colliders,
    /// One mechanism instance per unit, from this index of the gathered mechanisms on.
    Mechanisms(usize),
    /// Every unit is done: [`TrackRunBuild::finish`] hands the run over.
    Done,
}

/// [`TrackRun::new`] (or [`TrackRun::with_mechanisms`], [`TrackRun::with_marble`]) built one
/// bounded unit of work at a time (P3-T5), so the editor can spread a large track's build over
/// frames. The units make the same world changes in the same order however the calls split them,
/// and the chunk colliders come from the staged [`Sim::rebuild_step_chunk`], which installs the
/// colliders [`Sim::rebuild_chunks`] does, so the run's trajectory and mechanism hashes are
/// [`TrackRun::new`]'s. Units take no wall-clock input; a caller with a frame budget times them.
pub struct TrackRunBuild<'k> {
    kit: &'k KitCatalog,
    registry: MechanismRegistry,
    track: Track,
    kind: Option<MarbleKind>,
    sim: Sim,
    plan: Plan,
    stage: BuildStage,
    /// Chunk colliders installed so far, and the dirty chunks when the collider stage began.
    chunks: (usize, usize),
}

impl TrackRunBuild<'static> {
    /// The build of [`TrackRun::new`]`(track)`, with no work done yet.
    pub fn new(track: &Track) -> Self {
        Self::with_mechanisms(track, parble_kit::builtin(), &behaviour::registry(), None)
    }
}

impl<'k> TrackRunBuild<'k> {
    /// The build of [`TrackRun::with_mechanisms`], spawning `kind` as [`TrackRun::with_marble`]
    /// does when set.
    pub fn with_mechanisms(
        track: &Track,
        kit: &'k KitCatalog,
        registry: &MechanismRegistry,
        kind: Option<MarbleKind>,
    ) -> Self {
        let mut sim = Sim::empty();
        sim.world.colliders.insert(scene::track_floor_collider());
        Self {
            kit,
            registry: registry.clone(),
            track: track.clone(),
            kind,
            sim,
            plan: Plan::default(),
            stage: BuildStage::Place(0),
            chunks: (0, 0),
        }
    }

    pub fn stage(&self) -> BuildStage {
        self.stage
    }

    pub fn is_done(&self) -> bool {
        self.stage == BuildStage::Done
    }

    /// The track being built.
    pub fn track(&self) -> &Track {
        &self.track
    }

    /// Rough share of the work done, 0 to 1: placing is the first 5 %, the chunk colliders (by
    /// chunks installed) the next 90 %, the mechanisms the last 5 %.
    pub fn progress(&self) -> f32 {
        let part = |done: usize, total: usize| {
            if total == 0 {
                1.0
            } else {
                done as f32 / total as f32
            }
        };
        match self.stage {
            BuildStage::Place(i) => 0.05 * part(i, self.track.placements.len()),
            BuildStage::Colliders => 0.05 + 0.9 * part(self.chunks.0, self.chunks.1),
            BuildStage::Mechanisms(i) => 0.95 + 0.05 * part(i, self.plan.mechanisms.len()),
            BuildStage::Done => 1.0,
        }
    }

    /// Do one unit of work and return whether the build is done. After an error the build is
    /// spent: the track cannot run.
    pub fn step(&mut self) -> Result<bool, TrackRunError> {
        match self.stage {
            BuildStage::Place(from) => {
                let to = (from + BUILD_PLACE_PIECES).min(self.track.placements.len());
                for i in from..to {
                    self.plan
                        .place(&mut self.sim, self.kit, &self.registry, &self.track, i)?;
                }
                self.stage = if to < self.track.placements.len() {
                    BuildStage::Place(to)
                } else if self.plan.placed > 0 {
                    // As in [`TrackRun::new`]: a piece-free world never rebuilds.
                    self.chunks = (0, self.sim.chunk_colliders().dirty_chunks().count());
                    BuildStage::Colliders
                } else {
                    BuildStage::Mechanisms(0)
                };
            }
            BuildStage::Colliders => {
                let step = self.sim.rebuild_step_chunk(self.kit).map_err(|error| {
                    TrackRunError::Chunk {
                        placement: None,
                        error,
                    }
                })?;
                match step {
                    Some((_, installed)) => self.chunks.0 += usize::from(installed),
                    None => self.stage = BuildStage::Mechanisms(0),
                }
            }
            BuildStage::Mechanisms(i) => match self.plan.mechanisms.get(i) {
                Some((at, p)) => {
                    add_mechanism(
                        &mut self.sim,
                        self.kit,
                        &self.registry,
                        *at,
                        p,
                        self.track.seed,
                    )?;
                    self.stage = BuildStage::Mechanisms(i + 1);
                }
                None => self.stage = BuildStage::Done,
            },
            BuildStage::Done => {}
        }
        Ok(self.is_done())
    }

    /// The run, doing whatever work is left first.
    pub fn finish(mut self) -> Result<TrackRun, TrackRunError> {
        while !self.step()? {}
        Ok(TrackRun::finish(
            self.sim,
            &self.track,
            self.plan,
            self.kind,
        ))
    }
}

/// Live mechanism edits (P3-T5): the editor adds and removes a placement's mechanism instance on a
/// running run as its pieces change, so a mechanism moved mid-run works at its new cell. Only the
/// editor calls these; a track's own runs (and their goldens) never do.
impl TrackRun {
    /// Add the mechanism of kit placement `p` if `registry` has a behaviour for its piece, rooted
    /// at the track seed as the build roots it. A hopper placed while the run's hopper is gone
    /// becomes the run's hopper: it gets the queue and release mode, takes the removed hopper's
    /// state over (released marbles, gate angle) and marbles spawn from it again.
    pub fn add_mechanism(
        &mut self,
        kit: &KitCatalog,
        registry: &MechanismRegistry,
        p: &Placement,
    ) -> Result<Option<crate::mechanism::InstanceId>, MechanismError> {
        if registry.get(&p.piece).is_none() {
            return Ok(None);
        }
        let takes_over = p.piece.as_str() == behaviour::HOPPER && self.hopper_gone.is_some();
        let mut mp = p.clone();
        if takes_over && let Some((release, n)) = self.hopper_queue {
            mp.config.extend(behaviour::hopper_config(release, n));
        }
        let id = self.sim.add_registered(registry, kit, &mp, self.seed)?;
        if takes_over && let Some(state) = self.hopper_gone.take() {
            let mut one = Track::new(self.seed);
            one.placements = vec![mp];
            self.sim.restore_mechanism_states(
                &one,
                &[parble_core::MechanismState {
                    placement: 0,
                    state,
                }],
            );
            self.hopper = Some(p.clone());
        }
        Ok(id)
    }

    /// Remove the mechanism instance anchored at `anchor` and return whether there was one.
    /// Removing the run's hopper holds every release until a hopper is placed again.
    pub fn remove_mechanism(&mut self, anchor: CellCoord) -> bool {
        let Some((_, state)) = self.sim.remove_mechanism(anchor) else {
            return false;
        };
        if self.hopper.as_ref().is_some_and(|h| h.anchor == anchor) {
            self.hopper = None;
            self.hopper_gone = Some(state);
        }
        true
    }
}

/// Editing a run's pieces in place (P3-T4): the editor keeps the live colliders in step with its
/// track one changed placement at a time and rebuilds only the touched chunks. Placements go
/// straight to the chunk colliders, so ids must already be kit ids ([`kit_binding`]).
impl TrackRun {
    /// [`Sim::set_nudge_zones`]: the editor hands a live run the track's zones after an edit.
    pub fn set_nudge_zones(&mut self, zones: &[parble_core::NudgeZone]) {
        self.sim.set_nudge_zones(zones);
    }

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
    use parble_core::{HopperConfig, MechConfig};

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

    /// No hopper: the marble spawns where the tray ramp's top was and drops onto the open floor
    /// ([`scene::track_floor_collider`]), where it has left the track.
    #[test]
    fn a_hopperless_marble_drops_onto_the_floor_and_is_off_the_track() {
        let t = track(vec![MarbleSize::Standard], ReleaseMode::Sequential);
        let mut run = TrackRun::new(&t).unwrap();
        run.tick(240);
        let m = run.marbles()[0];
        assert!(
            (m.position[1] - marble_radius_m(MarbleSize::Standard)).abs() < 0.001,
            "{:?}",
            m.position
        );
        assert_eq!(run.motion()[0], MarbleMotion::OffTrack);
    }

    /// A hopper at L1 feeding five straights: the run on to the end and off it.
    fn fed_straights(end: bool) -> Track {
        let mut p = vec![(behaviour::HOPPER, [0, 1, 0], 0)];
        p.extend((1..6).map(|x| ("blk_straight_l3", [x, 0, 0], 0)));
        if end {
            // The end stop's closed end (yaw 2: port on +X) faces the run. Cell 6 spans x 330..390 mm.
            p.push(("blk_end_l3", [6, 0, 0], 2));
        }
        let mut t = with_pieces(&p);
        t.hopper.placement = Some(0);
        t
    }

    /// The all-pieces track with a `n`-marble queue released in `release` mode.
    fn allpieces_with(release: ReleaseMode, n: usize) -> Track {
        let mut t = crate::allpieces::track();
        t.hopper.queue.truncate(n);
        t.hopper.release = release;
        t
    }

    /// `(marble, arrival tick)` over every finish cup of the run.
    fn arrivals(run: &TrackRun) -> Vec<(usize, u64)> {
        run.mechanisms()
            .iter()
            .filter(|m| m.piece.as_str() == behaviour::FINISH)
            .flat_map(|m| behaviour::Finish::arrivals(&m.state))
            .collect()
    }

    #[test]
    fn the_default_release_mode_is_sequential() {
        assert_eq!(ReleaseMode::default(), ReleaseMode::Sequential);
        assert_eq!(Track::new(0).hopper.release, ReleaseMode::Sequential);
        let q = vec![MarbleSize::Standard, MarbleSize::Mini];
        assert_eq!(
            release_schedule(&track(q, ReleaseMode::Sequential)),
            [(0, MarbleSize::Standard)],
            "only the first sequential release has a fixed tick"
        );
    }

    #[test]
    fn a_one_marble_sequential_queue_runs_like_release_one() {
        let seq = allpieces_with(ReleaseMode::Sequential, 1);
        let one = allpieces_with(ReleaseMode::One, 1);
        let (mut a, mut b) = (TrackRun::new(&seq).unwrap(), TrackRun::new(&one).unwrap());
        a.tick(2_400);
        b.tick(2_400);
        assert_eq!(a.trajectory_hash(), b.trajectory_hash());
        assert_eq!(a.mechanism_hash(), b.mechanism_hash());
    }

    #[test]
    fn sequential_release_keeps_one_marble_moving_and_all_three_finish() {
        const N: usize = 3;
        let mut run = TrackRun::new(&allpieces_with(ReleaseMode::Sequential, N)).unwrap();
        let mut spawned_at = vec![0u64];
        let min2 = SETTLE_SPEED_UM_S * SETTLE_SPEED_UM_S;
        for _ in 0..12_000 {
            run.tick(1);
            let t = run.tick_count();
            if run.marbles().len() > spawned_at.len() {
                spawned_at.push(t);
            }
            let moving = run.motion().iter().filter(|m| m.is_moving()).count();
            assert!(moving <= 1, "tick {t}: {:?}", run.motion());
            // Independent of the run's own bookkeeping: at most one marble outside a finish cup
            // is above the settle speed.
            let done: Vec<usize> = arrivals(&run).into_iter().map(|(m, _)| m).collect();
            let fast = run
                .marbles()
                .iter()
                .enumerate()
                .filter(|(m, s)| {
                    let v = s.linvel.map(|c| quantize(c, VELOCITY_QUANTA_PER_M_PER_S));
                    !done.contains(m) && v.iter().map(|c| c * c).sum::<i64>() >= min2
                })
                .count();
            assert!(fast <= 1, "tick {t}: {fast} marbles moving outside a cup");
            if done.len() == N {
                break;
            }
        }
        let got = arrivals(&run);
        assert_eq!(run.marbles().len(), N, "the hopper released every marble");
        assert_eq!(got.len(), N, "every marble reached a cup: {got:?}");
        assert!(run.motion().iter().all(|&m| m == MarbleMotion::Finished));
        for (k, &at) in spawned_at.iter().enumerate().skip(1) {
            let prev = got.iter().find(|(m, _)| *m == k - 1).unwrap().1;
            assert!(
                at > prev,
                "marble {k} spawned at tick {at} before marble {} finished at {prev}",
                k - 1
            );
        }
        let hopper = run
            .mechanisms()
            .into_iter()
            .find(|m| m.piece.as_str() == behaviour::HOPPER)
            .unwrap();
        assert_eq!(hopper.state["released"], N as i64);
        assert_eq!(hopper.state["queue"], N as i64);
        eprintln!("sequential spawns {spawned_at:?}, arrivals {got:?}");
    }

    /// Runs `t` until `n` marbles have spawned or `ticks` pass; returns the run and spawn ticks.
    fn spawns(t: &Track, n: usize, ticks: u32) -> (TrackRun, Vec<u64>) {
        let mut run = TrackRun::new(t).unwrap();
        let mut at = vec![0u64];
        for _ in 0..ticks {
            run.tick(1);
            if run.marbles().len() > at.len() {
                at.push(run.tick_count() - 1);
            }
            if at.len() == n {
                break;
            }
        }
        (run, at)
    }

    #[test]
    fn sequential_release_waits_for_a_marble_to_leave_the_track() {
        // No hopper placement: the marbles spawn in the air and drop onto the open floor, and the
        // second spawns only once the first is down there, off the track.
        let t = track(vec![MarbleSize::Standard; 2], ReleaseMode::Sequential);
        let (run, at) = spawns(&t, 2, 20_000);
        assert_eq!(at.len(), 2, "the second marble never spawned");
        println!(
            "sequential spawns {at:?}, first marble {:?}",
            run.motion()[0]
        );
        assert_eq!(run.motion()[0], MarbleMotion::OffTrack);
        let first = run.marbles()[0];
        assert!(
            (first.position[1] - marble_radius_m(MarbleSize::Standard)).abs() < 0.001,
            "spawned at tick {} with the first not yet on the floor at {:?}",
            at[1],
            first.position
        );
        let mut probe = TrackRun::new(&t).unwrap();
        probe.tick(u32::try_from(at[1]).unwrap() - 1);
        assert_eq!(probe.marbles().len(), 1, "the second spawned early");
        assert!(probe.motion()[0].is_moving());
    }

    #[test]
    fn sequential_release_moves_on_from_a_lost_marble() {
        // A hopper far off the tray with nothing after it: its marble falls off the world.
        let mut t = with_pieces(&[(behaviour::HOPPER, [40, 2, 40], 0)]);
        t.hopper = HopperConfig {
            placement: Some(0),
            queue: vec![MarbleSize::Standard; 2],
            release: ReleaseMode::Sequential,
        };
        let (run, at) = spawns(&t, 2, 2_400);
        assert_eq!(at.len(), 2, "the second marble never spawned");
        assert_eq!(run.motion()[0], MarbleMotion::Lost);
    }

    #[test]
    fn timed_modes_keep_their_schedule_and_track_no_motion() {
        for release in [ReleaseMode::All, ReleaseMode::Interval { ticks: 480 }] {
            let mut run = TrackRun::new(&allpieces_with(release, 3)).unwrap();
            let mut spawned_at = vec![0u64];
            for _ in 0..1_000 {
                run.tick(1);
                if run.marbles().len() > spawned_at.len() {
                    spawned_at.push(run.tick_count() - 1);
                }
            }
            let expect: Vec<u64> = release_schedule(&allpieces_with(release, 3))
                .into_iter()
                .map(|(t, _)| t)
                .collect();
            assert_eq!(spawned_at, expect, "{release:?}");
            assert!(run.motion().is_empty(), "{release:?}");
        }
    }

    #[test]
    fn a_sequential_hopper_saves_and_restores() {
        use crate::mechanism::Mechanism;
        let config = MechConfig::from(behaviour::hopper_config(ReleaseMode::Sequential, 4));
        let mut run = TrackRun::new(&allpieces_with(ReleaseMode::Sequential, 4)).unwrap();
        run.tick(6_000);
        let live = run
            .mechanisms()
            .into_iter()
            .find(|m| m.piece.as_str() == behaviour::HOPPER)
            .unwrap()
            .state;
        assert!(live["released"] >= 2, "{live:?}");
        let saved: MechConfig = live
            .iter()
            .filter(|(k, _)| !k.starts_with("part/"))
            .map(|(k, &v)| (k.clone(), v))
            .collect();
        let mut fresh = behaviour::Hopper::new(&config);
        fresh.restore(&saved);
        assert_eq!(fresh.save(), saved);
        assert_eq!(saved["queue"], 4);
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
        let bare = with_pieces(&[]);
        let run = TrackRun::new(&bare).unwrap();
        assert_eq!(run.sim.pieces.chunks().count(), 0);

        // The same fed run with and without an end stop after it.
        let (stop, open) = (fed_straights(true), fed_straights(false));
        let mut run = TrackRun::new(&stop).unwrap();
        assert!(run.sim.pieces.chunks().count() >= 1);
        assert_ne!(run_hash(&stop, 2_400), run_hash(&open, 2_400));
        let mut free = TrackRun::new(&open).unwrap();
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
            "the marble without the stop stopped early: x = {}",
            furthest[1]
        );
    }

    /// `blk_finish` runs as the finish mechanism: the all-pieces track with its cups saved under
    /// the old id runs to the same hash and every marble is counted.
    #[test]
    fn blk_finish_runs_as_mech_finish_and_counts_finishes() {
        let kit = parble_kit::builtin();
        assert_eq!(
            kit_binding(kit, &id("blk_finish")),
            Some(KitBinding::Kit(id(behaviour::FINISH)))
        );
        let shipped = crate::allpieces::track();
        let mut saved = shipped.clone();
        let mut renamed = 0;
        for p in &mut saved.placements {
            if p.piece.as_str() == behaviour::FINISH {
                p.piece = id("blk_finish");
                renamed += 1;
            }
        }
        assert!(renamed > 0);
        let mut run = TrackRun::new(&saved).unwrap();
        run.tick(crate::allpieces::RUN_TICKS);
        let counts = crate::allpieces::finish_counts(&run);
        assert_eq!(
            counts.iter().sum::<usize>(),
            crate::allpieces::MARBLES,
            "cups {counts:?}"
        );
        assert_eq!(
            run.trajectory_hash(),
            run_hash(&shipped, crate::allpieces::RUN_TICKS).unwrap()
        );
    }

    /// `rail_curve_90` runs as the shipped rail curve: its collider is in the run, placed exactly
    /// as the kit id is, and a marble off the end of a fed run meets it.
    #[test]
    fn rail_curve_90_runs_as_the_shipped_rail_curve_with_its_collider() {
        assert!(NO_COLLIDER.is_empty());
        let kit = parble_kit::builtin();
        assert_eq!(
            kit_binding(kit, &id("rail_curve_90")),
            Some(KitBinding::Kit(id("rail_curve90_r90_l2")))
        );
        let with = |piece: &'static str| {
            let mut t = fed_straights(false);
            let one = with_pieces(&[(piece, [6, 0, 0], 0)]);
            t.placements.extend(one.placements);
            t
        };
        let (saved, shipped) = (with("rail_curve_90"), with("rail_curve90_r90_l2"));
        TrackRun::new(&saved).unwrap();
        assert_eq!(run_hash(&saved, 2_400), run_hash(&shipped, 2_400));
        assert_ne!(
            run_hash(&saved, 2_400),
            run_hash(&fed_straights(false), 2_400)
        );
    }

    /// `track` built with [`TrackRunBuild`] one unit per call, and the number of calls.
    fn stepped(track: &Track) -> (TrackRun, usize) {
        let mut b = TrackRunBuild::new(track);
        let mut calls = 0;
        while !b.step().unwrap() {
            calls += 1;
        }
        (b.finish().unwrap(), calls + 1)
    }

    /// What a run looks like after `ticks`: trajectory and mechanism hashes, marbles and every
    /// chunk collider as built.
    fn fingerprint(mut run: TrackRun, ticks: u32) -> (u64, u64, usize, Vec<crate::ChunkGeometry>) {
        let geometry = run.chunk_collider_geometry();
        run.tick(ticks);
        (
            run.trajectory_hash(),
            run.mechanism_hash(),
            run.marbles().len(),
            geometry,
        )
    }

    /// P3-T5: a track's run is the same whether [`TrackRun::new`] builds it in one go, a
    /// [`TrackRunBuild`] finishes it in one call, or the build is stepped one unit at a time over
    /// many calls (the editor's frames): same chunk colliders, same trajectory and mechanism
    /// hashes. Covers the mechanism-heavy all-pieces track, a piece-free track (no rebuild) and a
    /// small mixed track.
    #[test]
    fn a_stepped_build_runs_like_the_one_step_build() {
        let mut mixed = with_pieces(&[
            (behaviour::HOPPER, [0, 1, 0], 0),
            ("blk_straight_l3", [4, 0, 0], 0),
            (behaviour::TOGGLE, [6, 0, 0], 0),
            (behaviour::GATE, [8, 0, 2], 1),
        ]);
        mixed.hopper = HopperConfig {
            placement: Some(0),
            queue: vec![MarbleSize::Standard; 3],
            release: ReleaseMode::Interval { ticks: 120 },
        };
        let cases = [
            ("allpieces", crate::allpieces::track(), 2_400),
            (
                "piece-free",
                track(vec![MarbleSize::Standard], ReleaseMode::One),
                crate::golden::RAMP_10S_TICKS,
            ),
            ("mixed", mixed, 1_200),
        ];
        for (name, t, ticks) in cases {
            let whole = fingerprint(TrackRun::new(&t).unwrap(), ticks);
            let one_call = fingerprint(TrackRunBuild::new(&t).finish().unwrap(), ticks);
            let (run, calls) = stepped(&t);
            let many = fingerprint(run, ticks);
            assert!(calls > 1, "{name}: the build took one unit");
            assert_eq!(one_call, whole, "{name}: one-call build");
            assert_eq!(many, whole, "{name}: stepped over {calls} calls");
            eprintln!("{name}: {calls} units, hash {:016x}", whole.0);
        }
    }

    #[test]
    fn a_stepped_build_reports_its_stages_and_refuses_what_the_run_refuses() {
        let t = crate::allpieces::track();
        let mut b = TrackRunBuild::new(&t);
        assert_eq!(b.stage(), BuildStage::Place(0));
        let mut seen = vec![core::mem::discriminant(&b.stage())];
        let mut last = 0.0;
        while !b.step().unwrap() {
            let stage = core::mem::discriminant(&b.stage());
            if seen.last() != Some(&stage) {
                seen.push(stage);
            }
            assert!(b.progress() >= last, "progress went back");
            last = b.progress();
        }
        assert_eq!(seen.len(), 3, "place, colliders, mechanisms");
        assert_eq!(b.progress(), 1.0);
        let bad = with_pieces(&[
            ("blk_straight_l3", [4, 0, 0], 0),
            ("blk_spiral_l1", [5, 0, 0], 0),
        ]);
        let mut b = TrackRunBuild::new(&bad);
        assert_eq!(
            b.step().err(),
            Some(TrackRunError::UnknownPiece {
                placement: 1,
                piece: id("blk_spiral_l1")
            })
        );
    }

    fn world_counts(run: &TrackRun) -> (usize, usize, usize) {
        let w = &run.sim.world;
        (w.bodies.len(), w.colliders.len(), w.impulse_joints.len())
    }

    /// P3-T5 live edits: removing a mechanism takes its part bodies, colliders, joints and hinge
    /// frames out of the world and adding it back at another cell puts the same back, so the
    /// world returns to its counts; the instance and its sensors follow the move.
    #[test]
    fn live_mechanism_edits_add_and_remove_instances_and_their_bodies() {
        let kit = parble_kit::builtin();
        let registry = behaviour::registry();
        let t = with_pieces(&[
            (behaviour::GATE, [6, 0, 4], 0),
            (behaviour::TOGGLE, [2, 0, 4], 0),
            ("blk_straight_l3", [4, 0, 4], 0),
        ]);
        let mut run = TrackRun::new(&t).unwrap();
        run.tick(10);
        let full = world_counts(&run);
        let before = run.mechanisms().len();
        assert_eq!(before, 2);
        // The gate's flap is a hinged dynamic part: a body, its collider, a frame and a joint.
        assert!(full.2 >= 1, "{full:?}");
        let gate = t.placements[0].clone();
        assert!(run.remove_mechanism(gate.anchor));
        assert!(!run.remove_mechanism(gate.anchor), "removed twice");
        let gone = world_counts(&run);
        assert!(
            gone.0 < full.0 && gone.1 < full.1 && gone.2 < full.2,
            "{gone:?}"
        );
        assert_eq!(run.mechanisms().len(), 1);
        assert!(run.sensors().iter().all(|s| s.id.0 != gate.anchor));
        let moved = Placement {
            anchor: CellCoord::new(10, 0, 4),
            ..gate.clone()
        };
        let id = run.add_mechanism(kit, &registry, &moved).unwrap();
        assert_eq!(id, Some(crate::mechanism::InstanceId(moved.anchor)));
        assert_eq!(world_counts(&run), full);
        assert!(run.sensors().iter().any(|s| s.id.0 == moved.anchor));
        let views = run.mechanisms();
        assert_eq!(views.len(), before);
        assert!(views.iter().any(|v| v.id.0 == moved.anchor));
        // A piece with no behaviour adds nothing; the run keeps stepping.
        let plain = &t.placements[2];
        assert_eq!(run.add_mechanism(kit, &registry, plain).unwrap(), None);
        assert!(!run.remove_mechanism(plain.anchor));
        run.tick(240);
        assert_eq!(world_counts(&run), full);
    }

    /// P3-T5 live edits: the run's hopper moved mid-run (removed, then placed at another cell)
    /// holds releases while it is gone, then releases the rest of the queue from its new cell
    /// with its count carried over, so no past release repeats.
    #[test]
    fn a_hopper_moved_mid_run_releases_the_rest_from_its_new_cell() {
        let kit = parble_kit::builtin();
        let registry = behaviour::registry();
        let mut t = with_pieces(&[(behaviour::HOPPER, [0, 1, 0], 0)]);
        t.hopper = HopperConfig {
            placement: Some(0),
            queue: vec![MarbleSize::Standard; 3],
            release: ReleaseMode::Interval { ticks: 120 },
        };
        let mut run = TrackRun::new(&t).unwrap();
        run.tick(60);
        assert_eq!(run.marbles().len(), 1);
        let old = t.placements[0].clone();
        assert!(run.remove_mechanism(old.anchor));
        run.remove_piece(old.anchor).unwrap();
        run.tick(120);
        assert_eq!(
            run.marbles().len(),
            1,
            "a release while the hopper was gone"
        );
        let moved = Placement {
            anchor: CellCoord::new(-6, 1, 6),
            ..old.clone()
        };
        run.place_piece(kit, moved.clone()).unwrap();
        while run.rebuild_next_chunk(kit).unwrap().is_some() {}
        run.add_mechanism(kit, &registry, &moved).unwrap();
        let hopper = |run: &TrackRun| {
            run.mechanisms()
                .into_iter()
                .find(|m| m.piece.as_str() == behaviour::HOPPER)
                .unwrap()
        };
        assert_eq!(hopper(&run).id.0, moved.anchor);
        assert_eq!(hopper(&run).state["released"], 1, "the count carried over");
        // The overdue release spawns at once, in the new hopper's bowl.
        run.tick(1);
        assert_eq!(run.marbles().len(), 2);
        let spawned = run.marbles()[1].position;
        let bowl = place_point_um(moved.orientation(), moved.anchor, HOPPER_BOWL_UM);
        let bowl_m = |i: usize| (bowl[i] as f64 * 1.0e-6) as f32;
        assert!(
            (spawned[0] - bowl_m(0)).abs() < 0.03 && (spawned[2] - bowl_m(2)).abs() < 0.03,
            "spawned at {spawned:?}, bowl {bowl:?}"
        );
        run.tick(240);
        assert_eq!(run.marbles().len(), 3);
        assert_eq!(hopper(&run).state["released"], 3);
    }
}
