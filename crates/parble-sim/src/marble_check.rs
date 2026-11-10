//! The marble check (PRD §6 P3 editor, owner request 2026-11-09): run a track's release headless
//! and report, per marble, whether it finished, fell off the track or stalled, and at which
//! placement.
//!
//! The sim is deterministic, so a check of a release is exact for that release: the same track,
//! seed, release mode, queue and marble type give the same report on every platform.
//!
//! # Outcomes
//!
//! - [`Outcome::Finished`]: a finish cup counted the marble.
//! - [`Outcome::FellOff`]: the marble's centre went below [`FELL_Y_M`], below
//!   [`crate::track_run::LOST_Y_UM`], or reached the tray floor (within
//!   [`crate::track_run::OFF_TRACK_CLEARANCE_UM`] of its resting height) over no placement's
//!   footprint. Final from that tick; `last` is the placement its centre entered last in time.
//! - [`Outcome::Stalled`]: neither when the check ends. `at` is the placement holding its centre
//!   then (or, when its centre is in no placement's cell, the last one it entered), and `moving`
//!   says whether it was still rolling ([`STALL_SPEED_UM_S`] or faster) rather than at rest.
//!
//! # When a check ends
//!
//! Once the hopper has released every marble it will release, the check ends at the first of:
//!
//! - every marble finished, fell off or held below [`STALL_SPEED_UM_S`] for [`STALL_HOLD_TICKS`]
//!   ticks in a row (nothing is left moving that could knock a resting marble on);
//! - the cap: [`CAP_AFTER_LAST_RELEASE_TICKS`] after the last release, and no earlier than
//!   [`CAP_MIN_TICKS`].
//!
//! [`CAP_MAX_TICKS`] ends it in any case (a sequential release whose marble never settles). A
//! marble still rolling at the cap is reported as stalled with `moving` set: it did not reach a
//! cup in the time the release gives it. The cap is not lengthened to wait for it: on the
//! all-pieces track the shooters still rolling at 30 s go on to rest short of a cup, or to fall
//! off, after 30 s ([`crate::allpieces`] module docs), and the 30 s outcome is the one the track's
//! tests pin. The cap rule gives the all-pieces release (10 marbles every 2 s, the last at 18 s)
//! exactly its 30 s run: 18 s + 12 s.
//!
//! # Time slicing
//!
//! [`MarbleCheck::step`] does one bounded unit of work: one [`TrackRunBuild::step`] unit while the
//! run is being built, then [`CHECK_TICKS_PER_UNIT`] ticks. The units take no wall-clock input; a
//! caller with a frame budget times them, as the editor does.

use std::collections::BTreeMap;

use parble_core::{CellCoord, MarbleSize, ReleaseMode, Track};
use parble_kit::KitCatalog;

use crate::behaviour;
use crate::hash::{POSITION_QUANTA_PER_M, VELOCITY_QUANTA_PER_M_PER_S, quantize};
use crate::marble::MarbleKind;
use crate::scene;
use crate::track_run::{
    KitBinding, LOST_Y_UM, OFF_TRACK_CLEARANCE_UM, SETTLE_SPEED_UM_S, TrackRun, TrackRunBuild,
    TrackRunError, kit_binding, marble_radius_m, release_schedule,
};

/// A marble whose centre is below this height, m, has fallen off the track (the all-pieces
/// track's fall line, 60 mm under the tray floor).
pub const FELL_Y_M: f32 = -0.06;
/// Below this speed, µm/s, a marble counts as still ([`SETTLE_SPEED_UM_S`], 5 mm/s), on the
/// quantized velocity like the trajectory hash, so the test is integer-exact.
pub const STALL_SPEED_UM_S: i64 = SETTLE_SPEED_UM_S;
/// Ticks in a row a marble must stay below [`STALL_SPEED_UM_S`] to count as stalled for the early
/// end (2 s): longer than the top of a bounce, a rocker's turn or a toggle's swing.
pub const STALL_HOLD_TICKS: u32 = 480;
/// The cap is never earlier than this tick (30 s).
pub const CAP_MIN_TICKS: u64 = 7_200;
/// The cap is this many ticks after the last release (12 s).
pub const CAP_AFTER_LAST_RELEASE_TICKS: u64 = 2_880;
/// A check never runs past this tick (5 min).
pub const CAP_MAX_TICKS: u64 = 72_000;
/// Ticks per [`MarbleCheck::step`] once the run is built: small enough that a unit of a
/// 10-marble track stays under a few ms in wasm, so a frame budget can hold several.
pub const CHECK_TICKS_PER_UNIT: u32 = 8;

/// What became of one marble.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// A finish cup counted it.
    Finished,
    /// It left the track; `last` is the placement (track index) its centre entered last.
    FellOff { last: Option<usize> },
    /// It was on the track and in no cup when the check ended; `at` is the placement holding it.
    Stalled { at: Option<usize>, moving: bool },
}

/// One marble's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarbleOutcome {
    /// Spawn order.
    pub marble: usize,
    pub size: MarbleSize,
    pub outcome: Outcome,
    /// The tick it finished or fell off, or the tick the check ended for a stall.
    pub tick: u64,
    /// The cell holding its centre at that tick, or the last placement cell it entered.
    pub cell: Option<CellCoord>,
}

/// A whole check: every released marble's outcome and the tick the check ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    pub kind: MarbleKind,
    pub marbles: Vec<MarbleOutcome>,
    pub ticks: u64,
}

impl CheckReport {
    /// The marbles that did not finish.
    pub fn problems(&self) -> impl Iterator<Item = &MarbleOutcome> {
        self.marbles
            .iter()
            .filter(|m| m.outcome != Outcome::Finished)
    }

    /// One line per marble that did not finish, `marble size outcome placement tick`, sorted by
    /// marble: the canonical text of the marble-check golden.
    pub fn lines(&self) -> Vec<String> {
        let at = |p: Option<usize>| p.map_or("-".to_owned(), |p| p.to_string());
        self.problems()
            .map(|m| {
                let what = match m.outcome {
                    Outcome::Finished => unreachable!("problems skips finished marbles"),
                    Outcome::FellOff { last } => format!("fell-off {}", at(last)),
                    Outcome::Stalled { at: p, moving } => {
                        format!("{} {}", if moving { "rolling" } else { "stalled" }, at(p))
                    }
                };
                format!(
                    "{} {} {} {what} {}",
                    self.kind.name(),
                    size_name(m.size),
                    m.marble,
                    m.tick
                )
            })
            .collect()
    }
}

/// The cell holding a centre: cells are centred on integer `x` and `z` and stand on integer `y`.
pub fn cell_of(position: [f32; 3]) -> CellCoord {
    let cell = scene::cell_m();
    CellCoord::new(
        libm::floorf(position[0] / cell + 0.5) as i32,
        libm::floorf(position[1] / cell) as i32,
        libm::floorf(position[2] / cell + 0.5) as i32,
    )
}

/// Per tick bookkeeping for [`Outcome`]s on any track: which placement holds each cell, and per
/// marble the placement entered last, its still streak and whether it fell off.
pub struct MarbleWatch {
    owners: BTreeMap<CellCoord, usize>,
    footprint: std::collections::BTreeSet<(i32, i32)>,
    sizes: Vec<MarbleSize>,
    last: Vec<Option<(usize, CellCoord)>>,
    still: Vec<u32>,
    fell: Vec<Option<u64>>,
    finished: Vec<Option<u64>>,
}

impl MarbleWatch {
    /// The watch of a run of `track` against `kit`. A cell two placements claim keeps the first.
    pub fn new(track: &Track, kit: &KitCatalog) -> Self {
        let mut owners = BTreeMap::new();
        let mut footprint = std::collections::BTreeSet::new();
        for (i, p) in track.placements.iter().enumerate() {
            let Some(KitBinding::Kit(piece)) = kit_binding(kit, &p.piece) else {
                continue;
            };
            let Some(k) = kit.piece(&piece) else {
                continue;
            };
            for c in p.world_cells(&k.def) {
                owners.entry(c).or_insert(i);
                footprint.insert((c.x, c.z));
            }
        }
        let sizes = if track.hopper.release == ReleaseMode::Sequential {
            track.hopper.queue.clone()
        } else {
            release_schedule(track).into_iter().map(|r| r.1).collect()
        };
        Self {
            owners,
            footprint,
            sizes,
            last: Vec::new(),
            still: Vec::new(),
            fell: Vec::new(),
            finished: Vec::new(),
        }
    }

    /// The placement (track index) holding `cell`.
    pub fn owner(&self, cell: CellCoord) -> Option<usize> {
        self.owners.get(&cell).copied()
    }

    /// Marbles the run releases in all.
    pub fn expected(&self) -> usize {
        self.sizes.len()
    }

    /// Record the run's state after a tick.
    pub fn observe(&mut self, run: &TrackRun) {
        let marbles = run.marbles();
        let n = marbles.len();
        self.last.resize(n, None);
        self.still.resize(n, 0);
        self.fell.resize(n, None);
        self.finished.resize(n, None);
        let tick = run.tick;
        for v in run.mechanisms() {
            if v.piece.as_str() == behaviour::FINISH {
                for (m, _) in behaviour::Finish::arrivals(&v.state) {
                    if let Some(f) = self.finished.get_mut(m)
                        && f.is_none()
                        && self.fell[m].is_none()
                    {
                        *f = Some(tick);
                    }
                }
            }
        }
        let cell_um = f64::from(parble_core::contract::CELL_UM) * 1.0e-6;
        let column = |v: f32| libm::floor(f64::from(v) / cell_um + 0.5) as i32;
        for (m, s) in marbles.iter().enumerate() {
            if self.fell[m].is_some() || self.finished[m].is_some() {
                continue;
            }
            let cell = cell_of(s.position);
            if let Some(i) = self.owner(cell) {
                self.last[m] = Some((i, cell));
            }
            let y_um = quantize(s.position[1], POSITION_QUANTA_PER_M);
            let size = self.sizes.get(m).copied().unwrap_or(MarbleSize::Standard);
            let rest = quantize(marble_radius_m(size), POSITION_QUANTA_PER_M);
            let on_floor = (y_um - rest).abs() <= OFF_TRACK_CLEARANCE_UM
                && !self
                    .footprint
                    .contains(&(column(s.position[0]), column(s.position[2])));
            if s.position[1] <= FELL_Y_M || y_um < LOST_Y_UM || on_floor {
                self.fell[m] = Some(tick);
                continue;
            }
            self.still[m] = if speed2_um(s.linvel) < STALL_SPEED_UM_S * STALL_SPEED_UM_S {
                self.still[m].saturating_add(1)
            } else {
                0
            };
        }
    }

    /// Every marble the run will release is out, and each one finished, fell off or has been
    /// still for [`STALL_HOLD_TICKS`].
    pub fn settled(&self, run: &TrackRun) -> bool {
        run.marbles().len() >= self.expected()
            && (0..run.marbles().len()).all(|m| {
                self.fell[m].is_some()
                    || self.finished[m].is_some()
                    || self.still[m] >= STALL_HOLD_TICKS
            })
    }

    /// Each released marble's outcome as of now.
    pub fn report(&self, run: &TrackRun, kind: MarbleKind) -> CheckReport {
        let marbles = run
            .marbles()
            .iter()
            .enumerate()
            .map(|(m, s)| {
                let size = self.sizes.get(m).copied().unwrap_or(MarbleSize::Standard);
                let last = self.last.get(m).copied().flatten();
                let (outcome, tick, cell) = if let Some(t) = self.finished[m] {
                    (Outcome::Finished, t, None)
                } else if let Some(t) = self.fell[m] {
                    (
                        Outcome::FellOff {
                            last: last.map(|l| l.0),
                        },
                        t,
                        last.map(|l| l.1),
                    )
                } else {
                    let cell = cell_of(s.position);
                    let here = self.owner(cell).map(|i| (i, cell)).or(last);
                    (
                        Outcome::Stalled {
                            at: here.map(|h| h.0),
                            moving: speed2_um(s.linvel) >= STALL_SPEED_UM_S * STALL_SPEED_UM_S,
                        },
                        run.tick,
                        here.map(|h| h.1),
                    )
                };
                MarbleOutcome {
                    marble: m,
                    size,
                    outcome,
                    tick,
                    cell,
                }
            })
            .collect();
        CheckReport {
            kind,
            marbles,
            ticks: run.tick,
        }
    }
}

/// Squared quantized speed, µm²/s².
fn speed2_um(v: [f32; 3]) -> i64 {
    v.iter()
        .map(|&c| quantize(c, VELOCITY_QUANTA_PER_M_PER_S))
        .fold(0i64, |a, c| a.saturating_add(c.saturating_mul(c)))
}

enum Phase {
    Building(Box<TrackRunBuild<'static>>),
    Running(Box<TrackRun>),
    Done(CheckReport),
    Failed,
}

/// One check of `track`'s release with every marble spawned as one type, built and run one
/// bounded unit at a time ([`MarbleCheck::step`]).
pub struct MarbleCheck {
    kind: MarbleKind,
    watch: MarbleWatch,
    phase: Phase,
    /// The last tick a marble count rose, and the count then.
    last_release: (u64, usize),
}

impl MarbleCheck {
    /// The check of [`TrackRun::with_marble`]`(track, kind)`, with no work done yet.
    pub fn new(track: &Track, kind: MarbleKind) -> Self {
        let kit = parble_kit::builtin();
        Self {
            kind,
            watch: MarbleWatch::new(track, kit),
            phase: Phase::Building(Box::new(TrackRunBuild::with_mechanisms(
                track,
                kit,
                &behaviour::registry(),
                Some(kind),
            ))),
            last_release: (0, 0),
        }
    }

    pub fn kind(&self) -> MarbleKind {
        self.kind
    }

    /// The tick the check ends at if nothing settles it first, once every marble is out.
    fn cap(&self) -> u64 {
        cap_of(self.last_release.0)
    }

    /// Do one unit of work and return whether the check is done. After an error the check is
    /// spent: the track cannot run.
    pub fn step(&mut self) -> Result<bool, TrackRunError> {
        match &mut self.phase {
            Phase::Building(build) => match build.step() {
                Ok(true) => {
                    let Phase::Building(build) = std::mem::replace(&mut self.phase, Phase::Failed)
                    else {
                        unreachable!("building")
                    };
                    let run = build.finish()?;
                    self.last_release = (0, run.marbles().len());
                    self.phase = Phase::Running(Box::new(run));
                }
                Ok(false) => {}
                Err(e) => {
                    self.phase = Phase::Failed;
                    return Err(e);
                }
            },
            Phase::Running(run) => {
                let mut end = false;
                for _ in 0..CHECK_TICKS_PER_UNIT {
                    run.tick(1);
                    self.watch.observe(run);
                    let n = run.marbles().len();
                    if n > self.last_release.1 {
                        // Releases happen before the step, so the marble came out a tick ago.
                        self.last_release = (run.tick - 1, n);
                    }
                    let all_out = n >= self.watch.expected();
                    let cap = cap_of(self.last_release.0);
                    if (all_out && (self.watch.settled(run) || run.tick >= cap))
                        || run.tick >= CAP_MAX_TICKS
                    {
                        end = true;
                        break;
                    }
                }
                if end {
                    self.phase = Phase::Done(self.watch.report(run, self.kind));
                }
            }
            Phase::Done(_) | Phase::Failed => {}
        }
        Ok(self.is_done())
    }

    pub fn is_done(&self) -> bool {
        matches!(self.phase, Phase::Done(_))
    }

    /// Rough share of the work done, 0 to 1: the build is the first 10 %, the run (by tick, to
    /// the cap) the rest.
    pub fn progress(&self) -> f32 {
        match &self.phase {
            Phase::Building(b) => 0.1 * b.progress(),
            Phase::Running(run) => 0.1 + 0.9 * (run.tick as f32 / self.cap() as f32).min(0.99),
            Phase::Done(_) => 1.0,
            Phase::Failed => 0.0,
        }
    }

    /// The tick the run is at (0 while building).
    pub fn tick(&self) -> u64 {
        match &self.phase {
            Phase::Running(run) => run.tick,
            Phase::Done(r) => r.ticks,
            _ => 0,
        }
    }

    pub fn report(&self) -> Option<&CheckReport> {
        match &self.phase {
            Phase::Done(r) => Some(r),
            _ => None,
        }
    }

    /// The whole check in one go.
    pub fn run(track: &Track, kind: MarbleKind) -> Result<CheckReport, TrackRunError> {
        let mut check = Self::new(track, kind);
        while !check.step()? {}
        Ok(check.report().cloned().expect("a done check has a report"))
    }
}

/// The marble types with distinct physics, each standing for the types that share its spec
/// ([`MarbleKind::spec`]): cat's eye (with swirl, onionskin, opal and glow), glazed and steel. A
/// type reaches the sim only through its spec, so the types of one spec run the same.
pub fn check_kinds() -> Vec<(MarbleKind, Vec<MarbleKind>)> {
    let mut out: Vec<(MarbleKind, Vec<MarbleKind>)> = Vec::new();
    for k in MarbleKind::ALL {
        match out.iter_mut().find(|(s, _)| s.spec() == k.spec()) {
            Some((_, same)) => same.push(k),
            None => out.push((k, Vec::new())),
        }
    }
    out
}

/// `track` with every queued marble at `size`, the count kept.
pub fn track_at_size(track: &Track, size: MarbleSize) -> Track {
    let mut t = track.clone();
    t.hopper.queue.fill(size);
    t
}

/// The marble check of the all-pieces track's pinned release ([`crate::allpieces::track_sized`])
/// for every spec of [`check_kinds`] at every size: the lines of
/// `tests/golden/allpieces_marble_check.txt`, one `# kind size ticks` header per check followed by
/// its [`CheckReport::lines`].
pub fn allpieces_marble_check() -> String {
    let mut out = String::new();
    for (kind, _) in check_kinds() {
        for size in [MarbleSize::Mini, MarbleSize::Standard, MarbleSize::Shooter] {
            let r = MarbleCheck::run(&crate::allpieces::track_sized(size), kind)
                .expect("the all-pieces track runs");
            out.push_str(&format!(
                "# {} {} {}\n",
                kind.name(),
                size_name(size),
                r.ticks
            ));
            for l in r.lines() {
                out.push_str(&l);
                out.push('\n');
            }
        }
    }
    out
}

/// The cap for a run whose last release was at `last_release`.
fn cap_of(last_release: u64) -> u64 {
    (last_release + CAP_AFTER_LAST_RELEASE_TICKS).max(CAP_MIN_TICKS)
}

/// A size's lowercase name, as in `.parble` files.
pub fn size_name(size: MarbleSize) -> &'static str {
    match size {
        MarbleSize::Mini => "mini",
        MarbleSize::Standard => "standard",
        MarbleSize::Shooter => "shooter",
    }
}
