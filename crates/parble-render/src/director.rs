//! The run camera's director (PRD §5.6): shot rules, camera zones and smooth blends. Pure logic on
//! glam types, so it is unit-tested without an app; [`crate::camera`] feeds it the marbles, the
//! camera clock and the zones each frame and applies the [`Pose`] it returns.
//!
//! # Shots
//!
//! A [`Rule`] is a camera rule evaluated every frame into a [`Pose`] (eye, unit forward, vertical
//! field of view, DoF focus distance):
//!
//! - [`Rule::Follow`]: the low tracking shot. It frames the marble's *action window*
//!   ([`action_window`]): its path over the last [`FOLLOW_TRAIL_S`], its look-ahead along its
//!   velocity for [`FOLLOW_LEAD_S`], and any point of interest (a mechanism) near that path. The
//!   look point and the DoF focus sit on the window's centre, ahead of the marble, and the camera
//!   pulls back from its closest (`back`, `up`) to fit the whole window. Its bearing follows the
//!   window's chord (oldest trail point to the end of the look-ahead), so it starts to turn before
//!   a corner and averages out a switchback, and the yaw is rate- and acceleration-limited
//!   ([`FOLLOW_MAX_YAW_RATE`], [`FOLLOW_MAX_YAW_ACCEL`]): a corner never whips it round.
//!   Critically damped springs smooth the eye and the look point.
//! - [`Rule::Frame`]: pull back and fit a [`Target`] (the marble, a point, or a vertical axis
//!   segment) together with the marble, at a set elevation and either a set bearing or the bearing
//!   the camera had when the shot began. An axis stays centred in frame; the distance follows the
//!   marble, and where it will be [`FRAME_LEAD_S`] ahead, on a spring so the whole segment and the
//!   marble stay in view.
//! - [`Rule::Orbit`]: circle a point, starting from the camera's current bearing.
//! - [`Rule::Fixed`]: a fixed eye and look point.
//!
//! # Blends
//!
//! Every change of shot blends: a cue change, entering or leaving a zone, a change of followed
//! marble, a run restart. A blend mixes the outgoing [`Layer`] and the incoming shot, both still
//! evaluated live, with a smootherstep weight over the blend time: the eye is interpolated, the
//! forward direction's bearing and elevation interpolated, the field of view and focus distance interpolated. A blend that
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
//! [`EXIT_MARGIN_M`] past its boundary, and active [`Zone::hold_s`] longer, unless the marble has
//! sat past [`HOLD_FRAME_NDC`] of the frame for [`HOLD_EXIT_S`]. The active zone with
//! the highest priority (ties to the earlier zone) wins; with none, the scenario's cue plays. The
//! zones read the marble positions the app already copies out of the sim, so they cannot reach the
//! sim or a trajectory hash.

use bevy::math::{Vec2, Vec3};
use serde::Serialize;

/// Default vertical field of view (degrees).
pub const FOV_DEG: f32 = 35.0;

/// Follow shot defaults (metres): the closest the camera comes (`back` behind and `up` above the
/// action window's centre, which also sets the view's elevation), and the shortest look-ahead.
pub const FOLLOW_BACK_M: f32 = 0.14;
pub const FOLLOW_UP_M: f32 = 0.07;
pub const FOLLOW_AHEAD_M: f32 = 0.05;
/// Spring smoothing times (seconds) of the follow eye and look point. The spring targets lead by
/// `velocity × smoothing time`, which cancels the steady lag at constant speed, so the springs
/// only damp changes (the ramp foot, wall rebounds, the window growing and shrinking).
pub const FOLLOW_EYE_SMOOTH_S: f32 = 0.30;
pub const FOLLOW_LOOK_SMOOTH_S: f32 = 0.30;
/// Smoothing of a framing shot's look point and distance (seconds).
pub const FRAME_SMOOTH_S: f32 = 0.35;
/// Fastest a framing shot changes its distance (m/s).
pub const FRAME_MAX_PULL_M_S: f32 = 1.0;
/// A framing shot fits where the marble will be this far ahead (seconds) as well as where it is,
/// so it pulls back before a marble rolling away reaches the frame's edge rather than after.
pub const FRAME_LEAD_S: f32 = 0.25;
/// The follow shot's action window: the marble's path over the last [`FOLLOW_TRAIL_S`], and its
/// look-ahead along its velocity for [`FOLLOW_LEAD_S`] (at least the rule's `ahead`).
pub const FOLLOW_TRAIL_S: f32 = 0.25;
pub const FOLLOW_LEAD_S: f32 = 0.5;
/// Points of interest within this distance of the window's path join it (metres).
pub const FOLLOW_INTEREST_M: f32 = 0.06;
/// The follow camera's yaw: it turns towards the window's heading with this response time, never
/// faster than [`FOLLOW_MAX_YAW_RATE`] (rad/s) and never changing that rate faster than
/// [`FOLLOW_MAX_YAW_ACCEL`] (rad/s²).
pub const FOLLOW_YAW_RESPONSE_S: f32 = 0.7;
pub const FOLLOW_MAX_YAW_RATE: f32 = 0.85;
pub const FOLLOW_MAX_YAW_ACCEL: f32 = 2.5;
/// Farthest the follow shot pulls back while the marble is out of sight in a hidden path, to hold
/// the marble's way in and its exit together (metres).
pub const FOLLOW_HIDDEN_MAX_M: f32 = 1.4;
/// A view whose forward has a y below minus this counts as top-down.
pub const FOLLOW_TOP_DOWN_Y: f32 = 0.8;
/// Steepest the follow view looks down when it rises to see past solid track (radians).
pub const FOLLOW_MAX_ELEVATION: f32 = std::f32::consts::FRAC_PI_3;
/// The clear-view moves the follow shot tries, cheapest first: (extra elevation, extra bearing),
/// radians. The first with a clear line of sight to the marble and its look-ahead wins. Swings
/// come before rises, and no view looks down more steeply than [`FOLLOW_MAX_ELEVATION`] (60°).
pub const CLEAR_VIEWS: [(f32, f32); 20] = [
    (0.0, 0.0),
    (0.0, 0.5),
    (0.0, -0.5),
    (0.2, 0.0),
    (0.2, 0.5),
    (0.2, -0.5),
    (0.0, 1.0),
    (0.0, -1.0),
    (0.4, 0.0),
    (0.2, 1.0),
    (0.2, -1.0),
    (0.0, 1.5),
    (0.0, -1.5),
    (0.4, 0.5),
    (0.4, -0.5),
    (0.4, 1.0),
    (0.4, -1.0),
    (0.6, 0.0),
    (0.6, 1.0),
    (0.6, -1.0),
];
/// The clear-view moves of a framing shot: it keeps its bearing and only rises.
pub const FRAME_LIFTS: [(f32, f32); 5] =
    [(0.0, 0.0), (0.2, 0.0), (0.4, 0.0), (0.6, 0.0), (0.8, 0.0)];
/// The follow shot returns to its plain view once that has been clear this long (seconds).
pub const CLEAR_RELEASE_S: f32 = 0.5;
/// A camera whose bearing is this far off the shot's own (radians) counts as swung to that side.
pub const CLEAR_SWUNG: f32 = 0.05;
/// Acceleration limit of the rise a blend takes when its mixed view looks through solid track
/// (rad/s²). A blend lasts about a second, so the rise must reach its pace sooner than the follow
/// yaw's; its rate keeps the follow limit.
pub const BLEND_RISE_ACCEL: f32 = 6.0;
/// Below this chord the heading holds (the marble is nearly still).
const FOLLOW_MIN_CHORD_M: f32 = 0.02;
/// Farthest the follow shot pulls back to fit its window (metres), and how loose the fit is.
pub const FOLLOW_MAX_M: f32 = 0.9;
pub const FOLLOW_FIT_MARGIN: f32 = 1.15;

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

/// The view direction `t` of the way from `a` to `b`: bearing (the short way round) and elevation
/// interpolated apart, so a blend between views facing apart turns level and never tips over the
/// top through a steeper view than either end. A straight-down end has no bearing: slerp then.
pub fn mix_dir(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    if a.y.abs() > 0.999 || b.y.abs() > 0.999 {
        return slerp_dir(a, b, t);
    }
    let az = |f: Vec3| (-f.x).atan2(-f.z);
    let el = |f: Vec3| (-f.y).clamp(-1.0, 1.0).asin();
    let (aa, ab) = (az(a), az(b));
    let (ea, eb) = (el(a), el(b));
    forward_of(aa + wrap_angle(ab - aa) * t, ea + (eb - ea) * t)
}

/// The pose `t` of the way from `a` to `b`.
pub fn mix(a: &Pose, b: &Pose, t: f32) -> Pose {
    Pose {
        eye: a.eye.lerp(b.eye, t),
        forward: mix_dir(a.forward, b.forward, t),
        fov: a.fov + (b.fov - a.fov) * t,
        focus: a.focus + (b.focus - a.focus) * t,
    }
}

/// The pose `t` of the way from `a` to `b` about `pivot`: the eyes' bearing, elevation and
/// distance from the pivot interpolate, so the eye swings round the pivot rather than cutting
/// the chord between the two eyes (through the track between them). The elevation follows
/// `rise` instead of `t`.
pub fn mix_around(a: &Pose, b: &Pose, t: f32, rise: f32, pivot: Vec3) -> Pose {
    let polar = |e: Vec3| {
        let o = e - pivot;
        let r = o.length().max(1e-4);
        (o.x.atan2(o.z), (o.y / r).clamp(-1.0, 1.0).asin(), r)
    };
    let ((aa, ea, ra), (ab, eb, rb)) = (polar(a.eye), polar(b.eye));
    let (az, el, r) = (
        aa + wrap_angle(ab - aa) * t,
        ea + (eb - ea) * rise,
        ra + (rb - ra) * t,
    );
    let o = Vec3::new(el.cos() * az.sin(), el.sin(), el.cos() * az.cos()) * r;
    Pose {
        eye: pivot + o,
        forward: mix_dir(a.forward, b.forward, t),
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

/// Angle in (−π, π].
pub fn wrap_angle(a: f32) -> f32 {
    let t = std::f32::consts::TAU;
    let a = a.rem_euclid(t);
    if a > std::f32::consts::PI { a - t } else { a }
}

/// The follow shot's action window: its bounding-box centre, the radius around that centre that
/// holds every point (plus the marble's pad), and the chord from the oldest trail point to the end
/// of the look-ahead.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionWindow {
    pub centre: Vec3,
    pub radius: f32,
    pub chord: Vec3,
    /// The look-ahead from the marble.
    pub lead: Vec3,
    /// Every point of the window.
    pub points: Vec<Vec3>,
}

/// The action window of a marble with recent path `hist` (oldest first): the trail, the marble,
/// its look-ahead along its velocity, and the points of `interest` near that path.
pub fn action_window(
    m: MarbleIn,
    hist: &[(f32, Vec3)],
    ahead: f32,
    interest: &[Vec3],
) -> ActionWindow {
    let speed = m.v.length();
    let lead = if speed > 1e-3 {
        m.v / speed * (speed * FOLLOW_LEAD_S).max(ahead)
    } else {
        Vec3::ZERO
    };
    let mut pts: Vec<Vec3> = hist.iter().map(|(_, p)| *p).collect();
    pts.extend((1..=4).map(|k| m.p + lead * (k as f32 / 4.0)));
    pts.push(m.p);
    let path = pts.clone();
    pts.extend(
        interest
            .iter()
            .copied()
            .filter(|q| path.iter().any(|p| p.distance(*q) < FOLLOW_INTEREST_M)),
    );
    let (lo, hi) = pts
        .iter()
        .fold((pts[0], pts[0]), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    let centre = (lo + hi) * 0.5;
    let radius = pts.iter().map(|p| p.distance(centre)).fold(0.0, f32::max) + MARBLE_PAD_M;
    let oldest = hist.first().map_or(m.p, |(_, p)| *p);
    ActionWindow {
        centre,
        radius,
        chord: m.p + lead - oldest,
        lead,
        points: pts,
    }
}

/// Distance from `centre` along `-forward` at which every one of `points` (padded by the marble)
/// sits inside a view of vertical field of view `fov` and width/height `aspect`, `margin` loose.
/// Points are measured in the view's own basis, so a path running away from the camera, which
/// the view foreshortens, costs little distance.
pub fn fit_points(
    points: &[Vec3],
    centre: Vec3,
    forward: Vec3,
    fov: f32,
    aspect: f32,
    margin: f32,
) -> f32 {
    let right = forward.cross(Vec3::Y).normalize_or(Vec3::X);
    let up = right.cross(forward);
    let tv = (fov * 0.5).tan().max(1e-3);
    let th = tv * aspect.max(0.1);
    points
        .iter()
        .map(|p| {
            let r = *p - centre;
            let h = (r.dot(right).abs() + MARBLE_PAD_M) * margin / th;
            let v = (r.dot(up).abs() + MARBLE_PAD_M) * margin / tv;
            h.max(v) - r.dot(forward)
        })
        .fold(0.0, f32::max)
}

/// A piece that hides the marble (a drop shaft, a tower, a tunnel, a hole): its box, and where the
/// marble comes out, from the piece's exit port (metres).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HiddenPath {
    pub min: Vec3,
    pub max: Vec3,
    pub exit: Vec3,
}

impl HiddenPath {
    pub fn contains(&self, p: Vec3) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }
}

/// Solid track that can block the view: an axis-aligned box (metres).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Solid {
    pub min: Vec3,
    pub max: Vec3,
}

impl Solid {
    pub fn contains(&self, p: Vec3) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }

    /// Whether the segment `a`-`b` passes through the box (slab test).
    pub fn hit(&self, a: Vec3, b: Vec3) -> bool {
        let d = b - a;
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        for i in 0..3 {
            let (o, di, lo, hi) = (a[i], d[i], self.min[i], self.max[i]);
            if di.abs() < 1e-9 {
                if o < lo || o > hi {
                    return false;
                }
            } else {
                let (ta, tb) = ((lo - o) / di, (hi - o) / di);
                t0 = t0.max(ta.min(tb));
                t1 = t1.min(ta.max(tb));
                if t0 > t1 {
                    return false;
                }
            }
        }
        true
    }
}

/// Whether `eye` sees `target` past `solids`. A solid holding the target (the piece the marble
/// rides in) does not count.
pub fn line_of_sight(eye: Vec3, target: Vec3, solids: &[Solid]) -> bool {
    solids
        .iter()
        .filter(|s| !s.contains(target))
        .all(|s| !s.hit(eye, target))
}

/// Moves `x` towards `target` with the follow yaw's response, rate and acceleration limits.
fn approach(x: &mut f32, rate: &mut f32, target: f32, dt: f32) {
    approach_with(x, rate, target, dt, FOLLOW_MAX_YAW_ACCEL);
}

/// [`approach`] with its own acceleration limit (rad/s²).
fn approach_with(x: &mut f32, rate: &mut f32, target: f32, dt: f32, accel: f32) {
    let want =
        ((target - *x) / FOLLOW_YAW_RESPONSE_S).clamp(-FOLLOW_MAX_YAW_RATE, FOLLOW_MAX_YAW_RATE);
    let dv = accel * dt;
    *rate += (want - *rate).clamp(-dv, dv);
    *x += *rate * dt;
}

/// How far from the centre (in normalised device coordinates) the marble may sit before a zone's
/// hold starts to let it go.
pub const HOLD_FRAME_NDC: f32 = 0.9;
/// How long (seconds) the marble must stay past [`HOLD_FRAME_NDC`] before a zone's hold ends, so
/// one frame at the edge does not cut the shot.
pub const HOLD_EXIT_S: f32 = 0.25;

/// Whether `q` projects inside `pose`'s frame, within `lim` of the centre in normalised device
/// coordinates.
pub fn in_frame(pose: &Pose, aspect: f32, q: Vec3, lim: f32) -> bool {
    let right = pose.forward.cross(Vec3::Y).normalize_or(Vec3::X);
    let up = right.cross(pose.forward);
    let t = (pose.fov * 0.5).tan();
    let r = q - pose.eye;
    let z = r.dot(pose.forward);
    z > 0.0
        && (r.dot(right) / (z * t * aspect.max(0.1))).abs() < lim
        && (r.dot(up) / (z * t)).abs() < lim
}

/// One marble as the director sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarbleIn {
    pub p: Vec3,
    pub v: Vec3,
}

/// The inputs of one frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct Frame<'a> {
    /// Interpolated marbles in spawn order.
    pub marbles: &'a [MarbleIn],
    /// Camera time step (seconds, real time).
    pub dt: f32,
    /// Viewport width / height.
    pub aspect: f32,
    /// Completed runs (a change is a restart).
    pub run: u32,
    /// Points of interest (mechanisms, zone targets): the follow shot's action window takes in
    /// those near the marble's path.
    pub interest: &'a [Vec3],
    /// Pieces that hide the marble, and where it comes out of each.
    pub hidden: &'a [HiddenPath],
    /// Solid track that can block the view.
    pub solids: &'a [Solid],
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
#[derive(Debug, Clone)]
struct Shot {
    rule: Rule,
    marble: Option<usize>,
    started: bool,
    eye: Vec3,
    eye_vel: Vec3,
    look: Vec3,
    look_vel: Vec3,
    /// Framing: the bearing captured at the start, and the smoothed distance.
    azimuth: f32,
    dist: f32,
    dist_vel: Vec3,
    /// Orbit angle.
    angle: f32,
    /// Follow: the shot's own clock, the marble's recent path (time, position), the yaw rate, and
    /// the last heading, held while the marble is nearly still.
    clock: f32,
    hist: Vec<(f32, Vec3)>,
    yaw_rate: f32,
    heading: Option<f32>,
    /// Follow: the clear-view move in play (extra elevation and bearing), the elevation's
    /// rate-limited state, and how long the plain view has been clear.
    clear: (f32, f32),
    lift: f32,
    lift_rate: f32,
    /// Framing: the eased bearing swing of the clear-view move.
    swing: f32,
    swing_rate: f32,
    plain_clear_s: f32,
    lead_k: f32,
    last: Option<Pose>,
}

impl Shot {
    /// Picks the clear-view move in play from `moves` (cheapest first) and eases the elevation
    /// towards it with the follow rate limits. `clear` tests a move from where the shot is going,
    /// `blocked_now` says the camera's own eye is blocked (its springs lag), and `off` turns the
    /// search off (nothing solid). `at` is where the camera is now: its eased elevation and its
    /// bearing offset from the shot's own bearing.
    #[allow(clippy::too_many_arguments)]
    fn avoid(
        &mut self,
        off: bool,
        first: bool,
        dt: f32,
        clear: &dyn Fn((f32, f32)) -> bool,
        blocked_now: bool,
        at: (f32, f32),
        moves: &[(f32, f32)],
    ) {
        if off {
            self.clear = (0.0, 0.0);
            self.plain_clear_s = 0.0;
        } else {
            self.plain_clear_s = if clear((0.0, 0.0)) {
                self.plain_clear_s + dt
            } else {
                0.0
            };
            if first || self.plain_clear_s >= CLEAR_RELEASE_S {
                self.clear = (0.0, 0.0);
            }
            // The camera itself still blocked: try only the moves past the one in play.
            let from = if !first && blocked_now {
                moves
                    .iter()
                    .position(|c| *c == self.clear)
                    .map_or(0, |i| i + 1)
            } else {
                0
            };
            if from > 0 || !clear(self.clear) {
                // A swing keeps the side the camera is on: reversing it while the camera is still
                // on its way leaves the eased swing where it was, behind the same piece. An
                // escalation that runs out of moves keeps the one in play while that is clear
                // where it is going, else starts again from the cheapest clear one on that side
                // rather than holding a blocked view, and the other side is tried only while the
                // camera is at its own bearing.
                let swung = at.1.abs() >= CLEAR_SWUNG;
                let side = if swung { at.1 } else { self.clear.1 };
                let on_side = |c: &(f32, f32)| c.1 * side >= 0.0;
                let first_clear = |from: usize, side_only: bool| {
                    moves[from.min(moves.len())..]
                        .iter()
                        .copied()
                        .filter(|c| !side_only || on_side(c))
                        .find(|c| clear(*c))
                };
                self.clear = first_clear(from, true)
                    .or_else(|| clear(self.clear).then_some(self.clear))
                    .or_else(|| first_clear(0, true))
                    .or_else(|| (!swung).then(|| first_clear(0, false))?)
                    .unwrap_or(self.clear);
            }
        }
        if first {
            self.lift = self.clear.0;
            self.lift_rate = 0.0;
        } else {
            approach(&mut self.lift, &mut self.lift_rate, self.clear.0, dt);
        }
    }

    fn new(rule: Rule, marble: Option<usize>) -> Self {
        Self {
            rule,
            marble,
            started: false,
            eye: Vec3::ZERO,
            eye_vel: Vec3::ZERO,
            look: Vec3::ZERO,
            look_vel: Vec3::ZERO,
            azimuth: 0.0,
            dist: 0.0,
            dist_vel: Vec3::ZERO,
            angle: 0.0,
            clock: 0.0,
            hist: Vec::new(),
            yaw_rate: 0.0,
            heading: None,
            clear: (0.0, 0.0),
            lift: 0.0,
            lift_rate: 0.0,
            swing: 0.0,
            swing_rate: 0.0,
            plain_clear_s: 0.0,
            lead_k: 1.0,
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
                self.clock += dt;
                self.hist.push((self.clock, m.p));
                let keep = self.clock - FOLLOW_TRAIL_S;
                let old = self.hist.iter().take_while(|(t, _)| *t < keep).count();
                self.hist.drain(..old.min(self.hist.len() - 1));
                let mut w = action_window(m, &self.hist, ahead, f.interest);
                // A hidden path: the marble is out of sight, so the window reaches to where it
                // will come out, and the heading turns towards the exit.
                let hidden = f.hidden.iter().find(|h| h.contains(m.p));
                if let Some(h) = hidden {
                    w.points.push(h.exit);
                    let (lo, hi) = w
                        .points
                        .iter()
                        .fold((h.exit, h.exit), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
                    w.centre = (lo + hi) * 0.5;
                    w.chord = h.exit - self.hist.first().map_or(m.p, |(_, p)| *p);
                }
                let fov = FOV_DEG.to_radians();
                // Heading: the window's chord, from the oldest trail point to the end of the
                // look-ahead, so a corner turns the camera before the marble and a switchback
                // averages out instead of swinging it.
                let chord = Vec3::new(w.chord.x, 0.0, w.chord.z);
                let heading =
                    (chord.length() > FOLLOW_MIN_CHORD_M).then(|| azimuth_of(Vec3::ZERO, chord));
                if first {
                    // Start from the bearing the camera already has, so a shot change never
                    // swings it round: the yaw limiter turns it towards the heading from there. A
                    // near top-down view has no bearing worth keeping, so it starts on the heading.
                    self.azimuth = from
                        .filter(|p| p.forward.y > -FOLLOW_TOP_DOWN_Y)
                        .map(|p| azimuth_of(p.eye, w.centre))
                        .or(heading)
                        .unwrap_or(-std::f32::consts::FRAC_PI_2);
                    self.yaw_rate = 0.0;
                    self.heading = heading.or(Some(self.azimuth));
                } else if let Some(h) = heading.or(self.heading) {
                    // The bearing swing of the clear-view move in play joins the heading, through
                    // the same yaw limiter. A nearly still marble keeps its last heading, so a
                    // swing still turns the camera round it.
                    self.heading = Some(h);
                    let want = h + self.clear.1;
                    // Rate- and acceleration-limited yaw: a slow, eased swing whatever the turn.
                    let err = wrap_angle(want - self.azimuth);
                    let rate = (err / FOLLOW_YAW_RESPONSE_S)
                        .clamp(-FOLLOW_MAX_YAW_RATE, FOLLOW_MAX_YAW_RATE);
                    let dv = FOLLOW_MAX_YAW_ACCEL * dt;
                    self.yaw_rate += (rate - self.yaw_rate).clamp(-dv, dv);
                    self.azimuth = wrap_angle(self.azimuth + self.yaw_rate * dt);
                } else {
                    self.yaw_rate = 0.0;
                }
                // Distance: the rule's close shot, pulled back to fit the action window.
                let base = up.atan2(back);
                let near = back.hypot(up);
                let far = if hidden.is_some() {
                    FOLLOW_HIDDEN_MAX_M
                } else {
                    FOLLOW_MAX_M
                };
                let view = |azimuth: f32, lift: f32| {
                    let fwd = forward_of(azimuth, (base + lift).min(FOLLOW_MAX_ELEVATION));
                    let d = fit_points(&w.points, w.centre, fwd, fov, f.aspect, FOLLOW_FIT_MARGIN)
                        .clamp(near, far);
                    (fwd, w.centre - fwd * d)
                };
                // Line of sight: the marble and its look-ahead must stay in view past solid track.
                // A candidate view must be clear both at the bearing the camera has now (the yaw
                // limiter lags a turn) and at the bearing it is heading for, and against the
                // look-ahead as well as the marble, so the camera starts to rise or swing before
                // the marble passes behind a piece. A hidden path is the exception: there the
                // marble is meant to be out of sight.
                // While the marble is hidden, the view must see the exit instead, and a little
                // way out of it.
                let probes: Vec<Vec3> = match hidden {
                    Some(h) => {
                        let mid = (h.min + h.max) * 0.5;
                        let out = Vec3::new(h.exit.x - mid.x, 0.0, h.exit.z - mid.z);
                        vec![h.exit, h.exit + out.normalize_or_zero() * 0.04]
                    }
                    None => vec![m.p, m.p + w.lead * 0.5, m.p + w.lead, m.p + w.lead * 2.0],
                };
                let (az, eye_now) = (self.azimuth, self.eye);
                let aim = self.heading.unwrap_or(az);
                let sees = |eye: Vec3| probes.iter().all(|p| line_of_sight(eye, *p, f.solids));
                // Once the camera's own view is blocked, the bearing it has now is no constraint:
                // a move need only be clear where it is going, so a swing can still take it out.
                let blocked_now = !sees(eye_now);
                self.avoid(
                    f.solids.is_empty(),
                    first,
                    dt,
                    &|(lift, swing)| {
                        sees(view(aim + swing, lift).1) && (blocked_now || sees(view(az, lift).1))
                    },
                    blocked_now,
                    (self.lift, wrap_angle(az - aim)),
                    &CLEAR_VIEWS,
                );
                let (_, eye_t) = view(self.azimuth, self.lift);
                let look_t = w.centre;
                // While hidden, the marble's velocity is no guide to the shot: no spring lead,
                // and it eases back in over half a second once the marble is out.
                self.lead_k = if hidden.is_some() {
                    0.0
                } else {
                    (self.lead_k + dt / 0.5).min(1.0)
                };
                let lead_v = m.v * self.lead_k;
                let slow = 1.0;
                if first {
                    self.eye = eye_t;
                    self.look = look_t;
                } else {
                    // The targets lead by the marble's velocity × smoothing time, which cancels
                    // the springs' steady lag at constant speed.
                    self.eye = smooth_damp(
                        self.eye,
                        eye_t + lead_v * FOLLOW_EYE_SMOOTH_S,
                        &mut self.eye_vel,
                        FOLLOW_EYE_SMOOTH_S * slow,
                        dt,
                    );
                    self.look = smooth_damp(
                        self.look,
                        look_t + lead_v * FOLLOW_LOOK_SMOOTH_S,
                        &mut self.look_vel,
                        FOLLOW_LOOK_SMOOTH_S * slow,
                        dt,
                    );
                }
                let fwd = (self.look - self.eye).normalize_or(Vec3::NEG_Z);
                Pose {
                    eye: self.eye,
                    forward: fwd,
                    fov,
                    // The action area is in focus: the window's centre, not the marble point.
                    focus: depth_to(self.eye, fwd, self.look),
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
                    .chain([
                        m.p.distance(centre) + MARBLE_PAD_M,
                        (m.p + m.v * FRAME_LEAD_S).distance(centre) + MARBLE_PAD_M,
                    ])
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
                    let next = smooth_damp(
                        Vec3::splat(self.dist),
                        Vec3::splat(dist),
                        &mut self.dist_vel,
                        FRAME_SMOOTH_S,
                        dt,
                    )
                    .x;
                    // A marble rolling away from the target pulls the camera back no faster
                    // than this, so the shot never races after it.
                    let cap = FRAME_MAX_PULL_M_S * dt;
                    self.dist += (next - self.dist).clamp(-cap, cap);
                }
                // The marble stays in sight past solid track: the view rises when it would not.
                // A shot with a set bearing keeps it and only rises; one that took its bearing
                // from the camera may also swing round.
                let (az, look, dist) = (self.azimuth, self.look, self.dist);
                let eye_at = |lift: f32, swing: f32| {
                    look - forward_of(az + swing, (elevation + lift).min(FOLLOW_MAX_ELEVATION))
                        * dist
                };
                // While the marble is in a hidden path the view must see the exit instead, and a
                // little way out of it, as the follow shot's does: the camera is then already
                // round when the marble comes out, rather than easing back from the swing it took
                // on the way in.
                let probes: Vec<Vec3> = match f.hidden.iter().find(|h| h.contains(m.p)) {
                    Some(h) => {
                        let mid = (h.min + h.max) * 0.5;
                        let out = Vec3::new(h.exit.x - mid.x, 0.0, h.exit.z - mid.z);
                        vec![h.exit, h.exit + out.normalize_or_zero() * 0.04]
                    }
                    None => vec![m.p, m.p + m.v * 0.25, m.p + m.v * 0.5],
                };
                let eye_now = eye_at(self.lift, self.swing);
                let moves: &[(f32, f32)] = if azimuth.is_some() {
                    &FRAME_LIFTS
                } else {
                    &CLEAR_VIEWS
                };
                self.avoid(
                    f.solids.is_empty(),
                    first,
                    dt,
                    &|(lift, swing)| {
                        let e = eye_at(lift, swing);
                        probes.iter().all(|p| line_of_sight(e, *p, f.solids))
                    },
                    !line_of_sight(eye_now, probes[0], f.solids),
                    (self.lift, self.swing),
                    moves,
                );
                if first {
                    self.swing = self.clear.1;
                    self.swing_rate = 0.0;
                } else {
                    approach(&mut self.swing, &mut self.swing_rate, self.clear.1, dt);
                }
                let fwd = forward_of(
                    az + self.swing,
                    (elevation + self.lift).min(FOLLOW_MAX_ELEVATION),
                );
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
        /// A retarget blend's pivot, the incoming marble's latest position: the eye swings round
        /// it ([`mix_around`]) rather than along the chord from the old marble's view.
        around: Option<Vec3>,
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
            Self::Blend {
                from,
                to,
                t,
                dur,
                around,
            } => match (from.last_pose(), to.last) {
                (Some(a), Some(b)) => Some(blend(&a, &b, *t / *dur, *around)),
                (a, b) => b.or(a),
            },
        }
    }

    /// This frame's pose; advances blends by `f.dt` and collapses finished ones.
    fn eval(&mut self, f: &Frame, start: Option<&Pose>) -> Option<Pose> {
        match self {
            Self::Shot(s) => s.eval(f, start),
            Self::Frozen(p) => Some(*p),
            Self::Blend {
                from,
                to,
                t,
                dur,
                around,
            } => {
                let a = from.eval(f, None);
                let b = to.eval(f, a.as_ref().or(start));
                *t = (*t + f.dt.max(0.0)).min(*dur);
                let progress = *t / *dur;
                if around.is_some()
                    && let Some(m) = to.marble.and_then(|i| f.marbles.get(i))
                {
                    *around = Some(m.p);
                }
                let pose = match (a, b) {
                    (Some(a), Some(b)) => Some(blend(&a, &b, progress, *around)),
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

/// A blend's pose `progress` (0 to 1, linear) of the way: about its pivot when it has one
/// ([`mix_around`]), else along the chord, both eased. About a pivot the elevation leads on an
/// ease-out: an old view level with the new marble looks through the track around it, and
/// rising first clears it soonest.
fn blend(a: &Pose, b: &Pose, progress: f32, around: Option<Vec3>) -> Pose {
    let w = ease(progress);
    match around {
        Some(pivot) => {
            let u = 1.0 - progress;
            mix_around(a, b, w, 1.0 - u * u * u, pivot)
        }
        None => mix(a, b, w),
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
    /// The followed marble's hidden path, when it is in one: where it will come out and the
    /// estimated seconds until it does (distance over its speed).
    pub hidden: Option<HiddenDiag>,
    /// Whether the camera sees the followed marble past solid track this frame.
    pub clear: Option<bool>,
}

/// A hidden path the followed marble is in.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HiddenDiag {
    pub exit: [f32; 3],
    pub eta_s: f32,
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
    /// Seconds the followed marble has sat past [`HOLD_FRAME_NDC`] in a row.
    out_of_frame_s: f32,
    pub diag: DirectorDiag,
    /// The rise applied to the mixed pose when a blend between two clear shots still looks
    /// through solid track: the move in play, its eased state, and the plain view's clear time.
    post: (f32, f32),
    post_lift: f32,
    post_rate: f32,
    post_clear_s: f32,
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
        // A zone's hold never leaves the marble out of frame: once it has left the volume and the
        // camera has not had it in frame for [`HOLD_EXIT_S`], the hold ends and the shot blends
        // back.
        let out = matches!(
            (p, self.layer.as_ref().and_then(Layer::last_pose)),
            (Some(p), Some(pose)) if !in_frame(&pose, f.aspect, p, HOLD_FRAME_NDC)
        );
        self.out_of_frame_s = if out && !restarted {
            self.out_of_frame_s + dt
        } else {
            0.0
        };
        if self.out_of_frame_s >= HOLD_EXIT_S {
            for i in 0..zones.len() {
                if !self.inside[i] {
                    self.since[i] = f32::INFINITY;
                }
            }
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
                    // The followed marble changed: swing round the new one.
                    let around = (retarget && !restarted)
                        .then(|| self.followed.and_then(|i| f.marbles.get(i)).map(|m| m.p))
                        .flatten();
                    Layer::Blend {
                        from: Box::new(from),
                        to: shot,
                        t: 0.0,
                        dur,
                        around,
                    }
                }
            });
            self.key = Some(key);
        }
        let start = self.layer.as_ref().and_then(Layer::last_pose);
        let pose = self.layer.as_mut()?.eval(f, start.as_ref());
        let pose = pose.map(|p| self.keep_in_sight(f, p));

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
        let m = self.followed.and_then(|i| f.marbles.get(i)).copied();
        d.hidden = m.and_then(|m| {
            let h = f.hidden.iter().find(|h| h.contains(m.p))?;
            Some(HiddenDiag {
                exit: h.exit.to_array(),
                eta_s: m.p.distance(h.exit) / m.v.length().max(0.2),
            })
        });
        d.clear = m
            .zip(pose)
            .map(|(m, p)| line_of_sight(p.eye, m.p, f.solids));
        pose
    }
}

impl Director {
    /// The mixed pose, risen about the marble's depth when it would look through solid track
    /// (a blend mixes two clear views into one that need not be). The rise eases with the follow
    /// yaw's response and rate limit but the quicker [`BLEND_RISE_ACCEL`], and is released once the
    /// plain pose has been clear for [`CLEAR_RELEASE_S`].
    fn keep_in_sight(&mut self, f: &Frame, p: Pose) -> Pose {
        let m = self.followed.and_then(|i| f.marbles.get(i)).copied();
        let dt = f.dt.max(0.0);
        // Only a blend needs it: a settled shot keeps itself clear, and stacking the two rises
        // would double the pitch rate.
        let blending = matches!(self.layer, Some(Layer::Blend { .. }));
        let Some(m) = m.filter(|m| {
            blending && !f.solids.is_empty() && !f.hidden.iter().any(|h| h.contains(m.p))
        }) else {
            self.post = (0.0, 0.0);
            approach(&mut self.post_lift, &mut self.post_rate, 0.0, dt);
            return self.lifted(p, None);
        };
        let depth = (m.p - p.eye).dot(p.forward).max(0.05);
        let pivot = p.eye + p.forward * depth;
        let probes = [m.p, m.p + m.v * 0.25];
        let clear = |lift: f32| {
            let e = Self::rise(p, pivot, lift).eye;
            probes.iter().all(|q| line_of_sight(e, *q, f.solids))
        };
        self.post_clear_s = if clear(0.0) {
            self.post_clear_s + dt
        } else {
            0.0
        };
        if self.post_clear_s >= CLEAR_RELEASE_S {
            self.post = (0.0, 0.0);
        }
        let blocked_now = !clear(self.post_lift);
        if blocked_now || !clear(self.post.0) {
            let from = FRAME_LIFTS
                .iter()
                .position(|c| *c == self.post)
                .map_or(0, |i| if blocked_now { i + 1 } else { 0 });
            self.post = FRAME_LIFTS[from.min(FRAME_LIFTS.len() - 1)..]
                .iter()
                .copied()
                .find(|c| clear(c.0))
                // No lift clears (the marble is under a piece): rising further buys nothing.
                .unwrap_or(self.post);
        }
        approach_with(
            &mut self.post_lift,
            &mut self.post_rate,
            self.post.0,
            dt,
            BLEND_RISE_ACCEL,
        );
        self.lifted(p, Some(pivot))
    }

    fn lifted(&self, p: Pose, pivot: Option<Vec3>) -> Pose {
        if self.post_lift.abs() < 1e-4 {
            return p;
        }
        Self::rise(p, pivot.unwrap_or_else(|| p.look()), self.post_lift)
    }

    /// `p` swung up by `lift` about `pivot`, still looking at it.
    fn rise(p: Pose, pivot: Vec3, lift: f32) -> Pose {
        let r = pivot.distance(p.eye).max(0.01);
        let az = azimuth_of(p.eye, pivot);
        let el = (-p.forward.y).clamp(-1.0, 1.0).asin();
        let fwd = forward_of(az, (el + lift).min(FOLLOW_MAX_ELEVATION));
        let eye = pivot - fwd * r;
        Pose {
            eye,
            forward: fwd,
            fov: p.fov,
            focus: p.focus,
        }
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
            interest: &[],
            ..Default::default()
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
            // Speed changes at 4 m/s² (a marble rebounding off a wall), never instantly.
            let want = match i {
                0..100 => 0.5,
                100..200 => -0.5,
                _ => 0.0,
            };
            vx += (want - vx).clamp(-4.0 * DT, 4.0 * DT);
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
                        interest: &[],
                        ..Default::default()
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
        // Just out of both, still in frame: the hold keeps zone 0 for 0.5 s, then the cue returns.
        for _ in 0..20 {
            d.update(&frame(&at(2.02), 0), Some((0, Rule::FOLLOW)), &zones);
        }
        assert_eq!(d.diag.zone, Some(0));
        for _ in 0..30 {
            d.update(&frame(&at(2.02), 0), Some((0, Rule::FOLLOW)), &zones);
        }
        assert_eq!(d.diag.source, Some("cue"));
        // A marble that leaves the frame ends the hold once it has been out for HOLD_EXIT_S.
        d.update(&frame(&at(0.5), 0), Some((0, Rule::FOLLOW)), &zones);
        assert_eq!(d.diag.zone, Some(0));
        let mut out_s = 0.0;
        while out_s + DT < HOLD_EXIT_S {
            d.update(&frame(&at(6.0), 0), Some((0, Rule::FOLLOW)), &zones);
            out_s += DT;
        }
        assert_eq!(
            d.diag.zone,
            Some(0),
            "one moment out of frame keeps the hold"
        );
        d.update(&frame(&at(6.0), 0), Some((0, Rule::FOLLOW)), &zones);
        d.update(&frame(&at(6.0), 0), Some((0, Rule::FOLLOW)), &zones);
        assert_eq!(d.diag.source, Some("cue"), "the hold let the marble go");
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

    /// A marble path: `n` frames of (position, velocity). Straight +X at `v` for `straight` frames,
    /// a 90° left turn onto −Z round a 90 mm arc (the kit's rail curve), then straight −Z.
    fn corner_path(v: f32, straight: usize, after: usize) -> Vec<MarbleIn> {
        let r = 0.09;
        let mut out = Vec::new();
        let mut p = Vec3::new(0.0, 0.1, 0.0);
        for _ in 0..straight {
            p.x += v * DT;
            out.push(MarbleIn { p, v: Vec3::X * v });
        }
        let c = p + Vec3::new(0.0, 0.0, -r);
        let arc_frames = ((std::f32::consts::FRAC_PI_2 * r / v) / DT).round() as usize;
        for k in 1..=arc_frames {
            let a = std::f32::consts::FRAC_PI_2 * k as f32 / arc_frames as f32;
            let q = c + Vec3::new(a.sin() * r, 0.0, a.cos() * r);
            let dir = Vec3::new(a.cos(), 0.0, -a.sin());
            out.push(MarbleIn { p: q, v: dir * v });
        }
        p = *out.last().map(|m| &m.p).unwrap();
        for _ in 0..after {
            p.z -= v * DT;
            out.push(MarbleIn {
                p,
                v: Vec3::NEG_Z * v,
            });
        }
        out
    }

    fn yaw_of(f: Vec3) -> f32 {
        (-f.x).atan2(-f.z)
    }

    /// Through a 90° corner the marble's heading turns at v / r = 5.6 rad/s; the follow camera's
    /// view turns under 1 rad/s (measured 0.93, 53°/s) and its yaw rate changes under 3 rad/s²
    /// (measured 1.98): the limits are [`FOLLOW_MAX_YAW_RATE`] and [`FOLLOW_MAX_YAW_ACCEL`].
    #[test]
    fn a_corner_does_not_whip_the_follow_camera() {
        let path = corner_path(0.5, 60, 120);
        let mut d = Director::default();
        let (mut last, mut prev_rate) = (None::<f32>, 0f32);
        let (mut peak_rate, mut peak_accel) = (0f32, 0f32);
        for (i, m) in path.iter().enumerate() {
            let p = d
                .update(
                    &frame(std::slice::from_ref(m), 0),
                    Some((0, Rule::FOLLOW)),
                    &[],
                )
                .unwrap();
            let yaw = yaw_of(p.forward);
            if let Some(l) = last {
                let rate = wrap_angle(yaw - l) / DT;
                peak_rate = peak_rate.max(rate.abs());
                if i > 2 {
                    peak_accel = peak_accel.max(((rate - prev_rate) / DT).abs());
                }
                prev_rate = rate;
            }
            last = Some(yaw);
        }
        println!("corner: peak yaw {peak_rate:.3} rad/s, accel {peak_accel:.3} rad/s²");
        assert!(peak_rate < 1.0, "peak yaw rate {peak_rate} rad/s");
        assert!(
            peak_accel < 3.0,
            "peak yaw acceleration {peak_accel} rad/s²"
        );
        // It still ends up behind the marble on the new heading.
        assert!(
            wrap_angle(last.unwrap() - 0.0).abs() < 0.35,
            "final yaw {:?}",
            last
        );
    }

    /// On a straight the look point leads the marble and the eye trails it.
    #[test]
    fn the_follow_look_point_leads_the_marble() {
        let path = corner_path(0.5, 150, 0);
        let mut d = Director::default();
        let mut pose = None;
        for m in &path[..120] {
            pose = d.update(
                &frame(std::slice::from_ref(m), 0),
                Some((0, Rule::FOLLOW)),
                &[],
            );
        }
        let (pose, m) = (pose.unwrap(), path[119]);
        let look = pose.look();
        assert!(look.x - m.p.x > 0.03, "look {look} vs marble {}", m.p);
        assert!(pose.eye.x < m.p.x, "eye {} is behind", pose.eye);
        // The focus is on the action window's centre, ahead of the marble.
        let marble_depth = (m.p - pose.eye).dot(pose.forward);
        assert!(
            pose.focus > marble_depth,
            "focus {} vs marble {marble_depth}",
            pose.focus
        );
    }

    /// A mechanism just ahead joins the action window and pulls the look point towards it.
    #[test]
    fn a_point_of_interest_ahead_joins_the_action_window() {
        let m = MarbleIn {
            p: Vec3::new(0.0, 0.1, 0.0),
            v: Vec3::X * 0.5,
        };
        let hist = [(0.0, Vec3::new(-0.15, 0.1, 0.0))];
        let plain = action_window(m, &hist, FOLLOW_AHEAD_M, &[]);
        let side = [Vec3::new(0.2, 0.1, 0.055), Vec3::new(2.0, 0.1, 0.0)];
        let with = action_window(m, &hist, FOLLOW_AHEAD_M, &side);
        assert!(with.centre.z > plain.centre.z + 0.02);
        assert!(with.radius > plain.radius);
        assert!(
            plain.centre.x > m.p.x,
            "the window leans ahead of the marble"
        );
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

    /// Projects `q` into normalised device coordinates of `pose`: (depth, ndc).
    fn ndc(pose: &Pose, aspect: f32, q: Vec3) -> (f32, Vec2) {
        let right = pose.forward.cross(Vec3::Y).normalize();
        let up = right.cross(pose.forward);
        let t = (pose.fov * 0.5).tan();
        let r = q - pose.eye;
        let z = r.dot(pose.forward);
        (
            z,
            Vec2::new(r.dot(right) / (z * t * aspect), r.dot(up) / (z * t)),
        )
    }

    /// Peak yaw and pitch rates of a pose sequence (rad/s).
    fn peak_rates(poses: &[Pose]) -> (f32, f32) {
        let pitch = |f: Vec3| f.y.asin();
        poses.windows(2).fold((0f32, 0f32), |(y, p), w| {
            (
                y.max((wrap_angle(yaw_of(w[1].forward) - yaw_of(w[0].forward)) / DT).abs()),
                p.max(((pitch(w[1].forward) - pitch(w[0].forward)) / DT).abs()),
            )
        })
    }

    /// The marble rolls into a drop shaft, falls out of sight and comes out of the shaft's side
    /// port heading a new way. While it is hidden the camera turns and pulls back to the exit the
    /// track predicts, through the follow yaw limits, so the exit is in frame when it reappears.
    #[test]
    fn a_hidden_path_puts_its_exit_in_view_before_the_marble_reappears() {
        let shaft = HiddenPath {
            min: Vec3::new(0.50, 0.0, -0.03),
            max: Vec3::new(0.56, 0.24, 0.03),
            exit: Vec3::new(0.53, 0.03, 0.03),
        };
        let mut path = Vec::new();
        let mut p = Vec3::new(0.0, 0.2, 0.0);
        let mut push = |p: Vec3, v: Vec3| path.push(MarbleIn { p, v });
        while p.x < 0.53 {
            p.x += 0.5 * DT;
            push(p, Vec3::X * 0.5);
        }
        while p.y > 0.03 {
            p.y -= 0.4 * DT;
            push(p, Vec3::NEG_Y * 0.4);
        }
        for _ in 0..90 {
            p.z += 0.3 * DT;
            push(p, Vec3::Z * 0.3);
        }
        let hidden = [shaft];
        let mut d = Director::default();
        let aspect = 16.0 / 9.0;
        let mut poses = Vec::new();
        let mut reappeared = None;
        let mut was_hidden = false;
        let mut saw_eta = false;
        for m in &path {
            let pose = d
                .update(
                    &Frame {
                        marbles: std::slice::from_ref(m),
                        dt: DT,
                        aspect,
                        hidden: &hidden,
                        ..Default::default()
                    },
                    Some((0, Rule::FOLLOW)),
                    &[],
                )
                .unwrap();
            poses.push(pose);
            let inside = shaft.contains(m.p);
            saw_eta |= d.diag.hidden.is_some_and(|h| h.eta_s > 0.0);
            if was_hidden && !inside && reappeared.is_none() {
                reappeared = Some(pose);
            }
            was_hidden |= inside;
        }
        assert!(saw_eta, "the diag reports the exit and its time");
        let pose = reappeared.expect("the marble comes out");
        let (z, n) = ndc(&pose, aspect, shaft.exit);
        assert!(z > 0.0, "exit behind the camera");
        assert!(n.x.abs() < 1.0 && n.y.abs() < 1.0, "exit off frame at {n}");
        let (yaw, _) = peak_rates(&poses);
        assert!(yaw < 1.0, "peak yaw rate {yaw} rad/s");
    }

    /// A tall piece stands where the plain follow view of the corner would look through it. With
    /// the solids known, the camera rises or swings ahead of the blockage, within the follow rate
    /// limits, and sees the marble in at least 95% of frames.
    #[test]
    fn a_tall_piece_never_hides_the_followed_marble() {
        let path = corner_path(0.5, 60, 120);
        let run = |solids: &[Solid]| {
            let mut d = Director::default();
            path.iter()
                .map(|m| {
                    d.update(
                        &Frame {
                            marbles: std::slice::from_ref(m),
                            dt: DT,
                            aspect: 16.0 / 9.0,
                            solids,
                            ..Default::default()
                        },
                        Some((0, Rule::FOLLOW)),
                        &[],
                    )
                    .unwrap()
                })
                .collect::<Vec<Pose>>()
        };
        // The tower: a 40 mm column, 0.3 m tall, halfway along the plain view's line of sight
        // to the marble a little after the corner.
        let plain = run(&[]);
        let k = 150;
        let mid = (plain[k].eye + path[k].p) * 0.5;
        let tower = Solid {
            min: Vec3::new(mid.x - 0.02, 0.0, mid.z - 0.02),
            max: Vec3::new(mid.x + 0.02, 0.3, mid.z + 0.02),
        };
        assert!(path.iter().all(|m| !tower.contains(m.p)), "off the path");
        let solids = [tower];
        let seen = |poses: &[Pose]| {
            poses
                .iter()
                .zip(&path)
                .filter(|(p, m)| line_of_sight(p.eye, m.p, &solids))
                .count() as f32
                / path.len() as f32
        };
        let blind = seen(&plain);
        assert!(blind < 0.95, "the tower blocks the plain view ({blind})");
        let avoided = run(&solids);
        let clear = seen(&avoided);
        let (yaw, pitch) = peak_rates(&avoided);
        println!(
            "line of sight: plain {blind:.3}, avoiding {clear:.3}; peak yaw {yaw:.3}, pitch {pitch:.3} rad/s"
        );
        assert!(clear >= 0.95, "clear line of sight in {clear} of frames");
        assert!(yaw < 1.0, "peak yaw rate {yaw} rad/s");
        assert!(pitch < 1.0, "peak pitch rate {pitch} rad/s");
    }
}
