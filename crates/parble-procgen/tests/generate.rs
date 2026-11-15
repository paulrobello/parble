use std::collections::BTreeSet;

use parble_core::file::load;
use parble_core::{
    CellCoord, MapCatalog, ParbleFile, PieceCatalog, PieceDef, Track, controlled_drops, is_finish,
    validate_track,
};
use parble_procgen::{Families, GenError, GenParams, generate};

fn defs() -> Vec<PieceDef> {
    parble_kit::builtin()
        .iter()
        .map(|p| p.def.clone())
        .collect()
}

fn catalog() -> MapCatalog {
    defs().into_iter().collect()
}

fn presets() -> Vec<GenParams> {
    let d = GenParams::default();
    vec![
        d,
        GenParams {
            height: 12,
            length: 80,
            footprint: 14,
            ..d
        },
        GenParams {
            height: 3,
            length: 14,
            footprint: 5,
            special_per_mille: 0,
            ..d
        },
        GenParams {
            difficulty: 100,
            special_per_mille: 600,
            ..d
        },
        GenParams {
            families: Families {
                blk: true,
                rail: false,
                mech: false,
            },
            ..d
        },
        GenParams {
            families: Families {
                blk: true,
                rail: true,
                mech: false,
            },
            ..d
        },
    ]
}

fn bytes(t: &Track) -> String {
    ParbleFile::new(t.clone(), "generated").to_json()
}

#[test]
fn same_seed_and_params_give_byte_identical_files() {
    let defs = defs();
    for p in presets() {
        for seed in [1u64, 7, 99] {
            let a = bytes(&generate(&defs, seed, &p).unwrap());
            let b = bytes(&generate(&defs, seed, &p).unwrap());
            assert_eq!(a, b, "seed {seed} {p:?}");
        }
    }
}

#[test]
fn the_catalog_order_does_not_change_the_track() {
    let defs = defs();
    let mut reversed = defs.clone();
    reversed.reverse();
    let p = GenParams::default();
    assert_eq!(
        bytes(&generate(&defs, 5, &p).unwrap()),
        bytes(&generate(&reversed, 5, &p).unwrap())
    );
}

#[test]
fn a_generated_file_loads_and_round_trips() {
    let t = generate(&defs(), 3, &GenParams::default()).unwrap();
    let text = bytes(&t);
    let file = load(&text).expect("a generated file loads");
    assert_eq!(file.to_json(), text);
    assert_eq!(file.track, t);
}

#[test]
fn different_seeds_give_different_tracks() {
    let defs = defs();
    let p = GenParams::default();
    let distinct: BTreeSet<String> = (0..20u64)
        .map(|s| bytes(&generate(&defs, s, &p).unwrap()))
        .collect();
    assert!(
        distinct.len() >= 18,
        "only {} distinct of 20",
        distinct.len()
    );
}

#[test]
fn every_preset_validates_with_no_issues_across_seeds() {
    let defs = defs();
    let cat = catalog();
    for p in presets() {
        for seed in 0..40u64 {
            let t = generate(&defs, seed, &p).unwrap_or_else(|e| panic!("seed {seed} {p:?}: {e}"));
            let issues = validate_track(&t, &cat);
            assert!(issues.is_empty(), "seed {seed} {p:?}: {issues:?}");
        }
    }
}

#[test]
fn a_track_starts_at_a_hopper_and_ends_on_layer_zero_finishes() {
    let defs = defs();
    let cat = catalog();
    for seed in 0..20u64 {
        let p = GenParams::default();
        let t = generate(&defs, seed, &p).unwrap();
        assert_eq!(t.seed, seed);
        assert_eq!(t.hopper.placement, Some(0));
        assert_eq!(t.hopper.queue.len(), p.marbles as usize);
        assert_eq!(t.placements[0].piece.as_str(), "mech_hopper");
        assert_eq!(t.placements[0].anchor.y, p.height as i32);
        let finishes: Vec<_> = t
            .placements
            .iter()
            .filter(|pl| is_finish(&pl.piece))
            .collect();
        assert!(!finishes.is_empty());
        assert!(finishes.iter().all(|f| f.anchor.y == 0));
        let pieces = t
            .placements
            .iter()
            .filter(|pl| !cat.get(&pl.piece).unwrap().structural)
            .count();
        assert!(
            pieces * 4 >= p.length as usize * 3,
            "seed {seed}: {pieces} pieces"
        );
    }
}

#[test]
fn every_elevated_cell_has_something_under_it() {
    let defs = defs();
    let cat = catalog();
    let t = generate(&defs, 11, &GenParams::default()).unwrap();
    let mut cells: BTreeSet<CellCoord> = BTreeSet::new();
    for pl in &t.placements {
        cells.extend(pl.world_cells(cat.get(&pl.piece).unwrap()));
    }
    let drops = controlled_drops(&t, &cat);
    assert!(!drops.is_empty(), "the hopper exit is a controlled drop");
    let gaps: BTreeSet<CellCoord> = drops
        .iter()
        .map(|d| {
            let src = &t.placements[d.from];
            let def = cat.get(&src.piece).unwrap().transformed(src.orientation());
            let port = def.ports[d.from_port];
            src.anchor + port.cell + port.face.normal()
        })
        .collect();
    for c in &cells {
        let below = CellCoord::new(c.x, c.y - 1, c.z);
        assert!(
            c.y == 0 || cells.contains(&below) || gaps.contains(&below),
            "cell {c:?} floats"
        );
    }
}

#[test]
fn family_and_density_filters_hold() {
    let defs = defs();
    let no_rail = GenParams {
        families: Families {
            blk: true,
            rail: false,
            mech: true,
        },
        ..GenParams::default()
    };
    let no_special = GenParams {
        special_per_mille: 0,
        ..GenParams::default()
    };
    for seed in 0..20u64 {
        let t = generate(&defs, seed, &no_rail).unwrap();
        assert!(t.placements.iter().all(|p| p.piece.family() != "rail"));
        let t = generate(&defs, seed, &no_special).unwrap();
        assert!(t.placements.iter().all(|p| {
            let s = p.piece.as_str();
            !(s.contains("gate") || s.contains("checkpoint") || s.contains("arch"))
        }));
    }
}

#[test]
fn families_that_cannot_reach_a_finish_report_no_route() {
    let defs = defs();
    let rails = GenParams {
        families: Families {
            blk: false,
            rail: true,
            mech: true,
        },
        ..GenParams::default()
    };
    assert_eq!(generate(&defs, 0, &rails).unwrap_err(), GenError::NoRoute);
}

#[test]
fn bad_params_and_catalogs_are_refused() {
    let defs = defs();
    let d = GenParams::default();
    let bad = |p: GenParams| generate(&defs, 0, &p).unwrap_err();
    assert_eq!(
        bad(GenParams { footprint: 1, ..d }),
        GenError::BadParams("footprint")
    );
    assert_eq!(
        bad(GenParams { height: 0, ..d }),
        GenError::BadParams("height")
    );
    assert_eq!(
        bad(GenParams { length: 2, ..d }),
        GenError::BadParams("length")
    );
    assert_eq!(
        bad(GenParams {
            difficulty: 101,
            ..d
        }),
        GenError::BadParams("difficulty")
    );
    assert_eq!(
        bad(GenParams { marbles: 0, ..d }),
        GenError::BadParams("marbles")
    );
    let none: Vec<PieceDef> = defs
        .iter()
        .filter(|p| !p.id.as_str().starts_with("mech_finish"))
        .cloned()
        .collect();
    assert_eq!(
        generate(&none, 0, &d).unwrap_err(),
        GenError::MissingPiece("finish")
    );
    let no_pillar: Vec<PieceDef> = defs.iter().filter(|p| !p.structural).cloned().collect();
    assert_eq!(
        generate(&no_pillar, 0, &d).unwrap_err(),
        GenError::MissingPiece("support pillar")
    );
}

/// A length the hopper's layer cannot carry is refused at once, naming the most it can.
#[test]
fn a_length_too_long_for_the_height_is_refused_at_once() {
    let defs = defs();
    let p = GenParams {
        length: 100,
        height: 6,
        ..GenParams::default()
    };
    assert_eq!(p.max_length(), 72);
    let start = std::time::Instant::now();
    assert_eq!(
        generate(&defs, 1, &p).unwrap_err(),
        GenError::TooLong {
            length: 100,
            max_length: 72
        }
    );
    assert!(start.elapsed().as_millis() < 100, "{:?}", start.elapsed());
    assert!(generate(&defs, 1, &GenParams { length: 72, ..p }).is_ok());
}

/// Long tracks on a low hopper (more than seven pieces a layer) generate, valid and descending
/// to a layer-0 finish, at about the target length.
#[test]
fn long_tracks_on_a_low_hopper_generate() {
    let defs = defs();
    let catalog = catalog();
    for (length, height) in [(100, 10), (160, 16)] {
        let p = GenParams {
            length,
            height,
            footprint: 12,
            ..GenParams::default()
        };
        for seed in 0..3u64 {
            let t = generate(&defs, seed, &p)
                .unwrap_or_else(|e| panic!("{length}/{height} seed {seed}: {e}"));
            assert!(validate_track(&t, &catalog).is_empty());
            let track_pieces = t
                .placements
                .iter()
                .filter(|pl| !catalog.get(&pl.piece).is_some_and(|d| d.structural))
                .count() as u32;
            assert!(
                track_pieces * 4 >= length * 3 && track_pieces <= 2 * length + 16,
                "{length}/{height} seed {seed}: {track_pieces} pieces"
            );
        }
    }
}

/// Diagnostic: how often long tracks generate, and how fast. `PROCGEN_LONG` lists
/// `length:height:footprint` cases (default `100:10:8,200:20:10,500:40:14`), `PROCGEN_SEEDS`
/// the seed count.
#[test]
#[ignore = "diagnostic: long-track generation census"]
fn long_track_census() {
    let defs = defs();
    let seeds: u64 = std::env::var("PROCGEN_SEEDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let cases =
        std::env::var("PROCGEN_LONG").unwrap_or_else(|_| "100:10:8,200:20:10,500:40:14".into());
    for case in cases.split(',') {
        let v: Vec<u32> = case.split(':').map(|x| x.parse().unwrap()).collect();
        let p = GenParams {
            length: v[0],
            height: v[1],
            footprint: v[2],
            ..GenParams::default()
        };
        let start = std::time::Instant::now();
        let mut ok = 0;
        let mut pieces = Vec::new();
        for seed in 0..seeds {
            match generate(&defs, seed, &p) {
                Ok(t) => {
                    ok += 1;
                    pieces.push(t.placements.len());
                }
                Err(e) => println!("  {case} seed {seed}: {e}"),
            }
        }
        println!(
            "{case}: {ok}/{seeds} in {:.1} s, placements {pieces:?}",
            start.elapsed().as_secs_f64()
        );
    }
}
