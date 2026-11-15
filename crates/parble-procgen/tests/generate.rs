use std::collections::BTreeSet;

use parble_core::file::load;
use parble_core::{
    CellCoord, MapCatalog, ParbleFile, PieceCatalog, PieceDef, Track, clearance_cells,
    controlled_drops, is_finish, validate_track,
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

/// Placements that are not supports.
fn track_pieces(t: &Track, catalog: &MapCatalog) -> u32 {
    t.placements
        .iter()
        .filter(|pl| !catalog.get(&pl.piece).is_some_and(|d| d.structural))
        .count() as u32
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
            pieces * 10 >= p.length as usize * 9,
            "seed {seed}: {pieces} pieces"
        );
    }
}

/// The cells that must stay clear over the `L2` and `L3` channels of `t`.
fn clearance(t: &Track, cat: &MapCatalog) -> BTreeSet<CellCoord> {
    t.placements
        .iter()
        .flat_map(|pl| {
            let def = cat.get(&pl.piece).unwrap().transformed(pl.orientation());
            clearance_cells(&def.footprint, &def.ports)
                .into_iter()
                .map(move |c| pl.anchor + c)
        })
        .collect()
}

/// Every elevated cell stands on a piece, a support column or a transfer's gap, except over an
/// `L2` or `L3` channel, where the column is left out so nothing stands in its clearance.
#[test]
fn every_elevated_cell_has_something_under_it() {
    let defs = defs();
    let cat = catalog();
    let mut spans = 0;
    for seed in 0..40u64 {
        let t = generate(&defs, seed, &GenParams::default()).unwrap();
        let mut cells: BTreeSet<CellCoord> = BTreeSet::new();
        for pl in &t.placements {
            cells.extend(pl.world_cells(cat.get(&pl.piece).unwrap()));
        }
        let gaps: BTreeSet<CellCoord> = controlled_drops(&t, &cat)
            .iter()
            .map(|d| {
                let src = &t.placements[d.from];
                let def = cat.get(&src.piece).unwrap().transformed(src.orientation());
                let port = def.ports[d.from_port];
                src.anchor + port.cell + port.face.normal()
            })
            .collect();
        let clear = clearance(&t, &cat);
        for c in &cells {
            let below = CellCoord::new(c.x, c.y - 1, c.z);
            if c.y == 0 || cells.contains(&below) || gaps.contains(&below) {
                continue;
            }
            // The first thing under an unsupported cell is a channel's clearance.
            let over_channel = (0..c.y)
                .rev()
                .map(|y| CellCoord::new(c.x, y, c.z))
                .find(|w| cells.contains(w) || clear.contains(w))
                .is_some_and(|w| clear.contains(&w));
            assert!(over_channel, "seed {seed}: cell {c:?} floats");
            spans += 1;
        }
    }
    assert!(spans > 0, "no track of 40 crossed over a channel");
}

/// No support stands in the overhead clearance of an `L2` or `L3` channel.
#[test]
fn no_support_stands_over_an_l2_or_l3_channel() {
    let defs = defs();
    let cat = catalog();
    for p in presets() {
        for seed in 0..40u64 {
            let t = generate(&defs, seed, &p).unwrap();
            let clear = clearance(&t, &cat);
            for pl in &t.placements {
                let def = cat.get(&pl.piece).unwrap();
                assert!(
                    !def.structural || pl.world_cells(def).iter().all(|c| !clear.contains(c)),
                    "seed {seed} {p:?}: a support at {:?}",
                    pl.anchor
                );
            }
        }
    }
}

/// The supports of `t` as `(foot cell, height)`, sorted.
fn supports(t: &Track, cat: &MapCatalog) -> Vec<(CellCoord, i32)> {
    let mut out: Vec<(CellCoord, i32)> = t
        .placements
        .iter()
        .filter_map(|pl| {
            let def = cat.get(&pl.piece).unwrap();
            let h = def.footprint.len() as i32;
            def.structural.then(|| {
                (
                    CellCoord::new(pl.anchor.x, pl.anchor.y - h + 1, pl.anchor.z),
                    h,
                )
            })
        })
        .collect();
    out.sort();
    out
}

/// Tall pillars change the supports' piece count and nothing else: with only the one-cell pillar
/// in the catalog the same seed gives the same track pieces and supports in the same cells.
#[test]
fn tall_pillars_fill_the_cells_one_cell_pillars_would() {
    let defs = defs();
    let cat = catalog();
    let short: Vec<PieceDef> = defs
        .iter()
        .filter(|d| !d.structural || d.footprint.len() == 1)
        .cloned()
        .collect();
    let cells = |t: &Track| -> BTreeSet<CellCoord> {
        supports(t, &cat)
            .iter()
            .flat_map(|&(foot, h)| (0..h).map(move |k| CellCoord::new(foot.x, foot.y + k, foot.z)))
            .collect()
    };
    let (mut tall_count, mut short_count) = (0, 0);
    for p in presets() {
        for seed in 0..10u64 {
            let tall = generate(&defs, seed, &p).unwrap();
            let single = generate(&short, seed, &p).unwrap();
            let n = track_pieces(&tall, &cat) as usize;
            assert_eq!(n, track_pieces(&single, &cat) as usize);
            assert_eq!(tall.placements[..n], single.placements[..n], "seed {seed}");
            assert_eq!(cells(&tall), cells(&single), "seed {seed} {p:?}");
            assert_eq!(single.placements.len() - n, cells(&single).len());
            tall_count += tall.placements.len() - n;
            short_count += single.placements.len() - n;
        }
    }
    assert!(
        2 * tall_count < short_count,
        "{tall_count} supports against {short_count} one-cell pillars"
    );
}

/// Each support column is stacked from its foot with the tallest pillar that fits what is left:
/// eights, then at most one four, one two and one single.
#[test]
fn a_support_column_is_stacked_from_the_tallest_pillar_that_fits() {
    let defs = defs();
    let cat = catalog();
    let p = GenParams {
        length: 120,
        height: 24,
        footprint: 10,
        ..GenParams::default()
    };
    let mut tallest = 0;
    for seed in 0..10u64 {
        let t = generate(&defs, seed, &p).unwrap();
        let s = supports(&t, &cat);
        let mut i = 0;
        while i < s.len() {
            // One column: supports in one cell column, each standing on the one before.
            let mut j = i + 1;
            while j < s.len()
                && s[j].0.x == s[i].0.x
                && s[j].0.z == s[i].0.z
                && s[j].0.y == s[j - 1].0.y + s[j - 1].1
            {
                j += 1;
            }
            let heights: Vec<i32> = s[i..j].iter().map(|&(_, h)| h).collect();
            let mut left: i32 = heights.iter().sum();
            let mut want = Vec::new();
            for h in [8, 4, 2, 1] {
                while left >= h {
                    want.push(h);
                    left -= h;
                }
            }
            assert_eq!(heights, want, "seed {seed}: column at {:?}", s[i].0);
            tallest = tallest.max(heights[0]);
            i = j;
        }
    }
    assert_eq!(tallest, 8, "no column of 10 tracks reached eight layers");
}

/// A 500-piece track stays under 2,000 placements (procgen review 2026-11-15, F9: about 7,000
/// with one-cell pillars).
#[test]
fn a_500_piece_track_stays_under_2000_placements() {
    let defs = defs();
    let cat = catalog();
    let p = GenParams {
        length: 500,
        height: 48,
        footprint: 14,
        ..GenParams::default()
    };
    for seed in 0..3u64 {
        let t = generate(&defs, seed, &p).unwrap();
        assert!(track_pieces(&t, &cat) >= 450, "seed {seed}");
        assert!(
            t.placements.len() < 2_000,
            "seed {seed}: {} placements",
            t.placements.len()
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
                track_pieces * 10 >= length * 9 && track_pieces <= 2 * length + 16,
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
    let catalog = catalog();
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
        let mut track = Vec::new();
        for seed in 0..seeds {
            match generate(&defs, seed, &p) {
                Ok(t) => {
                    ok += 1;
                    pieces.push(t.placements.len());
                    track.push(track_pieces(&t, &catalog));
                }
                Err(e) => println!("  {case} seed {seed}: {e}"),
            }
        }
        println!(
            "{case}: {ok}/{seeds} in {:.1} s, placements {pieces:?}",
            start.elapsed().as_secs_f64()
        );
        if let (Some(min), Some(max)) = (track.iter().min(), track.iter().max()) {
            println!(
                "  track pieces min {min} mean {:.1} max {max} of length {}",
                track.iter().sum::<u32>() as f64 / track.len() as f64,
                p.length
            );
        }
    }
}
