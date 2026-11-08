//! Running a loaded [`Track`] (P3-T3).
//!
//! Piece colliders arrive with the kit loader and chunk merge (P1-T3). Until then a track runs in
//! the P0 tray: the parts of the track the sim can consume are its hopper queue and release mode,
//! which decide which marbles enter and on which tick. Placements, mechanisms and the seed do not
//! reach physics yet.
//!
//! Releases happen between steps, on whole ticks, so a run is a pure function of the track and the
//! tick count.

use crate::{Sim, scene};
use parble_core::contract::{
    MARBLE_MINI_DIAMETER_UM, MARBLE_SHOOTER_DIAMETER_UM, MARBLE_STANDARD_DIAMETER_UM,
};
use parble_core::{MarbleSize, ReleaseMode, Track};
use rapier3d::prelude::*;

/// `ReleaseMode::All` drops the queue one marble every 48 ticks (0.2 s) so no two spawn inside
/// each other.
pub const ALL_STAGGER_TICKS: u64 = 48;

/// Radius in metres of a marble size (contract §6).
pub fn marble_radius_m(size: MarbleSize) -> f32 {
    let d = match size {
        MarbleSize::Mini => MARBLE_MINI_DIAMETER_UM,
        MarbleSize::Standard => MARBLE_STANDARD_DIAMETER_UM,
        MarbleSize::Shooter => MARBLE_SHOOTER_DIAMETER_UM,
    };
    scene::um_to_m(d / 2)
}

/// Release ticks for the hopper queue, in queue order.
pub fn release_schedule(track: &Track) -> Vec<(u64, MarbleSize)> {
    let queue = &track.hopper.queue;
    match track.hopper.release {
        ReleaseMode::One => queue.first().map(|&m| vec![(0, m)]).unwrap_or_default(),
        ReleaseMode::All => (0u64..)
            .zip(queue)
            .map(|(i, &m)| (i * ALL_STAGGER_TICKS, m))
            .collect(),
        ReleaseMode::Interval { ticks } => (0u64..)
            .zip(queue)
            .map(|(i, &m)| (i * u64::from(ticks.max(1)), m))
            .collect(),
    }
}

/// A [`Sim`] plus the release schedule that feeds it. Reads go through `Deref` to the sim;
/// [`TrackRun::tick`] releases due marbles before each step.
pub struct TrackRun {
    sim: Sim,
    releases: Vec<(u64, MarbleSize)>,
    next: usize,
}

impl TrackRun {
    /// A run with no releases (the P0 scenarios).
    pub fn from_sim(sim: Sim) -> Self {
        Self {
            sim,
            releases: Vec::new(),
            next: 0,
        }
    }

    /// `track` in the tray, at tick 0 with its tick-0 releases in and sampled.
    pub fn new(track: &Track) -> Self {
        let mut sim = Sim::empty();
        sim.world.colliders.insert(scene::tray_collider());
        let mut run = Self {
            sim,
            releases: release_schedule(track),
            next: 0,
        };
        run.release_due();
        run.sim.sample();
        run
    }

    /// Advance `n` fixed steps, releasing marbles whose tick has come before each step.
    pub fn tick(&mut self, n: u32) {
        for _ in 0..n {
            self.release_due();
            self.sim.tick(1);
        }
    }

    fn release_due(&mut self) {
        while let Some(&(at, size)) = self.releases.get(self.next) {
            if at > self.sim.tick {
                break;
            }
            self.spawn(size);
            self.next += 1;
        }
    }

    fn spawn(&mut self, size: MarbleSize) {
        let r = marble_radius_m(size);
        let x = r + 0.004;
        let surface_y = scene::ramp_height_m() * (1.0 - x / scene::ramp_run_m());
        let body = RigidBodyBuilder::dynamic()
            .translation(Vector::new(x, surface_y + 1.25 * r, 0.0))
            .ccd_enabled(true)
            .can_sleep(false)
            .build();
        let collider = ColliderBuilder::ball(r)
            .density(scene::GLASS_DENSITY)
            .friction(scene::GLASS_FRICTION)
            .restitution(scene::GLASS_RESTITUTION)
            .build();
        let w = &mut self.sim.world;
        let handle = w.bodies.insert(body);
        w.colliders
            .insert_with_parent(collider, handle, &mut w.bodies);
        self.sim.marbles.push(handle);
    }
}

impl core::ops::Deref for TrackRun {
    type Target = Sim;

    fn deref(&self) -> &Sim {
        &self.sim
    }
}

/// Run `track` for `ticks` and return its trajectory hash.
pub fn run_hash(track: &Track, ticks: u32) -> u64 {
    let mut run = TrackRun::new(track);
    run.tick(ticks);
    run.trajectory_hash()
}

#[cfg(test)]
mod tests {
    use super::*;
    use parble_core::HopperConfig;

    fn track(queue: Vec<MarbleSize>, release: ReleaseMode) -> Track {
        let mut t = Track::new(1);
        t.hopper = HopperConfig {
            placement: None,
            queue,
            release,
        };
        t
    }

    #[test]
    fn schedules_follow_the_release_mode() {
        use MarbleSize::*;
        let q = vec![Standard, Mini, Shooter];
        assert_eq!(
            release_schedule(&track(q.clone(), ReleaseMode::One)),
            [(0, Standard)]
        );
        assert_eq!(
            release_schedule(&track(q.clone(), ReleaseMode::All)),
            [(0, Standard), (48, Mini), (96, Shooter)]
        );
        assert_eq!(
            release_schedule(&track(q, ReleaseMode::Interval { ticks: 10 })),
            [(0, Standard), (10, Mini), (20, Shooter)]
        );
    }

    #[test]
    fn marbles_enter_on_their_tick_and_runs_repeat() {
        let t = track(
            vec![MarbleSize::Standard, MarbleSize::Mini],
            ReleaseMode::Interval { ticks: 5 },
        );
        let mut run = TrackRun::new(&t);
        assert_eq!(run.marbles().len(), 1);
        run.tick(5);
        assert_eq!(run.marbles().len(), 1);
        run.tick(1);
        assert_eq!(run.marbles().len(), 2);
        assert_eq!(run_hash(&t, 600), run_hash(&t, 600));
        assert_ne!(
            run_hash(&t, 600),
            run_hash(&track(vec![MarbleSize::Standard], ReleaseMode::One), 600)
        );
    }

    #[test]
    fn one_standard_marble_matches_the_ramp_golden() {
        let t = track(vec![MarbleSize::Standard], ReleaseMode::One);
        assert_eq!(
            run_hash(&t, crate::golden::RAMP_10S_TICKS),
            crate::golden::ramp_10s_hash()
        );
    }
}
