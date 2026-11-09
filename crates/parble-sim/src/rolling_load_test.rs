//! Rolling-resistance load on every contact type (2026-11-08).
//!
//! Coulomb rolling resistance ([`crate::marble::apply_rolling_resistance`]) decelerates a marble
//! rolling on the level at 5/7·c_rr·g, 14.0 mm/s² for a cat's eye. Each case below rolls a marble
//! slowly over one contact type twice, with and without rolling resistance, and takes the
//! deceleration rolling resistance adds as the difference of the two runs' speed slopes over the
//! ticks both spend in the case's window. Dividing by the normal load the control run's motion
//! needs (|a − g| / g, about 1 on the level and on these slow bends) gives the level-equivalent
//! deceleration, so the slope is compared on the same terms.

use crate::marble::{CATS_EYE, G};
use crate::{DT, MarbleState, Sim};
use parble_core::{CellCoord, PieceId, Placement, Yaw4};
use rapier3d::prelude::*;

/// The design deceleration, 5/7·c_rr·g, mm/s².
fn design_mm_s2() -> f32 {
    5.0 / 7.0 * CATS_EYE.rolling_resistance * G * 1_000.0
}

/// Allowed relative error of the level-equivalent deceleration.
const TOLERANCE: f32 = 0.10;

fn place(piece: &str, x: i32, y: i32, z: i32, yaw: u8, mirror: bool) -> Placement {
    Placement::new(
        PieceId::new(piece).unwrap(),
        CellCoord::new(x, y, z),
        Yaw4::new(yaw),
        mirror.into(),
    )
}

/// Rest centre height on a floor `level` in cell layer 0.
fn rest(level: i32) -> f32 {
    0.015 * level as f32 + CATS_EYE.radius_m
}

struct Case {
    name: &'static str,
    layout: Vec<Placement>,
    at: Vector,
    vel: Vector,
    ticks: u32,
    window: fn(&MarbleState) -> bool,
    /// Whether the run goes along +x (and is compared by x rather than by path length).
    along_x: bool,
}

fn cases() -> Vec<Case> {
    let s = |piece: &str, x: i32, z: i32| place(piece, x, 0, z, 0, false);
    vec![
        Case {
            name: "straight (mesh)",
            layout: (0..20).map(|x| s("blk_straight_l3", x, 0)).collect(),
            at: Vector::new(0.03, rest(3), 0.0),
            vel: Vector::X * 0.2,
            ticks: 720,
            window: |m| (0.09..1.11).contains(&m.position[0]),
            along_x: true,
        },
        Case {
            name: "bend (blk_curve90_l2 ring)",
            layout: vec![
                place("blk_curve90_l2", 0, 0, 0, 2, false),
                place("blk_curve90_l2", 1, 0, 0, 1, false),
                place("blk_curve90_l2", 1, 0, 1, 0, false),
                place("blk_curve90_l2", 0, 0, 1, 3, false),
            ],
            at: Vector::new(0.03, rest(2), 0.0),
            vel: Vector::X * 0.05,
            ticks: 480,
            window: |_| true,
            along_x: false,
        },
        Case {
            // The slope's exact contact owns the ball from a ball diameter before its cell, on the
            // level end of the straight before it: the level-equivalent of the slope contact. On
            // the descent itself (x 30..78 mm, gravity cancelled by matching x) it measures 12.47
            // mm/s² (12.39 for its 1.06 g mean load) and 16.42 with the stale load, while the
            // solved load matches the |a − g| the motion needs to 0.01 g every tick.
            name: "slope (blk_slope_gentle_l3 exact contact, level lead-in)",
            layout: vec![
                s("blk_straight_l3", 0, 0),
                s("blk_slope_gentle_l3", 1, 0),
                s("blk_straight_l2", 2, 0),
                s("blk_straight_l2", 3, 0),
                s("blk_straight_l2", 4, 0),
            ],
            at: Vector::new(0.012, rest(3), 0.0),
            vel: Vector::X * 0.03,
            ticks: 300,
            window: |m| (0.014..0.030).contains(&m.position[0]),
            along_x: true,
        },
        Case {
            name: "junction (blk_join_l3)",
            layout: vec![
                s("blk_straight_l3", 0, 0),
                s("blk_join_l3", 1, 0),
                place("blk_straight_l3", 1, 0, -1, 1, false),
                s("blk_straight_l3", 2, 0),
                s("blk_straight_l3", 3, 0),
            ],
            at: Vector::new(0.0, rest(3), 0.0),
            vel: Vector::X * 0.06,
            ticks: 720,
            window: |m| (0.035..0.085).contains(&m.position[0]),
            along_x: true,
        },
        Case {
            name: "arc (rail_curve90_r90_l2)",
            layout: vec![
                s("blk_straight_l2", 0, 0),
                s("blk_straight_l2", 1, 0),
                s("rail_curve90_r90_l2", 2, 0),
                place("blk_straight_l2", 3, 0, -2, 1, false),
                place("blk_straight_l2", 3, 0, -3, 1, false),
            ],
            at: Vector::new(0.05, rest(2), 0.0),
            vel: Vector::X * 0.12,
            ticks: 720,
            window: |m| m.position[0] > 0.1 && m.position[2] > -0.08,
            along_x: false,
        },
    ]
}

/// One run of `case`: the marble's state each tick, with rolling resistance `crr` and the load
/// summed over stale manifold points when `stale`.
fn run(case: &Case, crr: f32, stale: bool) -> Vec<MarbleState> {
    let kit = parble_kit::builtin();
    let mut sim = Sim::empty();
    sim.stale_rolling_load = stale;
    for p in &case.layout {
        sim.place_piece(kit, p.clone()).unwrap();
    }
    sim.rebuild_chunks(kit).unwrap();
    let spec = crate::marble::MarbleSpec {
        rolling_resistance: crr,
        ..CATS_EYE
    };
    let (mut body, collider) = spec.build(case.at);
    let speed = case.vel.length();
    body.set_linvel(case.vel, true);
    body.set_angvel(
        Vector::Y.cross(case.vel / speed) * (speed / spec.radius_m),
        true,
    );
    let h = sim.world.bodies.insert(body);
    sim.world
        .colliders
        .insert_with_parent(collider, h, &mut sim.world.bodies);
    sim.marbles.push(h);
    (0..case.ticks)
        .map(|_| {
            sim.tick(1);
            sim.marbles()[0]
        })
        .collect()
}

fn speed(m: &MarbleState) -> f32 {
    let [x, y, z] = m.linvel;
    (x * x + y * y + z * z).sqrt()
}

/// Least-squares slope of `(x, y)`.
fn slope(points: &[(f32, f32)]) -> f32 {
    let n = points.len() as f32;
    let (mx, my) = points
        .iter()
        .fold((0.0, 0.0), |(a, b), &(x, y)| (a + x / n, b + y / n));
    let (num, den) = points.iter().fold((0.0, 0.0), |(a, b), &(x, y)| {
        (a + (x - mx) * (y - my), b + (x - mx) * (x - mx))
    });
    num / den
}

/// Smoothing half-width of the load estimate, ticks.
const SMOOTH: usize = 6;

/// `(key, path length, ½v²)` after each tick of `tr`: the key is the marble's x when `along_x`,
/// else its path length from the start.
fn energy_along(tr: &[MarbleState], along_x: bool) -> Vec<(f32, f32, f32)> {
    let mut s = 0.0;
    tr.iter()
        .map(|m| {
            let v = speed(m);
            s += v * DT;
            (if along_x { m.position[0] } else { s }, s, 0.5 * v * v)
        })
        .collect()
}

/// `(raw, level-equivalent)` deceleration rolling resistance adds on `case`, mm/s², with the
/// `stale` load, and the ticks it was measured over.
///
/// The two runs are compared at the same place, so gravity (which depends on where the marble is)
/// cancels: the specific kinetic energy ½v² the run with rolling resistance has lost against the
/// control grows by the deceleration per metre it travels. Runs along x are matched by x (on the
/// slope a 34 µm mismatch is a 10% error, more than the two runs' path lengths drift apart), the
/// bend ring and the rail curve by path length. The bend's own losses grow fast with speed (the
/// control ring loses 0.35 mm/s² at 0.05 m/s and 7.7 at 0.15), so every case runs slowly enough
/// that the two runs' speeds stay close.
fn deceleration(case: &Case, stale: bool) -> (f32, f32, usize) {
    let with = run(case, CATS_EYE.rolling_resistance, stale);
    let without = run(case, 0.0, stale);
    let (e1, e0) = (
        energy_along(&with, case.along_x),
        energy_along(&without, case.along_x),
    );
    // The control's ½v² at `key`, linearly interpolated.
    let e0_at = |key: f32| {
        let k = e0.partition_point(|&(ki, _, _)| ki < key);
        (k > 0 && k < e0.len()).then(|| {
            let ((ka, _, ea), (kb, _, eb)) = (e0[k - 1], e0[k]);
            ea + (eb - ea) * (key - ka) / (kb - ka)
        })
    };
    let points: Vec<(usize, (f32, f32))> = (SMOOTH + 1..with.len() - SMOOTH)
        .filter(|&i| (case.window)(&with[i]))
        .filter_map(|i| {
            let (key, s, e) = e1[i];
            e0_at(key).map(|e0| (i, (s, e0 - e)))
        })
        .collect();
    assert!(
        points.len() > 30,
        "{}: {} ticks in the window",
        case.name,
        points.len()
    );
    let raw = slope(&points.iter().map(|p| p.1).collect::<Vec<_>>()) * 1_000.0;
    // Normal load per unit mass the motion needs, |a − g|, from a velocity difference over
    // 2·SMOOTH ticks (the contact jitter alternates tick to tick), averaged over path length like
    // the energy it loads.
    let (sum, path) = points.iter().fold((0.0, 0.0), |(sum, path), &(i, _)| {
        let v = |k: usize| Vector::from(with[k].linvel);
        let acc = (v(i + SMOOTH) - v(i - SMOOTH)) / (2.0 * SMOOTH as f32 * DT);
        let ds = speed(&with[i]);
        (sum + (acc + Vector::Y * G).length() * ds, path + ds)
    });
    let load = sum / path;
    (raw, raw * G / load, points.len())
}

#[test]
fn rolling_resistance_decelerates_at_the_design_value_on_every_contact_type() {
    let design = design_mm_s2();
    let mut failures = Vec::new();
    let mut control_failures = 0;
    for case in cases() {
        let (raw, level, n) = deceleration(&case, false);
        let (stale_raw, stale_level, _) = deceleration(&case, true);
        eprintln!(
            "{}: {raw:.2} mm/s² ({level:.2} level-equivalent) over {n} ticks; stale load \
             {stale_raw:.2} ({stale_level:.2}); design {design:.2}",
            case.name
        );
        if (level / design - 1.0).abs() > TOLERANCE {
            failures.push(format!("{} {level:.2}", case.name));
        }
        if (stale_level / design - 1.0).abs() > TOLERANCE {
            control_failures += 1;
        }
    }
    assert!(
        failures.is_empty(),
        "off the design {design:.2}: {failures:?}"
    );
    // Negative control: the load summed over stale manifold points misses the design value on
    // the exact contacts.
    assert!(
        control_failures > 0,
        "the stale-load control met the design everywhere"
    );
}
