//! parble-sim: the deterministic Rapier world. This crate is the determinism boundary.
//!
//! Determinism rules (PRD §5.4, R4):
//! - Fixed 240 Hz step, advanced only by [`Sim::tick`]. No wall-clock input anywhere.
//! - `rapier3d` with `enhanced-determinism` (forces simba `libm_force`, glam `libm` +
//!   `scalar-math`), no `parallel`, no SIMD. Any trig or sqrt in this crate goes through `libm`.
//! - Marbles live in a `Vec` in spawn order. No `HashMap` iteration, no threads.
//! - [`Sim::trajectory_hash`] is FNV-1a 64 over quantized marble state, sampled every
//!   [`HASH_SAMPLE_TICKS`] ticks.
//!
//! Length unit: the world is in metres (kit contract §1) and Rapier's
//! `IntegrationParameters::length_unit` is set to one cell (0.06 m). Rapier scales its contact
//! tolerances by `length_unit` (allowed linear error 0.005 × unit, prediction distance
//! 0.02 × unit, max corrective velocity 3 × unit/s), so the metre-tuned default of 1.0 would
//! allow 5 mm of penetration, more than half a standard marble's radius. At 0.06 they are
//! 0.3 mm, 1.2 mm and 0.18 m/s. All other integration parameters are Rapier 0.36.1 defaults.

pub mod golden;
pub mod hash;
pub mod marble;
pub mod scene;
pub mod track_run;

use hash::{Fnv1a64, POSITION_QUANTA_PER_M, VELOCITY_QUANTA_PER_M_PER_S, quantize};
use rapier3d::prelude::*;

/// Fixed simulation rate.
pub const TICK_HZ: u32 = 240;
/// Fixed step, seconds.
pub const DT: f32 = 1.0 / TICK_HZ as f32;
/// The trajectory hash samples marble state every 24 ticks (10 Hz, 100 samples per 10 s run)
/// plus once at tick 0.
pub const HASH_SAMPLE_TICKS: u64 = 24;

/// Rapier `length_unit`: one cell in metres (see the crate docs).
pub fn length_unit() -> f32 {
    scene::cell_m()
}

/// Snapshot of one marble for rendering and tests.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarbleState {
    /// Centre, metres.
    pub position: [f32; 3],
    /// Orientation quaternion `[x, y, z, w]`.
    pub rotation: [f32; 4],
    /// Linear velocity, m/s.
    pub linvel: [f32; 3],
}

/// A deterministic sim world advanced in whole ticks.
pub struct Sim {
    world: PhysicsWorld,
    marbles: Vec<RigidBodyHandle>,
    tick: u64,
    hasher: Fnv1a64,
}

impl Sim {
    /// An empty world: 240 Hz, cell length unit, gravity −9.81 m/s² on Y.
    pub fn empty() -> Self {
        let mut world = PhysicsWorld::new();
        world.gravity = Vector::new(0.0, -marble::G, 0.0);
        world.integration_parameters.dt = DT;
        world.integration_parameters.length_unit = length_unit();
        Self {
            world,
            marbles: Vec::new(),
            tick: 0,
            hasher: Fnv1a64::default(),
        }
    }

    /// The P0 scene: one marble with the [`marble::P0_LEGACY`] parameters at the top of the tray
    /// ramp. This is the golden run (`tests/golden/ramp_10s.hash`).
    pub fn ramp_scene() -> Self {
        Self::ramp_scene_with(marble::P0_LEGACY)
    }

    /// The P0 tray ramp with one marble of the given spec (P1-T6 `--marble`).
    pub fn ramp_scene_with(spec: marble::MarbleSpec) -> Self {
        let mut sim = Self::empty();
        sim.world.colliders.insert(scene::tray_collider());
        let (body, collider) = spec.build(scene::marble_spawn_m());
        let handle = sim.world.bodies.insert(body);
        sim.world
            .colliders
            .insert_with_parent(collider, handle, &mut sim.world.bodies);
        sim.marbles.push(handle);
        sim.sample();
        sim
    }

    /// Look-dev close-up (P1-T6): the tray with one marble of the given spec resting on the flat
    /// floor at [`scene::closeup_marble_m`], so a fixed close camera can frame it.
    pub fn closeup_scene(spec: marble::MarbleSpec) -> Self {
        let mut sim = Self::empty();
        sim.world.colliders.insert(scene::tray_collider());
        let (body, collider) = spec.build(scene::closeup_marble_m(spec.radius_m));
        let handle = sim.world.bodies.insert(body);
        sim.world
            .colliders
            .insert_with_parent(collider, handle, &mut sim.world.bodies);
        sim.marbles.push(handle);
        sim.sample();
        sim
    }

    /// Advance `n` fixed steps, sampling the trajectory hash on every multiple of
    /// [`HASH_SAMPLE_TICKS`].
    pub fn tick(&mut self, n: u32) {
        for _ in 0..n {
            self.world.step();
            self.tick += 1;
            if self.tick.is_multiple_of(HASH_SAMPLE_TICKS) {
                self.sample();
            }
        }
    }

    /// Ticks advanced since construction.
    pub fn tick_count(&self) -> u64 {
        self.tick
    }

    /// FNV-1a 64 over every sample so far. Each sample absorbs the tick number, then for each
    /// marble in spawn order its position (1 µm quanta) and linear velocity (1 µm/s quanta) as
    /// little-endian `i64`s.
    pub fn trajectory_hash(&self) -> u64 {
        self.hasher.finish()
    }

    /// Marble snapshots in spawn order.
    pub fn marbles(&self) -> Vec<MarbleState> {
        self.marbles
            .iter()
            .map(|&h| {
                let b = &self.world.bodies[h];
                let t = b.translation();
                let r = b.rotation();
                let v = b.linvel();
                MarbleState {
                    position: [t.x, t.y, t.z],
                    rotation: [r.x, r.y, r.z, r.w],
                    linvel: [v.x, v.y, v.z],
                }
            })
            .collect()
    }

    fn sample(&mut self) {
        self.hasher.write_u64(self.tick);
        for &h in &self.marbles {
            let b = &self.world.bodies[h];
            let t = b.translation();
            let v = b.linvel();
            for p in [t.x, t.y, t.z] {
                self.hasher.write_i64(quantize(p, POSITION_QUANTA_PER_M));
            }
            for c in [v.x, v.y, v.z] {
                self.hasher
                    .write_i64(quantize(c, VELOCITY_QUANTA_PER_M_PER_S));
            }
        }
    }
}
