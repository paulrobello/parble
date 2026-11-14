//! Estimated marble speed along a generated track.
//!
//! The generator cannot run the physics, so it carries an integer estimate of the squared speed
//! (in 1e-3 m²/s²) of a marble leaving each open end, and refuses steps the sim measured to fail
//! at that speed. The numbers come from the first marble of 100 generated tracks run in the sim
//! (`crates/parble-sim/tests/procgen_pass_rate.rs`, `transfer_census`).

use crate::template::Shape;

/// Squared speed a rolling marble gains per port level of descent (15 mm).
pub(crate) const GAIN_PER_LEVEL: i64 = 210;

/// Squared speed a marble leaves the hopper or a catch with (measured: 0.25 m/s).
pub(crate) const HOPPER_V2: i64 = 60;

/// Highest squared speed at which a marble leaving an `L1` exit still lands on the `L3` inlet
/// 30 mm below. Every transfer predicted at 0.3 m/s or less crossed, about half above 0.5 m/s.
pub(crate) const TRANSFER_MAX_V2: i64 = 110;

/// Lowest squared speed a marble may leave a piece with. Marbles creeping below about 0.17 m/s
/// come to rest on a block curve or straight.
pub(crate) const MIN_FLOW_V2: i64 = 30;

/// Squared speed window a marble may enter a drop piece in (300 seeds, first marble): around
/// 0.4 m/s 6 of 29 stall at the hole's lip, at 0.5 to 0.6 m/s about 3%.
const DROP_MIN_V2: i64 = 200;
const DROP_MAX_V2: i64 = i64::MAX;

/// Squared speed a marble leaves a redirect tunnel with, whatever it entered at (measured:
/// 0.3 to 0.4 m/s from entries at 0.4 to 0.5 m/s, though the tunnel drops it a whole layer).
const REDIRECT_V2: i64 = 100;

/// Highest squared speed a marble may enter a turn with. On block curves above 0.8 m/s about
/// half jump the wall, 0.4 to 0.6 m/s about 7%, below 0.4 m/s about 1%.
const TURN_MAX_V2: i64 = 250;

/// Share of the squared speed a piece keeps, per mille. Block curves settle a marble near
/// 0.2 m/s whatever it came in at, straights and slopes keep it.
fn retain_per_mille(shape: Shape) -> i64 {
    match shape {
        Shape::Straight => 950,
        Shape::Turn => 350,
        Shape::RailTurn => 950,
        Shape::Redirect => 500,
        Shape::Catch => 0,
        Shape::Slope | Shape::Steep => 900,
        Shape::Special | Shape::Other => 950,
        Shape::Drop => 700,
    }
}

/// Whether a marble at squared speed `v2` can enter a piece of `shape`.
pub(crate) fn enters(shape: Shape, v2: i64) -> bool {
    match shape {
        Shape::Drop => (DROP_MIN_V2..=DROP_MAX_V2).contains(&v2),
        Shape::Turn | Shape::RailTurn => v2 <= TURN_MAX_V2,
        _ => true,
    }
}

/// Squared speed leaving a piece of `shape` that descends `d` port levels.
pub(crate) fn exit_v2(v2_in: i64, shape: Shape, d: i32) -> i64 {
    match shape {
        Shape::Catch => return HOPPER_V2,
        Shape::Redirect => return REDIRECT_V2,
        _ => {}
    }
    v2_in * retain_per_mille(shape) / 1000 + GAIN_PER_LEVEL * i64::from(d.max(0))
}
