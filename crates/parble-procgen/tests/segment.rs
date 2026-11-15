//! The segment entry point: a segment is a pure function of its inputs, segments chain into a
//! valid track, and a goal no walk can reach is refused with a clear error.

use std::collections::BTreeSet;

use parble_core::file::load;
use parble_core::{
    CellCoord, MapCatalog, PieceCatalog, PieceDef, PortClass, PortFace, PortLevel, Track,
    validate_track,
};
use parble_procgen::{
    GENERATOR_VERSION, GenError, GenParams, Goal, OpenPort, Segment, Space, chain_segment,
    generate, generate_chain, generate_segment, generated_file, segment_plan, start_segment,
    supports,
};

fn defs() -> Vec<PieceDef> {
    parble_kit::builtin()
        .iter()
        .map(|p| p.def.clone())
        .collect()
}

fn catalog() -> MapCatalog {
    defs().into_iter().collect()
}

fn params(length: u32, height: u32, footprint: u32) -> GenParams {
    GenParams {
        length,
        height,
        footprint,
        ..GenParams::default()
    }
}

/// A chain of `p`'s plan, each segment on the first of its seeds that generates: the seeds are
/// `base * 1000 + segment * 100 + attempt`.
fn chain(defs: &[PieceDef], p: &GenParams, base: u64) -> (Vec<Segment>, Vec<u64>) {
    let mut segments: Vec<Segment> = Vec::new();
    let mut seeds = Vec::new();
    let plan = segment_plan(p);
    for i in 0..plan.len() {
        let (seed, segment) = (0..8u64)
            .find_map(|a| {
                let seed = base * 1000 + i as u64 * 100 + a;
                chain_segment(defs, seed, p, &plan, &segments)
                    .ok()
                    .map(|s| (seed, s))
            })
            .unwrap_or_else(|| panic!("segment {i} of {p:?} base {base} did not generate"));
        seeds.push(seed);
        segments.push(segment);
    }
    (segments, seeds)
}

fn track_pieces(t: &Track, cat: &MapCatalog) -> usize {
    t.placements
        .iter()
        .filter(|pl| !cat.get(&pl.piece).is_some_and(|d| d.structural))
        .count()
}

#[test]
fn a_segment_is_byte_identical_per_seed() {
    let defs = defs();
    let p = params(30, 12, 10);
    let goal = Goal::OpenEnd { layer: 8 };
    for seed in [1u64, 7, 99] {
        let a = start_segment(&defs, seed, &p, goal).unwrap();
        let b = start_segment(&defs, seed, &p, goal).unwrap();
        assert_eq!(a, b);
        assert_eq!(format!("{a:?}").into_bytes(), format!("{b:?}").into_bytes());

        let start = a.end.clone().expect("an open end");
        let next = |s: u64| generate_segment(&defs, s, &p, &start, &a.space, Goal::Finish);
        let c = next(seed).unwrap();
        let d = next(seed).unwrap();
        assert_eq!(c, d);
        assert_eq!(format!("{c:?}").into_bytes(), format!("{d:?}").into_bytes());
        // The catalog's order does not matter either.
        let mut reversed = defs.clone();
        reversed.reverse();
        assert_eq!(
            generate_segment(&reversed, seed, &p, &start, &a.space, Goal::Finish).unwrap(),
            c
        );
    }
    let distinct: BTreeSet<String> = (0..10u64)
        .map(|s| format!("{:?}", start_segment(&defs, s, &p, goal).unwrap().pieces))
        .collect();
    assert!(distinct.len() >= 9, "{} distinct of 10", distinct.len());
}

/// The open end is an `L3` channel exit on the goal layer facing two free cells, the segment
/// keeps to the layers between its start and its goal, and it has nine tenths of its target
/// length.
#[test]
fn an_open_end_is_an_l3_exit_on_the_goal_layer() {
    let defs = defs();
    let cat = catalog();
    let p = params(40, 16, 10);
    for seed in 0..10u64 {
        let s = start_segment(&defs, seed, &p, Goal::OpenEnd { layer: 11 }).unwrap();
        let end = s.end.as_ref().expect("an open end");
        assert_eq!(end.cell.y, 11);
        assert_eq!(end.level, Some(PortLevel::L3));
        assert_eq!(end.class, PortClass::Channel);
        assert!(end.face.is_side());
        assert!(end.v2 > 0);
        assert!(!s.space.occupied.contains(&end.target()));
        assert!(!s.space.clear.contains(&end.target()));
        let ahead = end.target() + end.face.normal();
        assert!(!s.space.occupied.contains(&ahead) && ahead.x.abs() <= 10 && ahead.z.abs() <= 10);
        assert!(s.pieces.len() >= 36, "{} pieces", s.pieces.len());
        for pl in &s.pieces {
            for c in pl.world_cells(cat.get(&pl.piece).unwrap()) {
                assert!((11..=16).contains(&c.y), "seed {seed}: {c:?}");
                assert!(c.x.abs() <= 10 && c.z.abs() <= 10);
                assert!(s.space.occupied.contains(&c));
            }
        }
    }
}

/// A segment from an open port keeps out of the cells already in use and adds its own.
#[test]
fn a_segment_keeps_out_of_the_space_it_is_given() {
    let defs = defs();
    let cat = catalog();
    let p = params(30, 12, 10);
    for seed in 0..10u64 {
        let first = start_segment(&defs, seed, &p, Goal::OpenEnd { layer: 8 }).unwrap();
        let start = first.end.clone().unwrap();
        let second = generate_segment(
            &defs,
            seed + 50,
            &p,
            &start,
            &first.space,
            Goal::OpenEnd { layer: 4 },
        )
        .unwrap();
        let mut own = BTreeSet::new();
        for pl in &second.pieces {
            for c in pl.world_cells(cat.get(&pl.piece).unwrap()) {
                assert!(!first.space.occupied.contains(&c), "seed {seed}: {c:?}");
                assert!(!first.space.clear.contains(&c), "seed {seed}: {c:?}");
                assert!(!first.space.reserved.contains(&c), "seed {seed}: {c:?}");
                assert!((4..=8).contains(&c.y));
                own.insert(c);
            }
        }
        let all: BTreeSet<CellCoord> = first.space.occupied.union(&own).copied().collect();
        assert_eq!(second.space.occupied, all);
        assert!(own.contains(&start.target()));
        assert_eq!(second.end.as_ref().unwrap().cell.y, 4);
    }
}

/// Chained segments make a track the validator passes with no issues, the track regenerates
/// byte for byte from its segment seeds, and its supports are the chain's own.
#[test]
fn segments_chain_into_a_track_that_validates_with_zero_issues() {
    let defs = defs();
    let cat = catalog();
    for (p, segments, bases) in [
        (params(80, 12, 14), 2, 0..6u64),
        (params(160, 16, 12), 3, 0..4),
        (params(500, 48, 14), 8, 0..3),
    ] {
        assert_eq!(segment_plan(&p).len(), segments);
        for base in bases {
            let (chain, seeds) = chain(&defs, &p, base);
            let t = generate_chain(&defs, base, &p, &seeds)
                .unwrap_or_else(|e| panic!("{p:?} base {base}: {e}"));
            let issues = validate_track(&t, &cat);
            assert!(issues.is_empty(), "{p:?} base {base}: {issues:?}");
            assert_eq!(t.seed, base);
            assert_eq!(t.hopper.placement, Some(0));
            assert_eq!(t.hopper.queue.len(), p.marbles as usize);
            let pieces = track_pieces(&t, &cat);
            assert_eq!(pieces, chain.iter().map(|s| s.pieces.len()).sum::<usize>());
            assert!(
                pieces as u32 * 10 >= p.length * 9,
                "{p:?} base {base}: {pieces} pieces"
            );
            let last = chain.last().unwrap();
            assert!(last.end.is_none());
            let all: Vec<_> = t.placements[..pieces].to_vec();
            assert_eq!(
                supports(&defs, &all, &last.space).unwrap(),
                t.placements[pieces..]
            );
            // Byte identical again from the same seeds.
            let again = generate_chain(&defs, base, &p, &seeds).unwrap();
            assert_eq!(
                generated_file(again, "generated", &seeds).to_json(),
                generated_file(t, "generated", &seeds).to_json()
            );
        }
    }
}

/// A track short enough for one segment is the whole-track generator's own track.
#[test]
fn a_one_segment_chain_is_the_whole_track() {
    let defs = defs();
    let p = GenParams::default();
    assert_eq!(segment_plan(&p).len(), 1);
    for seed in [0u64, 5, 31] {
        assert_eq!(
            generate_chain(&defs, seed, &p, &[seed]).unwrap(),
            generate(&defs, seed, &p).unwrap()
        );
    }
}

/// Every plan covers the length and comes down to the finish, each segment within what its
/// layers carry.
#[test]
fn a_plan_shares_out_the_length_and_the_layers() {
    for (length, height) in [
        (40, 6),
        (72, 6),
        (130, 11),
        (500, 48),
        (760, 64),
        (12, 1),
        (700, 60),
    ] {
        let p = params(length, height, 14);
        let plan = segment_plan(&p);
        assert_eq!(plan.iter().map(|s| s.length).sum::<u32>(), length);
        assert_eq!(plan.last().unwrap().goal, Goal::Finish);
        let mut top = height as i32;
        for s in &plan[..plan.len() - 1] {
            let Goal::OpenEnd { layer } = s.goal else {
                panic!("{length}:{height}: an inner segment ends at {:?}", s.goal);
            };
            assert!(layer < top && layer > 0, "{length}:{height}: {plan:?}");
            assert!(s.length <= 12 * (top - layer) as u32);
            top = layer;
        }
        assert!(plan.last().unwrap().length <= 12 * top as u32);
    }
}

#[test]
fn an_unreachable_goal_gives_a_clear_error() {
    let defs = defs();
    let p = params(20, 12, 10);
    let first = start_segment(&defs, 1, &p, Goal::OpenEnd { layer: 9 }).unwrap();
    let start = first.end.clone().unwrap();

    // A goal above the start port: a track only descends.
    let above = generate_segment(
        &defs,
        1,
        &p,
        &start,
        &first.space,
        Goal::OpenEnd { layer: 10 },
    )
    .unwrap_err();
    assert_eq!(
        above,
        GenError::GoalAbove {
            start_layer: 9,
            goal_layer: 10
        }
    );
    assert!(above.to_string().contains("above the start port"));
    assert_eq!(
        start_segment(&defs, 1, &p, Goal::OpenEnd { layer: 13 }).unwrap_err(),
        GenError::GoalAbove {
            start_layer: 12,
            goal_layer: 13
        }
    );

    // The cell the port faces is taken: no seed finds a way.
    let mut blocked = first.space.clone();
    blocked.occupied.insert(start.target());
    for seed in [1u64, 2] {
        let e = generate_segment(&defs, seed, &p, &start, &blocked, Goal::Finish).unwrap_err();
        assert_eq!(e, GenError::NoSegmentRoute);
        assert!(e.to_string().contains("no route from the start port"));
    }

    // A port walled in on every side but the one it faces: the first piece has nowhere to go.
    let mut walled = Space::default();
    let port = OpenPort::channel(CellCoord::new(0, 5, 0), PortFace::PosX, PortLevel::L3, 400);
    for x in 1..=3 {
        for z in -2..=2 {
            for y in 3..=5 {
                if (x, y, z) != (1, 5, 0) {
                    walled.occupied.insert(CellCoord::new(x, y, z));
                }
            }
        }
    }
    assert_eq!(
        generate_segment(&defs, 1, &p, &port, &walled, Goal::Finish).unwrap_err(),
        GenError::NoSegmentRoute
    );

    // A socket is not a port a marble leaves through.
    let socket = OpenPort {
        class: PortClass::Socket,
        ..port.clone()
    };
    assert_eq!(
        generate_segment(&defs, 1, &p, &socket, &Space::default(), Goal::Finish).unwrap_err(),
        GenError::NoSegmentRoute
    );

    // The chain's seeds must be one a segment.
    assert_eq!(
        generate_chain(&defs, 1, &params(160, 16, 12), &[1, 2]).unwrap_err(),
        GenError::BadParams("segment seeds")
    );
}

/// A segment from a port given by hand (fill-between and infinite descent start like this):
/// from an `L3` exit in open space down to a finish, and down to an open end.
#[test]
fn a_segment_starts_from_any_open_channel_port() {
    let defs = defs();
    let cat = catalog();
    let p = params(24, 6, 8);
    let port = OpenPort::channel(CellCoord::new(-3, 6, 2), PortFace::PosX, PortLevel::L3, 300);
    for seed in 0..10u64 {
        let s = generate_segment(&defs, seed, &p, &port, &Space::default(), Goal::Finish)
            .unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        assert!(s.end.is_none());
        let first = &s.pieces[0];
        assert!(
            first
                .world_cells(cat.get(&first.piece).unwrap())
                .contains(&port.target())
        );
        let last = s.pieces.last().unwrap();
        assert!(parble_core::is_finish(&last.piece));
        assert!(
            last.world_cells(cat.get(&last.piece).unwrap())
                .iter()
                .all(|c| c.y == 0)
        );
        let open = generate_segment(
            &defs,
            seed,
            &p,
            &port,
            &Space::default(),
            Goal::OpenEnd { layer: 2 },
        )
        .unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        assert_eq!(open.end.unwrap().cell.y, 2);
    }
}

/// A generated file records the generator version (and the segment seeds), loads and saves byte
/// for byte, and a file without the stamp is unchanged.
#[test]
fn a_generated_file_records_the_generator_version() {
    let defs = defs();
    let t = generate(&defs, 3, &GenParams::default()).unwrap();
    let whole = generated_file(t.clone(), "generated", &[]);
    let text = whole.to_json();
    assert!(text.contains(&format!("\"version\": {GENERATOR_VERSION}")));
    assert!(!text.contains("\"segments\""));
    let back = load(&text).unwrap();
    assert_eq!(back, whole);
    assert_eq!(back.to_json(), text);
    assert_eq!(back.track, t);

    let segmented = generated_file(t.clone(), "generated", &[5, 9]);
    let back = load(&segmented.to_json()).unwrap();
    assert_eq!(back.metadata.generator.unwrap().segments, vec![5, 9]);

    let plain = parble_core::ParbleFile::new(t, "generated").to_json();
    assert!(!plain.contains("generator"));
    assert_eq!(load(&plain).unwrap().metadata.generator, None);
}
