//! P2-T2 passive block pieces: a cat's eye marble is run through each new piece in a small
//! layout through the real API (chunk merge, simplification, contact hooks), and the test asserts
//! the port it leaves by and its speed there.
//!
//! The exit speed is bounded above by energy: with the launch rolling (ω = v / r), kinetic
//! energy is 0.7 m v0², so at the exit `v² ≤ 1.4 v0² + 2 g Δh` whatever mix of rolling,
//! sliding and falling happened on the way. The lower bound is half the speed of the ideal
//! loss-free roll for the same drop (`v² = v0² + (10/7) g Δh`): it catches a marble that stalls,
//! jams or crawls out, not normal rolling and impact losses.

use crate::marble::{CATS_EYE, G};
use crate::{MarbleState, Sim};
use parble_core::{CellCoord, PieceId, Placement, Yaw4};
use rapier3d::prelude::*;

const R: f32 = 0.008;
const V0: f32 = 0.3;

fn place(piece: &str, x: i32, y: i32, z: i32, yaw: u8, mirror: bool) -> Placement {
    Placement::new(
        PieceId::new(piece).unwrap(),
        CellCoord::new(x, y, z),
        Yaw4::new(yaw),
        mirror.into(),
    )
}

/// `ticks` of a marble launched rolling at `at` with velocity `vel` over `layout`.
fn run(layout: &[Placement], at: Vector, vel: Vector, ticks: u32) -> Vec<MarbleState> {
    let kit = parble_kit::builtin();
    let mut sim = Sim::empty();
    for p in layout {
        sim.place_piece(kit, p.clone()).unwrap();
    }
    sim.rebuild_chunks(kit).unwrap();
    let (mut body, collider) = CATS_EYE.build(at);
    let speed = vel.length();
    body.set_linvel(vel, true);
    body.set_angvel(
        Vector::Y.cross(vel / speed) * (speed / CATS_EYE.radius_m),
        true,
    );
    let h = sim.world.bodies.insert(body);
    sim.world
        .colliders
        .insert_with_parent(collider, h, &mut sim.world.bodies);
    sim.marbles.push(h);
    (0..ticks)
        .map(|_| {
            sim.tick(1);
            sim.marbles()[0]
        })
        .collect()
}

/// A port as a plane the marble centre crosses: `axis` (0 x, 1 y, 2 z), plane coordinate and
/// travel sign, and where the centre must be on the plane (`centre`, metres; the plane axis entry
/// is ignored) within `lateral` across the port and, for side ports, `rise` above the rest height.
struct Exit {
    name: &'static str,
    axis: usize,
    plane: f32,
    sign: f32,
    centre: [f32; 3],
    lateral: f32,
    rise: f32,
    /// For a piece the marble falls through and lands in: the drop from the landing floor to this
    /// port, metres. A landing absorbs the fall, so the lower bound counts only this drop.
    landing: Option<f32>,
}

/// A side port on an X or Z face: the marble rests on the floor at `floor` (world metres).
fn side(name: &'static str, axis: usize, plane: f32, sign: f32, u: f32, floor: f32) -> Exit {
    let mut centre = [0.0; 3];
    centre[2 - axis] = u;
    centre[1] = floor + R;
    Exit {
        name,
        axis,
        plane,
        sign,
        centre,
        lateral: 0.0061, // §6: a standard marble's 6 mm play in the 28 mm channel, + 0.1 mm
        rise: 0.015,
        landing: None,
    }
}

/// `e` reached after a fall onto a redirect's landing floor, `LANDING_RISE` above the exit floor
/// (blender/kit/kitblocks.py).
fn landed(e: Exit) -> Exit {
    landed_from(e, 0.015)
}

/// `e` reached after a fall onto a landing floor `h` metres above the exit floor.
fn landed_from(e: Exit, h: f32) -> Exit {
    Exit {
        landing: Some(h),
        ..e
    }
}

/// A bottom drop port at plan centre `(x, z)`, face height `y`.
fn bottom(name: &'static str, x: f32, y: f32, z: f32) -> Exit {
    Exit {
        name,
        axis: 1,
        plane: y,
        sign: -1.0,
        centre: [x, y, z],
        lateral: 0.008, // §4: 32 mm hole, 16 mm marble
        rise: 0.0,
        landing: None,
    }
}

/// Checks the first crossing of `exit` after tick `after` and returns (tick, speed).
fn check_exit(tr: &[MarbleState], exit: &Exit, start_y: f32, after: usize) -> (usize, f32) {
    let ax = exit.axis;
    let past = |m: &MarbleState| (m.position[ax] - exit.plane) * exit.sign >= 0.0;
    let i = (after.max(1)..tr.len())
        .find(|&i| past(&tr[i]) && !past(&tr[i - 1]))
        .unwrap_or_else(|| {
            let last = tr.last().unwrap().position;
            panic!(
                "{}: marble never left by this port (last {last:?})",
                exit.name
            )
        });
    let m = &tr[i];
    for k in (0..3).filter(|&k| k != ax) {
        let d = m.position[k] - exit.centre[k];
        if ax != 1 && k == 1 {
            assert!(
                (-0.001..=exit.rise).contains(&d),
                "{}: centre {:.4} m at the port, rest height {:.4}",
                exit.name,
                m.position[1],
                exit.centre[1]
            );
        } else {
            assert!(
                d.abs() <= exit.lateral,
                "{}: centre {:?} is {:.4} m off the port centre on axis {k}",
                exit.name,
                m.position,
                d
            );
        }
    }
    let v = Vector::from(m.linvel);
    assert!(
        v[ax] * exit.sign > 0.0,
        "{}: velocity {:?} does not leave through the port",
        exit.name,
        m.linvel
    );
    let dh = start_y - m.position[1];
    let hi = (1.4 * V0 * V0 + 2.0 * G * dh).sqrt();
    let lo = match exit.landing {
        Some(h) => 0.5 * (10.0 / 7.0 * G * h).sqrt(),
        None => 0.5 * (V0 * V0 + 10.0 / 7.0 * G * dh).max(0.0).sqrt(),
    };
    let s = v.length();
    eprintln!(
        "{}: tick {i}, centre {:?}, speed {s:.3} m/s (range {lo:.3}..{hi:.3}, drop {:.1} mm)",
        exit.name,
        m.position,
        dh * 1000.0
    );
    assert!(
        (lo..=hi).contains(&s),
        "{}: exit speed {s:.3} m/s outside {lo:.3}..{hi:.3}",
        exit.name
    );
    (i, s)
}

/// Checks each exit in order along one run.
fn check_exits(tr: &[MarbleState], exits: &[Exit], start_y: f32) {
    let mut after = 0;
    for e in exits {
        after = check_exit(tr, e, start_y, after).0;
    }
}

/// Rest centre height on a floor `level` in cell layer `layer`.
fn rest(layer: i32, level: i32) -> f32 {
    0.06 * layer as f32 + 0.015 * level as f32 + R
}

/// Diagnostic sweep (`cargo test -p parble-sim --release drop_l2_entry_sweep -- --ignored
/// --nocapture`): a marble rolling down `blk_straight_l2` into `blk_drop_l2`, over entry speed,
/// lateral offset and spin. Prints whether it falls through the bore or stalls.
#[test]
#[ignore = "diagnostic sweep"]
fn drop_l2_entry_sweep() {
    let mut layout: Vec<Placement> = (0..4)
        .map(|x| place("blk_straight_l2", x, 1, 0, 0, false))
        .collect();
    layout.push(place("blk_drop_l2", 4, 1, 0, 0, false));
    layout.push(place("blk_catch_l1", 4, 0, 0, 0, false));
    let kit = parble_kit::builtin();
    for spin in [1.0_f32, 0.0] {
        for v in [0.25_f32, 0.35, 0.45, 0.55, 0.7] {
            let mut row = String::new();
            for dz in [-0.006_f32, -0.003, 0.0, 0.003, 0.006] {
                let mut sim = Sim::empty();
                for p in &layout {
                    sim.place_piece(kit, p.clone()).unwrap();
                }
                sim.rebuild_chunks(kit).unwrap();
                let at = Vector::new(-0.02, rest(1, 2), dz);
                let (mut body, collider) = CATS_EYE.build(at);
                let vel = Vector::X * v;
                body.set_linvel(vel, true);
                body.set_angvel(
                    Vector::Y.cross(vel / v) * (spin * v / CATS_EYE.radius_m),
                    true,
                );
                let h = sim.world.bodies.insert(body);
                sim.world
                    .colliders
                    .insert_with_parent(collider, h, &mut sim.world.bodies);
                sim.marbles.push(h);
                let mut verdict = "stall";
                let mut stall_x = 0.0;
                for _ in 0..900 {
                    sim.tick(1);
                    let m = sim.marbles()[0];
                    if m.position[1] < 0.06 {
                        verdict = "fall ";
                        break;
                    }
                    stall_x = m.position[0];
                }
                row += &format!(" {verdict}({:+.0}mm)", (stall_x - 0.24) * 1000.0);
            }
            println!("spin {spin:.0} v {v:.2}:{row}");
        }
    }
}

/// Runs a marble from `snap` (rolling) over `pieces` and says whether it falls below `drop_y`
/// within 900 ticks, and where it ended.
fn bisect_trial(pieces: &[Placement], snap: MarbleState, drop_y: f32) -> (bool, [f32; 3]) {
    let kit = parble_kit::builtin();
    let mut sim = Sim::empty();
    for p in pieces {
        sim.place_piece(kit, p.clone()).unwrap();
    }
    sim.rebuild_chunks(kit).unwrap();
    let (mut body, collider) = CATS_EYE.build(Vector::from(snap.position));
    let vel = Vector::from(snap.linvel);
    body.set_linvel(vel, true);
    let flat = Vector::new(vel.x, 0.0, vel.z);
    let s = flat.length();
    if s > 0.01 {
        body.set_angvel(Vector::Y.cross(flat / s) * (s / CATS_EYE.radius_m), true);
    }
    let h = sim.world.bodies.insert(body);
    sim.world
        .colliders
        .insert_with_parent(collider, h, &mut sim.world.bodies);
    sim.marbles.push(h);
    let mut last = snap.position;
    for _ in 0..900 {
        sim.tick(1);
        last = sim.marbles()[0].position;
        if last[1] < drop_y {
            return (true, last);
        }
    }
    (false, last)
}

/// Whether a marble from `snap` (rolling) over `pieces` ever drops below `y_lo` in 900 ticks.
fn falls_below(
    pieces: &[Placement],
    snap: MarbleState,
    y_lo: f32,
    zone: Option<(CellCoord, f32)>,
) -> bool {
    let kit = parble_kit::builtin();
    let mut sim = Sim::empty();
    for p in pieces {
        sim.place_piece(kit, p.clone()).unwrap();
    }
    sim.rebuild_chunks(kit).unwrap();
    let (mut body, collider) = CATS_EYE.build(Vector::from(snap.position));
    let vel = Vector::from(snap.linvel);
    body.set_linvel(vel, true);
    let flat = Vector::new(vel.x, 0.0, vel.z);
    let s = flat.length();
    if s > 0.01 {
        body.set_angvel(Vector::Y.cross(flat / s) * (s / CATS_EYE.radius_m), true);
    }
    let h = sim.world.bodies.insert(body);
    sim.world
        .colliders
        .insert_with_parent(collider, h, &mut sim.world.bodies);
    sim.marbles.push(h);
    (0..900).any(|_| {
        sim.tick(1);
        let m = sim.marbles()[0].position;
        // Only a fall inside the zone's cell counts, `inset` metres in from its edges.
        let inside = zone.is_none_or(|(c, inset)| {
            (m[0] - c.x as f32 * 0.06).abs() <= 0.03 - inset
                && (m[2] - c.z as f32 * 0.06).abs() <= 0.03 - inset
        });
        inside && m[1] < y_lo
    })
}

/// Generated track `PROCGEN_FALL_SEED` (default 72): the first marble leaves its track through
/// the floor of the last non-pillar piece it touches. Replays it from two pieces earlier and
/// shrinks the layout while the fall persists.
#[test]
#[ignore = "diagnostic: bisect a fall through the floor"]
fn fall_bisect() {
    use crate::track_run::{MarbleMotion, TrackRun};
    use parble_core::PieceCatalog;
    use parble_procgen::{GenParams, generate};
    let kit = parble_kit::builtin();
    let defs: Vec<_> = kit.iter().map(|p| p.def.clone()).collect();
    let seed: u64 = std::env::var("PROCGEN_FALL_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(72);
    let track = generate(&defs, seed, &GenParams::default()).unwrap();
    let cell_of = |p: [f32; 3]| {
        CellCoord::new(
            (p[0] / 0.06).round() as i32,
            (p[1] / 0.06).floor() as i32,
            (p[2] / 0.06).round() as i32,
        )
    };
    let mut run = TrackRun::new(&track).unwrap();
    let mut snaps: Vec<(usize, MarbleState)> = Vec::new();
    let mut last = None;
    for _ in 0..1600 {
        run.tick(15);
        let Some(m) = run.marbles().first().copied() else {
            continue;
        };
        let c = cell_of(m.position);
        if let Some(i) = track.placements.iter().position(|p| {
            kit.get(&p.piece)
                .is_some_and(|d| p.world_cells(d).contains(&c))
        }) && last != Some(i)
        {
            if track.placements[i].piece.as_str() == "blk_pillar" {
                break;
            }
            snaps.push((i, m));
            last = Some(i);
        }
        if !matches!(run.motion().first(), Some(MarbleMotion::Moving { .. })) {
            break;
        }
    }
    let d = last.expect("the marble touched a piece");
    let piece = &track.placements[d];
    let level_m = match piece.piece.as_str().rsplit('_').next() {
        Some("l1") => 0.015,
        Some("l2") => 0.030,
        _ => 0.045,
    };
    let y_lo = 0.06 * piece.anchor.y as f32 + level_m - 0.006;
    let snap = snaps[snaps.len() - 3].1;
    println!(
        "seed {seed}: falls through #{d} {} (floor {:.0} mm), replay from #{}",
        piece.piece,
        (y_lo + 0.006) * 1000.0,
        snaps[snaps.len() - 3].0
    );
    let zone = Some((piece.anchor, 0.003));
    println!(
        "whole track falls: {}",
        falls_below(&track.placements, snap, y_lo, zone)
    );
    let keep: std::collections::BTreeSet<usize> = (snaps[snaps.len() - 3].0..=d + 1).collect();
    let mut set: Vec<(usize, Placement)> = track.placements.iter().cloned().enumerate().collect();
    let mut i = 0;
    while i < set.len() {
        if keep.contains(&set[i].0) {
            i += 1;
            continue;
        }
        let mut trial = set.clone();
        trial.remove(i);
        let pieces: Vec<Placement> = trial.iter().map(|(_, p)| p.clone()).collect();
        if falls_below(&pieces, snap, y_lo, zone) {
            set = trial;
        } else {
            i += 1;
        }
    }
    println!("minimal layout that still falls: {} pieces", set.len());
    for (idx, p) in &set {
        println!(
            "  #{idx} {} anchor ({},{},{}) yaw {:?} mirror {:?}",
            p.piece, p.anchor.x, p.anchor.y, p.anchor.z, p.yaw, p.mirror
        );
    }
}

/// A gentle slope four layers above a curve's straights, in the same plan cell as one of them:
/// the bend's flat continuation tags those straights, and `covers` once looked at the plan only,
/// so the slope far above "covered" the marble and cleared the straight's contacts. Generated seed
/// 266 (P6-T1b follow-up): a marble at 0.17 m/s fell through the floor onto the support pillar.
#[test]
fn a_slope_far_above_does_not_open_the_floor_of_straights_after_a_curve() {
    let layout = vec![
        place("blk_slope_gentle_l3", -4, 5, -4, 1, false),
        place("blk_curve90_l3", -7, 1, -4, 3, true),
        place("blk_straight_l3", -6, 1, -4, 0, false),
        place("blk_straight_l3", -5, 1, -4, 0, false),
        place("blk_straight_l3", -4, 1, -4, 2, false),
        place("blk_redirect_l3", -3, 1, -4, 0, false),
    ];
    let zone = Some((CellCoord::new(-4, 1, -4), 0.003));
    for v in [0.15_f32, 0.2, 0.3] {
        let snap = MarbleState {
            position: [-6.0 * 0.06, 0.06 + 0.045 + R, -4.0 * 0.06],
            rotation: [0.0, 0.0, 0.0, 1.0],
            linvel: [v, 0.0, 0.0],
        };
        assert!(
            !falls_below(&layout, snap, 0.06 + 0.045 - 0.006, zone),
            "{v} m/s: the marble fell through the straight under the slope"
        );
    }
}

/// Seed 72's failing stretch: three L3 straights into a redirect, a catch one layer above the
/// last straight. Variants of it say which piece the fall needs.
#[test]
#[ignore = "diagnostic sweep"]
fn straight_into_redirect_under_a_catch() {
    let base = |catch: Option<&str>, redirect: &str, ry: u8| {
        let mut l = vec![
            place("blk_straight_l3", -6, 1, 5, 3, false),
            place("blk_straight_l3", -6, 1, 4, 1, false),
            place("blk_straight_l3", -6, 1, 3, 1, false),
            place(redirect, -6, 1, 2, ry, false),
        ];
        if let Some(c) = catch {
            l.push(place(c, -6, 2, 3, 2, false));
        }
        l
    };
    let zone = Some((CellCoord::new(-6, 1, 3), 0.003));
    for (name, layout) in [
        (
            "seed 72 as is",
            base(Some("blk_catch_l1"), "blk_redirect_l3", 1),
        ),
        ("without the catch", base(None, "blk_redirect_l3", 1)),
        (
            "a straight above",
            base(Some("blk_straight_l3"), "blk_redirect_l3", 1),
        ),
        (
            "a drop above",
            base(Some("blk_drop_l2"), "blk_redirect_l3", 1),
        ),
        ("no redirect (a straight)", {
            let mut l = base(Some("blk_catch_l1"), "blk_straight_l3", 1);
            l.retain(|p| p.piece.as_str() != "blk_pillar");
            l
        }),
    ] {
        let mut row = String::new();
        for v in [0.15_f32, 0.25, 0.4] {
            let snap = MarbleState {
                position: [-6.0 * 0.06, 0.06 + 0.045 + R, 5.0 * 0.06],
                rotation: [0.0, 0.0, 0.0, 1.0],
                linvel: [0.0, 0.0, -v],
            };
            let fell = falls_below(&layout, snap, 0.06 + 0.045 - 0.006, zone);
            row += &format!(" {v:.2} {}", if fell { "FALLS" } else { "ok   " });
        }
        println!("{name:<28}{row}");
    }
}

/// A row of L3 straights on layer 1 with one piece in the layer above, over speeds: does the
/// marble ever drop through the straights' floor?
#[test]
#[ignore = "diagnostic sweep"]
fn floor_under_a_piece_above_sweep() {
    for above in [
        "",
        "blk_catch_l1",
        "blk_straight_l3",
        "blk_drop_l2",
        "blk_redirect_l3",
        "blk_pillar",
    ] {
        let mut row = String::new();
        for v in [0.15_f32, 0.3, 0.5] {
            let mut layout: Vec<Placement> = (0..40)
                .map(|x| place("blk_straight_l3", x, 1, 0, 0, false))
                .collect();
            if !above.is_empty() {
                layout.push(place(above, 2, 2, 0, 0, false));
            }
            let snap = MarbleState {
                position: [0.0, 0.06 + 0.045 + R, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                linvel: [v, 0.0, 0.0],
            };
            let fell = falls_below(&layout, snap, 0.06 + 0.045 - 0.004, None);
            row += &format!(" {v:.2} {}", if fell { "FALLS" } else { "ok   " });
        }
        println!(
            "above: {:<16}{row}",
            if above.is_empty() { "nothing" } else { above }
        );
    }
}

/// A hand-built layout: an optional gentle slope, `n` straights, the drop and its catch, all on one
/// row. The marble starts rolling at the first straight's centre.
#[test]
#[ignore = "diagnostic sweep"]
fn drop_after_slope_sweep() {
    for slope in [
        None,
        Some("blk_slope_gentle_l3"),
        Some("rail_slope_gentle_l3"),
    ] {
        for n in [1, 2, 3, 4, 5] {
            let mut layout = Vec::new();
            let mut x = 0;
            if let Some(s) = slope {
                layout.push(place(s, x, 1, 0, 0, false));
                x += 1;
            }
            let first = x;
            for _ in 0..n {
                layout.push(place("blk_straight_l2", x, 1, 0, 0, false));
                x += 1;
            }
            layout.push(place("blk_drop_l2", x, 1, 0, 0, false));
            layout.push(place("blk_catch_l1", x, 0, 0, 0, false));
            let mut row = String::new();
            for v in [0.3_f32, 0.5, 0.7] {
                let snap = MarbleState {
                    position: [first as f32 * 0.06, rest(1, 2), 0.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    linvel: [v, 0.0, 0.0],
                };
                let (fell, end) = bisect_trial(&layout, snap, 0.06);
                row += &format!(
                    " v{v:.1} {} ({:.0},{:.0})",
                    if fell { "falls " } else { "STALLS" },
                    (end[0] - x as f32 * 0.06) * 1000.0,
                    end[1] * 1000.0
                );
            }
            println!("slope {:?} + {n} straights:{row}", slope.unwrap_or("none"));
        }
    }
}

/// A gentle slope on the drop's row, in its chunk, tagged the drop cell's floor triangles through
/// its flat continuation, and the catch's landing floor (under the drop, so "over" the marble in
/// plan) took their contacts: a marble at 0.5 m/s sank 6 mm and stopped 20 mm short of the bore.
#[test]
fn a_slope_on_the_drops_row_does_not_stall_the_marble() {
    for slope in ["blk_slope_gentle_l3", "rail_slope_gentle_l3"] {
        for straights in 1..=4 {
            let mut layout = vec![place(slope, 0, 1, 0, 0, false)];
            layout.extend((1..=straights).map(|x| place("blk_straight_l2", x, 1, 0, 0, false)));
            layout.push(place("blk_drop_l2", straights + 1, 1, 0, 0, false));
            layout.push(place("blk_catch_l1", straights + 1, 0, 0, 0, false));
            for v in [0.4_f32, 0.5, 0.6] {
                let snap = MarbleState {
                    position: [0.06, rest(1, 2), 0.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    linvel: [v, 0.0, 0.0],
                };
                let (fell, end) = bisect_trial(&layout, snap, 0.06);
                assert!(
                    fell,
                    "{slope} + {straights} straights, {v} m/s: stalled at {end:?}"
                );
            }
        }
    }
}

/// Which extra piece, and where, makes the marble stall in the drop: straights, drop and catch on
/// one row, plus one off-row piece.
#[test]
#[ignore = "diagnostic sweep"]
fn drop_stall_triggers() {
    let extras: [(&str, &str, [i32; 3]); 9] = [
        (
            "slope on the row, same chunk",
            "blk_slope_gentle_l3",
            [0, 1, 0],
        ),
        (
            "slope on the row, other chunk",
            "blk_slope_gentle_l3",
            [-9, 1, 0],
        ),
        (
            "slope off the row, same chunk",
            "blk_slope_gentle_l3",
            [0, 1, 3],
        ),
        ("steep slope off the row", "blk_slope_steep_l3", [0, 1, 3]),
        ("block curve off the row", "blk_curve90_l2", [0, 1, 3]),
        ("block straight off the row", "blk_straight_l2", [0, 1, 3]),
        ("rail straight off the row", "rail_straight_l2", [0, 1, 3]),
        ("redirect off the row", "blk_redirect_l2", [0, 1, 3]),
        ("nothing", "", [0, 0, 0]),
    ];
    for (name, piece, at) in extras {
        let mut layout: Vec<Placement> = (1..4)
            .map(|x| place("blk_straight_l2", x, 1, 0, 0, false))
            .collect();
        layout.push(place("blk_drop_l2", 4, 1, 0, 0, false));
        layout.push(place("blk_catch_l1", 4, 0, 0, 0, false));
        if !piece.is_empty() {
            layout.push(place(piece, at[0], at[1], at[2], 0, false));
        }
        let mut row = String::new();
        for v in [0.4_f32, 0.45, 0.5, 0.55, 0.6] {
            let snap = MarbleState {
                position: [0.06, rest(1, 2), 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                linvel: [v, 0.0, 0.0],
            };
            let (fell, _) = bisect_trial(&layout, snap, 0.06);
            row += &format!(" {v:.2} {}", if fell { "ok   " } else { "STALL" });
        }
        println!("{name:<32}{row}");
    }
}

/// Finds generated tracks whose first marble rests inside `blk_drop_l2` and replays that marble
/// from two pieces before the drop over smaller and smaller layouts, to find what the stall needs.
#[test]
#[ignore = "diagnostic: bisect a drop stall"]
fn drop_stall_bisect() {
    use crate::track_run::{MarbleMotion, TrackRun};
    use parble_core::PieceCatalog;
    use parble_procgen::{GenParams, generate};
    let kit = parble_kit::builtin();
    let defs: Vec<_> = kit.iter().map(|p| p.def.clone()).collect();
    let cell_of = |p: [f32; 3]| {
        CellCoord::new(
            (p[0] / 0.06).round() as i32,
            (p[1] / 0.06).floor() as i32,
            (p[2] / 0.06).round() as i32,
        )
    };
    let mut found = 0;
    for seed in 0..300u64 {
        let Ok(track) = generate(&defs, seed, &GenParams::default()) else {
            continue;
        };
        let mut run = TrackRun::new(&track).unwrap();
        let mut snaps: Vec<(usize, MarbleState)> = Vec::new();
        let mut last = None;
        let mut at_rest = false;
        for _ in 0..1600 {
            run.tick(15);
            let Some(m) = run.marbles().first().copied() else {
                continue;
            };
            let c = cell_of(m.position);
            if let Some(i) = track.placements.iter().position(|p| {
                kit.get(&p.piece)
                    .is_some_and(|d| p.world_cells(d).contains(&c))
            }) && last != Some(i)
            {
                snaps.push((i, m));
                last = Some(i);
            }
            match run.motion().first() {
                Some(MarbleMotion::AtRest) => {
                    at_rest = true;
                    break;
                }
                Some(MarbleMotion::Finished | MarbleMotion::Lost | MarbleMotion::OffTrack) => break,
                _ => {}
            }
        }
        let Some(d) = last else { continue };
        if !at_rest || track.placements[d].piece.as_str() != "blk_drop_l2" || snaps.len() < 3 {
            continue;
        }
        let snap = snaps[snaps.len() - 3].1;
        let drop = &track.placements[d];
        let drop_y = 0.06 * drop.anchor.y as f32;
        let near = |p: &Placement| {
            (p.anchor.x - drop.anchor.x).abs() <= 3
                && (p.anchor.z - drop.anchor.z).abs() <= 3
                && (p.anchor.y - drop.anchor.y).abs() <= 3
        };
        let chain: Vec<Placement> = track.placements[d - 2..=d + 1].to_vec();
        let pillars: Vec<Placement> = track
            .placements
            .iter()
            .filter(|p| p.piece.as_str() == "blk_pillar")
            .cloned()
            .collect();
        let nearby: Vec<Placement> = track
            .placements
            .iter()
            .filter(|p| near(p))
            .cloned()
            .collect();
        let mut with_pillars = chain.clone();
        with_pillars.extend(pillars.iter().filter(|p| near(p)).cloned());
        let variants: [(&str, Vec<Placement>); 4] = [
            ("two straights + drop + catch", chain.clone()),
            ("+ pillars within 3 cells", with_pillars),
            ("every piece within 3 cells", nearby),
            ("the whole track", track.placements.clone()),
        ];
        let mut line = String::new();
        for (name, pieces) in &variants {
            let (fell, end) = bisect_trial(pieces, snap, drop_y);
            line += &format!(
                " | {name}: {} ({:.0},{:.0},{:.0}) mm",
                if fell { "falls" } else { "STALLS" },
                end[0] * 1000.0,
                end[1] * 1000.0,
                end[2] * 1000.0
            );
        }
        println!(
            "seed {seed} drop yaw {:?} entry v {:.2}{line}",
            drop.yaw,
            Vector::from(snap.linvel).length()
        );
        // Greedy delta debugging from the whole track: drop every piece the stall survives
        // without, keeping the marble's own chain.
        if bisect_trial(&track.placements, snap, drop_y).0 {
            continue;
        }
        let keep: std::collections::BTreeSet<usize> = (d - 2..=d + 1).collect();
        let mut set: Vec<(usize, Placement)> =
            track.placements.iter().cloned().enumerate().collect();
        let mut i = 0;
        while i < set.len() {
            if keep.contains(&set[i].0) {
                i += 1;
                continue;
            }
            let mut trial = set.clone();
            trial.remove(i);
            let pieces: Vec<Placement> = trial.iter().map(|(_, p)| p.clone()).collect();
            if bisect_trial(&pieces, snap, drop_y).0 {
                i += 1;
            } else {
                set = trial;
            }
        }
        println!(
            "  minimal stalling layout for seed {seed}: {} pieces",
            set.len()
        );
        for (idx, p) in &set {
            println!(
                "    #{idx} {} anchor ({},{},{}) yaw {:?} mirror {:?}",
                p.piece, p.anchor.x, p.anchor.y, p.anchor.z, p.yaw, p.mirror
            );
        }
        found += 1;
        if found >= 8 {
            break;
        }
    }
}

#[test]
fn steep_slope_drops_two_levels_onto_l1() {
    let mut layout: Vec<Placement> = (0..2)
        .map(|x| place("blk_straight_l3", x, 0, 0, 0, false))
        .collect();
    layout.push(place("blk_slope_steep_l3", 2, 0, 0, 0, false));
    for x in 3..6 {
        layout.push(place("blk_straight_l1", x, 0, 0, 0, false));
    }
    let start = Vector::new(-0.02, rest(0, 3), 0.0);
    let tr = run(&layout, start, Vector::X * V0, 1_200);
    check_exits(
        &tr,
        &[
            side("blk_slope_steep_l3 +x (L1)", 0, 0.15, 1.0, 0.0, 0.015),
            side("blk_straight_l1 run-out", 0, 0.33, 1.0, 0.0, 0.015),
        ],
        start.y,
    );
}

/// Straight, gentle slope (L3 -> L2), then the L2 pieces that now follow it: straight, curve,
/// mirrored curve, gentle slope (L2 -> L1), then the L1 straight.
fn gentle_chain() -> Vec<Placement> {
    vec![
        place("blk_straight_l3", 0, 0, 0, 0, false),
        place("blk_slope_gentle_l3", 1, 0, 0, 0, false),
        place("blk_straight_l2", 2, 0, 0, 0, false),
        place("blk_curve90_l2", 3, 0, 0, 1, false),
        place("blk_straight_l2", 3, 0, 1, 1, false),
        place("blk_curve90_l2", 3, 0, 2, 0, true),
        place("blk_slope_gentle_l2", 4, 0, 2, 0, false),
        place("blk_straight_l1", 5, 0, 2, 0, false),
        place("blk_straight_l1", 6, 0, 2, 0, false),
    ]
}

#[test]
fn gentle_slope_exit_composes_with_the_l2_pieces() {
    let start = Vector::new(-0.02, rest(0, 3), 0.0);
    let tr = run(&gentle_chain(), start, Vector::X * V0, 2_000);
    check_exits(
        &tr,
        &[
            side("blk_slope_gentle_l3 +x (L2)", 0, 0.09, 1.0, 0.0, 0.030),
            side("blk_straight_l2 +x", 0, 0.15, 1.0, 0.0, 0.030),
            side("blk_curve90_l2 +z", 2, 0.03, 1.0, 0.18, 0.030),
            side("blk_straight_l2 (yaw 1) +z", 2, 0.09, 1.0, 0.18, 0.030),
            side("blk_curve90_l2 mirrored +x", 0, 0.21, 1.0, 0.12, 0.030),
            side("blk_slope_gentle_l2 +x (L1)", 0, 0.27, 1.0, 0.12, 0.015),
            side("blk_straight_l1 run-out", 0, 0.39, 1.0, 0.12, 0.015),
        ],
        start.y,
    );
}

#[test]
fn drop_pillar_and_tower_pass_a_marble_down() {
    // Drop hole on layer 2, support pillar on layer 1, tower socket block on layer 0: the marble
    // falls through the drop's bottom port, the pillar's bore and the tower's funnel, and leaves by
    // the tower's socket port (collision plane 10 mm inside the -x face, x = 40 mm).
    let layout = vec![
        place("blk_straight_l3", 0, 2, 0, 0, false),
        place("blk_drop_l3", 1, 2, 0, 0, false),
        place("blk_pillar", 1, 1, 0, 0, false),
        place("blk_tower_socket_l1", 1, 0, 0, 0, false),
    ];
    let start = Vector::new(-0.02, rest(2, 3), 0.0);
    let tr = run(&layout, start, Vector::X * V0, 1_200);
    check_exits(
        &tr,
        &[
            bottom("blk_drop_l3 bottom", 0.06, 0.12, 0.0),
            bottom("blk_pillar bottom", 0.06, 0.06, 0.0),
            // The tower socket's pad falls 0.75 mm to its end and the descent 1.3 mm more
            // (kitblocks `TOWER_PAD_FALL` and `TOWER_DESCENT`).
            landed_from(
                side("blk_tower_socket_l1 socket (-x)", 0, 0.04, -1.0, 0.0, 0.015),
                0.002_05,
            ),
        ],
        start.y,
    );
}

#[test]
fn drop_l2_follows_the_gentle_slope() {
    let layout = vec![
        place("blk_straight_l3", 0, 1, 0, 0, false),
        place("blk_slope_gentle_l3", 1, 1, 0, 0, false),
        place("blk_drop_l2", 2, 1, 0, 0, false),
    ];
    let start = Vector::new(-0.02, rest(1, 3), 0.0);
    let tr = run(&layout, start, Vector::X * V0, 900);
    check_exits(
        &tr,
        &[bottom("blk_drop_l2 bottom", 0.12, 0.06, 0.0)],
        start.y,
    );
}

#[test]
fn redirect_drops_one_layer_and_exits_sideways() {
    // L3: straight in on layer 1, out on layer 0 at L3 into a straight.
    let layout = vec![
        place("blk_straight_l3", 0, 1, 0, 0, false),
        place("blk_redirect_l3", 1, 1, 0, 0, false),
        place("blk_straight_l3", 2, 0, 0, 0, false),
        place("blk_straight_l3", 3, 0, 0, 0, false),
    ];
    let start = Vector::new(-0.02, rest(1, 3), 0.0);
    let tr = run(&layout, start, Vector::X * V0, 1_200);
    check_exits(
        &tr,
        &[
            landed(side(
                "blk_redirect_l3 +x (layer 0, L3)",
                0,
                0.09,
                1.0,
                0.0,
                0.045,
            )),
            landed(side("blk_straight_l3 run-out", 0, 0.21, 1.0, 0.0, 0.045)),
        ],
        start.y,
    );
    // L2, after the gentle slope's L2 exit.
    let layout = vec![
        place("blk_straight_l3", 0, 1, 0, 0, false),
        place("blk_slope_gentle_l3", 1, 1, 0, 0, false),
        place("blk_redirect_l2", 2, 1, 0, 0, false),
        place("blk_straight_l2", 3, 0, 0, 0, false),
        place("blk_straight_l2", 4, 0, 0, 0, false),
    ];
    let tr = run(&layout, start, Vector::X * V0, 1_200);
    check_exits(
        &tr,
        &[
            landed(side(
                "blk_redirect_l2 +x (layer 0, L2)",
                0,
                0.15,
                1.0,
                0.0,
                0.030,
            )),
            landed(side("blk_straight_l2 run-out", 0, 0.27, 1.0, 0.0, 0.030)),
        ],
        start.y,
    );
}

/// Joiner at (1, 0, 0): run-out straights east of it, a straight into port 0 (-x) and one into
/// port 1 (-z) along +z.
fn joiner(level: u8) -> Vec<Placement> {
    let (j, s) = match level {
        3 => ("blk_join_l3", "blk_straight_l3"),
        _ => ("blk_join_l2", "blk_straight_l2"),
    };
    vec![
        place(s, 0, 0, 0, 0, false),
        place(j, 1, 0, 0, 0, false),
        place(s, 1, 0, -1, 1, false),
        place(s, 2, 0, 0, 0, false),
        place(s, 3, 0, 0, 0, false),
    ]
}

#[test]
fn joiner_merges_both_inlets_into_its_outlet() {
    for level in [3_u8, 2] {
        let floor = 0.015 * f32::from(level);
        let layout = joiner(level);
        for (inlet, at, vel) in [
            (
                "port 0 (-x)",
                Vector::new(-0.02, floor + R, 0.0),
                Vector::X * V0,
            ),
            (
                "port 1 (-z)",
                Vector::new(0.06, floor + R, -0.08),
                Vector::Z * V0,
            ),
        ] {
            eprintln!("blk_join_l{level} from {inlet}:");
            let tr = run(&layout, at, vel, 1_000);
            check_exits(
                &tr,
                &[
                    side("joiner +x", 0, 0.09, 1.0, 0.0, floor),
                    side("run-out", 0, 0.21, 1.0, 0.0, floor),
                ],
                at.y,
            );
        }
    }
}
