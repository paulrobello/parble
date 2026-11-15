//! Estimated marble speed along a generated track.
//!
//! The generator cannot run the physics, so it carries an integer estimate of the squared speed
//! (in 1e-3 m²/s²) of a marble leaving each open end, and refuses steps the sim measured to fail
//! at that speed. The numbers come from the first marble of 300 generated tracks run in the sim
//! (`crates/parble-sim/tests/procgen_pass_rate.rs`, 2026-11-15: `speed_census` for the share of
//! its squared horizontal speed a marble keeps through each piece and the marbles each piece
//! loses by entry speed, `transfer_census` for the transfers).

use crate::template::Shape;

/// Squared speed a rolling marble gains per port level of descent (15 mm).
pub(crate) const GAIN_PER_LEVEL: i64 = 210;

/// Squared speed a marble leaves the hopper with (measured: 0.24 m/s).
pub(crate) const HOPPER_V2: i64 = 60;

/// Squared speed a marble leaves a catch with, whatever it fell in at (measured: 0.17 m/s).
const CATCH_V2: i64 = 30;

/// Highest squared speed at which a marble leaving an `L1` exit still lands on the `L3` inlet
/// 30 mm below. Every transfer predicted at 0.3 m/s or less crossed, about half above 0.5 m/s.
pub(crate) const TRANSFER_MAX_V2: i64 = 110;

/// Lowest squared speed a marble may leave a piece with. Marbles creeping below about 0.17 m/s
/// come to rest on a block curve or straight.
const MIN_FLOW_V2: i64 = 30;

/// Lowest squared speed a marble may enter a drop piece with (300 seeds, first marble): around
/// 0.4 m/s 6 of 29 stall at the hole's lip, at 0.5 to 0.6 m/s about 3%.
const DROP_MIN_V2: i64 = 200;

/// Highest squared speed a marble may enter an `L1` drop with: above 0.8 m/s 6 of 207 came to
/// rest on it, at 0.6 to 0.8 m/s 1 of 91, below that none of 37. The `L2` drop lost none of 163
/// at any speed.
const DROP_LOW_MAX_V2: i64 = 640;

/// Squared speed a marble leaves a redirect tunnel with, whatever it entered at. The tunnel
/// drops it a whole layer and the exit speed is erratic (0.17 to 0.73 m/s, median 0.59 from
/// `L3`), so the estimate is the fast end: nothing that needs a slow marble follows a redirect.
const REDIRECT_V2: i64 = 350;

/// Highest squared speed a marble may enter a redirect with: the three `blk_redirect_l3` that
/// lost the first marble of 300 tracks took it at 0.6 to 1.1 m/s.
const REDIRECT_MAX_V2: i64 = 300;

/// Highest squared speed a marble may enter a block curve with. Above 0.8 m/s about half jump
/// the wall, 0.4 to 0.6 m/s about 7%, below 0.5 m/s none of 357.
const TURN_MAX_V2: i64 = 250;

/// Highest squared speed a marble may enter a rail curve or S with: at 0.4 to 0.5 m/s 2 of 15
/// were lost, below 0.4 m/s none of 250.
const RAIL_TURN_MAX_V2: i64 = 160;

/// Share of the squared speed a piece keeps, per mille (medians of `speed_census`): straights
/// keep it all, block curves 0.87 at `L1`, 0.88 at `L2` and 0.90 at `L3`, rail curves 0.94 to
/// 0.98, the gate 0.94 and the checkpoint 1.00, and the rope arch a quarter (0.24 to 0.25, the
/// middle four fifths of 506 passes within 0.20 to 0.27).
fn retain_per_mille(shape: Shape) -> i64 {
    match shape {
        Shape::Straight => 1000,
        Shape::Turn => 870,
        Shape::TurnTop => 900,
        Shape::RailTurn => 950,
        Shape::Redirect | Shape::Catch => 0,
        Shape::Slope | Shape::Steep => 900,
        Shape::Special | Shape::Other => 950,
        Shape::Arch => 250,
        Shape::Drop | Shape::DropLow => 700,
    }
}

/// Whether a marble at squared speed `v2` can enter a piece of `shape`.
pub(crate) fn enters(shape: Shape, v2: i64) -> bool {
    match shape {
        Shape::Drop => v2 >= DROP_MIN_V2,
        Shape::DropLow => (DROP_MIN_V2..=DROP_LOW_MAX_V2).contains(&v2),
        Shape::Turn | Shape::TurnTop => v2 <= TURN_MAX_V2,
        Shape::RailTurn => v2 <= RAIL_TURN_MAX_V2,
        Shape::Redirect => v2 <= REDIRECT_MAX_V2,
        _ => true,
    }
}

/// Whether a marble leaving a piece at squared speed `v2` keeps rolling.
pub(crate) fn flows(v2: i64) -> bool {
    v2 >= MIN_FLOW_V2
}

/// Squared speed leaving a piece of `shape` that descends `d` port levels.
pub(crate) fn exit_v2(v2_in: i64, shape: Shape, d: i32) -> i64 {
    match shape {
        Shape::Catch => CATCH_V2,
        Shape::Redirect => REDIRECT_V2,
        _ => v2_in * retain_per_mille(shape) / 1000 + GAIN_PER_LEVEL * i64::from(d.max(0)),
    }
}
