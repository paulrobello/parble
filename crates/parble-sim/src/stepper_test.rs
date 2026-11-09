//! Scripted runs of the stepper elevator: real marbles from an L3 straight up one cell layer.

use super::*;
use crate::lift_run::{INTERVAL_TICKS, MARBLES, launch, layout, rig, ten_marbles_tick};
use crate::marble::{CATS_EYE, MarbleKind};
use crate::mechanism::{MechEventKind, SimEvent};
use crate::{MarbleState, Sim};
use parble_core::{IssueKind, PieceId, Track, validate_track};
use rapier3d::prelude::*;

const CELL: f32 = 0.06;

fn slats(sim: &Sim) -> i32 {
    sim.mechanisms()[0].part_angles[0]
}

fn stroke() -> i32 {
    parble_kit::builtin()
        .piece(&PieceId::new(STEPPER).unwrap())
        .unwrap()
        .animated_parts[0]
        .limits_mdeg[1]
}

fn signals(events: &[SimEvent], name: &str) -> Vec<(u64, i64)> {
    events
        .iter()
        .filter_map(|e| match e {
            SimEvent::Mechanism(m) => match m.kind {
                MechEventKind::Signal { name: n, value } if n == name => Some((m.tick, value)),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// Exit port plane (x, m) and floor (y, m): the +x face of cell (1, 1, 0) at L3.
const EXIT_X: f32 = 1.5 * CELL;
const EXIT_FLOOR: f32 = CELL + 0.045;

/// Run until the marble is a quarter cell into the exit straight (or `ticks` pass). Returns the
/// tick it got there, every tick's marble state and slat position.
fn climb(sim: &mut Sim, ticks: u32) -> (Option<usize>, Vec<MarbleState>, Vec<i32>) {
    let (mut tr, mut part) = (Vec::new(), Vec::new());
    for t in 0..ticks as usize {
        sim.tick(1);
        let m = sim.marbles()[0];
        part.push(slats(sim));
        let out = m.position[0] > EXIT_X + 0.25 * CELL;
        tr.push(m);
        if out {
            return (Some(t), tr, part);
        }
    }
    (None, tr, part)
}

#[test]
fn a_marble_entering_low_leaves_by_the_high_port() {
    let mut sim = rig();
    launch(&mut sim, CATS_EYE, 0.3, 0.0);
    let (out, tr, part) = climb(&mut sim, 4_800);
    let out = out.expect("the marble reached the exit straight within 20 s");
    let m = &tr[out];
    // On the exit straight's floor, one layer and nothing else up, still rolling out along +X.
    assert!(
        (m.position[1] - (EXIT_FLOOR + CATS_EYE.radius_m)).abs() < 0.003,
        "exit height {:?}",
        m.position
    );
    assert!(m.position[2].abs() < 0.008, "{:?}", m.position);
    assert!(m.linvel[0] > 0.0, "{:?}", m.linvel);
    // Four moving slats: four full strokes carried it up.
    let lifts = part
        .windows(2)
        .filter(|w| w[0] < stroke() && w[1] == stroke())
        .count();
    assert_eq!(lifts, 4, "{lifts} full strokes");
    // The slats go back to rest once the marble has left and stay there.
    let mut ev = sim.drain_events();
    sim.tick(DOWN_TICKS + EMPTY_TICKS as u32 + 240);
    ev.extend(sim.drain_events());
    assert_eq!(slats(&sim), 0);
    assert_eq!(signals(&ev, "start").len(), 1);
    assert_eq!(signals(&ev, "reset").len(), 1);
}

#[test]
fn every_standard_marble_climbs_at_any_entry_speed() {
    // 20 runs: every standard kind, entry speeds 0.10-0.52 m/s, up to 3 mm off the centreline.
    // (From about 0.58 m/s a steel marble bounces off the first slat back out of the entry.)
    let mut done = Vec::new();
    for i in 0..20 {
        let mut sim = rig();
        let kind = MarbleKind::ALL[i % MarbleKind::ALL.len()];
        let speed = 0.10 + 0.022 * i as f32;
        let dz = [-0.003, 0.0, 0.003][i % 3];
        launch(&mut sim, kind.spec(), speed, dz);
        let (out, tr, _) = climb(&mut sim, 4_800);
        let y = tr.last().unwrap().position[1];
        assert!(
            out.is_some() && y > EXIT_FLOOR,
            "run {i} ({}, {speed} m/s, dz {dz}): stalled at {:?}",
            kind.name(),
            tr.last().unwrap().position
        );
        done.push(out.unwrap());
    }
    assert_eq!(done.len(), 20, "{done:?}");
}

#[test]
fn ten_marbles_in_a_row_all_reach_the_high_port() {
    // One marble every 1.5 s: later marbles arrive while earlier ones are still climbing, so the
    // lift carries several at once, one per moving slat.
    let mut sim = rig();
    let mut out = vec![None; MARBLES];
    for t in 0..(MARBLES as u32 * INTERVAL_TICKS + 2_400) {
        ten_marbles_tick(&mut sim, t);
        for (i, m) in sim.marbles().iter().enumerate() {
            if out[i].is_none()
                && m.position[0] > EXIT_X + 0.25 * CELL
                && m.position[1] > EXIT_FLOOR
            {
                out[i] = Some(t);
            }
        }
    }
    let fin: Vec<_> = sim
        .marbles()
        .iter()
        .map(|m| m.position.map(|v| (v * 1000.0).round()))
        .collect();
    assert!(
        out.iter().all(Option::is_some),
        "exit ticks {out:?} at {fin:?}"
    );
    sim.tick(DOWN_TICKS + EMPTY_TICKS as u32);
    assert_eq!(slats(&sim), 0);
}

#[test]
fn the_slats_idle_while_the_lift_is_empty() {
    let mut sim = rig();
    let mut ev = Vec::new();
    for _ in 0..2_400 {
        sim.tick(1);
        assert_eq!(slats(&sim), 0, "tick {}", sim.tick_count());
        ev.extend(sim.drain_events());
    }
    assert!(signals(&ev, "start").is_empty() && signals(&ev, "cycle").is_empty());
}

#[test]
fn the_slats_reset_smoothly_when_the_marble_is_moved_back_off_the_lift() {
    let mut sim = rig();
    let h = launch(&mut sim, CATS_EYE, 0.3, 0.0);
    // Run into the first lift, until the slats are half-way up.
    let half = stroke() / 2;
    let mut t = 0;
    while slats(&sim) < half {
        sim.tick(1);
        t += 1;
        assert!(t < 1_200, "the slats never started");
    }
    let lifted = slats(&sim);
    // Move the marble back to the entry straight, at rest.
    let b = &mut sim.world.bodies[h];
    b.set_translation(Vector::new(-CELL, 0.045 + CATS_EYE.radius_m, 0.0), true);
    b.set_linvel(Vector::ZERO, true);
    b.set_angvel(Vector::ZERO, true);
    sim.drain_events();
    let mut part = vec![lifted];
    for _ in 0..(UP_TICKS + DOWN_TICKS + EMPTY_TICKS as u32 + 480) {
        sim.tick(1);
        part.push(slats(&sim));
    }
    let ev = sim.drain_events();
    assert_eq!(*part.last().unwrap(), 0, "the slats are back at rest");
    let reset = signals(&ev, "reset");
    assert_eq!(reset.len(), 1, "{reset:?}");
    // After the grace ticks the slats only go down, by at most a smoothstep's peak speed
    // (1.5 x the mean) per tick, and once at rest stay there.
    let at = (reset[0].0 - (sim.tick_count() - part.len() as u64 + 1)) as usize;
    let max_step = stroke() * 3 / (2 * DOWN_TICKS as i32) + stroke() / 16;
    for w in part[at..].windows(2) {
        assert!(w[1] <= w[0], "slats rose during the reset: {w:?}");
        assert!(
            w[0] - w[1] <= max_step,
            "a jolt of {} µm in one tick",
            w[0] - w[1]
        );
    }
    let rest = part.iter().position(|&p| p == 0).unwrap();
    assert!(part[rest..].iter().all(|&p| p == 0));
    assert!(
        signals(&ev, "start").is_empty(),
        "the moved marble restarted the lift"
    );
}

#[test]
fn the_validator_sees_real_port_links_at_both_ends() {
    let mut track = Track::new(0);
    track.placements = layout();
    let issues = validate_track(&track, parble_kit::builtin());
    let lift = 1;
    for i in &issues {
        let on_lift = i.placement == Some(lift)
            || matches!(i.kind, IssueKind::BrokenLink { other, .. } if other == lift);
        assert!(
            !(on_lift
                && matches!(
                    i.kind,
                    IssueKind::OpenPort { .. } | IssueKind::BrokenLink { .. }
                )),
            "{i:?}"
        );
    }
    // Without the upper straight the exit port is open: the link above is a real one.
    track.placements.pop();
    let issues = validate_track(&track, parble_kit::builtin());
    assert!(issues.iter().any(
        |i| i.placement == Some(lift) && matches!(i.kind, IssueKind::OpenPort { port: 1, .. })
    ));
}

#[test]
fn the_cycle_eases_through_integer_keys() {
    let s = stroke();
    let c = cycle(0, s, BOTTOM_HOLD_TICKS);
    assert_eq!(
        c.duration(),
        BOTTOM_HOLD_TICKS + UP_TICKS + TOP_HOLD_TICKS + DOWN_TICKS
    );
    assert_eq!(c.sample(0), 0);
    assert_eq!(c.sample(u64::from(BOTTOM_HOLD_TICKS + UP_TICKS)), s);
    assert_eq!(c.sample(u64::from(c.duration())), 0);
    // From part-way up it first comes down, at the down stroke's pace.
    let c = cycle(s / 2, s, ENTRY_HOLD_TICKS);
    assert_eq!(c.sample(0), s / 2);
    assert_eq!(c.sample(u64::from(DOWN_TICKS / 2)), 0);
    let r = reset(s, s);
    assert_eq!(r.duration(), DOWN_TICKS);
    assert_eq!(reset(0, s).sample(5), 0);
}

#[test]
fn the_stepper_restores_what_it_saved() {
    let mut sim = rig();
    launch(&mut sim, CATS_EYE, 0.3, 0.0);
    sim.tick(400);
    let v = &sim.mechanisms()[0];
    let mut b = Stepper::factory(&MechConfig::new());
    b.restore(&v.state);
    for (k, val) in &b.save() {
        assert_eq!(v.state.get(k), Some(val), "{k}");
    }
}

// --- hand-over checks: one step per lift cycle, no stall on a top, no roll-back ---

/// `blender/kit/kitstepper.py` `STEP`: at the top of the stroke a moving slat's low edge sits this
/// far above the next fixed slat's high edge, so it is level with it `STEP` before the top.
const STEP_UM: i32 = 1_000;

/// A straight at the stepper's entry level into `piece` at the origin, an L3 straight on its exit
/// one layer up. `part` replaces the piece's kit definition for its moving part (the flat control).
fn hand_over_rig(piece: &str, entry: &str, part: Option<&parble_kit::KitPiece>) -> Sim {
    let kit = parble_kit::builtin();
    let id = |s: &str| PieceId::new(s).unwrap();
    let at = |s: &str, x, y| {
        parble_core::Placement::new(
            id(s),
            parble_core::CellCoord::new(x, y, 0),
            parble_core::Yaw4::new(0),
            parble_core::Mirror::Off,
        )
    };
    let layout = [
        at(entry, -1, 0),
        at(piece, 0, 0),
        at("blk_straight_l3", 2, 1),
    ];
    let mut sim = Sim::empty();
    for p in &layout {
        sim.place_piece(kit, p.clone()).unwrap();
    }
    sim.rebuild_chunks(kit).unwrap();
    let def = part.unwrap_or_else(|| kit.piece(&id(piece)).unwrap());
    sim.add_mechanism(def, &layout[1], 0, Stepper::factory(&MechConfig::new()))
        .unwrap();
    sim
}

/// A marble rolling along +X at `speed` on the entry straight, whose floor is `floor` m.
fn launch_at(sim: &mut Sim, floor: f32, speed: f32) {
    let spec = CATS_EYE;
    let (mut body, collider) = spec.build(Vector::new(-CELL - 0.015, floor + spec.radius_m, 0.0));
    body.set_linvel(Vector::X * speed, true);
    body.set_angvel(Vector::new(0.0, 0.0, -speed / spec.radius_m), true);
    let h = sim.world.bodies.insert(body);
    sim.world
        .colliders
        .insert_with_parent(collider, h, &mut sim.world.bodies);
    sim.marbles.push(h);
}

/// The moving slats' x extents (m), in order along +x, from the part collision (one 8-vertex prism
/// per slat, `kitstepper.parts`).
fn moving_columns(piece: &parble_kit::KitPiece) -> Vec<(f32, f32)> {
    let v = &piece.animated_parts[0].collision[0].vertices_um;
    v.chunks(8)
        .map(|p| (p[0][0] as f32 * 1e-6, p[1][0] as f32 * 1e-6))
        .collect()
}

/// What one run of a marble up a stepper did.
#[derive(Debug)]
struct HandOver {
    /// Per lift cycle: ticks from the moving slat coming level with the step after it to the
    /// marble's centre crossing the moving slat's exit edge (`None`: it never crossed that cycle).
    stall: Vec<Option<u32>>,
    /// Per lift cycle: the moving slat the marble was on when its slat came level.
    on_slat: Vec<Option<usize>>,
    /// Largest backward excursion of the marble's x below its running maximum, m, from the first
    /// level moment to the exit.
    roll_back: f32,
    /// Tick the marble reached the exit straight.
    out: Option<u32>,
}

fn hand_over(
    piece: &str,
    entry: &str,
    floor: f32,
    part: Option<&parble_kit::KitPiece>,
) -> HandOver {
    let kit = parble_kit::builtin();
    let def = part.unwrap_or_else(|| kit.piece(&PieceId::new(piece).unwrap()).unwrap());
    let cols = moving_columns(def);
    let stroke = def.animated_parts[0].limits_mdeg[1];
    let level = stroke - STEP_UM;
    let mut sim = hand_over_rig(piece, entry, part);
    launch_at(&mut sim, floor, 0.3);
    let (mut stall, mut on_slat) = (Vec::new(), Vec::new());
    let mut pending: Option<(u32, usize)> = None;
    let mut prev = 0;
    let mut max_x: Option<f32> = None;
    let mut roll_back = 0.0f32;
    let mut out = None;
    for t in 0..6_000u32 {
        sim.tick(1);
        let part_at = slats(&sim);
        let x = sim.marbles()[0].position[0];
        let slat = cols
            .iter()
            .position(|&(a, b)| x >= a - 0.0005 && x <= b + 0.0005);
        if prev < level && part_at >= level {
            if pending.take().is_some() {
                stall.push(None);
            }
            on_slat.push(slat);
            if let Some(j) = slat {
                pending = Some((t, j));
                max_x.get_or_insert(x);
            } else {
                stall.push(None);
            }
        }
        if let Some((t0, j)) = pending
            && x > cols[j].1 + 0.0005
        {
            stall.push(Some(t - t0));
            pending = None;
        }
        if let Some(m) = max_x.as_mut() {
            roll_back = roll_back.max(*m - x);
            *m = m.max(x);
        }
        prev = part_at;
        if x > EXIT_X + 0.25 * CELL {
            out = Some(t);
            break;
        }
    }
    if pending.is_some() {
        stall.push(None);
    }
    HandOver {
        stall,
        on_slat,
        roll_back,
        out,
    }
}

/// Most ticks a marble may sit on a moving slat after it comes level with the step after it. From
/// rest against the riser ahead its centre has ~7.5 mm to roll to the exit edge: ~26 ticks at
/// 11.06° with Coulomb rolling resistance; the slat stays level for ~62 ticks.
const MAX_STALL_TICKS: u32 = 30;
/// Most the marble may move back toward the entry, m, once on the first slat: the bounce off each
/// riser it rolls against, well short of the 12.8 mm slat it came from.
const ROLL_BACK_M: f32 = 0.0025;
/// Moving slats in each stepper (`kitstepper.STEPPERS`).
const MOVING_SLATS: usize = 4;

/// The stepper pieces with their entry straight and its floor, m.
const STEPPERS: [(&str, &str, f32); 2] = [
    (STEPPER, "blk_straight_l3", 0.045),
    (crate::stepper::STEPPER_L1, "blk_straight_l1", 0.015),
];

/// `Err` naming the first hand-over check `h` fails.
fn check(h: &HandOver) -> Result<(), String> {
    let stalled = h
        .stall
        .iter()
        .position(|s| s.is_none_or(|t| t > MAX_STALL_TICKS));
    if let Some(c) = stalled {
        return Err(format!("stalled on cycle {c}: {h:?}"));
    }
    let up: Vec<_> = (0..MOVING_SLATS).map(Some).collect();
    if h.on_slat != up || h.out.is_none() {
        return Err(format!("not one slat per cycle to the exit: {h:?}"));
    }
    if h.roll_back > ROLL_BACK_M {
        return Err(format!("rolled back {} m: {h:?}", h.roll_back));
    }
    Ok(())
}

#[test]
fn the_slanted_tops_hand_the_marble_on_one_step_per_lift() {
    for (piece, entry, floor) in STEPPERS {
        let h = hand_over(piece, entry, floor, None);
        eprintln!("{piece}: {h:?}");
        check(&h).unwrap_or_else(|e| panic!("{piece}: {e}"));
    }
}

#[test]
fn flat_slat_tops_stall_the_marble() {
    // Control: the same pieces with every moving slat's top levelled at its exit edge (0°), so the
    // hand-over height is unchanged and only the slant is gone.
    for (piece, entry, floor) in STEPPERS {
        let mut flat = parble_kit::builtin()
            .piece(&PieceId::new(piece).unwrap())
            .unwrap()
            .clone();
        for g in &mut flat.animated_parts[0].collision {
            for prism in g.vertices_um.chunks_mut(8) {
                prism[3][1] = prism[2][1];
                prism[7][1] = prism[6][1];
            }
        }
        let h = hand_over(piece, entry, floor, Some(&flat));
        assert!(
            h.stall
                .first()
                .is_some_and(|s| s.is_none_or(|t| t > MAX_STALL_TICKS)),
            "{piece}: a flat top did not stall the marble: {h:?}"
        );
        assert!(check(&h).is_err() && h.out.is_none(), "{piece}: {h:?}");
    }
}

#[test]
fn the_sidecar_slant_is_the_slats_collider_slant() {
    for (piece, _, _) in STEPPERS {
        let p = parble_kit::builtin()
            .piece(&PieceId::new(piece).unwrap())
            .unwrap();
        let part = &p.animated_parts[0];
        let slant = part
            .slant_mdeg
            .expect("the stepper sidecar records slant_mdeg");
        for prism in part.collision[0].vertices_um.chunks(8) {
            // (x1, low top) and (x0, high top): the top falls toward +x.
            let (lo, hi) = (prism[2], prism[3]);
            assert!(lo[0] > hi[0] && lo[1] < hi[1], "{piece}: {prism:?}");
            let mdeg = f64::from(hi[1] - lo[1])
                .atan2(f64::from(lo[0] - hi[0]))
                .to_degrees()
                * 1000.0;
            assert!(
                (mdeg - f64::from(slant)).abs() < 20.0,
                "{piece}: {mdeg} vs {slant}"
            );
        }
    }
}
