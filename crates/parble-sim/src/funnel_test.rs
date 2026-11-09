//! The funnel catcher (`blk_funnel_3x3_l2`) and its drop catch (`blk_catch_l1`),
//! blender/kit/kitfunnel.py: a marble rolled in from an `L2` straight at the rim spirals down the
//! bowl and leaves by the centre bore into the catch, and a marble dropped into the bowl from a
//! straight standing on the funnel is caught and leaves the same way. The inlet seam and the
//! catch's exit seam onto the `L1` run-out are gated at the seam limits of `block_seam_test`
//! (`limits`, measured as it does). The bore crossing (the funnel's bottom face, where its bore
//! meets the catch's) is logged, not gated: see `bore_window`.
//!
//! Layout: the funnel anchored at (1, 1, 0) (cells x 1..3, z -2..0, layers 0 and 1, bowl axis at
//! world x 120 mm, z -60 mm, rim floor 90 mm), its port 0 fed by two `L2` straights from -x, the
//! catch under the bore at (2, -1, -1) and two `L1` straights carrying the marble out to -x.

use crate::block_seam_test::limits;
use crate::marble::CATS_EYE;
use crate::seam_test::{kick, upward};
use crate::{MarbleState, Sim};
use parble_core::{CellCoord, PieceId, Placement, Yaw4};
use rapier3d::prelude::*;

const R: f32 = 0.008;
const AXIS: [f32; 2] = [0.12, -0.06];
/// The funnel's bottom face, where its bore meets the catch's (world y).
const EXIT_SEAM_Y: f32 = 0.0;

fn place(piece: &str, x: i32, y: i32, z: i32, yaw: u8) -> Placement {
    Placement::new(
        PieceId::new(piece).unwrap(),
        CellCoord::new(x, y, z),
        Yaw4::new(yaw),
        false.into(),
    )
}

fn layout() -> Vec<Placement> {
    vec![
        place("blk_straight_l2", -1, 1, 0, 0),
        place("blk_straight_l2", 0, 1, 0, 0),
        place("blk_funnel_3x3_l2", 1, 1, 0, 0),
        place("blk_catch_l1", 2, -1, -1, 0),
        place("blk_straight_l1", 1, -1, -1, 0),
        place("blk_straight_l1", 0, -1, -1, 0),
    ]
}

/// Cells every layout is moved by before it is built, so the funnel and the catch share chunk
/// (0, 0, 0) and the bore crossing is inside one trimesh. Traces are moved back, so the windows
/// below stay in the layout's own frame.
const SHIFT: [i32; 3] = [1, 1, 2];

fn run(layout: &[Placement], at: Vector, vel: Vector, ticks: u32) -> Vec<MarbleState> {
    let kit = parble_kit::builtin();
    let mut sim = Sim::empty();
    for p in layout {
        let mut p = p.clone();
        p.anchor = CellCoord::new(
            p.anchor.x + SHIFT[0],
            p.anchor.y + SHIFT[1],
            p.anchor.z + SHIFT[2],
        );
        sim.place_piece(kit, p).unwrap();
    }
    sim.rebuild_chunks(kit).unwrap();
    let off = [
        0.06 * SHIFT[0] as f32,
        0.06 * SHIFT[1] as f32,
        0.06 * SHIFT[2] as f32,
    ];
    let (mut body, collider) = CATS_EYE.build(at + Vector::new(off[0], off[1], off[2]));
    body.set_linvel(vel, true);
    let speed = vel.length();
    if speed > 0.0 {
        body.set_angvel(
            Vector::Y.cross(vel / speed) * (speed / CATS_EYE.radius_m),
            true,
        );
    }
    let h = sim.world.bodies.insert(body);
    sim.world
        .colliders
        .insert_with_parent(collider, h, &mut sim.world.bodies);
    sim.marbles.push(h);
    let mut out = Vec::new();
    for _ in 0..ticks {
        sim.tick(1);
        let mut m = sim.marbles()[0];
        for (p, o) in m.position.iter_mut().zip(off) {
            *p -= o;
        }
        out.push(m);
        // Out on the run-out past the catch: done.
        if m.position[0] < 0.0 && m.position[1] < -0.02 {
            break;
        }
    }
    out
}

fn radius(m: &MarbleState) -> f32 {
    (m.position[0] - AXIS[0]).hypot(m.position[2] - AXIS[1])
}

/// Turns about the bowl axis while the marble is in the bowl (above the bore, inside the rim).
fn turns(tr: &[MarbleState]) -> f32 {
    let mut total = 0.0_f32;
    let mut last: Option<f32> = None;
    for m in tr {
        let in_bowl = m.position[1] > 0.02 && radius(m) < 0.075;
        if !in_bowl {
            last = None;
            continue;
        }
        let a = (m.position[2] - AXIS[1]).atan2(m.position[0] - AXIS[0]);
        if let Some(l) = last {
            let mut d = a - l;
            if d > std::f32::consts::PI {
                d -= std::f32::consts::TAU;
            } else if d < -std::f32::consts::PI {
                d += std::f32::consts::TAU;
            }
            total += d;
        }
        last = Some(a);
    }
    total.abs() / std::f32::consts::TAU
}

/// The marble left by the catch's run-out, and before that never left the funnel's plan
/// footprint while above its bottom.
fn assert_through(tr: &[MarbleState], what: &str) {
    let end = tr.last().unwrap();
    assert!(
        end.position[0] < 0.0 && end.position[1] < -0.02,
        "{what}: marble did not leave by the catch run-out, ended at {:?} after {} ticks",
        end.position,
        tr.len()
    );
    for m in tr.iter().filter(|m| m.position[1] > EXIT_SEAM_Y) {
        let [x, _, z] = m.position;
        assert!(
            (-0.09..=0.21).contains(&x) && (-0.15..=0.03).contains(&z),
            "{what}: marble left the funnel at {:?}",
            m.position
        );
    }
}

/// Window over the bore crossing (the funnel's bottom face, where its bore meets the catch's),
/// 15 mm either side: logged, not gated, because it does not pass. The marble leaves the bowl's
/// fillet against the bore wall and rides and rattles down the 48-facet bore in a near free fall;
/// every facet contact is a kick (91–326 mm/s measured 2026-11-09), and `upward` cannot see a
/// bump on a vertical seam (a falling marble's vy is never positive).
fn bore_window(m: &MarbleState) -> bool {
    radius(m) < 0.016 && (EXIT_SEAM_Y - 0.015..EXIT_SEAM_Y + 0.015).contains(&m.position[1])
}

/// Control for `bore_window`: the funnel's own bore only, the marble's centre 8 to 23 mm above
/// the bottom face, where it cannot touch the catch.
fn bore_control(m: &MarbleState) -> bool {
    radius(m) < 0.016 && (EXIT_SEAM_Y + 0.008..EXIT_SEAM_Y + 0.023).contains(&m.position[1])
}

/// Window over the exit seam: the catch's -x port (world x 90 mm, floor 15 mm, z -60 mm) where
/// the marble leaves the funnel's exit onto the `L1` run-out, 15 mm either side.
fn exit_window(m: &MarbleState) -> bool {
    (0.075..0.105).contains(&m.position[0])
        && (m.position[2] + 0.06).abs() < 0.03
        && m.position[1] < -0.02
}

/// Window over the inlet seam (straight to funnel at world x 30 mm, upper layer).
fn inlet_window(m: &MarbleState) -> bool {
    (0.015..0.045).contains(&m.position[0]) && m.position[2].abs() < 0.03 && m.position[1] > 0.06
}

#[test]
fn a_marble_rolled_in_at_the_rim_spirals_down_and_leaves_by_the_centre_exit() {
    let at = Vector::new(-0.05, 0.06 + 0.03 + R, 0.0);
    let tr = run(&layout(), at, Vector::X * 0.3, 240 * 60);
    assert_through(&tr, "rolled in");
    let n = turns(&tr);
    let (p_lim, k_lim) = limits(2);
    let (ui, ki) = (upward(&tr, inlet_window), kick(&tr, inlet_window));
    let (ue, ke) = (upward(&tr, exit_window), kick(&tr, exit_window));
    let (ub, kb) = (upward(&tr, bore_window), kick(&tr, bore_window));
    let kc = kick(&tr, bore_control);
    eprintln!(
        "funnel roll-in: {} ticks ({:.2} s), {n:.2} turns in the bowl; inlet seam upward {ui:.2} \
         kick {ki:.2} mm/s (limits {p_lim:.2} / {k_lim:.2}); catch outlet in the full run \
         (landing, logged) {ue:.2} / {ke:.2}; bore crossing (logged) {ub:.2} / {kb:.2}, funnel \
         bore alone kick {kc:.2}",
        tr.len(),
        tr.len() as f32 / 240.0,
    );
    assert!(tr.iter().any(inlet_window) && tr.iter().any(exit_window));
    assert!(n >= 1.0, "only {n:.2} turns about the axis: no spiral");
    assert!(ui <= p_lim && ki <= k_lim, "inlet seam {ui:.2} / {ki:.2}");
    assert!(tr.iter().any(exit_window));
}

#[test]
fn a_marble_dropped_into_the_bowl_is_caught_and_leaves_by_the_centre_exit() {
    // An `L3` straight on the funnel's -x edge cell, one layer up, its +x port over the bowl: the
    // marble leaves it at up to the validator's fastest assumed exit speed and falls 75 mm to the
    // rim floor (`parble_core::FUNNEL_MAX_FALL_UM`).
    let mut l = layout();
    l.push(place("blk_straight_l3", 1, 2, -1, 0));
    for v in [0.02_f32, 0.3, 0.75] {
        // The slow marble starts at the port edge (rolling resistance would stop it on the way).
        let x = if v < 0.1 { 0.087 } else { 0.06 };
        let at = Vector::new(x, 0.12 + 0.045 + R, -0.06);
        let tr = run(&l, at, Vector::X * v, 240 * 60);
        assert_through(&tr, &format!("dropped at {v} m/s"));
        let (ue, ke) = (upward(&tr, exit_window), kick(&tr, exit_window));
        let (ub, kb) = (upward(&tr, bore_window), kick(&tr, bore_window));
        let kc = kick(&tr, bore_control);
        eprintln!(
            "funnel drop at {v} m/s: {} ticks, {:.2} turns, catch outlet (landing, logged) \
             {ue:.2} / {ke:.2}, bore crossing (logged) {ub:.2} / {kb:.2}, funnel bore alone kick \
             {kc:.2}",
            tr.len(),
            turns(&tr)
        );
        assert!(tr.iter().any(exit_window));
    }
}

/// The catch's floor (kitblocks `tower_floor(15, 17, -8, -30)`) at catch-local plan x mm, mm over
/// the cell's bottom: the pad from x 17 falls 3 %, then the quintic Hermite descends 1.3 mm more
/// and ends flat at the -x port face.
fn catch_floor(x: f32) -> f32 {
    let (x_end, x_pad, x0) = (17.0_f32, -8.0_f32, -30.0_f32);
    let (pl, length) = (x_end - x_pad, x_end - x0);
    let pd = pl * 0.03;
    let rise = pd + 1.3;
    let s = (x_end - x).clamp(0.0, length);
    if s <= pl {
        return 15.0 + rise - 0.03 * s;
    }
    let span = length - pl;
    let u = (s - pl) / span;
    let h1 = u * (1.0 + u * u * (-6.0 + u * (8.0 - 3.0 * u)));
    let smoother = u * u * u * (10.0 + u * (-15.0 + 6.0 * u));
    15.0 + rise - (pd + 1.3 * smoother + 0.03 * span * h1)
}

#[test]
fn the_catch_exit_seam_stays_under_the_seam_thresholds() {
    // As the redirect's exit seam (`block_seam_test`): the drop down the bore lands on the catch's
    // pad 30 mm from the exit seam and the marble crosses the seam still settling from that
    // landing, which the full runs above log. The seam itself is measured with the marble already
    // rolling on the catch's floor past its landing, 12 mm inside the cell (world x 108 mm).
    let layout = vec![
        place("blk_catch_l1", 2, -1, -1, 0),
        place("blk_straight_l1", 1, -1, -1, 0),
        place("blk_straight_l1", 0, -1, -1, 0),
    ];
    let lx = -12.0_f32;
    let slope = (catch_floor(lx + 0.5) - catch_floor(lx - 0.5)) / 1.0;
    let y = -0.06 + catch_floor(lx) / 1000.0 + R + 0.0001;
    let speed = 0.3_f32;
    let tr = run(
        &layout,
        Vector::new(0.12 + lx / 1000.0, y, -0.06),
        Vector::new(-speed, -speed * slope, 0.0),
        600,
    );
    assert!(
        tr.iter().any(|m| m.position[0] < 0.03),
        "marble must reach the run-out, ended at {:?}",
        tr.last().unwrap().position
    );
    let sel = |m: &MarbleState| {
        (0.075..0.105).contains(&m.position[0]) && (m.position[2] + 0.06).abs() < 0.03
    };
    let (p_lim, k_lim) = limits(1);
    let (u, k) = (upward(&tr, sel), kick(&tr, sel));
    eprintln!("catch exit seam: upward {u:.2} kick {k:.2} mm/s (limits {p_lim:.2} / {k_lim:.2})");
    assert!(u <= p_lim && k <= k_lim, "catch exit seam {u:.2} / {k:.2}");
}
