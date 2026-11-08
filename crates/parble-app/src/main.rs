//! parble: the Bevy app entry for native and web.
//!
//! The sim advances in whole 240 Hz ticks from a wall-clock accumulator; the renderer interpolates
//! between the last two ticks. The P0 ramp scene restarts every 10 s so the page keeps moving.
//! Agent switches (screenshot, auto-exit, state dump, `window.__parble`) live in [`agent`].

mod agent;
mod audio;
mod bridge;
mod editor;
mod persist;

use agent::{AgentOpts, AgentPlugin, Scenario};
use bevy::{prelude::*, transform::TransformSystems};
use bridge::{Command, Event};
use parble_render::{
    MarbleFrame, ParbleRenderPlugin,
    camera::{self, DirectorClock, DirectorPlugin, MainCamera},
    kit::parts::PartFrame,
    marble::MarbleLook,
    sensors::{SensorDraw, SensorFrame, SensorOverlayPlugin, SensorShape},
};
use parble_sim::{
    DT, Sim,
    golden::RAMP_10S_TICKS,
    marble::{MarbleKind, MarbleSize},
    mechanism::SensorVolume,
    track_run::TrackRun,
};
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
    /// Marble type (`--marble`); `None` is the P0 legacy marble. A loaded track ignores it.
    marble: Option<MarbleKind>,
    /// Marble size (`--marble-size`, P5-T1).
    marble_size: MarbleSize,
    /// Unsimulated wall time, seconds.
    acc: f64,
    /// Completed runs.
    runs: u32,
    /// Trajectory hash of the last completed run (exactly [`SimClock::run_ticks`] ticks).
    last_run_hash: Option<u64>,
    /// Bridge restarts ([`SimClock::restart`]) so far. The editor resyncs to the restarted sim's
    /// track when it changes.
    restarts: u64,
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
    let exit = App::new()
        .add_plugins(parble_render::kit::KitAssetSourcePlugin {
            versions: agent::asset_versions(),
        })
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(window),
                    ..default()
                })
                // No `.meta` sidecars ship; probing for them 404s on the static web host.
                .set(AssetPlugin {
                    meta_check: bevy::asset::AssetMetaCheck::Never,
                    ..default()
                }),
        )
        .add_plugins(ParbleRenderPlugin)
        .add_plugins(AgentPlugin(opts.clone()))
        .add_plugins(DirectorPlugin {
            mode: opts.camera.mode(),
            cues: opts.camera.cues(opts.scenario, opts.view),
        })
        .add_plugins(SensorOverlayPlugin { on: opts.sensors })
        .add_plugins(editor::EditorPlugin {
            active: opts.scenario == Scenario::Editor,
            fill: opts.editor_fill,
        })
        .insert_resource(SimClock {
            sim: recording(opts.scenario.build(opts.marble, opts.marble_size)),
            scenario: opts.scenario,
            tracks: Tracks::default(),
            marble: opts.marble,
            marble_size: opts.marble_size,
            acc: 0.0,
            runs: 0,
            last_run_hash: None,
            restarts: 0,
        })
        .add_systems(Startup, (spawn_camera, publish_initial, announce_bridge))
        .add_systems(Update, (advance_sim, pump_bridge, publish_sensors).chain())
        .add_systems(PostUpdate, pump_audio.after(TransformSystems::Propagate))
        .run();
    // The web build's `run` returns at once while the browser drives the loop.
    #[cfg(not(target_arch = "wasm32"))]
    std::process::exit(i32::from(process_code(&exit)));
    #[cfg(target_arch = "wasm32")]
    let _ = exit;
}

/// Process exit code for an [`AppExit`] (the agent's documented 0/2/3).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn process_code(exit: &AppExit) -> u8 {
    match exit {
        AppExit::Success => 0,
        AppExit::Error(code) => code.get(),
    }
}

fn spawn_camera(mut commands: Commands, opts: Res<AgentOpts>) {
    camera::spawn_main_camera(&mut commands, opts.quality, &opts.post);
}

fn publish_initial(
    clock: Res<SimClock>,
    mut frame: ResMut<MarbleFrame>,
    mut parts: ResMut<PartFrame>,
) {
    let now = clock.sim.marbles();
    frame.prev = now.clone();
    frame.curr = now;
    frame.alpha = 0.0;
    parts.curr = part_angles(&clock.sim);
    parts.prev = parts.curr.clone();
}

/// Every mechanism instance's part angles (millidegrees), keyed by its anchor cell, for
/// [`PartFrame`].
fn part_angles(sim: &Sim) -> std::collections::BTreeMap<[i32; 3], Vec<i32>> {
    sim.mechanisms()
        .into_iter()
        .map(|m| ([m.id.0.x, m.id.0.y, m.id.0.z], m.part_angles))
        .collect()
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

/// Clamp a frame's ticks so a run stops exactly at `run_ticks`.
fn cap_to_run(ticks: u32, tick_count: u64, run_ticks: u32) -> u32 {
    let left = u64::from(run_ticks).saturating_sub(tick_count);
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
    mut parts: ResMut<PartFrame>,
    mut director: ResMut<DirectorClock>,
) {
    let (ticks, rest) = match opts.lockstep_after {
        Some(after) => (lockstep_ticks(real.elapsed_secs_f64(), after), 0.0),
        None => plan_ticks(clock.acc, time.delta_secs_f64()),
    };
    clock.acc = rest;
    if ticks > 0 {
        let run_ticks = clock.run_ticks();
        if clock.sim.tick_count() >= u64::from(run_ticks) {
            clock.last_run_hash = Some(clock.sim.trajectory_hash());
            clock.runs += 1;
            clock.sim = clock.build_run();
            frame.curr = clock.sim.marbles();
            parts.curr = part_angles(&clock.sim);
        }
        let ticks = cap_to_run(ticks, clock.sim.tick_count(), run_ticks);
        if ticks == 1 {
            frame.prev = std::mem::take(&mut frame.curr);
            parts.prev = std::mem::take(&mut parts.curr);
        } else {
            clock.sim.tick(ticks - 1);
            frame.prev = clock.sim.marbles();
            parts.prev = part_angles(&clock.sim);
        }
        clock.sim.tick(1);
        frame.curr = clock.sim.marbles();
        parts.curr = part_angles(&clock.sim);
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

/// The sim's sensor volumes and occupancy for the debug overlay.
fn publish_sensors(clock: Res<SimClock>, mut frame: ResMut<SensorFrame>) {
    frame.0.clear();
    frame
        .0
        .extend(clock.sim.sensors().into_iter().map(|s| SensorDraw {
            shape: match s.volume {
                SensorVolume::Box { center, half } => SensorShape::Box {
                    center: Vec3::from_array(center),
                    half: Vec3::from_array(half),
                },
                SensorVolume::Sphere { center, radius } => SensorShape::Sphere {
                    center: Vec3::from_array(center),
                    radius,
                },
            },
            occupied: s.occupants > 0,
        }));
}

fn announce_bridge() {
    bridge::push_event(Event::Ready {
        protocol: bridge::PROTOCOL,
    });
}

impl SimClock {
    /// Ticks in one run: a loaded track's 10 s, or the scenario's [`Scenario::run_ticks`]. The
    /// editor never restarts, loaded track or not, so its edits keep their incremental colliders.
    fn run_ticks(&self) -> u32 {
        match &self.tracks.loaded {
            Some(_) if self.scenario != Scenario::Editor => RAMP_10S_TICKS,
            _ => self.scenario.run_ticks(),
        }
    }

    /// A fresh run of the loaded track, or of the scenario when none is loaded.
    fn build_run(&self) -> TrackRun {
        recording(match &self.tracks.loaded {
            Some(f) => TrackRun::new(&f.track).expect("loadTrack refuses tracks that cannot run"),
            None => self.scenario.build(self.marble, self.marble_size),
        })
    }

    /// Start the live sim over at tick 0 of [`Self::build_run`].
    fn restart(&mut self, frame: &mut MarbleFrame, parts: &mut PartFrame) {
        self.sim = self.build_run();
        self.acc = 0.0;
        self.runs = 0;
        self.last_run_hash = None;
        self.restarts += 1;
        let now = self.sim.marbles();
        frame.prev = now.clone();
        frame.curr = now;
        frame.alpha = 0.0;
        parts.curr = part_angles(&self.sim);
        parts.prev = parts.curr.clone();
    }
}

/// Answers the commands the overlay posted and publishes this frame's tick and smoothed fps. Runs
/// after [`advance_sim`], so commands that replace the sim apply between ticks.
fn pump_bridge(
    time: Res<Time>,
    mut clock: ResMut<SimClock>,
    mut frame: ResMut<MarbleFrame>,
    mut parts: ResMut<PartFrame>,
    mut fps: Local<f32>,
) {
    for cmd in bridge::take_commands() {
        if let Command::Ping { id } = cmd {
            bridge::push_event(Event::Pong { id });
            continue;
        }
        if let Some((event, after)) = clock.tracks.apply(cmd) {
            if after == After::Restart {
                clock.restart(&mut frame, &mut parts);
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

/// A live run records its event stream for audio (read-only, so the run is unchanged).
fn recording(mut run: TrackRun) -> TrackRun {
    run.set_event_recording(true);
    run
}

/// Drains the frame's sim events into one [`audio::AudioFrame`] with the camera as rendered this
/// frame. Runs after transform propagation, so the director's camera pose is current.
fn pump_audio(
    mut clock: ResMut<SimClock>,
    look: Option<Res<MarbleLook>>,
    cameras: Query<&GlobalTransform, With<MainCamera>>,
) {
    let events = clock.sim.drain_events();
    // A loaded track releases standard glass marbles; the scenario looks do not apply to it.
    let kinds: &[(MarbleKind, f32)] = match (&clock.tracks.loaded, &look) {
        (None, Some(look)) => &look.0,
        _ => &[],
    };
    let kinds: Vec<MarbleKind> = kinds.iter().map(|l| l.0).collect();
    let marbles = audio::marbles_at(&clock.sim, &kinds);
    let piece = |id| clock.sim.mechanism_piece(id).map(|p| p.as_str().to_owned());
    let mut frame = audio::coalesce(&events, &marbles, piece, clock.sim.tick_count());
    frame.camera = cameras.single().ok().map(|t| audio::AudioCamera {
        position: t.translation().to_array(),
        forward: t.forward().to_array(),
        up: t.up().to_array(),
    });
    bridge::push_event(Event::AudioFrame(frame));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_exit_codes_reach_the_process() {
        assert_eq!(process_code(&AppExit::Success), 0);
        assert_eq!(process_code(&AppExit::from_code(3)), 3);
        assert_eq!(process_code(&AppExit::from_code(0)), 0);
    }

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
        assert_eq!(cap_to_run(12, 2_395, RAMP_10S_TICKS), 5);
        assert_eq!(cap_to_run(12, 2_400, RAMP_10S_TICKS), 0);
        assert_eq!(cap_to_run(3, 0, RAMP_10S_TICKS), 3);
        let mut sim = Sim::ramp_scene();
        while sim.tick_count() < u64::from(RAMP_10S_TICKS) {
            sim.tick(cap_to_run(
                MAX_TICKS_PER_FRAME,
                sim.tick_count(),
                RAMP_10S_TICKS,
            ));
        }
        assert_eq!(sim.trajectory_hash(), parble_sim::golden::ramp_10s_hash());
    }

    #[test]
    fn lockstep_holds_then_steps_a_fixed_count() {
        assert_eq!(lockstep_ticks(1.0, 3.0), 0);
        assert_eq!(lockstep_ticks(3.0, 3.0), LOCKSTEP_TICKS);
        // 2,400 ticks is a whole number of lockstep frames, so a run still ends on the golden hash.
        assert_eq!(RAMP_10S_TICKS % LOCKSTEP_TICKS, 0);
        assert_eq!(Scenario::Playroom.run_ticks() % LOCKSTEP_TICKS, 0);
        assert_eq!(Scenario::AllPieces.run_ticks() % LOCKSTEP_TICKS, 0);
    }

    #[test]
    fn a_loaded_track_restarts_every_10_s_except_in_the_editor() {
        let fixtures: serde_json::Value =
            serde_json::from_str(include_str!("../../../web/ui/src/parble-fixtures.json")).unwrap();
        let file = parble_core::file::load(&fixtures["valid"][1]["file"].to_string()).unwrap();
        let clock = |scenario: Scenario| SimClock {
            sim: TrackRun::new(&file.track).unwrap(),
            scenario,
            tracks: Tracks {
                loaded: Some(file.clone()),
                last_run: None,
            },
            marble: None,
            marble_size: MarbleSize::Standard,
            acc: 0.0,
            runs: 0,
            last_run_hash: None,
            restarts: 0,
        };
        assert_eq!(clock(Scenario::Ramp).run_ticks(), RAMP_10S_TICKS);
        assert_eq!(clock(Scenario::Editor).run_ticks(), u32::MAX);
        let mut c = clock(Scenario::Editor);
        c.restart(&mut MarbleFrame::default(), &mut PartFrame::default());
        assert_eq!(c.restarts, 1);
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
