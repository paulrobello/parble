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

use parble_core::file::MAX_SAFE_INTEGER;
use parble_core::{PieceDef, Track};
use parble_procgen::{GenError, GenParams, generate};

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

enum Stage {
    Generate,
    Build(Box<TrackRunBuild<'static>>),
    Run {
        run: Box<TrackRun>,
        tick: u32,
        limit: u32,
        /// Ticks in a row with every marble released and none moving.
        quiet: u32,
    },
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
            Stage::Run { tick, limit, .. } => 0.25 + 0.75 * (*tick as f32 / (*limit).max(1) as f32),
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
                        Stage::Run {
                            run: Box::new(run),
                            tick: 0,
                            limit,
                            quiet: 0,
                        }
                    }
                    Err(e) => self.reject(build_rejection(e)),
                },
                Err(e) => self.reject(build_rejection(e)),
            },
            Stage::Run {
                mut run,
                mut tick,
                limit,
                mut quiet,
            } => {
                let n = RUN_SLICE_TICKS.min(limit - tick);
                let marbles = self.params.marbles as usize;
                let mut verdict = None;
                for _ in 0..n {
                    run.tick(1);
                    tick += 1;
                    let m = run.motion();
                    let finished = m
                        .iter()
                        .filter(|s| matches!(s, MarbleMotion::Finished))
                        .count();
                    if m.iter().any(|s| matches!(s, MarbleMotion::Lost)) {
                        verdict = Some(Err(Rejection::Lost {
                            tick,
                            finished,
                            marbles,
                        }));
                        break;
                    }
                    if finished == marbles {
                        verdict = Some(Ok(()));
                        break;
                    }
                    if m.iter().any(|s| matches!(s, MarbleMotion::OffTrack)) {
                        verdict = Some(Err(Rejection::OffTrack {
                            tick,
                            finished,
                            marbles,
                        }));
                        break;
                    }
                    if m.len() == marbles && !m.iter().any(|s| s.is_moving()) {
                        quiet += 1;
                        if quiet >= STALL_HOLD_TICKS {
                            verdict = Some(Err(Rejection::Stalled {
                                tick,
                                finished,
                                marbles,
                            }));
                            break;
                        }
                    } else {
                        quiet = 0;
                    }
                }
                match verdict {
                    Some(Ok(())) => Stage::Accepted,
                    Some(Err(r)) => self.reject(r),
                    None if tick >= limit => {
                        let finished = run
                            .motion()
                            .iter()
                            .filter(|s| matches!(s, MarbleMotion::Finished))
                            .count();
                        self.reject(Rejection::TimedOut {
                            limit,
                            finished,
                            marbles,
                        })
                    }
                    None => Stage::Run {
                        run,
                        tick,
                        limit,
                        quiet,
                    },
                }
            }
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

/// The golden jobs: default params at seed 7 (its first candidate is rejected) and a small track
/// at seed 2. `tests/golden/procgen_job.txt` holds their [`result_line`]s; it changes with
/// [`parble_procgen::GENERATOR_VERSION`] and with the sim.
pub fn golden_cases() -> [(u64, GenParams); 2] {
    let small = GenParams {
        height: 3,
        length: 14,
        footprint: 5,
        marbles: 2,
        ..GenParams::default()
    };
    [(7, GenParams::default()), (2, small)]
}

/// One line for a finished job: base seed, accepted attempt and seed, rejections and the FNV-1a
/// hash of the accepted `.parble` file (`-` when it failed).
pub fn result_line(base: u64, job: &GenJob) -> String {
    let file = job.accepted().map_or_else(
        || "-".to_owned(),
        |t| {
            let mut h = crate::hash::Fnv1a64::default();
            h.write(
                parble_core::ParbleFile::new(t.clone(), "generated")
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

/// The [`golden_cases`] run to the end, one [`result_line`] each.
pub fn golden_lines() -> String {
    golden_cases()
        .into_iter()
        .map(|(base, params)| {
            let mut job = GenJob::new(base, params).expect("golden seeds are in range");
            job.run_to_end();
            result_line(base, &job) + "\n"
        })
        .collect()
}
