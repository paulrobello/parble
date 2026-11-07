//! The golden determinism run, shared by the native test and the wasm parity check (P0-T4).
//!
//! The committed hash lives in `crates/parble-sim/tests/golden/ramp_10s.hash` as 16 lowercase
//! hex digits and a newline.

use crate::Sim;

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
