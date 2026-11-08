//! parble: the Bevy app entry for native and web.
//!
//! The sim advances in whole 240 Hz ticks from a wall-clock accumulator; the renderer interpolates
//! between the last two ticks. The P0 ramp scene restarts every 10 s so the page keeps moving.
//! Agent switches (screenshot, auto-exit, state dump, `window.__parble`) live in [`agent`].

mod agent;
mod bridge;
mod persist;

use agent::{AgentOpts, AgentPlugin, Scenario};
use bevy::prelude::*;
use bridge::{Command, Event};
use parble_render::{
    MarbleFrame, ParbleRenderPlugin,
    camera::{self, DirectorClock, DirectorPlugin},
};
use parble_sim::{DT, golden::RAMP_10S_TICKS, track_run::TrackRun};
use persist::{After, Tracks};

/// At most this many ticks per frame (50 ms of sim time); a longer stall drops the backlog instead
/// of spiralling.
const MAX_TICKS_PER_FRAME: u32 = 12;
/// Ticks per frame in lockstep (`--lockstep-after`): 1/60 s.
pub const LOCKSTEP_TICKS: u32 = 4;

#[derive(Resource)]
struct SimClock {
    sim: TrackRun,
    scenario: Scenario,
    /// The `.parble` track loaded over the bridge; when set it replaces `scenario`.
    tracks: Tracks,
    /// Unsimulated wall time, seconds.
    acc: f64,
    /// Completed runs.
    runs: u32,
    /// Trajectory hash of the last completed run (exactly [`RAMP_10S_TICKS`] ticks).
    last_run_hash: Option<u64>,
}

fn main() {
    let opts = agent::read_opts();
    let mut window = Window {
        title: "Parble".into(),
        canvas: Some("#parble".into()),
        fit_canvas_to_parent: true,
        ..default()
    };
    // Native screenshots are 1280x720 physical on any display, like the web e2e viewport: the DoF
    // circle of confusion is in pixels, so a Retina 2x window would show a different blur.
    if cfg!(not(target_arch = "wasm32")) && opts.screenshot.is_some() {
        window.resolution =
            bevy::window::WindowResolution::new(1280, 720).with_scale_factor_override(1.0);
    }
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(window),
            ..default()
        }))
        .add_plugins(ParbleRenderPlugin)
        .add_plugins(AgentPlugin(opts.clone()))
        .add_plugins(DirectorPlugin {
            mode: opts.camera.mode(),
            cues: opts.camera.cues(opts.scenario),
        })
        .insert_resource(SimClock {
            sim: TrackRun::from_sim(opts.scenario.build()),
            scenario: opts.scenario,
            tracks: Tracks::default(),
            acc: 0.0,
            runs: 0,
            last_run_hash: None,
        })
        .add_systems(Startup, (spawn_camera, publish_initial, announce_bridge))
        .add_systems(Update, (advance_sim, pump_bridge).chain())
        .run();
}

fn spawn_camera(mut commands: Commands, opts: Res<AgentOpts>) {
    camera::spawn_main_camera(&mut commands, opts.quality, &opts.post);
}

fn publish_initial(clock: Res<SimClock>, mut frame: ResMut<MarbleFrame>) {
    let now = clock.sim.marbles();
    frame.prev = now.clone();
    frame.curr = now;
    frame.alpha = 0.0;
}

/// Whole ticks to run for `acc + delta` seconds of unsimulated time, and the remainder. Past
/// [`MAX_TICKS_PER_FRAME`] the backlog is dropped (remainder 0).
fn plan_ticks(acc: f64, delta: f64) -> (u32, f64) {
    let dt = f64::from(DT);
    let acc = acc + delta;
    let due = (acc / dt).floor() as u32;
    let ticks = due.min(MAX_TICKS_PER_FRAME);
    let rest = if due > ticks {
        0.0
    } else {
        acc - f64::from(ticks) * dt
    };
    (ticks, rest)
}

/// Clamp a frame's ticks so a run stops exactly at [`RAMP_10S_TICKS`].
fn cap_to_run(ticks: u32, tick_count: u64) -> u32 {
    let left = u64::from(RAMP_10S_TICKS).saturating_sub(tick_count);
    ticks.min(u32::try_from(left).unwrap_or(u32::MAX))
}

/// Lockstep ticks for this frame: none while holding for `after` real seconds, then a fixed count.
fn lockstep_ticks(real_elapsed: f64, after: f64) -> u32 {
    if real_elapsed < after {
        0
    } else {
        LOCKSTEP_TICKS
    }
}

fn advance_sim(
    time: Res<Time>,
    real: Res<Time<Real>>,
    opts: Res<AgentOpts>,
    mut clock: ResMut<SimClock>,
    mut frame: ResMut<MarbleFrame>,
    mut director: ResMut<DirectorClock>,
) {
    let (ticks, rest) = match opts.lockstep_after {
        Some(after) => (lockstep_ticks(real.elapsed_secs_f64(), after), 0.0),
        None => plan_ticks(clock.acc, time.delta_secs_f64()),
    };
    clock.acc = rest;
    if ticks > 0 {
        if clock.sim.tick_count() >= u64::from(RAMP_10S_TICKS) {
            clock.last_run_hash = Some(clock.sim.trajectory_hash());
            clock.runs += 1;
            clock.sim = clock.build_run();
            frame.curr = clock.sim.marbles();
        }
        let ticks = cap_to_run(ticks, clock.sim.tick_count());
        frame.prev = if ticks == 1 {
            std::mem::take(&mut frame.curr)
        } else {
            clock.sim.tick(ticks - 1);
            clock.sim.marbles()
        };
        clock.sim.tick(1);
        frame.curr = clock.sim.marbles();
    }
    frame.alpha = if opts.lockstep_after.is_some() {
        // Show the state at exactly the current tick.
        1.0
    } else {
        (clock.acc / f64::from(DT)) as f32
    };
    *director = DirectorClock {
        tick: clock.sim.tick_count(),
        run: clock.runs,
        dt: if opts.lockstep_after.is_some() {
            ticks as f32 * DT
        } else {
            time.delta_secs()
        },
    };
}

fn announce_bridge() {
    bridge::push_event(Event::Ready {
        protocol: bridge::PROTOCOL,
    });
}

impl SimClock {
    /// A fresh run of the loaded track, or of the scenario when none is loaded.
    fn build_run(&self) -> TrackRun {
        match &self.tracks.loaded {
            Some(f) => TrackRun::new(&f.track),
            None => TrackRun::from_sim(self.scenario.build()),
        }
    }

    /// Start the live sim over at tick 0 of [`Self::build_run`].
    fn restart(&mut self, frame: &mut MarbleFrame) {
        self.sim = self.build_run();
        self.acc = 0.0;
        self.runs = 0;
        self.last_run_hash = None;
        let now = self.sim.marbles();
        frame.prev = now.clone();
        frame.curr = now;
        frame.alpha = 0.0;
    }
}

/// Answers the commands the overlay posted and publishes this frame's tick and smoothed fps. Runs
/// after [`advance_sim`], so commands that replace the sim apply between ticks.
fn pump_bridge(
    time: Res<Time>,
    mut clock: ResMut<SimClock>,
    mut frame: ResMut<MarbleFrame>,
    mut fps: Local<f32>,
) {
    for cmd in bridge::take_commands() {
        if let Command::Ping { id } = cmd {
            bridge::push_event(Event::Pong { id });
            continue;
        }
        if let Some((event, after)) = clock.tracks.apply(cmd) {
            if after == After::Restart {
                clock.restart(&mut frame);
            }
            bridge::push_event(event);
        }
    }
    let dt = time.delta_secs();
    if dt > 0.0 {
        let now = 1.0 / dt;
        *fps = if *fps == 0.0 {
            now
        } else {
            *fps + (now - *fps) * 0.1
        };
    }
    bridge::push_event(Event::Frame {
        tick: clock.sim.tick_count(),
        fps: *fps,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use parble_sim::Sim;

    #[test]
    fn plan_ticks_keeps_the_sub_tick_remainder() {
        let dt = f64::from(DT);
        let (ticks, rest) = plan_ticks(0.25 * dt, 2.5 * dt);
        assert_eq!(ticks, 2);
        assert!((rest - 0.75 * dt).abs() < 1e-9);
        assert_eq!(plan_ticks(0.0, 0.5 * dt).0, 0);
    }

    #[test]
    fn capped_frames_end_a_run_on_the_golden_hash() {
        assert_eq!(cap_to_run(12, 2_395), 5);
        assert_eq!(cap_to_run(12, 2_400), 0);
        assert_eq!(cap_to_run(3, 0), 3);
        let mut sim = Sim::ramp_scene();
        while sim.tick_count() < u64::from(RAMP_10S_TICKS) {
            sim.tick(cap_to_run(MAX_TICKS_PER_FRAME, sim.tick_count()));
        }
        assert_eq!(sim.trajectory_hash(), parble_sim::golden::ramp_10s_hash());
    }

    #[test]
    fn lockstep_holds_then_steps_a_fixed_count() {
        assert_eq!(lockstep_ticks(1.0, 3.0), 0);
        assert_eq!(lockstep_ticks(3.0, 3.0), LOCKSTEP_TICKS);
        // 2,400 ticks is a whole number of lockstep frames, so a run still ends on the golden hash.
        assert_eq!(RAMP_10S_TICKS % LOCKSTEP_TICKS, 0);
    }

    #[test]
    fn plan_ticks_drops_a_stall_backlog() {
        let (ticks, rest) = plan_ticks(0.0, 1.0);
        assert_eq!(ticks, MAX_TICKS_PER_FRAME);
        assert_eq!(rest, 0.0);
    }

    #[test]
    fn trackside_cue_catches_the_marble_at_speed() {
        // Peak speed is at the ramp foot; the trackside cut must still see ≥ 90 % of it.
        let cue = parble_render::camera::RAMP_TRACKSIDE_TICK;
        let mut sim = Sim::ramp_scene();
        let (mut peak, mut at_cue) = (0f32, 0f32);
        while sim.tick_count() < cue + 60 {
            sim.tick(LOCKSTEP_TICKS);
            let v = sim.marbles()[0].linvel;
            let s = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            peak = peak.max(s);
            if sim.tick_count() >= cue && at_cue == 0.0 {
                at_cue = s;
            }
        }
        assert!(at_cue >= 0.9 * peak, "speed at cue {at_cue} vs peak {peak}");
    }
}
