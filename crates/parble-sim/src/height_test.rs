//! The 2-tall block slopes of contract §7 (`blk_slope_gentle_l1`, `_l0`, `blk_slope_steep_l2`,
//! `_l1`, `_l0`): a marble rolls down chains that carry it through `L0` and into the cell layer
//! below, leaves on the last straight centred and down in its channel, and no seam on the way
//! throws it upward.
//!
//! Speeds stay where the existing slopes are good: the block slopes at `L2` and `L3` throw a
//! marble up 330 to 530 mm/s off their exit above about 0.7 m/s (measured 2026-11-14), and a
//! slope's exit speed is its entry speed with 0.21 m²/s² more per level, so a run of four levels
//! from 0.3 m/s already leaves at about 0.9 m/s. The generator's speed estimate keeps its tracks
//! under that.

use crate::MarbleState;
use crate::rail_test::{launch, place, trace, track_sim};
use crate::seam_test::{kick, upward};
use parble_core::{CellCoord, PieceId, Placement, Yaw4};
use rapier3d::prelude::*;

const SPEED: f32 = 0.3;
/// The layer the runs start in. A run that crosses the layer 0 to -1 boundary also crosses a chunk
/// boundary, where a slope's exact contact does not reach the straights in the next chunk and the
/// exit bumps as it does every eight cells along x; the layers here are all in chunk y = 0.
const BASE: i32 = 4;
const TICKS: u32 = 900;
/// Straights after the last slope, so the marble settles before the run ends.
const RUNOUT: i32 = 5;
/// `seam_test::SEAM_SPIKE_MM_S`: allowed upward speed above the same run at another level.
const SEAM_SPIKE_MM_S: f32 = 2.0;
/// Allowed upward speed anywhere on the run, mm/s. The seamless channel has none, and the slope
/// seams of the existing chains stay under 2 mm/s (`rail_test`).
const UPWARD_MM_S: f32 = 5.0;
/// Allowed kick, mm/s: the block steep slope kicks 24.7 at 0.3 m/s and 35.3 at 0.5, so an entry
/// into or exit from a chain of them gets that plus the margin of `rail_test`.
const KICK_MM_S: f32 = 50.0;

/// Centre height of a standard marble resting on `level` of cell layer `layer`.
fn rest(layer: i32, level: i32) -> f32 {
    (BASE + layer) as f32 * 0.060 + level as f32 * 0.015 + 0.008
}

fn at(piece: &str, x: i32, layer: i32) -> Placement {
    place(piece, x, BASE + layer, 0)
}

/// A run along +x: two straights at `level`, the slopes (each with the layer it stands in), then
/// straights at the exit's `layer` and `level`.
fn run(level: i32, slopes: &[(&str, i32)], exit: (i32, i32)) -> Vec<Placement> {
    let mut l = vec![
        at(&format!("blk_straight_l{level}"), 0, 0),
        at(&format!("blk_straight_l{level}"), 1, 0),
    ];
    let mut x = 2;
    for (s, layer) in slopes {
        l.push(at(s, x, *layer));
        x += 1;
    }
    for k in 0..RUNOUT {
        l.push(at(&format!("blk_straight_l{}", exit.1), x + k, exit.0));
    }
    l
}

#[test]
fn two_tall_slopes_chain_down_through_l0_into_the_layer_below() {
    // (name, start level, run, exit layer and level)
    let chains = [
        (
            "gentle L1 to L0, then gentle L0 into the layer below",
            1,
            run(
                1,
                &[("blk_slope_gentle_l1", 0), ("blk_slope_gentle_l0", 0)],
                (-1, 3),
            ),
            (-1, 3),
        ),
        (
            "gentle L1 to L0, then steep L0 into the layer below",
            1,
            run(
                1,
                &[("blk_slope_gentle_l1", 0), ("blk_slope_steep_l0", 0)],
                (-1, 2),
            ),
            (-1, 2),
        ),
    ];
    for (name, start, layout, exit) in chains {
        let mut sim = track_sim(&layout);
        launch(
            &mut sim,
            Vector::new(-0.02, rest(0, start), 0.0),
            Vector::X,
            SPEED,
        );
        let tr = trace(&mut sim, TICKS);
        let last = 0.06 * (layout.len() as f32 - 1.0);
        let on = |m: &MarbleState| (0.09..last).contains(&m.position[0]);
        let (up, k) = (upward(&tr, on), kick(&tr, on));
        eprintln!("{name}: upward {up:.2} mm/s, kick {k:.2} mm/s");
        assert!(
            tr.iter().any(|m| m.position[0] > last - 0.09
                && m.position[0] < last
                && (m.position[1] - rest(exit.0, exit.1)).abs() < 0.003
                && m.position[2].abs() < 0.005),
            "{name}: marble must reach the last straight, centred and down in its channel"
        );
        assert!(up <= UPWARD_MM_S, "{name}: upward {up:.2} mm/s");
        assert!(k <= KICK_MM_S, "{name}: kick {k:.2} mm/s over {KICK_MM_S}");
    }
}

#[test]
fn each_two_tall_slope_takes_a_marble_in_and_lets_it_out_without_a_bump() {
    // (slope, entry level, exit layer, exit level); an exit at `L0` has no straight to mate, so
    // the window ends at the slope's far face.
    for (slope, entry, exit_layer, exit_level) in [
        ("blk_slope_gentle_l1", 1, 0, 0),
        ("blk_slope_gentle_l0", 0, -1, 3),
        ("blk_slope_steep_l2", 2, 0, 0),
        ("blk_slope_steep_l1", 1, -1, 3),
        ("blk_slope_steep_l0", 0, -1, 2),
    ] {
        for v in [0.3_f32, 0.5] {
            let mut layout = vec![];
            if entry > 0 {
                layout.push(at(&format!("blk_straight_l{entry}"), 0, 0));
            }
            layout.push(at(slope, 1, 0));
            if exit_level > 0 {
                for k in 2..8 {
                    layout.push(at(&format!("blk_straight_l{exit_level}"), k, exit_layer));
                }
            }
            let mut sim = track_sim(&layout);
            launch(
                &mut sim,
                Vector::new(if entry > 0 { 0.0 } else { 0.035 }, rest(0, entry), 0.0),
                Vector::X,
                v,
            );
            let tr = trace(&mut sim, 500);
            let end = if exit_level > 0 { 0.45 } else { 0.09 };
            let w = |m: &MarbleState| (0.03..end).contains(&m.position[0]);
            let (up, k) = (upward(&tr, w), kick(&tr, w));
            eprintln!("{slope} at {v} m/s: upward {up:.2} mm/s, kick {k:.2} mm/s");
            assert!(up <= UPWARD_MM_S, "{slope} at {v} m/s: upward {up:.2} mm/s");
            assert!(k <= KICK_MM_S, "{slope} at {v} m/s: kick {k:.2} mm/s");
            if exit_level > 0 {
                assert!(
                    tr.iter().any(|m| m.position[0] > 0.3
                        && m.position[0] < 0.45
                        && (m.position[1] - rest(exit_layer, exit_level)).abs() < 0.003
                        && m.position[2].abs() < 0.005),
                    "{slope} at {v} m/s: marble must leave on the exit straights"
                );
            }
        }
    }
}

/// A placement at an exact layer (not offset by [`BASE`]) with a quarter-turn count.
fn pl(piece: &str, x: i32, layer: i32, z: i32, yaw: u8) -> Placement {
    Placement::new(
        PieceId::new(piece).unwrap(),
        CellCoord::new(x, BASE + layer, z),
        Yaw4::new(yaw),
        false.into(),
    )
}

/// Rolls a marble at `speed` from `from` along `dir` for `ticks` and returns its trace.
fn roll(
    layout: &[Placement],
    from: [f32; 3],
    dir: Vector,
    speed: f32,
    ticks: u32,
) -> Vec<MarbleState> {
    let mut sim = track_sim(layout);
    launch(&mut sim, Vector::from(from), dir, speed);
    trace(&mut sim, ticks)
}

#[test]
fn the_l1_curve_turns_a_marble_and_the_l1_joiner_merges_two_inlets() {
    // Curve: two straights east, `blk_curve90_l1` at x 2 turning toward -z, three straights south.
    let mut curve = vec![
        pl("blk_straight_l1", 0, 0, 0, 0),
        pl("blk_straight_l1", 1, 0, 0, 0),
        pl("blk_curve90_l1", 2, 0, 0, 0),
    ];
    curve.extend((1..=3).map(|k| pl("blk_straight_l1", 2, 0, -k, 1)));
    let tr = roll(&curve, [-0.02, rest(0, 1), 0.0], Vector::X, SPEED, 700);
    let end = |m: &MarbleState| m.position[2] < -0.15;
    assert!(
        tr.iter().any(|m| end(m)
            && (m.position[0] - 0.12).abs() < 0.005
            && (m.position[1] - rest(0, 1)).abs() < 0.003),
        "blk_curve90_l1: marble must leave south centred and down in its channel"
    );
    let up = upward(&tr, |m| m.position[0] > 0.0);
    // The turn itself puts 27.5 mm/s up at every level (the marble enters the bend off a launch
    // 20 mm before the seam), so the L1 curve is held to the L2 one's.
    let mut c = vec![
        pl("blk_straight_l2", 0, 0, 0, 0),
        pl("blk_straight_l2", 1, 0, 0, 0),
        pl("blk_curve90_l2", 2, 0, 0, 0),
    ];
    c.extend((1..=3).map(|k| pl("blk_straight_l2", 2, 0, -k, 1)));
    let t = roll(&c, [-0.02, rest(0, 2), 0.0], Vector::X, SPEED, 700);
    let control = upward(&t, |m| m.position[0] > 0.0);
    assert!(
        up <= control + SEAM_SPIKE_MM_S,
        "blk_curve90_l1: upward {up:.2} mm/s over the L2 curve's {control:.2}"
    );

    // Joiner: inlets -x and -z, outlet +x, run from each inlet, held to the L2 joiner's run.
    let join_up = |level: i32, name: &str, from: [f32; 3], dir: Vector| {
        let st = format!("blk_straight_l{level}");
        let join = vec![
            pl(&st, 0, 0, 0, 0),
            pl(&format!("blk_join_l{level}"), 1, 0, 0, 0),
            pl(&st, 1, 0, -1, 1),
            pl(&st, 2, 0, 0, 0),
            pl(&st, 3, 0, 0, 0),
        ];
        let tr = roll(&join, from, dir, SPEED, 800);
        assert!(
            tr.iter().any(|m| m.position[0] > 0.2
                && m.position[2].abs() < 0.005
                && (m.position[1] - rest(0, level)).abs() < 0.003),
            "blk_join_l{level} from {name}: marble must leave east centred and down"
        );
        upward(&tr, |m| m.position[0] > 0.0)
    };
    for (name, dx, dz, dir) in [
        ("-x", -0.02, 0.0, Vector::X),
        ("-z", 0.06, -0.08, Vector::Z),
    ] {
        let up = join_up(1, name, [dx, rest(0, 1), dz], dir);
        let control = join_up(2, name, [dx, rest(0, 2), dz], dir);
        assert!(
            up <= control + SEAM_SPIKE_MM_S,
            "blk_join_l1 from {name}: upward {up:.2} mm/s over the L2 joiner's {control:.2}"
        );
    }
}

#[test]
fn the_l1_redirect_drops_a_marble_one_layer_and_the_l1_drop_feeds_the_catch() {
    // Redirect: in at L1 of layer 1, out at L1 of layer 0.
    let layout = vec![
        pl("blk_straight_l1", 0, 1, 0, 0),
        pl("blk_straight_l1", 1, 1, 0, 0),
        pl("blk_redirect_l1", 2, 1, 0, 0),
        pl("blk_straight_l1", 3, 0, 0, 0),
        pl("blk_straight_l1", 4, 0, 0, 0),
        pl("blk_straight_l1", 5, 0, 0, 0),
    ];
    let tr = roll(&layout, [-0.02, rest(1, 1), 0.0], Vector::X, SPEED, 1_100);
    assert!(
        tr.iter().any(|m| m.position[0] > 0.3
            && m.position[2].abs() < 0.005
            && (m.position[1] - rest(0, 1)).abs() < 0.003),
        "blk_redirect_l1: marble must leave on the lower straights, centred and down"
    );

    // Drop hole over the drop catch: the marble must fall through into the catch's layer.
    let drop = vec![
        pl("blk_straight_l1", 0, 1, 0, 0),
        pl("blk_straight_l1", 1, 1, 0, 0),
        pl("blk_straight_l1", 2, 1, 0, 0),
        pl("blk_drop_l1", 3, 1, 0, 0),
        pl("blk_catch_l1", 3, 0, 0, 0),
    ];
    let tr = roll(&drop, [-0.02, rest(1, 1), 0.0], Vector::X, 0.4, 900);
    assert!(
        tr.iter()
            .any(|m| m.position[1] < rest(0, 1) + 0.004 && (m.position[0] - 0.18).abs() < 0.03),
        "blk_drop_l1: marble must fall into the catch below"
    );
}

#[test]
fn the_end_stops_hold_a_marble_at_l1_and_l2() {
    for level in [1, 2] {
        let layout = vec![
            pl(&format!("blk_straight_l{level}"), 0, 0, 0, 0),
            pl(&format!("blk_straight_l{level}"), 1, 0, 0, 0),
            pl(&format!("blk_end_l{level}"), 2, 0, 0, 0),
        ];
        let tr = roll(&layout, [-0.02, rest(0, level), 0.0], Vector::X, 0.5, 900);
        assert!(
            tr.iter()
                .all(|m| m.position[0] < 0.15 && m.position[1] > rest(0, level) - 0.003),
            "blk_end_l{level}: marble must stay on the track"
        );
        assert!(
            tr.iter().any(|m| m.position[0] > 0.1),
            "blk_end_l{level}: marble must reach the stop"
        );
    }
}

#[test]
fn the_open_slot_tower_sockets_land_a_dropped_marble_at_l2_and_l3() {
    for level in [2, 3] {
        // A drop hole over the socket block: the marble falls down the shaft onto the landing.
        let layout = vec![
            pl(&format!("blk_straight_l{level}"), 0, 1, 0, 0),
            pl(&format!("blk_straight_l{level}"), 1, 1, 0, 0),
            pl(&format!("blk_drop_l{level}"), 2, 1, 0, 0),
            pl(&format!("blk_tower_socket_l{level}"), 2, 0, 0, 0),
        ];
        let tr = roll(&layout, [-0.02, rest(1, level), 0.0], Vector::X, 0.3, 900);
        let floor = rest(0, level);

        assert!(
            tr.iter()
                .any(|m| (m.position[1] - floor).abs() < 0.012
                    && (m.position[0] - 0.12).abs() < 0.03),
            "blk_tower_socket_l{level}: marble must land on the landing floor"
        );
        // Nothing mates the socket here, so the marble rolls out of its open end.
        assert!(
            tr.iter()
                .any(|m| m.position[0] < 0.095 && (m.position[1] - floor).abs() < 0.012),
            "blk_tower_socket_l{level}: marble must roll on to the socket"
        );
    }
}
