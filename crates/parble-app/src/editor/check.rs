//! The editor's marble check (PRD §6 P3, owner request 2026-11-09): on demand from the validation
//! panel (`check`), run the release the Run button would make once per marble spec
//! ([`check_kinds`]: cat's eye standing for swirl, onionskin, opal and glow, then glazed and
//! steel), headless and off the live sim, and report where each type falls off or stalls
//! ([`parble_sim::marble_check`]).
//!
//! - Release: the track with its hopper resolved and the release the action names, as `run`
//!   builds it ([`editor_track`]), with every queued marble spawned as the checked type. The
//!   queue's own sizes run, or with `allSizes` the queue at each of mini, standard and shooter.
//! - Never blocks a frame: [`step_check`] does [`MarbleCheck::step`] units inside
//!   [`CHECK_BUDGET_MS`] a frame, a unit starting only while the longest unit so far still fits,
//!   and waits while a run, stop or reset is building.
//! - Stale: an edit (any change of [`EditorDiag::edits`], a loaded track included) stops a check
//!   under way and marks a finished one stale. It never reruns by itself.
//! - Results are warnings, never validation issues: they sit in [`EditorDiag::check`], a list
//!   parallel to `issues` that the panel shows under them. Each warning is anchored to the
//!   placement holding the marble, so the panel's Show focuses it like an issue.

use std::collections::BTreeMap;

use bevy::{platform::time::Instant, prelude::*};
use parble_core::{MarbleSize, Track};
use parble_sim::marble::MarbleKind;
use parble_sim::marble_check::{
    CheckReport, MarbleCheck, Outcome, check_kinds, size_name, track_at_size,
};
use serde::Serialize;

use super::{EditState, EditorDiag, cell_arr, editor_track, release_mode, release_name};

/// Check work per frame, a ceiling: a unit starts only while the longest unit so far still fits.
/// The live sim, the renderer and the overlay share the frame, so this is well under the run
/// build's [`super::RUN_BUILD_BUDGET_MS`].
pub const CHECK_BUDGET_MS: f32 = 4.0;
/// Warnings listed in the diag at most (`warningCount` is always exact).
pub const MAX_WARNINGS: usize = 32;
/// The sizes `allSizes` checks.
const SIZES: [MarbleSize; 3] = [MarbleSize::Mini, MarbleSize::Standard, MarbleSize::Shooter];

/// The release a check ran: what the panel names.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CheckRelease {
    pub release: &'static str,
    pub interval_ticks: u32,
    pub marbles: usize,
    pub seed: u64,
    /// The sizes checked: the queue's own, or all three.
    pub sizes: Vec<&'static str>,
    pub all_sizes: bool,
}

/// One check run: one marble spec at one size (or the queue's own sizes).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CheckRunDiag {
    pub marble: &'static str,
    /// Types that share `marble`'s spec, and so its outcome.
    pub also: Vec<&'static str>,
    /// `null` when the queue's own sizes ran.
    pub size: Option<&'static str>,
    pub marbles: usize,
    pub finished: usize,
    pub fell_off: usize,
    pub stalled: usize,
    /// Ticks the run took to settle or reach its cap.
    pub ticks: u64,
}

/// Where marbles of one type and size fell off or stalled, anchored like an issue.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CheckWarning {
    /// `fellOff` or `stalled`.
    pub kind: &'static str,
    pub marble: &'static str,
    pub also: Vec<&'static str>,
    pub size: &'static str,
    /// The placement: for a fall, the one the marble entered last; for a stall, the one holding it.
    pub piece: Option<String>,
    pub anchor: Option<[i32; 3]>,
    pub cell: Option<[i32; 3]>,
    /// The marbles (release order) this happened to.
    pub marbles: Vec<usize>,
    /// Of a stall's marbles, how many were still rolling when the check ended.
    pub rolling: usize,
    /// Earliest tick it happened at (for a stall, when the check ended).
    pub tick: u64,
}

/// Timing of the last check, ms.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CheckTiming {
    pub frames: usize,
    pub work_ms: f32,
    pub frame_ms_max: f32,
    pub unit_ms_max: f32,
    pub wall_ms: f32,
}

/// `window.__parble.editor.check`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CheckDiag {
    /// `none`, `running`, `done`, `stale` or `failed`.
    pub state: &'static str,
    /// Share of the check done, 0 to 1.
    pub progress: f32,
    /// The run under way: marble type and size.
    pub current: Option<String>,
    pub release: Option<CheckRelease>,
    /// [`EditorDiag::edits`] the check is of.
    pub edits: u64,
    pub runs: Vec<CheckRunDiag>,
    pub warning_count: usize,
    pub warnings: Vec<CheckWarning>,
    pub error: Option<String>,
    pub timing: Option<CheckTiming>,
}

impl Default for CheckDiag {
    fn default() -> Self {
        Self {
            state: "none",
            progress: 0.0,
            current: None,
            release: None,
            edits: 0,
            runs: Vec::new(),
            warning_count: 0,
            warnings: Vec::new(),
            error: None,
            timing: None,
        }
    }
}

struct Job {
    track: Track,
    runs: Vec<(MarbleKind, Vec<MarbleKind>, Option<MarbleSize>)>,
    next: usize,
    current: Option<MarbleCheck>,
    reports: Vec<CheckReport>,
    started: Instant,
    timing: CheckTiming,
}

/// The editor's check under way, if any.
#[derive(Default)]
pub(super) struct CheckState {
    job: Option<Job>,
}

/// Start a check of `release` (a `run` release name and interval) on the editor's track, replacing
/// any check under way.
pub(super) fn start_check(
    st: &mut EditState,
    diag: &mut EditorDiag,
    release: &str,
    interval_ticks: Option<u32>,
    all_sizes: bool,
) -> Result<(), String> {
    let mode = release_mode(release, interval_ticks)?;
    let track = editor_track(st.editor.track(), Some(mode)).map_err(|e| format!("check: {e}"))?;
    let mut queued: Vec<MarbleSize> = track.hopper.queue.clone();
    queued.sort();
    queued.dedup();
    let sizes: Vec<Option<MarbleSize>> = if all_sizes {
        SIZES.into_iter().map(Some).collect()
    } else if queued.len() == 1 {
        vec![Some(queued[0])]
    } else {
        vec![None]
    };
    let runs = check_kinds()
        .into_iter()
        .flat_map(|(k, same)| sizes.iter().map(move |&s| (k, same.clone(), s)))
        .collect();
    diag.check = CheckDiag {
        state: "running",
        release: Some(CheckRelease {
            release: release_name(mode),
            interval_ticks: match mode {
                parble_core::ReleaseMode::Interval { ticks } => ticks,
                _ => 0,
            },
            marbles: track.hopper.queue.len(),
            seed: track.seed,
            sizes: if all_sizes {
                SIZES.into_iter().map(size_name).collect()
            } else {
                queued.into_iter().map(size_name).collect()
            },
            all_sizes,
        }),
        edits: diag.edits,
        ..default()
    };
    st.check.job = Some(Job {
        track,
        runs,
        next: 0,
        current: None,
        reports: Vec::new(),
        started: Instant::now(),
        timing: CheckTiming::default(),
    });
    Ok(())
}

/// The track of check run `(kind, size)`.
fn run_track(track: &Track, size: Option<MarbleSize>) -> Track {
    match size {
        Some(s) => track_at_size(track, s),
        None => track.clone(),
    }
}

fn label(kind: MarbleKind, size: Option<MarbleSize>) -> String {
    match size {
        Some(s) => format!("{} {}", kind.name(), size_name(s)),
        None => kind.name().to_owned(),
    }
}

/// Advance the check by up to `budget_ms` of work (all of it with `None`), and fill
/// [`EditorDiag::check`]. An edit since the check started stops it and marks it stale.
pub(super) fn step(st: &mut EditState, diag: &mut EditorDiag, budget_ms: Option<f32>) {
    if diag.check.edits != diag.edits && matches!(diag.check.state, "running" | "done") {
        diag.check.state = "stale";
        diag.check.current = None;
        st.check.job = None;
        return;
    }
    let Some(job) = st.check.job.as_mut() else {
        return;
    };
    let start = Instant::now();
    let ms = |t: Instant| t.elapsed().as_secs_f32() * 1e3;
    let mut units = 0;
    let mut failed = None;
    while job.next < job.runs.len() {
        if units > 0 && budget_ms.is_some_and(|b| ms(start) + job.timing.unit_ms_max > b) {
            break;
        }
        units += 1;
        let (kind, _, size) = job.runs[job.next].clone();
        let check = job
            .current
            .get_or_insert_with(|| MarbleCheck::new(&run_track(&job.track, size), kind));
        let one = Instant::now();
        let done = match check.step() {
            Ok(d) => d,
            Err(e) => {
                failed = Some(format!("check {}: {e}", label(kind, size)));
                break;
            }
        };
        job.timing.unit_ms_max = job.timing.unit_ms_max.max(ms(one));
        if done {
            let report = job
                .current
                .take()
                .and_then(|c| c.report().cloned())
                .expect("a done check has a report");
            job.reports.push(report);
            job.next += 1;
        }
    }
    let work = ms(start);
    job.timing.frames += 1;
    job.timing.work_ms += work;
    job.timing.frame_ms_max = job.timing.frame_ms_max.max(work);
    let total = job.runs.len() as f32;
    let part = job.current.as_ref().map_or(0.0, MarbleCheck::progress);
    diag.check.progress = ((job.next as f32 + part) / total).min(1.0);
    diag.check.current = job.runs.get(job.next).map(|(k, _, s)| label(*k, *s));
    if let Some(e) = failed {
        error!("editor: {e}");
        diag.check.state = "failed";
        diag.check.error = Some(e);
        st.check.job = None;
        return;
    }
    if job.next == job.runs.len() {
        let job = st.check.job.take().expect("the check is under way");
        let mut timing = job.timing;
        timing.wall_ms = ms(job.started);
        let (runs, warnings) = summarize(&job.track, &job.runs, &job.reports);
        diag.check.state = "done";
        diag.check.progress = 1.0;
        diag.check.current = None;
        diag.check.runs = runs;
        diag.check.warning_count = warnings.len();
        diag.check.warnings = warnings.into_iter().take(MAX_WARNINGS).collect();
        diag.check.timing = Some(timing);
    }
}

/// Per run counts, and the warnings grouped by type, size, outcome and placement, in run order
/// then placement order.
fn summarize(
    track: &Track,
    runs: &[(MarbleKind, Vec<MarbleKind>, Option<MarbleSize>)],
    reports: &[CheckReport],
) -> (Vec<CheckRunDiag>, Vec<CheckWarning>) {
    let names = |v: &[MarbleKind]| v.iter().map(|k| k.name()).collect::<Vec<_>>();
    let mut out_runs = Vec::new();
    let mut warnings = Vec::new();
    for ((kind, same, size), r) in runs.iter().zip(reports) {
        let count = |f: fn(&Outcome) -> bool| r.marbles.iter().filter(|m| f(&m.outcome)).count();
        out_runs.push(CheckRunDiag {
            marble: kind.name(),
            also: names(same),
            size: size.map(size_name),
            marbles: r.marbles.len(),
            finished: count(|o| *o == Outcome::Finished),
            fell_off: count(|o| matches!(o, Outcome::FellOff { .. })),
            stalled: count(|o| matches!(o, Outcome::Stalled { .. })),
            ticks: r.ticks,
        });
        // (outcome, placement, size) → warning, in placement order.
        let mut groups: BTreeMap<(u8, Option<usize>, MarbleSize), CheckWarning> = BTreeMap::new();
        for m in r.problems() {
            let (k, at, rolling) = match m.outcome {
                Outcome::FellOff { last } => (0, last, false),
                Outcome::Stalled { at, moving } => (1, at, moving),
                Outcome::Finished => continue,
            };
            let p = at.and_then(|i| track.placements.get(i));
            let w = groups
                .entry((k, at, m.size))
                .or_insert_with(|| CheckWarning {
                    kind: if k == 0 { "fellOff" } else { "stalled" },
                    marble: kind.name(),
                    also: names(same),
                    size: size_name(m.size),
                    piece: p.map(|p| p.piece.as_str().to_owned()),
                    anchor: p.map(|p| cell_arr(p.anchor)),
                    cell: m.cell.map(cell_arr),
                    marbles: Vec::new(),
                    rolling: 0,
                    tick: m.tick,
                });
            w.marbles.push(m.marble);
            w.rolling += usize::from(rolling);
            w.tick = w.tick.min(m.tick);
        }
        warnings.extend(groups.into_values());
    }
    (out_runs, warnings)
}
