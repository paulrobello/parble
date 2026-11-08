//! P3-T4: editing a run's pieces in place, one changed placement at a time with only the touched
//! chunks rebuilt, leaves exactly the colliders a fresh build of the final track has.

use parble_core::{CellCoord, Mirror, PieceId, Placement, Track, Yaw4};
use parble_sim::track_run::TrackRun;
use std::collections::BTreeMap;

const IDS: [&str; 4] = [
    "blk_straight_l3",
    "blk_curve90_l3",
    "blk_slope_gentle_l3",
    "blk_end_l3",
];

struct Lcg(u64);

impl Lcg {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }
}

fn rebuild_all(run: &mut TrackRun) -> usize {
    let kit = parble_kit::builtin();
    let mut n = 0;
    while run.rebuild_next_chunk(kit).unwrap().is_some() {
        n += 1;
    }
    n
}

#[test]
fn incremental_edits_match_a_fresh_build() {
    let kit = parble_kit::builtin();
    let mut run = TrackRun::new(&Track::new(0)).unwrap();
    let mut placed: BTreeMap<CellCoord, Placement> = BTreeMap::new();
    let mut rng = Lcg(7);
    let mut rebuilt = 0;
    for step in 0..400 {
        // A 20 x 3 x 20 cell box straddling chunk borders on every axis (chunks are 8 cells).
        let cell = CellCoord::new(
            rng.next(20) as i32 - 6,
            rng.next(3) as i32 + 6,
            rng.next(20) as i32 - 6,
        );
        if let Some(_old) = placed.remove(&cell) {
            run.remove_piece(cell).unwrap();
        } else {
            let p = Placement::new(
                PieceId::new(IDS[rng.next(4) as usize]).unwrap(),
                cell,
                Yaw4::new(rng.next(4) as u8),
                Mirror::Off,
            );
            run.place_piece(kit, p.clone()).unwrap();
            placed.insert(cell, p);
        }
        // Sometimes several edits share one rebuild, as a frame with a group edit does.
        if step % 3 == 0 {
            rebuilt += rebuild_all(&mut run);
        }
    }
    rebuilt += rebuild_all(&mut run);
    assert!(rebuilt > 0);
    let mut track = Track::new(0);
    track.placements = placed.into_values().collect();
    let fresh = TrackRun::new(&track).unwrap();
    let (a, b) = (
        run.chunk_collider_geometry(),
        fresh.chunk_collider_geometry(),
    );
    assert_eq!(
        a.iter().map(|g| g.chunk).collect::<Vec<_>>(),
        b.iter().map(|g| g.chunk).collect::<Vec<_>>()
    );
    assert_eq!(a, b);
    assert_eq!(run.chunk_colliders().dirty_chunks().count(), 0);
}
