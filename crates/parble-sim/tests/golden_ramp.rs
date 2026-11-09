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

use parble_sim::golden::{
    RAMP_10S_TICKS, mechanism_demo, ramp_10s_hash_hex, ramp_10s_variants, rope_arch_pass,
};

const UPDATE_ENV: &str = "PARBLE_UPDATE_GOLDEN";

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/ramp_10s.hash")
}

/// P5-T1: every marble type × size on the same ramp. Additive: the P0 golden above is untouched.
/// Regenerate with `PARBLE_UPDATE_GOLDEN=1` like the ramp golden.
#[test]
fn ramp_10s_variants_match_golden() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/ramp_10s_variants.hash");
    let actual = ramp_10s_variants();
    if std::env::var(UPDATE_ENV).as_deref() == Ok("1") {
        std::fs::write(&path, &actual).expect("write variants golden");
        eprintln!("wrote {}", path.display());
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing {} ({e}); generate it with {UPDATE_ENV}=1",
            path.display()
        )
    });
    assert_eq!(actual, expected, "type × size ramp hashes changed");
}

/// P2-T1: the mechanism framework fixture run (flap, sensor, RNG stream, event stream). Additive
/// like the variants golden; the wasm parity check compares the same file.
#[test]
fn mechanism_demo_matches_golden() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/mechanism_demo_10s.hash");
    let actual = mechanism_demo();
    if std::env::var(UPDATE_ENV).as_deref() == Ok("1") {
        std::fs::write(&path, &actual).expect("write mechanism golden");
        eprintln!("wrote {}", path.display());
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing {} ({e}); generate it with {UPDATE_ENV}=1",
            path.display()
        )
    });
    assert_eq!(actual, expected, "mechanism fixture run changed");
}

/// P2 item 18: the rope-curtain arch pass in both directions (drag and cosmetic swing). Additive
/// like the mechanism golden; the wasm parity check compares the same file.
#[test]
fn rope_arch_pass_matches_golden() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/rope_arch_pass.hash");
    let actual = rope_arch_pass();
    if std::env::var(UPDATE_ENV).as_deref() == Ok("1") {
        std::fs::write(&path, &actual).expect("write rope arch golden");
        eprintln!("wrote {}", path.display());
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing {} ({e}); generate it with {UPDATE_ENV}=1",
            path.display()
        )
    });
    assert_eq!(actual, expected, "rope arch pass changed");
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
