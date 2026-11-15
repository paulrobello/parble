//! Generate-and-validate (P6-T2): a resumable job that generates a track with
//! [`parble_procgen::generate`], runs it headless and accepts it only when every hopper marble
//! reaches the finish inside a tick limit. A failing candidate is retried with the next sub-seed,
//! up to [`MAX_ATTEMPTS`].
//!
//! The job does deterministic work units ([`GenJob::step`]): one candidate generation, one
//! collider build step or [`RUN_SLICE_TICKS`] sim ticks. It never reads a clock, so the accepted
//! track is the same however the units are sliced; the wasm wrapper (`parble-sim-wasm`) is the
//! only place that spends a time budget on them.
//!
//! The accepted track's own `seed` is the sub-seed that produced it, so
//! `generate(defs, track.seed, params)` regenerates it exactly, and it is also the root the
//! track's mechanisms draw their RNG streams from.
//!
//! [`SegmentedJob`] does the same for a track generated in segments
//! (`parble_procgen::segment_plan`), retrying only from the segment a marble failed on.

use std::collections::BTreeMap;

use parble_core::file::MAX_SAFE_INTEGER;
use parble_core::{CellCoord, ParbleFile, PieceDef, Track};
use parble_procgen::{
    GenError, GenParams, Segment, SegmentSpec, assemble, chain_segment, generate, generated_file,
    segment_plan,
};

use crate::track_run::{MarbleMotion, TrackRun, TrackRunBuild, TrackRunError};

/// Candidates tried before the job gives up.
pub const MAX_ATTEMPTS: u32 = 32;

/// Sim ticks per work unit while a candidate runs (a quarter second at 240 Hz).
pub const RUN_SLICE_TICKS: u32 = 60;

/// Ticks every marble gets before the first track piece is counted. With
/// [`LIMIT_TICKS_PER_PIECE`] about twice the slowest finish measured (2026-11-15,
/// `finish_time_census`, 3 marbles): length 40, slowest 3,688 ticks a marble over about 32 track
/// pieces (limit 6,240); length 80 at height 12, slowest 6,064 over about 62 (limit 9,840);
/// length 100 at height 10, slowest 6,544 over about 78 (limit 11,760). Supports are not counted:
/// they hold the track up and a marble does not roll along them.
pub const LIMIT_BASE_TICKS: u32 = 240 * 10;

/// Ticks per marble per track piece (non-structural placement) on top of [`LIMIT_BASE_TICKS`].
pub const LIMIT_TICKS_PER_PIECE: u32 = 120;

/// Ticks in a row with every marble released, none of them moving and not all finished before a
/// candidate is rejected as stalled. Nothing is left to knock a resting marble on but a
/// mechanism, and two seconds covers a gate's swing.
pub const STALL_HOLD_TICKS: u32 = 240 * 2;

/// Exhausted generations in a row before the job gives up: the params are feasible on paper but
/// the search keeps running out of nodes, and each try costs most of a second.
pub const MAX_EXHAUSTED_IN_A_ROW: u32 = 4;

/// The sub-seed of attempt `attempt`: the base seed itself first, then a splitmix64 walk from it,
/// kept within [`MAX_SAFE_INTEGER`] so the seed survives a JavaScript `number` and the file's
/// seed rule.
pub fn sub_seed(base: u64, attempt: u32) -> u64 {
    if attempt == 0 {
        return base;
    }
    let mut z = base.wrapping_add(u64::from(attempt).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (z ^ (z >> 31)) & MAX_SAFE_INTEGER as u64
}

/// The tick limit a track's marbles must all finish inside: the hopper releases them one after
/// another, so each gets [`LIMIT_BASE_TICKS`] plus [`LIMIT_TICKS_PER_PIECE`] per track piece.
/// A piece missing from the built-in kit counts as a track piece.
pub fn limit_ticks(track: &Track) -> u32 {
    let kit = parble_kit::builtin();
    let marbles = track.hopper.queue.len().max(1) as u32;
    let pieces = track
        .placements
        .iter()
        .filter(|pl| !kit.iter().any(|k| k.def.id == pl.piece && k.def.structural))
        .count() as u32;
    marbles.saturating_mul(
        LIMIT_BASE_TICKS.saturating_add(pieces.saturating_mul(LIMIT_TICKS_PER_PIECE)),
    )
}

/// Why one candidate was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    /// The generator produced no track for the sub-seed.
    Generate(GenError),
    /// The track did not build in the sim.
    Build(String),
    /// A marble fell below the world: `finished` of `marbles` had finished at `tick`.
    Lost {
        tick: u32,
        finished: usize,
        marbles: usize,
    },
    /// A marble left the track onto the open floor at `tick`.
    OffTrack {
        tick: u32,
        finished: usize,
        marbles: usize,
    },
    /// Every marble was released and none had moved for [`STALL_HOLD_TICKS`] at `tick`, with
    /// `finished` of `marbles` finished.
    Stalled {
        tick: u32,
        finished: usize,
        marbles: usize,
    },
    /// The limit passed with `finished` of `marbles` finished.
    TimedOut {
        limit: u32,
        finished: usize,
        marbles: usize,
    },
}

impl core::fmt::Display for Rejection {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Generate(e) => write!(f, "generation failed: {e}"),
            Self::Build(e) => write!(f, "the track did not build: {e}"),
            Self::Lost {
                tick,
                finished,
                marbles,
            } => write!(
                f,
                "a marble was lost at tick {tick} ({finished} of {marbles} finished)"
            ),
            Self::OffTrack {
                tick,
                finished,
                marbles,
            } => write!(
                f,
                "a marble left the track at tick {tick} ({finished} of {marbles} finished)"
            ),
            Self::Stalled {
                tick,
                finished,
                marbles,
            } => write!(
                f,
                "the marbles stalled by tick {tick} ({finished} of {marbles} finished)"
            ),
            Self::TimedOut {
                limit,
                finished,
                marbles,
            } => write!(
                f,
                "{finished} of {marbles} marbles finished inside {limit} ticks"
            ),
        }
    }
}

/// One rejected candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub attempt: u32,
    pub seed: u64,
    pub rejection: Rejection,
}

/// Where the job is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    Accepted,
    /// [`MAX_ATTEMPTS`] candidates were rejected, or the params are out of range.
    Failed,
}

/// Why a job could not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobError {
    /// The base seed is above [`MAX_SAFE_INTEGER`].
    SeedOutOfRange(u64),
}

/// A candidate's headless run, advanced a slice at a time.
struct Runner {
    run: Box<TrackRun>,
    tick: u32,
    limit: u32,
    /// Ticks in a row with every marble released and none moving.
    quiet: u32,
}

impl Runner {
    fn new(run: TrackRun, limit: u32) -> Self {
        Self {
            run: Box::new(run),
            tick: 0,
            limit,
            quiet: 0,
        }
    }

    fn progress(&self) -> f32 {
        self.tick as f32 / self.limit.max(1) as f32
    }

    fn finished(&self) -> usize {
        self.run
            .motion()
            .iter()
            .filter(|s| matches!(s, MarbleMotion::Finished))
            .count()
    }

    /// Run up to [`RUN_SLICE_TICKS`] ticks, calling `each` after every tick, and return the
    /// verdict once there is one: every one of `marbles` finished, or why the candidate fails.
    fn slice(
        &mut self,
        marbles: usize,
        mut each: impl FnMut(&TrackRun),
    ) -> Option<Result<(), Rejection>> {
        let n = RUN_SLICE_TICKS.min(self.limit - self.tick);
        for _ in 0..n {
            self.run.tick(1);
            self.tick += 1;
            each(&self.run);
            let tick = self.tick;
            let m = self.run.motion();
            let finished = self.finished();
            if m.iter().any(|s| matches!(s, MarbleMotion::Lost)) {
                return Some(Err(Rejection::Lost {
                    tick,
                    finished,
                    marbles,
                }));
            }
            if finished == marbles {
                return Some(Ok(()));
            }
            if m.iter().any(|s| matches!(s, MarbleMotion::OffTrack)) {
                return Some(Err(Rejection::OffTrack {
                    tick,
                    finished,
                    marbles,
                }));
            }
            if m.len() == marbles && !m.iter().any(|s| s.is_moving()) {
                self.quiet += 1;
                if self.quiet >= STALL_HOLD_TICKS {
                    return Some(Err(Rejection::Stalled {
                        tick,
                        finished,
                        marbles,
                    }));
                }
            } else {
                self.quiet = 0;
            }
        }
        (self.tick >= self.limit).then(|| {
            Err(Rejection::TimedOut {
                limit: self.limit,
                finished: self.finished(),
                marbles,
            })
        })
    }
}

enum Stage {
    Generate,
    Build(Box<TrackRunBuild<'static>>),
    Run(Runner),
    Accepted,
    Failed,
}

/// A resumable generate-and-validate job.
pub struct GenJob {
    defs: Vec<PieceDef>,
    base: u64,
    params: GenParams,
    attempt: u32,
    stage: Stage,
    candidate: Option<Track>,
    rejected: Vec<Attempt>,
    /// Exhausted generations in a row.
    exhausted: u32,
}

impl GenJob {
    /// A job over the built-in kit.
    pub fn new(base: u64, params: GenParams) -> Result<Self, JobError> {
        let defs = parble_kit::builtin()
            .iter()
            .map(|p| p.def.clone())
            .collect();
        Self::with_defs(defs, base, params)
    }

    /// A job over `defs`. The sim always builds against the built-in kit, so `defs` must be a
    /// subset of it.
    pub fn with_defs(defs: Vec<PieceDef>, base: u64, params: GenParams) -> Result<Self, JobError> {
        if base > MAX_SAFE_INTEGER as u64 {
            return Err(JobError::SeedOutOfRange(base));
        }
        Ok(Self {
            defs,
            base,
            params,
            attempt: 0,
            stage: Stage::Generate,
            candidate: None,
            rejected: Vec::new(),
            exhausted: 0,
        })
    }

    pub fn status(&self) -> JobStatus {
        match self.stage {
            Stage::Accepted => JobStatus::Accepted,
            Stage::Failed => JobStatus::Failed,
            _ => JobStatus::Running,
        }
    }

    /// The candidate being tried, 0-based.
    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// The accepted track, once [`JobStatus::Accepted`].
    pub fn accepted(&self) -> Option<&Track> {
        matches!(self.stage, Stage::Accepted)
            .then_some(self.candidate.as_ref())
            .flatten()
    }

    /// Every rejected candidate so far, in attempt order.
    pub fn rejected(&self) -> &[Attempt] {
        &self.rejected
    }

    /// Rough share of the current candidate's work done, 0 to 1: generation, then the build
    /// (the first quarter after it), then the run up to its limit.
    pub fn candidate_progress(&self) -> f32 {
        match &self.stage {
            Stage::Generate => 0.0,
            Stage::Build(b) => 0.25 * b.progress(),
            Stage::Run(r) => 0.25 + 0.75 * r.progress(),
            Stage::Accepted | Stage::Failed => 1.0,
        }
    }

    /// Run until the job is done, as one slice.
    pub fn run_to_end(&mut self) -> JobStatus {
        while self.step() == JobStatus::Running {}
        self.status()
    }

    /// Do one work unit and return the status after it.
    pub fn step(&mut self) -> JobStatus {
        let stage = core::mem::replace(&mut self.stage, Stage::Failed);
        self.stage = match stage {
            Stage::Generate => {
                let seed = sub_seed(self.base, self.attempt);
                match generate(&self.defs, seed, &self.params) {
                    Ok(track) => {
                        self.exhausted = 0;
                        let build = TrackRunBuild::new(&track);
                        self.candidate = Some(track);
                        Stage::Build(Box::new(build))
                    }
                    // These depend on the params and the catalog, never the seed: every sub-seed
                    // would fail the same way. Only an exhausted search is worth a retry.
                    Err(
                        e @ (GenError::BadParams(_)
                        | GenError::MissingPiece(_)
                        | GenError::NoPieces
                        | GenError::NoRoute
                        | GenError::TooLong { .. }),
                    ) => {
                        self.rejected.push(Attempt {
                            attempt: self.attempt,
                            seed,
                            rejection: Rejection::Generate(e),
                        });
                        Stage::Failed
                    }
                    Err(e @ GenError::Exhausted { .. }) => {
                        self.exhausted += 1;
                        let next = self.reject(Rejection::Generate(e));
                        if self.exhausted >= MAX_EXHAUSTED_IN_A_ROW {
                            Stage::Failed
                        } else {
                            next
                        }
                    }
                    Err(e) => self.reject(Rejection::Generate(e)),
                }
            }
            Stage::Build(mut build) => match build.step() {
                Ok(false) => Stage::Build(build),
                Ok(true) => match build.finish() {
                    Ok(run) => {
                        let limit = self.candidate.as_ref().map_or(0, limit_ticks);
                        Stage::Run(Runner::new(run, limit))
                    }
                    Err(e) => self.reject(build_rejection(e)),
                },
                Err(e) => self.reject(build_rejection(e)),
            },
            Stage::Run(mut runner) => match runner.slice(self.params.marbles as usize, |_| {}) {
                Some(Ok(())) => Stage::Accepted,
                Some(Err(r)) => self.reject(r),
                None => Stage::Run(runner),
            },
            done @ (Stage::Accepted | Stage::Failed) => done,
        };
        self.status()
    }

    /// Record the current candidate as rejected and move to the next sub-seed, or fail.
    fn reject(&mut self, rejection: Rejection) -> Stage {
        self.rejected.push(Attempt {
            attempt: self.attempt,
            seed: sub_seed(self.base, self.attempt),
            rejection,
        });
        self.candidate = None;
        self.attempt += 1;
        if self.attempt < MAX_ATTEMPTS {
            Stage::Generate
        } else {
            Stage::Failed
        }
    }
}

fn build_rejection(e: TrackRunError) -> Rejection {
    Rejection::Build(format!("{e:?}"))
}

/// The golden jobs: default params at seed 32 (its first candidate is rejected) and a small
/// track at seed 2. `tests/golden/procgen_job.txt` holds their [`result_line`]s; it changes with
/// [`parble_procgen::GENERATOR_VERSION`] and with the sim.
pub fn golden_cases() -> [(u64, GenParams); 2] {
    let small = GenParams {
        height: 3,
        length: 14,
        footprint: 5,
        marbles: 2,
        ..GenParams::default()
    };
    [(32, GenParams::default()), (2, small)]
}

/// One line for a finished job: base seed, accepted attempt and seed, rejections and the FNV-1a
/// hash of the accepted `.parble` file (`-` when it failed).
pub fn result_line(base: u64, job: &GenJob) -> String {
    let file = job.accepted().map_or_else(
        || "-".to_owned(),
        |t| {
            let mut h = crate::hash::Fnv1a64::default();
            h.write(
                parble_procgen::generated_file(t.clone(), "generated", &[])
                    .to_json()
                    .as_bytes(),
            );
            crate::hash::format_hash(h.finish())
        },
    );
    let seed = job.accepted().map_or(0, |t| t.seed);
    format!(
        "base {base} attempt {} seed {seed} rejected {} file {file}",
        job.attempt(),
        job.rejected().len()
    )
}

/// The [`golden_cases`] run to the end, one [`result_line`] each, then the segmented golden job
/// ([`segmented_golden_case`]).
pub fn golden_lines() -> String {
    let mut lines: String = golden_cases()
        .into_iter()
        .map(|(base, params)| {
            let mut job = GenJob::new(base, params).expect("golden seeds are in range");
            job.run_to_end();
            result_line(base, &job) + "\n"
        })
        .collect();
    let (base, params) = segmented_golden_case();
    let mut job = SegmentedJob::new(base, params).expect("golden seeds are in range");
    job.run_to_end();
    lines += &(segmented_result_line(base, &job) + "\n");
    lines
}

/// Candidates one segment gets before the job goes back to the segment above it.
pub const SEGMENT_ATTEMPTS: u32 = 32;

/// Rejected segment candidates, over all segments, before the job gives up.
pub const MAX_SEGMENT_REJECTIONS: usize = 96;

/// The seed of segment `segment`'s candidate `attempt`: [`sub_seed`] of a per-segment base, so
/// it stays within [`MAX_SAFE_INTEGER`]. Segment 0 draws the same seeds as a whole-track job.
pub fn segment_seed(base: u64, segment: u32, attempt: u32) -> u64 {
    sub_seed(sub_seed(base, segment << 16), attempt)
}

/// One rejected segment candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentAttempt {
    /// Index of the segment in the track's plan.
    pub segment: u32,
    pub attempt: u32,
    pub seed: u64,
    pub rejection: Rejection,
}

enum SegStage {
    Generate,
    Build(Box<TrackRunBuild<'static>>),
    Run {
        runner: Runner,
        /// Per released marble, the segment it has rolled into so far.
        reached: Vec<u32>,
    },
    Accepted,
    Failed,
}

/// A resumable generate-and-validate job for a track generated in segments.
///
/// The track is the chain of `parble_procgen::segment_plan(params)`, one seed a segment
/// ([`segment_seed`]). The job generates every segment, then runs the whole track headless from
/// the hopper under [`GenJob`]'s acceptance rule. When a marble fails, the segment it failed on
/// is the first one it had not rolled out of: that segment gets its next seed, the segments
/// below it are generated again from its new end, and the segments above it stay as they are.
/// The job accepts the track whose whole run passes, so the accepted file has been run from
/// scratch exactly as a player runs it.
///
/// Why the whole track is run again instead of the one segment: a segment run alone, with its
/// marbles started at the speed measured at its entry, is not the run the finished track gets
/// (the supports under it change with the pieces below, chunk colliders merge across the seam,
/// a marble arrives spinning), so its pass would not hold for the file. Running on from a saved
/// sim state would be exact, but [`crate::Sim`] cannot be copied yet. A rerun costs the ticks up
/// to the failure and nothing else: generation is under a second for 500 pieces.
///
/// Like [`GenJob`] it never reads a clock: the result is the same however [`SegmentedJob::step`]
/// calls are sliced. The accepted track's `seed` is the base seed, the root of its mechanisms'
/// random streams, and [`SegmentedJob::segment_seeds`] regenerate its placements with
/// `parble_procgen::generate_chain(defs, base, params, seeds)`.
pub struct SegmentedJob {
    defs: Vec<PieceDef>,
    base: u64,
    params: GenParams,
    plan: Vec<SegmentSpec>,
    /// The candidate each segment is on, 0-based.
    attempts: Vec<u32>,
    /// The segments generated so far, a chain from the hopper.
    segments: Vec<Segment>,
    /// The segment each track cell belongs to.
    owners: BTreeMap<CellCoord, u32>,
    stage: SegStage,
    candidate: Option<Track>,
    rejected: Vec<SegmentAttempt>,
    /// Failed generations in a row of the segment being generated.
    exhausted: u32,
    /// Whole-track runs started.
    runs: u32,
}

impl SegmentedJob {
    /// A job over the built-in kit.
    pub fn new(base: u64, params: GenParams) -> Result<Self, JobError> {
        let defs = parble_kit::builtin()
            .iter()
            .map(|p| p.def.clone())
            .collect();
        Self::with_defs(defs, base, params)
    }

    /// A job over `defs`, a subset of the built-in kit.
    pub fn with_defs(defs: Vec<PieceDef>, base: u64, params: GenParams) -> Result<Self, JobError> {
        if base > MAX_SAFE_INTEGER as u64 {
            return Err(JobError::SeedOutOfRange(base));
        }
        let plan = segment_plan(&params);
        Ok(Self {
            defs,
            base,
            params,
            attempts: vec![0; plan.len()],
            plan,
            segments: Vec::new(),
            owners: BTreeMap::new(),
            stage: SegStage::Generate,
            candidate: None,
            rejected: Vec::new(),
            exhausted: 0,
            runs: 0,
        })
    }

    pub fn status(&self) -> JobStatus {
        match self.stage {
            SegStage::Accepted => JobStatus::Accepted,
            SegStage::Failed => JobStatus::Failed,
            _ => JobStatus::Running,
        }
    }

    /// Segments in the track's plan.
    pub fn segment_count(&self) -> usize {
        self.plan.len()
    }

    /// Whole-track runs started so far.
    pub fn runs(&self) -> u32 {
        self.runs
    }

    /// The accepted track, once [`JobStatus::Accepted`].
    pub fn accepted(&self) -> Option<&Track> {
        matches!(self.stage, SegStage::Accepted)
            .then_some(self.candidate.as_ref())
            .flatten()
    }

    /// The seed of every segment of the current chain, in order: once accepted, the seeds that
    /// regenerate the track.
    pub fn segment_seeds(&self) -> Vec<u64> {
        (0..self.segments.len() as u32)
            .map(|i| segment_seed(self.base, i, self.attempts[i as usize]))
            .collect()
    }

    /// The accepted track as a `.parble` file whose metadata records the generator version and
    /// the segment seeds.
    pub fn file(&self, name: &str) -> Option<ParbleFile> {
        self.accepted()
            .map(|t| generated_file(t.clone(), name, &self.segment_seeds()))
    }

    /// Every rejected segment candidate so far, in order.
    pub fn rejected(&self) -> &[SegmentAttempt] {
        &self.rejected
    }

    /// Rough share of the current candidate track's work done, 0 to 1: the segments generated
    /// (the first tenth), the build (to a quarter), then the run up to its limit.
    pub fn candidate_progress(&self) -> f32 {
        match &self.stage {
            SegStage::Generate => 0.1 * self.segments.len() as f32 / self.plan.len().max(1) as f32,
            SegStage::Build(b) => 0.1 + 0.15 * b.progress(),
            SegStage::Run { runner, .. } => 0.25 + 0.75 * runner.progress(),
            SegStage::Accepted | SegStage::Failed => 1.0,
        }
    }

    /// Run until the job is done, as one slice.
    pub fn run_to_end(&mut self) -> JobStatus {
        while self.step() == JobStatus::Running {}
        self.status()
    }

    /// Do one work unit (one segment generated, one build step or [`RUN_SLICE_TICKS`] ticks) and
    /// return the status after it.
    pub fn step(&mut self) -> JobStatus {
        let stage = core::mem::replace(&mut self.stage, SegStage::Failed);
        self.stage = match stage {
            SegStage::Generate => self.generate_step(),
            SegStage::Build(mut build) => match build.step() {
                Ok(false) => SegStage::Build(build),
                Ok(true) => match build.finish() {
                    Ok(run) => {
                        let limit = self.candidate.as_ref().map_or(0, limit_ticks);
                        self.runs += 1;
                        SegStage::Run {
                            runner: Runner::new(run, limit),
                            reached: Vec::new(),
                        }
                    }
                    Err(e) => self.reject(self.last_segment(), build_rejection(e), false),
                },
                Err(e) => self.reject(self.last_segment(), build_rejection(e), false),
            },
            SegStage::Run {
                mut runner,
                mut reached,
            } => {
                let owners = &self.owners;
                let verdict = runner.slice(self.params.marbles as usize, |run| {
                    advance(owners, run, &mut reached);
                });
                match verdict {
                    Some(Ok(())) => SegStage::Accepted,
                    Some(Err(r)) => {
                        let failed = failed_segment(&runner.run, &reached);
                        self.reject(failed, r, false)
                    }
                    None => SegStage::Run { runner, reached },
                }
            }
            done @ (SegStage::Accepted | SegStage::Failed) => done,
        };
        self.status()
    }

    fn last_segment(&self) -> u32 {
        self.plan.len().saturating_sub(1) as u32
    }

    /// Generate the next segment of the chain, or assemble the track once the chain is whole.
    fn generate_step(&mut self) -> SegStage {
        let i = self.segments.len();
        if i == self.plan.len() {
            return match assemble(&self.defs, self.base, &self.params, &self.segments) {
                Ok(track) => {
                    let build = TrackRunBuild::new(&track);
                    self.candidate = Some(track);
                    SegStage::Build(Box::new(build))
                }
                Err(e) => self.reject(self.last_segment(), Rejection::Generate(e), false),
            };
        }
        let seed = segment_seed(self.base, i as u32, self.attempts[i]);
        match chain_segment(&self.defs, seed, &self.params, &self.plan, &self.segments) {
            Ok(segment) => {
                self.exhausted = 0;
                let before = self.segments.last().map(|s| &s.space.occupied);
                for c in &segment.space.occupied {
                    if !before.is_some_and(|b| b.contains(c)) {
                        self.owners.insert(*c, i as u32);
                    }
                }
                self.segments.push(segment);
                SegStage::Generate
            }
            // These depend on the params and the catalog, never the seed or the segments above.
            Err(
                e @ (GenError::BadParams(_)
                | GenError::MissingPiece(_)
                | GenError::NoPieces
                | GenError::NoRoute
                | GenError::TooLong { .. }
                | GenError::GoalAbove { .. }),
            ) => {
                self.rejected.push(SegmentAttempt {
                    segment: i as u32,
                    attempt: self.attempts[i],
                    seed,
                    rejection: Rejection::Generate(e),
                });
                SegStage::Failed
            }
            // The start the segment above left leads nowhere, or the search keeps running out of
            // nodes from it: no seed of this segment helps, the one above gets its next seed.
            Err(e) => {
                self.exhausted += 1;
                let back = matches!(e, GenError::NoSegmentRoute)
                    || self.exhausted >= MAX_EXHAUSTED_IN_A_ROW;
                self.reject(i as u32, Rejection::Generate(e), back)
            }
        }
    }

    /// Record segment `segment`'s candidate as rejected and move it to its next seed, dropping
    /// it and every segment below it from the chain. With `back`, or once the segment is out of
    /// candidates, the segment above it moves on instead.
    fn reject(&mut self, segment: u32, rejection: Rejection, back: bool) -> SegStage {
        let mut j = (segment as usize).min(self.plan.len().saturating_sub(1));
        self.rejected.push(SegmentAttempt {
            segment: j as u32,
            attempt: self.attempts[j],
            seed: segment_seed(self.base, j as u32, self.attempts[j]),
            rejection,
        });
        self.candidate = None;
        if self.rejected.len() >= MAX_SEGMENT_REJECTIONS {
            return SegStage::Failed;
        }
        let mut back = back;
        loop {
            if back {
                if j == 0 {
                    return SegStage::Failed;
                }
                self.attempts[j] = 0;
                j -= 1;
            }
            self.attempts[j] += 1;
            for a in &mut self.attempts[j + 1..] {
                *a = 0;
            }
            if self.attempts[j] < SEGMENT_ATTEMPTS {
                break;
            }
            back = true;
        }
        self.exhausted = 0;
        self.segments.truncate(j);
        self.owners.retain(|_, s| (*s as usize) < j);
        SegStage::Generate
    }
}

/// Move each released marble's segment on: the segment of the cell its centre is in, counted
/// only when it is the marble's own or the next one, so a marble that flies off one segment and
/// lands on a lower one stays charged to the segment it left.
fn advance(owners: &BTreeMap<CellCoord, u32>, run: &TrackRun, reached: &mut Vec<u32>) {
    let cell = f64::from(parble_core::contract::CELL_UM) * 1.0e-6;
    let marbles = run.marbles();
    reached.resize(marbles.len(), 0);
    for (r, m) in reached.iter_mut().zip(&marbles) {
        let column = |v: f32| libm::floor(f64::from(v) / cell + 0.5) as i32;
        let layer = libm::floor(f64::from(m.position[1]) / cell) as i32;
        let at = CellCoord::new(column(m.position[0]), layer, column(m.position[2]));
        if owners.get(&at) == Some(&(*r + 1)) {
            *r += 1;
        }
    }
}

/// The segment a failed run is charged to: the one nearest the hopper that a marble which did
/// not finish was still on.
fn failed_segment(run: &TrackRun, reached: &[u32]) -> u32 {
    run.motion()
        .iter()
        .zip(reached)
        .filter(|(m, _)| !matches!(m, MarbleMotion::Finished))
        .map(|(_, &r)| r)
        .min()
        .unwrap_or(0)
}

/// The segmented golden job: 80 pieces from layer 12 at seed 3, two segments.
pub fn segmented_golden_case() -> (u64, GenParams) {
    let params = GenParams {
        height: 12,
        length: 80,
        footprint: 14,
        ..GenParams::default()
    };
    (3, params)
}

/// One line for a finished segmented job: base seed, the accepted segment seeds, rejections,
/// whole-track runs and the FNV-1a hash of the accepted `.parble` file (`-` when it failed).
pub fn segmented_result_line(base: u64, job: &SegmentedJob) -> String {
    let file = job.file("generated").map_or_else(
        || "-".to_owned(),
        |f| {
            let mut h = crate::hash::Fnv1a64::default();
            h.write(f.to_json().as_bytes());
            crate::hash::format_hash(h.finish())
        },
    );
    let seeds: Vec<String> = job.segment_seeds().iter().map(u64::to_string).collect();
    format!(
        "segmented base {base} seeds {} rejected {} runs {} file {file}",
        seeds.join(","),
        job.rejected().len(),
        job.runs()
    )
}
