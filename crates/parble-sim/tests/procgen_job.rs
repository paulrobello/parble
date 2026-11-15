//! The generate-and-validate job (P6-T2): sub-seed retries, the acceptance rule and slicing
//! invariance.

use parble_core::ParbleFile;
use parble_core::file::MAX_SAFE_INTEGER;
use parble_procgen::{GenParams, generate};
use parble_sim::procgen_job::{
    GenJob, JobError, JobStatus, MAX_ATTEMPTS, MAX_EXHAUSTED_IN_A_ROW, Rejection, limit_ticks,
    sub_seed,
};
use parble_sim::track_run::{MarbleMotion, TrackRun};

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

/// Default params, seed 7: a marble of the first candidate leaves the track (finish-time census
/// 2026-11-15), so the job rejects it and accepts a later sub-seed.
#[test]
fn a_failing_candidate_retries_with_the_next_sub_seed() {
    let mut job = GenJob::new(7, GenParams::default()).unwrap();
    assert_eq!(job.run_to_end(), JobStatus::Accepted);
    assert!(job.attempt() >= 1);
    let first = &job.rejected()[0];
    assert_eq!((first.attempt, first.seed), (0, 7));
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
    assert_eq!(job.accepted().unwrap().seed, sub_seed(7, job.attempt()));
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

/// A marble that leaves the track rejects its candidate there, not at the tick limit: length 100
/// from layer 10, base 8 (2026-11-15: off the track at tick 10,837 against a limit of 94,320 when
/// supports counted).
#[test]
fn a_marble_leaving_the_track_rejects_the_candidate_at_once() {
    let params = GenParams {
        length: 100,
        height: 10,
        footprint: 12,
        ..GenParams::default()
    };
    let mut job = GenJob::new(8, params).unwrap();
    assert_eq!(job.run_to_end(), JobStatus::Accepted);
    let first = &job.rejected()[0];
    assert!(
        matches!(first.rejection, Rejection::OffTrack { tick, .. } if tick < 12_000),
        "{:?}",
        first.rejection
    );
    let limit = limit_ticks(job.accepted().unwrap());
    assert!(
        limit < 40_000,
        "the limit counts track pieces only: {limit}"
    );
}

/// A search that keeps running out of nodes fails after [`MAX_EXHAUSTED_IN_A_ROW`] candidates
/// instead of all [`MAX_ATTEMPTS`]: 500 pieces from layer 42 on footprint 14.
#[test]
fn repeated_exhausted_searches_fail_the_job() {
    let params = GenParams {
        length: 500,
        height: 42,
        footprint: 14,
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
/// `length:height:footprint:base` cases.
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
            println!("  accepted, limit {}", limit_ticks(t));
        }
    }
}
