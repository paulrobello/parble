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

/// P2-T1: the mechanism framework fixture ([`crate::mechanism::demo::ramp_with_flap`], seed 7)
/// for [`RAMP_10S_TICKS`], as the lines of `tests/golden/mechanism_demo_10s.hash`: the marble
/// trajectory hash, the mechanism state hash, and the event count with its
/// [`crate::mechanism::events_hash`].
pub fn mechanism_demo() -> String {
    let mut sim = crate::mechanism::demo::ramp_with_flap(7);
    sim.tick(RAMP_10S_TICKS);
    let events = sim.drain_events();
    let h = crate::hash::format_hash;
    format!(
        "trajectory {}\nmechanism {}\nevents {} {}\n",
        h(sim.trajectory_hash()),
        h(sim.mechanism_hash()),
        events.len(),
        h(crate::mechanism::events_hash(&events))
    )
}

/// Ticks of each [`rope_arch_pass`] run (3.75 s): the marble clears the far straight well before.
pub const ROPE_PASS_TICKS: u32 = 900;

/// Launch speed of [`rope_arch_pass`], m/s.
pub const ROPE_PASS_M_S: f32 = 0.6;

/// P2 item 18: a cat's eye through `mech_arch_rope_l1` between two L1 straights (the rig of the
/// `rope_arch_halves_the_speed_of_a_marble_passing_through` test) at [`ROPE_PASS_M_S`], once
/// along +X and once along −X, for [`ROPE_PASS_TICKS`] each. The lines of
/// `tests/golden/rope_arch_pass.hash`: per direction the trajectory hash, the mechanism state hash
/// (the ropes' segment angles and rates, so the cosmetic swing and bend too), and the event count with its
/// [`crate::mechanism::events_hash`].
pub fn rope_arch_pass() -> String {
    use crate::marble::CATS_EYE;
    use parble_core::{CellCoord, Mirror, PieceId, Placement, Yaw4};
    use rapier3d::prelude::Vector;

    let kit = parble_kit::builtin();
    let reg = crate::behaviour::registry();
    let place = |id: &str, x: i32| {
        Placement::new(
            PieceId::new(id).expect("kit id"),
            CellCoord::new(x, 0, 0),
            Yaw4::new(0),
            Mirror::Off,
        )
    };
    let layout = [
        place("blk_straight_l1", 0),
        place(crate::behaviour::ROPE_ARCH, 1),
        place("blk_straight_l1", 2),
    ];
    let h = crate::hash::format_hash;
    let mut out = String::new();
    for (name, dir, x0) in [("px", 1.0_f32, -0.02_f32), ("nx", -1.0, 0.14)] {
        let mut sim = Sim::empty();
        for p in &layout {
            sim.place_piece(kit, p.clone())
                .expect("the rig does not overlap");
        }
        sim.rebuild_chunks(kit).expect("the rig builds its chunks");
        for p in &layout {
            sim.add_registered(&reg, kit, p, 1)
                .expect("the rope arch builds");
        }
        sim.set_event_recording(true);
        let rest = 0.015 + CATS_EYE.radius_m;
        sim.add_marble(CATS_EYE, Vector::new(x0, rest, 0.0));
        let v = Vector::X * ROPE_PASS_M_S * dir;
        let body = &mut sim.world.bodies[*sim.marbles.last().expect("just added")];
        body.set_linvel(v, true);
        body.set_angvel(Vector::Y.cross(v) / CATS_EYE.radius_m, true);
        sim.sample();
        sim.tick(ROPE_PASS_TICKS);
        let events = sim.drain_events();
        out.push_str(&format!(
            "{name} trajectory {}\n{name} mechanism {}\n{name} events {} {}\n",
            h(sim.trajectory_hash()),
            h(sim.mechanism_hash()),
            events.len(),
            h(crate::mechanism::events_hash(&events))
        ));
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
