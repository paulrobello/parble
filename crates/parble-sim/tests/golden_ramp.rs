//! Native golden determinism test for the P0 ramp scene.
//!
//! Regenerate the golden file only after an intended physics change (and bump
//! `parble_core::PHYSICS_VERSION`):
//!
//! ```sh
//! PARBLE_UPDATE_GOLDEN=1 cargo test -p parble-sim --test golden_ramp
//! ```
//!
//! Without the variable the test only compares and never writes.

use std::path::PathBuf;

use parble_sim::golden::{RAMP_10S_TICKS, ramp_10s_hash_hex};

const UPDATE_ENV: &str = "PARBLE_UPDATE_GOLDEN";

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/ramp_10s.hash")
}

#[test]
fn ramp_10s_matches_golden() {
    assert_eq!(RAMP_10S_TICKS, 2_400);
    let actual = ramp_10s_hash_hex();
    let path = golden_path();
    if std::env::var(UPDATE_ENV).as_deref() == Ok("1") {
        std::fs::write(&path, format!("{actual}\n")).expect("write golden file");
        eprintln!("wrote {} = {actual}", path.display());
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing golden file {} ({e}); generate it with {UPDATE_ENV}=1 cargo test -p parble-sim --test golden_ramp",
            path.display()
        )
    });
    assert_eq!(
        actual,
        expected.trim(),
        "ramp_10s trajectory hash changed; if the physics change is intended, bump PHYSICS_VERSION and rerun with {UPDATE_ENV}=1"
    );
}
