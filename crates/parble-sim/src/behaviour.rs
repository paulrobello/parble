//! Kit mechanism behaviours (P2-T5): the state machines behind the nine `mech_*` pieces, driven
//! by the [`crate::mechanism`] framework.
//!
//! Every behaviour reads its sensors (sidecar order) and the tick (the rope arch also marble speeds,
//! [`MechContext::marble`], and drags them, [`MechContext::drag_marble`]), moves a kinematic part
//! through [`MechContext::play`] / [`MechContext::move_part`] (a physical part moves itself, see
//! [`Mechanism::part_physics`]), and reports through [`MechContext::emit`]
//! (kept in the event stream while recording, for audio and render sync). State is integers, saved
//! by [`Mechanism::save`] under keys that never use the framework's `rng/` and `part/` prefixes.
//!
//! Direction: the context carries no marble velocity, so the one-way pieces tell travel direction
//! from which of their two sensors a marble touches first ([`Heading`]).
//!
//! | piece | sensors | part | behaviour |
//! |---|---|---|---|
//! | `mech_hopper` | `release` | `anim_gate` | opens on each release tick of its queue ([`release_ticks`]), or in `sequential` mode when the run spawns the next marble, and closes once the marble has left `release` |
//! | `mech_finish` | `cup` | — | counts each marble once and keeps its arrival tick |
//! | `mech_splitter_l3` | `entry` | `anim_flap` | the flap is set before each marble arrives: to a seeded side in Random mode (the default), to the other side in Alternate mode (config [`SPLITTER_MODE`], first side `first`) |
//! | `mech_toggle_l3` | `entry` | `anim_rocker` | the rocker flips to the other side after each marble |
//! | `mech_checkpoint_l1` | `gate` | — | split tick and a camera cue per pass; its inlay lights on the first pass and stays lit for the run |
//! | `mech_gate_oneway_l1` | `approach`, `exit` | `anim_flap` (physical) | marbles from `approach` push the hinged flap open; the flap's closed stop holds the others back |
//! | `mech_hole_oneway_l1` | `trap`, `over` | `anim_trapdoor` | opens under marbles that reach `trap` first; marbles from `over` roll across the shut door |
//! | `mech_stepper_up1_l3`, `mech_stepper_up1_l1` | `lift` | `anim_slats` (slide) | cycles up and down while a marble is on the lift, eases back to rest once it is empty ([`crate::stepper`]) |
//! | `mech_arch_rope_l1` | `curtain_nx`, `curtain_px` | `anim_rope_0`..`7` (no collider) | a drag zone that halves a marble's speed; each rope row swings away from a marble entering it, and every rope bends round the marbles reaching it ([`RopeModel`], [`crate::rope`]) |
//!
//! Both fork parts are a switch blade hinged about +Y with its tip upstream, over the end wall of
//! the fork's flat frog; at its ±32° limits the tip is 12.7 mm off the channel centre, so the
//! marble meets the blade's face and is steered to the open branch. The behaviours drive the part to its limits ([`fork_angle`]),
//! and swing slowly when a marble is in the blade's sweep, so the blade eases it off the frog instead of throwing it.

use std::collections::BTreeMap;

use parble_core::{MechConfig, PieceId, ReleaseMode};

use crate::inlay::Inlay;
use crate::mechanism::{MechContext, Mechanism, MechanismRegistry};

/// The kit behaviours by piece id.
pub fn registry() -> MechanismRegistry {
    let mut r = MechanismRegistry::new();
    let id = |s: &str| PieceId::new(s).expect("kit ids follow the id grammar");
    r.register(id(HOPPER), Hopper::factory);
    r.register(id(FINISH), Finish::factory);
    r.register(id(SPLITTER), Splitter::factory);
    r.register(id(TOGGLE), Toggle::factory);
    r.register(id(CHECKPOINT), Checkpoint::factory);
    r.register(id(GATE), OneWayGate::factory);
    r.register(id(HOLE), OneWayHole::factory);
    for s in [crate::stepper::STEPPER, crate::stepper::STEPPER_L1] {
        r.register(id(s), crate::stepper::Stepper::factory);
    }
    r.register(id(ROPE_ARCH), RopeArch::factory);
    r
}

pub const HOPPER: &str = "mech_hopper";
pub const FINISH: &str = "mech_finish";
pub const SPLITTER: &str = "mech_splitter_l3";
pub const TOGGLE: &str = "mech_toggle_l3";
pub const CHECKPOINT: &str = "mech_checkpoint_l1";
pub const GATE: &str = "mech_gate_oneway_l1";
pub const HOLE: &str = "mech_hole_oneway_l1";
pub const ROPE_ARCH: &str = "mech_arch_rope_l1";

/// `ReleaseMode::All` drops the queue one marble every 48 ticks (0.2 s) so no two spawn inside
/// each other.
pub const ALL_STAGGER_TICKS: u64 = 48;

/// Release tick of each marble of an `n`-marble queue, in queue order. `One` releases only the
/// first marble. `Sequential` has a fixed tick only for the first marble (0): the run releases
/// the rest when the marbles before them have stopped moving ([`crate::track_run::TrackRun`]).
pub fn release_ticks(release: ReleaseMode, n: usize) -> Vec<u64> {
    let step = match release {
        ReleaseMode::One | ReleaseMode::Sequential => {
            return if n > 0 { vec![0] } else { Vec::new() };
        }
        ReleaseMode::All => ALL_STAGGER_TICKS,
        ReleaseMode::Interval { ticks } => u64::from(ticks.max(1)),
    };
    (0..n as u64).map(|i| i * step).collect()
}

/// Hopper config keys, filled in from [`parble_core::HopperConfig`] by
/// [`crate::track_run::TrackRun`]: queue length, release mode (0 one, 1 all, 2 interval,
/// 3 sequential) and the interval in ticks.
pub const HOPPER_QUEUE: &str = "queue";
pub const HOPPER_RELEASE: &str = "release";
pub const HOPPER_INTERVAL: &str = "interval";

/// The hopper config entries for `release` and an `n`-marble queue.
pub fn hopper_config(release: ReleaseMode, n: usize) -> [(String, i64); 3] {
    let (mode, interval) = match release {
        ReleaseMode::One => (0, 0),
        ReleaseMode::All => (1, 0),
        ReleaseMode::Interval { ticks } => (2, i64::from(ticks)),
        ReleaseMode::Sequential => (3, 0),
    };
    [
        (HOPPER_QUEUE.to_owned(), n as i64),
        (HOPPER_RELEASE.to_owned(), mode),
        (HOPPER_INTERVAL.to_owned(), interval),
    ]
}

fn get(state: &MechConfig, key: &str) -> i64 {
    state.get(key).copied().unwrap_or(0)
}

/// Hopper gate fully open, millidegrees (limit 85°).
pub const HOPPER_OPEN_MDEG: i32 = 85_000;
/// Gate opening and closing time, ticks.
pub const HOPPER_OPEN_TICKS: u32 = 6;
pub const HOPPER_CLOSE_TICKS: u32 = 24;
/// The gate closes this long after opening even if no marble left (a stalled release).
pub const HOPPER_OPEN_TIMEOUT: u64 = 480;

/// Start hopper: a queue of `queue` marbles released on [`release_ticks`]. The queue is virtual
/// (the 1-cell bowl holds one marble): the run spawns each marble in the bowl on its release tick
/// and the gate opens for it. In `sequential` mode the release tick is not known in advance, so
/// the gate opens when the run has spawned more marbles than the hopper has released.
#[derive(Debug, Clone, Default)]
pub struct Hopper {
    ticks: Vec<u64>,
    /// `Some(queue length)` in `sequential` mode.
    sequential: Option<i64>,
    released: i64,
    open: bool,
    opened_at: i64,
    passed: i64,
}

impl Hopper {
    pub fn new(config: &MechConfig) -> Self {
        let n = get(config, HOPPER_QUEUE).max(0) as usize;
        let release = match get(config, HOPPER_RELEASE) {
            1 => ReleaseMode::All,
            2 => ReleaseMode::Interval {
                ticks: get(config, HOPPER_INTERVAL).clamp(1, i64::from(u32::MAX)) as u32,
            },
            3 => ReleaseMode::Sequential,
            _ => ReleaseMode::One,
        };
        Self {
            ticks: release_ticks(release, n),
            sequential: (release == ReleaseMode::Sequential).then_some(n as i64),
            ..Self::default()
        }
    }

    pub fn factory(config: &MechConfig) -> Box<dyn Mechanism> {
        Box::new(Self::new(config))
    }
}

impl Mechanism for Hopper {
    fn kind(&self) -> &'static str {
        "hopper"
    }

    fn step(&mut self, ctx: &mut MechContext<'_>) {
        let t = ctx.tick();
        let left = ctx.sensor(0).map_or(0, |s| s.exited.len()) as i64;
        self.passed += left;
        let mut due = false;
        if let Some(n) = self.sequential {
            let spawned = (ctx.marble_count() as i64).min(n);
            while self.released < spawned {
                ctx.emit("release", self.released);
                self.released += 1;
                due = true;
            }
        } else {
            while let Some(&at) = self.ticks.get(self.released as usize) {
                if at >= t {
                    break;
                }
                ctx.emit("release", self.released);
                self.released += 1;
                due = true;
            }
        }
        if due {
            if !self.open {
                ctx.move_part(0, HOPPER_OPEN_MDEG, HOPPER_OPEN_TICKS);
                ctx.emit("gate_open", self.released - 1);
            }
            self.open = true;
            self.opened_at = t as i64;
        } else if self.open
            && (self.passed >= self.released
                || t as i64 - self.opened_at >= HOPPER_OPEN_TIMEOUT as i64)
        {
            self.open = false;
            ctx.move_part(0, 0, HOPPER_CLOSE_TICKS);
            ctx.emit("gate_close", self.released);
        }
    }

    fn save(&self) -> MechConfig {
        MechConfig::from([
            ("released".to_owned(), self.released),
            ("open".to_owned(), i64::from(self.open)),
            ("opened_at".to_owned(), self.opened_at),
            ("passed".to_owned(), self.passed),
            (
                "queue".to_owned(),
                self.sequential.unwrap_or(self.ticks.len() as i64),
            ),
        ])
    }

    fn restore(&mut self, state: &MechConfig) {
        self.released = get(state, "released");
        self.open = get(state, "open") != 0;
        self.opened_at = get(state, "opened_at");
        self.passed = get(state, "passed");
    }
}

/// Finish cup: every marble counted once, with its arrival tick.
#[derive(Debug, Clone, Default)]
pub struct Finish {
    /// Marble → arrival tick, in arrival order by `order`.
    arrived: BTreeMap<usize, i64>,
    order: Vec<usize>,
}

impl Finish {
    pub fn factory(_: &MechConfig) -> Box<dyn Mechanism> {
        Box::new(Self::default())
    }

    /// `(marble, arrival tick)` in arrival order.
    pub fn arrivals(state: &MechConfig) -> Vec<(usize, u64)> {
        (0..get(state, "count"))
            .map(|i| {
                (
                    get(state, &format!("marble/{i}")) as usize,
                    get(state, &format!("tick/{i}")) as u64,
                )
            })
            .collect()
    }
}

impl Mechanism for Finish {
    fn kind(&self) -> &'static str {
        "finish"
    }

    fn step(&mut self, ctx: &mut MechContext<'_>) {
        let entered = ctx.sensor(0).map(|s| s.entered.clone()).unwrap_or_default();
        for m in entered {
            if self.arrived.contains_key(&m) {
                continue;
            }
            self.arrived.insert(m, ctx.tick() as i64);
            self.order.push(m);
            ctx.emit("finish", m as i64);
            ctx.emit("finish_count", self.order.len() as i64);
        }
    }

    fn save(&self) -> MechConfig {
        let mut s = MechConfig::from([("count".to_owned(), self.order.len() as i64)]);
        for (i, m) in self.order.iter().enumerate() {
            s.insert(format!("marble/{i}"), *m as i64);
            s.insert(format!("tick/{i}"), self.arrived[m]);
        }
        s
    }

    fn restore(&mut self, state: &MechConfig) {
        self.order.clear();
        self.arrived.clear();
        for (m, t) in Self::arrivals(state) {
            self.order.push(m);
            self.arrived.insert(m, t as i64);
        }
    }
}

/// Checkpoint arch: each marble entering the gate sensor is a pass with a split tick (ticks since
/// the run started) and a camera cue. Its glow inlay (inlay 0) lights on the first pass and holds
/// for the rest of the run; a restarted run starts dark.
#[derive(Debug, Clone, Default)]
pub struct Checkpoint {
    passes: i64,
    last_marble: i64,
    last_split: i64,
    inlay: Inlay,
}

impl Checkpoint {
    pub fn factory(_: &MechConfig) -> Box<dyn Mechanism> {
        Box::new(Self {
            last_marble: -1,
            last_split: -1,
            ..Self::default()
        })
    }
}

impl Mechanism for Checkpoint {
    fn kind(&self) -> &'static str {
        "checkpoint"
    }

    fn step(&mut self, ctx: &mut MechContext<'_>) {
        let entered = ctx.sensor(0).map(|s| s.entered.clone()).unwrap_or_default();
        for m in entered {
            self.passes += 1;
            self.last_marble = m as i64;
            self.last_split = ctx.tick() as i64;
            self.inlay.set(true, ctx.tick());
            ctx.emit("split", self.last_split);
            ctx.emit("camera_cue", m as i64);
        }
    }

    fn save(&self) -> MechConfig {
        let mut s = MechConfig::from([
            ("passes".to_owned(), self.passes),
            ("last_marble".to_owned(), self.last_marble),
            ("last_split".to_owned(), self.last_split),
        ]);
        self.inlay.save(0, &mut s);
        s
    }

    fn restore(&mut self, state: &MechConfig) {
        self.passes = get(state, "passes");
        self.last_marble = state.get("last_marble").copied().unwrap_or(-1);
        self.last_split = state.get("last_split").copied().unwrap_or(-1);
        // State saved before the inlay existed: lit (with no fade) once a marble has passed.
        self.inlay = Inlay::restore(state, 0).unwrap_or(Inlay {
            on: self.passes > 0,
            since: None,
        });
    }
}

/// A fork's exit: `0` leaves by port 1 (+Z), `1` by port 2 (−Z).
pub type Side = i64;

/// Fork part command for `side`, millidegrees: past either limit, so the part settles at its
/// lower limit for side 0 and its upper limit for side 1 (the framework clamps). For a blade
/// hinged about +Y with its free end upstream of the pivot, a positive turn swings that end to
/// +Z and puts the marble on the −Z face (side 1).
pub fn fork_angle(side: Side) -> i32 {
    if side == 0 { -180_000 } else { 180_000 }
}

/// Fork parts move this long after a marble left the entry sensor.
pub const FORK_SETTLE_TICKS: i64 = 24;
/// Fork part travel time, ticks.
pub const FORK_MOVE_TICKS: u32 = 24;
/// Fork blade speed while a marble is in the sector it still has to sweep
/// ([`blade_sweep_occupied`]), millidegrees per tick: the tip moves at 0.05 m/s and eases the
/// marble off the frog into the branch it was sent to, where the [`FORK_MOVE_TICKS`] swing threw
/// it.
pub const FORK_NUDGE_MDEG_PER_TICK: i32 = 533;

/// The fork blade's reach from its hinge axis, m: the farthest vertex of either fork's blade
/// collider is 24.02 mm from the pivot.
pub const FORK_BLADE_REACH_M: f32 = 0.0241;
/// Half the fork blade's thickness, m.
pub const FORK_BLADE_HALF_M: f32 = 0.002;
/// The fork blade's height band above its pivot, m.
pub const FORK_BLADE_SPAN_M: [f32; 2] = [0.0005, 0.014];
/// Clearance kept between a marble and the blade's sweep when a move starts and while the blade
/// eases, m: the contact reaches a marble before the shapes touch (a steel mini 0.1 mm beyond
/// the tip's arc, rolling at 0.012 m/s, took a 0.58 m/s kick from the full-speed swing). A swing
/// already under way slows only for a marble it would touch: the tip passes a glazed shooter
/// arriving at 0.39 m/s 1.1 mm clear, where an easing blade stops in its path and throws it back
/// up the entry (all-pieces track, seed 2).
pub const FORK_BLADE_CLEAR_M: f32 = 0.002;

/// Whether a marble overlaps the sector the fork blade (part 0) still has to sweep, between
/// angles `from` and `to` (millidegrees), with `clear` to spare. The entry sensor (sensor 0)
/// lies upstream of the hinge, where the blade's tip points.
///
/// The entry sensor ends 5 mm short of the blade tip, so a marble that leaves it is over the
/// blade, and a slow one is still there [`FORK_SETTLE_TICKS`] later. The kinematic blade's full
/// speed swing into a steel mini rocking on the frog at 0.14 m/s threw it out of the cell at
/// 1.2 m/s (all-pieces track, seeds 6 and 7, 2026-11-10). Only marbles there now count: easing
/// for one still rolling in parks the blade in its path (a following marble bounced off a toggle
/// and rolled back up through the one above it, which counted it twice; seeds 3, 4 and 9).
fn blade_sweep_occupied(ctx: &MechContext<'_>, from: i32, to: i32, clear: f32) -> bool {
    (0..ctx.marble_count()).any(|m| blade_covers(ctx, m, from.min(to), from.max(to), clear))
}

/// Whether marble `m` overlaps the sector the fork blade (part 0) covers between angles `lo` and
/// `hi` (millidegrees), with `clear` to spare. The blade is a capsule of [`FORK_BLADE_HALF_M`]
/// radius from the hinge axis out to [`FORK_BLADE_REACH_M`], within [`FORK_BLADE_SPAN_M`].
fn blade_covers(ctx: &MechContext<'_>, m: usize, lo: i32, hi: i32, clear: f32) -> bool {
    let (Some((pivot, axis, _)), Some((smin, smax)), Some((p, _, r))) =
        (ctx.part_hinge(0), ctx.sensor_bounds(0), ctx.marble_body(m))
    else {
        return false;
    };
    let rad = core::f32::consts::PI / 180_000.0;
    let to_entry = (smin + smax) * 0.5 - pivot;
    let up = (to_entry - axis * to_entry.dot(axis)).normalize();
    let side = axis.cross(up);
    let r = r + clear;
    let d = p - pivot;
    let dy = d.dot(axis);
    if dy < FORK_BLADE_SPAN_M[0] - r || dy > FORK_BLADE_SPAN_M[1] + r {
        return false;
    }
    let (along, across) = (d.dot(up), d.dot(side));
    let (near, length) = (
        r + FORK_BLADE_HALF_M,
        FORK_BLADE_REACH_M - FORK_BLADE_HALF_M,
    );
    let phi = libm::atan2f(across, along);
    let (lo, hi) = (lo as f32 * rad, hi as f32 * rad);
    if phi >= lo && phi <= hi {
        return libm::sqrtf(along * along + across * across) <= length + near;
    }
    // Outside the swept angles the nearest blade pose is the one at the closer end.
    let edge = if libm::fabsf(phi - lo) < libm::fabsf(phi - hi) {
        lo
    } else {
        hi
    };
    let (u, w) = (libm::cosf(edge), libm::sinf(edge));
    let t = (along * u + across * w).clamp(0.0, length);
    let (ex, ey) = (along - t * u, across - t * w);
    ex * ex + ey * ey <= near * near
}

/// Shared fork state: the side set for the next marble and a pending move.
#[derive(Debug, Clone, Default)]
struct Fork {
    started: bool,
    side: Side,
    passes: i64,
    /// Tick at which the part moves to `side` (-1: none pending).
    move_at: i64,
    /// The marble that made the last pass, while it is still in the entry sensor or the blade's
    /// sweep: a marble rocking across the sensor's edge leaves it again, and is not counted
    /// twice.
    held: Option<usize>,
    /// The blade's goal while it moves there, and whether it is easing past a marble.
    target: Option<i32>,
    slow: bool,
}

impl Fork {
    fn save(&self) -> MechConfig {
        MechConfig::from([
            ("started".to_owned(), i64::from(self.started)),
            ("side".to_owned(), self.side),
            ("passes".to_owned(), self.passes),
            ("move_at".to_owned(), self.move_at),
            ("held".to_owned(), self.held.map_or(-1, |m| m as i64)),
            ("target".to_owned(), self.target.map_or(i64::MIN, i64::from)),
            ("slow".to_owned(), i64::from(self.slow)),
        ])
    }

    fn restore(&mut self, state: &MechConfig) {
        self.started = get(state, "started") != 0;
        self.side = get(state, "side");
        self.passes = get(state, "passes");
        self.move_at = state.get("move_at").copied().unwrap_or(-1);
        self.held = state.get("held").and_then(|&m| usize::try_from(m).ok());
        self.target = state.get("target").and_then(|&a| i32::try_from(a).ok());
        self.slow = get(state, "slow") != 0;
    }

    /// One tick: `next` picks the side for the coming marble; `angle` maps a side to the part.
    fn step(
        &mut self,
        ctx: &mut MechContext<'_>,
        next: &mut dyn FnMut(&mut MechContext<'_>, Side) -> Side,
        angle: fn(Side) -> i32,
    ) {
        let t = ctx.tick() as i64;
        if !self.started {
            self.started = true;
            self.side = next(ctx, -1);
            ctx.move_part(0, angle(self.side), FORK_MOVE_TICKS);
            ctx.emit("set", self.side);
            self.move_at = -1;
        }
        let left: Vec<usize> = ctx.sensor(0).map_or(Vec::new(), |s| s.exited.clone());
        for m in left {
            if self.held == Some(m) {
                continue;
            }
            self.held = Some(m);
            ctx.emit("pass", self.side);
            self.passes += 1;
            self.side = next(ctx, self.side);
            self.move_at = t + FORK_SETTLE_TICKS;
        }
        let starting = self.move_at >= 0 && t >= self.move_at;
        if starting {
            self.move_at = -1;
            let to = angle(self.side);
            let [lo, hi] = ctx.part_limits(0).unwrap_or([to, to]);
            self.target = Some(to.clamp(lo, hi));
            self.slow = false;
            ctx.move_part(0, to, FORK_MOVE_TICKS);
            ctx.emit("set", self.side);
        }
        self.drive(ctx, angle(self.side), starting);
        if let Some(m) = self.held
            && !ctx.sensor(0).is_some_and(|s| s.occupants.contains(&m))
            && !ctx
                .part_limits(0)
                .is_some_and(|[lo, hi]| blade_covers(ctx, m, lo, hi, FORK_BLADE_CLEAR_M))
        {
            self.held = None;
        }
    }

    /// Re-check the blade's move to `target` each tick: it eases at [`FORK_NUDGE_MDEG_PER_TICK`]
    /// while a marble is in the sector it still has to sweep ([`blade_sweep_occupied`]), and
    /// swings at full speed toward `command` otherwise.
    fn drive(&mut self, ctx: &mut MechContext<'_>, command: i32, starting: bool) {
        let (Some(target), Some(at)) = (self.target, ctx.part_angle(0)) else {
            return;
        };
        if at == target {
            self.target = None;
            self.slow = false;
            return;
        }
        let clear = if starting || self.slow {
            FORK_BLADE_CLEAR_M
        } else {
            0.0
        };
        let occupied = blade_sweep_occupied(ctx, at, target, clear);
        if occupied && !self.slow {
            self.slow = true;
            let left = (target - at).unsigned_abs();
            ctx.move_part(0, target, left.div_ceil(FORK_NUDGE_MDEG_PER_TICK as u32));
        } else if !occupied && self.slow {
            self.slow = false;
            ctx.move_part(0, command, FORK_MOVE_TICKS);
        }
    }
}

/// Splitter config key: [`SPLITTER_RANDOM`] (the default, also for a placement without the key)
/// or [`SPLITTER_ALTERNATE`].
pub const SPLITTER_MODE: &str = "mode";
pub const SPLITTER_RANDOM: i64 = 0;
pub const SPLITTER_ALTERNATE: i64 = 1;

/// Splitter. In Random mode each marble's side is the next
/// [`crate::mechanism::MechRng::below`]`(2)` of the instance's `choice` stream, drawn when the
/// previous marble has passed. In Alternate mode it draws nothing: the first marble goes to side
/// `first` (config, default 0: +Z) and the sides alternate after it, as on the toggle.
#[derive(Debug, Clone, Default)]
pub struct Splitter {
    fork: Fork,
    /// `Some(first side)` in Alternate mode.
    alternate: Option<Side>,
}

impl Splitter {
    pub fn factory(config: &MechConfig) -> Box<dyn Mechanism> {
        Box::new(Self {
            fork: Fork {
                move_at: -1,
                ..Fork::default()
            },
            alternate: (get(config, SPLITTER_MODE) == SPLITTER_ALTERNATE)
                .then(|| get(config, "first").clamp(0, 1)),
        })
    }
}

/// The splitter's RNG stream name.
pub const SPLITTER_STREAM: &str = "choice";

impl Mechanism for Splitter {
    fn kind(&self) -> &'static str {
        "splitter"
    }

    fn step(&mut self, ctx: &mut MechContext<'_>) {
        match self.alternate {
            None => self.fork.step(
                ctx,
                &mut |ctx, _| i64::from(ctx.rng(SPLITTER_STREAM).below(2)),
                fork_angle,
            ),
            Some(first) => self.fork.step(
                ctx,
                &mut |_, prev| if prev < 0 { first } else { 1 - prev },
                fork_angle,
            ),
        }
    }

    fn save(&self) -> MechConfig {
        self.fork.save()
    }

    fn restore(&mut self, state: &MechConfig) {
        self.fork.restore(state);
    }
}

/// L/R toggle: the first marble goes to side `first` (config, default 0: +Z), then the rocker
/// alternates after every marble.
#[derive(Debug, Clone, Default)]
pub struct Toggle {
    fork: Fork,
    first: Side,
}

impl Toggle {
    pub fn factory(config: &MechConfig) -> Box<dyn Mechanism> {
        Box::new(Self {
            fork: Fork {
                move_at: -1,
                ..Fork::default()
            },
            first: get(config, "first").clamp(0, 1),
        })
    }
}

impl Mechanism for Toggle {
    fn kind(&self) -> &'static str {
        "toggle"
    }

    fn step(&mut self, ctx: &mut MechContext<'_>) {
        let first = self.first;
        self.fork.step(
            ctx,
            &mut |_, prev| if prev < 0 { first } else { 1 - prev },
            fork_angle,
        );
    }

    fn save(&self) -> MechConfig {
        self.fork.save()
    }

    fn restore(&mut self, state: &MechConfig) {
        self.fork.restore(state);
    }
}

/// Which sensor of a two-sensor piece each marble touched first, kept until the marble has been
/// in neither sensor for [`HEADING_FORGET_TICKS`] (a behaviour can keep marbles longer, see
/// [`Heading::update`]).
#[derive(Debug, Clone, Default)]
struct Heading {
    /// marble → (first sensor, last tick seen in either sensor)
    seen: BTreeMap<usize, (usize, i64)>,
}

/// A marble that has been out of both sensors this long is forgotten (it left the piece), unless
/// the behaviour keeps it.
pub const HEADING_FORGET_TICKS: i64 = 240;

impl Heading {
    /// Update from both sensors and return the marbles newly seen at each sensor. A marble for
    /// which `keep` holds is never forgotten, however long it has been out of both sensors.
    fn update(&mut self, ctx: &MechContext<'_>, keep: impl Fn(usize) -> bool) -> [Vec<usize>; 2] {
        let t = ctx.tick() as i64;
        let mut new: [Vec<usize>; 2] = Default::default();
        for (si, fresh) in new.iter_mut().enumerate() {
            let Some(s) = ctx.sensor(si) else { continue };
            for &m in &s.occupants {
                match self.seen.get_mut(&m) {
                    Some(e) => e.1 = t,
                    None => {
                        self.seen.insert(m, (si, t));
                        fresh.push(m);
                    }
                }
            }
        }
        self.seen
            .retain(|&m, e| t - e.1 < HEADING_FORGET_TICKS || keep(m));
        new
    }

    /// Marbles still on the piece that arrived at `sensor` first.
    fn from(&self, sensor: usize) -> impl Iterator<Item = usize> + '_ {
        self.seen
            .iter()
            .filter(move |(_, e)| e.0 == sensor)
            .map(|(&m, _)| m)
    }

    fn save(&self, s: &mut MechConfig) {
        for (m, (si, t)) in &self.seen {
            s.insert(format!("first/{m}"), *si as i64);
            s.insert(format!("seen/{m}"), *t);
        }
    }

    fn restore(&mut self, state: &MechConfig) {
        self.seen.clear();
        for (k, &v) in state {
            if let Some(m) = k
                .strip_prefix("first/")
                .and_then(|m| m.parse::<usize>().ok())
            {
                let t = get(state, &format!("seen/{m}"));
                self.seen.insert(m, (v as usize, t));
            }
        }
    }
}

/// The gate reports `open` when the flap swings past this angle, millidegrees.
pub const GATE_OPEN_MDEG: i32 = 20_000;
/// and `close` when it falls back below this one.
pub const GATE_CLOSED_MDEG: i32 = 2_000;

/// The gate flap's physics: a counterweighted flap hanging from its top edge, returned to its
/// closed stop by gravity alone. The damping settles it on the stop in a few small bounces.
pub const GATE_FLAP: crate::mechanism::PartPhysics = crate::mechanism::PartPhysics {
    mass_kg: GATE_FLAP_MASS_KG,
    angular_damping: 2.0,
    com_drop_m: Some(
        GATE_CLOSING_TORQUE_NM_AT_20DEG / (GATE_FLAP_MASS_KG * crate::marble::G * SIN_20DEG),
    ),
};

/// A 34 × 24 × 2 mm wooden plate with its counterweight.
pub const GATE_FLAP_MASS_KG: f32 = 0.001;

/// Gravity torque closing the counterweighted flap at 20° open, N·m (centre of mass 0.30 mm below
/// the pivot). To let a marble out the flap swings about 55°, until its tip clears the marble's
/// top at 31 mm. Uncounterweighted (0.3 g, centre of mass 17 mm down) that lift costs 2.1e-5 J,
/// more than a 5.4 g cat's eye carries below about 0.09 m/s, so slow marbles stopped under the
/// flap. At 1e-6 the lift costs 1.3e-6 J and the flap still falls shut behind every marble from
/// [`GATE_MIN_LAUNCH_SPEED_M_S`] up (measured 2026-11-08,
/// `one_way_gate_flap_passes_every_forward_speed`). A tuned value, not a sourced one.
pub const GATE_CLOSING_TORQUE_NM_AT_20DEG: f32 = 1.0e-6;

const SIN_20DEG: f32 = 0.342_020_15;

/// Slowest launch speed, m/s, at which a cat's eye gets through the gate on the L1 test layout
/// (launched at x = −20 mm on a level L1 straight, the flap's pivot at 60 mm), on the 5 mm/s
/// grid. From energy: rolling resistance decelerates it at 14 mm/s² over the 116 mm to where the
/// open flap's tip clears its back, and the flap's lift takes 1.3e-6 J, so
/// v² ≥ 2 × 0.014 × 0.116 + 2 × 1.3e-6 / (7/5 × 0.0054), v ≥ 0.060 m/s. At 0.06 it stops under
/// the flap (x 81 mm) and at 0.05 it stops at the flap even with no flap there (x 61 mm).
pub const GATE_MIN_LAUNCH_SPEED_M_S: f32 = 0.065;

/// One-way gate: the flap is a dynamic body hinged at its top edge ([`GATE_FLAP`]). A marble from
/// `approach` (−X, the in port) pushes it open toward +X and rolls under it; a marble from `exit`
/// pushes it against its closed stop and goes back. `forward` / `reverse` say which sensor a
/// marble reached first, `open` / `close` follow the flap angle.
#[derive(Debug, Clone, Default)]
pub struct OneWayGate {
    heading: Heading,
    open: bool,
    passes: i64,
    blocked: i64,
}

impl OneWayGate {
    pub fn factory(_: &MechConfig) -> Box<dyn Mechanism> {
        Box::new(Self::default())
    }
}

/// Kinematic open/close of the one-way hole: open while any marble from `trigger` is in a sensor,
/// close `hold` ticks after the last one.
#[allow(clippy::too_many_arguments)]
fn one_way_step(
    ctx: &mut MechContext<'_>,
    heading: &mut Heading,
    open: &mut bool,
    last_forward: &mut i64,
    passes: &mut i64,
    blocked: &mut i64,
    trigger: usize,
    open_mdeg: i32,
    open_ticks: u32,
    close_ticks: u32,
    hold: i64,
    names: [&'static str; 4],
) {
    let t = ctx.tick() as i64;
    // A marble resting on the piece between the sensors keeps its heading until it leaves.
    let fresh = heading.update(ctx, |m| ctx.between_sensors(0, 1, m));
    for &m in &fresh[trigger] {
        *passes += 1;
        ctx.emit(names[0], m as i64);
    }
    for &m in &fresh[1 - trigger] {
        *blocked += 1;
        ctx.emit(names[1], m as i64);
    }
    let in_sensor =
        |m: usize| (0..2).any(|si| ctx.sensor(si).is_some_and(|s| s.occupants.contains(&m)));
    let forward_in = heading.from(trigger).any(in_sensor);
    if forward_in {
        *last_forward = t;
        if !*open {
            *open = true;
            ctx.move_part(0, open_mdeg, open_ticks);
            ctx.emit(names[2], 1);
        }
    } else if *open && t - *last_forward >= hold {
        *open = false;
        ctx.move_part(0, 0, close_ticks);
        ctx.emit(names[3], 0);
    }
}

impl Mechanism for OneWayGate {
    fn kind(&self) -> &'static str {
        "gate_oneway"
    }

    fn step(&mut self, ctx: &mut MechContext<'_>) {
        let fresh = self.heading.update(ctx, |_| false);
        for &m in &fresh[0] {
            self.passes += 1;
            ctx.emit("forward", m as i64);
        }
        for &m in &fresh[1] {
            self.blocked += 1;
            ctx.emit("reverse", m as i64);
        }
        let angle = ctx.part_angle(0).unwrap_or(0);
        // A shut flap pushed from `exit` rests on its stop in the frame.
        let in_sensor =
            |m: usize| (0..2).any(|si| ctx.sensor(si).is_some_and(|s| s.occupants.contains(&m)));
        let reverse_in = self.heading.from(1).any(in_sensor);
        let forward_in = self.heading.from(0).any(in_sensor);
        ctx.brace_part(0, reverse_in && !forward_in && angle <= GATE_CLOSED_MDEG);
        if !self.open && angle >= GATE_OPEN_MDEG {
            self.open = true;
            ctx.emit("open", 1);
        } else if self.open && angle <= GATE_CLOSED_MDEG {
            self.open = false;
            ctx.emit("close", 0);
        }
    }

    fn save(&self) -> MechConfig {
        let mut s = MechConfig::from([
            ("open".to_owned(), i64::from(self.open)),
            ("passes".to_owned(), self.passes),
            ("blocked".to_owned(), self.blocked),
        ]);
        self.heading.save(&mut s);
        s
    }

    fn restore(&mut self, state: &MechConfig) {
        self.open = get(state, "open") != 0;
        self.passes = get(state, "passes");
        self.blocked = get(state, "blocked");
        self.heading.restore(state);
    }

    fn part_physics(&self, part: usize) -> Option<crate::mechanism::PartPhysics> {
        (part == 0).then_some(GATE_FLAP)
    }
}

/// One-way hole trapdoor open angle and timing (limit 90°: the door hangs along the bore wall).
pub const HOLE_OPEN_MDEG: i32 = 90_000;
pub const HOLE_OPEN_TICKS: u32 = 6;
pub const HOLE_CLOSE_TICKS: u32 = 24;
/// The trapdoor stays open this long after the last trap-side marble left the sensors, so it
/// never swings up into a marble still falling through the bore.
pub const HOLE_HOLD_TICKS: i64 = 72;

/// One-way hole: a marble that reaches `trap` (−X) first drops through the opened trapdoor to the
/// bottom port; a marble that reaches `over` (+X) first rolls across the shut door and leaves by −X.
/// A marble keeps its heading while it is in or between the two sensors, so one parked on the
/// shut door never opens it.
#[derive(Debug, Clone, Default)]
pub struct OneWayHole {
    heading: Heading,
    open: bool,
    last_trap: i64,
    drops: i64,
    overs: i64,
}

impl OneWayHole {
    pub fn factory(_: &MechConfig) -> Box<dyn Mechanism> {
        Box::new(Self::default())
    }
}

impl Mechanism for OneWayHole {
    fn kind(&self) -> &'static str {
        "hole_oneway"
    }

    fn step(&mut self, ctx: &mut MechContext<'_>) {
        one_way_step(
            ctx,
            &mut self.heading,
            &mut self.open,
            &mut self.last_trap,
            &mut self.drops,
            &mut self.overs,
            0,
            HOLE_OPEN_MDEG,
            HOLE_OPEN_TICKS,
            HOLE_CLOSE_TICKS,
            HOLE_HOLD_TICKS,
            ["drop", "over", "open", "close"],
        );
    }

    fn save(&self) -> MechConfig {
        let mut s = MechConfig::from([
            ("open".to_owned(), i64::from(self.open)),
            ("last_trap".to_owned(), self.last_trap),
            ("drops".to_owned(), self.drops),
            ("overs".to_owned(), self.overs),
        ]);
        self.heading.save(&mut s);
        s
    }

    fn restore(&mut self, state: &MechConfig) {
        self.open = get(state, "open") != 0;
        self.last_trap = get(state, "last_trap");
        self.drops = get(state, "drops");
        self.overs = get(state, "overs");
        self.heading.restore(state);
    }
    fn part_flush_door(&self, part: usize) -> bool {
        part == 0
    }
}

/// Rope-curtain arch: half the length of its drag zone along travel, µm (the `curtain_nx` and
/// `curtain_px` sensors, one per rope row, side by side over the arch).
pub const ROPE_ARCH_HALF_UM: i32 = 12_000;
/// Share of its speed a marble keeps across the curtain.
pub const ROPE_KEEP: f32 = 0.5;
/// Slowest speed, m/s, at which a marble enters the curtain zone and still has its speed halved
/// to within ±0.025, measured against the same pass with no curtain. Below it the 14 mm/s²
/// rolling resistance across the zone is significant: rolling at `a` against the curtain's
/// `k = ln 2 / L` over the `L` = 40 mm the marble's centre covers, the exit speed squared is
/// `v²/4 − 3a/(4k)` with the curtain and `v² − 2aL` without it, and their ratio's square root
/// falls to 0.475 at 0.1204 m/s. Half of the ±0.05 test band goes to that, the other half to the
/// drag's own error (0.533 measured at 0.8 m/s). Slower marbles still get through, slowed.
pub const ROPE_MIN_HALVING_SPEED_M_S: f32 = 0.121;
/// Rope kick per m/s of speed the curtain takes from the marble, millidegrees, and its bounds:
/// the peak a row's top segments swing to ([`crate::rope::RopeChain::kick`]).
pub const ROPE_SWING_GAIN: f32 = 100_000.0;
pub const ROPE_SWING_MIN_MDEG: i32 = 8_000;
pub const ROPE_SWING_MAX_MDEG: i32 = 40_000;
/// Ropes per row; the outer rope at each wall swings this share of the inner ropes' kick.
pub const ROPES_PER_ROW: usize = 4;
const ROPE_OUTER_SHARE: f32 = 0.6;
const ROPES: usize = 2 * ROPES_PER_ROW;

/// How the arch's ropes move. The curtain's drag does not depend on it.
///
/// `Cosmetic` (shipped): each rope is a kinematic chain of segments with no collider
/// ([`crate::rope`]), kicked when a marble enters its row and bent out of the way of every marble
/// that reaches it. Its part angle is the pivot-to-tip chord; the renderer draws the strand along
/// the segments ([`Mechanism::part_segments`]). To make the ropes physical, add a variant whose
/// `physics` returns a light [`crate::mechanism::PartPhysics`] (a dynamic body on the sidecar
/// hinge whose collider is the strand, already the sidecar part collision), then point
/// [`ROPE_MODEL`] at it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RopeModel {
    Cosmetic,
}

/// The rope model the arch is built with.
pub const ROPE_MODEL: RopeModel = RopeModel::Cosmetic;

impl RopeModel {
    fn physics(self, _part: usize) -> Option<crate::mechanism::PartPhysics> {
        match self {
            Self::Cosmetic => None,
        }
    }

    fn collides(self, _part: usize) -> bool {
        match self {
            Self::Cosmetic => false,
        }
    }
}

/// Every marble near rope `part` as a disc in its swing plane ([`crate::rope::Disc`]).
fn rope_discs(ctx: &MechContext<'_>, part: usize) -> Vec<crate::rope::Disc> {
    use crate::rope::{CLEAR_M, Disc, LENGTH_UM, STRAND_R_M};
    use rapier3d::prelude::Vector;
    let Some((pivot, axis, sign)) = ctx.part_hinge(part) else {
        return Vec::new();
    };
    // A positive part angle swings the strand's lower end along `u`.
    let u = Vector::Y.cross(axis) * sign;
    let length = LENGTH_UM as f32 * 1.0e-6;
    (0..ctx.marble_count())
        .filter_map(|m| ctx.marble_body(m))
        .filter_map(|(c, v, r)| {
            let rel = c - pivot;
            let reach = r + STRAND_R_M + CLEAR_M;
            let dz = rel.dot(axis);
            let (du, dy) = (rel.dot(u), rel.y);
            let far = length + reach;
            if dz.abs() >= reach || du * du + dy * dy > far * far {
                return None;
            }
            Some(Disc {
                u: du,
                y: dy,
                rho: libm::sqrtf(reach * reach - dz * dz),
                vu: v.dot(u),
            })
        })
        .collect()
}

/// Rope-curtain arch: every marble touching a curtain sensor gets a quadratic drag sized so it
/// keeps [`ROPE_KEEP`] of its speed over the zone ([`MechContext::drag_marble`]), each rope row
/// swings away from a marble entering it, and every rope bends out of the way of the marbles
/// reaching it ([`RopeModel`]). Signals: `pass` once per marble, `swing` per kicked row with the
/// inner ropes' kick in millidegrees.
#[derive(Debug, Clone, Default)]
pub struct RopeArch {
    heading: Heading,
    passes: i64,
    swings: i64,
    ropes: [crate::rope::RopeChain; ROPES],
}

impl RopeArch {
    pub fn factory(_: &MechConfig) -> Box<dyn Mechanism> {
        Box::new(Self::default())
    }

    /// Drag constant (per metre) that keeps [`ROPE_KEEP`] of a marble's speed while its surface
    /// is in the zone: its centre covers the zone plus its radius on each side.
    pub fn drag_k(radius_m: f32) -> f32 {
        let zone = 2.0 * (ROPE_ARCH_HALF_UM as f32 * 1.0e-6 + radius_m);
        -libm::logf(ROPE_KEEP) / zone
    }
}

#[cfg(test)]
thread_local! {
    /// Test switch: the rope arch's curtain applies no drag, for its zero-drag reference pass.
    pub(crate) static ROPE_DRAG_OFF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn rope_drag_on() -> bool {
    !ROPE_DRAG_OFF.with(std::cell::Cell::get)
}

#[cfg(not(test))]
fn rope_drag_on() -> bool {
    true
}

impl Mechanism for RopeArch {
    fn kind(&self) -> &'static str {
        "arch_rope"
    }

    fn step(&mut self, ctx: &mut MechContext<'_>) {
        let fresh = self.heading.update(ctx, |_| false);
        for &m in fresh.iter().flatten() {
            self.passes += 1;
            ctx.emit("pass", m as i64);
        }
        let mut inside = BTreeMap::new();
        let mut entered = Vec::new();
        for row in 0..2 {
            if let Some(s) = ctx.sensor(row) {
                inside.extend(s.occupants.iter().map(|&m| (m, ())));
                entered.extend(s.entered.iter().map(|&m| (row, m)));
            }
        }
        for &m in inside.keys().filter(|_| rope_drag_on()) {
            if let Some((_, r)) = ctx.marble(m) {
                ctx.drag_marble(m, Self::drag_k(r));
            }
        }
        for (row, m) in entered {
            let Some((v, _)) = ctx.marble(m) else {
                continue;
            };
            let dir = match self.heading.seen.get(&m) {
                Some((1, _)) => -1.0,
                _ => 1.0,
            };
            let kick = (v * (1.0 - ROPE_KEEP) * ROPE_SWING_GAIN)
                .clamp(ROPE_SWING_MIN_MDEG as f32, ROPE_SWING_MAX_MDEG as f32);
            for i in 0..ROPES_PER_ROW {
                let share = if i == 0 || i == ROPES_PER_ROW - 1 {
                    ROPE_OUTER_SHARE
                } else {
                    1.0
                };
                let mdeg = libm::roundf(dir * kick * share) as i32;
                self.ropes[row * ROPES_PER_ROW + i]
                    .kick(mdeg as f32 * (core::f32::consts::PI / 180_000.0));
            }
            self.swings += 1;
            ctx.emit("swing", libm::roundf(dir * kick) as i64);
        }
        for (k, rope) in self.ropes.iter_mut().enumerate() {
            let discs = rope_discs(ctx, k);
            rope.step(&discs);
            let [lo, hi] = ctx.part_limits(k).unwrap_or([i32::MIN, i32::MAX]);
            let chord = crate::rope::to_mdeg(rope.chord()).clamp(lo, hi);
            if ctx.part_angle(k) != Some(chord) {
                ctx.play(k, crate::mechanism::Keyframes::hold(chord));
            }
        }
    }

    fn save(&self) -> MechConfig {
        let mut s = MechConfig::from([
            ("passes".to_owned(), self.passes),
            ("swings".to_owned(), self.swings),
        ]);
        self.heading.save(&mut s);
        // Exact `f32` bits, so a restored chain continues bit for bit.
        for (k, rope) in self.ropes.iter().enumerate() {
            for j in 0..crate::rope::SEGMENTS {
                s.insert(
                    format!("rope/{k}/{j}/a"),
                    i64::from(rope.angle[j].to_bits()),
                );
                s.insert(format!("rope/{k}/{j}/w"), i64::from(rope.rate[j].to_bits()));
            }
        }
        s
    }

    fn restore(&mut self, state: &MechConfig) {
        self.passes = get(state, "passes");
        self.swings = get(state, "swings");
        self.heading.restore(state);
        let bits = |key: String| f32::from_bits(get(state, &key) as u32);
        for (k, rope) in self.ropes.iter_mut().enumerate() {
            for j in 0..crate::rope::SEGMENTS {
                rope.angle[j] = bits(format!("rope/{k}/{j}/a"));
                rope.rate[j] = bits(format!("rope/{k}/{j}/w"));
            }
        }
    }

    fn part_physics(&self, part: usize) -> Option<crate::mechanism::PartPhysics> {
        ROPE_MODEL.physics(part)
    }

    fn part_collides(&self, part: usize) -> bool {
        ROPE_MODEL.collides(part)
    }

    fn part_segments(&self, part: usize) -> Vec<i32> {
        self.ropes
            .get(part)
            .map(|r| r.angle.iter().map(|&a| crate::rope::to_mdeg(a)).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
#[path = "behaviour_test.rs"]
mod tests;
