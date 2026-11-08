//! P3-T4: editing a run's pieces in place, one changed placement at a time with only the touched
//! chunks rebuilt, leaves exactly the colliders a fresh build of the final track has: the same
//! trimeshes, active hooks and contact-hook tags (stitched edges, channel bends, channel slopes,
//! rail arcs).

use parble_core::{CellCoord, Mirror, PieceId, Placement, Track, Yaw4};
use parble_sim::track_run::TrackRun;
use std::collections::BTreeMap;

const IDS: [&str; 6] = [
    "blk_straight_l3",
    "blk_curve90_l3",
    "blk_slope_gentle_l3",
    "blk_end_l3",
    "rail_straight_l2",
    "rail_curve90_r90_l2",
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

/// Up to `steps` stages of the staged rebuild, as one editor frame runs.
fn rebuild_steps(run: &mut TrackRun, steps: usize) -> usize {
    let kit = parble_kit::builtin();
    let mut installed = 0;
    for _ in 0..steps {
        match run.rebuild_step_chunk(kit).unwrap() {
            Some((_, done)) => installed += usize::from(done),
            None => break,
        }
    }
    installed
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
            let id = IDS[rng.next(IDS.len() as u64) as usize];
            let chiral = kit.piece(&PieceId::new(id).unwrap()).unwrap().def.chiral;
            // Chiral pieces are mirrored half the time.
            let mirror = if chiral && rng.next(2) == 1 {
                Mirror::On
            } else {
                Mirror::Off
            };
            let p = Placement::new(
                PieceId::new(id).unwrap(),
                cell,
                Yaw4::new(rng.next(4) as u8),
                mirror,
            );
            // A multi-cell rail may not fit among the pieces already placed.
            if run.place_piece(kit, p.clone()).is_ok() {
                placed.insert(cell, p);
            }
        }
        // Sometimes several edits share one rebuild, as a frame with a group edit does.
        if step % 3 == 0 {
            rebuilt += rebuild_all(&mut run);
        } else if step % 3 == 1 {
            // A staged rebuild part way through when the next edit lands.
            rebuilt += rebuild_steps(&mut run, 5);
        }
    }
    rebuilt += rebuild_steps(&mut run, usize::MAX);
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
    // Every tag kind is exercised, so equal tags are not equal emptiness.
    assert!(a.iter().any(|g| g.stitched.iter().any(|&m| m != 0)));
    assert!(a.iter().any(|g| g.bend_of.iter().any(|&t| t != 0)));
    assert!(a.iter().any(|g| g.slope_of.iter().any(|k| !k.is_empty())));
    assert!(a.iter().any(|g| g.arc_of.iter().any(|&t| t != 0)));
    assert!(a.iter().any(|g| g.active_hooks != 0));
    for (x, y) in a.iter().zip(&b) {
        let at = format!("chunk {:?}", x.chunk);
        assert_eq!(x.vertices, y.vertices, "{at} vertices");
        assert_eq!(x.triangles, y.triangles, "{at} triangles");
        assert_eq!(x.active_hooks, y.active_hooks, "{at} active hooks");
        assert_eq!(x.stitched, y.stitched, "{at} stitched edges");
        assert_eq!(
            (&x.bends, &x.bend_of),
            (&y.bends, &y.bend_of),
            "{at} bend tags"
        );
        assert_eq!(
            (&x.slopes, &x.slope_of),
            (&y.slopes, &y.slope_of),
            "{at} slope tags"
        );
        assert_eq!(
            (&x.arcs, &x.arc_of),
            (&y.arcs, &y.arc_of),
            "{at} rail arc tags"
        );
    }
    assert_eq!(a, b);
    assert_eq!(run.chunk_colliders().dirty_chunks().count(), 0);
}
