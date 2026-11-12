//! The main camera: the run director ([`crate::director`]), which blends between scripted cues and
//! camera zones, and an orbit camera for the editor. Render-side only: it reads [`MarbleFrame`],
//! the app's [`DirectorClock`] and [`CameraZones`] and writes the camera's transform, field of
//! view and focus. Nothing here reaches the sim.
//!
//! The default shot is the low, close tracking shot behind the marble, with the DoF focus on the
//! marble. Every shot change is a smooth blend. Only a switch of camera mode (`C`, `F`, `Esc`)
//! snaps, and resets the TAA history so the previous view never ghosts into the next.
//!
//! `F` switches to a free-fly camera from the current view ([`Fly`], `docs/agent-operability.md`)
//! and `F` or `Esc` hands back to the camera it replaced. It moves only on input, so agent runs with
//! no input frame exactly as before.
//!
//! With [`FlyOnly`] (the mechanism showcase's default) the camera starts in fly at the scenario's
//! opening shot and stays there: `C`, `F` and `Esc` never hand it to the director or the orbit.

use std::collections::VecDeque;

use bevy::{
    anti_alias::taa::TemporalAntiAliasing,
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    post_process::dof::DepthOfField,
    prelude::*,
    transform::TransformSystems,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use parble_sim::{MarbleState, scene};
use serde::Serialize;

use crate::{MarbleFrame, Quality, director, post};

use crate::director::{
    Director, DirectorDiag, Focus, Frame, HiddenPath, MarbleIn, Rule, Solid, Zone,
};
pub use crate::director::{
    FOLLOW_AHEAD_M, FOLLOW_BACK_M, FOLLOW_EYE_SMOOTH_S, FOLLOW_LOOK_SMOOTH_S, FOLLOW_MAX_YAW_RATE,
    FOLLOW_UP_M, FOV_DEG, smooth_damp,
};

/// Screen positions of the marble kept for the ghost check, newest first.
pub const TRAIL_FRAMES: usize = 8;

/// One camera shot.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Shot {
    /// Low, close tracking shot behind the marble.
    Follow,
    /// Fixed camera; the marble crosses the frame.
    Fixed { eye: [f32; 3], look: [f32; 3] },
    /// Fixed set shot (the playroom views): the DoF focus stays on the set at `focus_m` instead of
    /// following the marble.
    Set {
        eye: [f32; 3],
        look: [f32; 3],
        #[serde(rename = "focusM")]
        focus_m: f32,
    },
}

impl Shot {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Follow => "follow",
            Self::Fixed { .. } => "fixed",
            Self::Set { .. } => "set",
        }
    }

    /// The director rule that plays this cue.
    pub fn rule(&self) -> Rule {
        match *self {
            Self::Follow => Rule::FOLLOW,
            Self::Fixed { eye, look } => Rule::Fixed {
                eye,
                look,
                focus: Focus::Marble,
                fov: None,
            },
            Self::Set { eye, look, focus_m } => Rule::Fixed {
                eye,
                look,
                focus: Focus::Distance { m: focus_m },
                fov: None,
            },
        }
    }
}

/// The camera zones of the loaded track or scenario, in metres. The app fills it from the track
/// model (authored zones and piece hints); the director tests the followed marble against it.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct CameraZones(pub Vec<Zone>);

/// Points of interest for the follow shot's action window (mechanism centres, metres). The app
/// fills it from the track; the window takes in those near the marble's path.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct CameraInterest(pub Vec<Vec3>);

/// The track's pieces that hide the marble (with their exits) and its solid pieces that can block
/// the view, in metres. The app fills it from the track model; the follow shot reads it only.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct CameraObstacles {
    pub hidden: Vec<HiddenPath>,
    pub solids: Vec<Solid>,
}

/// Scripted cues: at sim tick `.0` of every run, blend to shot `.1`. Sorted by tick; the first cue
/// should be at tick 0.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct Cues(pub Vec<(u64, Shot)>);

impl Cues {
    /// The shot active at `tick` and its cue index.
    pub fn at(&self, tick: u64) -> Option<(usize, Shot)> {
        self.0
            .iter()
            .enumerate()
            .rev()
            .find(|(_, (t, _))| *t <= tick)
            .map(|(i, (_, s))| (i, *s))
    }

    /// Ramp scenario: follow down the ramp and along the floor, cut to a trackside shot as the
    /// marble nears the end wall, cut back to the follow shot for the rebound.
    pub fn ramp() -> Self {
        let len = scene::tray_length_m();
        Self(vec![
            (0, Shot::Follow),
            (
                RAMP_TRACKSIDE_TICK,
                Shot::Fixed {
                    // Steep: the 60 mm walls hide the 28 mm channel from shallow angles.
                    eye: [len * 0.81, 0.25, 0.04],
                    look: [len * 0.81, 0.0, 0.0],
                },
            ),
            (RAMP_TRACKSIDE_TICK + 120, Shot::Follow),
        ])
    }

    /// Look-dev close-up (P1-T6 `--scenario closeup`): one fixed shot of the resting marble from
    /// above and to the side, close enough to judge its material.
    pub fn closeup() -> Self {
        let m = scene::closeup_marble_m(scene::standard_marble_radius_m());
        Self(vec![(
            0,
            Shot::Fixed {
                eye: [m.x - 0.045, m.y + 0.075, m.z + 0.012],
                look: [m.x, m.y, m.z],
            },
        )])
    }

    /// Marble lineup (P5-T1 `--scenario lineup`): one set shot from in front of and above the row,
    /// wide enough for the whole row, focused on its centre.
    pub fn lineup() -> Self {
        let n = parble_sim::marble::LINEUP.len();
        let mid = scene::lineup_marble_m(n / 2, n, scene::standard_marble_radius_m());
        let eye = Vec3::new(mid.x, 0.08, mid.z + 0.235);
        let look = Vec3::new(mid.x, mid.y * 0.75, mid.z);
        Self(vec![(
            0,
            Shot::Set {
                eye: eye.to_array(),
                look: look.to_array(),
                focus_m: eye.distance(look),
            },
        )])
    }

    /// Mechanism showcase (P2-T6 `--scenario mechanisms`): a set shot from in front of and above
    /// the row, the toggle, gate and lift lanes in frame, focused on its middle. At
    /// `ROPE_CUT_TICK` it cuts to a close set shot looking down the rope lane (−Z) into the arch,
    /// where the ropes hang under the bridge, as the marble rolls in, and at `ROPE_RETURN_TICK`
    /// back to the row.
    pub fn mechanisms() -> Self {
        use parble_sim::mech_showcase::{
            LIFT_CELL, ROPE_CELL, ROPE_CUT_TICK, ROPE_RETURN_TICK, TOGGLE_CELL,
        };
        let cell = scene::cell_m();
        let at = |c: [i32; 3]| Vec3::new(c[0] as f32, 0.0, c[2] as f32) * cell;
        let set = |look: Vec3, eye: Vec3| Shot::Set {
            eye: eye.to_array(),
            look: look.to_array(),
            focus_m: eye.distance(look),
        };
        // From the toggle lane's first straight to the straight on the lift's high exit.
        let first = at(TOGGLE_CELL) - Vec3::X * cell;
        let last = at(LIFT_CELL) + Vec3::X * cell * 2.0;
        let look = (first + last) / 2.0 + Vec3::Y * 0.04;
        let row = set(look, look + Vec3::new(0.0, 0.60, 0.26));
        let rope = at(ROPE_CELL) + Vec3::Y * 0.03;
        Self(vec![
            (0, row),
            (ROPE_CUT_TICK, set(rope, rope + Vec3::new(0.0, 0.09, 0.15))),
            (ROPE_RETURN_TICK, row),
        ])
    }

    /// All-pieces track (P2-T7 `--scenario allpieces`): the follow shot, which the track's camera
    /// zones (its demo zone over the tower drop and its pieces' hints) take over on the way.
    pub fn allpieces() -> Self {
        Self(vec![(0, Shot::Follow)])
    }

    /// All-pieces overview (`--scenario allpieces --camera overview`): one set shot from in front
    /// of and above the whole track, focused on its middle.
    pub fn allpieces_overview() -> Self {
        let look = Vec3::new(0.72, 0.18, -0.36);
        let eye = look + Vec3::new(0.0, 0.95, 1.70);
        Self(vec![(
            0,
            Shot::Set {
                eye: eye.to_array(),
                look: look.to_array(),
                focus_m: eye.distance(look),
            },
        )])
    }

    /// One playroom set shot (`--scenario playroom --view rug|blocks`).
    pub fn playroom(view: crate::PlayroomView) -> Self {
        let (eye, look) = view.eye_target();
        Self(vec![(
            0,
            Shot::Set {
                eye: eye.to_array(),
                look: look.to_array(),
                focus_m: view.focus_m(),
            },
        )])
    }

    /// Playroom kit run (`--scenario playroom` without `--view`): follow the lead marble out of the
    /// start and round both curves, then cut to a set shot over the whole run as the marbles reach
    /// the finish stop.
    pub fn playroom_run() -> Self {
        let cell = scene::cell_m();
        let finish = Vec3::new(0.0, 0.05, parble_sim::playroom_run::ROW_FAR as f32 * cell);
        let eye = Vec3::new(-0.20, 0.36, 0.30);
        Self(vec![
            (0, Shot::Follow),
            (
                PLAYROOM_FINISH_TICK,
                Shot::Set {
                    eye: eye.to_array(),
                    look: [0.22, 0.0, -0.12],
                    focus_m: eye.distance(finish),
                },
            ),
        ])
    }

    /// The kit showcase grid (P1-T4) in a high three-quarter view from its near corner across the
    /// diagonal: the near pieces draw LOD0 with their half-cell gaps and the far corner LOD2, with
    /// both crossfade bands in frame.
    pub fn kit() -> Self {
        let eye = Vec3::from_array(KIT_EYE);
        let look = Vec3::from_array(KIT_LOOK);
        Self(vec![(
            0,
            Shot::Set {
                eye: KIT_EYE,
                look: KIT_LOOK,
                focus_m: eye.distance(look),
            },
        )])
    }

    /// One fixed overview of the whole tray (`--camera overview`).
    pub fn overview() -> Self {
        let len = scene::tray_length_m();
        Self(vec![(
            0,
            Shot::Fixed {
                eye: [len * 0.5, 0.80, 0.13],
                look: [len * 0.5, scene::ramp_height_m() * 0.35, 0.0],
            },
        )])
    }
}

/// The kit2000 set shot's eye and look point (metres): beside the grid's near corner, so every
/// type and wood variant has LOD0 pieces in frame while the rest of the grid, and its LOD2 far
/// side, still fits (`e2e/tests/kit.spec.mjs` asserts both).
pub const KIT_EYE: [f32; 3] = [-2.40, 0.50, -0.10];
pub const KIT_LOOK: [f32; 3] = [-1.04, 0.12, -1.01];

/// Tick of the playroom run's cut to the finish shot (2.5 s, just before the lead marble arrives).
pub const PLAYROOM_FINISH_TICK: u64 = 600;

/// Tick of the ramp scenario's trackside cut.
pub const RAMP_TRACKSIDE_TICK: u64 = 140;

/// Written by the app every frame: the sim tick and completed runs at the rendered state, and the
/// frame's camera time step (seconds).
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct DirectorClock {
    pub tick: u64,
    pub run: u32,
    pub dt: f32,
}

/// Director (cued shots) or orbit (editor). `C` toggles. `F` enters fly, and `F` or `Esc` leaves it,
/// except under [`FlyOnly`].
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CameraMode {
    #[default]
    Director,
    Orbit,
    /// Free-fly ([`Fly`]).
    Fly,
}

/// Fly-only camera: the camera starts in [`CameraMode::Fly`] and no key leaves it. The mechanism
/// showcase uses it by default; `--camera director` turns it off.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FlyOnly(pub bool);

/// Free-fly speed at entry (m/s), its wheel range, and the factor per wheel line.
pub const FLY_SPEED_M_S: f32 = 0.3;
/// Fly focuses this far when the view centre meets no track, marble or floor (metres).
pub const FLY_FAR_FOCUS_M: f32 = 3.0;
/// Fly's focus eases to a new distance with this time constant (seconds).
const FLY_FOCUS_EASE_S: f32 = 0.15;
pub const FLY_SPEED_RANGE: (f32, f32) = (0.02, 5.0);
const FLY_WHEEL_STEP: f32 = 1.25;
/// Look sensitivity, radians per pixel of drag.
const FLY_LOOK_RAD_PER_PX: f32 = 0.004;

/// Free-fly camera state: eye, yaw about +Y (0 looks down −Z) and pitch (positive looks up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fly {
    pub eye: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub speed: f32,
    /// The mode `F` or `Esc` returns to.
    pub back: CameraMode,
}

impl Default for Fly {
    fn default() -> Self {
        Self {
            eye: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            speed: FLY_SPEED_M_S,
            back: CameraMode::Director,
        }
    }
}

impl Fly {
    /// A fly camera at `eye` looking along `forward`.
    pub fn from_view(eye: Vec3, forward: Vec3, back: CameraMode) -> Self {
        let f = forward.normalize_or(Vec3::NEG_Z);
        Self {
            eye,
            yaw: (-f.x).atan2(-f.z),
            pitch: f.y.clamp(-1.0, 1.0).asin(),
            back,
            ..default()
        }
    }

    /// A fly camera at the eye of `cues`' opening shot, looking at its look point, or at the default
    /// orbit view when the opening shot follows the marble.
    pub fn from_opening(cues: &Cues, back: CameraMode) -> Self {
        let (eye, look) = match cues.at(0).map(|(_, s)| s) {
            Some(Shot::Fixed { eye, look } | Shot::Set { eye, look, .. }) => {
                (Vec3::from_array(eye), Vec3::from_array(look))
            }
            _ => {
                let o = Orbit::default();
                (o.eye(), o.target)
            }
        };
        Self::from_view(eye, look - eye, back)
    }

    pub fn rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0)
    }

    /// Move by `axes` (x right, y up, z forward, each in −1..1) for `dt` seconds. Forward and right
    /// follow the view; up is world +Y.
    pub fn step(&mut self, axes: Vec3, dt: f32) {
        let r = self.rotation();
        let v = r * Vec3::X * axes.x + Vec3::Y * axes.y + r * Vec3::NEG_Z * axes.z;
        self.eye += v.clamp_length_max(1.0) * self.speed * dt;
    }

    /// Turn by a drag of `px` pixels.
    pub fn look(&mut self, px: Vec2) {
        self.yaw -= px.x * FLY_LOOK_RAD_PER_PX;
        self.pitch = (self.pitch - px.y * FLY_LOOK_RAD_PER_PX).clamp(-1.5, 1.5);
    }

    /// Scale the speed by `lines` wheel lines (up is faster).
    pub fn wheel(&mut self, lines: f32) {
        self.speed =
            (self.speed * FLY_WHEEL_STEP.powf(lines)).clamp(FLY_SPEED_RANGE.0, FLY_SPEED_RANGE.1);
    }
}

/// Marks the main camera.
#[derive(Component, Debug, Clone, Copy)]
pub struct MainCamera;

/// Orbit camera state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orbit {
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub radius: f32,
}

impl Default for Orbit {
    fn default() -> Self {
        Self {
            target: Vec3::new(scene::tray_length_m() * 0.5, 0.03, 0.0),
            yaw: -0.6,
            pitch: 0.55,
            radius: 0.7,
        }
    }
}

impl Orbit {
    pub fn eye(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        self.target + self.radius * Vec3::new(cp * sy, sp, cp * cy)
    }
}

/// Editor camera input (P3-T4). When `active`, the orbit camera leaves the left button to the
/// editor tools: right drag orbits, middle drag or Shift + right drag pans along the ground, and
/// the wheel zooms. `set` replaces the orbit once (the agent's `camera` action).
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct EditorCamera {
    pub active: bool,
    pub set: Option<Orbit>,
}

/// The orbit state in the state JSON (`camera.orbit`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrbitDiag {
    pub target: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub radius: f32,
}

#[derive(Resource, Debug, Default)]
struct DirectorState {
    director: Director,
    orbit: Orbit,
    fly: Fly,
    trail: VecDeque<[f32; 2]>,
    /// The mode last framed, to reset TAA when the mode changes.
    framed: Option<CameraMode>,
    /// Fly's eased focus distance (metres); 0 until fly first frames.
    fly_focus: f32,
}

/// The camera's state for the state JSON (`camera`).
#[derive(Resource, Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraDiag {
    pub mode: Option<CameraMode>,
    /// The scripted cue playing (the director may be in a zone over it, see `director`).
    pub shot: Option<Shot>,
    /// Hard cuts since start: only camera mode switches cut, so the director adds none.
    pub cuts: u32,
    /// The director: active shot rule, source (cue or zone), zone, blend progress, followed marble.
    pub director: DirectorDiag,
    /// Vertical field of view this frame (degrees).
    pub fov_deg: f32,
    /// Whether the camera zone overlay is drawn (`V`, `--zones`).
    pub zone_overlay: bool,
    pub eye: [f32; 3],
    /// Unit view direction.
    pub forward: [f32; 3],
    /// Free-fly speed (m/s); it applies once `mode` is `fly`.
    pub fly_speed: f32,
    /// [`FlyOnly`]: the camera stays in fly, and `C`, `F` and `Esc` do not leave it.
    pub fly_only: bool,
    /// Marble centre in physical pixels, newest first, up to [`TRAIL_FRAMES`].
    pub marble_px: Vec<[f32; 2]>,
    /// Marble radius in physical pixels.
    pub marble_radius_px: f32,
    pub post: post::LivePost,
    pub orbit: OrbitDiag,
}

/// Spawn the main camera with its tier's post stack. The director moves it from the first frame.
pub fn spawn_main_camera(commands: &mut Commands, quality: Quality, o: &post::PostOverrides) {
    let start = Orbit::default();
    let mut cam = commands.spawn((
        Camera3d::default(),
        MainCamera,
        Projection::Perspective(PerspectiveProjection {
            fov: FOV_DEG.to_radians(),
            near: 0.005,
            ..default()
        }),
        Transform::from_translation(start.eye()).looking_at(start.target, Vec3::Y),
    ));
    post::insert_post(&mut cam, quality, o);
}

pub struct DirectorPlugin {
    pub mode: CameraMode,
    pub cues: Cues,
    /// Start in fly at the cues' opening shot and stay there ([`FlyOnly`]); `mode` is then ignored.
    pub fly_only: bool,
}

impl Plugin for DirectorPlugin {
    fn build(&self, app: &mut App) {
        let mut state = DirectorState::default();
        let mode = if self.fly_only {
            state.fly = Fly::from_opening(&self.cues, CameraMode::Fly);
            CameraMode::Fly
        } else {
            self.mode
        };
        app.insert_resource(mode)
            .insert_resource(FlyOnly(self.fly_only))
            .insert_resource(self.cues.clone())
            .init_resource::<DirectorClock>()
            .insert_resource(state)
            .init_resource::<CameraDiag>()
            .init_resource::<EditorCamera>()
            .init_resource::<CameraZones>()
            .init_resource::<CameraInterest>()
            .init_resource::<CameraObstacles>()
            .add_systems(
                Update,
                (toggle_mode, fly_toggle, orbit_input, fly_input).chain(),
            )
            .add_systems(PostUpdate, direct.before(TransformSystems::Propagate));
    }
}

fn toggle_mode(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    fly_only: Res<FlyOnly>,
    mut mode: ResMut<CameraMode>,
) {
    if !fly_only.0 && keys.is_some_and(|k| k.just_pressed(KeyCode::KeyC)) {
        *mode = match *mode {
            CameraMode::Director => CameraMode::Orbit,
            CameraMode::Orbit => CameraMode::Director,
            CameraMode::Fly => CameraMode::Fly,
        };
    }
}

/// `F` enters fly from the current view and locks the cursor; `F` or `Esc` returns to the mode it
/// replaced and releases it. Under [`FlyOnly`] neither key does anything: the mouse looks by drag.
fn fly_toggle(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    fly_only: Res<FlyOnly>,
    mut mode: ResMut<CameraMode>,
    mut st: ResMut<DirectorState>,
    cams: Query<&Transform, With<MainCamera>>,
    mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let Some(keys) = keys.filter(|_| !fly_only.0) else {
        return;
    };
    let mut lock = |locked: bool| {
        if let Ok(mut c) = cursors.single_mut() {
            c.grab_mode = if locked {
                CursorGrabMode::Locked
            } else {
                CursorGrabMode::None
            };
            c.visible = !locked;
        }
    };
    if *mode == CameraMode::Fly {
        if keys.any_just_pressed([KeyCode::KeyF, KeyCode::Escape]) {
            *mode = st.fly.back;
            // Hand back to the director with a snap to its current shot.
            st.director.reset();
            lock(false);
        }
    } else if keys.just_pressed(KeyCode::KeyF)
        && let Ok(tf) = cams.single()
    {
        let speed = st.fly.speed;
        st.fly = Fly {
            speed,
            ..Fly::from_view(tf.translation, tf.forward().as_vec3(), *mode)
        };
        *mode = CameraMode::Fly;
        lock(true);
    }
}

/// Fly input: WASD, Q/E or Shift/Space down and up, left or right drag to look, wheel for speed.
/// Real time, so lockstep's held sim does not freeze the camera.
#[allow(clippy::too_many_arguments)]
fn fly_input(
    mode: Res<CameraMode>,
    time: Res<Time<Real>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    scroll: Option<Res<AccumulatedMouseScroll>>,
    motion: Option<Res<AccumulatedMouseMotion>>,
    windows: Query<(&Window, &CursorOptions), With<PrimaryWindow>>,
    mut last_cursor: Local<Option<Vec2>>,
    mut st: ResMut<DirectorState>,
) {
    if *mode != CameraMode::Fly {
        *last_cursor = None;
        return;
    }
    let dt = time.delta_secs().min(0.1);
    if let Some(k) = keys {
        let axis = |pos: &[KeyCode], neg: &[KeyCode]| {
            f32::from(u8::from(k.any_pressed(pos.iter().copied())))
                - f32::from(u8::from(k.any_pressed(neg.iter().copied())))
        };
        let axes = Vec3::new(
            axis(&[KeyCode::KeyD], &[KeyCode::KeyA]),
            axis(
                &[KeyCode::KeyE, KeyCode::Space],
                &[KeyCode::KeyQ, KeyCode::ShiftLeft, KeyCode::ShiftRight],
            ),
            axis(&[KeyCode::KeyW], &[KeyCode::KeyS]),
        );
        st.fly.step(axes, dt);
    }
    let locked = windows
        .single()
        .is_ok_and(|(_, c)| c.grab_mode == CursorGrabMode::Locked);
    if locked
        && let Some(m) = motion
        && m.delta != Vec2::ZERO
    {
        // Pointer lock: the mouse looks without a drag, from raw motion (the cursor stays put).
        st.fly.look(m.delta);
    }
    // Without the lock, cursor deltas while dragging: the browser reports them without pointer lock.
    let cursor = windows.single().ok().and_then(|(w, _)| w.cursor_position());
    let dragging =
        !locked && buttons.is_some_and(|b| b.any_pressed([MouseButton::Left, MouseButton::Right]));
    if let (true, Some(now), Some(before)) = (dragging, cursor, *last_cursor) {
        st.fly.look(now - before);
    }
    *last_cursor = if dragging { cursor } else { None };
    if let Some(s) = scroll {
        let lines = match s.unit {
            bevy::input::mouse::MouseScrollUnit::Line => s.delta.y,
            bevy::input::mouse::MouseScrollUnit::Pixel => s.delta.y / 40.0,
        };
        if lines != 0.0 {
            st.fly.wheel(lines);
        }
    }
}

/// Left drag orbits, the wheel zooms. With [`EditorCamera::active`] see its input scheme.
#[allow(clippy::too_many_arguments)]
fn orbit_input(
    mode: Res<CameraMode>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    motion: Option<Res<AccumulatedMouseMotion>>,
    scroll: Option<Res<AccumulatedMouseScroll>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut editor: ResMut<EditorCamera>,
    mut last_cursor: Local<Option<Vec2>>,
    mut st: ResMut<DirectorState>,
) {
    if let Some(o) = editor.set.take() {
        st.orbit = o;
    }
    if *mode != CameraMode::Orbit {
        *last_cursor = None;
        return;
    }
    if editor.active {
        // Cursor deltas: the browser reports them without pointer lock.
        let cursor = windows.single().ok().and_then(Window::cursor_position);
        let (right, middle) = buttons.as_ref().map_or((false, false), |b| {
            (
                b.pressed(MouseButton::Right),
                b.pressed(MouseButton::Middle),
            )
        });
        let shift = keys
            .as_ref()
            .is_some_and(|k| k.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]));
        let dragging = right || middle;
        if let (true, Some(now), Some(before)) = (dragging, cursor, *last_cursor) {
            let d = now - before;
            let o = &mut st.orbit;
            if middle || shift {
                let (sy, cy) = o.yaw.sin_cos();
                let right_v = Vec3::new(cy, 0.0, -sy);
                let fwd = Vec3::new(-sy, 0.0, -cy);
                let k = o.radius * 0.0015;
                o.target += (-right_v * d.x + fwd * d.y) * k;
            } else {
                o.yaw -= d.x * 0.005;
                o.pitch = (o.pitch + d.y * 0.005).clamp(0.05, 1.5);
            }
        }
        *last_cursor = if dragging { cursor } else { None };
        if let Some(s) = scroll {
            let lines = match s.unit {
                bevy::input::mouse::MouseScrollUnit::Line => s.delta.y,
                bevy::input::mouse::MouseScrollUnit::Pixel => s.delta.y / 40.0,
            };
            st.orbit.radius = (st.orbit.radius * (1.0 - lines * 0.1)).clamp(0.08, 8.0);
        }
        return;
    }
    let o = &mut st.orbit;
    if let (Some(b), Some(m)) = (buttons, motion)
        && b.pressed(MouseButton::Left)
    {
        o.yaw -= m.delta.x * 0.005;
        o.pitch = (o.pitch + m.delta.y * 0.005).clamp(0.05, 1.5);
    }
    if let Some(s) = scroll {
        o.radius = (o.radius * (1.0 - s.delta.y * 0.1)).clamp(0.08, 3.0);
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn direct(
    mode: Res<CameraMode>,
    fly_only: Res<FlyOnly>,
    cues: Res<Cues>,
    zones: Res<CameraZones>,
    interest: Res<CameraInterest>,
    obstacles: Res<CameraObstacles>,
    overlay: Option<Res<crate::zone_overlay::ZoneOverlay>>,
    clock: Res<DirectorClock>,
    frame: Res<MarbleFrame>,
    mut st: ResMut<DirectorState>,
    mut diag: ResMut<CameraDiag>,
    mut cams: Query<
        (
            &Camera,
            &mut Transform,
            &mut Projection,
            Option<&mut DepthOfField>,
            Option<&mut TemporalAntiAliasing>,
            (
                &Msaa,
                Option<&bevy::pbr::ScreenSpaceAmbientOcclusion>,
                Option<&bevy::post_process::bloom::Bloom>,
                Option<&bevy::core_pipeline::tonemapping::Tonemapping>,
            ),
        ),
        With<MainCamera>,
    >,
) {
    let Ok((camera, mut tf, mut proj, dof, mut taa, (msaa, ssao, bloom, tm))) = cams.single_mut()
    else {
        return;
    };
    let t = frame.alpha;
    let marbles: Vec<MarbleIn> = frame
        .curr
        .iter()
        .enumerate()
        .map(|(i, curr): (usize, &MarbleState)| {
            let prev = frame.prev.get(i).unwrap_or(curr);
            MarbleIn {
                p: Vec3::from_array(prev.position).lerp(Vec3::from_array(curr.position), t),
                v: Vec3::from_array(prev.linvel).lerp(Vec3::from_array(curr.linvel), t),
            }
        })
        .collect();
    let has_taa = taa.is_some();
    let aspect = match &*proj {
        Projection::Perspective(p) => p.aspect_ratio,
        _ => 16.0 / 9.0,
    };

    if st.framed.is_some_and(|m| m != *mode) {
        // A user switch of camera mode: the one hard cut. The director snaps when it is back.
        st.director.reset();
        if let Some(t) = taa.as_deref_mut() {
            t.reset = true;
        }
        st.trail.clear();
        diag.cuts += 1;
    }

    let cue = cues.at(clock.tick);
    let still = |eye: Vec3, forward: Vec3| director::Pose {
        eye,
        forward,
        fov: FOV_DEG.to_radians(),
        focus: 0.0,
    };
    let pose = match *mode {
        CameraMode::Fly => Some(still(st.fly.eye, st.fly.rotation() * Vec3::NEG_Z)),
        CameraMode::Orbit => {
            let (eye, look) = (st.orbit.eye(), st.orbit.target);
            Some(still(eye, (look - eye).normalize_or(Vec3::NEG_Z)))
        }
        CameraMode::Director => st.director.update(
            &Frame {
                marbles: &marbles,
                dt: clock.dt.max(0.0),
                aspect,
                run: clock.run,
                interest: &interest.0,
                hidden: &obstacles.hidden,
                solids: &obstacles.solids,
            },
            cue.map(|(i, s)| (i, s.rule())),
            &zones.0,
        ),
    };
    // The editor's empty track has no marble: orbit and fly still frame, the director waits.
    let Some(pose) = pose else {
        return;
    };
    st.framed = Some(*mode);

    *tf = Transform::from_translation(pose.eye).looking_to(pose.forward, Vec3::Y);
    if let Projection::Perspective(p) = &mut *proj
        && (p.fov - pose.fov).abs() > 1e-6
    {
        p.fov = pose.fov;
    }
    let forward = tf.forward().as_vec3();
    let followed = st
        .director
        .diag
        .followed
        .and_then(|i| marbles.get(i))
        .or(marbles.first())
        .copied();
    let depth = match *mode {
        CameraMode::Director => pose.focus,
        // Fly focuses on what the view centre looks at.
        CameraMode::Fly => {
            let target = fly_focus(pose.eye, forward, &obstacles.solids, &marbles);
            st.fly_focus = if st.fly_focus > 0.0 {
                let k = 1.0 - (-clock.dt.max(0.0) / FLY_FOCUS_EASE_S).exp();
                st.fly_focus + (target - st.fly_focus) * k
            } else {
                target
            };
            st.fly_focus
        }
        // Orbit focuses on the marble, or on its pivot without one.
        CameraMode::Orbit => {
            let p = followed.map_or(st.orbit.target, |m| m.p);
            (p - pose.eye).dot(forward).max(0.01)
        }
    };
    if *mode != CameraMode::Fly {
        st.fly_focus = 0.0;
    }
    let mut dof = dof;
    if let Some(d) = dof.as_deref_mut() {
        d.focal_distance = depth;
    }

    // Screen-space marble for the ghost check (the transform is final for this frame).
    if let Some(m) = followed {
        let gt = GlobalTransform::from(*tf);
        let scale = camera.target_scaling_factor().unwrap_or(1.0);
        let r = scene::standard_marble_radius_m();
        if let (Ok(c), Ok(e)) = (
            camera.world_to_viewport(&gt, m.p),
            camera.world_to_viewport(&gt, m.p + tf.right().as_vec3() * r),
        ) {
            st.trail.push_front([c.x * scale, c.y * scale]);
            st.trail.truncate(TRAIL_FRAMES);
            diag.marble_radius_px = c.distance(e) * scale;
        }
    }
    let directing = *mode == CameraMode::Director;
    diag.mode = Some(*mode);
    diag.shot = cue.filter(|_| directing).map(|(_, s)| s);
    diag.director = if directing {
        st.director.diag.clone()
    } else {
        DirectorDiag::default()
    };
    diag.fov_deg = pose.fov.to_degrees();
    diag.zone_overlay = overlay.is_some_and(|o| o.0);
    diag.eye = pose.eye.to_array();
    diag.forward = forward.to_array();
    diag.fly_speed = st.fly.speed;
    diag.fly_only = fly_only.0;
    diag.orbit = OrbitDiag {
        target: st.orbit.target.to_array(),
        yaw: st.orbit.yaw,
        pitch: st.orbit.pitch,
        radius: st.orbit.radius,
    };
    diag.marble_px = st.trail.iter().copied().collect();
    diag.post = post::live_post((msaa, None, ssao, bloom, dof.as_deref(), tm));
    diag.post.taa = has_taa;
}

/// Distance along the view ray from `eye` (unit `forward`) to the first track solid, marble or the
/// floor (y = 0) it meets, or [`FLY_FAR_FOCUS_M`] when it meets none nearer.
pub fn fly_focus(eye: Vec3, forward: Vec3, solids: &[Solid], marbles: &[MarbleIn]) -> f32 {
    let mut t = FLY_FAR_FOCUS_M;
    if forward.y < -1e-6 && eye.y > 0.0 {
        t = t.min(eye.y / -forward.y);
    }
    // A box the eye is inside (flying through track) would focus at zero.
    for b in solids.iter().filter(|b| !b.contains(eye)) {
        let (mut t0, mut t1) = (0.0f32, t);
        let mut hit = true;
        for i in 0..3 {
            let (o, d, lo, hi) = (eye[i], forward[i], b.min[i], b.max[i]);
            if d.abs() < 1e-9 {
                if o < lo || o > hi {
                    hit = false;
                    break;
                }
            } else {
                let (ta, tb) = ((lo - o) / d, (hi - o) / d);
                t0 = t0.max(ta.min(tb));
                t1 = t1.min(ta.max(tb));
                if t0 > t1 {
                    hit = false;
                    break;
                }
            }
        }
        if hit {
            t = t.min(t0);
        }
    }
    let r = scene::standard_marble_radius_m();
    for m in marbles {
        let along = (m.p - eye).dot(forward);
        if along > 0.0 && along < t && (m.p - eye).length_squared() - along * along <= r * r {
            t = along;
        }
    }
    t.max(0.01)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fly_focus_meets_the_nearest_thing_ahead_or_the_far_distance() {
        let eye = Vec3::new(0.0, 0.5, 0.0);
        let solid = Solid {
            min: Vec3::new(-0.1, 0.0, -1.1),
            max: Vec3::new(0.1, 1.0, -1.0),
        };
        let marble = MarbleIn {
            p: Vec3::new(0.0, 0.5, -0.4),
            v: Vec3::ZERO,
        };
        // Straight ahead: the solid, or the marble in front of it.
        assert!((fly_focus(eye, Vec3::NEG_Z, &[solid], &[]) - 1.0).abs() < 1e-4);
        assert!((fly_focus(eye, Vec3::NEG_Z, &[solid], &[marble]) - 0.4).abs() < 0.02);
        // Looking away from the marble: not focused on it.
        assert!((fly_focus(eye, Vec3::Z, &[solid], &[marble]) - FLY_FAR_FOCUS_M).abs() < 1e-4);
        // Looking down 45 degrees at the floor.
        let down = Vec3::new(0.0, -1.0, 1.0).normalize();
        assert!((fly_focus(eye, down, &[], &[]) - 0.5 * 2f32.sqrt()).abs() < 1e-4);
    }

    #[test]
    fn cues_pick_the_latest_started_shot() {
        let c = Cues::ramp();
        assert_eq!(c.at(0).unwrap().1, Shot::Follow);
        assert_eq!(c.at(RAMP_TRACKSIDE_TICK).unwrap().0, 1);
        assert_eq!(c.at(RAMP_TRACKSIDE_TICK - 1).unwrap().0, 0);
        assert_eq!(c.at(10_000).unwrap().0, 2);
    }

    #[test]
    fn playroom_views_are_set_shots_focused_on_their_lantern() {
        for v in [crate::PlayroomView::Rug, crate::PlayroomView::Blocks] {
            let Some((0, Shot::Set { eye, look, focus_m })) = Cues::playroom(v).at(0) else {
                panic!("{v:?}: not a set shot");
            };
            let (e, l) = v.eye_target();
            assert_eq!((eye, look), (e.to_array(), l.to_array()));
            assert!(focus_m > 0.2 && focus_m < 3.0, "{v:?}: focus {focus_m}");
        }
    }

    #[test]
    fn fly_round_trips_a_view_and_moves_along_it() {
        let eye = Vec3::new(0.1, 0.2, 0.3);
        for fwd in [Vec3::NEG_Z, Vec3::X, Vec3::new(0.3, -0.4, -0.8).normalize()] {
            let f = Fly::from_view(eye, fwd, CameraMode::Director);
            assert!((f.rotation() * Vec3::NEG_Z).distance(fwd) < 1e-5, "{fwd}");
            let mut g = f;
            g.step(Vec3::Z, 1.0);
            assert!((g.eye - eye).distance(fwd * FLY_SPEED_M_S) < 1e-5);
            let mut h = f;
            h.step(Vec3::Y, 0.5);
            assert!((h.eye - eye).distance(Vec3::Y * FLY_SPEED_M_S * 0.5) < 1e-5);
        }
        let mut f = Fly::default();
        f.look(Vec2::new(100.0, 0.0));
        assert!(f.yaw < 0.0, "dragging right turns right");
        f.look(Vec2::new(0.0, 1.0e5));
        assert_eq!(f.pitch, -1.5);
        f.wheel(1.0);
        assert!(f.speed > FLY_SPEED_M_S);
        f.wheel(-100.0);
        assert_eq!(f.speed, FLY_SPEED_RANGE.0);
    }

    /// The key handling systems in a bare app, as [`DirectorPlugin`] sets their resources up.
    fn key_app(fly_only: bool) -> App {
        let mut app = App::new();
        let plugin = DirectorPlugin {
            mode: CameraMode::Director,
            cues: Cues::mechanisms(),
            fly_only,
        };
        let mut state = DirectorState::default();
        let mode = if fly_only {
            state.fly = Fly::from_opening(&plugin.cues, CameraMode::Fly);
            CameraMode::Fly
        } else {
            plugin.mode
        };
        app.insert_resource(mode)
            .insert_resource(FlyOnly(fly_only))
            .insert_resource(state)
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, (toggle_mode, fly_toggle).chain());
        app.world_mut().spawn((MainCamera, Transform::default()));
        app
    }

    fn press(app: &mut App, key: KeyCode) -> CameraMode {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        app.update();
        let mut k = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        k.release(key);
        k.clear();
        *app.world().resource::<CameraMode>()
    }

    #[test]
    fn fly_only_starts_in_fly_at_the_opening_shot() {
        let mut app = App::new();
        app.add_plugins(DirectorPlugin {
            mode: CameraMode::Director,
            cues: Cues::mechanisms(),
            fly_only: true,
        });
        assert_eq!(*app.world().resource::<CameraMode>(), CameraMode::Fly);
        assert_eq!(*app.world().resource::<FlyOnly>(), FlyOnly(true));
        let Some((0, Shot::Set { eye, look, .. })) = Cues::mechanisms().at(0) else {
            panic!("mechanisms opens on a set shot");
        };
        let (eye, look) = (Vec3::from_array(eye), Vec3::from_array(look));
        let fly = app.world().resource::<DirectorState>().fly;
        assert_eq!(fly.eye, eye);
        assert!((fly.rotation() * Vec3::NEG_Z).distance((look - eye).normalize()) < 1e-5);
        assert_eq!(fly.back, CameraMode::Fly);
    }

    #[test]
    fn fly_only_keys_never_reach_the_director() {
        let mut app = key_app(true);
        for key in [
            KeyCode::KeyC,
            KeyCode::KeyF,
            KeyCode::Escape,
            KeyCode::KeyF,
            KeyCode::KeyF,
            KeyCode::KeyC,
            KeyCode::Escape,
        ] {
            assert_eq!(press(&mut app, key), CameraMode::Fly, "{key:?}");
        }
    }

    #[test]
    fn without_fly_only_c_f_and_esc_switch_as_before() {
        let mut app = key_app(false);
        assert_eq!(*app.world().resource::<CameraMode>(), CameraMode::Director);
        assert_eq!(press(&mut app, KeyCode::KeyC), CameraMode::Orbit);
        assert_eq!(press(&mut app, KeyCode::KeyC), CameraMode::Director);
        assert_eq!(press(&mut app, KeyCode::KeyF), CameraMode::Fly);
        assert_eq!(press(&mut app, KeyCode::Escape), CameraMode::Director);
        assert_eq!(press(&mut app, KeyCode::KeyF), CameraMode::Fly);
        assert_eq!(press(&mut app, KeyCode::KeyF), CameraMode::Director);
        // The plugin keeps the requested mode when fly-only is off.
        let mut app = App::new();
        app.add_plugins(DirectorPlugin {
            mode: CameraMode::Orbit,
            cues: Cues::mechanisms(),
            fly_only: false,
        });
        assert_eq!(*app.world().resource::<CameraMode>(), CameraMode::Orbit);
        assert_eq!(*app.world().resource::<FlyOnly>(), FlyOnly(false));
    }

    #[test]
    fn fly_from_a_follow_opening_starts_at_the_orbit_view() {
        let f = Fly::from_opening(&Cues::allpieces(), CameraMode::Fly);
        assert_eq!(f.eye, Orbit::default().eye());
    }

    #[test]
    fn orbit_eye_is_at_radius() {
        let o = Orbit::default();
        assert!((o.eye().distance(o.target) - o.radius).abs() < 1e-5);
    }
}
