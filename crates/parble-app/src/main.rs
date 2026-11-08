//! parble: the Bevy app entry for native and web.
//!
//! The sim advances in whole 240 Hz ticks from a wall-clock accumulator; the renderer interpolates
//! between the last two ticks. The P0 ramp scene restarts every 10 s so the page keeps moving.
//! Agent switches (screenshot, auto-exit, state dump, `window.__parble`) live in [`agent`].

mod agent;

use agent::{AgentOpts, AgentPlugin, Scenario};
use bevy::prelude::*;
use parble_render::{MarbleFrame, ParbleRenderPlugin};
use parble_sim::{DT, Sim, golden::RAMP_10S_TICKS, scene};

/// At most this many ticks per frame (50 ms of sim time); a longer stall drops the backlog instead
/// of spiralling.
const MAX_TICKS_PER_FRAME: u32 = 12;

#[derive(Resource)]
struct SimClock {
    sim: Sim,
    scenario: Scenario,
    /// Unsimulated wall time, seconds.
    acc: f64,
    /// Completed runs.
    runs: u32,
    /// Trajectory hash of the last completed run (exactly [`RAMP_10S_TICKS`] ticks).
    last_run_hash: Option<u64>,
}

fn main() {
    let opts = agent::read_opts();
    let window = Window {
        title: "Parble".into(),
        canvas: Some("#parble".into()),
        fit_canvas_to_parent: true,
        ..default()
    };
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(window),
            ..default()
        }))
        .add_plugins(ParbleRenderPlugin)
        .add_plugins(AgentPlugin(opts.clone()))
        .insert_resource(SimClock {
            sim: opts.scenario.build(),
            scenario: opts.scenario,
            acc: 0.0,
            runs: 0,
            last_run_hash: None,
        })
        .add_systems(Startup, (spawn_camera, publish_initial))
        .add_systems(Update, advance_sim)
        .run();
}

fn spawn_camera(mut commands: Commands, opts: Res<AgentOpts>) {
    let len = scene::tray_length_m();
    let target = Vec3::new(len * 0.5, scene::ramp_height_m() * 0.35, 0.0);
    commands.spawn((
        Camera3d::default(),
        opts.quality.msaa(),
        Projection::Perspective(PerspectiveProjection {
            fov: 35f32.to_radians(),
            near: 0.005,
            ..default()
        }),
        // Near top-down (~80°): the 60 mm walls hide the 28 mm channel from shallower angles.
        Transform::from_xyz(len * 0.5, 0.80, 0.13).looking_at(target, Vec3::Y),
    ));
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

fn advance_sim(time: Res<Time>, mut clock: ResMut<SimClock>, mut frame: ResMut<MarbleFrame>) {
    let (ticks, rest) = plan_ticks(clock.acc, time.delta_secs_f64());
    clock.acc = rest;
    if ticks > 0 {
        if clock.sim.tick_count() >= u64::from(RAMP_10S_TICKS) {
            clock.last_run_hash = Some(clock.sim.trajectory_hash());
            clock.runs += 1;
            clock.sim = clock.scenario.build();
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
    frame.alpha = (clock.acc / f64::from(DT)) as f32;
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn plan_ticks_drops_a_stall_backlog() {
        let (ticks, rest) = plan_ticks(0.0, 1.0);
        assert_eq!(ticks, MAX_TICKS_PER_FRAME);
        assert_eq!(rest, 0.0);
    }
}
