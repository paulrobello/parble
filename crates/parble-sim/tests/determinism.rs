//! In-process determinism and physical sanity of the P0 ramp scene.

use parble_sim::golden::{RAMP_10S_TICKS, ramp_10s_hash};
use parble_sim::{HASH_SAMPLE_TICKS, Sim, scene};

#[test]
fn two_runs_hash_identically() {
    assert_eq!(ramp_10s_hash(), ramp_10s_hash());
}

#[test]
fn tick_batching_does_not_change_the_hash() {
    let mut whole = Sim::ramp_scene();
    whole.tick(RAMP_10S_TICKS);
    let mut halves = Sim::ramp_scene();
    halves.tick(RAMP_10S_TICKS / 2);
    halves.tick(RAMP_10S_TICKS / 2);
    let mut odd = Sim::ramp_scene();
    for _ in 0..RAMP_10S_TICKS / 16 {
        odd.tick(7);
        odd.tick(9);
    }
    assert_eq!(whole.tick_count(), u64::from(RAMP_10S_TICKS));
    assert_eq!(whole.trajectory_hash(), halves.trajectory_hash());
    assert_eq!(whole.trajectory_hash(), odd.trajectory_hash());
}

#[test]
fn hash_only_changes_on_sample_ticks() {
    let mut sim = Sim::ramp_scene();
    let h0 = sim.trajectory_hash();
    sim.tick((HASH_SAMPLE_TICKS - 1) as u32);
    assert_eq!(sim.trajectory_hash(), h0);
    sim.tick(1);
    assert_ne!(sim.trajectory_hash(), h0);
}

/// The marble rolls down the ramp, reaches the flat floor and never tunnels out of the tray.
#[test]
fn marble_stays_in_tray_and_reaches_floor() {
    let r = scene::standard_marble_radius_m();
    let half_w = scene::tray_width_m() / 2.0;
    let l = scene::tray_length_m();
    let a = scene::ramp_run_m();
    let h = scene::ramp_height_m();
    // Tunnelling bound, 1/8 of a radius. Observed: rolling at ~1.17 m/s on the floor the
    // marble sinks 0.4–0.66 mm (0.08 mm at rest).
    let tol = 1.0e-3;

    let mut sim = Sim::ramp_scene();
    let mut max_x = f32::MIN;
    let mut max_speed_sq = 0.0_f32;
    for _ in 0..RAMP_10S_TICKS {
        sim.tick(1);
        let m = sim.marbles()[0];
        let [x, y, z] = m.position;
        let surface = if x < a { h * (1.0 - x / a) } else { 0.0 };
        assert!(
            y >= surface + r - tol,
            "tick {}: y {y} below surface",
            sim.tick_count()
        );
        assert!(
            x >= r - tol && x <= l - r + tol,
            "tick {}: x {x} left tray",
            sim.tick_count()
        );
        assert!(
            z.abs() <= half_w - r + tol,
            "tick {}: z {z} left tray",
            sim.tick_count()
        );
        max_x = max_x.max(x);
        let [vx, vy, vz] = m.linvel;
        max_speed_sq = max_speed_sq.max(vx * vx + vy * vy + vz * vz);
    }
    assert!(
        max_x > a + 4.0 * scene::cell_m(),
        "marble never rolled out onto the floor: max x {max_x}"
    );
    // Rolling sphere from rest: v = sqrt(10/7 g h) ≈ 1.30 m/s for h = 0.12 m.
    assert!(
        max_speed_sq > 1.0 && max_speed_sq < 2.0,
        "peak speed² {max_speed_sq} m²/s²"
    );
}
