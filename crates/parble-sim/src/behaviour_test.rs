//! P2-T5 scripted runs: real marbles through each kit mechanism, asserting where they go.

use super::*;
use crate::marble::CATS_EYE;
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
    let layout = [
        place("blk_straight_l3", 0, 0, 0, 0),
        place(piece, 1, 0, 0, 0),
    ];
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
    for _ in 0..600 {
        sim.tick(1);
        tr.push(sim.marbles());
        angles.push(sim.mechanisms()[0].part_angles[0]);
    }
    let ev = sim.drain_events();
    (sim, tr, angles, ev)
}

/// Launch speeds of the gate sweeps, m/s: from a marble barely rolling to a fast drop.
const GATE_SPEEDS: [f32; 6] = [0.06, 0.1, 0.2, 0.3, 0.6, 1.0];

#[test]
fn one_way_gate_flap_is_pushed_open_by_a_forward_marble() {
    let [_, hi] = parble_kit::builtin()
        .piece(&PieceId::new(GATE).unwrap())
        .unwrap()
        .animated_parts[0]
        .limits_mdeg;
    for speed in GATE_SPEEDS {
        let (sim, tr, angles, ev) = one_way_at(GATE, 1.0, speed);
        let past = first(&tr, 0, |s| s.position[0] > 1.5 * CELL)
            .unwrap_or_else(|| panic!("{speed} m/s: the forward marble never passed"));
        assert!(tr[past][0].position[1] > 0.015, "{speed} m/s: marble fell");
        assert!(
            tr.last().unwrap()[0].linvel[0] > 0.02,
            "{speed} m/s: marble stalled"
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
        assert!(
            (open[0].0 as usize) < past,
            "{speed} m/s: opened after the pass"
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
        assert!(
            tr.last().unwrap()[0].linvel[0] > 0.0,
            "{speed} m/s: reverse marble did not turn back"
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
    assert_eq!(mech.len(), 8);
    for id in mech {
        assert!(reg.get(&id).is_some(), "{id} has no behaviour");
    }
}

/// Each fork part routes a marble to its commanded branch whatever its entry speed (up to 0.6 m/s:
/// faster marbles leave the L3 bend over its 15 mm lip) or lateral offset in the feed channel.
#[test]
fn shipped_fork_parts_route_slow_and_off_centre_marbles() {
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
    assert!(bad.is_empty(), "missed the commanded branch: {bad:?}");
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
