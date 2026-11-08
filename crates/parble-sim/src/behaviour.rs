//! Kit mechanism behaviours (P2-T5): the state machines behind the seven `mech_*` pieces, driven
//! by the [`crate::mechanism`] framework.
//!
//! Every behaviour reads only its sensors (sidecar order) and the tick, moves its part through
//! [`MechContext::play`] / [`MechContext::move_part`], and reports through [`MechContext::emit`]
//! (kept in the event stream while recording, for audio and render sync). State is integers, saved
//! by [`Mechanism::save`] under keys that never use the framework's `rng/` and `part/` prefixes.
//!
//! Direction: the context carries no marble velocity, so the one-way pieces tell travel direction
//! from which of their two sensors a marble touches first ([`Heading`]).
//!
//! | piece | sensors | part | behaviour |
//! |---|---|---|---|
//! | `mech_hopper` | `release` | `anim_gate` | opens on each release tick of its queue ([`release_ticks`]), closes once the marble has left `release` |
//! | `mech_finish` | `cup` | — | counts each marble once and keeps its arrival tick |
//! | `mech_splitter_l3` | `entry` | `anim_flap` | the flap is set to a seeded side before each marble arrives |
//! | `mech_toggle_l3` | `entry` | `anim_rocker` | the rocker flips to the other side after each marble |
//! | `mech_checkpoint_l1` | `gate` | — | split tick and a camera cue per pass |
//! | `mech_gate_oneway_l1` | `approach`, `exit` | `anim_flap` | opens for marbles that reach `approach` first; the closed flap blocks the others |
//! | `mech_hole_oneway_l1` | `trap`, `over` | `anim_trapdoor` | opens under marbles that reach `trap` first; marbles from `over` roll across the shut door |
//!
//! Both fork parts are a switch blade hinged about +Y with its tip upstream; at its ±25° limits
//! the tip is 10.1 mm off the channel centre, so the marble meets the blade's face and is steered
//! to the open branch. The behaviours drive the part to its limits ([`fork_angle`]).

use std::collections::BTreeMap;

use parble_core::{MechConfig, PieceId, ReleaseMode};

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
    r
}

pub const HOPPER: &str = "mech_hopper";
pub const FINISH: &str = "mech_finish";
pub const SPLITTER: &str = "mech_splitter_l3";
pub const TOGGLE: &str = "mech_toggle_l3";
pub const CHECKPOINT: &str = "mech_checkpoint_l1";
pub const GATE: &str = "mech_gate_oneway_l1";
pub const HOLE: &str = "mech_hole_oneway_l1";

/// `ReleaseMode::All` drops the queue one marble every 48 ticks (0.2 s) so no two spawn inside
/// each other.
pub const ALL_STAGGER_TICKS: u64 = 48;

/// Release tick of each marble of an `n`-marble queue, in queue order. `One` releases only the
/// first marble.
pub fn release_ticks(release: ReleaseMode, n: usize) -> Vec<u64> {
    let step = match release {
        ReleaseMode::One => return if n > 0 { vec![0] } else { Vec::new() },
        ReleaseMode::All => ALL_STAGGER_TICKS,
        ReleaseMode::Interval { ticks } => u64::from(ticks.max(1)),
    };
    (0..n as u64).map(|i| i * step).collect()
}

/// Hopper config keys, filled in from [`parble_core::HopperConfig`] by
/// [`crate::track_run::TrackRun`]: queue length, release mode (0 one, 1 all, 2 interval) and the
/// interval in ticks.
pub const HOPPER_QUEUE: &str = "queue";
pub const HOPPER_RELEASE: &str = "release";
pub const HOPPER_INTERVAL: &str = "interval";

/// The hopper config entries for `release` and an `n`-marble queue.
pub fn hopper_config(release: ReleaseMode, n: usize) -> [(String, i64); 3] {
    let (mode, interval) = match release {
        ReleaseMode::One => (0, 0),
        ReleaseMode::All => (1, 0),
        ReleaseMode::Interval { ticks } => (2, i64::from(ticks)),
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
/// and the gate opens for it.
#[derive(Debug, Clone, Default)]
pub struct Hopper {
    ticks: Vec<u64>,
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
            _ => ReleaseMode::One,
        };
        Self {
            ticks: release_ticks(release, n),
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
        while let Some(&at) = self.ticks.get(self.released as usize) {
            if at >= t {
                break;
            }
            ctx.emit("release", self.released);
            self.released += 1;
            due = true;
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
            ("queue".to_owned(), self.ticks.len() as i64),
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
/// the run started) and a camera cue.
#[derive(Debug, Clone, Default)]
pub struct Checkpoint {
    passes: i64,
    last_marble: i64,
    last_split: i64,
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
            ctx.emit("split", self.last_split);
            ctx.emit("camera_cue", m as i64);
        }
    }

    fn save(&self) -> MechConfig {
        MechConfig::from([
            ("passes".to_owned(), self.passes),
            ("last_marble".to_owned(), self.last_marble),
            ("last_split".to_owned(), self.last_split),
        ])
    }

    fn restore(&mut self, state: &MechConfig) {
        self.passes = get(state, "passes");
        self.last_marble = state.get("last_marble").copied().unwrap_or(-1);
        self.last_split = state.get("last_split").copied().unwrap_or(-1);
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

/// Fork parts move this long after a marble left the entry sensor, so the part never moves under
/// a marble still on it.
pub const FORK_SETTLE_TICKS: i64 = 24;
/// Fork part travel time, ticks.
pub const FORK_MOVE_TICKS: u32 = 24;

/// Shared fork state: the side set for the next marble and a pending move.
#[derive(Debug, Clone, Default)]
struct Fork {
    started: bool,
    side: Side,
    passes: i64,
    /// Tick at which the part moves to `side` (-1: none pending).
    move_at: i64,
}

impl Fork {
    fn save(&self) -> MechConfig {
        MechConfig::from([
            ("started".to_owned(), i64::from(self.started)),
            ("side".to_owned(), self.side),
            ("passes".to_owned(), self.passes),
            ("move_at".to_owned(), self.move_at),
        ])
    }

    fn restore(&mut self, state: &MechConfig) {
        self.started = get(state, "started") != 0;
        self.side = get(state, "side");
        self.passes = get(state, "passes");
        self.move_at = state.get("move_at").copied().unwrap_or(-1);
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
        let left = ctx.sensor(0).map_or(0, |s| s.exited.len());
        for _ in 0..left {
            ctx.emit("pass", self.side);
            self.passes += 1;
            self.side = next(ctx, self.side);
            self.move_at = t + FORK_SETTLE_TICKS;
        }
        if self.move_at >= 0 && t >= self.move_at {
            self.move_at = -1;
            ctx.move_part(0, angle(self.side), FORK_MOVE_TICKS);
            ctx.emit("set", self.side);
        }
    }
}

/// Random splitter: each marble's side is the next [`crate::mechanism::MechRng::below`]`(2)` of
/// the instance's `choice` stream, drawn when the previous marble has passed.
#[derive(Debug, Clone, Default)]
pub struct Splitter(Fork);

impl Splitter {
    pub fn factory(_: &MechConfig) -> Box<dyn Mechanism> {
        Box::new(Self(Fork {
            move_at: -1,
            ..Fork::default()
        }))
    }
}

/// The splitter's RNG stream name.
pub const SPLITTER_STREAM: &str = "choice";

impl Mechanism for Splitter {
    fn kind(&self) -> &'static str {
        "splitter"
    }

    fn step(&mut self, ctx: &mut MechContext<'_>) {
        self.0.step(
            ctx,
            &mut |ctx, _| i64::from(ctx.rng(SPLITTER_STREAM).below(2)),
            fork_angle,
        );
    }

    fn save(&self) -> MechConfig {
        self.0.save()
    }

    fn restore(&mut self, state: &MechConfig) {
        self.0.restore(state);
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
/// in neither sensor for [`HEADING_FORGET_TICKS`].
#[derive(Debug, Clone, Default)]
struct Heading {
    /// marble → (first sensor, last tick seen in either sensor)
    seen: BTreeMap<usize, (usize, i64)>,
}

/// A marble that has been out of both sensors this long is forgotten (it left the piece).
pub const HEADING_FORGET_TICKS: i64 = 240;

impl Heading {
    /// Update from both sensors and return the marbles newly seen at each sensor.
    fn update(&mut self, ctx: &MechContext<'_>) -> [Vec<usize>; 2] {
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
        self.seen.retain(|_, e| t - e.1 < HEADING_FORGET_TICKS);
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

/// One-way gate flap open angle and timing.
pub const GATE_OPEN_MDEG: i32 = 80_000;
pub const GATE_OPEN_TICKS: u32 = 4;
pub const GATE_CLOSE_TICKS: u32 = 24;
/// The flap closes once no forward marble has been in either sensor for this long.
pub const GATE_HOLD_TICKS: i64 = 12;

/// One-way gate: a marble that reaches `approach` (−X, the in port) first opens the flap; a
/// marble that reaches `exit` first meets the shut flap and goes back.
#[derive(Debug, Clone, Default)]
pub struct OneWayGate {
    heading: Heading,
    open: bool,
    last_forward: i64,
    passes: i64,
    blocked: i64,
}

impl OneWayGate {
    pub fn factory(_: &MechConfig) -> Box<dyn Mechanism> {
        Box::new(Self::default())
    }
}

/// Shared open/close of the one-way pieces: open while any marble from `trigger` is in a sensor,
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
    let fresh = heading.update(ctx);
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
        one_way_step(
            ctx,
            &mut self.heading,
            &mut self.open,
            &mut self.last_forward,
            &mut self.passes,
            &mut self.blocked,
            0,
            GATE_OPEN_MDEG,
            GATE_OPEN_TICKS,
            GATE_CLOSE_TICKS,
            GATE_HOLD_TICKS,
            ["forward", "reverse", "open", "close"],
        );
    }

    fn save(&self) -> MechConfig {
        let mut s = MechConfig::from([
            ("open".to_owned(), i64::from(self.open)),
            ("last_forward".to_owned(), self.last_forward),
            ("passes".to_owned(), self.passes),
            ("blocked".to_owned(), self.blocked),
        ]);
        self.heading.save(&mut s);
        s
    }

    fn restore(&mut self, state: &MechConfig) {
        self.open = get(state, "open") != 0;
        self.last_forward = get(state, "last_forward");
        self.passes = get(state, "passes");
        self.blocked = get(state, "blocked");
        self.heading.restore(state);
    }
}

/// One-way hole trapdoor open angle and timing (limit 60°).
pub const HOLE_OPEN_MDEG: i32 = 60_000;
pub const HOLE_OPEN_TICKS: u32 = 6;
pub const HOLE_CLOSE_TICKS: u32 = 24;
/// The trapdoor stays open this long after the last trap-side marble left the sensors, so it
/// never swings up into a marble still falling through the bore.
pub const HOLE_HOLD_TICKS: i64 = 72;

/// One-way hole: a marble that reaches `trap` (−X) first drops through the opened trapdoor to the
/// bottom port; a marble that reaches `over` (+X) first rolls across the shut door and leaves by −X.
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
}

#[cfg(test)]
#[path = "behaviour_test.rs"]
mod tests;
