//! Pause and slow motion for the live sim. The sim step stays the fixed 240 Hz tick: a slower
//! time scale only feeds less real time into the tick accumulator, and pause feeds none, so a run's
//! trajectory and hash depend on the tick count alone. Keys: `P` pause, `,` slower, `.` faster,
//! `N` one step while paused. Switches: `--time-scale`, `--paused` (`docs/agent-operability.md`).

use bevy::prelude::*;

use crate::{LOCKSTEP_TICKS, plan_ticks};

/// The time scales `,` and `.` step through, fastest first.
pub const TIME_SCALES: [f64; 4] = [1.0, 0.5, 0.25, 0.1];

/// Playback state for the live sim.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
pub struct Playback {
    /// Index into [`TIME_SCALES`].
    pub level: usize,
    pub paused: bool,
    /// A single step requested while paused, run on the next frame.
    pub step: bool,
}

impl Playback {
    pub fn new(time_scale: f64, paused: bool) -> Self {
        Self {
            level: level_of(time_scale).unwrap_or(0),
            paused,
            step: false,
        }
    }

    /// The chosen time scale (kept while paused).
    pub fn scale(self) -> f64 {
        TIME_SCALES[self.level.min(TIME_SCALES.len() - 1)]
    }

    pub fn slower(&mut self) {
        self.level = (self.level + 1).min(TIME_SCALES.len() - 1);
    }

    pub fn faster(&mut self) {
        self.level = self.level.saturating_sub(1);
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        self.step = false;
    }

    /// Ask for one step; ignored unless paused.
    pub fn request_step(&mut self) {
        self.step = self.paused;
    }

    /// Real time is not passing at 1x: paused or slowed.
    pub fn slowed(self) -> bool {
        self.paused || self.level > 0
    }
}

/// The [`TIME_SCALES`] index of `scale`, if it is one of them.
pub fn level_of(scale: f64) -> Option<usize> {
    TIME_SCALES.iter().position(|s| (s - scale).abs() < 1e-9)
}

/// Whole ticks to run this frame and the new accumulator, for `delta` real seconds and `acc`
/// seconds of unsimulated time. Paused: none (one [`LOCKSTEP_TICKS`] step when requested) and the
/// accumulator is left as it is, so resuming does not jump and the frozen frame keeps its
/// interpolation. Otherwise `delta` is scaled before it reaches the accumulator.
pub fn frame_ticks(pb: &mut Playback, acc: f64, delta: f64) -> (u32, f64) {
    if pb.paused {
        let ticks = if std::mem::take(&mut pb.step) {
            LOCKSTEP_TICKS
        } else {
            0
        };
        return (ticks, acc);
    }
    plan_ticks(acc, delta * pb.scale())
}

/// Lockstep (`--lockstep-after`) ticks for this frame: pause and step apply, the time scale does
/// not (a scaled lockstep frame would not be a whole tick count).
pub fn lockstep_frame_ticks(pb: &mut Playback, holding: bool) -> u32 {
    if pb.paused {
        if std::mem::take(&mut pb.step) {
            LOCKSTEP_TICKS
        } else {
            0
        }
    } else if holding {
        0
    } else {
        LOCKSTEP_TICKS
    }
}

/// `P`, `,`, `.` and `N`. Ignored with Ctrl or Super held (browser print and preferences).
pub fn playback_keys(keys: Option<Res<ButtonInput<KeyCode>>>, mut pb: ResMut<Playback>) {
    let Some(keys) = keys else {
        return;
    };
    if keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]) {
        return;
    }
    if keys.just_pressed(KeyCode::KeyP) {
        pb.toggle_pause();
    }
    if keys.just_pressed(KeyCode::Comma) {
        pb.slower();
    }
    if keys.just_pressed(KeyCode::Period) {
        pb.faster();
    }
    if keys.just_pressed(KeyCode::KeyN) {
        pb.request_step();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cap_to_run;
    use parble_sim::{Sim, golden};

    const FRAME_S: f64 = 1.0 / 60.0;

    /// Drive the ramp run the way `advance_sim` does, one 60 Hz frame at a time, with `pb` changed
    /// per frame by `script(frame, &mut pb)`, until the run's last tick.
    fn run_ramp(mut script: impl FnMut(u32, &mut Playback)) -> (u64, u32) {
        let mut sim = Sim::ramp_scene();
        let mut pb = Playback::default();
        let mut acc = 0.0;
        let mut frame = 0;
        while sim.tick_count() < u64::from(golden::RAMP_10S_TICKS) {
            script(frame, &mut pb);
            let before = sim.tick_count();
            let (ticks, rest) = frame_ticks(&mut pb, acc, FRAME_S);
            acc = rest;
            let ticks = cap_to_run(ticks, sim.tick_count(), golden::RAMP_10S_TICKS);
            if ticks > 0 {
                sim.tick(ticks);
            }
            if pb.paused {
                assert!(sim.tick_count() - before <= u64::from(LOCKSTEP_TICKS));
            }
            frame += 1;
            assert!(frame < 100_000, "the run never finished");
        }
        (sim.trajectory_hash(), frame)
    }

    #[test]
    fn a_quarter_speed_run_reaches_the_golden_hash_in_four_times_the_frames() {
        let (hash, frames) = run_ramp(|f, pb| {
            if f == 0 {
                *pb = Playback::new(0.25, false);
            }
        });
        assert_eq!(hash, golden::ramp_10s_hash());
        let (one, one_frames) = run_ramp(|_, _| {});
        assert_eq!(one, golden::ramp_10s_hash());
        // 2,400 ticks are 600 frames at 1x and about 2,400 at 0.25x.
        assert!(one_frames.abs_diff(600) <= 2, "{one_frames}");
        assert!(frames.abs_diff(2_400) <= 4, "{frames}");
    }

    #[test]
    fn every_time_scale_reaches_the_golden_hash() {
        for scale in TIME_SCALES {
            let (hash, _) = run_ramp(|f, pb| {
                if f == 0 {
                    *pb = Playback::new(scale, false);
                }
            });
            assert_eq!(hash, golden::ramp_10s_hash(), "{scale}");
        }
    }

    #[test]
    fn a_paused_stepped_and_resumed_run_reaches_the_golden_hash() {
        let mut frozen_at = None;
        let (hash, _) = run_ramp(|f, pb| match f {
            100 => pb.toggle_pause(),
            101..=399 => {
                // Steps on a few paused frames; the rest hold.
                if f % 50 == 0 {
                    pb.request_step();
                }
                frozen_at.get_or_insert(f);
            }
            400 => {
                pb.toggle_pause();
                pb.slower();
            }
            700 => pb.faster(),
            _ => {}
        });
        assert_eq!(frozen_at, Some(101));
        assert_eq!(hash, golden::ramp_10s_hash());
    }

    #[test]
    fn pause_holds_the_accumulator_and_steps_one_sample() {
        let mut pb = Playback::new(1.0, true);
        assert_eq!(frame_ticks(&mut pb, 0.002, 1.0), (0, 0.002));
        pb.request_step();
        assert_eq!(frame_ticks(&mut pb, 0.002, 1.0), (LOCKSTEP_TICKS, 0.002));
        assert_eq!(frame_ticks(&mut pb, 0.002, 1.0).0, 0);
        // A step asked for while running is dropped.
        let mut running = Playback::default();
        running.request_step();
        assert!(!running.step);
        assert_eq!(lockstep_frame_ticks(&mut pb, false), 0);
        pb.request_step();
        assert_eq!(lockstep_frame_ticks(&mut pb, false), LOCKSTEP_TICKS);
        assert_eq!(lockstep_frame_ticks(&mut running, true), 0);
        assert_eq!(lockstep_frame_ticks(&mut running, false), LOCKSTEP_TICKS);
    }

    #[test]
    fn the_scale_ladder_clamps_at_both_ends() {
        let mut pb = Playback::default();
        pb.faster();
        assert_eq!(pb.scale(), 1.0);
        for want in [0.5, 0.25, 0.1, 0.1] {
            pb.slower();
            assert_eq!(pb.scale(), want);
        }
        assert!(pb.slowed());
        assert_eq!(level_of(0.25), Some(2));
        assert_eq!(level_of(0.3), None);
        assert!(!Playback::default().slowed());
        assert!(Playback::new(1.0, true).slowed());
    }
}
