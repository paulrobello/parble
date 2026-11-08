//! Stepper elevator behaviour (PRD P2 item 17, `ref-09`): the moving slats of a
//! `mech_stepper_*` piece, one kinematic slide part (`anim_slats`, µm along +Y) driven through
//! up-down cycles while a marble is on the lift.
//!
//! Geometry (`blender/kit/kitstepper.py`): every slat top falls toward +x, the fixed and moving
//! slats alternate, and one stroke (the part's upper limit) takes a marble from the slat or stub
//! before a moving slat to the fixed slat or stub after it. At rest (0 µm) the first moving slat
//! sits just under the entry stub, so the lift is always ready for the next marble.
//!
//! Each tick, from the `lift` sensor:
//! - a marble on the lift with the slats idle starts a cycle: back to rest if they were part-way
//!   down, [`ENTRY_HOLD_TICKS`] at rest while the marble settles on the first slat, up, a hold at
//!   the top, down. While a marble stays on the lift, cycles repeat with [`BOTTOM_HOLD_TICKS`] at
//!   rest, so a marble climbs one moving slat per cycle and leaves by the exit stub;
//! - once the lift has been empty for [`EMPTY_TICKS`] (the marble left at the top, or was moved
//!   back off the lift toward the start), the slats ease back to rest from wherever they are and
//!   stay there.
//!
//! Every move is a smoothstep sampled at [`EASE_STEPS`] integer keyframes, so the slats start and
//! stop without a jolt and the motion stays a pure function of the tick.

use parble_core::MechConfig;

use crate::mechanism::{Keyframes, MechContext, Mechanism};

/// The shipped stepper: in at L3, out at L3 one cell layer up, two cells along.
pub const STEPPER: &str = "mech_stepper_up1_l3";

/// Rest to full stroke, and back, ticks (0.25 s at 240 Hz).
pub const UP_TICKS: u32 = 60;
pub const DOWN_TICKS: u32 = 60;
/// At the top: the marble rolls off the raised slat onto the next fixed one.
pub const TOP_HOLD_TICKS: u32 = 48;
/// At rest between cycles: the marble rolls off a fixed slat onto the lowered moving one.
pub const BOTTOM_HOLD_TICKS: u32 = 48;
/// At rest before the first cycle: the arriving marble settles on the first slat.
pub const ENTRY_HOLD_TICKS: u32 = 96;
/// The lift counts as empty after this many ticks with no marble in `lift`.
pub const EMPTY_TICKS: i64 = 24;
/// Keyframes per eased move.
pub const EASE_STEPS: u32 = 8;

fn get(state: &MechConfig, key: &str) -> i64 {
    state.get(key).copied().unwrap_or(0)
}

/// `keys` extended by a smoothstep from its last value to `to` over `ticks`.
fn ease(keys: &mut Vec<(u32, i32)>, to: i32, ticks: u32) {
    let &(t0, from) = keys.last().expect("keys start with the current position");
    if ticks == 0 || from == to {
        return;
    }
    let n = EASE_STEPS.min(ticks);
    let (d, n64) = (i64::from(to) - i64::from(from), i64::from(n));
    for k in 1..=n {
        let k64 = i64::from(k);
        // smoothstep(k / n) = (3 k² n − 2 k³) / n³, in integers
        let v =
            i64::from(from) + d * (3 * k64 * k64 * n64 - 2 * k64 * k64 * k64) / (n64 * n64 * n64);
        keys.push((t0 + ticks * k / n, v as i32));
    }
}

/// `keys` extended by `ticks` at its last value.
fn hold(keys: &mut Vec<(u32, i32)>, ticks: u32) {
    let &(t0, v) = keys.last().expect("keys start with the current position");
    if ticks > 0 {
        keys.push((t0 + ticks, v));
    }
}

/// Ticks to cover `d` µm of a `stroke` µm move that takes `full` ticks.
fn share(full: u32, d: i32, stroke: i32) -> u32 {
    if stroke <= 0 {
        return 0;
    }
    (i64::from(full) * i64::from(d.abs()) / i64::from(stroke)) as u32
}

/// One cycle from position `from`: down to rest, `rest_hold` there, up to `stroke`, hold, down.
pub fn cycle(from: i32, stroke: i32, rest_hold: u32) -> Keyframes {
    let mut keys = vec![(0, from)];
    ease(&mut keys, 0, share(DOWN_TICKS, from, stroke));
    hold(&mut keys, rest_hold);
    ease(&mut keys, stroke, UP_TICKS);
    hold(&mut keys, TOP_HOLD_TICKS);
    ease(&mut keys, 0, DOWN_TICKS);
    Keyframes::new(keys).expect("cycle keys are strictly increasing")
}

/// From position `from` back to rest, at the down stroke's pace.
pub fn reset(from: i32, stroke: i32) -> Keyframes {
    let mut keys = vec![(0, from)];
    ease(&mut keys, 0, share(DOWN_TICKS, from, stroke).max(1));
    Keyframes::new(keys).expect("reset keys are strictly increasing")
}

/// Stepper elevator: see the module docs. Signals: `start` (a marble arrived on the idle lift),
/// `cycle` (count), `reset` (the slats' position when the lift emptied).
#[derive(Debug, Clone, Default)]
pub struct Stepper {
    running: bool,
    /// Tick the current cycle ends.
    cycle_end: i64,
    /// First tick of the current empty spell, -1 while a marble is on the lift.
    empty_since: i64,
    cycles: i64,
}

impl Stepper {
    pub fn factory(_: &MechConfig) -> Box<dyn Mechanism> {
        Box::new(Self {
            empty_since: -1,
            ..Self::default()
        })
    }
}

impl Mechanism for Stepper {
    fn kind(&self) -> &'static str {
        "stepper"
    }

    fn step(&mut self, ctx: &mut MechContext<'_>) {
        let t = ctx.tick() as i64;
        let occupied = ctx.sensor(0).is_some_and(|s| !s.occupants.is_empty());
        let stroke = ctx.part_limits(0).map_or(0, |l| l[1]);
        let at = ctx.part_angle(0).unwrap_or(0);
        if occupied {
            self.empty_since = -1;
        } else if self.empty_since < 0 {
            self.empty_since = t;
        }
        if occupied && (!self.running || t >= self.cycle_end) {
            let rest_hold = if self.running {
                BOTTOM_HOLD_TICKS
            } else {
                ctx.emit("start", 1);
                ENTRY_HOLD_TICKS
            };
            let motion = cycle(at, stroke, rest_hold);
            self.cycle_end = t + i64::from(motion.duration());
            ctx.play(0, motion);
            self.running = true;
            self.cycles += 1;
            ctx.emit("cycle", self.cycles);
        } else if self.running && !occupied && t - self.empty_since >= EMPTY_TICKS {
            self.running = false;
            ctx.play(0, reset(at, stroke));
            ctx.emit("reset", i64::from(at));
        }
    }

    fn save(&self) -> MechConfig {
        MechConfig::from([
            ("running".to_owned(), i64::from(self.running)),
            ("cycle_end".to_owned(), self.cycle_end),
            ("empty_since".to_owned(), self.empty_since),
            ("cycles".to_owned(), self.cycles),
        ])
    }

    fn restore(&mut self, state: &MechConfig) {
        self.running = get(state, "running") != 0;
        self.cycle_end = get(state, "cycle_end");
        self.empty_since = state.get("empty_since").copied().unwrap_or(-1);
        self.cycles = get(state, "cycles");
    }
}

#[cfg(test)]
#[path = "stepper_test.rs"]
mod tests;
