//! The main camera: a follow-marble director that cuts between shots on scripted cues, and an
//! orbit camera for the editor. Render-side only: it reads [`MarbleFrame`] and the app's
//! [`DirectorClock`] and writes the camera's transform and focus. Nothing here reaches the sim.
//!
//! The follow shot is a low, close tracking shot behind the marble, a critically damped spring on
//! the eye and the look point. The DoF focus is the marble's view-space depth every frame. A cut
//! snaps the spring and resets the TAA history so the previous shot never ghosts into the next.

use std::collections::VecDeque;

use bevy::{
    anti_alias::taa::TemporalAntiAliasing,
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    post_process::dof::DepthOfField,
    prelude::*,
    transform::TransformSystems,
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

    /// One fixed overview of the whole tray (`--camera overview`).
    /// The kit stress wall (P1-T4) from its near end: the near pieces draw LOD0 and the far end
    /// LOD2, with both crossfade bands in frame.
    pub fn kit() -> Self {
        Self(vec![(
            0,
            Shot::Set {
                eye: [-0.45, 0.55, -0.25],
                look: [1.6, 0.22, -0.87],
                focus_m: 1.5,
            },
        )])
    }

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

/// Director (cued shots) or orbit (editor). `C` toggles.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CameraMode {
    #[default]
    Director,
    Orbit,
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

#[derive(Resource, Debug, Default)]
struct DirectorState {
    cue: Option<(u32, usize)>,
    eye: Vec3,
    eye_vel: Vec3,
    look: Vec3,
    look_vel: Vec3,
    dir: Vec3,
    orbit: Orbit,
    trail: VecDeque<[f32; 2]>,
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
    /// Marble centre in physical pixels, newest first, up to [`TRAIL_FRAMES`].
    pub marble_px: Vec<[f32; 2]>,
    /// Marble radius in physical pixels.
    pub marble_radius_px: f32,
    pub post: post::LivePost,
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
            .add_systems(Update, (toggle_mode, orbit_input))
            .add_systems(PostUpdate, direct.before(TransformSystems::Propagate));
    }
}

fn toggle_mode(keys: Option<Res<ButtonInput<KeyCode>>>, mut mode: ResMut<CameraMode>) {
    if keys.is_some_and(|k| k.just_pressed(KeyCode::KeyC)) {
        *mode = match *mode {
            CameraMode::Director => CameraMode::Orbit,
            CameraMode::Orbit => CameraMode::Director,
        };
    }
}

/// Left drag orbits, the wheel zooms.
fn orbit_input(
    mode: Res<CameraMode>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    motion: Option<Res<AccumulatedMouseMotion>>,
    scroll: Option<Res<AccumulatedMouseScroll>>,
    mut st: ResMut<DirectorState>,
) {
    if *mode != CameraMode::Orbit {
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
    let Ok((camera, mut tf, dof, taa, (msaa, ssao, bloom, tm))) = cams.single_mut() else {
        return;
    };
    let Some((p, v)) = lerp_marble(&frame) else {
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
        CameraMode::Orbit => (false, None),
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
    if *mode == CameraMode::Orbit {
        st.cue = None;
    }

    let (eye, look) = match shot {
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
    fn orbit_eye_is_at_radius() {
        let o = Orbit::default();
        assert!((o.eye().distance(o.target) - o.radius).abs() < 1e-5);
    }
}
