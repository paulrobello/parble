//! Physical pass rate of generated tracks (P6-T1). `validate_track` proves a generated track is
//! statically sound, not that its marbles finish: this runs each one in the sim.
//!
//! Ignored by default (slow). `cargo test -p parble-sim --release --test procgen_pass_rate --
//! --ignored --nocapture`. Seeds from `PROCGEN_SEEDS` (default 30), ticks from `PROCGEN_TICKS`.

use parble_procgen::{GenParams, generate};
use parble_sim::track_run::{MarbleMotion, TrackRun};

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[test]
#[ignore = "slow: runs generated tracks in the sim"]
fn generated_tracks_finish_in_the_sim() {
    let defs: Vec<_> = parble_kit::builtin()
        .iter()
        .map(|p| p.def.clone())
        .collect();
    let seeds = env("PROCGEN_SEEDS", 30);
    let ticks = env("PROCGEN_TICKS", 240 * 60) as u32;
    let params = GenParams::default();
    let mut passed = 0;
    for seed in 0..seeds {
        let track = generate(&defs, seed, &params).expect("generates");
        let mut run = TrackRun::new(&track).expect("runs");
        run.tick(ticks);
        let m = run.motion();
        let fin = m
            .iter()
            .filter(|s| matches!(s, MarbleMotion::Finished))
            .count();
        let ok = fin == m.len() && !m.is_empty();
        passed += usize::from(ok);
        println!("seed {seed}: {fin}/{} finished {:?}", m.len(), m);
    }
    println!("PASS RATE {passed}/{seeds}");
}

/// Where does the first marble go? Prints its cell and the piece there every 30 ticks.
#[test]
#[ignore = "diagnostic: prints a marble's path"]
fn trace_first_marble() {
    use parble_core::{CellCoord, PieceCatalog};
    let defs: Vec<_> = parble_kit::builtin()
        .iter()
        .map(|p| p.def.clone())
        .collect();
    let seed = env("PROCGEN_TRACE_SEED", 0);
    let track = generate(&defs, seed, &GenParams::default()).expect("generates");
    let kit = parble_kit::builtin();
    let mut run = TrackRun::new(&track).expect("runs");
    for step in 0..400 {
        run.tick(30);
        let Some(m) = run.marbles().first().copied() else {
            continue;
        };
        let cell = CellCoord::new(
            (m.position[0] / 0.06).round() as i32,
            (m.position[1] / 0.06).floor() as i32,
            (m.position[2] / 0.06).round() as i32,
        );
        let piece = track.placements.iter().enumerate().find(|(_, p)| {
            kit.get(&p.piece)
                .is_some_and(|d| p.world_cells(d).contains(&cell))
        });
        let speed = (m.linvel[0].powi(2) + m.linvel[1].powi(2) + m.linvel[2].powi(2)).sqrt();
        println!(
            "t={:5.2}s cell=({},{},{}) {:?} v={speed:.2} {:?}",
            f64::from(step + 1) * 0.125,
            cell.x,
            cell.y,
            cell.z,
            piece.map(|(i, p)| format!("#{i} {}", p.piece)),
            run.motion().first()
        );
        if matches!(
            run.motion().first(),
            Some(MarbleMotion::Lost | MarbleMotion::Finished | MarbleMotion::OffTrack)
        ) {
            break;
        }
    }
}

/// Where does each seed's first marble leave its track, and how fast was it going?
#[test]
#[ignore = "diagnostic: failure census"]
fn failure_census() {
    use parble_core::{CellCoord, PieceCatalog};
    let defs: Vec<_> = parble_kit::builtin()
        .iter()
        .map(|p| p.def.clone())
        .collect();
    let kit = parble_kit::builtin();
    let seeds = env("PROCGEN_SEEDS", 20);
    for seed in 0..seeds {
        let track = generate(&defs, seed, &GenParams::default()).expect("generates");
        let mut run = TrackRun::new(&track).expect("runs");
        let mut last: Vec<(usize, String, f32)> = Vec::new();
        let mut outcome = String::from("timeout");
        for _ in 0..1600 {
            run.tick(15);
            let Some(m) = run.marbles().first().copied() else {
                continue;
            };
            let cell = CellCoord::new(
                (m.position[0] / 0.06).round() as i32,
                (m.position[1] / 0.06).floor() as i32,
                (m.position[2] / 0.06).round() as i32,
            );
            let speed = (m.linvel[0].powi(2) + m.linvel[1].powi(2) + m.linvel[2].powi(2)).sqrt();
            if let Some((i, p)) = track.placements.iter().enumerate().find(|(_, p)| {
                kit.get(&p.piece)
                    .is_some_and(|d| p.world_cells(d).contains(&cell))
            }) && last.last().map(|l| l.0) != Some(i)
            {
                last.push((i, p.piece.to_string(), speed));
            }
            match run.motion().first() {
                Some(MarbleMotion::Finished) => {
                    outcome = "FINISHED".into();
                    break;
                }
                Some(MarbleMotion::Lost | MarbleMotion::OffTrack | MarbleMotion::AtRest) => {
                    outcome = format!("{:?}", run.motion().first().unwrap());
                    break;
                }
                _ => {}
            }
        }
        let tail: Vec<String> = last
            .iter()
            .rev()
            .take(3)
            .rev()
            .map(|(i, p, v)| format!("#{i} {p} v={v:.2}"))
            .collect();
        println!(
            "seed {seed}: {outcome} after {} pieces; {}",
            last.len(),
            tail.join(" | ")
        );
    }
}
