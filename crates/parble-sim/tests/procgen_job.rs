//! The generate-and-validate job (P6-T2): sub-seed retries, the acceptance rule and slicing
//! invariance.

use parble_core::ParbleFile;
use parble_core::file::MAX_SAFE_INTEGER;
use parble_procgen::{GENERATOR_VERSION, GenParams, generate, generate_chain, segment_plan};
use parble_sim::procgen_job::{
    GenJob, JobError, JobStatus, MAX_ATTEMPTS, MAX_EXHAUSTED_IN_A_ROW, Rejection, SEGMENT_ATTEMPTS,
    SegmentedJob, limit_ticks, segment_seed, sub_seed,
};
use parble_sim::track_run::{MarbleMotion, TrackRun};

/// A base whose first whole-track run loses a marble on the second segment of
/// [`two_segments`] (`a_failing_segment_retries_without_the_segments_above_it`).
const SEGMENTED_RETRY_BASE: u64 = 57;

fn small() -> GenParams {
    GenParams {
        height: 3,
        length: 14,
        footprint: 5,
        marbles: 2,
        ..GenParams::default()
    }
}

fn bytes(job: &GenJob) -> String {
    ParbleFile::new(job.accepted().expect("accepted").clone(), "generated").to_json()
}

#[test]
fn sub_seeds_start_at_the_base_and_stay_safe_for_javascript() {
    for base in [0, 1, 7, MAX_SAFE_INTEGER as u64] {
        assert_eq!(sub_seed(base, 0), base);
        let seeds: std::collections::BTreeSet<u64> =
            (0..MAX_ATTEMPTS).map(|a| sub_seed(base, a)).collect();
        assert_eq!(seeds.len(), MAX_ATTEMPTS as usize, "base {base}");
        assert!(seeds.iter().all(|&s| s <= MAX_SAFE_INTEGER as u64));
    }
}

#[test]
fn a_seed_above_the_safe_integer_range_is_refused() {
    let big = MAX_SAFE_INTEGER as u64 + 1;
    assert_eq!(
        GenJob::new(big, small()).err(),
        Some(JobError::SeedOutOfRange(big))
    );
}

#[test]
fn bad_params_fail_at_once() {
    let mut job = GenJob::new(
        1,
        GenParams {
            length: 0,
            ..small()
        },
    )
    .unwrap();
    assert_eq!(job.run_to_end(), JobStatus::Failed);
    assert_eq!(job.rejected().len(), 1);
    assert!(matches!(
        job.rejected()[0].rejection,
        Rejection::Generate(_)
    ));
}

/// A length the height cannot carry fails on the first candidate instead of retrying every
/// sub-seed.
#[test]
fn a_length_too_long_for_the_height_fails_without_retrying() {
    let mut job = GenJob::new(
        1,
        GenParams {
            length: 100,
            height: 6,
            ..small()
        },
    )
    .unwrap();
    assert_eq!(job.run_to_end(), JobStatus::Failed);
    assert_eq!(job.rejected().len(), 1);
    assert!(
        matches!(
            job.rejected()[0].rejection,
            Rejection::Generate(parble_procgen::GenError::TooLong { max_length: 72, .. })
        ),
        "{:?}",
        job.rejected()
    );
}

/// The accepted track is the sub-seed's own generated track, its seed is that sub-seed, and every
/// marble of it finishes inside the limit when run from scratch.
#[test]
fn the_accepted_track_regenerates_from_its_seed_and_finishes() {
    let params = small();
    for base in [1u64, 2, 3] {
        let mut job = GenJob::new(base, params).unwrap();
        assert_eq!(
            job.run_to_end(),
            JobStatus::Accepted,
            "{:?}",
            job.rejected()
        );
        let track = job.accepted().unwrap().clone();
        assert_eq!(track.seed, sub_seed(base, job.attempt()));
        let defs: Vec<_> = parble_kit::builtin()
            .iter()
            .map(|p| p.def.clone())
            .collect();
        assert_eq!(generate(&defs, track.seed, &params).unwrap(), track);

        let mut run = TrackRun::new(&track).unwrap();
        run.tick(limit_ticks(&track));
        let m = run.motion();
        assert_eq!(m.len(), params.marbles as usize);
        assert!(
            m.iter().all(|s| matches!(s, MarbleMotion::Finished)),
            "{m:?}"
        );
    }
}

/// Default params, seed 32: a marble of the first candidate leaves the track (job census
/// 2026-11-15, generator versions 8 and 9), so the job rejects it and accepts a later sub-seed.
#[test]
fn a_failing_candidate_retries_with_the_next_sub_seed() {
    let mut job = GenJob::new(32, GenParams::default()).unwrap();
    assert_eq!(job.run_to_end(), JobStatus::Accepted);
    assert!(job.attempt() >= 1);
    let first = &job.rejected()[0];
    assert_eq!((first.attempt, first.seed), (0, 32));
    assert!(
        matches!(
            first.rejection,
            Rejection::Lost { .. }
                | Rejection::OffTrack { .. }
                | Rejection::Stalled { .. }
                | Rejection::TimedOut { .. }
        ),
        "{:?}",
        first.rejection
    );
    assert_eq!(job.accepted().unwrap().seed, sub_seed(32, job.attempt()));
}

/// However the work units are sliced, the job accepts the same sub-seed and the same bytes.
#[test]
fn slicing_does_not_change_the_result() {
    for (base, params) in [(7u64, GenParams::default()), (2, small())] {
        let mut whole = GenJob::new(base, params).unwrap();
        assert_eq!(whole.run_to_end(), JobStatus::Accepted);

        let mut sliced = GenJob::new(base, params).unwrap();
        let mut units = 0u32;
        let mut slice = 1u32;
        while sliced.status() == JobStatus::Running {
            for _ in 0..slice {
                if sliced.step() != JobStatus::Running {
                    break;
                }
                units += 1;
            }
            let p = sliced.candidate_progress();
            assert!((0.0..=1.0).contains(&p), "{p}");
            slice = slice % 7 + 1;
        }
        assert!(units > 10, "{units}");
        assert_eq!(sliced.status(), JobStatus::Accepted);
        assert_eq!(sliced.attempt(), whole.attempt());
        assert_eq!(sliced.rejected(), whole.rejected());
        assert_eq!(bytes(&sliced), bytes(&whole));
    }
}

/// The golden jobs' results, which the wasm build must also produce (`make determinism-wasm`).
#[test]
fn the_golden_jobs_match_their_golden() {
    let want = include_str!("golden/procgen_job.txt");
    let got = parble_sim::procgen_job::golden_lines();
    if got != want {
        eprintln!("PROCGEN JOB GOLDEN\n{got}");
    }
    assert_eq!(got, want);
}

/// A marble that leaves the track rejects its candidate there, not at the tick limit: default
/// params, base 517 (2026-11-15, generator version 9: off the track at tick 444 against a limit
/// of 20,160). Base 98, this test's seed under version 8, finishes now that its supports are tall
/// pillars.
#[test]
fn a_marble_leaving_the_track_rejects_the_candidate_at_once() {
    let mut job = GenJob::new(517, GenParams::default()).unwrap();
    assert_eq!(job.run_to_end(), JobStatus::Accepted);
    let first = &job.rejected()[0];
    assert!(
        matches!(first.rejection, Rejection::OffTrack { tick, .. } if tick < 2_000),
        "{:?}",
        first.rejection
    );
    let limit = limit_ticks(job.accepted().unwrap());
    assert!(
        limit < 25_000,
        "the limit counts track pieces only: {limit}"
    );
}

/// A search that keeps running out of nodes fails after [`MAX_EXHAUSTED_IN_A_ROW`] candidates
/// instead of all [`MAX_ATTEMPTS`]: 72 pieces from layer 6 on footprint 2, a plan of 25 cells.
#[test]
fn repeated_exhausted_searches_fail_the_job() {
    let params = GenParams {
        length: 72,
        height: 6,
        footprint: 2,
        ..GenParams::default()
    };
    let mut job = GenJob::new(0, params).unwrap();
    assert_eq!(job.run_to_end(), JobStatus::Failed);
    assert_eq!(job.rejected().len(), MAX_EXHAUSTED_IN_A_ROW as usize);
    assert!(job.rejected().iter().all(|a| matches!(
        a.rejection,
        Rejection::Generate(parble_procgen::GenError::Exhausted { .. })
    )));
}

/// Diagnostic: run jobs to the end and print every rejection. `PROCGEN_JOBS` lists
/// `length:height:footprint:base` cases; `PROCGEN_JOB_OUT` names a file the last accepted track
/// is written to as a `.parble` file.
#[test]
#[ignore = "diagnostic: job census"]
fn job_census() {
    let cases = std::env::var("PROCGEN_JOBS").unwrap_or_else(|_| "100:10:12:8".into());
    for case in cases.split(',') {
        let v: Vec<u64> = case.split(':').map(|x| x.parse().unwrap()).collect();
        let params = GenParams {
            length: v[0] as u32,
            height: v[1] as u32,
            footprint: v[2] as u32,
            ..GenParams::default()
        };
        let start = std::time::Instant::now();
        let mut job = GenJob::new(v[3], params).unwrap();
        let status = job.run_to_end();
        println!(
            "{case}: {status:?} attempt {} in {:.1} s",
            job.attempt(),
            start.elapsed().as_secs_f64()
        );
        for a in job.rejected() {
            println!("  attempt {} seed {}: {}", a.attempt, a.seed, a.rejection);
        }
        if let Some(t) = job.accepted() {
            println!(
                "  accepted, limit {}, {} placements",
                limit_ticks(t),
                t.placements.len()
            );
            if let Ok(path) = std::env::var("PROCGEN_JOB_OUT") {
                std::fs::write(&path, bytes(&job)).unwrap();
                println!("  wrote {path}");
            }
        }
    }
}

fn two_segments() -> GenParams {
    GenParams {
        height: 12,
        length: 80,
        footprint: 14,
        ..GenParams::default()
    }
}

fn kit_defs() -> Vec<parble_core::PieceDef> {
    parble_kit::builtin()
        .iter()
        .map(|p| p.def.clone())
        .collect()
}

#[test]
fn segment_seeds_are_distinct_and_stay_safe_for_javascript() {
    for base in [0, 1, 7, MAX_SAFE_INTEGER as u64] {
        let mut seeds = std::collections::BTreeSet::new();
        for segment in 0..16 {
            for attempt in 0..SEGMENT_ATTEMPTS {
                let s = segment_seed(base, segment, attempt);
                assert!(s <= MAX_SAFE_INTEGER as u64);
                seeds.insert(s);
            }
        }
        assert_eq!(seeds.len(), 16 * SEGMENT_ATTEMPTS as usize, "base {base}");
        // The first segment draws a whole-track job's seeds.
        assert_eq!(segment_seed(base, 0, 0), base);
        assert_eq!(segment_seed(base, 0, 3), sub_seed(base, 3));
    }
    assert_eq!(
        SegmentedJob::new(MAX_SAFE_INTEGER as u64 + 1, two_segments()).err(),
        Some(JobError::SeedOutOfRange(MAX_SAFE_INTEGER as u64 + 1))
    );
}

/// The accepted segmented track regenerates byte for byte from the seeds its file records, its
/// seed is the base seed, and every marble of it finishes inside the limit when run from scratch.
#[test]
fn a_segmented_track_regenerates_from_its_seeds_and_finishes() {
    let params = two_segments();
    assert_eq!(segment_plan(&params).len(), 2);
    for base in [1u64, 2, 3] {
        let mut job = SegmentedJob::new(base, params).unwrap();
        assert_eq!(
            job.run_to_end(),
            JobStatus::Accepted,
            "{:?}",
            job.rejected()
        );
        let track = job.accepted().unwrap().clone();
        assert_eq!(track.seed, base);
        let seeds = job.segment_seeds();
        assert_eq!(seeds.len(), 2);
        assert_eq!(
            generate_chain(&kit_defs(), base, &params, &seeds).unwrap(),
            track
        );

        let file = job.file("generated").unwrap();
        let loaded = parble_core::file::load(&file.to_json()).unwrap();
        assert_eq!(loaded, file);
        let stamp = loaded.metadata.generator.unwrap();
        assert_eq!(stamp.version, GENERATOR_VERSION);
        assert_eq!(stamp.segments, seeds);
        let again = generate_chain(&kit_defs(), loaded.track.seed, &params, &stamp.segments);
        assert_eq!(
            parble_procgen::generated_file(again.unwrap(), "generated", &stamp.segments).to_json(),
            file.to_json()
        );

        let mut run = TrackRun::new(&track).unwrap();
        run.tick(limit_ticks(&track));
        let m = run.motion();
        assert_eq!(m.len(), params.marbles as usize);
        assert!(
            m.iter().all(|s| matches!(s, MarbleMotion::Finished)),
            "{m:?}"
        );
    }
}

/// A marble that fails on a lower segment costs that segment and the ones below it: the segments
/// above keep their seeds. [`SEGMENTED_RETRY_BASE`] (2026-11-15, generator version 9, physics version 11): the
/// marbles of the first run stall on the second of two segments by tick 9,221.
#[test]
fn a_failing_segment_retries_without_the_segments_above_it() {
    let (base, params) = (SEGMENTED_RETRY_BASE, two_segments());
    let mut job = SegmentedJob::new(base, params).unwrap();
    assert_eq!(job.run_to_end(), JobStatus::Accepted);
    let first = &job.rejected()[0];
    assert!(first.segment >= 1, "{:?}", job.rejected());
    assert!(
        matches!(
            first.rejection,
            Rejection::Lost { .. }
                | Rejection::OffTrack { .. }
                | Rejection::Stalled { .. }
                | Rejection::TimedOut { .. }
        ),
        "{:?}",
        first.rejection
    );
    assert!(job.rejected().iter().all(|a| a.segment >= 1));
    let seeds = job.segment_seeds();
    assert_eq!(seeds[0], segment_seed(base, 0, 0));
    assert_ne!(seeds[1], segment_seed(base, 1, 0));
    assert_eq!(job.runs() as usize, job.rejected().len() + 1);
}

/// However the work units are sliced, the segmented job accepts the same seeds and bytes.
#[test]
fn slicing_does_not_change_a_segmented_result() {
    let (base, params) = (SEGMENTED_RETRY_BASE, two_segments());
    let mut whole = SegmentedJob::new(base, params).unwrap();
    assert_eq!(whole.run_to_end(), JobStatus::Accepted);

    let mut sliced = SegmentedJob::new(base, params).unwrap();
    let mut slice = 1u32;
    while sliced.status() == JobStatus::Running {
        for _ in 0..slice {
            if sliced.step() != JobStatus::Running {
                break;
            }
        }
        let p = sliced.candidate_progress();
        assert!((0.0..=1.0).contains(&p), "{p}");
        slice = slice % 7 + 1;
    }
    assert_eq!(sliced.status(), JobStatus::Accepted);
    assert_eq!(sliced.segment_seeds(), whole.segment_seeds());
    assert_eq!(sliced.rejected(), whole.rejected());
    assert_eq!(
        sliced.file("generated").unwrap().to_json(),
        whole.file("generated").unwrap().to_json()
    );
}

#[test]
fn a_segmented_job_with_bad_params_fails_at_once() {
    let mut job = SegmentedJob::new(
        1,
        GenParams {
            length: 0,
            ..two_segments()
        },
    )
    .unwrap();
    assert_eq!(job.run_to_end(), JobStatus::Failed);
    assert_eq!(job.rejected().len(), 1);
    assert!(job.accepted().is_none() && job.file("x").is_none());
}

/// The 1-minute load average, from `uptime`.
fn load_1m() -> String {
    std::process::Command::new("uptime")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| {
            s.rsplit("load average")
                .next()
                .and_then(|t| t.split([' ', ',']).find(|w| w.parse::<f64>().is_ok()))
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "?".into())
}

/// The segmented job accepts a 500-piece track (500 from layer 48 on footprint 14), and reports
/// its wall time and the 1-minute load. `PROCGEN_SEG_SEEDS` lists the base seeds (default `0`),
/// `PROCGEN_SEG_CASE` another `length:height:footprint`. Run with `--release -- --ignored
/// --nocapture`: a debug build takes minutes.
#[test]
#[ignore = "long: a 500-piece track run headless"]
fn the_segmented_job_accepts_a_500_piece_track() {
    let case = std::env::var("PROCGEN_SEG_CASE").unwrap_or_else(|_| "500:48:14".into());
    let v: Vec<u32> = case.split(':').map(|x| x.parse().unwrap()).collect();
    let params = GenParams {
        length: v[0],
        height: v[1],
        footprint: v[2],
        ..GenParams::default()
    };
    let seeds = std::env::var("PROCGEN_SEG_SEEDS").unwrap_or_else(|_| "0".into());
    let kit = parble_kit::builtin();
    for base in seeds.split(',').map(|s| s.parse::<u64>().unwrap()) {
        let before = load_1m();
        let start = std::time::Instant::now();
        let mut job = SegmentedJob::new(base, params).unwrap();
        let status = job.run_to_end();
        let secs = start.elapsed().as_secs_f64();
        println!(
            "segmented {case} base {base}: {status:?} in {secs:.1} s, {} runs, {} rejected, {} segments, load {before} before and {} after",
            job.runs(),
            job.rejected().len(),
            job.segment_count(),
            load_1m()
        );
        for a in job.rejected() {
            println!(
                "  segment {} attempt {} seed {}: {}",
                a.segment, a.attempt, a.seed, a.rejection
            );
        }
        assert_eq!(status, JobStatus::Accepted, "base {base}");
        let track = job.accepted().unwrap();
        let pieces = track
            .placements
            .iter()
            .filter(|pl| !kit.iter().any(|k| k.def.id == pl.piece && k.def.structural))
            .count();
        println!(
            "  accepted: {pieces} track pieces, {} placements, limit {} ticks, seeds {:?}",
            track.placements.len(),
            limit_ticks(track),
            job.segment_seeds()
        );
        assert!(pieces as u32 * 10 >= params.length * 9);
        assert_eq!(
            &generate_chain(&kit_defs(), base, &params, &job.segment_seeds()).unwrap(),
            track
        );
    }
}
