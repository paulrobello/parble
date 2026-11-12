//! The rope-curtain arch's ropes as kinematic chains (`mech_arch_rope_l1`, [`crate::behaviour::RopeArch`]).
//!
//! Each rope is [`SEGMENTS`] rigid segments of [`SEGMENT_M`] hung from its sidecar pivot, all in the
//! rope's swing plane (the plane through the pivot normal to its hinge axis). A segment's state is
//! its absolute angle from hanging straight down (radians, positive toward the part's positive
//! swing) and its angular velocity (radians per tick). No Rapier joint or collider is involved: the
//! chain is advanced once per tick by [`RopeChain::step`], in plain `f32` with `libm` trig, so it is
//! the same on every platform.
//!
//! - Segment 0 is a damped pendulum about the pivot ([`PERIOD_TICKS`], [`DECAY_TICKS`]).
//! - Segment `j > 0` is a damped spring follower of the segment above it, aiming at
//!   [`SAG`] × its angle (gravity pulls the lower rope back toward hanging), so a push travels down
//!   the rope with a lag and the free end trails and whips.
//! - Marbles are discs in the swing plane (the marble's section at the rope, grown by the strand
//!   radius and [`CLEAR_M`]). Every segment that cuts a disc is turned about its top joint until it
//!   clears, top to bottom, pushing its nearest point radially out of the marble. The turn feeds the
//!   segment's velocity ([`PUSH_VELOCITY_SHARE`]), so the rope keeps moving after the marble leaves.
//! - A rope whose motion falls under [`REST_RAD`] snaps to rest, exactly 0.
//!
//! [`polyline`] turns segment angles into joint points; the renderer draws the strand along it.

/// Segments per rope.
pub const SEGMENTS: usize = 6;
/// Rope length from the pivot (the arch underside) to the tip of its knot, µm.
pub const LENGTH_UM: i32 = 28_000;
/// Length of one segment, m.
pub const SEGMENT_M: f32 = LENGTH_UM as f32 * 1.0e-6 / SEGMENTS as f32;
/// Strand radius, m (the generator's `ROPE_R`; the knot is wider, 1.6 mm, over the last 3 mm).
pub const STRAND_R_M: f32 = 0.001;
/// Extra clearance between the strand surface and a marble, m: it covers the knot, the polygonal
/// strand and a segment's sag between its joints.
pub const CLEAR_M: f32 = 0.0008;
/// Period of the top segment's free swing, ticks.
pub const PERIOD_TICKS: f32 = 76.0;
/// Time constant of the top segment's amplitude decay, ticks.
pub const DECAY_TICKS: f32 = 70.0;
/// Period of a follower segment's spring toward the one above it, ticks.
pub const FOLLOW_PERIOD_TICKS: f32 = 34.0;
/// Damping ratio of a follower segment's spring.
pub const FOLLOW_DAMPING: f32 = 0.3;
/// Share of the segment above's angle a follower aims at.
pub const SAG: f32 = 0.85;
/// Kick boost over a free swing's rate ([`RopeChain::kick`]).
pub const KICK_GAIN: f32 = 1.3;
/// Share of a contact turn that becomes the segment's angular velocity.
pub const PUSH_VELOCITY_SHARE: f32 = 0.6;
/// Fastest a segment turns, radians per tick (about 1,650°/s).
pub const MAX_RATE: f32 = 0.12;
/// Furthest a segment turns from hanging, radians (about 172°: a strand end may wrap over the top
/// of a marble passing under the rope).
pub const MAX_ANGLE: f32 = 3.0;
/// A rope whose every angle and rate is below this (radians, radians per tick) snaps to rest.
pub const REST_RAD: f32 = 2.0e-4;
/// Clearance passes per tick.
const CLEAR_PASSES: usize = 3;
/// Position-based contact iterations per tick.
const SOLVE_ITERATIONS: usize = 8;

/// A marble's section in a rope's swing plane: centre `(u, y)` from the pivot (m, `u` along the
/// swing), radius `rho` (m, already grown by the strand radius and clearance), and its velocity
/// along `u` (m/s), which picks the push side when the marble's centre is on the strand.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Disc {
    pub u: f32,
    pub y: f32,
    pub rho: f32,
    pub vu: f32,
}

/// One rope: segment angles (radians) and rates (radians per tick), top first.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RopeChain {
    pub angle: [f32; SEGMENTS],
    pub rate: [f32; SEGMENTS],
}

/// Joint points of a rope with segment `angles` (radians), from the pivot `(0, 0)` to the tip:
/// `(u, y)` in the swing plane, m.
pub fn polyline(angles: &[f32; SEGMENTS]) -> [(f32, f32); SEGMENTS + 1] {
    let mut out = [(0.0, 0.0); SEGMENTS + 1];
    for (j, &a) in angles.iter().enumerate() {
        let (u, y) = out[j];
        out[j + 1] = (u + SEGMENT_M * libm::sinf(a), y - SEGMENT_M * libm::cosf(a));
    }
    out
}

/// [`polyline`] of angles in millidegrees.
pub fn polyline_mdeg(mdeg: &[i32]) -> [(f32, f32); SEGMENTS + 1] {
    let mut a = [0.0; SEGMENTS];
    for (o, &m) in a.iter_mut().zip(mdeg) {
        *o = m as f32 * (core::f32::consts::PI / 180_000.0);
    }
    polyline(&a)
}

/// Radians to millidegrees, rounded.
pub fn to_mdeg(rad: f32) -> i32 {
    libm::roundf(rad * (180_000.0 / core::f32::consts::PI)) as i32
}

/// Closest point to `q` on the segment from `a` along unit `d` for [`SEGMENT_M`]: `(t, point)`
/// with `t` in `[0, 1]`.
fn closest(a: (f32, f32), d: (f32, f32), q: (f32, f32)) -> (f32, (f32, f32)) {
    let along = (q.0 - a.0) * d.0 + (q.1 - a.1) * d.1;
    let t = (along / SEGMENT_M).clamp(0.0, 1.0);
    (t, (a.0 + d.0 * SEGMENT_M * t, a.1 + d.1 * SEGMENT_M * t))
}

impl RopeChain {
    /// At rest: hanging straight down and still.
    pub fn is_rest(&self) -> bool {
        self.angle.iter().chain(&self.rate).all(|&v| v == 0.0)
    }

    /// Kick the rope's top segment so the rope's chord swings out to about `peak` radians: the top
    /// segment gets the rate of a free swing [`KICK_GAIN`] times larger, for the decay and the lower
    /// segments' sag.
    pub fn kick(&mut self, peak: f32) {
        let omega = 2.0 * core::f32::consts::PI / PERIOD_TICKS;
        self.rate[0] = (self.rate[0] + KICK_GAIN * peak * omega).clamp(-MAX_RATE, MAX_RATE);
    }

    /// The pivot-to-tip chord's angle from hanging, radians: the rope's swing as one part angle.
    pub fn chord(&self) -> f32 {
        let tip = polyline(&self.angle)[SEGMENTS];
        libm::atan2f(tip.0, -tip.1)
    }

    /// Advance one tick against the marble `discs`.
    pub fn step(&mut self, discs: &[Disc]) {
        if self.is_rest() && discs.is_empty() {
            return;
        }
        let w0 = 2.0 * core::f32::consts::PI / PERIOD_TICKS;
        let c0 = 2.0 / DECAY_TICKS;
        let wf = 2.0 * core::f32::consts::PI / FOLLOW_PERIOD_TICKS;
        let cf = 2.0 * FOLLOW_DAMPING * wf;
        let (before, before_rate) = (self.angle, self.rate);
        for j in 0..SEGMENTS {
            let acc = if j == 0 {
                -w0 * w0 * before[0] - c0 * before_rate[0]
            } else {
                -wf * wf * (before[j] - SAG * before[j - 1])
                    - cf * (before_rate[j] - SAG * before_rate[j - 1])
                    - c0 * before_rate[j]
            };
            self.rate[j] = (self.rate[j] + acc).clamp(-MAX_RATE, MAX_RATE);
            self.angle[j] = (self.angle[j] + self.rate[j]).clamp(-MAX_ANGLE, MAX_ANGLE);
        }

        if !discs.is_empty() {
            let free = self.angle;
            self.solve(discs);
            for _ in 0..CLEAR_PASSES {
                self.clear(discs);
            }
            for ((rate, &angle), &was) in self.rate.iter_mut().zip(&self.angle).zip(&free) {
                *rate = (*rate + PUSH_VELOCITY_SHARE * (angle - was)).clamp(-MAX_RATE, MAX_RATE);
            }
        }

        let still = self
            .angle
            .iter()
            .chain(&self.rate)
            .all(|v| v.abs() < REST_RAD);
        if still && discs.is_empty() {
            *self = Self::default();
        }
    }

    /// Position-based contact solve: the joints as points (the pivot fixed), each segment's nearest
    /// point inside a disc pushed radially out to its edge and shared between the segment's two
    /// ends, then the segment lengths restored, [`SOLVE_ITERATIONS`] times. The whole rope above a
    /// contact gives way with it, so the rope swings out of the marble's way and bends where the
    /// marble touches it. Then the angles are read back from the points.
    fn solve(&mut self, discs: &[Disc]) {
        let mut p = polyline(&self.angle);
        for _ in 0..SOLVE_ITERATIONS {
            for j in 0..SEGMENTS {
                for disc in discs {
                    let (a, b) = (p[j], p[j + 1]);
                    let (eu, ey) = (b.0 - a.0, b.1 - a.1);
                    let len2 = eu * eu + ey * ey;
                    if len2 <= 0.0 {
                        continue;
                    }
                    let t = (((disc.u - a.0) * eu + (disc.y - a.1) * ey) / len2).clamp(0.0, 1.0);
                    let c = (a.0 + eu * t, a.1 + ey * t);
                    let (du, dy) = (c.0 - disc.u, c.1 - disc.y);
                    let dist = libm::sqrtf(du * du + dy * dy);
                    if dist >= disc.rho {
                        continue;
                    }
                    let n = if dist > 1.0e-7 {
                        (du / dist, dy / dist)
                    } else if disc.vu >= 0.0 {
                        (1.0, 0.0)
                    } else {
                        (-1.0, 0.0)
                    };
                    let depth = disc.rho - dist;
                    // Inverse masses: the pivot is fixed.
                    let wa = if j == 0 { 0.0 } else { 1.0 };
                    let (sa, sb) = (wa * (1.0 - t), t);
                    let denom = sa * (1.0 - t) + sb * t;
                    if denom <= 0.0 {
                        continue;
                    }
                    let k = depth / denom;
                    p[j] = (p[j].0 + n.0 * k * sa, p[j].1 + n.1 * k * sa);
                    p[j + 1] = (p[j + 1].0 + n.0 * k * sb, p[j + 1].1 + n.1 * k * sb);
                }
            }
            for j in 0..SEGMENTS {
                let (a, b) = (p[j], p[j + 1]);
                let (eu, ey) = (b.0 - a.0, b.1 - a.1);
                let len = libm::sqrtf(eu * eu + ey * ey);
                if len <= 0.0 {
                    continue;
                }
                let err = (len - SEGMENT_M) / len;
                if j == 0 {
                    p[1] = (b.0 - eu * err, b.1 - ey * err);
                } else {
                    let h = 0.5 * err;
                    p[j] = (a.0 + eu * h, a.1 + ey * h);
                    p[j + 1] = (b.0 - eu * h, b.1 - ey * h);
                }
            }
        }
        for j in 0..SEGMENTS {
            let (a, b) = (p[j], p[j + 1]);
            self.angle[j] = libm::atan2f(b.0 - a.0, -(b.1 - a.1)).clamp(-MAX_ANGLE, MAX_ANGLE);
        }
    }

    /// One clearance pass, top to bottom: turn every segment that cuts a disc about its top joint
    /// so its nearest point moves radially out to the disc's edge.
    fn clear(&mut self, discs: &[Disc]) {
        let mut top = (0.0_f32, 0.0_f32);
        for angle in &mut self.angle {
            for disc in discs {
                let q = (disc.u, disc.y);
                let d = (libm::sinf(*angle), -libm::cosf(*angle));
                let (t, c) = closest(top, d, q);
                if t <= 0.0 {
                    // The top joint itself is inside: only the segments above can move it.
                    continue;
                }
                let (du, dy) = (c.0 - q.0, c.1 - q.1);
                let dist = libm::sqrtf(du * du + dy * dy);
                if dist >= disc.rho {
                    continue;
                }
                // Push direction: away from the centre, or along the marble's travel when the
                // centre is on the strand.
                let n = if dist > 1.0e-7 {
                    (du / dist, dy / dist)
                } else if disc.vu >= 0.0 {
                    (1.0, 0.0)
                } else {
                    (-1.0, 0.0)
                };
                let target = (q.0 + n.0 * disc.rho, q.1 + n.1 * disc.rho);
                let a = libm::atan2f(target.0 - top.0, -(target.1 - top.1));
                *angle = a.clamp(-MAX_ANGLE, MAX_ANGLE);
            }
            top = (
                top.0 + SEGMENT_M * libm::sinf(*angle),
                top.1 - SEGMENT_M * libm::cosf(*angle),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rope_at_rest_hangs_straight_and_stays_put() {
        let mut r = RopeChain::default();
        r.step(&[]);
        assert!(r.is_rest());
        let p = polyline(&r.angle);
        assert_eq!(p[SEGMENTS].0, 0.0);
        assert!((p[SEGMENTS].1 + LENGTH_UM as f32 * 1.0e-6).abs() < 1.0e-7);
        assert_eq!(to_mdeg(r.chord()), 0);
    }

    #[test]
    fn a_kicked_rope_bends_lags_and_settles_to_rest() {
        let mut r = RopeChain::default();
        r.kick(0.4);
        let mut bent = false;
        let mut ticks = 0;
        while !r.is_rest() {
            r.step(&[]);
            ticks += 1;
            // The free end lags the top early in the swing: the rope curves.
            if ticks == 6 {
                assert!(r.angle[0] > r.angle[SEGMENTS - 1], "{:?}", r.angle);
            }
            let spread = r.angle.iter().fold(f32::MIN, |m, &a| m.max(a))
                - r.angle.iter().fold(f32::MAX, |m, &a| m.min(a));
            bent |= spread > 0.1;
            assert!(ticks < 2_000, "never settles");
        }
        assert!(bent);
        assert!(ticks < 800, "settles in {ticks} ticks");
    }

    #[test]
    fn a_disc_pushes_the_rope_out_and_bends_it_round_the_marble() {
        // A 16 mm marble's section, centre 25 mm below the pivot and 3 mm behind the strand.
        let disc = Disc {
            u: -0.003,
            y: -0.025,
            rho: 0.008 + STRAND_R_M + CLEAR_M,
            vu: 0.3,
        };
        let mut r = RopeChain::default();
        r.step(&[disc]);
        for (u, y) in polyline(&r.angle) {
            let d = libm::sqrtf((u - disc.u).powi(2) + (y - disc.y).powi(2));
            assert!(d >= disc.rho - 1.0e-4, "joint ({u}, {y}) {d} inside");
        }
        // The contact region turns most; the top barely moves.
        assert!(
            r.angle[0].abs() < r.angle[SEGMENTS - 2].abs(),
            "{:?}",
            r.angle
        );
        assert!(
            r.angle[SEGMENTS - 2] > 0.0,
            "pushed along the travel: {:?}",
            r.angle
        );
    }
}
