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

/// Default params, with `PROCGEN_FAMILIES` (letters `b`, `r`, `m`) narrowing the piece families.
fn params_from_env() -> GenParams {
    let mut params = GenParams::default();
    if let Ok(f) = std::env::var("PROCGEN_FAMILIES") {
        params.families = parble_procgen::Families {
            blk: f.contains('b'),
            rail: f.contains('r'),
            mech: f.contains('m'),
        };
    }
    params.length = env("PROCGEN_LENGTH", u64::from(params.length)) as u32;
    params.marbles = env("PROCGEN_MARBLES", u64::from(params.marbles)) as u32;
    params.height = env("PROCGEN_HEIGHT", u64::from(params.height)) as u32;
    params.special_per_mille = env("PROCGEN_SPECIAL", u64::from(params.special_per_mille)) as u32;
    params
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
    let params = params_from_env();
    let mut passed = 0;
    for seed in 0..seeds {
        let track = match generate(&defs, seed, &params) {
            Ok(t) => t,
            Err(e) => {
                println!("seed {seed}: generation failed: {e:?}");
                continue;
            }
        };
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
    let track = generate(&defs, seed, &params_from_env()).expect("generates");
    let kit = parble_kit::builtin();
    let mut run = TrackRun::new(&track).expect("runs");
    for step in 0..(env("PROCGEN_TRACE_STEPS", 400) as u32) {
        run.tick(env("PROCGEN_TRACE_TICKS", 30) as u32);
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
            "t={:5.2}s cell=({},{},{}) {:?} v={speed:.2} pos=({:.3},{:.3},{:.3}) {:?}",
            f64::from(step + 1) * 0.004_166_67 * env("PROCGEN_TRACE_TICKS", 30) as f64,
            cell.x,
            cell.y,
            cell.z,
            piece.map(|(i, p)| format!("#{i} {}", p.piece)),
            m.position[0],
            m.position[1],
            m.position[2],
            run.motion().first()
        );
        if matches!(
            run.motion().first(),
            Some(MarbleMotion::Lost | MarbleMotion::Finished | MarbleMotion::OffTrack)
        ) {
            break;
        }
        if matches!(run.motion().first(), Some(MarbleMotion::AtRest)) {
            println!("rest position {:?} (metres)", m.position);
            for (i, p) in track
                .placements
                .iter()
                .enumerate()
                .take(env("PROCGEN_TRACE_DUMP", 16) as usize)
            {
                println!(
                    "  #{i} {} anchor ({},{},{}) yaw {:?} mirror {:?}",
                    p.piece, p.anchor.x, p.anchor.y, p.anchor.z, p.yaw, p.mirror
                );
            }
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
        let Ok(track) = generate(&defs, seed, &params_from_env()) else {
            println!("seed {seed}: generation failed");
            continue;
        };
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

/// Per-seed first-marble walk: speed on entering each piece, and which pieces are transfer
/// sources. Prints `seed outcome | piece(v)[T]...` so a failure can be tied to a transfer.
#[test]
#[ignore = "diagnostic: transfer census"]
fn transfer_census() {
    use parble_core::{CellCoord, PieceCatalog, controlled_drops};
    let defs: Vec<_> = parble_kit::builtin()
        .iter()
        .map(|p| p.def.clone())
        .collect();
    let kit = parble_kit::builtin();
    let seeds = env("PROCGEN_SEEDS", 20);
    for seed in 0..seeds {
        let Ok(track) = generate(&defs, seed, &params_from_env()) else {
            println!("seed {seed}: generation failed");
            continue;
        };
        let sources: std::collections::BTreeSet<usize> = controlled_drops(&track, kit)
            .iter()
            .map(|d| d.from)
            .collect();
        let mut run = TrackRun::new(&track).expect("runs");
        let mut walk: Vec<(usize, f32)> = Vec::new();
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
            if let Some((i, _)) = track.placements.iter().enumerate().find(|(_, p)| {
                kit.get(&p.piece)
                    .is_some_and(|d| p.world_cells(d).contains(&cell))
            }) && walk.last().map(|l| l.0) != Some(i)
            {
                walk.push((i, speed));
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
        let line: Vec<String> = walk
            .iter()
            .map(|(i, v)| {
                format!(
                    "{}{}({v:.2})",
                    track.placements[*i].piece,
                    if sources.contains(i) { "[T]" } else { "" }
                )
            })
            .collect();
        if let (Some(m), Some((i, _))) = (run.marbles().first().copied(), walk.last().copied())
            && outcome == "AtRest"
            && track.placements[i].piece.as_str() == "blk_drop_l2"
        {
            // Rest position in the drop's own frame: millimetres from its cell centre, rotated
            // back by the placement's yaw (mirror ignored).
            let p = &track.placements[i];
            let dx = m.position[0] - p.anchor.x as f32 * 0.06;
            let dz = m.position[2] - p.anchor.z as f32 * 0.06;
            let (s, c) = (f32::from(p.yaw.quarter_turns()) * std::f32::consts::FRAC_PI_2).sin_cos();
            println!(
                "DROPREST seed {seed} yaw {} mirror {:?} local ({:.1}, {:.1}) mm height {:.1} mm above cell floor",
                p.yaw.quarter_turns(),
                p.mirror,
                (dx * c - dz * s) * 1000.0,
                (dx * s + dz * c) * 1000.0,
                (m.position[1] - p.anchor.y as f32 * 0.06) * 1000.0
            );
        }
        // The piece the last non-pillar piece should have handed the marble to.
        let next = walk
            .iter()
            .rev()
            .find(|(i, _)| !track.placements[*i].piece.as_str().contains("pillar"))
            .and_then(|(i, _)| track.placements.get(i + 1))
            .map_or("-", |p| p.piece.as_str());
        println!("seed {seed}: {outcome} | {} NEXT {next}", line.join(" "));
    }
}

/// Does a marble get through each `blk_drop_l2` orientation? Counts, per yaw, the drops the
/// first marble reached and the ones it entered the catch below.
#[test]
#[ignore = "diagnostic: drop orientation census"]
fn drop_orientation_census() {
    use parble_core::{CellCoord, PieceCatalog};
    let defs: Vec<_> = parble_kit::builtin()
        .iter()
        .map(|p| p.def.clone())
        .collect();
    let kit = parble_kit::builtin();
    let mut tally: std::collections::BTreeMap<String, [u32; 2]> = Default::default();
    for seed in 0..env("PROCGEN_SEEDS", 20) {
        let Ok(track) = generate(&defs, seed, &params_from_env()) else {
            continue;
        };
        let mut run = TrackRun::new(&track).expect("runs");
        let mut seen: std::collections::BTreeSet<usize> = Default::default();
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
            if let Some((i, _)) = track.placements.iter().enumerate().find(|(_, p)| {
                kit.get(&p.piece)
                    .is_some_and(|d| p.world_cells(d).contains(&cell))
            }) {
                seen.insert(i);
            }
            if !matches!(run.motion().first(), Some(MarbleMotion::Moving { .. })) {
                break;
            }
        }
        for (i, p) in track.placements.iter().enumerate() {
            if p.piece.as_str() != "blk_drop_l2" || !seen.contains(&i) {
                continue;
            }
            let key = format!("yaw {:?} mirror {:?}", p.yaw, p.mirror);
            let e = tally.entry(key).or_default();
            e[0] += 1;
            e[1] += u32::from(seen.contains(&(i + 1)));
        }
    }
    for (k, v) in &tally {
        println!("{k}: reached {} then entered the catch {}", v[0], v[1]);
    }
}
