//! The stepper lift track's committed trajectory hash ([`parble_sim::lift_run::track`]), one
//! golden for every platform (the wasm parity check, `make determinism-wasm`, compares the same
//! file).
//!
//! Regenerate only after an intended physics or track change:
//!
//! ```sh
//! PARBLE_UPDATE_GOLDEN=1 cargo test -p parble-sim --test golden_lift_track
//! ```

use std::path::PathBuf;

use parble_sim::lift_run::lift_track_hash_hex;

#[test]
fn lift_track_matches_golden() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/lift_track.hash");
    let actual = lift_track_hash_hex();
    if std::env::var("PARBLE_UPDATE_GOLDEN").as_deref() == Ok("1") {
        std::fs::write(&path, format!("{actual}\n")).expect("write lift track golden");
        eprintln!("wrote {} = {actual}", path.display());
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing {} ({e}); generate it with PARBLE_UPDATE_GOLDEN=1",
            path.display()
        )
    });
    assert_eq!(actual, expected.trim(), "lift track run hash changed");
}
