//! parble-sim-wasm: wasm-bindgen exports of the parble-sim golden runs, used by the wasm
//! determinism parity check (`make determinism-wasm`, `scripts/determinism/`), and the
//! generate-and-validate job of the generator Web Worker ([`generate_and_validate`], P6-T2).

mod generate;

pub use generate::{
    GenerateJob, ParamsWire, generate_and_validate, generate_and_validate_segmented,
    run_procgen_job,
};

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

/// Run the rope-curtain arch pass (P2 item 18) both ways and return the lines of
/// `crates/parble-sim/tests/golden/rope_arch_pass.hash`.
#[wasm_bindgen]
pub fn run_rope_arch() -> String {
    parble_sim::golden::rope_arch_pass()
}

/// Run the stepper lift track (hopper to finish, up both steppers) and return its hash, the same
/// format as `crates/parble-sim/tests/golden/lift_track.hash` (without the newline).
#[wasm_bindgen]
pub fn run_lift_track() -> String {
    parble_sim::lift_run::lift_track_hash_hex()
}

/// Run the editor's marble check of the all-pieces track's pinned release for every marble spec at
/// every size and return the lines of `crates/parble-sim/tests/golden/allpieces_marble_check.txt`.
#[wasm_bindgen]
pub fn run_marble_check() -> String {
    parble_sim::marble_check::allpieces_marble_check()
}

/// Run the nudge track (rope arches kept moving by a nudge zone) and return its hash, the same
/// format as `crates/parble-sim/tests/golden/nudge_track.hash` (without the newline).
#[wasm_bindgen]
pub fn run_nudge_track() -> String {
    parble_sim::nudge::nudge_track_hash_hex()
}

/// Run the nudge track with its zone in constant mode and return its hash, the same format as
/// `crates/parble-sim/tests/golden/nudge_constant_track.hash` (without the newline).
#[wasm_bindgen]
pub fn run_nudge_constant_track() -> String {
    parble_sim::nudge::constant_track_hash_hex()
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
        assert_eq!(super::run_rope_arch(), parble_sim::golden::rope_arch_pass());
        assert_eq!(
            super::run_lift_track(),
            parble_sim::lift_run::lift_track_hash_hex()
        );
        assert_eq!(
            super::run_marble_check(),
            parble_sim::marble_check::allpieces_marble_check()
        );
        assert_eq!(
            super::run_nudge_track(),
            parble_sim::nudge::nudge_track_hash_hex()
        );
        assert_eq!(
            super::run_nudge_constant_track(),
            parble_sim::nudge::constant_track_hash_hex()
        );
        assert_eq!(
            super::run_procgen_job(),
            parble_sim::procgen_job::golden_lines()
        );
    }

    #[test]
    fn params_json_defaults_every_missing_slider() {
        let p: super::ParamsWire = serde_json::from_str("{}").unwrap();
        assert_eq!(p.params(), parble_procgen::GenParams::default());
        let p: super::ParamsWire = serde_json::from_str(
            r#"{"height":3,"length":14,"specialPerMille":0,"families":{"blk":true,"rail":false,"mech":false}}"#,
        )
        .unwrap();
        let p = p.params();
        assert_eq!((p.height, p.length, p.special_per_mille), (3, 14, 0));
        assert!(!p.families.rail);
        assert!(serde_json::from_str::<super::ParamsWire>(r#"{"seed":1}"#).is_err());
    }

    #[test]
    fn a_job_refuses_seeds_javascript_cannot_hold() {
        for bad in [-1.0, 1.5, f64::NAN, 9_007_199_254_740_992.0] {
            assert!(super::GenerateJob::create(bad, "{}").is_err(), "{bad}");
        }
        assert!(super::GenerateJob::create(0.0, "{nope").is_err());
    }

    /// A zero budget still does one unit a slice, and slicing that finely gives the golden result.
    #[test]
    fn zero_budget_slices_reach_the_golden_result() {
        let golden = parble_sim::procgen_job::golden_lines();
        let line = golden.lines().nth(1).unwrap();
        let mut job = super::GenerateJob::create(
            2.0,
            r#"{"height":3,"length":14,"footprint":5,"marbles":2}"#,
        )
        .unwrap();
        let mut slices = 0u64;
        while job.run_slice(0.0).status == "running" {
            slices += 1;
        }
        assert_eq!(job.status().units, slices + 1);
        assert_eq!(job.result_line(2), line);
        let file = job.file_json("generated").unwrap();
        assert!(
            file.contains("\"seed\": 2") || file.contains("\"seed\":2"),
            "{file}"
        );
        let loaded = parble_core::file::load(&file).unwrap();
        let stamp = loaded.metadata.generator.expect("generator stamp");
        assert_eq!(stamp.version, parble_procgen::GENERATOR_VERSION);
        assert!(stamp.segments.is_empty());
    }

    /// The segmented job through the same wrapper: zero-budget slices reach the golden result,
    /// and the file records the generator version and one seed a segment.
    #[test]
    fn a_segmented_job_reaches_its_golden_result_and_records_its_seeds() {
        let golden = parble_sim::procgen_job::golden_lines();
        let line = golden.lines().nth(2).unwrap();
        let mut job =
            super::GenerateJob::create_as(3.0, r#"{"height":12,"length":80,"footprint":14}"#, true)
                .unwrap();
        while job.run_slice(0.0).status == "running" {}
        assert_eq!(job.result_line(3), line);
        let status = job.status();
        let seeds = status.segment_seeds.expect("segment seeds");
        assert_eq!(seeds.len(), 2);
        let loaded = parble_core::file::load(&job.file_json("generated").unwrap()).unwrap();
        assert_eq!(loaded.track.seed, 3);
        let stamp = loaded.metadata.generator.expect("generator stamp");
        assert_eq!(stamp.version, parble_procgen::GENERATOR_VERSION);
        assert_eq!(stamp.segments, seeds);
    }
}
