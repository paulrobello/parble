//! Tests for the mechanism framework (P2-T1).

use super::demo::{SwingFlap, piece, ramp_with_flap};
use super::*;
use crate::golden::{RAMP_10S_TICKS, ramp_10s_hash};

#[derive(Debug, PartialEq)]
struct Outcome {
    trajectory: u64,
    mechanism: u64,
    views: Vec<MechanismView>,
    events: Vec<SimEvent>,
}

fn finish(sim: &mut Sim, mut events: Vec<SimEvent>) -> Outcome {
    assert_eq!(sim.tick_count(), u64::from(RAMP_10S_TICKS));
    events.extend(sim.drain_events());
    Outcome {
        trajectory: sim.trajectory_hash(),
        mechanism: sim.mechanism_hash(),
        views: sim.mechanisms(),
        events,
    }
}

/// Drive the flap run in tick chunks, draining events after each chunk (one "frame").
fn by_chunks(chunks: impl IntoIterator<Item = u32>) -> Outcome {
    let mut sim = ramp_with_flap(7);
    let mut events = Vec::new();
    let mut left = RAMP_10S_TICKS;
    for n in chunks.into_iter().chain(core::iter::repeat(1)) {
        if left == 0 {
            break;
        }
        let n = n.min(left);
        sim.tick(n);
        left -= n;
        events.extend(sim.drain_events());
    }
    finish(&mut sim, events)
}

/// The app's frame loop: a wall-clock accumulator turned into whole ticks per frame.
fn by_frames(frame_s: impl Fn(u32) -> f64) -> Outcome {
    let dt = f64::from(crate::DT);
    let mut acc = 0.0;
    let mut chunks = Vec::new();
    let mut total = 0u32;
    let mut frame = 0;
    while total < RAMP_10S_TICKS {
        acc += frame_s(frame);
        frame += 1;
        let n = (acc / dt) as u32;
        acc -= f64::from(n) * dt;
        chunks.push(n);
        total += n;
    }
    by_chunks(chunks)
}

#[test]
fn keyframes_interpolate_on_integer_ticks() {
    let k = Keyframes::new(vec![(0, 0), (24, 48_000), (48, 48_000), (72, 0)]).unwrap();
    assert_eq!(
        [0, 1, 12, 24, 30, 48, 60, 72, 1_000].map(|t| k.sample(t)),
        [0, 2_000, 24_000, 48_000, 48_000, 48_000, 24_000, 0, 0]
    );
    assert_eq!(Keyframes::ramp(10, -7, 3).sample(1), 5);
    assert!(Keyframes::new(vec![(4, 0), (4, 1)]).is_none());
    assert!(Keyframes::new(vec![]).is_none());
}

#[test]
fn mechanisms_advance_by_tick_count_independent_of_frame_cadence() {
    let reference = by_chunks([RAMP_10S_TICKS]);
    // The flap is in the marble's path, so the run is not the bare ramp.
    assert_ne!(reference.trajectory, ramp_10s_hash());
    assert_eq!(reference.views[0].state["swings"], 10);
    assert!(reference.events.iter().any(|e| matches!(
        e,
        SimEvent::Contact(ContactEvent {
            other: ContactOther::Part { .. },
            ..
        })
    )));

    let single = by_chunks(core::iter::repeat_n(1, RAMP_10S_TICKS as usize));
    assert_eq!(single, reference);
    let irregular = by_chunks([1, 3, 7, 2, 50, 0, 11, 240, 5, 97].into_iter().cycle());
    assert_eq!(irregular, reference);
    for hz in [30.0, 60.0, 144.0, 165.0] {
        assert_eq!(by_frames(|_| 1.0 / hz), reference, "{hz} Hz frames");
    }
    // Jittered frame times between 4 ms and 36 ms.
    let jitter = by_frames(|f| 0.004 + f64::from(f.wrapping_mul(2_654_435_761) % 33) * 1.0e-3);
    assert_eq!(jitter, reference);
}

fn at(x: i32, y: i32, z: i32) -> Placement {
    Placement::new(
        piece().def.id.clone(),
        CellCoord::new(x, y, z),
        parble_core::Yaw4::new(0),
        Mirror::Off,
    )
}

/// The `swing` signal values of the instance at `watch` over 10 s, with `placements` added in
/// order.
fn swings(seed: u64, placements: &[Placement], watch: CellCoord) -> Vec<i64> {
    let mut sim = Sim::empty();
    sim.set_event_recording(true);
    for p in placements {
        sim.add_mechanism(piece(), p, seed, SwingFlap::factory(&p.config))
            .unwrap();
    }
    sim.tick(RAMP_10S_TICKS);
    sim.drain_events()
        .into_iter()
        .filter_map(|e| match e {
            SimEvent::Mechanism(MechanismEvent {
                instance,
                kind: MechEventKind::Signal { value, .. },
                ..
            }) if instance.0 == watch => Some(value),
            _ => None,
        })
        .collect()
}

#[test]
fn rng_sub_streams_are_per_instance_and_stable_across_edits() {
    let (a, b, c) = (at(0, 10, 0), at(5, 10, 0), at(-5, 10, 0));
    let watch = a.anchor;
    let base = swings(42, &[a.clone(), b.clone()], watch);
    assert_eq!(base.len(), 10);
    // Adding C (ahead of A in id and insertion order) and removing B leave A's draws alone.
    assert_eq!(swings(42, &[c.clone(), a.clone()], watch), base);
    assert_eq!(swings(42, &[b.clone(), c.clone(), a.clone()], watch), base);
    assert_eq!(swings(42, std::slice::from_ref(&a), watch), base);
    // The draws are the stream keyed by (seed, piece, anchor, name).
    let mut r = MechRng::new(SeededRng::new(42).stream(&stream_key(&a.piece, watch, "swing")));
    let expect: Vec<i64> = (0..10)
        .map(|_| 30_000 + i64::from(r.below(50_000)))
        .collect();
    assert_eq!(base, expect);
    // Another instance, another seed or another stream name draws differently.
    assert_ne!(swings(42, &[a.clone(), b.clone()], b.anchor), base);
    assert_ne!(swings(43, std::slice::from_ref(&a), watch), base);
    let mut other = SeededRng::new(42).stream(&stream_key(&a.piece, watch, "other"));
    let mut swing = SeededRng::new(42).stream(&stream_key(&a.piece, watch, "swing"));
    assert_ne!(
        (0..4).map(|_| other.next_u32()).collect::<Vec<_>>(),
        (0..4).map(|_| swing.next_u32()).collect::<Vec<_>>()
    );
}

#[test]
fn saved_state_restores_behaviour_rng_and_parts() {
    let mut track = Track::new(9);
    track.placements = vec![at(0, 10, 0), at(3, 10, 0)];
    let build = || {
        let mut sim = Sim::empty();
        for p in &track.placements {
            sim.add_mechanism(piece(), p, track.seed, SwingFlap::factory(&p.config))
                .unwrap();
        }
        sim
    };
    let mut straight = build();
    straight.tick(1_000);
    let saved = straight.save_mechanism_states(&track);
    assert_eq!(saved.len(), 2);
    assert_eq!(saved[0].placement, 0);
    // Swings at ticks 240, 480, 720 and 960 (an instance first steps the tick after it is added).
    assert_eq!(saved[0].state["swings"], 4);
    assert_eq!(saved[0].state["rng/swing"], 4);
    // Tick 1 000 is 40 ticks into the swing that started at 960, so the flap is off rest.
    assert_ne!(saved[0].state["part/anim_flap"], 0);
    straight.tick(1_400);

    let mut fresh = build();
    // Move the clock without the mechanisms seeing it, then restore.
    fresh.tick = 1_000;
    fresh.restore_mechanism_states(&track, &saved);
    assert_eq!(fresh.save_mechanism_states(&track), saved);
    fresh.tick(1_400);
    let state = |s: &Sim| {
        s.mechanisms()
            .into_iter()
            .map(|v| {
                (
                    v.state["last_open"],
                    v.state["swings"],
                    v.state["rng/swing"],
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(state(&fresh), state(&straight));
}

#[test]
fn events_report_contacts_rolls_and_mechanisms() {
    let mut sim = ramp_with_flap(7);
    sim.tick(RAMP_10S_TICKS);
    let events = sim.drain_events();
    assert!(sim.events().is_empty());
    let wood = SurfaceId::Kit(Surface::Wood);
    let contacts: Vec<&ContactEvent> = events
        .iter()
        .filter_map(|e| match e {
            SimEvent::Contact(c) => Some(c),
            _ => None,
        })
        .collect();
    assert!(
        contacts
            .iter()
            .any(|c| c.other == ContactOther::Static && c.surface == wood && c.impulse > 0.0)
    );
    assert!(contacts.iter().any(|c| c.other
        == ContactOther::Part {
            instance: InstanceId(CellCoord::new(6, 0, 0)),
            part: 0
        }
        && c.surface == SurfaceId::Kit(Surface::WoodEndGrain)
        && c.impulse > 0.0));

    let rolls: Vec<&RollSample> = events
        .iter()
        .filter_map(|e| match e {
            SimEvent::Roll(r) => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(
        rolls.len() as u64,
        u64::from(RAMP_10S_TICKS) / ROLL_SAMPLE_TICKS
    );
    assert!(
        rolls
            .iter()
            .any(|r| r.surface == Some(wood) && r.speed > 0.1)
    );

    let mech: Vec<&MechEventKind> = events
        .iter()
        .filter_map(|e| match e {
            SimEvent::Mechanism(m) => Some(&m.kind),
            _ => None,
        })
        .collect();
    let enters = mech
        .iter()
        .filter(|k| {
            matches!(
                k,
                MechEventKind::SensorEnter {
                    sensor: 0,
                    marble: 0
                }
            )
        })
        .count();
    assert!(enters > 0);
    assert_eq!(sim.mechanisms()[0].state["passes"], enters as i64);
    assert_eq!(
        mech.iter()
            .filter(|k| matches!(k, MechEventKind::Signal { name: "swing", .. }))
            .count(),
        10
    );

    let ticks: Vec<u64> = events
        .iter()
        .map(|e| match e {
            SimEvent::Mechanism(m) => m.tick,
            SimEvent::Contact(c) => c.tick,
            SimEvent::Roll(r) => r.tick,
        })
        .collect();
    assert!(ticks.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn recording_and_an_empty_framework_leave_the_ramp_golden_unchanged() {
    let mut sim = Sim::ramp_scene();
    sim.set_event_recording(true);
    sim.tick(RAMP_10S_TICKS);
    assert_eq!(
        crate::hash::format_hash(sim.trajectory_hash()),
        "0c9206fc016d9b17"
    );
    assert!(!sim.drain_events().is_empty());
    assert_eq!(sim.mechanism_hash(), Fnv1a64::default().finish());
}

#[test]
fn placed_parts_follow_yaw_and_mirror() {
    // The flap's free edge, 36 mm below the pivot, swings toward local +X as it opens.
    let edge = |yaw: u8, mirror: Mirror| {
        let mut sim = Sim::empty();
        let p = Placement::new(
            piece().def.id.clone(),
            CellCoord::new(0, 0, 0),
            parble_core::Yaw4::new(yaw),
            mirror,
        );
        sim.add_mechanism(piece(), &p, 1, SwingFlap::factory(&p.config))
            .unwrap();
        // The first swing starts at tick 240 and is fully open 24 to 48 ticks later.
        sim.tick(270);
        let part = &sim.mechanisms.instances[&InstanceId(p.anchor)].parts[0];
        let pose = *sim.world.bodies[part.body].position();
        let free = pose.transform_point(Vector::new(0.0, -0.036, 0.0));
        (free - pose.translation, pose.translation)
    };
    let (d, pivot) = edge(0, Mirror::Off);
    assert!(d.x > 0.02 && libm::fabsf(d.z) < 1.0e-6, "{d:?}");
    assert!(libm::fabsf(pivot.y - 0.040) < 1.0e-6, "{pivot:?}");
    // Yaw 1 maps local +X to world −Z (contract §8: (x, z) → (z, −x)).
    let (d, _) = edge(1, Mirror::Off);
    assert!(d.z < -0.02 && libm::fabsf(d.x) < 1.0e-6, "{d:?}");
    // Mirror maps local +X to world −X.
    let (d, _) = edge(0, Mirror::On);
    assert!(d.x < -0.02 && libm::fabsf(d.z) < 1.0e-6, "{d:?}");
}

#[test]
fn registry_builds_track_mechanisms() {
    let kit = KitCatalog::from_sidecars([demo::SIDECAR]).unwrap();
    let mut track = Track::new(7);
    track.placements = vec![at(6, 0, 0)];
    let mut registry = MechanismRegistry::new();
    let bare = crate::track_run::TrackRun::with_mechanisms(&track, &kit, &registry).unwrap();
    assert!(bare.mechanisms().is_empty());
    registry.register(piece().def.id.clone(), SwingFlap::factory);
    let mut run = crate::track_run::TrackRun::with_mechanisms(&track, &kit, &registry).unwrap();
    run.tick(RAMP_10S_TICKS);
    let views = run.mechanisms();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].kind, "fixture_swing_flap");
    assert_eq!(views[0].state["swings"], 10);
}
