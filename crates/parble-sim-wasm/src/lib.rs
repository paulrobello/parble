//! parble-sim-wasm: wasm-bindgen exports of the parble-sim golden runs, used by the wasm
//! determinism parity check (`make determinism-wasm`, `scripts/determinism/`).

use wasm_bindgen::prelude::wasm_bindgen;

/// Run the 10 s ramp golden scene and return its trajectory hash as 16 lowercase hex digits,
/// the same format as `crates/parble-sim/tests/golden/ramp_10s.hash` (without the newline).
#[wasm_bindgen]
pub fn run_golden() -> String {
    parble_sim::golden::ramp_10s_hash_hex()
}

/// Run every marble type × size on the ramp (P5-T1) and return the lines of
/// `crates/parble-sim/tests/golden/ramp_10s_variants.hash`.
#[wasm_bindgen]
pub fn run_variants() -> String {
    parble_sim::golden::ramp_10s_variants()
}

/// Run the P2-T1 mechanism framework fixture and return the lines of
/// `crates/parble-sim/tests/golden/mechanism_demo_10s.hash`.
#[wasm_bindgen]
pub fn run_mechanism_demo() -> String {
    parble_sim::golden::mechanism_demo()
}

/// Run the playroom kit run (P1-T3 chunk colliders) and return its hash, the same format as
/// `crates/parble-sim/tests/golden/playroom_4s.hash` (without the newline).
#[wasm_bindgen]
pub fn run_playroom() -> String {
    parble_sim::playroom_run::playroom_4s_hash_hex()
}

/// Run the P2-T7 all-pieces track and return its hash, the same format as
/// `crates/parble-sim/tests/golden/allpieces_30s.hash` (without the newline).
#[wasm_bindgen]
pub fn run_allpieces() -> String {
    parble_sim::allpieces::allpieces_hash_hex()
}

#[cfg(test)]
mod tests {
    #[test]
    fn run_golden_matches_native_api() {
        assert_eq!(super::run_golden(), parble_sim::golden::ramp_10s_hash_hex());
        assert_eq!(
            super::run_variants(),
            parble_sim::golden::ramp_10s_variants()
        );
        assert_eq!(
            super::run_mechanism_demo(),
            parble_sim::golden::mechanism_demo()
        );
        assert_eq!(
            super::run_playroom(),
            parble_sim::playroom_run::playroom_4s_hash_hex()
        );
        assert_eq!(
            super::run_allpieces(),
            parble_sim::allpieces::allpieces_hash_hex()
        );
    }
}
