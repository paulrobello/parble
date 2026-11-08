//! The golden determinism run, shared by the native test and the wasm parity check (P0-T4).
//!
//! The committed hash lives in `crates/parble-sim/tests/golden/ramp_10s.hash` as 16 lowercase
//! hex digits and a newline. The P5-T1 type × size runs are in `tests/golden/ramp_10s_variants.hash`
//! ([`ramp_10s_variants`]).

use crate::Sim;
use crate::marble::{MarbleKind, MarbleSize};

/// Run the ramp scene with `kind` at `size` (P5-T1) for [`RAMP_10S_TICKS`] and return its hash.
pub fn ramp_10s_variant_hash(kind: MarbleKind, size: MarbleSize) -> u64 {
    let mut sim = Sim::ramp_scene_with(kind.spec_sized(size));
    sim.tick(RAMP_10S_TICKS);
    sim.trajectory_hash()
}

/// Every type × size ramp hash, one `"<type> <size> <hash>"` line each, in [`MarbleKind::ALL`] ×
/// [`MarbleSize::ALL`] order: the format of `tests/golden/ramp_10s_variants.hash`, which both the
/// native test and the wasm parity check compare against.
pub fn ramp_10s_variants() -> String {
    let mut out = String::new();
    for kind in MarbleKind::ALL {
        for size in MarbleSize::ALL {
            let hash = crate::hash::format_hash(ramp_10s_variant_hash(kind, size));
            out.push_str(&format!("{} {} {hash}\n", kind.name(), size.name()));
        }
    }
    out
}

/// 10 s at 240 Hz.
pub const RAMP_10S_TICKS: u32 = 2_400;

/// Run the ramp scene for [`RAMP_10S_TICKS`] and return its trajectory hash.
pub fn ramp_10s_hash() -> u64 {
    let mut sim = Sim::ramp_scene();
    sim.tick(RAMP_10S_TICKS);
    sim.trajectory_hash()
}

/// [`ramp_10s_hash`] in the golden file format (without the newline).
pub fn ramp_10s_hash_hex() -> String {
    crate::hash::format_hash(ramp_10s_hash())
}
