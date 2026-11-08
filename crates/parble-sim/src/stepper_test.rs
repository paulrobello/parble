//! Scripted runs of the stepper elevator: real marbles from an L3 straight up one cell layer.

use super::*;
use crate::behaviour::registry;
use crate::marble::{CATS_EYE, MarbleKind, MarbleSpec};
use crate::mechanism::{MechEventKind, SimEvent};
use crate::{MarbleState, Sim};
use parble_core::{CellCoord, IssueKind, Mirror, PieceId, Placement, Track, Yaw4, validate_track};
use rapier3d::prelude::*;

const CELL: f32 = 0.06;

fn place(piece: &str, x: i32, y: i32, z: i32) -> Placement {
    Placement::new(
        PieceId::new(piece).unwrap(),
        CellCoord::new(x, y, z),
        Yaw4::new(0),
        Mirror::Off,
    )
}

/// An L3 straight into the stepper at the origin, and an L3 straight on its exit one layer up.
fn layout() -> Vec<Placement> {
    vec![
        place("blk_straight_l3", -1, 0, 0),
        place(STEPPER, 0, 0, 0),
        place("blk_straight_l3", 2, 1, 0),
    ]
}

fn rig() -> Sim {
    let kit = parble_kit::builtin();
    let reg = registry();
    let mut sim = Sim::empty();
    let layout = layout();
    for p in &layout {
        sim.place_piece(kit, p.clone()).unwrap();
    }
    sim.rebuild_chunks(kit).unwrap();
    for p in &layout {
        if let Some(f) = reg.get(&p.piece) {
            sim.add_mechanism(kit.piece(&p.piece).unwrap(), p, 0, f(&p.config))
                .unwrap();
        }
    }
    sim.set_event_recording(true);
    sim
}

/// A marble rolling along +X on the entry straight at `speed`, `dz` off the centreline.
fn launch(sim: &mut Sim, spec: MarbleSpec, speed: f32, dz: f32) -> RigidBodyHandle {
    let at = Vector::new(-CELL - 0.015, 0.045 + spec.radius_m, dz);
    let (mut body, collider) = spec.build(at);
    body.set_linvel(Vector::X * speed, true);
    body.set_angvel(Vector::new(0.0, 0.0, -speed / spec.radius_m), true);
    let h = sim.world.bodies.insert(body);
    sim.world
        .colliders
        .insert_with_parent(collider, h, &mut sim.world.bodies);
    sim.marbles.push(h);
    h
}

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
    let n = 10;
    let mut out = vec![None; n];
    for t in 0..(n as u32 * 360 + 2_400) {
        if t % 360 == 0 && (t / 360) < n as u32 {
            let kind = MarbleKind::ALL[(t / 360) as usize % MarbleKind::ALL.len()];
            launch(&mut sim, kind.spec(), 0.3, 0.0);
        }
        sim.tick(1);
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
