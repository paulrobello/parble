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
        Vector::X * 0.3 * dir,
    );
    let (mut tr, mut ev) = (Vec::new(), Vec::new());
    run(&mut sim, 600, &mut tr, &mut ev);
    (sim, tr, ev)
}

#[test]
fn one_way_gate_opens_forward_and_blocks_reverse() {
    let (sim, tr, ev) = one_way(GATE, 1.0);
    let past = first(&tr, 0, |s| s.position[0] > 1.5 * CELL).expect("forward marble never passed");
    assert!(tr[past][0].position[1] > 0.015, "forward marble fell");
    let open = signals(&ev, "open");
    assert_eq!(open.len(), 1);
    assert!(
        (open[0].0 as usize) < past,
        "the flap opened after the marble passed"
    );
    assert_eq!(signals(&ev, "close").len(), 1);
    assert_eq!(signals(&ev, "forward"), [(open[0].0, 0)]);
    let s = state(&sim);
    assert_eq!((s["passes"], s["blocked"], s["open"]), (1, 0, 0));
    assert_eq!(sim.mechanisms()[0].part_angles, [0]);

    let (sim, tr, ev) = one_way(GATE, -1.0);
    // The shut flap (2 mm plate at the cell centre) stops the marble 9 mm short of it.
    let closest = tr.iter().map(|s| s[0].position[0]).fold(f32::MAX, f32::min);
    assert!(
        closest > CELL + 0.008,
        "reverse marble reached x = {closest}"
    );
    assert!(signals(&ev, "open").is_empty());
    assert_eq!(signals(&ev, "reverse").len(), 1);
    assert_eq!(state(&sim)["blocked"], 1);
    assert!(
        tr.last().unwrap()[0].linvel[0] > 0.0,
        "reverse marble did not turn back"
    );
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
        assert_eq!(signals(&ev, "gate_open").len(), 3, "{release:?}");
        assert_eq!(signals(&ev, "gate_close").len(), 3, "{release:?}");
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
    assert_eq!(mech.len(), 7);
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
