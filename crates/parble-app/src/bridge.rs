//! The JS bridge for the web UI overlay (`web/ui`). The page posts commands as JSON with
//! `Bridge.postCommand` and drains events as a JSON array with `Bridge.drainEvents` once per frame.
//!
//! The wire shapes are `type`-tagged camelCase objects. `web/ui/src/bridge.ts` mirrors them by hand;
//! `web/ui/src/bridge-fixtures.json` holds an example of every command and event (the audio event
//! twice, with and without a camera) and events both sides refuse; this module's tests and the
//! Vitest suite check it, so either side drifting fails a gate.
//!
//! Commands are applied by `pump_bridge`, which runs after the frame's last sim tick, so a command
//! that replaces or runs the sim always lands on a tick boundary. Track commands (P3-T3) are handled
//! in [`crate::persist`].

use crate::audio::AudioFrame;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::VecDeque;

/// Bumped on any incompatible change to [`Command`] or [`Event`].
pub const PROTOCOL: u32 = 3;

/// Outbound bound: with no drainer (the native build, a hidden tab) the oldest events are dropped.
const MAX_EVENTS: usize = 256;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Command {
    /// Answered with [`Event::Pong`] carrying the same id on the next frame.
    Ping { id: u32 },
    /// Load `.parble` text (any supported schema; migrated on load) and restart the live sim on it.
    /// Answered with [`Event::TrackLoaded`] or [`Event::BridgeError`].
    LoadTrack { json: String },
    /// Run the loaded track headless from tick 0 for `ticks` ticks and restart the live sim on it.
    /// Answered with [`Event::RunFinished`] or [`Event::BridgeError`].
    RunTrack { ticks: u32 },
    /// Answered with [`Event::TrackExported`] carrying the loaded track's canonical file text.
    ExportTrack {},
}

fn one() -> f32 {
    1.0
}

#[allow(clippy::trivially_copy_pass_by_ref, clippy::float_cmp)]
fn is_one(v: &f32) -> bool {
    *v == 1.0
}

#[cfg(test)]
impl Command {
    /// Every wire tag, in declaration order.
    pub const TAGS: &'static [&'static str] = &["ping", "loadTrack", "runTrack", "exportTrack"];

    pub fn tag(&self) -> &'static str {
        match self {
            Command::Ping { .. } => "ping",
            Command::LoadTrack { .. } => "loadTrack",
            Command::RunTrack { .. } => "runTrack",
            Command::ExportTrack {} => "exportTrack",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Event {
    /// Queued once at startup.
    Ready {
        protocol: u32,
    },
    /// The latest frame only: a newer frame replaces an undrained one. `paused` and `timeScale`
    /// are the playback state ([`crate::playback`]), sent only when not the defaults (running, 1).
    Frame {
        tick: u64,
        fps: f32,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        paused: bool,
        #[serde(default = "one", skip_serializing_if = "is_one")]
        time_scale: f32,
    },
    Pong {
        id: u32,
    },
    TrackLoaded {
        name: String,
        schema: u32,
        placements: u32,
        marbles: u32,
    },
    /// `hash` is the trajectory hash as 16 lowercase hex digits (a u64 does not fit a JS number).
    RunFinished {
        ticks: u32,
        hash: String,
    },
    TrackExported {
        name: String,
        json: String,
    },
    /// A command that could not be applied, with the command's wire tag.
    BridgeError {
        command: String,
        message: String,
    },
    /// The audio telemetry since the last drain (P4-T3, [`crate::audio`]). At most one is queued:
    /// a newer frame absorbs an undrained one ([`AudioFrame::absorb_older`]).
    AudioFrame(AudioFrame),
    /// A checkpoint pass: the checkpoint's `split` signal, at `tick` (sim ticks since the run
    /// started, 240 Hz), by the checkpoint anchored at `instance`, for `marble` (its `camera_cue`
    /// of the same tick). One per pass; older drainers skip the unknown tag, so it needs no
    /// protocol bump.
    Split {
        tick: u64,
        instance: [i32; 3],
        marble: u32,
    },
    /// The renderer was rebuilt after a lost GPU device; `count` recoveries so far
    /// ([`crate::gpu_recovery`]). The sim and the track are untouched. Older drainers skip the
    /// unknown tag, so it needs no protocol bump.
    GpuRecovered {
        count: u32,
    },
}

/// One [`Event::Split`] per checkpoint `split` signal in `events`, in order.
pub fn splits(events: &[parble_sim::mechanism::SimEvent]) -> Vec<Event> {
    use parble_sim::mechanism::{MechEventKind, SimEvent};
    let signal = |e: &SimEvent, want: &str| match e {
        SimEvent::Mechanism(m) => match &m.kind {
            MechEventKind::Signal { name, value } if *name == want => {
                Some((m.tick, m.instance, *value))
            }
            _ => None,
        },
        _ => None,
    };
    events
        .iter()
        .filter_map(|e| signal(e, "split"))
        .map(|(tick, id, _)| {
            let marble = events
                .iter()
                .filter_map(|e| signal(e, "camera_cue"))
                .find(|&(t, i, _)| t == tick && i == id)
                .map_or(u32::MAX, |(_, _, m)| u32::try_from(m).unwrap_or(u32::MAX));
            let c = id.0;
            Event::Split {
                tick,
                instance: [c.x, c.y, c.z],
                marble,
            }
        })
        .collect()
}

#[cfg(test)]
impl Event {
    /// Every wire tag, in declaration order.
    pub const TAGS: &'static [&'static str] = &[
        "ready",
        "frame",
        "pong",
        "trackLoaded",
        "runFinished",
        "trackExported",
        "bridgeError",
        "audioFrame",
        "split",
        "gpuRecovered",
    ];

    pub fn tag(&self) -> &'static str {
        match self {
            Event::Ready { .. } => "ready",
            Event::Frame { .. } => "frame",
            Event::Pong { .. } => "pong",
            Event::TrackLoaded { .. } => "trackLoaded",
            Event::RunFinished { .. } => "runFinished",
            Event::TrackExported { .. } => "trackExported",
            Event::BridgeError { .. } => "bridgeError",
            Event::AudioFrame(_) => "audioFrame",
            Event::Split { .. } => "split",
            Event::GpuRecovered { .. } => "gpuRecovered",
        }
    }
}

#[derive(Default)]
struct Queues {
    inbox: VecDeque<Command>,
    outbox: VecDeque<Event>,
}

thread_local! {
    static QUEUES: RefCell<Queues> = RefCell::new(Queues::default());
}

/// Parses and queues one command for the next frame.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn post_command(json: &str) -> Result<(), String> {
    let cmd: Command =
        serde_json::from_str(json).map_err(|e| format!("bad bridge command: {e}"))?;
    QUEUES.with_borrow_mut(|q| q.inbox.push_back(cmd));
    Ok(())
}

/// Takes the queued commands, oldest first.
pub fn take_commands() -> Vec<Command> {
    QUEUES.with_borrow_mut(|q| q.inbox.drain(..).collect())
}

pub fn push_event(mut event: Event) {
    QUEUES.with_borrow_mut(|q| {
        if let Event::AudioFrame(newer) = &mut event
            && let Some(at) = q
                .outbox
                .iter()
                .position(|e| matches!(e, Event::AudioFrame(_)))
            && let Some(Event::AudioFrame(older)) = q.outbox.remove(at)
        {
            newer.absorb_older(older);
        }
        if matches!(event, Event::Frame { .. })
            && let Some(at) = q
                .outbox
                .iter()
                .position(|e| matches!(e, Event::Frame { .. }))
        {
            q.outbox.remove(at);
        }
        if q.outbox.len() >= MAX_EVENTS {
            q.outbox.pop_front();
        }
        q.outbox.push_back(event);
    });
}

/// Takes the queued events, oldest first, as a JSON array.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn drain_events_json() -> String {
    let events: Vec<Event> = QUEUES.with_borrow_mut(|q| q.outbox.drain(..).collect());
    serde_json::to_string(&events).expect("bridge events serialize")
}

#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::prelude::*;

    /// `new wasmBindings.Bridge()`; every instance shares the one pair of queues.
    #[wasm_bindgen]
    #[derive(Default)]
    pub struct Bridge {}

    #[wasm_bindgen]
    impl Bridge {
        #[wasm_bindgen(constructor)]
        pub fn new() -> Bridge {
            Bridge {}
        }

        /// Throws on JSON that is not a known command.
        #[wasm_bindgen(js_name = postCommand)]
        pub fn post_command(&self, json: &str) -> Result<(), JsError> {
            super::post_command(json).map_err(|e| JsError::new(&e))
        }

        /// The events queued since the last drain, as a JSON array.
        #[wasm_bindgen(js_name = drainEvents)]
        pub fn drain_events(&self) -> String {
            super::drain_events_json()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const FIXTURES: &str = include_str!("../../../web/ui/src/bridge-fixtures.json");

    fn fixture(key: &str) -> Vec<Value> {
        let all: Value = serde_json::from_str(FIXTURES).expect("fixture JSON");
        all[key].as_array().expect("fixture array").clone()
    }

    #[test]
    fn every_command_fixture_round_trips_and_every_variant_has_one() {
        let mut tags = Vec::new();
        for v in fixture("commands") {
            let cmd: Command = serde_json::from_value(v.clone()).expect("command fixture parses");
            assert_eq!(serde_json::to_value(&cmd).unwrap(), v, "command round trip");
            tags.push(cmd.tag());
        }
        assert_eq!(tags, Command::TAGS);
    }

    #[test]
    fn every_event_fixture_round_trips_and_every_variant_has_one() {
        let mut tags = Vec::new();
        for v in fixture("events") {
            let ev: Event = serde_json::from_value(v.clone()).expect("event fixture parses");
            assert_eq!(serde_json::to_value(&ev).unwrap(), v, "event round trip");
            tags.push(ev.tag());
        }
        tags.dedup();
        assert_eq!(tags, Event::TAGS);
    }

    #[test]
    fn every_rejected_event_fixture_is_refused() {
        for v in fixture("rejectedEvents") {
            assert!(
                serde_json::from_value::<Event>(v.clone()).is_err(),
                "accepted {v}"
            );
        }
    }

    fn audio(tick: u64, marble: u32, impulse: f32, mechanisms: u64) -> Event {
        Event::AudioFrame(AudioFrame {
            tick,
            contacts: vec![crate::audio::AudioContact {
                marble,
                kind: crate::audio::WireMarble::Steel,
                size: crate::audio::WireSize::Standard,
                surface: crate::audio::WireSurface::Wood,
                other_kind: None,
                impulse,
                count: 1,
                position: [0.0; 3],
            }],
            mechanisms: (0..mechanisms)
                .map(|t| crate::audio::AudioMechanism::Signal {
                    tick: tick * 100 + t,
                    instance: [0; 3],
                    piece: "mech_hopper".into(),
                    name: "release".into(),
                    value: 1,
                })
                .collect(),
            ..AudioFrame::default()
        })
    }

    #[test]
    fn a_newer_audio_frame_absorbs_an_undrained_one() {
        push_event(audio(4, 0, 0.5, 1));
        push_event(Event::Pong { id: 1 });
        push_event(audio(8, 0, 0.25, 1));
        push_event(audio(12, 1, 0.125, 0));
        let out: Vec<Event> = serde_json::from_str(&drain_events_json()).unwrap();
        assert_eq!(out.len(), 2, "{out:?}");
        assert_eq!(out[0], Event::Pong { id: 1 });
        let Event::AudioFrame(f) = &out[1] else {
            panic!("{out:?}")
        };
        assert_eq!(f.tick, 12);
        assert_eq!(
            f.contacts
                .iter()
                .map(|c| (c.marble, c.impulse, c.count))
                .collect::<Vec<_>>(),
            vec![(0, 0.5, 2), (1, 0.125, 1)]
        );
        assert_eq!(f.mechanisms.len(), 2);
        assert_eq!(drain_events_json(), "[]");
    }

    #[test]
    fn audio_frames_never_evict_other_events() {
        push_event(Event::Ready { protocol: PROTOCOL });
        for t in 0..(MAX_EVENTS as u64 * 2) {
            push_event(audio(t, 0, 0.5, 1));
        }
        let out: Vec<Event> = serde_json::from_str(&drain_events_json()).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], Event::Ready { protocol: PROTOCOL });
        let Event::AudioFrame(f) = &out[1] else {
            panic!("{out:?}")
        };
        assert_eq!(f.contacts[0].count, MAX_EVENTS as u32 * 2);
        assert_eq!(f.mechanisms.len(), crate::audio::MAX_AUDIO_MECHANISMS);
    }

    #[test]
    fn every_rejected_fixture_is_refused() {
        for v in fixture("rejectedCommands") {
            assert!(post_command(&v.to_string()).is_err(), "accepted {v}");
        }
        assert!(post_command("not json").is_err());
        assert!(take_commands().is_empty());
    }

    #[test]
    fn protocol_matches_the_fixture() {
        let all: Value = serde_json::from_str(FIXTURES).unwrap();
        assert_eq!(all["protocol"], PROTOCOL);
    }

    #[test]
    fn commands_queue_in_order() {
        post_command(r#"{"type":"ping","id":1}"#).unwrap();
        post_command(r#"{"type":"ping","id":2}"#).unwrap();
        assert_eq!(
            take_commands(),
            vec![Command::Ping { id: 1 }, Command::Ping { id: 2 }]
        );
        assert!(take_commands().is_empty());
    }

    #[test]
    fn a_newer_frame_replaces_an_undrained_one() {
        push_event(Event::Ready { protocol: PROTOCOL });
        push_event(Event::Frame {
            tick: 1,
            fps: 60.0,
            paused: false,
            time_scale: 1.0,
        });
        push_event(Event::Pong { id: 7 });
        push_event(Event::Frame {
            tick: 2,
            fps: 60.0,
            paused: false,
            time_scale: 1.0,
        });
        let out: Vec<Event> = serde_json::from_str(&drain_events_json()).unwrap();
        assert_eq!(
            out,
            vec![
                Event::Ready { protocol: PROTOCOL },
                Event::Pong { id: 7 },
                Event::Frame {
                    tick: 2,
                    fps: 60.0,
                    paused: false,
                    time_scale: 1.0
                },
            ]
        );
        assert_eq!(drain_events_json(), "[]");
    }

    #[test]
    fn a_checkpoint_pass_is_one_split_with_its_marble() {
        use parble_sim::Sim;
        use parble_sim::behaviour::CHECKPOINT;
        let mut sim = Sim::mech_showcase_scene();
        sim.set_event_recording(true);
        sim.tick(parble_sim::mech_showcase::CHECKPOINT_LIT_SHOT_TICK as u32);
        let out = splits(&sim.drain_events());
        let cell = parble_sim::mech_showcase::CHECKPOINT_CELL;
        assert_eq!(
            sim.mechanism_piece(parble_sim::mechanism::InstanceId(
                parble_core::CellCoord::new(cell[0], cell[1], cell[2])
            ))
            .map(|p| p.as_str()),
            Some(CHECKPOINT)
        );
        let [
            Event::Split {
                tick,
                instance,
                marble,
            },
        ] = out.as_slice()
        else {
            panic!("{out:?}")
        };
        assert_eq!((*instance, *marble), (cell, 4));
        assert!(*tick > 0 && *tick < parble_sim::mech_showcase::CHECKPOINT_LIT_SHOT_TICK);
    }

    #[test]
    fn the_outbox_is_bounded() {
        for id in 0..(MAX_EVENTS as u32 + 10) {
            push_event(Event::Pong { id });
        }
        let out: Vec<Event> = serde_json::from_str(&drain_events_json()).unwrap();
        assert_eq!(out.len(), MAX_EVENTS);
        assert_eq!(out[0], Event::Pong { id: 10 });
    }
}
