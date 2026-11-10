//! The nudge track's committed trajectory hash ([`parble_sim::nudge::track`]): marbles kept moving
//! through a run of rope arches by a nudge zone. One golden for every platform (the wasm parity
//! check, `make determinism-wasm`, compares the same file).
//!
//! Regenerate only after an intended physics or track change:
//!
//! ```sh
//! PARBLE_UPDATE_GOLDEN=1 cargo test -p parble-sim --test golden_nudge_track
//! ```

use std::path::PathBuf;

use parble_sim::nudge::nudge_track_hash_hex;

#[test]
fn nudge_track_matches_golden() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/nudge_track.hash");
    let actual = nudge_track_hash_hex();
    if std::env::var("PARBLE_UPDATE_GOLDEN").as_deref() == Ok("1") {
        std::fs::write(&path, format!("{actual}\n")).expect("write nudge track golden");
        eprintln!("wrote {} = {actual}", path.display());
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing {} ({e}); generate it with PARBLE_UPDATE_GOLDEN=1",
            path.display()
        )
    });
    assert_eq!(actual, expected.trim(), "nudge track run hash changed");
}

/// The zone is what moves the run: the same track without it hashes differently.
#[test]
fn the_zone_changes_the_run() {
    use parble_sim::nudge::{TRACK_TICKS, flat_track};
    let mut bare = parble_sim::track_run::TrackRun::new(&flat_track()).unwrap();
    bare.tick(TRACK_TICKS);
    assert_ne!(
        parble_sim::hash::format_hash(bare.trajectory_hash()),
        nudge_track_hash_hex()
    );
}
