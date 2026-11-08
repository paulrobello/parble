//! The main camera: a follow-marble director that cuts between shots on scripted cues, and an
//! orbit camera for the editor. Render-side only: it reads [`MarbleFrame`] and the app's
//! [`DirectorClock`] and writes the camera's transform and focus. Nothing here reaches the sim.
//!
//! The follow shot is a low, close tracking shot behind the marble, a critically damped spring on
//! the eye and the look point. The DoF focus is the marble's view-space depth every frame. A cut
//! snaps the spring and resets the TAA history so the previous shot never ghosts into the next.
//!
//! `F` switches to a free-fly camera from the current view ([`Fly`], `docs/agent-operability.md`)
//! and `F` or `Esc` hands back to the camera it replaced. It moves only on input, so agent runs with
//! no input frame exactly as before.

use std::collections::VecDeque;

use bevy::{
    anti_alias::taa::TemporalAntiAliasing,
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    post_process::dof::DepthOfField,
    prelude::*,
    transform::TransformSystems,
    window::PrimaryWindow,
};
use parble_sim::{MarbleState, scene};
use serde::Serialize;

use crate::{MarbleFrame, Quality, post};

/// Vertical field of view of the main camera.
pub const FOV_DEG: f32 = 35.0;
/// Screen positions of the marble kept for the ghost check, newest first.
pub const TRAIL_FRAMES: usize = 8;

/// Follow shot: distance behind the marble, height above it, sideways offset, look-ahead (metres).
/// The side offset stays inside the 28 mm channel so the near wall never blocks the marble.
pub const FOLLOW_BACK_M: f32 = 0.14;
pub const FOLLOW_UP_M: f32 = 0.07;
pub const FOLLOW_SIDE_M: f32 = 0.010;
pub const FOLLOW_AHEAD_M: f32 = 0.05;
/// Spring smoothing times (seconds) of the eye and the look point. The spring targets lead by
/// `velocity × smoothing time`, which cancels the steady lag at constant speed, so the springs
/// only damp changes (the ramp foot, wall rebounds).
pub const FOLLOW_EYE_SMOOTH_S: f32 = 0.15;
pub const FOLLOW_LOOK_SMOOTH_S: f32 = 0.06;
/// Below this speed the follow direction holds its last value (m/s).
const DIR_MIN_SPEED: f32 = 0.05;

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
}

/// Scripted cues: at sim tick `.0` of every run, cut to shot `.1`. Sorted by tick; the first cue
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

/// Director (cued shots) or orbit (editor). `C` toggles. `F` enters fly, and `F` or `Esc` leaves it.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CameraMode {
    #[default]
    Director,
    Orbit,
    /// Free-fly ([`Fly`]).
    Fly,
}

/// Free-fly speed at entry (m/s), its wheel range, and the factor per wheel line.
pub const FLY_SPEED_M_S: f32 = 0.3;
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
    cue: Option<(u32, usize)>,
    eye: Vec3,
    eye_vel: Vec3,
    look: Vec3,
    look_vel: Vec3,
    dir: Vec3,
    orbit: Orbit,
    fly: Fly,
    trail: VecDeque<[f32; 2]>,
    /// The mode last framed, to reset TAA when the mode changes.
    framed: Option<CameraMode>,
}

/// The camera's state for the state JSON (`camera`).
#[derive(Resource, Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraDiag {
    pub mode: Option<CameraMode>,
    pub shot: Option<Shot>,
    /// Cuts since start.
    pub cuts: u32,
    pub eye: [f32; 3],
    /// Unit view direction.
    pub forward: [f32; 3],
    /// Free-fly speed (m/s); it applies once `mode` is `fly`.
    pub fly_speed: f32,
    /// Marble centre in physical pixels, newest first, up to [`TRAIL_FRAMES`].
    pub marble_px: Vec<[f32; 2]>,
    /// Marble radius in physical pixels.
    pub marble_radius_px: f32,
    pub post: post::LivePost,
    pub orbit: OrbitDiag,
}

/// Critically damped spring towards `target` (Game Programming Gems 4, "Critically Damped Ease-In/
/// Ease-Out Smoothing"): stable for any `dt`.
pub fn smooth_damp(cur: Vec3, target: Vec3, vel: &mut Vec3, smooth_s: f32, dt: f32) -> Vec3 {
    let omega = 2.0 / smooth_s.max(1e-4);
    let x = omega * dt;
    let exp = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let change = cur - target;
    let temp = (*vel + omega * change) * dt;
    *vel = (*vel - omega * temp) * exp;
    target + (change + temp) * exp
}

/// Ideal follow eye and look point for a marble at `p` heading `dir` (unit, horizontal).
pub fn follow_pose(p: Vec3, dir: Vec3) -> (Vec3, Vec3) {
    let side = dir.cross(Vec3::Y).normalize_or_zero();
    let eye = p - dir * FOLLOW_BACK_M + Vec3::Y * FOLLOW_UP_M + side * FOLLOW_SIDE_M;
    (eye, p + dir * FOLLOW_AHEAD_M)
}

fn lerp_marble(frame: &MarbleFrame) -> Option<(Vec3, Vec3)> {
    let curr: &MarbleState = frame.curr.first()?;
    let prev = frame.prev.first().unwrap_or(curr);
    let t = frame.alpha;
    let p = Vec3::from_array(prev.position).lerp(Vec3::from_array(curr.position), t);
    let v = Vec3::from_array(prev.linvel).lerp(Vec3::from_array(curr.linvel), t);
    Some((p, v))
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
}

impl Plugin for DirectorPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.mode)
            .insert_resource(self.cues.clone())
            .init_resource::<DirectorClock>()
            .init_resource::<DirectorState>()
            .init_resource::<CameraDiag>()
            .init_resource::<EditorCamera>()
            .add_systems(
                Update,
                (toggle_mode, fly_toggle, orbit_input, fly_input).chain(),
            )
            .add_systems(PostUpdate, direct.before(TransformSystems::Propagate));
    }
}

fn toggle_mode(keys: Option<Res<ButtonInput<KeyCode>>>, mut mode: ResMut<CameraMode>) {
    if keys.is_some_and(|k| k.just_pressed(KeyCode::KeyC)) {
        *mode = match *mode {
            CameraMode::Director => CameraMode::Orbit,
            CameraMode::Orbit => CameraMode::Director,
            CameraMode::Fly => CameraMode::Fly,
        };
    }
}

/// `F` enters fly from the current view; `F` or `Esc` returns to the mode it replaced.
fn fly_toggle(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut mode: ResMut<CameraMode>,
    mut st: ResMut<DirectorState>,
    cams: Query<&Transform, With<MainCamera>>,
) {
    let Some(keys) = keys else {
        return;
    };
    if *mode == CameraMode::Fly {
        if keys.any_just_pressed([KeyCode::KeyF, KeyCode::Escape]) {
            *mode = st.fly.back;
            // Re-enter the director's current cue as a cut.
            st.cue = None;
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
    windows: Query<&Window, With<PrimaryWindow>>,
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
    // Cursor deltas rather than raw mouse motion: the browser reports them without pointer lock.
    let cursor = windows.single().ok().and_then(Window::cursor_position);
    let dragging = buttons.is_some_and(|b| b.any_pressed([MouseButton::Left, MouseButton::Right]));
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

#[allow(clippy::type_complexity)]
fn direct(
    mode: Res<CameraMode>,
    cues: Res<Cues>,
    clock: Res<DirectorClock>,
    frame: Res<MarbleFrame>,
    mut st: ResMut<DirectorState>,
    mut diag: ResMut<CameraDiag>,
    mut cams: Query<
        (
            &Camera,
            &mut Transform,
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
    let Ok((camera, mut tf, dof, mut taa, (msaa, ssao, bloom, tm))) = cams.single_mut() else {
        return;
    };
    // The editor's empty track has no marble: orbit and fly still frame, the director waits.
    let Some((p, v)) = lerp_marble(&frame)
        .or_else(|| (*mode != CameraMode::Director).then_some((st.orbit.target, Vec3::ZERO)))
    else {
        return;
    };
    let has_taa = taa.is_some();
    let dt = clock.dt.max(0.0);
    let flat = Vec3::new(v.x, 0.0, v.z);
    if flat.length() > DIR_MIN_SPEED || st.dir == Vec3::ZERO {
        let want = if flat.length() > DIR_MIN_SPEED {
            flat.normalize()
        } else {
            Vec3::X
        };
        st.dir = if st.dir == Vec3::ZERO {
            want
        } else {
            st.dir.lerp(want, (dt * 6.0).min(1.0)).normalize_or(want)
        };
    }

    let (cut, shot) = match *mode {
        CameraMode::Orbit | CameraMode::Fly => (false, None),
        CameraMode::Director => {
            let Some((idx, shot)) = cues.at(clock.tick) else {
                return;
            };
            let key = (clock.run, idx);
            let cut = st.cue != Some(key);
            st.cue = Some(key);
            (cut, Some(shot))
        }
    };
    if *mode != CameraMode::Director {
        st.cue = None;
    }
    let mode_changed = st.framed.is_some_and(|m| m != *mode);
    st.framed = Some(*mode);
    if mode_changed && let Some(t) = taa.as_deref_mut() {
        t.reset = true;
    }

    let (eye, look) = match shot {
        None if *mode == CameraMode::Fly => {
            let f = st.fly;
            (f.eye, f.eye + f.rotation() * Vec3::NEG_Z)
        }
        None => (st.orbit.eye(), st.orbit.target),
        Some(Shot::Fixed { eye, look } | Shot::Set { eye, look, .. }) => {
            (Vec3::from_array(eye), Vec3::from_array(look))
        }
        Some(Shot::Follow) => {
            let (eye_t, look_t) = follow_pose(p, st.dir);
            if cut {
                (eye_t, look_t)
            } else {
                let (mut ev, mut lv) = (st.eye_vel, st.look_vel);
                let eye_t = eye_t + v * FOLLOW_EYE_SMOOTH_S;
                let look_t = look_t + v * FOLLOW_LOOK_SMOOTH_S;
                let eye = smooth_damp(st.eye, eye_t, &mut ev, FOLLOW_EYE_SMOOTH_S, dt);
                let look = smooth_damp(st.look, look_t, &mut lv, FOLLOW_LOOK_SMOOTH_S, dt);
                st.eye_vel = ev;
                st.look_vel = lv;
                (eye, look)
            }
        }
    };
    if cut {
        st.eye_vel = Vec3::ZERO;
        st.look_vel = Vec3::ZERO;
        st.trail.clear();
        diag.cuts += 1;
        if let Some(mut taa) = taa {
            taa.reset = true;
        }
    }
    st.eye = eye;
    st.look = look;
    *tf = Transform::from_translation(eye).looking_at(look, Vec3::Y);

    let forward = tf.forward().as_vec3();
    let depth = match shot {
        Some(Shot::Set { focus_m, .. }) => focus_m,
        _ => (p - eye).dot(forward).max(0.01),
    };
    let mut dof = dof;
    if let Some(d) = dof.as_deref_mut() {
        d.focal_distance = depth;
    }

    // Screen-space marble for the ghost check (the transform is final for this frame).
    let gt = GlobalTransform::from(*tf);
    let scale = camera.target_scaling_factor().unwrap_or(1.0);
    let r = scene::standard_marble_radius_m();
    if let (Ok(c), Ok(e)) = (
        camera.world_to_viewport(&gt, p),
        camera.world_to_viewport(&gt, p + tf.right().as_vec3() * r),
    ) {
        st.trail.push_front([c.x * scale, c.y * scale]);
        st.trail.truncate(TRAIL_FRAMES);
        diag.marble_radius_px = c.distance(e) * scale;
    }
    diag.mode = Some(*mode);
    diag.shot = shot;
    diag.eye = eye.to_array();
    diag.forward = forward.to_array();
    diag.fly_speed = st.fly.speed;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooth_damp_converges_without_overshoot() {
        let mut v = Vec3::ZERO;
        let mut x = Vec3::ZERO;
        let target = Vec3::new(1.0, 0.0, 0.0);
        for _ in 0..240 {
            x = smooth_damp(x, target, &mut v, 0.2, 1.0 / 60.0);
            assert!(x.x <= 1.0 + 1e-4);
        }
        assert!((x - target).length() < 1e-3);
        // A huge step stays finite.
        let y = smooth_damp(Vec3::ZERO, target, &mut Vec3::ZERO.clone(), 0.2, 10.0);
        assert!(y.is_finite());
    }

    #[test]
    fn follow_pose_is_behind_above_and_looks_ahead() {
        let p = Vec3::new(0.3, 0.008, 0.0);
        let (eye, look) = follow_pose(p, Vec3::X);
        assert!(eye.x < p.x && eye.y > p.y);
        assert!(look.x > p.x);
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

    #[test]
    fn orbit_eye_is_at_radius() {
        let o = Orbit::default();
        assert!((o.eye().distance(o.target) - o.radius).abs() < 1e-5);
    }
}
