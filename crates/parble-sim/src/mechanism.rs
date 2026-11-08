//! Mechanism framework (P2-T1): sensor volumes, tick-driven kinematic parts, a [`Mechanism`] state
//! machine per placed instance, named RNG sub-streams and the event stream for audio and UI.
//!
//! Everything here advances by tick count. A part's angle is a pure function of the tick
//! ([`Keyframes::sample`] on integer millidegrees), never an accumulated `angle += ω·dt`, so how
//! the caller groups ticks into frames cannot change the run. The exception is a physical part
//! ([`Mechanism::part_physics`]): a dynamic body on a revolute joint whose angle is read back from
//! the body after each step, rounded to millidegrees.
//!
//! Each tick, after the physics step that reached tick `t`, [`Mechanisms::after_step`]:
//! 0. reads every physical part's angle from its body;
//! 1. updates every sensor's occupancy from the marble positions at `t` (sphere against the placed
//!    box or sphere, so sensors add nothing to Rapier's broad phase);
//! 2. calls [`Mechanism::step`] for each instance in [`InstanceId`] order, then applies the marble
//!    drags it queued ([`MechContext::drag_marble`]);
//! 3. sets each kinematic part's next pose to its angle at `t + 1` (a physical part instead takes
//!    its brace, [`MechContext::brace_part`]);
//! 4. on every multiple of [`crate::HASH_SAMPLE_TICKS`], absorbs every instance's state into
//!    [`Sim::mechanism_hash`];
//! 5. when recording is on ([`Sim::set_event_recording`]), reads contacts back from the narrow phase
//!    (read-only: no `ActiveEvents`, no event handler) and emits [`ContactEvent`]s on contact start
//!    and a [`RollSample`] per marble every [`ROLL_SAMPLE_TICKS`].
//!
//! With no instances and recording off this is a no-op, so piece-free and mechanism-free runs keep
//! their golden hashes.
//!
//! RNG: an instance's stream `name` is [`SeededRng::stream`] of the track seed with the key
//! `mech/<piece id>/<x>,<y>,<z>/<name>` (the placement's anchor cell). It depends on nothing else,
//! so adding, removing or reordering other placements leaves it unchanged.
//!
//! Surfaces: a contact on a chunk collider reports the static surface
//! ([`parble_kit::KitPiece::static_surface`]) of the placement whose cell holds the contact point,
//! a hair inside the collider ([`StaticSurfaces`]): rail on a rail piece, wood on a block or a
//! mechanism's frame. The merged chunk trimesh keeps no per-triangle surface, so this relies on
//! every builtin piece having one static surface (checked in parble-kit). The tray, and a point in
//! no placed cell, report [`Surface::Wood`]. The lookup only reads the world, so it cannot change a
//! run.

use std::collections::{BTreeMap, BTreeSet};

use parble_core::{
    CellCoord, MechConfig, MechanismState, Mirror, Orientation, PieceId, Placement, SeededRng,
    SubStream, Track,
};
use parble_kit::{Joint, KitCatalog, KitPiece, SensorShape, Surface};
use rapier3d::prelude::*;

use crate::chunk::place_point_um;
use crate::hash::Fnv1a64;
use crate::{HASH_SAMPLE_TICKS, Sim, scene};

/// A [`RollSample`] per marble every 4 ticks (60 Hz).
pub const ROLL_SAMPLE_TICKS: u64 = 4;

/// Saved-state key prefix for an RNG stream's draw count ([`MechRng::draws`]).
pub const RNG_STATE_PREFIX: &str = "rng/";
/// Saved-state key prefix for a part's angle in millidegrees.
pub const PART_STATE_PREFIX: &str = "part/";

/// A placed mechanism instance: its anchor cell. Placements cannot overlap, so the anchor is
/// unique, and unlike the placement index it does not move when other placements change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstanceId(pub CellCoord);

/// The behaviour of one mechanism kind. The kit behaviours are in [`crate::behaviour`].
pub trait Mechanism: Send + Sync {
    /// Stable behaviour name (diagnostics and [`Sim::mechanism_hash`]).
    fn kind(&self) -> &'static str;
    /// Called once per tick, after the physics step that reached [`MechContext::tick`].
    fn step(&mut self, ctx: &mut MechContext<'_>);
    /// Behaviour state as integers. Saved as [`MechanismState::state`] next to the framework's own
    /// `rng/` and `part/` keys, so it must not use those prefixes.
    fn save(&self) -> MechConfig;
    /// Restore from a map [`Mechanism::save`] produced (unknown keys are ignored).
    fn restore(&mut self, state: &MechConfig);
    /// `Some` for a part simulated as a hinged dynamic body ([`PartPhysics`]) instead of a
    /// kinematic pose. Asked once per part when the instance is added.
    fn part_physics(&self, _part: usize) -> Option<PartPhysics> {
        None
    }
    /// `false` for a kinematic part drawn and posed but given no collider, so marbles pass
    /// through it (a cosmetic part). Asked once per part when the instance is added; ignored for a
    /// physical part.
    fn part_collides(&self, _part: usize) -> bool {
        true
    }
}

/// A physical part: a dynamic body on a revolute joint about the part's pivot and axis, limited to
/// the sidecar's `limits_mdeg`. Contacts push it and gravity returns it; the behaviour reads its
/// angle ([`MechContext::part_angle`]) and may brace it ([`MechContext::brace_part`]), and
/// [`MechContext::play`] does nothing. Its collider is the convex hull of the sidecar collision
/// and touches marbles only ([`FREE_PART_GROUP`]), so a part modelled flush with its frame swings
/// freely.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PartPhysics {
    pub mass_kg: f32,
    /// Rapier body angular damping, 1/s.
    pub angular_damping: f32,
    /// Centre of mass this far straight below the pivot at rest, m (a counterweighted part).
    /// `None` keeps the hull's own centre of mass.
    pub com_drop_m: Option<f32>,
}

/// Extra solver iterations for a physical part's island. A marble lifting a light part from
/// below presses it onto its joint limit at about 18:1 mass ratio, and Rapier's 4 iterations let
/// the part sink up to 3.3° through the stop (gate flap, 0.05 m/s marble). At 48 it stays within
/// 0.2° at every speed from 0.05 to 0.2 m/s.
pub const PART_SOLVER_ITERATIONS: usize = 48;

/// Collision group of physical part colliders. Every static and kinematic collider leaves it out
/// of its memberships ([`fixed_groups`]) and marbles keep the default (all groups), so a physical
/// part collides with marbles and other physical parts only.
pub const FREE_PART_GROUP: Group = Group::GROUP_32;

/// Interaction groups of the static and kinematic colliders: every membership but
/// [`FREE_PART_GROUP`]. This only filters pairs with physical parts.
pub fn fixed_groups() -> InteractionGroups {
    InteractionGroups::new(
        Group::ALL.difference(FREE_PART_GROUP),
        Group::ALL,
        InteractionTestMode::And,
    )
}

/// Builds a behaviour from its placement's config.
pub type MechanismFactory = fn(&MechConfig) -> Box<dyn Mechanism>;

/// Behaviour factories by piece id.
#[derive(Debug, Clone, Default)]
pub struct MechanismRegistry {
    factories: BTreeMap<PieceId, MechanismFactory>,
}

impl MechanismRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) the factory for `piece`.
    pub fn register(&mut self, piece: PieceId, factory: MechanismFactory) {
        self.factories.insert(piece, factory);
    }

    pub fn get(&self, piece: &PieceId) -> Option<MechanismFactory> {
        self.factories.get(piece).copied()
    }

    pub fn is_empty(&self) -> bool {
        self.factories.is_empty()
    }
}

/// A part motion: `(tick offset, millidegrees)` keys with strictly increasing ticks, linearly
/// interpolated in integer arithmetic and held at the ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keyframes(Vec<(u32, i32)>);

impl Keyframes {
    /// `None` when `keys` is empty or its ticks are not strictly increasing.
    pub fn new(keys: Vec<(u32, i32)>) -> Option<Self> {
        let ordered = keys.windows(2).all(|w| w[0].0 < w[1].0);
        (!keys.is_empty() && ordered).then_some(Self(keys))
    }

    /// A constant angle.
    pub fn hold(mdeg: i32) -> Self {
        Self(vec![(0, mdeg)])
    }

    /// From `from` to `to` over `ticks` (an immediate jump when `ticks` is 0).
    pub fn ramp(from: i32, to: i32, ticks: u32) -> Self {
        if ticks == 0 {
            Self::hold(to)
        } else {
            Self(vec![(0, from), (ticks, to)])
        }
    }

    pub fn keys(&self) -> &[(u32, i32)] {
        &self.0
    }

    /// Tick offset of the last key.
    pub fn duration(&self) -> u32 {
        self.0.last().map_or(0, |k| k.0)
    }

    /// Angle `dt` ticks after the motion started. Rounds toward zero.
    pub fn sample(&self, dt: u64) -> i32 {
        let first = self.0[0];
        if dt <= u64::from(first.0) {
            return first.1;
        }
        for w in self.0.windows(2) {
            let ((ta, a), (tb, b)) = (w[0], w[1]);
            if dt <= u64::from(tb) {
                let num = (i64::from(b) - i64::from(a)) * (dt as i64 - i64::from(ta));
                return (i64::from(a) + num / i64::from(tb - ta)) as i32;
            }
        }
        self.0[self.0.len() - 1].1
    }
}

/// One named random stream of an instance. Only `u32` draws, counted, so a saved count restores
/// the stream position exactly.
#[derive(Debug, Clone)]
pub struct MechRng {
    stream: SubStream,
    draws: u64,
}

impl MechRng {
    fn new(stream: SubStream) -> Self {
        Self { stream, draws: 0 }
    }

    pub fn next_u32(&mut self) -> u32 {
        self.draws += 1;
        self.stream.next_u32()
    }

    /// Uniform integer in `0..n`, unbiased. Panics if `n == 0`.
    pub fn below(&mut self, n: u32) -> u32 {
        assert!(n > 0, "MechRng::below(0)");
        let zone = u32::MAX - (u32::MAX - n + 1) % n;
        loop {
            let v = self.next_u32();
            if v <= zone {
                return v % n;
            }
        }
    }

    /// `u32` words drawn so far.
    pub fn draws(&self) -> u64 {
        self.draws
    }
}

/// The sub-stream key of `name` for the instance of `piece` anchored at `anchor`.
pub fn stream_key(piece: &PieceId, anchor: CellCoord, name: &str) -> String {
    format!("mech/{piece}/{},{},{}/{name}", anchor.x, anchor.y, anchor.z)
}

/// Marble indices (spawn order) in one sensor, and who entered or left at this tick.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SensorStatus {
    pub occupants: BTreeSet<usize>,
    pub entered: Vec<usize>,
    pub exited: Vec<usize>,
}

/// What happened in a mechanism, for UI and audio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MechEventKind {
    /// A marble entered the sensor at this index in the piece's sidecar list.
    SensorEnter {
        sensor: usize,
        marble: usize,
    },
    SensorExit {
        sensor: usize,
        marble: usize,
    },
    /// A behaviour signal ([`MechContext::emit`]).
    Signal {
        name: &'static str,
        value: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MechanismEvent {
    pub tick: u64,
    pub instance: InstanceId,
    pub kind: MechEventKind,
}

/// The surface on the other side of a contact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SurfaceId {
    /// Another marble.
    Glass,
    Kit(Surface),
}

/// What a marble touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactOther {
    Marble(usize),
    /// Chunk colliders, the tray and the floor.
    Static,
    /// An animated part, by index in the piece's sidecar list.
    Part {
        instance: InstanceId,
        part: usize,
    },
}

/// A contact that started at this tick, with the solver's normal impulse on it, N·s.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContactEvent {
    pub tick: u64,
    pub marble: usize,
    pub other: ContactOther,
    pub surface: SurfaceId,
    pub impulse: f32,
}

/// One marble's speed and the surface carrying it (the strongest active contact), or `None` in
/// the air.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RollSample {
    pub tick: u64,
    pub marble: usize,
    pub speed: f32,
    pub surface: Option<SurfaceId>,
}

/// The event stream. Within a tick: mechanism events in instance order, then contacts by
/// `(marble, other collider)`, then roll samples by marble.
#[derive(Debug, Clone, PartialEq)]
pub enum SimEvent {
    Mechanism(MechanismEvent),
    Contact(ContactEvent),
    Roll(RollSample),
}

/// Why a mechanism could not be added.
#[derive(Debug, Clone, PartialEq)]
pub enum MechanismError {
    /// An instance is already anchored at this cell.
    Occupied(CellCoord),
    /// A part's collision group is not a valid trimesh.
    PartMesh { part: usize, group: usize },
    /// The placement's piece is not in the catalogue.
    UnknownPiece(PieceId),
}

impl core::fmt::Display for MechanismError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for MechanismError {}

struct SensorRuntime {
    shape: PlacedShape,
    status: SensorStatus,
}

#[derive(Clone, Copy)]
enum PlacedShape {
    Box { center: Vector, half: Vector },
    Sphere { center: Vector, radius: f32 },
}

impl PlacedShape {
    fn touches(&self, p: Vector, r: f32) -> bool {
        match *self {
            Self::Box { center, half } => {
                let d = ((p - center).abs() - half).max(Vector::ZERO);
                d.dot(d) <= r * r
            }
            Self::Sphere { center, radius } => {
                let d = p - center;
                d.dot(d) <= (radius + r) * (radius + r)
            }
        }
    }

    /// World-aligned bounds `(min, max)`.
    fn bounds(&self) -> (Vector, Vector) {
        match *self {
            Self::Box { center, half } => (center - half, center + half),
            Self::Sphere { center, radius } => (
                center - Vector::splat(radius),
                center + Vector::splat(radius),
            ),
        }
    }
}

struct PartRuntime {
    name: String,
    body: RigidBodyHandle,
    pivot: Vector,
    /// World hinge (or slide) axis.
    axis: Vector,
    /// A slide part: its position is µm along `axis`, unrotated ([`Joint::Slide`]).
    slide: bool,
    /// −1 when mirrored: a reflection turns a rotation by θ into one by −θ about the image axis.
    sign: f32,
    limits: [i32; 2],
    start: u64,
    motion: Keyframes,
    /// Physical part: its angle read from the body after the last step, millidegrees.
    physical: Option<i32>,
    /// Physical part held against its stop: marbles cannot move it ([`MechContext::brace_part`]).
    braced: bool,
}

impl PartRuntime {
    fn angle_at(&self, tick: u64) -> i32 {
        if let Some(mdeg) = self.physical {
            return mdeg;
        }
        self.motion
            .sample(tick.saturating_sub(self.start))
            .clamp(self.limits[0], self.limits[1])
    }

    fn pose_at(&self, tick: u64) -> Pose {
        if self.slide {
            // A reflection maps a translation along the axis to one along the image axis.
            let d = self.angle_at(tick) as f32 * 1.0e-6;
            return Pose::from_parts(self.pivot + self.axis * d, Rotation::IDENTITY);
        }
        let theta = self.sign * self.angle_at(tick) as f32 * (core::f32::consts::PI / 180_000.0);
        let (s, c) = (libm::sinf(theta / 2.0), libm::cosf(theta / 2.0));
        let a = self.axis * s;
        Pose::from_parts(self.pivot, Rotation::from_xyzw(a.x, a.y, a.z, c))
    }

    /// A physical part's angle from its body rotation, millidegrees (rounded).
    fn measure(&self, world: &PhysicsWorld) -> i32 {
        let q = world.bodies[self.body].rotation();
        let (v, w) = if q.w < 0.0 {
            (-Vector::new(q.x, q.y, q.z), -q.w)
        } else {
            (Vector::new(q.x, q.y, q.z), q.w)
        };
        let theta = 2.0 * libm::atan2f(v.dot(self.axis), w) * self.sign;
        libm::roundf(theta * (180_000.0 / core::f32::consts::PI)) as i32
    }
}

/// The view of one instance a [`Mechanism::step`] works through.
pub struct MechContext<'a> {
    tick: u64,
    id: InstanceId,
    config: &'a MechConfig,
    sensors: &'a [SensorRuntime],
    parts: &'a mut [PartRuntime],
    rng: &'a mut BTreeMap<String, MechRng>,
    root: SeededRng,
    piece: &'a PieceId,
    signals: &'a mut Vec<(&'static str, i64)>,
    /// Every marble's `(centre, velocity, radius, collider)` after this step.
    balls: &'a [(Vector, Vector, f32, ColliderHandle)],
    /// `(marble, k)` drags queued by [`MechContext::drag_marble`] this tick.
    drags: &'a mut Vec<(usize, f32)>,
}

impl MechContext<'_> {
    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn id(&self) -> InstanceId {
        self.id
    }

    /// Marbles in the world, in spawn order.
    pub fn marble_count(&self) -> usize {
        self.balls.len()
    }

    /// A value from the placement's [`Placement::config`].
    pub fn config(&self, key: &str) -> Option<i64> {
        self.config.get(key).copied()
    }

    /// The sensor at `index` in the sidecar list.
    pub fn sensor(&self, index: usize) -> Option<&SensorStatus> {
        self.sensors.get(index).map(|s| &s.status)
    }

    /// Whether `marble` touches the world-aligned box spanning sensors `a` and `b`: in either of
    /// them or anywhere between them.
    pub fn between_sensors(&self, a: usize, b: usize, marble: usize) -> bool {
        let (Some(sa), Some(sb), Some(&(p, _, r, _))) = (
            self.sensors.get(a),
            self.sensors.get(b),
            self.balls.get(marble),
        ) else {
            return false;
        };
        let ((amin, amax), (bmin, bmax)) = (sa.shape.bounds(), sb.shape.bounds());
        let (min, max) = (amin.min(bmin), amax.max(bmax));
        PlacedShape::Box {
            center: (min + max) * 0.5,
            half: (max - min) * 0.5,
        }
        .touches(p, r)
    }

    /// The instance's stream `name`, derived on first use and kept for the run.
    pub fn rng(&mut self, name: &str) -> &mut MechRng {
        let (root, piece, anchor) = (self.root, self.piece, self.id.0);
        self.rng
            .entry(name.to_owned())
            .or_insert_with(|| MechRng::new(root.stream(&stream_key(piece, anchor, name))))
    }

    /// The angle of part `index` at this tick, millidegrees (µm along its axis for a slide part).
    pub fn part_angle(&self, index: usize) -> Option<i32> {
        self.parts.get(index).map(|p| p.angle_at(self.tick))
    }

    /// The sidecar `[min, max]` of part `index`, in the part's unit.
    pub fn part_limits(&self, index: usize) -> Option<[i32; 2]> {
        self.parts.get(index).map(|p| p.limits)
    }

    /// Start `motion` on part `index` now: the part is at `motion.sample(0)` at this tick (the
    /// pose the next step moves it to is `motion.sample(1)`). Angles are clamped to the part's
    /// limits. Returns `false` for an unknown part.
    pub fn play(&mut self, index: usize, motion: Keyframes) -> bool {
        let Some(p) = self.parts.get_mut(index).filter(|p| p.physical.is_none()) else {
            return false;
        };
        p.start = self.tick;
        p.motion = motion;
        true
    }

    /// Move part `index` from its current angle to `to` over `ticks`.
    pub fn move_part(&mut self, index: usize, to: i32, ticks: u32) -> bool {
        match self.part_angle(index) {
            Some(from) => self.play(index, Keyframes::ramp(from, to, ticks)),
            None => false,
        }
    }

    /// Brace physical part `index` on its stop (or release it) from the next step: braced, a
    /// marble pressing it meets the stop's full stiffness, as if the frame took the push (a
    /// higher Rapier dominance group). A part much lighter than the marble cannot pass that push
    /// through the solver's joint chain on its own. Returns `false` for a kinematic part.
    pub fn brace_part(&mut self, index: usize, braced: bool) -> bool {
        match self.parts.get_mut(index).filter(|p| p.physical.is_some()) {
            Some(p) => {
                p.braced = braced;
                true
            }
            None => false,
        }
    }

    /// Emit a [`MechEventKind::Signal`] (kept only while recording).
    pub fn emit(&mut self, name: &'static str, value: i64) {
        self.signals.push((name, value));
    }

    /// Marble `marble`'s speed (m/s) and radius (m) at this tick.
    pub fn marble(&self, marble: usize) -> Option<(f32, f32)> {
        self.balls
            .get(marble)
            .map(|&(_, v, r, _)| (libm::sqrtf(v.dot(v)), r))
    }

    /// Quadratic drag on marble `marble` before the next step: its linear and angular velocity are
    /// both scaled by `exp(−k·|v|·dt)`, so a rolling marble keeps rolling and its speed falls by
    /// the factor `exp(−k·d)` over a distance `d` travelled under the drag, whatever its speed.
    /// `k` is per metre.
    pub fn drag_marble(&mut self, marble: usize, k: f32) {
        if marble < self.balls.len() {
            self.drags.push((marble, k));
        }
    }
}

/// Apply queued [`MechContext::drag_marble`] drags.
fn apply_drags(world: &mut PhysicsWorld, marbles: &[RigidBodyHandle], drags: &[(usize, f32)]) {
    for &(m, k) in drags {
        let body = &mut world.bodies[marbles[m]];
        let v = body.linvel();
        let f = libm::expf(-k * libm::sqrtf(v.dot(v)) * crate::DT);
        let w = body.angvel();
        body.set_linvel(v * f, true);
        body.set_angvel(w * f, true);
    }
}

struct Instance {
    piece: PieceId,
    config: MechConfig,
    root: SeededRng,
    behaviour: Box<dyn Mechanism>,
    sensors: Vec<SensorRuntime>,
    parts: Vec<PartRuntime>,
    rng: BTreeMap<String, MechRng>,
}

impl Instance {
    /// Behaviour state plus the framework's RNG positions and part angles at `tick`.
    fn save(&self, tick: u64) -> MechConfig {
        let mut s = self.behaviour.save();
        for (name, r) in &self.rng {
            s.insert(format!("{RNG_STATE_PREFIX}{name}"), r.draws as i64);
        }
        for p in &self.parts {
            s.insert(
                format!("{PART_STATE_PREFIX}{}", p.name),
                i64::from(p.angle_at(tick)),
            );
        }
        s
    }
}

/// How far past the marble's surface, into the collider, the contact point is looked up (m): it
/// keeps a point on a cell's bottom face in the piece that owns the face.
const SURFACE_PROBE_M: f32 = 2.5e-4;

/// The static surface lookup for chunk collider contacts (see the module docs).
pub(crate) struct StaticSurfaces<'a> {
    pub pieces: &'a crate::chunk::ChunkColliders,
    /// Placed piece id → its static surface, recorded at [`Sim::place_piece`].
    pub surfaces: &'a BTreeMap<PieceId, Surface>,
}

impl StaticSurfaces<'_> {
    /// The surface of the placement whose cell holds world point `p` (metres), if any.
    fn at(&self, p: Vector) -> Option<Surface> {
        // Cell (x, y, z) spans x ± half a cell, y .. y + 1 cell, z ± half a cell (kit contract §2).
        let cell_m = scene::um_to_m(parble_core::contract::CELL_UM);
        let c = |v: f32, offset: f32| libm::floorf(v / cell_m + offset) as i32;
        let cell = CellCoord::new(c(p.x, 0.5), c(p.y, 0.0), c(p.z, 0.5));
        let placement = self.pieces.placement_at(cell)?;
        self.surfaces.get(&placement.piece).copied()
    }
}

/// All mechanism instances of a [`Sim`], plus the event stream.
#[derive(Default)]
pub struct Mechanisms {
    instances: BTreeMap<InstanceId, Instance>,
    recording: bool,
    events: Vec<SimEvent>,
    /// `(marble, other collider)` pairs with an active contact at the last tick.
    touching: BTreeSet<(usize, (u32, u32))>,
    /// Part colliders → `(instance, part, surface)`.
    part_colliders: BTreeMap<(u32, u32), (InstanceId, usize, Surface)>,
    hasher: Fnv1a64,
}

fn um_m(v: [i64; 3]) -> Vector {
    Vector::new(
        (v[0] as f64 * 1.0e-6) as f32,
        (v[1] as f64 * 1.0e-6) as f32,
        (v[2] as f64 * 1.0e-6) as f32,
    )
}

fn sub3(a: [i64; 3], b: [i64; 3]) -> [i64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn marble_ball(world: &PhysicsWorld, h: RigidBodyHandle) -> (Vector, Vector, f32, ColliderHandle) {
    let b = &world.bodies[h];
    let ch = b.colliders()[0];
    let r = world.colliders[ch]
        .shape()
        .as_ball()
        .map_or(0.0, |ball| ball.radius);
    (b.translation(), b.linvel(), r, ch)
}

impl Mechanisms {
    fn add(
        &mut self,
        world: &mut PhysicsWorld,
        tick: u64,
        piece: &KitPiece,
        placement: &Placement,
        seed: u64,
        behaviour: Box<dyn Mechanism>,
    ) -> Result<InstanceId, MechanismError> {
        let id = InstanceId(placement.anchor);
        if self.instances.contains_key(&id) {
            return Err(MechanismError::Occupied(placement.anchor));
        }
        let o: Orientation = placement.orientation();
        let anchor = placement.anchor;
        let origin = CellCoord::new(0, 0, 0);
        let mirrored = o.mirror == Mirror::On;
        let odd_yaw = o.yaw.quarter_turns() % 2 == 1;

        let sensors = piece
            .sensors
            .iter()
            .map(|s| {
                let shape = match s.shape {
                    SensorShape::Box {
                        center_um,
                        half_extents_um: h,
                    } => {
                        let [hx, hy, hz] = if odd_yaw { [h[2], h[1], h[0]] } else { h };
                        PlacedShape::Box {
                            center: um_m(place_point_um(o, anchor, center_um)),
                            half: um_m([hx.into(), hy.into(), hz.into()]),
                        }
                    }
                    SensorShape::Sphere {
                        center_um,
                        radius_um,
                    } => PlacedShape::Sphere {
                        center: um_m(place_point_um(o, anchor, center_um)),
                        radius: (f64::from(radius_um) * 1.0e-6) as f32,
                    },
                };
                SensorRuntime {
                    shape,
                    status: SensorStatus::default(),
                }
            })
            .collect();

        // Build every part collider before touching the world, so a bad mesh leaves it unchanged.
        let mut built = Vec::with_capacity(piece.animated_parts.len());
        for (pi, part) in piece.animated_parts.iter().enumerate() {
            let pivot_um = place_point_um(o, anchor, part.pivot_um);
            let mut colliders = Vec::with_capacity(part.collision.len());
            for (gi, g) in part.collision.iter().enumerate() {
                let vertices: Vec<Vector> = g
                    .vertices_um
                    .iter()
                    .map(|&v| um_m(sub3(place_point_um(o, anchor, v), pivot_um)))
                    .collect();
                let triangles: Vec<[u32; 3]> = g
                    .triangles
                    .iter()
                    .map(|&[a, b, c]| if mirrored { [a, c, b] } else { [a, b, c] })
                    .collect();
                let collider = ColliderBuilder::trimesh(vertices, triangles)
                    .map_err(|_| MechanismError::PartMesh {
                        part: pi,
                        group: gi,
                    })?
                    .friction(scene::WOOD_FRICTION)
                    .restitution(scene::WOOD_RESTITUTION)
                    .collision_groups(fixed_groups())
                    .build();
                colliders.push((collider, g.surface));
            }
            // A slide part is always kinematic.
            let slide = part.joint == Joint::Slide;
            let physics = behaviour.part_physics(pi).filter(|_| !slide);
            if let Some(ph) = physics {
                // One convex hull over every group carries the whole mass, with the first group's
                // surface.
                let points: Vec<Vector> = part
                    .collision
                    .iter()
                    .flat_map(|g| &g.vertices_um)
                    .map(|&v| um_m(sub3(place_point_um(o, anchor, v), pivot_um)))
                    .collect();
                let surface = colliders.first().map_or(Surface::Wood, |c| c.1);
                // A trimesh of the hull, not the hull itself: Parry's GJK point projection on a
                // thin hull reports a marble centre outside it as inside, and the far-face normal
                // drives the marble into the part (`tests/parry_thin_hull_projection.rs`).
                let hull = SharedShape::convex_hull(&points)
                    .ok_or(MechanismError::PartMesh { part: pi, group: 0 })?;
                let mut mass = hull.mass_properties(1.0);
                mass.set_mass(ph.mass_kg, true);
                if let Some(drop) = ph.com_drop_m {
                    mass.local_com = Vector::new(0.0, -drop, 0.0);
                }
                let (vertices, indices) = hull.as_convex_polyhedron().unwrap().to_trimesh();
                // No `FIX_INTERNAL_EDGES` / `ORIENTED`: with them a 0.6 m/s marble reached 1 mm
                // into the shut flap before its contact was found.
                let hull = ColliderBuilder::trimesh(vertices, indices)
                    .map_err(|_| MechanismError::PartMesh { part: pi, group: 0 })?
                    .mass_properties(mass)
                    .friction(scene::WOOD_FRICTION)
                    .restitution(scene::WOOD_RESTITUTION)
                    .collision_groups(InteractionGroups::new(
                        FREE_PART_GROUP,
                        FREE_PART_GROUP,
                        InteractionTestMode::And,
                    ))
                    .build();
                colliders = vec![(hull, surface)];
            } else if !behaviour.part_collides(pi) {
                colliders.clear();
            }
            let runtime = PartRuntime {
                name: part.name.clone(),
                body: RigidBodyHandle::invalid(),
                pivot: um_m(pivot_um),
                axis: um_m(place_point_um(o, origin, part.axis)) * 1.0e6,
                slide,
                sign: if mirrored { -1.0 } else { 1.0 },
                limits: part.limits_mdeg,
                start: tick,
                motion: Keyframes::hold(part.rest_mdeg),
                physical: physics.map(|_| part.rest_mdeg),
                braced: false,
            };
            built.push((runtime, colliders, physics));
        }

        let mut parts = Vec::with_capacity(built.len());
        for (pi, (mut runtime, colliders, physics)) in built.into_iter().enumerate() {
            let body = match physics {
                Some(ph) => RigidBodyBuilder::dynamic()
                    .angular_damping(ph.angular_damping)
                    .additional_solver_iterations(PART_SOLVER_ITERATIONS),
                None => RigidBodyBuilder::kinematic_position_based(),
            }
            .pose(runtime.pose_at(tick))
            .can_sleep(false)
            .build();
            let handle = world.bodies.insert(body);
            for (collider, surface) in colliders {
                let ch = world
                    .colliders
                    .insert_with_parent(collider, handle, &mut world.bodies);
                self.part_colliders
                    .insert(ch.into_raw_parts(), (id, pi, surface));
            }
            runtime.body = handle;
            if physics.is_some() {
                let frame = world
                    .bodies
                    .insert(RigidBodyBuilder::fixed().translation(runtime.pivot).build());
                let rad = |mdeg: i32| mdeg as f32 * (core::f32::consts::PI / 180_000.0);
                // About `sign · axis`, so the joint angle is the part angle in radians.
                let joint = RevoluteJointBuilder::new(runtime.axis * runtime.sign)
                    .limits([rad(runtime.limits[0]), rad(runtime.limits[1])])
                    .contacts_enabled(false);
                world.impulse_joints.insert(frame, handle, joint, true);
            } else {
                world.bodies[handle].set_next_kinematic_position(runtime.pose_at(tick + 1));
            }
            parts.push(runtime);
        }

        self.instances.insert(
            id,
            Instance {
                piece: placement.piece.clone(),
                config: placement.config.clone(),
                root: SeededRng::new(seed),
                behaviour,
                sensors,
                parts,
                rng: BTreeMap::new(),
            },
        );
        Ok(id)
    }

    /// The per-tick pass described in the module docs. A no-op with no instances and recording
    /// off.
    pub(crate) fn after_step(
        &mut self,
        world: &mut PhysicsWorld,
        marbles: &[RigidBodyHandle],
        tick: u64,
        statics: &StaticSurfaces<'_>,
    ) {
        if self.instances.is_empty() && !self.recording {
            return;
        }
        let balls: Vec<_> = marbles.iter().map(|&h| marble_ball(world, h)).collect();

        for inst in self.instances.values_mut() {
            for p in &mut inst.parts {
                if p.physical.is_some() {
                    p.physical = Some(p.measure(world));
                }
            }
        }

        for (&id, inst) in &mut self.instances {
            let mut sensor_events = Vec::new();
            for (si, s) in inst.sensors.iter_mut().enumerate() {
                let now: BTreeSet<usize> = balls
                    .iter()
                    .enumerate()
                    .filter(|(_, (p, _, r, _))| s.shape.touches(*p, *r))
                    .map(|(i, _)| i)
                    .collect();
                let entered: Vec<usize> = now.difference(&s.status.occupants).copied().collect();
                let exited: Vec<usize> = s.status.occupants.difference(&now).copied().collect();
                for &marble in &entered {
                    sensor_events.push(MechEventKind::SensorEnter { sensor: si, marble });
                }
                for &marble in &exited {
                    sensor_events.push(MechEventKind::SensorExit { sensor: si, marble });
                }
                s.status = SensorStatus {
                    occupants: now,
                    entered,
                    exited,
                };
            }

            let mut signals = Vec::new();
            let mut drags = Vec::new();
            let mut ctx = MechContext {
                tick,
                id,
                config: &inst.config,
                sensors: &inst.sensors,
                parts: &mut inst.parts,
                rng: &mut inst.rng,
                root: inst.root,
                piece: &inst.piece,
                signals: &mut signals,
                balls: &balls,
                drags: &mut drags,
            };
            inst.behaviour.step(&mut ctx);
            apply_drags(world, marbles, &drags);

            for p in &inst.parts {
                let body = &mut world.bodies[p.body];
                if p.physical.is_some() {
                    body.set_dominance_group(i8::from(p.braced));
                } else {
                    body.set_next_kinematic_position(p.pose_at(tick + 1));
                }
            }

            if self.recording {
                let signals = signals
                    .into_iter()
                    .map(|(name, value)| MechEventKind::Signal { name, value });
                for kind in sensor_events.into_iter().chain(signals) {
                    self.events.push(SimEvent::Mechanism(MechanismEvent {
                        tick,
                        instance: id,
                        kind,
                    }));
                }
            }
        }

        if !self.instances.is_empty() && tick.is_multiple_of(HASH_SAMPLE_TICKS) {
            self.sample(tick);
        }
        if self.recording {
            self.record_contacts(world, &balls, tick, statics);
        }
    }

    fn sample(&mut self, tick: u64) {
        self.hasher.write_u64(tick);
        for (id, inst) in &self.instances {
            for c in [id.0.x, id.0.y, id.0.z] {
                self.hasher.write_i64(c.into());
            }
            self.hasher.write(inst.behaviour.kind().as_bytes());
            for (k, v) in inst.save(tick) {
                self.hasher.write(k.as_bytes());
                self.hasher.write_i64(v);
            }
        }
    }

    fn record_contacts(
        &mut self,
        world: &PhysicsWorld,
        balls: &[(Vector, Vector, f32, ColliderHandle)],
        tick: u64,
        statics: &StaticSurfaces<'_>,
    ) {
        let chunk_colliders: BTreeSet<(u32, u32)> = statics
            .pieces
            .chunks()
            .map(|(_, h)| h.into_raw_parts())
            .collect();
        let marble_of: BTreeMap<(u32, u32), usize> = balls
            .iter()
            .enumerate()
            .map(|(i, b)| (b.3.into_raw_parts(), i))
            .collect();
        let mut touching = BTreeSet::new();
        let mut contacts = Vec::new();
        let mut rolls = Vec::new();
        for (i, &(centre, v, r, ch)) in balls.iter().enumerate() {
            let mut active: Vec<((u32, u32), f32, Option<Vector>)> = world
                .narrow_phase
                .contact_pairs_with(ch)
                .filter(|pair| pair.has_any_active_contact())
                .map(|pair| {
                    let (other, toward) = if pair.collider1 == ch {
                        (pair.collider2, 1.0)
                    } else {
                        (pair.collider1, -1.0)
                    };
                    let key = other.into_raw_parts();
                    // The deepest point of the marble toward the collider, by the manifold that
                    // carries the most impulse; manifold normals point from collider 1 to 2.
                    let probe = chunk_colliders.contains(&key).then(|| {
                        pair.manifolds()
                            .iter()
                            .filter(|m| !m.points.is_empty())
                            .max_by(|a, b| {
                                let w = |m: &ContactManifold| {
                                    m.points.iter().map(|p| p.data.impulse).sum::<f32>()
                                };
                                w(a).total_cmp(&w(b))
                            })
                            .map(|m| centre + m.data.normal * (toward * (r + SURFACE_PROBE_M)))
                    });
                    (key, pair.total_impulse_magnitude(), probe.flatten())
                })
                .collect();
            active.sort_by_key(|a| a.0);
            let classify = |key: (u32, u32), probe: Option<Vector>| match (
                marble_of.get(&key),
                self.part_colliders.get(&key),
            ) {
                (Some(&j), _) => (ContactOther::Marble(j), SurfaceId::Glass),
                (None, Some(&(instance, part, s))) => {
                    (ContactOther::Part { instance, part }, SurfaceId::Kit(s))
                }
                (None, None) => (
                    ContactOther::Static,
                    SurfaceId::Kit(probe.and_then(|p| statics.at(p)).unwrap_or(Surface::Wood)),
                ),
            };
            let mut carrier: Option<(f32, SurfaceId)> = None;
            for &(key, impulse, probe) in &active {
                let (other, surface) = classify(key, probe);
                if carrier.is_none_or(|(best, _)| impulse > best) {
                    carrier = Some((impulse, surface));
                }
                touching.insert((i, key));
                // A marble pair is reported once, by the lower marble index.
                let reported_by_other = matches!(other, ContactOther::Marble(j) if j < i);
                if !reported_by_other && !self.touching.contains(&(i, key)) {
                    contacts.push(ContactEvent {
                        tick,
                        marble: i,
                        other,
                        surface,
                        impulse,
                    });
                }
            }
            if tick.is_multiple_of(ROLL_SAMPLE_TICKS) {
                rolls.push(RollSample {
                    tick,
                    marble: i,
                    speed: libm::sqrtf(v.dot(v)),
                    surface: carrier.map(|c| c.1),
                });
            }
        }
        self.touching = touching;
        self.events
            .extend(contacts.into_iter().map(SimEvent::Contact));
        self.events.extend(rolls.into_iter().map(SimEvent::Roll));
    }
}

/// A snapshot of one instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MechanismView {
    pub id: InstanceId,
    pub piece: PieceId,
    pub kind: &'static str,
    /// [`Mechanism::save`] plus the `rng/` draw counts and `part/` angles.
    pub state: MechConfig,
    /// Part angles at the current tick, millidegrees, in sidecar order.
    pub part_angles: Vec<i32>,
}

/// A sensor volume in world metres, for the debug overlay. Boxes are axis-aligned: placement has
/// already applied the anchor, yaw and mirror.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SensorVolume {
    Box { center: [f32; 3], half: [f32; 3] },
    Sphere { center: [f32; 3], radius: f32 },
}

/// One sensor of one instance at the current tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SensorView {
    pub id: InstanceId,
    /// Sensor index in the piece's sidecar order.
    pub index: usize,
    pub volume: SensorVolume,
    /// Marbles inside at the last tick.
    pub occupants: usize,
}

impl Sim {
    /// Add a mechanism instance for `placement` of `piece`: its sensors, a kinematic body per
    /// animated part (at the part's rest angle) and `behaviour`. `seed` roots its RNG streams
    /// (the track seed). The piece's static collision is not placed here; that is
    /// [`Sim::place_piece`].
    pub fn add_mechanism(
        &mut self,
        piece: &KitPiece,
        placement: &Placement,
        seed: u64,
        behaviour: Box<dyn Mechanism>,
    ) -> Result<InstanceId, MechanismError> {
        self.mechanisms.add(
            &mut self.world,
            self.tick,
            piece,
            placement,
            seed,
            behaviour,
        )
    }

    /// Add the registered behaviour for `placement`, if `registry` has one for its piece.
    pub fn add_registered(
        &mut self,
        registry: &MechanismRegistry,
        kit: &KitCatalog,
        placement: &Placement,
        seed: u64,
    ) -> Result<Option<InstanceId>, MechanismError> {
        let Some(factory) = registry.get(&placement.piece) else {
            return Ok(None);
        };
        let piece = kit
            .piece(&placement.piece)
            .ok_or_else(|| MechanismError::UnknownPiece(placement.piece.clone()))?;
        self.add_mechanism(piece, placement, seed, factory(&placement.config))
            .map(Some)
    }

    /// Every instance in [`InstanceId`] order.
    pub fn mechanisms(&self) -> Vec<MechanismView> {
        self.mechanisms
            .instances
            .iter()
            .map(|(&id, inst)| MechanismView {
                id,
                piece: inst.piece.clone(),
                kind: inst.behaviour.kind(),
                state: inst.save(self.tick),
                part_angles: inst.parts.iter().map(|p| p.angle_at(self.tick)).collect(),
            })
            .collect()
    }

    /// Every sensor of every instance, in [`InstanceId`] then sidecar order. Read-only.
    pub fn sensors(&self) -> Vec<SensorView> {
        let a = |v: Vector| [v.x, v.y, v.z];
        self.mechanisms
            .instances
            .iter()
            .flat_map(|(&id, inst)| {
                inst.sensors
                    .iter()
                    .enumerate()
                    .map(move |(index, s)| SensorView {
                        id,
                        index,
                        volume: match s.shape {
                            PlacedShape::Box { center, half } => SensorVolume::Box {
                                center: a(center),
                                half: a(half),
                            },
                            PlacedShape::Sphere { center, radius } => SensorVolume::Sphere {
                                center: a(center),
                                radius,
                            },
                        },
                        occupants: s.status.occupants.len(),
                    })
            })
            .collect()
    }

    /// The piece of the instance anchored at `id`, if there is one. Cheaper than
    /// [`Sim::mechanisms`], which saves every instance's state.
    pub fn mechanism_piece(&self, id: InstanceId) -> Option<&PieceId> {
        self.mechanisms.instances.get(&id).map(|inst| &inst.piece)
    }

    /// Every marble's radius in spawn order (its ball collider), metres.
    pub fn marble_radii(&self) -> Vec<f32> {
        self.marbles
            .iter()
            .map(|&h| marble_ball(&self.world, h).2)
            .collect()
    }

    /// FNV-1a 64 over every instance's state, sampled on each multiple of
    /// [`crate::HASH_SAMPLE_TICKS`] while any instance exists. Separate from
    /// [`Sim::trajectory_hash`], which stays marble-only.
    pub fn mechanism_hash(&self) -> u64 {
        self.mechanisms.hasher.finish()
    }

    /// Saved state for [`parble_core::ParbleFile::mechanisms`]: one entry per instance whose
    /// anchor is a placement of `track`, sorted by placement index.
    pub fn save_mechanism_states(&self, track: &Track) -> Vec<MechanismState> {
        let mut out: Vec<MechanismState> = track
            .placements
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let inst = self.mechanisms.instances.get(&InstanceId(p.anchor))?;
                Some(MechanismState {
                    placement: u32::try_from(i).ok()?,
                    state: inst.save(self.tick),
                })
            })
            .collect();
        out.sort_by_key(|m| m.placement);
        out
    }

    /// Restore states saved by [`Sim::save_mechanism_states`] against the same `track`: behaviour
    /// state, RNG stream positions and part angles (held until the behaviour moves them).
    pub fn restore_mechanism_states(&mut self, track: &Track, states: &[MechanismState]) {
        let tick = self.tick;
        for m in states {
            let Some(p) = track.placements.get(m.placement as usize) else {
                continue;
            };
            let Some(inst) = self.mechanisms.instances.get_mut(&InstanceId(p.anchor)) else {
                continue;
            };
            inst.behaviour.restore(&m.state);
            for (k, &v) in &m.state {
                if let Some(name) = k.strip_prefix(RNG_STATE_PREFIX) {
                    let key = stream_key(&inst.piece, p.anchor, name);
                    let mut r = MechRng::new(inst.root.stream(&key));
                    for _ in 0..v.max(0) {
                        r.next_u32();
                    }
                    inst.rng.insert(name.to_owned(), r);
                } else if let Some(name) = k.strip_prefix(PART_STATE_PREFIX)
                    && let Some(part) = inst.parts.iter_mut().find(|q| q.name == name)
                {
                    part.start = tick;
                    part.motion = Keyframes::hold(v as i32);
                    if part.physical.is_some() {
                        part.physical = Some(v as i32);
                    }
                }
            }
            for part in &inst.parts {
                let w = &mut self.world.bodies[part.body];
                w.set_position(part.pose_at(tick), true);
                if part.physical.is_some() {
                    w.set_linvel(Vector::ZERO, true);
                    w.set_angvel(Vector::ZERO, true);
                } else {
                    w.set_next_kinematic_position(part.pose_at(tick + 1));
                }
            }
        }
    }

    /// Turn the event stream on or off (default off). Off, no contact or roll work is done and
    /// mechanism events are dropped.
    pub fn set_event_recording(&mut self, on: bool) {
        self.mechanisms.recording = on;
        if !on {
            self.mechanisms.touching.clear();
        }
    }

    /// Events recorded since the last drain.
    pub fn events(&self) -> &[SimEvent] {
        &self.mechanisms.events
    }

    /// Take the recorded events. The app drains once per frame; draining never changes the run.
    pub fn drain_events(&mut self) -> Vec<SimEvent> {
        core::mem::take(&mut self.mechanisms.events)
    }
}

/// A framework fixture, not a kit behaviour (those are P2-T5): a flap that swings open by a
/// seeded angle on a fixed tick period and counts marbles through its approach sensor. The cadence,
/// RNG and event tests and the wasm parity run ([`crate::golden::mechanism_demo`]) use it.
pub mod demo {
    use super::*;

    /// Sidecar of the fixture piece: one cell, an approach sensor and a flap hinged at 40 mm
    /// above the cell floor that hangs to 4 mm at rest and swings toward +X.
    pub const SIDECAR: &str = r#"{
  "animated_parts": [
    {
      "axis": [0, 0, 1],
      "collision": [
        {
          "indices": [0, 1, 2, 0, 2, 3],
          "surface": "wood_end_grain",
          "vertices_um": [0, 4000, -13000, 0, 40000, -13000, 0, 40000, 13000, 0, 4000, 13000]
        }
      ],
      "limits_mdeg": [0, 85000],
      "name": "anim_flap",
      "pivot_um": [0, 40000, 0],
      "rest_mdeg": 0
    }
  ],
  "chiral": false,
  "collision": [],
  "family": "mech",
  "footprint": [[0, 0, 0]],
  "id": "mech_fixture_flap",
  "kit_contract": 1,
  "piece_version": 1,
  "ports": [
    { "cell": [0, 0, 0], "class": "channel", "face": "-x", "flow": "in", "index": 0, "inset_um": 0, "kind": "side", "level": 1 },
    { "cell": [0, 0, 0], "class": "channel", "face": "+x", "flow": "out", "index": 1, "inset_um": 0, "kind": "side", "level": 1 }
  ],
  "schema": 1,
  "sensors": [
    { "center_um": [-20000, 10000, 0], "half_extents_um": [6000, 10000, 13000], "id": "approach", "shape": "box" }
  ]
}"#;

    /// The fixture piece, parsed once.
    pub fn piece() -> &'static KitPiece {
        static PIECE: std::sync::OnceLock<KitPiece> = std::sync::OnceLock::new();
        PIECE.get_or_init(|| parble_kit::parse_sidecar(SIDECAR).expect("fixture sidecar is valid"))
    }

    /// Swing period when the placement sets no `period` config, ticks.
    pub const DEFAULT_PERIOD: i64 = 240;

    #[derive(Debug, Clone, Default)]
    pub struct SwingFlap {
        period: u64,
        swings: i64,
        passes: i64,
        last_open: i64,
    }

    impl SwingFlap {
        pub fn new(config: &MechConfig) -> Self {
            let period = config.get("period").copied().unwrap_or(DEFAULT_PERIOD);
            Self {
                period: period.max(80) as u64,
                ..Self::default()
            }
        }

        pub fn factory(config: &MechConfig) -> Box<dyn Mechanism> {
            Box::new(Self::new(config))
        }
    }

    impl Mechanism for SwingFlap {
        fn kind(&self) -> &'static str {
            "fixture_swing_flap"
        }

        fn step(&mut self, ctx: &mut MechContext<'_>) {
            let entered = ctx.sensor(0).map_or(0, |s| s.entered.len());
            self.passes += entered as i64;
            let t = ctx.tick();
            if t.is_multiple_of(self.period) {
                let open = 30_000 + i64::from(ctx.rng("swing").below(50_000));
                let open = open as i32;
                ctx.play(
                    0,
                    Keyframes::new(vec![(0, 0), (24, open), (48, open), (72, 0)])
                        .expect("increasing keys"),
                );
                self.swings += 1;
                self.last_open = i64::from(open);
                ctx.emit("swing", i64::from(open));
            }
        }

        fn save(&self) -> MechConfig {
            MechConfig::from([
                ("swings".to_owned(), self.swings),
                ("passes".to_owned(), self.passes),
                ("last_open".to_owned(), self.last_open),
            ])
        }

        fn restore(&mut self, state: &MechConfig) {
            let get = |k: &str| state.get(k).copied().unwrap_or(0);
            self.swings = get("swings");
            self.passes = get("passes");
            self.last_open = get("last_open");
        }
    }

    /// The P0 ramp with the fixture flap at cell `(6, 0, 0)`, on the flat floor in the marble's
    /// path, events recording.
    pub fn ramp_with_flap(seed: u64) -> Sim {
        let mut sim = Sim::ramp_scene();
        let p = Placement::new(
            piece().def.id.clone(),
            CellCoord::new(6, 0, 0),
            parble_core::Yaw4::new(0),
            Mirror::Off,
        );
        sim.add_mechanism(piece(), &p, seed, SwingFlap::factory(&p.config))
            .expect("free anchor");
        sim.set_event_recording(true);
        sim
    }
}

/// FNV-1a 64 over an event list: every field, floats quantized to 1 µ-unit. The wasm parity run
/// compares it ([`crate::golden::mechanism_demo`]).
pub fn events_hash(events: &[SimEvent]) -> u64 {
    use crate::hash::quantize;
    let mut h = Fnv1a64::default();
    let surface = |h: &mut Fnv1a64, s: Option<SurfaceId>| {
        h.write_i64(match s {
            None => -1,
            Some(SurfaceId::Glass) => 0,
            Some(SurfaceId::Kit(k)) => 1 + k as i64,
        });
    };
    for e in events {
        match e {
            SimEvent::Mechanism(m) => {
                h.write_u64(0);
                h.write_u64(m.tick);
                for c in [m.instance.0.x, m.instance.0.y, m.instance.0.z] {
                    h.write_i64(c.into());
                }
                match &m.kind {
                    MechEventKind::SensorEnter { sensor, marble } => {
                        h.write_u64(0);
                        h.write_u64(*sensor as u64);
                        h.write_u64(*marble as u64);
                    }
                    MechEventKind::SensorExit { sensor, marble } => {
                        h.write_u64(1);
                        h.write_u64(*sensor as u64);
                        h.write_u64(*marble as u64);
                    }
                    MechEventKind::Signal { name, value } => {
                        h.write_u64(2);
                        h.write(name.as_bytes());
                        h.write_i64(*value);
                    }
                }
            }
            SimEvent::Contact(c) => {
                h.write_u64(1);
                h.write_u64(c.tick);
                h.write_u64(c.marble as u64);
                match c.other {
                    ContactOther::Marble(j) => {
                        h.write_u64(0);
                        h.write_u64(j as u64);
                    }
                    ContactOther::Static => h.write_u64(1),
                    ContactOther::Part { instance, part } => {
                        h.write_u64(2);
                        for v in [instance.0.x, instance.0.y, instance.0.z] {
                            h.write_i64(v.into());
                        }
                        h.write_u64(part as u64);
                    }
                }
                surface(&mut h, Some(c.surface));
                h.write_i64(quantize(c.impulse, 1.0e6));
            }
            SimEvent::Roll(r) => {
                h.write_u64(2);
                h.write_u64(r.tick);
                h.write_u64(r.marble as u64);
                h.write_i64(quantize(r.speed, 1.0e6));
                surface(&mut h, r.surface);
            }
        }
    }
    h.finish()
}

#[cfg(test)]
#[path = "mechanism_test.rs"]
mod tests;
