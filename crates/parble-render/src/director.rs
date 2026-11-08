//! The run camera's director (PRD §5.6): shot rules, camera zones and smooth blends. Pure logic on
//! glam types, so it is unit-tested without an app; [`crate::camera`] feeds it the marbles, the
//! camera clock and the zones each frame and applies the [`Pose`] it returns.
//!
//! # Shots
//!
//! A [`Rule`] is a camera rule evaluated every frame into a [`Pose`] (eye, unit forward, vertical
//! field of view, DoF focus distance):
//!
//! - [`Rule::Follow`]: the low, close tracking shot behind the marble (distance, height, lead),
//!   critically damped springs on the eye and the look point.
//! - [`Rule::Frame`]: pull back and fit a [`Target`] (the marble, a point, or a vertical axis
//!   segment) together with the marble, at a set elevation and either a set bearing or the bearing
//!   the camera had when the shot began. An axis stays centred in frame; the distance follows the
//!   marble on a spring so the whole segment and the marble stay in view.
//! - [`Rule::Orbit`]: circle a point, starting from the camera's current bearing.
//! - [`Rule::Fixed`]: a fixed eye and look point.
//!
//! # Blends
//!
//! Every change of shot blends: a cue change, entering or leaving a zone, a change of followed
//! marble, a run restart. A blend mixes the outgoing [`Layer`] and the incoming shot, both still
//! evaluated live, with a smootherstep weight over the blend time: the eye is interpolated, the
//! forward direction slerped, the field of view and focus distance interpolated. A blend that
//! starts during another keeps the one in flight as its source (nested up to [`MAX_DEPTH`], past
//! which the oldest freezes at its last pose), so retargeting never stops the camera dead. A run
//! restart freezes the outgoing layer first, so the teleported marble cannot drag it.
//!
//! The only snaps are the director's first frame and a hand-back from the orbit or fly camera
//! ([`Director::reset`]): those are user mode switches, not shot changes.
//!
//! # Zones
//!
//! A [`Zone`] is a volume (an axis-aligned box or a vertical cylinder) with a rule, a priority,
//! blend-in and blend-out times and a hold. The followed marble's interpolated position is tested
//! against every zone each frame; a zone it is inside stays inside until the marble is
//! [`EXIT_MARGIN_M`] past its boundary, and active [`Zone::hold_s`] longer. The active zone with
//! the highest priority (ties to the earlier zone) wins; with none, the scenario's cue plays. The
//! zones read the marble positions the app already copies out of the sim, so they cannot reach the
//! sim or a trajectory hash.

use bevy::math::{Vec2, Vec3};
use serde::Serialize;

/// Default vertical field of view (degrees).
pub const FOV_DEG: f32 = 35.0;

/// Follow shot defaults: distance behind the marble, height above it, sideways offset, look-ahead
/// (metres). The side offset stays inside the 28 mm channel so the near wall never blocks the
/// marble.
pub const FOLLOW_BACK_M: f32 = 0.14;
pub const FOLLOW_UP_M: f32 = 0.07;
pub const FOLLOW_SIDE_M: f32 = 0.010;
pub const FOLLOW_AHEAD_M: f32 = 0.05;
/// Spring smoothing times (seconds) of the follow eye and look point. The spring targets lead by
/// `velocity × smoothing time`, which cancels the steady lag at constant speed, so the springs
/// only damp changes (the ramp foot, wall rebounds).
pub const FOLLOW_EYE_SMOOTH_S: f32 = 0.15;
pub const FOLLOW_LOOK_SMOOTH_S: f32 = 0.06;
/// Smoothing of a framing shot's look point and distance (seconds).
pub const FRAME_SMOOTH_S: f32 = 0.35;
/// Below this speed the follow heading holds its last value (m/s).
const DIR_MIN_SPEED: f32 = 0.05;

/// Blend time of a scripted cue change (seconds).
pub const CUE_BLEND_S: f32 = 0.8;
/// Blend time when the followed marble changes or a run restarts (seconds).
pub const RETARGET_BLEND_S: f32 = 1.0;
/// The shortest blend: a zone's 0 ms still eases (seconds).
pub const MIN_BLEND_S: f32 = 0.15;
/// Nested blends kept live; deeper sources freeze.
pub const MAX_DEPTH: usize = 3;
/// A marble inside a zone leaves it only this far past its boundary (metres).
pub const EXIT_MARGIN_M: f32 = 0.008;
/// A marble slower than this counts as at rest (m/s), and the director moves on to a moving one
/// after it has rested [`REST_S`].
pub const REST_SPEED: f32 = 0.03;
pub const REST_S: f32 = 1.0;
/// Radius kept around the marble when framing (metres).
const MARBLE_PAD_M: f32 = 0.015;

/// What a framing shot keeps in view (with the marble).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Target {
    Marble,
    Point {
        at: [f32; 3],
    },
    /// A vertical segment from `base` to `top`.
    Axis {
        base: [f32; 3],
        top: [f32; 3],
    },
}

/// Where the depth of field focuses.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "on")]
pub enum Focus {
    Marble,
    /// The shot's target or look point.
    Target,
    /// A set distance (the look-dev set shots).
    Distance {
        m: f32,
    },
}

/// A camera rule. Lengths in metres, angles in radians, speeds in radians per second.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "rule")]
pub enum Rule {
    Follow {
        back: f32,
        up: f32,
        ahead: f32,
    },
    Frame {
        target: Target,
        focus: Focus,
        /// 1 fits the target edge to edge; larger is looser.
        margin: f32,
        elevation: f32,
        /// Bearing of the view (0 looks along −Z, π/2 along −X); `None` keeps the bearing the
        /// camera had when the shot began.
        azimuth: Option<f32>,
        fov: Option<f32>,
    },
    Orbit {
        center: [f32; 3],
        radius: f32,
        up: f32,
        speed: f32,
        focus: Focus,
    },
    Fixed {
        eye: [f32; 3],
        look: [f32; 3],
        focus: Focus,
        fov: Option<f32>,
    },
}

impl Rule {
    /// The default follow shot.
    pub const FOLLOW: Self = Self::Follow {
        back: FOLLOW_BACK_M,
        up: FOLLOW_UP_M,
        ahead: FOLLOW_AHEAD_M,
    };

    pub fn name(&self) -> &'static str {
        match self {
            Self::Follow { .. } => "follow",
            Self::Frame { .. } => "frame",
            Self::Orbit { .. } => "orbit",
            Self::Fixed { .. } => "fixed",
        }
    }

    /// Whether the rule moves with the marble (so a new marble or a restart is a new shot).
    pub fn tracks_marble(&self) -> bool {
        match self {
            Self::Follow { .. } | Self::Frame { .. } => true,
            Self::Orbit { focus, .. } | Self::Fixed { focus, .. } => *focus == Focus::Marble,
        }
    }
}

/// A camera pose: eye, unit forward, vertical field of view (radians), DoF focus distance (m).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub eye: Vec3,
    pub forward: Vec3,
    pub fov: f32,
    pub focus: f32,
}

impl Pose {
    pub fn look(&self) -> Vec3 {
        self.eye + self.forward * self.focus.max(0.05)
    }
}

/// Smootherstep: zero first and second derivative at both ends. Its peak slope is 1.875.
pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Spherical interpolation of unit vectors.
pub fn slerp_dir(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    let d = a.dot(b).clamp(-1.0, 1.0);
    if d > 0.9995 {
        return a.lerp(b, t).normalize_or(b);
    }
    let axis = if d < -0.9995 {
        // Opposite: turn about any axis perpendicular to `a`, preferring a level turn.
        let side = a.cross(Vec3::Y);
        if side.length_squared() > 1e-6 {
            Vec3::Y
        } else {
            Vec3::X
        }
    } else {
        a.cross(b).normalize()
    };
    let angle = d.acos();
    bevy::math::Quat::from_axis_angle(axis, angle * t) * a
}

/// The pose `t` of the way from `a` to `b`.
pub fn mix(a: &Pose, b: &Pose, t: f32) -> Pose {
    Pose {
        eye: a.eye.lerp(b.eye, t),
        forward: slerp_dir(a.forward, b.forward, t),
        fov: a.fov + (b.fov - a.fov) * t,
        focus: a.focus + (b.focus - a.focus) * t,
    }
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
pub fn follow_pose(p: Vec3, dir: Vec3, back: f32, up: f32, ahead: f32) -> (Vec3, Vec3) {
    let side = dir.cross(Vec3::Y).normalize_or_zero();
    let eye = p - dir * back + Vec3::Y * up + side * FOLLOW_SIDE_M;
    (eye, p + dir * ahead)
}

/// Unit forward for a bearing and elevation (positive looks down).
pub fn forward_of(azimuth: f32, elevation: f32) -> Vec3 {
    let (sa, ca) = azimuth.sin_cos();
    let (se, ce) = elevation.sin_cos();
    Vec3::new(-sa * ce, -se, -ca * ce)
}

/// Bearing of a view from `eye` towards `at` (see [`Rule::Frame::azimuth`]).
pub fn azimuth_of(eye: Vec3, at: Vec3) -> f32 {
    let o = eye - at;
    if o.x.abs() + o.z.abs() < 1e-6 {
        0.0
    } else {
        o.x.atan2(o.z)
    }
}

/// Distance from the look point at which a sphere of `radius` fills a view of vertical field of
/// view `fov` and width/height `aspect`, times `margin`.
pub fn fit_distance(radius: f32, fov: f32, aspect: f32, margin: f32) -> f32 {
    let half_v = fov * 0.5;
    let half_h = (half_v.tan() * aspect.max(0.1)).atan();
    radius * margin / half_v.min(half_h).sin().max(1e-3)
}

/// One marble as the director sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarbleIn {
    pub p: Vec3,
    pub v: Vec3,
}

/// The inputs of one frame.
#[derive(Debug, Clone, Copy)]
pub struct Frame<'a> {
    /// Interpolated marbles in spawn order.
    pub marbles: &'a [MarbleIn],
    /// Camera time step (seconds, real time).
    pub dt: f32,
    /// Viewport width / height.
    pub aspect: f32,
    /// Completed runs (a change is a restart).
    pub run: u32,
}

/// A trigger volume in metres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "shape")]
pub enum Volume {
    Box {
        min: [f32; 3],
        max: [f32; 3],
    },
    Cylinder {
        base: [f32; 3],
        height: f32,
        radius: f32,
    },
}

impl Volume {
    /// Whether `p` is inside, grown by `grow` metres.
    pub fn contains(&self, p: Vec3, grow: f32) -> bool {
        match *self {
            Self::Box { min, max } => {
                let (lo, hi) = (Vec3::from_array(min) - grow, Vec3::from_array(max) + grow);
                p.cmpge(lo).all() && p.cmple(hi).all()
            }
            Self::Cylinder {
                base,
                height,
                radius,
            } => {
                let b = Vec3::from_array(base);
                let r = Vec2::new(p.x - b.x, p.z - b.z).length();
                r <= radius + grow && p.y >= b.y - grow && p.y <= b.y + height + grow
            }
        }
    }
}

/// Where a zone came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ZoneSource {
    /// Authored on the track (or the scenario's demo zone).
    Track,
    /// A piece's default camera hint.
    Piece,
}

/// A camera zone (see the module docs).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Zone {
    pub name: String,
    pub volume: Volume,
    pub rule: Rule,
    pub priority: i32,
    pub blend_in_s: f32,
    pub blend_out_s: f32,
    pub hold_s: f32,
    pub source: ZoneSource,
}

/// What the director is playing.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Key {
    Cue { idx: usize, run: u32 },
    Zone(usize),
}

/// A live shot: its rule, the marble it is bound to, and its springs.
#[derive(Debug, Clone, Copy)]
struct Shot {
    rule: Rule,
    marble: Option<usize>,
    started: bool,
    eye: Vec3,
    eye_vel: Vec3,
    look: Vec3,
    look_vel: Vec3,
    dir: Vec3,
    /// Framing: the bearing captured at the start, and the smoothed distance.
    azimuth: f32,
    dist: f32,
    dist_vel: Vec3,
    /// Orbit angle.
    angle: f32,
    last: Option<Pose>,
}

impl Shot {
    fn new(rule: Rule, marble: Option<usize>) -> Self {
        Self {
            rule,
            marble,
            started: false,
            eye: Vec3::ZERO,
            eye_vel: Vec3::ZERO,
            look: Vec3::ZERO,
            look_vel: Vec3::ZERO,
            dir: Vec3::ZERO,
            azimuth: 0.0,
            dist: 0.0,
            dist_vel: Vec3::ZERO,
            angle: 0.0,
            last: None,
        }
    }

    /// The pose this frame. `from` is the camera's pose when the shot starts (for the framing
    /// bearing and the orbit's start angle). `None` before the shot has a marble to work from.
    fn eval(&mut self, f: &Frame, from: Option<&Pose>) -> Option<Pose> {
        let m = self.marble.and_then(|i| f.marbles.get(i)).copied();
        if self.rule.tracks_marble() && m.is_none() {
            // The marble is gone (a cleared track): hold the last pose.
            return self.last;
        }
        let first = !self.started;
        self.started = true;
        let dt = f.dt.max(0.0);
        let depth_to = |eye: Vec3, fwd: Vec3, p: Vec3| (p - eye).dot(fwd).max(0.01);
        let pose = match self.rule {
            Rule::Follow { back, up, ahead } => {
                let m = m?;
                let flat = Vec3::new(m.v.x, 0.0, m.v.z);
                let want = if flat.length() > DIR_MIN_SPEED {
                    Some(flat.normalize())
                } else {
                    None
                };
                self.dir = match (self.dir == Vec3::ZERO, want) {
                    (true, w) => w.unwrap_or(Vec3::X),
                    (false, Some(w)) => self.dir.lerp(w, (dt * 6.0).min(1.0)).normalize_or(w),
                    (false, None) => self.dir,
                };
                let (eye_t, look_t) = follow_pose(m.p, self.dir, back, up, ahead);
                if first {
                    self.eye = eye_t;
                    self.look = look_t;
                } else {
                    self.eye = smooth_damp(
                        self.eye,
                        eye_t + m.v * FOLLOW_EYE_SMOOTH_S,
                        &mut self.eye_vel,
                        FOLLOW_EYE_SMOOTH_S,
                        dt,
                    );
                    self.look = smooth_damp(
                        self.look,
                        look_t + m.v * FOLLOW_LOOK_SMOOTH_S,
                        &mut self.look_vel,
                        FOLLOW_LOOK_SMOOTH_S,
                        dt,
                    );
                }
                let fwd = (self.look - self.eye).normalize_or(Vec3::NEG_Z);
                Pose {
                    eye: self.eye,
                    forward: fwd,
                    fov: FOV_DEG.to_radians(),
                    focus: depth_to(self.eye, fwd, m.p),
                }
            }
            Rule::Frame {
                target,
                focus,
                margin,
                elevation,
                azimuth,
                fov,
            } => {
                let m = m?;
                let fov = fov.unwrap_or(FOV_DEG.to_radians());
                let (centre, pts): (Vec3, Vec<Vec3>) = match target {
                    Target::Marble => (m.p, vec![m.p]),
                    Target::Point { at } => (Vec3::from_array(at), vec![Vec3::from_array(at)]),
                    Target::Axis { base, top } => {
                        let (b, t) = (Vec3::from_array(base), Vec3::from_array(top));
                        ((b + t) * 0.5, vec![b, t])
                    }
                };
                let radius = pts
                    .iter()
                    .map(|q| q.distance(centre))
                    .chain([m.p.distance(centre) + MARBLE_PAD_M])
                    .fold(MARBLE_PAD_M, f32::max);
                let dist = fit_distance(radius, fov, f.aspect, margin);
                if first {
                    self.azimuth =
                        azimuth.unwrap_or_else(|| from.map_or(0.0, |p| azimuth_of(p.eye, centre)));
                    self.look = centre;
                    self.dist = dist;
                } else {
                    self.look =
                        smooth_damp(self.look, centre, &mut self.look_vel, FRAME_SMOOTH_S, dt);
                    self.dist = smooth_damp(
                        Vec3::splat(self.dist),
                        Vec3::splat(dist),
                        &mut self.dist_vel,
                        FRAME_SMOOTH_S,
                        dt,
                    )
                    .x;
                }
                let fwd = forward_of(self.azimuth, elevation);
                let eye = self.look - fwd * self.dist;
                let focus_m = match (focus, target) {
                    (Focus::Distance { m }, _) => m,
                    (Focus::Marble, _) | (Focus::Target, Target::Marble) => depth_to(eye, fwd, m.p),
                    (Focus::Target, Target::Point { .. }) => depth_to(eye, fwd, self.look),
                    (Focus::Target, Target::Axis { base, top }) => {
                        // The axis point level with the eye's line of sight: its centre's depth.
                        let mid = (Vec3::from_array(base) + Vec3::from_array(top)) * 0.5;
                        depth_to(eye, fwd, mid)
                    }
                };
                Pose {
                    eye,
                    forward: fwd,
                    fov,
                    focus: focus_m,
                }
            }
            Rule::Orbit {
                center,
                radius,
                up,
                speed,
                focus,
            } => {
                let c = Vec3::from_array(center);
                if first {
                    self.angle = from.map_or(0.0, |p| azimuth_of(p.eye, c));
                } else {
                    self.angle += speed * dt;
                }
                let (s, co) = self.angle.sin_cos();
                let eye = c + Vec3::new(s * radius, up, co * radius);
                let fwd = (c - eye).normalize_or(Vec3::NEG_Z);
                Pose {
                    eye,
                    forward: fwd,
                    fov: FOV_DEG.to_radians(),
                    focus: focus_depth(focus, eye, fwd, c, m),
                }
            }
            Rule::Fixed {
                eye,
                look,
                focus,
                fov,
            } => {
                let (eye, look) = (Vec3::from_array(eye), Vec3::from_array(look));
                let fwd = (look - eye).normalize_or(Vec3::NEG_Z);
                Pose {
                    eye,
                    forward: fwd,
                    fov: fov.unwrap_or(FOV_DEG.to_radians()),
                    focus: focus_depth(focus, eye, fwd, look, m),
                }
            }
        };
        self.last = Some(pose);
        Some(pose)
    }
}

fn focus_depth(focus: Focus, eye: Vec3, fwd: Vec3, target: Vec3, m: Option<MarbleIn>) -> f32 {
    match (focus, m) {
        (Focus::Distance { m }, _) => m,
        (Focus::Marble, Some(m)) => (m.p - eye).dot(fwd).max(0.01),
        _ => (target - eye).dot(fwd).max(0.01),
    }
}

/// The director's output stack: one shot, a frozen pose, or a blend from a layer into a shot.
#[derive(Debug, Clone)]
enum Layer {
    Shot(Box<Shot>),
    Frozen(Pose),
    Blend {
        from: Box<Layer>,
        to: Box<Shot>,
        t: f32,
        dur: f32,
    },
}

impl Layer {
    fn depth(&self) -> usize {
        match self {
            Self::Blend { from, .. } => 1 + from.depth(),
            _ => 0,
        }
    }

    /// Replaces the layer `level` blends down with its last pose.
    fn freeze_at(&mut self, level: usize) {
        if level == 0 {
            if let Some(p) = self.last_pose() {
                *self = Self::Frozen(p);
            }
        } else if let Self::Blend { from, .. } = self {
            from.freeze_at(level - 1);
        }
    }

    fn last_pose(&self) -> Option<Pose> {
        match self {
            Self::Shot(s) => s.last,
            Self::Frozen(p) => Some(*p),
            Self::Blend { from, to, t, dur } => match (from.last_pose(), to.last) {
                (Some(a), Some(b)) => Some(mix(&a, &b, ease(*t / *dur))),
                (a, b) => b.or(a),
            },
        }
    }

    /// This frame's pose; advances blends by `f.dt` and collapses finished ones.
    fn eval(&mut self, f: &Frame, start: Option<&Pose>) -> Option<Pose> {
        match self {
            Self::Shot(s) => s.eval(f, start),
            Self::Frozen(p) => Some(*p),
            Self::Blend { from, to, t, dur } => {
                let a = from.eval(f, None);
                let b = to.eval(f, a.as_ref().or(start));
                *t = (*t + f.dt.max(0.0)).min(*dur);
                let w = ease(*t / *dur);
                let pose = match (a, b) {
                    (Some(a), Some(b)) => Some(mix(&a, &b, w)),
                    (a, b) => b.or(a),
                };
                if *t >= *dur {
                    *self = Self::Shot(to.clone());
                }
                pose
            }
        }
    }

    fn top(&self) -> Option<&Shot> {
        match self {
            Self::Shot(s) | Self::Blend { to: s, .. } => Some(s),
            Self::Frozen(_) => None,
        }
    }
}

/// The active blend, for the state JSON.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlendDiag {
    /// 0 at the start, 1 at the end (eased weight in `weight`).
    pub progress: f32,
    pub weight: f32,
    pub duration_s: f32,
    /// The layers being mixed below the incoming shot.
    pub depth: usize,
}

/// The director's state for the state JSON (`camera.director`).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectorDiag {
    /// The incoming (or settled) shot's rule.
    pub shot: Option<Rule>,
    /// `cue` or `zone`.
    pub source: Option<&'static str>,
    /// The cue index when `source` is `cue`.
    pub cue: Option<usize>,
    /// The playing zone's index into the zone list and its name.
    pub zone: Option<usize>,
    pub zone_name: Option<String>,
    pub blend: Option<BlendDiag>,
    /// Blends started since start (cue changes, zone entries and exits, retargets, restarts).
    pub blends: u32,
    /// Index of the followed marble.
    pub followed: Option<usize>,
    /// Zones known, and the zones the followed marble is in (by index).
    pub zones: usize,
    pub inside: Vec<usize>,
}

/// The director (see the module docs).
#[derive(Debug, Clone, Default)]
pub struct Director {
    layer: Option<Layer>,
    key: Option<Key>,
    run: Option<u32>,
    followed: Option<usize>,
    rest_s: f32,
    /// Per zone: inside now, and seconds since the marble was last inside.
    inside: Vec<bool>,
    since: Vec<f32>,
    pub diag: DirectorDiag,
}

impl Director {
    /// Forget the shot: the next frame snaps to the director's shot (a hand-back from orbit or
    /// fly, which the user drove).
    pub fn reset(&mut self) {
        self.layer = None;
        self.key = None;
    }

    /// The followed marble: the current one while it moves or has rested under [`REST_S`];
    /// otherwise the first moving marble (the lead, in normal play the only one).
    fn pick_marble(&mut self, f: &Frame) -> Option<usize> {
        let moving = |m: &MarbleIn| m.v.length() > REST_SPEED;
        match self.followed.and_then(|i| f.marbles.get(i).map(|m| (i, m))) {
            Some((i, m)) if moving(m) => {
                self.rest_s = 0.0;
                Some(i)
            }
            Some((i, _)) => {
                self.rest_s += f.dt.max(0.0);
                if self.rest_s < REST_S {
                    return Some(i);
                }
                match f.marbles.iter().position(moving) {
                    Some(j) => {
                        self.rest_s = 0.0;
                        Some(j)
                    }
                    None => Some(i),
                }
            }
            None => {
                self.rest_s = 0.0;
                f.marbles
                    .iter()
                    .position(moving)
                    .or((!f.marbles.is_empty()).then_some(0))
            }
        }
    }

    /// One frame: picks the shot from `cue` (index and shot) and `zones`, and returns the pose, or
    /// `None` while there is nothing to frame yet.
    pub fn update(
        &mut self,
        f: &Frame,
        cue: Option<(usize, Rule)>,
        zones: &[Zone],
    ) -> Option<Pose> {
        let dt = f.dt.max(0.0);
        let restarted = self.run.is_some_and(|r| r != f.run);
        self.run = Some(f.run);
        if restarted {
            // The marbles teleported back to the start: nothing may keep tracking them.
            self.followed = None;
            let settled_still =
                matches!(&self.layer, Some(Layer::Shot(s)) if !s.rule.tracks_marble());
            if !settled_still && let Some(p) = self.layer.as_ref().and_then(Layer::last_pose) {
                self.layer = Some(Layer::Frozen(p));
            }
        }
        let before = self.followed;
        self.followed = self.pick_marble(f);
        let retarget = before.is_some() && before != self.followed;

        // Zone membership of the followed marble.
        if self.inside.len() != zones.len() {
            self.inside = vec![false; zones.len()];
            self.since = vec![f32::INFINITY; zones.len()];
        }
        let p = self.followed.and_then(|i| f.marbles.get(i)).map(|m| m.p);
        for (i, z) in zones.iter().enumerate() {
            let grow = if self.inside[i] { EXIT_MARGIN_M } else { 0.0 };
            self.inside[i] = !restarted && p.is_some_and(|p| z.volume.contains(p, grow));
            self.since[i] = if self.inside[i] {
                0.0
            } else if restarted {
                f32::INFINITY
            } else {
                self.since[i] + dt
            };
        }
        let active = zones
            .iter()
            .enumerate()
            .filter(|(i, z)| self.since[*i] <= z.hold_s.max(MIN_BLEND_S))
            .max_by(|(i, a), (j, b)| a.priority.cmp(&b.priority).then(j.cmp(i)))
            .map(|(i, _)| i);

        let (key, rule, blend) = match (active, cue) {
            (Some(i), _) => (Key::Zone(i), zones[i].rule, zones[i].blend_in_s),
            (None, Some((idx, rule))) => {
                let leaving = match self.key {
                    Some(Key::Zone(z)) => zones.get(z).map(|z| z.blend_out_s),
                    _ => None,
                };
                (
                    Key::Cue { idx, run: f.run },
                    rule,
                    leaving.unwrap_or(CUE_BLEND_S),
                )
            }
            (None, None) => return self.layer.as_ref().and_then(Layer::last_pose),
        };

        let same = |a: &Key, b: &Key| match (a, b) {
            // A restart re-enters the same cue: a new shot only if it tracks the marble.
            (Key::Cue { idx: i, .. }, Key::Cue { idx: j, .. }) => i == j,
            _ => a == b,
        };
        let changed = !self.key.is_some_and(|k| same(&k, &key))
            || (restarted && rule.tracks_marble())
            || (retarget && rule.tracks_marble())
            || self.layer.as_ref().and_then(Layer::top).is_none();
        if changed {
            let shot = Box::new(Shot::new(rule, self.followed));
            self.layer = Some(match self.layer.take() {
                None => Layer::Shot(shot),
                Some(mut from) => {
                    let dur = if restarted || (retarget && same_key(self.key, key)) {
                        RETARGET_BLEND_S
                    } else {
                        blend
                    }
                    .max(MIN_BLEND_S);
                    if from.depth() + 1 >= MAX_DEPTH {
                        from.freeze_at(MAX_DEPTH - 2);
                    }
                    self.diag.blends += 1;
                    Layer::Blend {
                        from: Box::new(from),
                        to: shot,
                        t: 0.0,
                        dur,
                    }
                }
            });
            self.key = Some(key);
        }
        let start = self.layer.as_ref().and_then(Layer::last_pose);
        let pose = self.layer.as_mut()?.eval(f, start.as_ref());

        let d = &mut self.diag;
        d.shot = Some(rule);
        (d.source, d.cue, d.zone) = match key {
            Key::Cue { idx, .. } => (Some("cue"), Some(idx), None),
            Key::Zone(i) => (Some("zone"), None, Some(i)),
        };
        d.zone_name = d.zone.map(|i| zones[i].name.clone());
        d.blend = match &self.layer {
            Some(l @ Layer::Blend { t, dur, .. }) => Some(BlendDiag {
                progress: *t / *dur,
                weight: ease(*t / *dur),
                duration_s: *dur,
                depth: l.depth(),
            }),
            _ => None,
        };
        d.followed = self.followed;
        d.zones = zones.len();
        d.inside = (0..zones.len()).filter(|i| self.inside[*i]).collect();
        pose
    }
}

fn same_key(a: Option<Key>, b: Key) -> bool {
    match (a, b) {
        (Some(Key::Cue { idx: i, .. }), Key::Cue { idx: j, .. }) => i == j,
        (Some(a), b) => a == b,
        (None, _) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn frame(marbles: &[MarbleIn], run: u32) -> Frame<'_> {
        Frame {
            marbles,
            dt: DT,
            aspect: 16.0 / 9.0,
            run,
        }
    }

    fn fixed(eye: [f32; 3], look: [f32; 3]) -> Rule {
        Rule::Fixed {
            eye,
            look,
            focus: Focus::Target,
            fov: None,
        }
    }

    /// Per-frame change of two poses: eye distance (m), view angle (rad), focus (m), fov (rad).
    fn step(a: &Pose, b: &Pose) -> (f32, f32, f32, f32) {
        (
            a.eye.distance(b.eye),
            a.forward.dot(b.forward).clamp(-1.0, 1.0).acos(),
            (a.focus - b.focus).abs(),
            (a.fov - b.fov).abs(),
        )
    }

    fn axis_zone() -> Zone {
        Zone {
            name: "axis".into(),
            volume: Volume::Cylinder {
                base: [1.0, 0.0, 0.0],
                height: 0.3,
                radius: 0.06,
            },
            rule: Rule::Frame {
                target: Target::Axis {
                    base: [1.0, 0.0, 0.0],
                    top: [1.0, 0.3, 0.0],
                },
                focus: Focus::Target,
                margin: 1.3,
                elevation: 0.3,
                azimuth: None,
                fov: Some(40f32.to_radians()),
            },
            priority: 10,
            blend_in_s: 0.9,
            blend_out_s: 1.2,
            hold_s: 0.0,
            source: ZoneSource::Track,
        }
    }

    #[test]
    fn ease_and_slerp_hit_their_ends() {
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(1.0), 1.0);
        assert!((ease(0.5) - 0.5).abs() < 1e-6);
        let (a, b) = (Vec3::X, Vec3::NEG_Z);
        assert!(slerp_dir(a, b, 0.0).distance(a) < 1e-5);
        assert!(slerp_dir(a, b, 1.0).distance(b) < 1e-5);
        assert!((slerp_dir(a, b, 0.5).length() - 1.0).abs() < 1e-5);
        assert!((slerp_dir(a, -a, 0.5).length() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn smooth_damp_converges_without_overshoot() {
        let mut v = Vec3::ZERO;
        let mut x = Vec3::ZERO;
        let target = Vec3::new(1.0, 0.0, 0.0);
        for _ in 0..240 {
            x = smooth_damp(x, target, &mut v, 0.2, DT);
            assert!(x.x <= 1.0 + 1e-4);
        }
        assert!((x - target).length() < 1e-3);
        let y = smooth_damp(Vec3::ZERO, target, &mut Vec3::ZERO.clone(), 0.2, 10.0);
        assert!(y.is_finite());
    }

    /// A cue change between two fixed shots 1 m apart: the eye never moves more per frame than
    /// smootherstep's peak slope allows, and the first and last frames sit on the two shots.
    #[test]
    fn a_cue_blend_bounds_the_per_frame_change() {
        let a = fixed([0.0, 0.5, 1.0], [0.0, 0.0, 0.0]);
        let b = fixed([1.0, 0.5, 1.0], [1.0, 0.0, -0.5]);
        let mut d = Director::default();
        let m = [MarbleIn {
            p: Vec3::ZERO,
            v: Vec3::ZERO,
        }];
        let first = d.update(&frame(&m, 0), Some((0, a)), &[]).unwrap();
        assert_eq!(first.eye, Vec3::new(0.0, 0.5, 1.0), "the first frame snaps");
        let mut last = first;
        let mut peak = 0f32;
        for _ in 0..120 {
            let p = d.update(&frame(&m, 0), Some((1, b)), &[]).unwrap();
            let (de, da, ..) = step(&last, &p);
            peak = peak.max(de);
            assert!(da < 0.05, "turned {da} rad in one frame");
            last = p;
        }
        let bound = 1.875 * 1.0 / (CUE_BLEND_S / DT);
        assert!(peak <= bound * 1.02, "peak {peak} > {bound}");
        assert!(peak > bound * 0.8, "the blend took its whole time");
        assert!(last.eye.distance(Vec3::new(1.0, 0.5, 1.0)) < 1e-4);
        assert_eq!(d.diag.blends, 1);
        assert!(d.diag.blend.is_none());
    }

    /// A marble rolling along +X through an axis zone, out, back in mid-blend-out, and out again;
    /// then the followed marble changes and the run restarts. No frame jumps.
    #[test]
    fn zones_retargets_and_restarts_never_jump() {
        let zones = [axis_zone()];
        let mut d = Director::default();
        let mut last: Option<Pose> = None;
        let mut entered = false;
        let mut max = (0f32, 0f32, 0f32, 0f32);
        let mut check = |p: Pose, max: &mut (f32, f32, f32, f32)| {
            if let Some(l) = last {
                let s = step(&l, &p);
                *max = (
                    max.0.max(s.0),
                    max.1.max(s.1),
                    max.2.max(s.2),
                    max.3.max(s.3),
                );
            }
            last = Some(p);
        };
        // Marble 0 rolls +X at 0.5 m/s through the zone (x 0.94..1.06), reverses inside the
        // blend-out, comes back through, then rests; marble 1 starts moving far away.
        let mut x = 0.4f32;
        let mut vx = 0.5f32;
        for i in 0..900 {
            if i == 100 {
                vx = -0.5;
            }
            if i == 200 {
                vx = 0.0;
            }
            x += vx * DT;
            let m1v = if i > 400 { 0.4 } else { 0.0 };
            let m = [
                MarbleIn {
                    p: Vec3::new(x, 0.1, 0.0),
                    v: Vec3::new(vx, 0.0, 0.0),
                },
                MarbleIn {
                    p: Vec3::new(0.2 + m1v * (i as f32 - 400.0).max(0.0) * DT, 0.1, 0.4),
                    v: Vec3::new(m1v, 0.0, 0.0),
                },
            ];
            let run = u32::from(i >= 700);
            let p = d
                .update(&frame(&m, run), Some((0, Rule::FOLLOW)), &zones)
                .unwrap();
            entered |= d.diag.zone == Some(0);
            check(p, &mut max);
        }
        assert!(entered, "the marble entered the zone");
        assert_eq!(
            d.diag.blends, 6,
            "enter, exit, re-enter, exit, retarget, restart"
        );
        // The worst case is the analytic blend bound: smootherstep peaks at 1.875× the mean rate,
        // so a ~0.7 m pull-back over 0.9 s moves the eye at most ~0.025 m a frame, and the second
        // exit (the marble now heading −X) turns the view half round over the 1.2 s blend-out.
        let half_turn = 1.875 * std::f32::consts::PI / (1.2 / DT);
        assert!(max.0 < 0.03, "eye jumped {} m in one frame", max.0);
        assert!(
            max.1 < half_turn + 0.01,
            "view turned {} rad in one frame",
            max.1
        );
        assert!(max.2 < 0.05, "focus jumped {} m in one frame", max.2);
        assert!(max.3 < 0.01, "fov jumped {} rad in one frame", max.3);
    }

    #[test]
    fn framing_an_axis_keeps_both_ends_and_the_marble_in_view() {
        let zone = axis_zone();
        let mut d = Director::default();
        let aspect = 16.0 / 9.0;
        for (i, mp) in [
            Vec3::new(1.05, 0.28, 0.02),
            Vec3::new(0.95, 0.02, -0.05),
            Vec3::new(1.0, 0.15, 0.06),
        ]
        .into_iter()
        .enumerate()
        {
            let m = [MarbleIn {
                p: mp,
                v: Vec3::new(0.0, -0.3, 0.0),
            }];
            // Settle: the first frame snaps into the zone shot, then the springs settle.
            let mut pose = None;
            for _ in 0..240 {
                pose = d.update(
                    &Frame {
                        marbles: &m,
                        dt: DT,
                        aspect,
                        run: i as u32,
                    },
                    None,
                    std::slice::from_ref(&zone),
                );
            }
            let pose = pose.unwrap();
            // Project into normalised device coordinates of the pose's view.
            let right = pose.forward.cross(Vec3::Y).normalize();
            let up = right.cross(pose.forward);
            let t = (pose.fov * 0.5).tan();
            let ndc = |q: Vec3| {
                let r = q - pose.eye;
                let z = r.dot(pose.forward);
                (
                    z,
                    Vec2::new(r.dot(right) / (z * t * aspect), r.dot(up) / (z * t)),
                )
            };
            for q in [Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 0.3, 0.0), mp] {
                let (z, n) = ndc(q);
                assert!(z > 0.0, "{q} behind the camera");
                assert!(
                    n.x.abs() < 0.95 && n.y.abs() < 0.95,
                    "{q} at {n} is outside the frame"
                );
            }
            // The axis is centred.
            assert!(ndc(Vec3::new(1.0, 0.15, 0.0)).1.x.abs() < 0.05);
        }
    }

    #[test]
    fn overlapping_zones_resolve_by_priority_and_hold() {
        let mut low = axis_zone();
        low.priority = 1;
        low.name = "low".into();
        low.volume = Volume::Box {
            min: [0.0, 0.0, -0.1],
            max: [2.0, 0.3, 0.1],
        };
        low.hold_s = 0.5;
        let high = axis_zone();
        let zones = [low, high];
        let mut d = Director::default();
        let at = |x: f32| {
            [MarbleIn {
                p: Vec3::new(x, 0.1, 0.0),
                v: Vec3::new(0.5, 0.0, 0.0),
            }]
        };
        d.update(&frame(&at(0.5), 0), Some((0, Rule::FOLLOW)), &zones);
        assert_eq!(d.diag.zone, Some(0));
        d.update(&frame(&at(1.0), 0), Some((0, Rule::FOLLOW)), &zones);
        assert_eq!(d.diag.zone, Some(1), "the higher priority wins");
        // Out of both: the hold keeps zone 0 for 0.5 s, then the cue returns.
        for _ in 0..20 {
            d.update(&frame(&at(2.5), 0), Some((0, Rule::FOLLOW)), &zones);
        }
        assert_eq!(d.diag.zone, Some(0));
        for _ in 0..30 {
            d.update(&frame(&at(2.5), 0), Some((0, Rule::FOLLOW)), &zones);
        }
        assert_eq!(d.diag.source, Some("cue"));
    }

    #[test]
    fn no_marble_frames_nothing_and_a_set_shot_survives_restarts() {
        let mut d = Director::default();
        assert!(
            d.update(&frame(&[], 0), Some((0, Rule::FOLLOW)), &[])
                .is_none()
        );
        let set = Rule::Fixed {
            eye: [0.0, 1.0, 1.0],
            look: [0.0, 0.0, 0.0],
            focus: Focus::Distance { m: 1.2 },
            fov: None,
        };
        let mut d = Director::default();
        let a = d.update(&frame(&[], 0), Some((0, set)), &[]).unwrap();
        let b = d.update(&frame(&[], 1), Some((0, set)), &[]).unwrap();
        assert_eq!(a, b);
        assert_eq!(d.diag.blends, 0);
        assert_eq!(a.focus, 1.2);
    }

    #[test]
    fn orbit_starts_from_the_current_bearing() {
        let mut d = Director::default();
        let m = [MarbleIn {
            p: Vec3::ZERO,
            v: Vec3::ZERO,
        }];
        d.update(
            &frame(&m, 0),
            Some((0, fixed([0.0, 0.3, 1.0], [0.0, 0.0, 0.0]))),
            &[],
        );
        let orbit = Rule::Orbit {
            center: [0.0, 0.0, 0.0],
            radius: 1.0,
            up: 0.3,
            speed: 0.5,
            focus: Focus::Target,
        };
        let p = d.update(&frame(&m, 0), Some((1, orbit)), &[]).unwrap();
        // One frame in, the camera is still essentially where it was.
        assert!(p.eye.distance(Vec3::new(0.0, 0.3, 1.0)) < 0.01);
    }

    #[test]
    fn volumes_contain_their_points() {
        let b = Volume::Box {
            min: [0.0, 0.0, 0.0],
            max: [1.0, 1.0, 1.0],
        };
        assert!(b.contains(Vec3::splat(0.5), 0.0));
        assert!(!b.contains(Vec3::new(1.005, 0.5, 0.5), 0.0));
        assert!(b.contains(Vec3::new(1.005, 0.5, 0.5), 0.01));
        let c = Volume::Cylinder {
            base: [0.0, 0.0, 0.0],
            height: 1.0,
            radius: 0.1,
        };
        assert!(c.contains(Vec3::new(0.05, 0.9, 0.05), 0.0));
        assert!(!c.contains(Vec3::new(0.2, 0.5, 0.0), 0.0));
        assert!(!c.contains(Vec3::new(0.0, 1.1, 0.0), 0.0));
    }
}
