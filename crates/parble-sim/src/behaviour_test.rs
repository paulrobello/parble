//! P2-T5 scripted runs: real marbles through each kit mechanism, asserting where they go.

use super::*;
use crate::marble::{CATS_EYE, MarbleKind, MarbleSpec};
use crate::mechanism::{MechEventKind, SimEvent, stream_key};
use crate::track_run::TrackRun;
use crate::{MarbleState, Sim};
use parble_core::{CellCoord, HopperConfig, MarbleSize, Mirror, Placement, SeededRng, Track, Yaw4};
use rapier3d::prelude::*;

const R: f32 = 0.008;
const CELL: f32 = 0.06;

fn place(piece: &str, x: i32, y: i32, z: i32, yaw: u8) -> Placement {
    Placement::new(
        PieceId::new(piece).unwrap(),
        CellCoord::new(x, y, z),
        Yaw4::new(yaw),
        Mirror::Off,
    )
}

/// `layout` as chunk colliders plus a kit behaviour for every mechanism placement, events on.
fn rig(layout: &[Placement], seed: u64) -> Sim {
    let kit = parble_kit::builtin();
    let reg = registry();
    let mut sim = Sim::empty();
    for p in layout {
        sim.place_piece(kit, p.clone()).unwrap();
    }
    sim.rebuild_chunks(kit).unwrap();
    for p in layout {
        let Some(factory) = reg.get(&p.piece) else {
            continue;
        };
        sim.add_mechanism(kit.piece(&p.piece).unwrap(), p, seed, factory(&p.config))
            .unwrap();
    }
    sim.set_event_recording(true);
    sim
}

/// A cat's eye marble rolling at `vel` from `at`.
fn launch(sim: &mut Sim, at: Vector, vel: Vector) {
    launch_spec(sim, &CATS_EYE, at, vel);
}

/// A `spec` marble rolling at `vel` from `at`.
fn launch_spec(sim: &mut Sim, spec: &MarbleSpec, at: Vector, vel: Vector) {
    let (mut body, collider) = spec.build(at);
    let speed = vel.length();
    body.set_linvel(vel, true);
    body.set_angvel(Vector::Y.cross(vel / speed) * (speed / spec.radius_m), true);
    let h = sim.world.bodies.insert(body);
    sim.world
        .colliders
        .insert_with_parent(collider, h, &mut sim.world.bodies);
    sim.marbles.push(h);
}

/// Rest centre height on floor `level` of layer 0.
fn rest(level: i32) -> f32 {
    0.015 * level as f32 + R
}

/// Every marble's state after each tick, plus the events.
fn run(sim: &mut Sim, ticks: u32, tr: &mut Vec<Vec<MarbleState>>, ev: &mut Vec<SimEvent>) {
    for _ in 0..ticks {
        sim.tick(1);
        tr.push(sim.marbles());
    }
    ev.extend(sim.drain_events());
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

fn sensor_events(events: &[SimEvent], enter: bool, sensor: usize) -> Vec<(u64, usize)> {
    events
        .iter()
        .filter_map(|e| match e {
            SimEvent::Mechanism(m) => match m.kind {
                MechEventKind::SensorEnter { sensor: s, marble } if enter && s == sensor => {
                    Some((m.tick, marble))
                }
                MechEventKind::SensorExit { sensor: s, marble } if !enter && s == sensor => {
                    Some((m.tick, marble))
                }
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// First tick index at which marble `m` satisfies `f`.
fn first(tr: &[Vec<MarbleState>], m: usize, f: impl Fn(&MarbleState) -> bool) -> Option<usize> {
    tr.iter().position(|s| s.get(m).is_some_and(&f))
}

fn state(sim: &Sim) -> MechConfig {
    sim.mechanisms()[0].state.clone()
}

// ---------------------------------------------------------------------------------------------
// Forks (toggle, splitter)
// ---------------------------------------------------------------------------------------------

/// Which branch marble `m` left the fork at cell `(1, 0, 0)` by: 0 for +Z, 1 for −Z.
fn fork_side(tr: &[Vec<MarbleState>], m: usize) -> Option<Side> {
    let i = first(tr, m, |s| libm::fabsf(s.position[2]) > CELL / 2.0)?;
    let s = tr[i][m];
    let x = s.position[0];
    assert!(
        (CELL / 2.0..1.5 * CELL).contains(&x),
        "marble {m} left the fork sideways at x = {x}"
    );
    Some(if s.position[2] > 0.0 { 0 } else { 1 })
}

/// `n` marbles fed one at a time from an L3 straight into the fork `piece` at cell (1, 0, 0),
/// 240 ticks apart. Returns each marble's branch and the events.
fn feed_fork(piece: &str, seed: u64, n: usize) -> (Vec<Option<Side>>, Vec<SimEvent>) {
    feed_fork_with(piece, &[], seed, n)
}

/// [`feed_fork`] with `config` set on the fork's placement.
fn feed_fork_with(
    piece: &str,
    config: &[(&str, i64)],
    seed: u64,
    n: usize,
) -> (Vec<Option<Side>>, Vec<SimEvent>) {
    let mut fork = place(piece, 1, 0, 0, 0);
    for &(k, v) in config {
        fork.config.insert(k.into(), v);
    }
    let layout = [place("blk_straight_l3", 0, 0, 0, 0), fork];
    let mut sim = rig(&layout, seed);
    let (mut tr, mut ev) = (Vec::new(), Vec::new());
    for _ in 0..n {
        launch(&mut sim, Vector::new(-0.02, rest(3), 0.0), Vector::X * 0.6);
        run(&mut sim, 240, &mut tr, &mut ev);
    }
    ((0..n).map(|m| fork_side(&tr, m)).collect(), ev)
}

#[test]
fn shipped_toggle_alternates_six_marbles() {
    let (sides, ev) = feed_fork(TOGGLE, 1, 6);
    // Side 0 (+Z) is "L" for a marble travelling +X.
    assert_eq!(sides, [0, 1, 0, 1, 0, 1].map(Some));
    let passes: Vec<i64> = signals(&ev, "pass").into_iter().map(|p| p.1).collect();
    assert_eq!(passes, [0, 1, 0, 1, 0, 1]);
}

#[test]
fn toggle_first_side_follows_config() {
    let layout = {
        let mut t = place(TOGGLE, 1, 0, 0, 0);
        t.config.insert("first".into(), 1);
        [place("blk_straight_l3", 0, 0, 0, 0), t]
    };
    let mut sim = rig(&layout, 1);
    let (mut tr, mut ev) = (Vec::new(), Vec::new());
    for _ in 0..2 {
        launch(&mut sim, Vector::new(-0.02, rest(3), 0.0), Vector::X * 0.6);
        run(&mut sim, 240, &mut tr, &mut ev);
    }
    assert_eq!([fork_side(&tr, 0), fork_side(&tr, 1)], [Some(1), Some(0)]);
}

/// The splitter's seeded sequence, drawn independently of the behaviour: the `choice` stream of
/// the instance at (1, 0, 0), one `below(2)` per marble (for `n = 2` every `u32` is accepted, so a
/// draw is `next_u32() % 2`).
fn expected_splits(seed: u64, n: usize) -> Vec<Option<Side>> {
    let key = stream_key(
        &PieceId::new(SPLITTER).unwrap(),
        CellCoord::new(1, 0, 0),
        SPLITTER_STREAM,
    );
    let mut s = SeededRng::new(seed).stream(&key);
    (0..n).map(|_| Some(i64::from(s.next_u32() % 2))).collect()
}

#[test]
fn shipped_splitter_matches_its_seeded_sequence() {
    let mut both = [false; 2];
    for seed in [3, 11] {
        let want = expected_splits(seed, 8);
        let (sides, ev) = feed_fork(SPLITTER, seed, 8);
        assert_eq!(sides, want, "seed {seed}");
        for s in sides.iter().flatten() {
            both[*s as usize] = true;
        }
        let set: Vec<i64> = signals(&ev, "set").into_iter().map(|p| p.1).collect();
        // One `set` before the first marble, then one per pass (the next marble's side).
        assert_eq!(set.len(), 9);
        assert_eq!(set[..8].iter().map(|&s| Some(s)).collect::<Vec<_>>(), want);
    }
    assert!(both[0] && both[1], "the sequences never used both branches");
}

/// An explicit Random mode is the default: the same seeded sides, events and saved state as a
/// placement without the key.
#[test]
fn splitter_random_mode_matches_the_seeded_sequence() {
    for seed in [3, 11] {
        let (sides, ev) = feed_fork_with(SPLITTER, &[(SPLITTER_MODE, SPLITTER_RANDOM)], seed, 8);
        assert_eq!(sides, expected_splits(seed, 8), "seed {seed}");
        assert_eq!(ev, feed_fork(SPLITTER, seed, 8).1, "seed {seed}");
    }
}

/// Alternate mode sends marbles left and right in turn from the chosen first side, whatever the
/// seed, and never draws from the RNG.
#[test]
fn splitter_alternate_mode_alternates_from_the_first_side() {
    for first in [0, 1] {
        let want: Vec<Option<Side>> = (0..6).map(|i| Some((first + i) % 2)).collect();
        for seed in [3, 11] {
            let config = [(SPLITTER_MODE, SPLITTER_ALTERNATE), ("first", first)];
            let (sides, ev) = feed_fork_with(SPLITTER, &config, seed, 6);
            assert_eq!(sides, want, "first {first}, seed {seed}");
            let passes: Vec<Option<Side>> = signals(&ev, "pass")
                .into_iter()
                .map(|p| Some(p.1))
                .collect();
            assert_eq!(passes, want, "first {first}, seed {seed}");
        }
    }
    let layout = {
        let mut s = place(SPLITTER, 1, 0, 0, 0);
        s.config.insert(SPLITTER_MODE.into(), SPLITTER_ALTERNATE);
        [place("blk_straight_l3", 0, 0, 0, 0), s]
    };
    let mut sim = rig(&layout, 3);
    let (mut tr, mut ev) = (Vec::new(), Vec::new());
    launch(&mut sim, Vector::new(-0.02, rest(3), 0.0), Vector::X * 0.6);
    run(&mut sim, 240, &mut tr, &mut ev);
    assert_eq!(fork_side(&tr, 0), Some(0), "first side defaults to 0");
    assert!(
        !state(&sim).keys().any(|k| k.starts_with("rng/")),
        "Alternate mode drew from the RNG"
    );
}

/// The shipped fork parts steer the marble: it touches the part and leaves by a branch, never back
/// out of the -X port (the P2-T3 parts it replaced turned every marble back).
#[test]
fn shipped_fork_parts_steer_the_marble_into_a_branch() {
    for piece in [TOGGLE, SPLITTER] {
        let (sides, ev) = feed_fork(piece, 1, 1);
        assert!(
            sides[0].is_some(),
            "{piece}: the marble never left by a branch"
        );
        assert!(
            ev.iter().any(|e| matches!(e, SimEvent::Contact(c) if matches!(c.other, crate::mechanism::ContactOther::Part { .. }))),
            "{piece}: the marble never touched the part"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// One-way pieces
// ---------------------------------------------------------------------------------------------

/// L1 straight, `piece` at cell (1, 0, 0), L1 straight; one marble from `dir` (+1: from −X).
fn one_way(piece: &str, dir: f32) -> (Sim, Vec<Vec<MarbleState>>, Vec<SimEvent>) {
    let (sim, tr, _, ev) = one_way_at(piece, dir, 0.3);
    (sim, tr, ev)
}

/// [`one_way`] at launch speed `speed` (m/s), also returning the part angle after every tick.
fn one_way_at(
    piece: &str,
    dir: f32,
    speed: f32,
) -> (Sim, Vec<Vec<MarbleState>>, Vec<i32>, Vec<SimEvent>) {
    one_way_for(piece, dir, speed, 600)
}

/// [`one_way_at`] for `ticks` ticks.
fn one_way_for(
    piece: &str,
    dir: f32,
    speed: f32,
    ticks: usize,
) -> (Sim, Vec<Vec<MarbleState>>, Vec<i32>, Vec<SimEvent>) {
    let layout = [
        place("blk_straight_l1", 0, 0, 0, 0),
        place(piece, 1, 0, 0, 0),
        place("blk_straight_l1", 2, 0, 0, 0),
    ];
    let mut sim = rig(&layout, 1);
    let x0 = if dir > 0.0 { -0.02 } else { 0.14 };
    launch(
        &mut sim,
        Vector::new(x0, rest(1), 0.0),
        Vector::X * speed * dir,
    );
    let (mut tr, mut angles) = (Vec::new(), Vec::new());
    for _ in 0..ticks {
        sim.tick(1);
        tr.push(sim.marbles());
        angles.push(sim.mechanisms()[0].part_angles[0]);
    }
    let ev = sim.drain_events();
    (sim, tr, angles, ev)
}

/// The gate flap's farthest +X reach in world X with the flap swung `mdeg` open about its pivot,
/// from its collider (piece frame, centred on the cell), and its open limit in mdeg.
fn flap_reach_and_limit(mdeg: i32) -> (f32, i32) {
    let kit = parble_kit::builtin();
    let flap = &kit
        .piece(&PieceId::new(GATE).unwrap())
        .unwrap()
        .animated_parts[0];
    let (sin, cos) = (mdeg as f32 * 1e-3).to_radians().sin_cos();
    let py = flap.pivot_um[1];
    let reach = CELL
        + flap
            .collision
            .iter()
            .flat_map(|c| c.vertices_um.iter())
            .map(|v| (v[0] as f32 * cos - (v[1] - py) as f32 * sin) * 1e-6)
            .fold(f32::MIN, f32::max);
    (reach, flap.limits_mdeg[1])
}

/// Launch speeds of the gate sweeps, m/s: from the slowest marble that gets through to a fast drop.
const GATE_SPEEDS: [f32; 6] = [GATE_MIN_LAUNCH_SPEED_M_S, 0.1, 0.2, 0.3, 0.6, 1.0];

/// Forward gate runs last this long: at [`GATE_MIN_LAUNCH_SPEED_M_S`] the flap falls shut behind
/// the marble at tick 764, and the run then checks 120 ticks of the flap at rest.
const GATE_FORWARD_TICKS: usize = 1_200;

#[test]
fn one_way_gate_flap_is_pushed_open_by_a_forward_marble() {
    let (far_edge, hi) = flap_reach_and_limit(0);
    let open_edge = flap_reach_and_limit(GATE_OPEN_MDEG).0;
    let beyond = |s: &MarbleState, edge: f32| s.position[0] - R > edge;
    for speed in GATE_SPEEDS {
        let (sim, tr, angles, ev) = one_way_for(GATE, 1.0, speed, GATE_FORWARD_TICKS);
        let past = first(&tr, 0, |s| beyond(s, far_edge))
            .unwrap_or_else(|| panic!("{speed} m/s: the forward marble never passed"));
        assert!(tr[past][0].position[1] > 0.015, "{speed} m/s: marble fell");
        let end = tr.last().unwrap()[0];
        assert!(
            beyond(&end, far_edge),
            "{speed} m/s: marble ended at x = {} before the flap's far edge {far_edge}",
            end.position[0]
        );
        // The flap rests shut until the marble touches it, then swings open under the push.
        let touched = first(&tr, 0, |s| s.position[0] > CELL - 0.0015 - R - 0.001).unwrap();
        assert!(
            angles[..touched].iter().all(|&a| a == 0),
            "{speed} m/s: flap moved early"
        );
        let peak = *angles.iter().max().unwrap();
        assert!(
            (25_000..=hi + 1_000).contains(&peak),
            "{speed} m/s: flap peaked at {peak} mdeg"
        );
        // One open and one close, and the flap falls back onto its stop without chatter.
        let open = signals(&ev, "open");
        let close = signals(&ev, "close");
        assert_eq!(
            (open.len(), close.len()),
            (1, 1),
            "{speed} m/s: {open:?} {close:?}"
        );
        // It opens under the push, before the marble is clear of the flap swung to that angle.
        let cleared = first(&tr, 0, |s| beyond(s, open_edge)).unwrap_or(tr.len());
        assert!(
            (open[0].0 as usize) < cleared,
            "{speed} m/s: opened after the marble cleared the flap"
        );
        assert_eq!(signals(&ev, "forward"), [(signals(&ev, "forward")[0].0, 0)]);
        assert!(signals(&ev, "reverse").is_empty());
        assert!(
            angles.iter().all(|&a| a > -1_500),
            "{speed} m/s: flap swung back through its stop"
        );
        let settled = &angles[angles.len() - 120..];
        assert!(
            settled.iter().all(|&a| a == 0),
            "{speed} m/s: flap not at rest: {settled:?}"
        );
        let s = state(&sim);
        assert_eq!((s["passes"], s["blocked"], s["open"]), (1, 0, 0));
        assert_eq!(sim.mechanisms()[0].part_angles, [0]);
    }
}

#[test]
fn one_way_gate_flap_stops_a_reverse_marble() {
    for speed in GATE_SPEEDS {
        let (sim, tr, angles, ev) = one_way_at(GATE, -1.0, speed);
        // The shut flap (3 mm plate at the cell centre) stops the marble one radius short of it.
        let closest = tr.iter().map(|s| s[0].position[0]).fold(f32::MAX, f32::min);
        assert!(
            closest > CELL + 0.0015 + R - 0.001,
            "{speed} m/s: reverse marble reached x = {closest}"
        );
        assert!(
            angles.iter().all(|&a| (-500..GATE_OPEN_MDEG).contains(&a)),
            "{speed} m/s: flap moved off its stop: {:?}",
            (angles.iter().min(), angles.iter().max())
        );
        assert!(signals(&ev, "open").is_empty(), "{speed} m/s");
        assert_eq!(signals(&ev, "reverse").len(), 1);
        assert_eq!(state(&sim)["blocked"], 1);
        // Turned back: it rolls away from the flap. Rolling resistance (14 mm/s² on the level)
        // stops the slow rebounds inside the window (0.06 m/s leaves at 22 mm/s and rests by
        // tick 532), so the check is the distance it rolled back, not its final speed.
        let end = tr.last().unwrap()[0].position[0];
        assert!(
            end > closest + 0.002,
            "{speed} m/s: reverse marble did not turn back (closest {closest}, end {end})"
        );
    }
}

#[test]
fn one_way_gate_flap_angle_is_its_body_rotation() {
    // Mirrored and yawed placements report the same opening angle for the same push.
    let mut peaks = Vec::new();
    for (yaw, mirror) in [(0, Mirror::Off), (0, Mirror::On), (2, Mirror::Off)] {
        let mut gate = place(GATE, 1, 0, 0, yaw);
        gate.mirror = mirror;
        let layout = [
            place("blk_straight_l1", 0, 0, 0, 0),
            gate,
            place("blk_straight_l1", 2, 0, 0, 0),
        ];
        let mut sim = rig(&layout, 1);
        // A mirror (x → −x) or a half turn puts the in port on +X.
        let dir = if (yaw == 2) != (mirror == Mirror::On) {
            -1.0
        } else {
            1.0
        };
        let x0 = if dir > 0.0 { -0.02 } else { 0.14 };
        launch(
            &mut sim,
            Vector::new(x0, rest(1), 0.0),
            Vector::X * 0.3 * dir,
        );
        let mut peak = 0;
        for _ in 0..300 {
            sim.tick(1);
            peak = peak.max(sim.mechanisms()[0].part_angles[0]);
        }
        peaks.push(peak);
    }
    assert!(peaks.iter().all(|&p| p > 45_000), "{peaks:?}");
}

#[test]
fn one_way_hole_drops_from_the_trap_side_and_carries_from_the_other() {
    let (sim, tr, ev) = one_way(HOLE, 1.0);
    // Through the bore: below the cell floor, inside the 16 mm bore radius of the cell centre.
    let fell = first(&tr, 0, |s| s.position[1] < 0.0).expect("trap-side marble did not drop");
    let p = tr[fell][0].position;
    assert!(
        libm::fabsf(p[0] - CELL) < 0.008 && libm::fabsf(p[2]) < 0.008,
        "dropped at {p:?}"
    );
    assert!(
        tr[..fell].iter().all(|s| s[0].position[0] < 1.5 * CELL),
        "trap-side marble crossed the hole"
    );
    let open = signals(&ev, "open");
    assert_eq!(open.len(), 1);
    assert_eq!(signals(&ev, "drop").len(), 1);
    assert_eq!(state(&sim)["drops"], 1);
    assert_eq!(signals(&ev, "close").len(), 1);

    let (sim, tr, ev) = one_way(HOLE, -1.0);
    let out = first(&tr, 0, |s| s.position[0] < CELL / 2.0).expect("over-side marble stuck");
    let lowest = tr[..out]
        .iter()
        .map(|s| s[0].position[1])
        .fold(f32::MAX, f32::min);
    assert!(
        lowest > rest(1) - 0.002,
        "over-side marble dipped to y = {lowest}"
    );
    assert!(signals(&ev, "open").is_empty());
    assert_eq!(signals(&ev, "over").len(), 1);
    assert_eq!(state(&sim)["overs"], 1);
}

/// An over-side marble stopped on the shut door, between the sensors, for well past
/// [`HEADING_FORGET_TICKS`] keeps its heading: rolled on into `trap`, it crosses the shut door
/// and leaves by −X. Forgotten, it read as a fresh trap-side arrival and the door opened under it.
#[test]
fn one_way_hole_keeps_the_heading_of_a_marble_parked_on_the_door() {
    let layout = [
        place("blk_straight_l1", 0, 0, 0, 0),
        place(HOLE, 1, 0, 0, 0),
        place("blk_straight_l1", 2, 0, 0, 0),
    ];
    let mut sim = rig(&layout, 1);
    launch(&mut sim, Vector::new(0.14, rest(1), 0.0), Vector::X * -0.3);
    let (mut tr, mut ev) = (Vec::new(), Vec::new());
    // Stop it on the door once it has left `over` and before it reaches `trap`.
    let mut parked = false;
    for _ in 0..600 {
        run(&mut sim, 1, &mut tr, &mut ev);
        let x = tr.last().unwrap()[0].position[0];
        if !sensor_events(&ev, false, 1).is_empty() && (x - CELL).abs() < 0.004 {
            parked = true;
            break;
        }
    }
    assert!(parked, "marble never reached the door");
    let body = &mut sim.world.bodies[sim.marbles[0]];
    body.set_linvel(Vector::ZERO, true);
    body.set_angvel(Vector::ZERO, true);
    run(&mut sim, 3 * HEADING_FORGET_TICKS as u32, &mut tr, &mut ev);
    let p = tr.last().unwrap()[0].position;
    assert!(
        (p[0] - CELL).abs() < 0.008 && libm::fabsf(p[1] - rest(1)) < 0.002,
        "marble did not stay on the door: {p:?}"
    );
    let body = &mut sim.world.bodies[sim.marbles[0]];
    body.set_linvel(Vector::X * -0.3, true);
    body.set_angvel(
        Vector::Y.cross(-Vector::X) * (0.3 / CATS_EYE.radius_m),
        true,
    );
    run(&mut sim, 480, &mut tr, &mut ev);

    let left_over = sensor_events(&ev, false, 1)[0].0;
    let reached_trap = sensor_events(&ev, true, 0).first().map(|e| e.0);
    let reached_trap = reached_trap.expect("parked marble never reached trap");
    assert!(
        reached_trap - left_over > HEADING_FORGET_TICKS as u64,
        "parked only {} ticks",
        reached_trap - left_over
    );
    assert!(
        signals(&ev, "open").is_empty(),
        "the trapdoor opened under it"
    );
    assert!(signals(&ev, "drop").is_empty());
    assert_eq!(signals(&ev, "over").len(), 1);
    let s = state(&sim);
    assert_eq!((s["drops"], s["overs"], s["open"]), (0, 1, 0));
    let out = first(&tr, 0, |s| s.position[0] < CELL / 2.0)
        .unwrap_or_else(|| panic!("parked marble stuck at {:?}", tr.last().unwrap()[0]));
    let lowest = tr[..out]
        .iter()
        .map(|s| s[0].position[1])
        .fold(f32::MAX, f32::min);
    assert!(
        lowest > rest(1) - 0.002,
        "parked marble dipped to y = {lowest}"
    );
}

// ---------------------------------------------------------------------------------------------
// Checkpoint and finish
// ---------------------------------------------------------------------------------------------

#[test]
fn checkpoint_reports_split_ticks_and_camera_cues() {
    let layout = [
        place("blk_straight_l1", 0, 0, 0, 0),
        place(CHECKPOINT, 1, 0, 0, 0),
        place("blk_straight_l1", 2, 0, 0, 0),
    ];
    let mut sim = rig(&layout, 1);
    let (mut tr, mut ev) = (Vec::new(), Vec::new());
    let dark = crate::inlay::Inlay::default();
    assert_eq!(crate::inlay::inlays(&state(&sim)), [dark]);
    for _ in 0..2 {
        launch(&mut sim, Vector::new(-0.02, rest(1), 0.0), Vector::X * 0.3);
        run(&mut sim, 200, &mut tr, &mut ev);
    }
    let enters = sensor_events(&ev, true, 0);
    assert_eq!(enters.iter().map(|e| e.1).collect::<Vec<_>>(), [0, 1]);
    let splits = signals(&ev, "split");
    assert_eq!(
        splits,
        enters
            .iter()
            .map(|&(t, _)| (t, t as i64))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        signals(&ev, "camera_cue"),
        enters
            .iter()
            .map(|&(t, m)| (t, m as i64))
            .collect::<Vec<_>>()
    );
    // The sensor half-depth is 4 mm, so a marble enters it 12 mm before its centre reaches the
    // arch: within 60 ticks at 0.3 m/s.
    for &(t, m) in &enters {
        let at = first(&tr, m, |s| s.position[0] > CELL).unwrap() as u64 + 1;
        assert!(t <= at && at - t < 60, "marble {m}: split {t}, arch {at}");
    }
    let s = state(&sim);
    assert_eq!((s["passes"], s["last_marble"]), (2, 1));
    assert_eq!(s["last_split"], enters[1].0 as i64);
    // The inlay lit on the first pass and held through the second.
    let lit = crate::inlay::Inlay {
        on: true,
        since: Some(enters[0].0),
    };
    assert_eq!(crate::inlay::inlays(&s), [lit]);
}

#[test]
fn finish_counts_each_marble_once_with_its_arrival_tick() {
    let layout = [
        place("blk_straight_l3", 0, 0, 0, 0),
        place(FINISH, 1, 0, 0, 0),
    ];
    let mut sim = rig(&layout, 1);
    let (mut tr, mut ev) = (Vec::new(), Vec::new());
    for _ in 0..3 {
        launch(&mut sim, Vector::new(-0.02, rest(3), 0.0), Vector::X * 0.4);
        run(&mut sim, 240, &mut tr, &mut ev);
    }
    run(&mut sim, 480, &mut tr, &mut ev);
    let s = state(&sim);
    assert_eq!(s["count"], 3);
    let arrivals = Finish::arrivals(&s);
    let first_enter: Vec<(usize, u64)> = (0..3)
        .map(|m| {
            let t = sensor_events(&ev, true, 0)
                .into_iter()
                .find(|e| e.1 == m)
                .unwrap()
                .0;
            (m, t)
        })
        .collect();
    assert_eq!(arrivals, first_enter);
    assert_eq!(signals(&ev, "finish").len(), 3);
    assert_eq!(
        signals(&ev, "finish_count")
            .into_iter()
            .map(|c| c.1)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    // Every marble ends in the cup (cell (1, 0, 0), below the L3 floor) and at rest.
    for (m, st) in sim.marbles().iter().enumerate() {
        let p = st.position;
        assert!(
            libm::fabsf(p[0] - CELL) < 0.03 && p[1] < 0.045 && p[1] > 0.0,
            "marble {m} at {p:?}"
        );
        let v = Vector::from(st.linvel).length();
        assert!(v < 0.05, "marble {m} still moving at {v} m/s");
    }
}

// ---------------------------------------------------------------------------------------------
// Hopper (through TrackRun: the run spawns the queue in the bowl)
// ---------------------------------------------------------------------------------------------

fn hopper_track(release: ReleaseMode, n: usize) -> Track {
    let mut t = Track::new(3);
    t.placements = vec![
        place(HOPPER, 6, 0, 0, 0),
        place("blk_straight_l1", 7, 0, 0, 0),
        place("blk_straight_l1", 8, 0, 0, 0),
    ];
    t.hopper = HopperConfig {
        placement: Some(0),
        queue: vec![MarbleSize::Standard; n],
        release,
    };
    t
}

/// Ticks at which each marble left the hopper's release sensor, by marble.
fn hopper_exits(
    release: ReleaseMode,
    n: usize,
    ticks: u32,
) -> (TrackRun, Vec<SimEvent>, Vec<(u64, usize)>) {
    let mut run = TrackRun::new(&hopper_track(release, n)).unwrap();
    run.set_event_recording(true);
    let mut ev = Vec::new();
    for _ in 0..ticks {
        run.tick(1);
        ev.extend(run.drain_events());
    }
    let exits = sensor_events(&ev, false, 0);
    (run, ev, exits)
}

#[test]
fn hopper_releases_one_all_and_interval() {
    // One: only the first queued marble enters the run.
    let (run, ev, exits) = hopper_exits(ReleaseMode::One, 3, 600);
    assert_eq!(run.marbles().len(), 1);
    assert_eq!(exits.iter().map(|e| e.1).collect::<Vec<_>>(), [0]);
    assert_eq!(signals(&ev, "release"), [(1, 0)]);

    for (release, step) in [
        (ReleaseMode::All, ALL_STAGGER_TICKS),
        (ReleaseMode::Interval { ticks: 200 }, 200),
    ] {
        let (run, ev, exits) = hopper_exits(release, 3, 900);
        assert_eq!(run.marbles().len(), 3);
        assert_eq!(
            exits.iter().map(|e| e.1).collect::<Vec<_>>(),
            [0, 1, 2],
            "{release:?}"
        );
        // Each marble is released on its tick (the step after it spawns) and leaves through the
        // open gate the same number of ticks later.
        let rel = signals(&ev, "release");
        assert_eq!(
            rel,
            (0..3).map(|i| (i * step + 1, i as i64)).collect::<Vec<_>>()
        );
        let lag = exits[0].0 - rel[0].0;
        for (e, r) in exits.iter().zip(&rel) {
            assert_eq!(e.0 - r.0, lag, "{release:?}");
        }
        // All releases every 48 ticks, before a marble has cleared the gate's sweep (it leaves
        // `release` 16 mm past the cell), so the gate stays open for the whole queue.
        let cycles = if release == ReleaseMode::All { 1 } else { 3 };
        assert_eq!(signals(&ev, "gate_open").len(), cycles, "{release:?}");
        assert_eq!(signals(&ev, "gate_close").len(), cycles, "{release:?}");
        // Out of the hopper and along the run-out, in queue order.
        let xs: Vec<f32> = run.marbles().iter().map(|m| m.position[0]).collect();
        assert!(xs.iter().all(|&x| x > 0.39), "{release:?}: {xs:?}");
        assert!(xs.windows(2).all(|w| w[0] > w[1]), "{release:?}: {xs:?}");
        let v = &run.mechanisms()[0];
        assert_eq!(v.kind, "hopper");
        assert_eq!(
            (v.state["released"], v.state["passed"], v.state["open"]),
            (3, 3, 0)
        );
        assert_eq!(v.part_angles, [0]);
    }
}

#[test]
fn hopper_gate_is_shut_between_releases() {
    let mut run = TrackRun::new(&hopper_track(ReleaseMode::Interval { ticks: 300 }, 2)).unwrap();
    run.tick(100);
    assert_eq!(
        run.mechanisms()[0].part_angles,
        [0],
        "gate still open after the first marble left"
    );
    run.tick(200);
    // Tick 300: the second marble is not in yet (it spawns before the step from 300).
    assert_eq!(run.marbles().len(), 1);
    assert_eq!(run.mechanisms()[0].part_angles, [0]);
    run.tick(1 + HOPPER_OPEN_TICKS);
    assert_eq!(run.marbles().len(), 2);
    assert_eq!(run.mechanisms()[0].part_angles, [HOPPER_OPEN_MDEG]);
}

// ---------------------------------------------------------------------------------------------
// Save and restore
// ---------------------------------------------------------------------------------------------

#[test]
fn every_behaviour_restores_what_it_saved() {
    let reg = registry();
    let kit = parble_kit::builtin();
    let layout = [
        place("blk_straight_l1", 0, 0, 0, 0),
        place(GATE, 1, 0, 0, 0),
        place(CHECKPOINT, 2, 0, 0, 0),
        place(HOLE, 3, 0, 0, 0),
        place(ROPE_ARCH, 4, 0, 0, 0),
    ];
    let mut sim = rig(&layout, 5);
    launch(&mut sim, Vector::new(-0.02, rest(1), 0.0), Vector::X * 0.3);
    sim.tick(300);
    for v in sim.mechanisms() {
        let mut b = reg.get(&v.piece).unwrap()(&MechConfig::new());
        b.restore(&v.state);
        let saved = b.save();
        for (k, val) in &saved {
            assert_eq!(v.state.get(k), Some(val), "{}: {k}", v.kind);
        }
        assert!(
            saved
                .keys()
                .all(|k| !k.starts_with("rng/") && !k.starts_with("part/"))
        );
    }
    for id in [HOPPER, FINISH, SPLITTER, TOGGLE] {
        let f = reg.get(&PieceId::new(id).unwrap()).unwrap();
        let b = f(&MechConfig::new());
        let mut c = f(&MechConfig::new());
        c.restore(&b.save());
        assert_eq!(c.save(), b.save(), "{id}");
        assert!(kit.piece(&PieceId::new(id).unwrap()).is_some());
    }
}

#[test]
fn registry_covers_every_mechanism_piece() {
    let reg = registry();
    let kit = parble_kit::builtin();
    let mech: Vec<_> = kit
        .iter()
        .filter(|p| p.family == "mech")
        .map(|p| p.def.id.clone())
        .collect();
    assert_eq!(mech.len(), 10);
    for id in mech {
        assert!(reg.get(&id).is_some(), "{id} has no behaviour");
    }
}

/// Each fork part routes a marble to its commanded branch whatever its entry speed (up to 0.6 m/s:
/// faster marbles leave the L3 bend over its 15 mm lip) or lateral offset in the feed channel.
#[test]
fn shipped_fork_parts_route_slow_and_off_centre_marbles() {
    let bad = slow_and_off_centre_misses();
    assert!(bad.is_empty(), "missed the commanded branch: {bad:?}");
}

/// The runs of [`shipped_fork_parts_route_slow_and_off_centre_marbles`] that miss their branch:
/// `(piece, commanded side, speed, lateral offset)`.
pub(crate) fn slow_and_off_centre_misses() -> Vec<(&'static str, Side, f32, f32)> {
    let mut bad = Vec::new();
    for piece in [TOGGLE, SPLITTER] {
        for (first, seed) in [(0, 3), (1, 11)] {
            let want = if piece == TOGGLE {
                first
            } else {
                expected_splits(seed, 1)[0].unwrap()
            };
            for v in [0.2, 0.3, 0.4, 0.5, 0.6] {
                for dz in [-0.004, -0.002, 0.0, 0.002, 0.004] {
                    let mut t = place(piece, 1, 0, 0, 0);
                    t.config.insert("first".into(), first);
                    let mut sim = rig(&[place("blk_straight_l3", 0, 0, 0, 0), t], seed);
                    let (mut tr, mut ev) = (Vec::new(), Vec::new());
                    launch(&mut sim, Vector::new(-0.02, rest(3), dz), Vector::X * v);
                    run(&mut sim, 400, &mut tr, &mut ev);
                    if fork_side(&tr, 0) != Some(want) {
                        bad.push((piece, want, v, dz));
                    }
                }
            }
        }
    }
    bad
}

/// Every marble type at every size (mini 10 mm, standard 16 mm, shooter 25 mm) passes each fork
/// and leaves by its commanded branch, from the slowest entry speed the routing test uses to the
/// fastest the L3 feed bend holds, centred in the feed channel (a shooter has 1.5 mm either side).
/// Each run lasts 6 s, so a marble the blade slows to a crawl still has time to leave; a failure
/// is a marble at rest in the fork or back in the feed (`None`), or one that left sideways
/// (`Some(None)`).
///
/// The acceptance test for the shooter fork fix (owner option D1, 2026-11-09). On the two arcs
/// alone the 25° blade pivoted at x -2 failed all 140 shooter runs, and the best blade of the
/// sweep (pivot 2 mm, 24 mm, 33°) failed 7: centred minis at 0.2 m/s in the splitter climbed
/// 2.3 mm up the ridge between the arcs, stalled and rolled back before the blade reached them.
/// The flat frog under the split (kitmech `FORK_FROG_X`, `chunk::FORK_FROG_UM`) and the blade
/// pivoted over its end wall (3 mm, 24 mm, 32°) pass every run (2026-11-09). The contact
/// diagnosis of the D1 failures is in `fork_contact_test`.
#[test]
fn shipped_fork_parts_route_every_marble_type_and_size() {
    let bad = every_type_and_size_misses();
    assert!(
        bad.is_empty(),
        "{} missed the commanded branch: {bad:?}",
        bad.len()
    );
}

/// One missed run of [`shipped_fork_parts_route_every_marble_type_and_size`]: piece, size, type,
/// commanded side, speed, the exit (`None` at rest in the fork or back in the feed, `Some(None)`
/// sideways), the end position (mm) and speed (mm/s).
pub(crate) type Miss = (
    &'static str,
    &'static str,
    &'static str,
    Side,
    f32,
    Option<Option<Side>>,
    [f32; 3],
    f32,
);

/// The runs of [`shipped_fork_parts_route_every_marble_type_and_size`] that miss their branch.
pub(crate) fn every_type_and_size_misses() -> Vec<Miss> {
    let mut bad = Vec::new();
    for piece in [TOGGLE, SPLITTER] {
        for (first_side, seed) in [(0, 3), (1, 11)] {
            let want = if piece == TOGGLE {
                first_side
            } else {
                expected_splits(seed, 1)[0].unwrap()
            };
            for size in crate::marble::MarbleSize::ALL {
                for kind in MarbleKind::ALL {
                    let spec = kind.spec_sized(size);
                    for v in [0.2, 0.3, 0.4, 0.5, 0.6] {
                        let mut t = place(piece, 1, 0, 0, 0);
                        t.config.insert("first".into(), first_side);
                        let mut sim = rig(&[place("blk_straight_l3", 0, 0, 0, 0), t], seed);
                        let (mut tr, mut ev) = (Vec::new(), Vec::new());
                        let at = Vector::new(-0.02, 0.045 + spec.radius_m, 0.0);
                        launch_spec(&mut sim, &spec, at, Vector::X * v);
                        run(&mut sim, 1440, &mut tr, &mut ev);
                        // Like `fork_side`, but a sideways exit is a recorded failure, not a panic.
                        let got =
                            first(&tr, 0, |s| libm::fabsf(s.position[2]) > CELL / 2.0).map(|i| {
                                let p = tr[i][0].position;
                                if (CELL / 2.0..1.5 * CELL).contains(&p[0]) {
                                    Some(if p[2] > 0.0 { 0 } else { 1 })
                                } else {
                                    None
                                }
                            });
                        if got != Some(Some(want)) {
                            let last = tr.last().unwrap()[0];
                            let end = last.position.map(|c| (c * 1e4).round() / 10.0);
                            let l = last.linvel;
                            let mm_s = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt() * 1e3;
                            bad.push((piece, size.name(), kind.name(), want, v, got, end, mm_s));
                        }
                    }
                }
            }
        }
    }
    bad
}

/// Blade sweep (diagnostic, not a gate): each [`crate::fork_contact_test::Blade`] in
/// `PARBLE_BLADES` (`pivot_x,len,limit[,clearance];...`, mm and degrees) scored on the acceptance
/// and routing tests and the fork seam windows, on the shipped fork floor. `3,24,32` is the
/// shipped blade's design.
#[test]
#[ignore]
fn fork_blade_sweep() {
    use crate::fork_contact_test::{Blade, set_blade};
    let mut spec = std::env::var("PARBLE_BLADES").unwrap_or_else(|_| "1,24,36".into());
    if let Some(path) = spec.strip_prefix('@') {
        spec = std::fs::read_to_string(path).unwrap().trim().to_owned();
    }
    for b in spec.split(';') {
        let v: Vec<f32> = b.split(',').map(|x| x.trim().parse().unwrap()).collect();
        set_blade(Some(Blade {
            pivot_x_mm: v[0],
            len_mm: v[1],
            limit_deg: v[2],
            clear_mm: v.get(3).copied().unwrap_or(0.5),
        }));
        let accept = every_type_and_size_misses();
        let route = slow_and_off_centre_misses();
        let seams = crate::block_seam_test::fork_seam_metrics();
        set_blade(None);
        if accept.len() <= 20 {
            eprintln!("MISSES {b}: {accept:?}");
        }
        let mut by: BTreeMap<String, usize> = BTreeMap::new();
        for m in &accept {
            *by.entry(format!("{}/{}/{}", &m.0[5..], m.1, m.4))
                .or_default() += 1;
        }
        let seams: Vec<String> = seams
            .iter()
            .map(|(id, w, u, k)| format!("{}:{w} {u:.2}/{k:.2}", &id[5..]))
            .collect();
        eprintln!(
            "BLADE {b}: accept {} {by:?} route {} {route:?} seams {seams:?}",
            accept.len(),
            route.len()
        );
    }
}

/// One toggle run of [`toggle_swing_kicks`]: the highest speed the moving blade pushed the marble
/// to (its speed after a tick in which the moving blade touched it and it sped up), m/s, the
/// marble's peak speed on the toggle's cell, m/s, and, if it left the cell by a branch, whether
/// that was its commanded branch and whether the blade was then at the next side's limit.
fn toggle_swing_kick(
    spec: &MarbleSpec,
    first_side: Side,
    at: Vector,
    vel: Vector,
) -> (f32, f32, Option<(bool, bool)>) {
    let mut t = place(TOGGLE, 1, 0, 0, 0);
    t.config.insert("first".into(), first_side);
    let mut sim = rig(&[place("blk_straight_l3", 0, 0, 0, 0), t], 3);
    launch_spec(&mut sim, spec, at, vel);
    let speed = |s: &MarbleState| {
        let v = s.linvel;
        (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
    };
    let (mut kick, mut peak) = (0.0f32, 0.0f32);
    let mut angle = sim.mechanisms()[0].part_angles[0];
    let mut before = speed(&sim.marbles()[0]);
    let mut left = None;
    for _ in 0..1440 {
        sim.tick(1);
        let now = sim.mechanisms()[0].part_angles[0];
        let s = sim.marbles()[0];
        let after = speed(&s);
        // Only while the marble is on the toggle's cell: past it the open branch drops it.
        let on = (0.5 * CELL..1.5 * CELL).contains(&s.position[0])
            && libm::fabsf(s.position[2]) < 0.5 * CELL;
        let touched = crate::fork_contact_test::touches(&sim)
            .iter()
            .any(|c| c.kind == "part" && c.impulse > 0.0);
        if on && now != angle && touched && after > before {
            kick = kick.max(after);
        }
        if on {
            peak = peak.max(after);
        }
        if left.is_none() && libm::fabsf(s.position[2]) > 0.5 * CELL {
            left = Some(if s.position[2] > 0.0 { 0 } else { 1 });
        }
        angle = now;
        before = after;
    }
    let next = if first_side == 0 { 32_000 } else { -32_000 };
    (
        kick,
        peak,
        left.map(|side| (side == first_side, angle == next)),
    )
}

/// A failed run of [`toggle_swing_kicks`]: spec, size, commanded side, speed, lateral offset
/// (mm), the blade's largest kick and the peak speed (m/s), and how it left
/// ([`toggle_swing_kick`]).
type SwingMiss = (
    &'static str,
    &'static str,
    Side,
    f32,
    f32,
    f32,
    f32,
    Option<(bool, bool)>,
);

/// The runs of [`toggle_blade_never_swings_into_a_marble`] in which the moving blade pushed the
/// marble faster than `limit`, a marble left with the blade not then set for the next one, or a
/// marble fed at 0.2 m/s or faster did not leave by its commanded branch.
fn toggle_swing_kicks(limit: f32) -> Vec<SwingMiss> {
    let mut bad = Vec::new();
    for kind in [MarbleKind::CatsEye, MarbleKind::Glazed, MarbleKind::Steel] {
        for size in crate::marble::MarbleSize::ALL {
            let spec = kind.spec_sized(size);
            let side_room = 0.014 - spec.radius_m - 0.0005;
            for first_side in [0, 1] {
                for i in 1..=12 {
                    let v = 0.05 * i as f32;
                    for k in [-1.0, -0.5, 0.0, 0.5, 1.0] {
                        let dz = k * side_room;
                        let at = Vector::new(-0.02, 0.045 + spec.radius_m, dz);
                        let (kick, peak, left) =
                            toggle_swing_kick(&spec, first_side, at, Vector::X * v);
                        let routed = left.is_some_and(|(side, _)| side);
                        let set = left.is_none_or(|(_, set)| set);
                        if kick > limit || !set || (v >= 0.2 && !routed) {
                            bad.push((
                                kind.name(),
                                size.name(),
                                first_side,
                                v,
                                dz * 1e3,
                                kick,
                                peak,
                                left,
                            ));
                        }
                    }
                }
            }
        }
    }
    bad
}

/// The toggle's blade never throws a marble: a marble of every spec (cat's eye, glazed, steel;
/// swirl, onionskin, opal and glow share the cat's eye spec) at every size, fed at 0.05-0.6 m/s
/// across the feed channel's width, is never pushed faster than 0.2 m/s (the slowest entry speed
/// the fork routing tests use) by the moving blade. Every marble fed at 0.2 m/s or faster leaves
/// by its commanded branch, and whenever a marble leaves, the blade is then set for the next
/// marble: one pass per marble, even for one rocking across the sensor's edge. Below 0.2 m/s a
/// marble may stop on the frog clear of the blade, or take either branch.
///
/// The blade used to swing at full speed [`FORK_SETTLE_TICKS`] after the marble left the entry
/// sensor, which ends 5 mm short of the blade's tip, so a marble at 0.2 m/s or less was still in
/// the blade's sweep: 100 of these 1,080 runs took a kick of up to 0.87 m/s in one tick (glazed
/// mini at 0.05 m/s, 4.25 mm off centre). On the all-pieces track at seeds 6 and 7 the layer-0
/// toggle at [15, 0, -8] threw a steel mini rocking on its frog at 0.14 m/s out of the cell at
/// 1.2 m/s (2026-11-10). The blade now swings over [`FORK_NUDGE_TICKS`] when a marble is in its
/// sweep ([`blade_sweep_occupied`]).
#[test]
fn toggle_blade_never_swings_into_a_marble() {
    for k in MarbleKind::ALL {
        assert!(
            [MarbleKind::CatsEye, MarbleKind::Glazed, MarbleKind::Steel]
                .iter()
                .any(|s| s.spec() == k.spec()),
            "{} has a spec of its own",
            k.name()
        );
    }
    let bad = toggle_swing_kicks(0.2);
    assert!(bad.is_empty(), "{} runs failed: {bad:?}", bad.len());
}

/// The blade constants of [`blade_sweep_occupied`] cover both forks' blade colliders.
#[test]
fn fork_blade_constants_cover_the_blade_colliders() {
    let kit = parble_kit::builtin();
    for id in [TOGGLE, SPLITTER] {
        let part = &kit
            .piece(&PieceId::new(id).unwrap())
            .unwrap()
            .animated_parts[0];
        let pivot = part.pivot_um;
        for g in &part.collision {
            for v in &g.vertices_um {
                let d = [0, 1, 2].map(|k| (v[k] - pivot[k]) as f32 * 1e-6);
                let reach = libm::sqrtf(d[0] * d[0] + d[2] * d[2]);
                assert!(reach <= FORK_BLADE_REACH_M, "{id}: reach {reach}");
                assert!(
                    libm::fabsf(d[2]) <= FORK_BLADE_HALF_M + 1e-7,
                    "{id}: {}",
                    d[2]
                );
                assert!(
                    (FORK_BLADE_SPAN_M[0] - 1e-7..=FORK_BLADE_SPAN_M[1] + 1e-7).contains(&d[1]),
                    "{id}: height {}",
                    d[1]
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Capacity and throughput (defects found building the P2-T7 all-pieces track)
// ---------------------------------------------------------------------------------------------

/// The cup holds 10 standard marbles and its sensor reaches the cell top, so it counts all of
/// them (the old 38 mm sensor top missed the highest ones, centres 47–53 mm up).
#[test]
fn finish_counts_ten_marbles_in_one_cup() {
    let layout = [
        place("blk_straight_l3", 0, 0, 0, 0),
        place(FINISH, 1, 0, 0, 0),
    ];
    let mut sim = rig(&layout, 1);
    let (mut tr, mut ev) = (Vec::new(), Vec::new());
    for i in 0..10 {
        let v = 0.3 + 0.03 * i as f32;
        launch(&mut sim, Vector::new(-0.02, rest(3), 0.0), Vector::X * v);
        run(&mut sim, 240, &mut tr, &mut ev);
    }
    run(&mut sim, 960, &mut tr, &mut ev);
    assert_eq!(state(&sim)["count"], 10);
    for (m, st) in sim.marbles().iter().enumerate() {
        let p = st.position;
        assert!(
            libm::fabsf(p[0] - CELL - 0.006) < 0.021 && p[1] > 0.0 && p[1] < CELL,
            "marble {m} not in the cup: {p:?}"
        );
    }
}

/// Trap-side marbles drop through the open trapdoor into the drop port below and leave by the
/// tower socket's side port: straight into the tower, and through a pillar into it. The 60° door
/// hung 11 mm into the cell below and wedged the marbles against the bore.
#[test]
fn one_way_hole_trap_side_delivers_ten_marbles_into_the_piece_below() {
    for stack in [
        &["blk_tower_socket_l1"][..],
        &["blk_pillar", "blk_tower_socket_l1"],
    ] {
        let top = stack.len() as i32;
        let mut layout = vec![
            place("blk_straight_l1", 0, top, 0, 0),
            place(HOLE, 1, top, 0, 0),
        ];
        for (i, below) in stack.iter().enumerate() {
            layout.push(place(below, 1, top - 1 - i as i32, 0, 0));
        }
        let mut sim = rig(&layout, 1);
        let (mut tr, mut ev) = (Vec::new(), Vec::new());
        for i in 0..10 {
            let v = 0.3 + 0.02 * i as f32;
            let y = CELL * top as f32 + rest(1);
            launch(&mut sim, Vector::new(-0.02, y, 0.0), Vector::X * v);
            run(&mut sim, 360, &mut tr, &mut ev);
        }
        // Out of the socket: past the tower cell's -X face, in layer 0.
        let out: Vec<Option<usize>> = (0..10)
            .map(|m| {
                first(&tr, m, |s| {
                    s.position[0] < CELL / 2.0 && s.position[1] < CELL
                })
            })
            .collect();
        assert!(out.iter().all(Option::is_some), "{stack:?}: {out:?}");
        let s = state(&sim);
        assert_eq!((s["drops"], s["overs"]), (10, 0), "{stack:?}");
        assert_eq!(signals(&ev, "open").len(), 10, "{stack:?}");
    }
}

/// Rolled over from the +X side, the shut trapdoor carries every marble out of the -X port: 120
/// crossings, seeds 0–11, entry 0.35–0.5 m/s and up to 4 mm off centre. With the port faces
/// capped the seam threw marbles up and some stalled on the door.
#[test]
fn one_way_hole_carries_every_marble_over_from_the_other_side() {
    let mut stalled = Vec::new();
    for seed in 0..12u64 {
        let mut rng = SeededRng::new(seed).stream("hole_over");
        let mut u = || rng.next_u32() as f32 / u32::MAX as f32;
        for _ in 0..10 {
            let v = 0.35 + 0.15 * u();
            let dz = 0.004 * (2.0 * u() - 1.0);
            let layout = [
                place("blk_straight_l1", 0, 0, 0, 0),
                place(HOLE, 1, 0, 0, 0),
                place("blk_straight_l1", 2, 0, 0, 0),
            ];
            let mut sim = rig(&layout, seed);
            launch(&mut sim, Vector::new(0.14, rest(1), dz), Vector::X * -v);
            let (mut tr, mut ev) = (Vec::new(), Vec::new());
            run(&mut sim, 480, &mut tr, &mut ev);
            let out = first(&tr, 0, |s| s.position[0] < CELL / 2.0);
            let low = tr[..out.unwrap_or(tr.len())]
                .iter()
                .map(|s| s[0].position[1])
                .fold(f32::MAX, f32::min);
            if out.is_none() || low < rest(1) - 0.002 {
                stalled.push((seed, v, dz, tr.last().unwrap()[0].position));
            }
        }
    }
    assert!(
        stalled.is_empty(),
        "{} of 120 stalled: {stalled:?}",
        stalled.len()
    );
}

/// One over-side crossing of the shut trapdoor, or of a straight in its place (`hole` false):
/// speeds where the marble passes 15 mm before the hole cell's +X face and 15 mm past its −X
/// face, the seam metrics across that window, and its highest rise above rest.
#[derive(Debug, Clone, Copy)]
struct OverCrossing {
    v_in: f32,
    v_out: Option<f32>,
    upward: f32,
    kick: f32,
    rise_mm: f32,
}

const OVER_IN_X: f32 = 1.5 * CELL + 0.015;
const OVER_OUT_X: f32 = CELL / 2.0 - 0.015;

fn over_crossing(
    kind: MarbleKind,
    size: crate::marble::MarbleSize,
    speed: f32,
    hole: bool,
) -> OverCrossing {
    let spec = kind.spec_sized(size);
    let middle = if hole { HOLE } else { "blk_straight_l1" };
    let layout = [
        place("blk_straight_l1", 0, 0, 0, 0),
        place(middle, 1, 0, 0, 0),
        place("blk_straight_l1", 2, 0, 0, 0),
        place("blk_straight_l1", 3, 0, 0, 0),
        place("blk_straight_l1", 4, 0, 0, 0),
    ];
    let mut sim = rig(&layout, 1);
    let y0 = 0.015 + spec.radius_m;
    // Launched 35 mm before the window, a little faster, so it reaches the window at about
    // `speed` (rolling resistance on the level is about 15 mm/s²).
    let launch_x = OVER_IN_X + 0.035;
    let v0 = libm::sqrtf(speed * speed + 2.0 * 0.015 * 0.035);
    let (mut body, collider) = spec.build(Vector::new(launch_x, y0, 0.0));
    body.set_linvel(Vector::X * -v0, true);
    body.set_angvel(Vector::Z * (v0 / spec.radius_m), true);
    let h = sim.world.bodies.insert(body);
    sim.world
        .colliders
        .insert_with_parent(collider, h, &mut sim.world.bodies);
    sim.marbles.push(h);
    let mut tr = Vec::new();
    for _ in 0..4_800 {
        sim.tick(1);
        let s = sim.marbles()[0];
        tr.push(s);
        if s.position[0] < OVER_OUT_X - 0.005 {
            break;
        }
    }
    let plan = |s: &MarbleState| s.linvel[0].hypot(s.linvel[2]);
    let v_in = tr
        .iter()
        .find(|s| s.position[0] < OVER_IN_X)
        .map_or(0.0, plan);
    let v_out = tr.iter().find(|s| s.position[0] < OVER_OUT_X).map(plan);
    let sel = |s: &MarbleState| {
        (OVER_OUT_X..OVER_IN_X).contains(&s.position[0]) && libm::fabsf(s.position[2]) < 0.03
    };
    let rise_mm = tr
        .iter()
        .filter(|s| sel(s))
        .map(|s| (s.position[1] - y0) * 1_000.0)
        .fold(0.0, f32::max);
    OverCrossing {
        v_in,
        v_out,
        upward: crate::seam_test::upward(&tr, sel),
        kick: crate::seam_test::kick(&tr, sel),
        rise_mm,
    }
}

/// Over-side sweep across the shut trapdoor: standard cat's eye, glazed and steel marbles and
/// their minis at 0.06–0.65 m/s. Each keeps its speed to within 1.5× the loss a straight in the
/// door's place costs (rolling resistance alone over the same window), stays under the `L1` seam
/// thresholds (`block_seam_test::limits`) and in the channel.
///
/// The v4 door is flush with the floor, but it failed every row (measured 2026-11-09): at each
/// seam the marble met contacts on the other body's edge (at the hinge a door contact 0.37 mm
/// ahead tilted 0.30 against the travel, at the free edge a floor contact 0.69 mm ahead tilted
/// 0.39), up to 474 mm/s upward, and below 0.09 m/s it stopped on the door. The door seam hook
/// (`crate::door_seam`) drops those edge contacts, and the shut door is a fixed body, so it rides
/// as deep as the floor (35 µm, against 139 µm on the kinematic door). Measured 2026-11-09 with
/// both: loss ratio 0.83–1.31 (the most at 0.65 m/s for the standard marbles), upward at most
/// 0.20 mm/s, rise at most 0.011 mm.
#[test]
fn one_way_hole_over_side_sweep_keeps_rolling_resistance_speed() {
    let (up_lim, kick_lim) = crate::block_seam_test::limits(1);
    let mut bad = Vec::new();
    for size in [
        crate::marble::MarbleSize::Standard,
        crate::marble::MarbleSize::Mini,
    ] {
        for kind in [MarbleKind::CatsEye, MarbleKind::Glazed, MarbleKind::Steel] {
            for speed in [0.06, 0.09, 0.15, 0.26, 0.4, 0.65] {
                let door = over_crossing(kind, size, speed, true);
                let ctrl = over_crossing(kind, size, speed, false);
                let ctrl_loss = ctrl.v_in - ctrl.v_out.unwrap_or(0.0);
                let loss = door.v_out.map(|v| door.v_in - v);
                eprintln!(
                    "{:>8} {:>8} {speed:.2}: in {:.4} out {} (straight {:.4}) loss ratio {} \
                     upward {:.2} kick {:.2} rise {:.3} mm (straight upward {:.2} kick {:.2})",
                    kind.name(),
                    size.name(),
                    door.v_in,
                    door.v_out.map_or("stopped".into(), |v| format!("{v:.4}")),
                    ctrl.v_out.unwrap_or(0.0),
                    loss.map_or("-".into(), |l| format!("{:.2}", l / ctrl_loss)),
                    door.upward,
                    door.kick,
                    door.rise_mm,
                    ctrl.upward,
                    ctrl.kick,
                );
                let ok = loss.is_some_and(|l| l <= 1.5 * ctrl_loss)
                    && door.upward <= up_lim
                    && door.kick <= kick_lim
                    && door.rise_mm < 2.0;
                if !ok {
                    bad.push((kind.name(), size.name(), speed));
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "over-side crossings outside the window (upward limit {up_lim:.2}, kick {kick_lim:.2}): \
         {bad:?}"
    );
}

/// Ten hopper releases into a flat L1 run all roll out near the kick speed. The gate's 34 mm
/// sweep reaches past the cell, and closing as soon as a marble left the old 26 mm `release` box
/// caught it 13 mm past the port; the next opening flicked it out at up to 1.5 m/s.
#[test]
fn hopper_releases_ten_into_a_flat_straight_without_a_stall() {
    for release in [
        ReleaseMode::Interval { ticks: 480 },
        ReleaseMode::Interval { ticks: 240 },
        ReleaseMode::All,
    ] {
        let mut t = Track::new(3);
        t.placements = vec![place(HOPPER, 0, 0, -5, 0)];
        for x in 1..=8 {
            t.placements.push(place("blk_straight_l1", x, 0, -5, 0));
        }
        t.hopper = HopperConfig {
            placement: Some(0),
            queue: vec![MarbleSize::Standard; 10],
            release,
        };
        let mut run = TrackRun::new(&t).unwrap();
        // Lowest and highest +X speed of each marble in the first straight's cell.
        let mut speed = [(f32::MAX, 0.0f32); 10];
        for _ in 0..5_000 {
            run.tick(1);
            for (m, s) in run.marbles().iter().enumerate() {
                if (CELL / 2.0..1.5 * CELL).contains(&s.position[0]) {
                    speed[m].0 = speed[m].0.min(s.linvel[0]);
                    speed[m].1 = speed[m].1.max(s.linvel[0]);
                }
            }
        }
        assert_eq!(run.marbles().len(), 10, "{release:?}");
        for (m, &(lo, hi)) in speed.iter().enumerate() {
            assert!(
                lo > 0.15 && hi < 0.35,
                "{release:?}: marble {m} at {lo}..{hi} m/s past the gate"
            );
        }
        assert!(
            run.marbles().iter().all(|s| s.position[0] > 4.0 * CELL),
            "{release:?}: a marble stayed near the hopper"
        );
        let v = &run.mechanisms()[0];
        assert_eq!(
            (v.state["released"], v.state["passed"]),
            (10, 10),
            "{release:?}"
        );
        assert_eq!(v.part_angles, [0], "{release:?}");
    }
}

// ---------------------------------------------------------------------------------------------
// Rope-curtain arch
// ---------------------------------------------------------------------------------------------

/// Launch speeds of the rope arch sweeps, m/s.
const ROPE_SPEEDS: [f32; 7] = [0.1, 0.15, 0.2, 0.3, 0.45, 0.6, 0.8];

/// A cat's eye rolled along `dir` through L1 straight, rope arch, L1 straight at `speed`: its
/// speed as its surface enters the curtain zone and as it leaves it (m/s), the rope angles after
/// every tick, and the trajectory.
fn rope_pass(dir: f32, speed: f32) -> (f32, f32, Vec<Vec<i32>>, Vec<Vec<MarbleState>>) {
    let layout = [
        place("blk_straight_l1", 0, 0, 0, 0),
        place(ROPE_ARCH, 1, 0, 0, 0),
        place("blk_straight_l1", 2, 0, 0, 0),
    ];
    let mut sim = rig(&layout, 1);
    let x0 = if dir > 0.0 { -0.02 } else { 0.14 };
    launch(
        &mut sim,
        Vector::new(x0, rest(1), 0.0),
        Vector::X * speed * dir,
    );
    let half = ROPE_ARCH_HALF_UM as f32 * 1.0e-6 + R;
    let (mut tr, mut angles) = (Vec::new(), Vec::new());
    for _ in 0..900 {
        sim.tick(1);
        tr.push(sim.marbles());
        angles.push(sim.mechanisms()[0].part_angles.clone());
    }
    // Along the travel direction, from the arch centre (x = 1 cell).
    let s = |m: &MarbleState| (m.position[0] - CELL) * dir;
    let speed_at = |edge: f32| {
        tr.iter()
            .map(|t| &t[0])
            .find(|m| s(m) >= edge)
            .map_or(0.0, |m| Vector::from(m.linvel).length())
    };
    (speed_at(-half), speed_at(half), angles, tr)
}

/// [`rope_pass`] with the curtain's drag switched off: the same pass's rolling loss alone.
fn rope_pass_no_drag(dir: f32, speed: f32) -> (f32, f32) {
    ROPE_DRAG_OFF.with(|c| c.set(true));
    let (entry, exit, _, _) = rope_pass(dir, speed);
    ROPE_DRAG_OFF.with(|c| c.set(false));
    (entry, exit)
}

#[test]
fn rope_arch_halves_the_speed_of_a_marble_passing_through() {
    let mut report = Vec::new();
    for dir in [1.0, -1.0] {
        for v in ROPE_SPEEDS {
            let (entry, exit, _, tr) = rope_pass(dir, v);
            let (entry0, exit0) = rope_pass_no_drag(dir, v);
            // The curtain's own effect: the pass's ratio over the same pass's with no drag.
            let ratio = (exit / entry) / (exit0 / entry0);
            report.push(format!(
                "dir {dir} launch {v}: entry {entry:.4} exit {exit:.4} no drag {entry0:.4} -> \
                 {exit0:.4} curtain {ratio:.4}"
            ));
            assert!(
                entry > 0.0 && exit > 0.0,
                "dir {dir} launch {v}: never left the arch"
            );
            if entry >= ROPE_MIN_HALVING_SPEED_M_S {
                assert!(
                    (0.45..=0.55).contains(&ratio),
                    "dir {dir} launch {v}: entry {entry} exit {exit} curtain ratio {ratio}"
                );
            } else {
                assert!(exit < entry, "dir {dir} launch {v}: {entry} -> {exit}");
            }
            // The ropes have no collider: the marble stays on the channel floor, centred, and
            // leaves by the far straight.
            let on_track = |m: &&MarbleState| (m.position[0] - CELL).abs() < 1.5 * CELL - R;
            for m in tr.iter().map(|t| &t[0]).filter(on_track) {
                assert!(
                    (m.position[1] - rest(1)).abs() < 1.0e-3,
                    "dir {dir} launch {v}: {m:?}"
                );
                assert!(m.position[2].abs() < 1.0e-3, "dir {dir} launch {v}: {m:?}");
            }
            // Above the minimum it rolls off the far straight; below it, rolling resistance may
            // stop it on that straight, past the curtain.
            let last = tr.last().unwrap()[0].position[0];
            let past = if entry >= ROPE_MIN_HALVING_SPEED_M_S {
                1.5 * CELL
            } else {
                ROPE_ARCH_HALF_UM as f32 * 1.0e-6 + R
            };
            assert!(
                (last - CELL) * dir > past,
                "dir {dir} launch {v}: stopped at x = {last}"
            );
        }
        // The card's criterion speed is in the sweep and above the minimum.
        assert!(ROPE_SPEEDS.contains(&0.6) && ROPE_SPEEDS[0] < ROPE_MIN_HALVING_SPEED_M_S);
        // A crawling marble (0.05 m/s) enters the zone at 0.026 m/s and stops in it, curtain or
        // not: rolling resistance alone takes it.
        let (entry, exit, _, _) = rope_pass(dir, 0.05);
        let (entry0, exit0) = rope_pass_no_drag(dir, 0.05);
        assert!(
            entry > 0.0 && exit == 0.0 && entry0 > 0.0 && exit0 == 0.0,
            "dir {dir}: {entry} -> {exit}, no drag {entry0} -> {exit0}"
        );
    }
    println!("{}", report.join("\n"));
}

#[test]
fn rope_arch_ropes_swing_away_from_the_marble_and_settle() {
    for dir in [1.0_f32, -1.0] {
        let (_, _, angles, _) = rope_pass(dir, 0.3);
        assert!(angles[0].iter().all(|&a| a == 0), "ropes start at rest");
        for rope in 0..8 {
            let away = angles
                .iter()
                .map(|a| a[rope] as f32 * dir)
                .fold(f32::MIN, f32::max);
            assert!(
                away >= ROPE_SWING_MIN_MDEG as f32 * 0.6 - 1.0,
                "dir {dir} rope {rope} swings away only {away} mdeg"
            );
            // The swing reverses past rest before it dies out.
            assert!(
                angles.iter().any(|a| a[rope] as f32 * dir < -1_000.0),
                "dir {dir} rope {rope} never swings back"
            );
            assert_eq!(
                angles.last().unwrap()[rope],
                0,
                "dir {dir} rope {rope} settles"
            );
        }
    }
}

/// The ropes are drawn along their segments ([`crate::rope::polyline_mdeg`] of
/// `part_segments`). At every tick of a pass, in both directions and across the speed sweep, no
/// drawn strand point is inside the marble, and the ropes the marble reaches bend along their
/// length (their segments differ by more than 20°) rather than turning as sticks.
///
/// The far LOD draws each rope as two straight bands, the chords of joints 0–3 and 3–6
/// (`parble_render::kit::parts::ROPE_LOD_BANDS`), which cut the inside of a bend. That LOD is
/// drawn only once a 60 mm cell is under 48 × 1.15 px tall at 1440p (`LOD_PX`, `LOD_FADE`), at
/// most 0.92 px per mm, so the chords may enter the marble by up to a pixel there:
/// [`FAR_CHORD_INTRUSION_M`].
/// How far the far LOD's two-band chords may enter the marble: one pixel where that LOD starts
/// (0.92 px per mm).
const FAR_CHORD_INTRUSION_M: f32 = 1.0e-3 / 0.92;

#[test]
fn rope_arch_ropes_bend_round_the_marble_and_never_pass_through_it() {
    let piece = parble_kit::builtin()
        .piece(&parble_core::PieceId::new(ROPE_ARCH).unwrap())
        .unwrap();
    for dir in [1.0_f32, -1.0] {
        for speed in [0.15_f32, 0.3, 0.6, 0.8] {
            let layout = [
                place("blk_straight_l1", 0, 0, 0, 0),
                place(ROPE_ARCH, 1, 0, 0, 0),
                place("blk_straight_l1", 2, 0, 0, 0),
            ];
            let mut sim = rig(&layout, 1);
            let x0 = if dir > 0.0 { -0.02 } else { 0.14 };
            launch(
                &mut sim,
                Vector::new(x0, rest(1), 0.0),
                Vector::X * speed * dir,
            );
            let mut worst = f32::MAX;
            let mut far = f32::MAX;
            let mut bend = [0_i32; 8];
            for _ in 0..600 {
                sim.tick(1);
                let v = &sim.mechanisms()[0];
                let c = Vector::from(sim.marbles()[0].position);
                for (k, part) in piece.animated_parts.iter().enumerate() {
                    let seg = &v.part_segments[k];
                    assert_eq!(seg.len(), crate::rope::SEGMENTS);
                    let spread = seg.iter().max().unwrap() - seg.iter().min().unwrap();
                    bend[k] = bend[k].max(spread);
                    let [px, py, pz] = part.pivot_um.map(|c| c as f32 * 1.0e-6);
                    let pivot = Vector::new(CELL + px, py, pz);
                    let pts = crate::rope::polyline_mdeg(seg);
                    for w in pts.windows(2) {
                        for i in 0..=4 {
                            let t = i as f32 / 4.0;
                            let u = w[0].0 + (w[1].0 - w[0].0) * t;
                            let y = w[0].1 + (w[1].1 - w[0].1) * t;
                            let p = pivot + Vector::new(u, y, 0.0);
                            let gap = (p - c).length() - R - crate::rope::STRAND_R_M;
                            worst = worst.min(gap);
                        }
                    }
                    for w in [[pts[0], pts[3]], [pts[3], pts[6]]] {
                        for i in 0..=16 {
                            let t = i as f32 / 16.0;
                            let u = w[0].0 + (w[1].0 - w[0].0) * t;
                            let y = w[0].1 + (w[1].1 - w[0].1) * t;
                            let p = pivot + Vector::new(u, y, 0.0);
                            far = far.min((p - c).length() - R - crate::rope::STRAND_R_M);
                        }
                    }
                }
            }
            assert!(
                far > -FAR_CHORD_INTRUSION_M,
                "dir {dir} speed {speed}: a far-LOD chord is {:.2} mm inside the marble",
                -far * 1.0e3
            );
            assert!(
                worst > -1.0e-4,
                "dir {dir} speed {speed}: a strand point is {:.2} mm inside the marble",
                -worst * 1.0e3
            );
            // The inner ropes (z = ±4.5 mm) are in the marble's path.
            for k in [1, 2, 5, 6] {
                assert!(
                    bend[k] > 20_000,
                    "dir {dir} speed {speed}: rope {k} bends at most {} mdeg",
                    bend[k]
                );
            }
        }
    }
}

#[test]
fn rope_arch_drag_keeps_the_same_share_for_every_marble_size() {
    // The zone a marble's centre crosses grows with its radius, and k shrinks with it.
    for r in [0.005_f32, 0.008, 0.0125] {
        let zone = 2.0 * (ROPE_ARCH_HALF_UM as f32 * 1.0e-6 + r);
        let keep = libm::expf(-RopeArch::drag_k(r) * zone);
        assert!(
            (keep - ROPE_KEEP).abs() < 1.0e-5,
            "radius {r}: keeps {keep}"
        );
    }
}

#[test]
fn one_way_gate_flap_passes_every_forward_speed() {
    // A sweep with no stall band, from the gate's minimum speed to 0.2 m/s in 5 mm/s steps.
    // Passing is the marble's back clearing the shut flap's far face with the flap shut behind it,
    // and 1,200 ticks leave the slowest marbles time against rolling resistance.
    let far_edge = flap_reach_and_limit(0).0;
    let mut stalled = Vec::new();
    for k in 0..=27 {
        let speed = GATE_MIN_LAUNCH_SPEED_M_S + 0.005 * k as f32;
        if !gate_lets_through(speed, 1_200, far_edge) {
            stalled.push(speed);
        }
    }
    assert!(
        stalled.is_empty(),
        "the flap stalled marbles at {stalled:?} m/s"
    );
}

#[test]
fn one_way_gate_flap_holds_marbles_below_its_minimum_speed() {
    // Below [`GATE_MIN_LAUNCH_SPEED_M_S`] a marble runs out of energy before it clears the flap:
    // 0.06 m/s stops under it, 0.05 at it (and 0.05 stops there with no flap at all).
    let far_edge = flap_reach_and_limit(0).0;
    for speed in [0.05, 0.055, 0.06] {
        assert!(
            !gate_lets_through(speed, 2_400, far_edge),
            "{speed} m/s got through"
        );
    }
}

/// One forward marble at `speed` on the gate test layout: whether within `ticks` its back clears
/// the shut flap's far face with the flap shut behind it.
fn gate_lets_through(speed: f32, ticks: usize, far_edge: f32) -> bool {
    let layout = [
        place("blk_straight_l1", 0, 0, 0, 0),
        place(GATE, 1, 0, 0, 0),
        place("blk_straight_l1", 2, 0, 0, 0),
    ];
    let mut sim = rig(&layout, 1);
    launch(
        &mut sim,
        Vector::new(-0.02, rest(1), 0.0),
        Vector::X * speed,
    );
    (0..ticks).any(|_| {
        sim.tick(1);
        sim.marbles()[0].position[0] - R > far_edge
            && sim.mechanisms()[0].part_angles[0] <= GATE_CLOSED_MDEG
    })
}

/// The marble specs (swirl, onionskin, opal and glow share the cat's eye spec) at every size.
fn every_spec_and_size() -> Vec<(String, MarbleSpec)> {
    for k in MarbleKind::ALL {
        assert!(
            [MarbleKind::CatsEye, MarbleKind::Glazed, MarbleKind::Steel]
                .iter()
                .any(|b| b.spec() == k.spec()),
            "{} has a spec of its own",
            k.name()
        );
    }
    [MarbleKind::CatsEye, MarbleKind::Glazed, MarbleKind::Steel]
        .into_iter()
        .flat_map(|k| {
            crate::marble::MarbleSize::ALL
                .map(|s| (format!("{} {}", k.name(), s.name()), k.spec_sized(s)))
        })
        .collect()
}

/// One `spec` marble on the gate test layout, launched `dz` m off the axis at `speed` m/s from
/// x = −20 mm (forward, `dir` +1) or x = 140 mm (reverse, −1), for `ticks` ticks: the marble's
/// trace, the flap angle after every tick, the events and the gate's state.
fn gate_spec_run(
    spec: &MarbleSpec,
    dir: f32,
    speed: f32,
    dz: f32,
    ticks: usize,
) -> (Vec<MarbleState>, Vec<i32>, Vec<SimEvent>, MechConfig) {
    let layout = [
        place("blk_straight_l1", 0, 0, 0, 0),
        place(GATE, 1, 0, 0, 0),
        place("blk_straight_l1", 2, 0, 0, 0),
    ];
    let mut sim = rig(&layout, 1);
    let x0 = if dir > 0.0 { -0.02 } else { 0.14 };
    launch_spec(
        &mut sim,
        spec,
        Vector::new(x0, 0.015 + spec.radius_m, dz),
        Vector::X * speed * dir,
    );
    let (mut tr, mut angles) = (Vec::new(), Vec::new());
    for _ in 0..ticks {
        sim.tick(1);
        tr.push(sim.marbles()[0]);
        angles.push(sim.mechanisms()[0].part_angles[0]);
    }
    let ev = sim.drain_events();
    (tr, angles, ev, state(&sim))
}

/// Forward launch speeds of the every-spec gate sweep, m/s: 0.1-0.4 in 50 mm/s steps, and two
/// fast marbles.
const GATE_SWEEP_SPEEDS: [f32; 9] = [0.1, 0.15, 0.2, 0.25, 0.3, 0.35, 0.4, 0.6, 1.0];

#[test]
fn one_way_gate_passes_every_marble_spec_and_size() {
    // Every spec at every size, 1 mm either side of the axis or centred, one cell upstream of the
    // gate: the marble's centre leaves the gate's cell by its `exit` port (x 90 mm) on the floor,
    // and the flap is shut behind it at the end, never swung back through its stop.
    let mut bad = Vec::new();
    for (name, spec) in every_spec_and_size() {
        for speed in GATE_SWEEP_SPEEDS {
            for dz in [-0.001, 0.0, 0.001] {
                let (tr, angles, _, s) = gate_spec_run(&spec, 1.0, speed, dz, 1_200);
                let end = tr.last().unwrap();
                let out = tr
                    .iter()
                    .any(|m| m.position[0] > 1.5 * CELL && m.position[1] > 0.015);
                let shut = *angles.last().unwrap() <= GATE_CLOSED_MDEG;
                let through = angles.iter().all(|&a| a > -1_500);
                eprintln!(
                    "gate {name} {speed:.2} m/s {:+.0} mm: end x {:.3}, flap {} mdeg, passes {}",
                    dz * 1e3,
                    end.position[0],
                    angles.last().unwrap(),
                    s["passes"]
                );
                if !(out && shut && through && s["passes"] == 1) {
                    bad.push(format!(
                        "{name} {speed:.2} {:+.0} mm: end {:?}",
                        dz * 1e3,
                        end.position
                    ));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn one_way_gate_stops_reverse_marbles_of_every_spec_and_size() {
    // The one-way function for every spec and size: from `exit` the shut flap stops the marble
    // one radius short of its plate (within 2 mm: the 0.6 and 1 m/s minis and shooters press
    // 1.1-1.6 mm into the soft contact), never opens and counts it blocked. Minis stay against
    // the plate rather than rolling back, so the check is the distance, not a rebound.
    let mut bad = Vec::new();
    for (name, spec) in every_spec_and_size() {
        let r = spec.radius_m;
        for speed in GATE_SPEEDS {
            let (tr, angles, ev, s) = gate_spec_run(&spec, -1.0, speed, 0.0, 600);
            let closest = tr.iter().map(|m| m.position[0]).fold(f32::MAX, f32::min);
            let end = tr.last().unwrap().position[0];
            let ok = closest > CELL + 0.0015 + r - 0.002
                && angles.iter().all(|&a| (-500..GATE_OPEN_MDEG).contains(&a))
                && signals(&ev, "open").is_empty()
                && s["blocked"] == 1
                && s["passes"] == 0;
            if !ok {
                bad.push(format!(
                    "{name} {speed:.3} m/s: closest x {closest:.4}, end x {end:.4}"
                ));
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// Diagnostic, not a gate: the slowest forward launch speed, on a 5 mm/s grid, at which each
/// spec and size gets through the gate on its test layout
/// (`cargo test -p parble-sim --release gate_min_launch_speeds -- --ignored --nocapture`).
#[test]
#[ignore]
fn gate_min_launch_speeds() {
    for (name, spec) in every_spec_and_size() {
        let min = (6..=40).map(|k| 0.005 * k as f32).find(|&v| {
            let (tr, angles, ..) = gate_spec_run(&spec, 1.0, v, 0.0, 2_400);
            tr.last().unwrap().position[0] > 1.5 * CELL
                && *angles.last().unwrap() <= GATE_CLOSED_MDEG
        });
        println!("GATE MIN {name}: {min:?} m/s");
    }
}
