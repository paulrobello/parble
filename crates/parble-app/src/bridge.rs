//! The JS bridge for the web UI overlay (`web/ui`). The page posts commands as JSON with
//! `Bridge.postCommand` and drains events as a JSON array with `Bridge.drainEvents` once per frame.
//!
//! The wire shapes are `type`-tagged camelCase objects. `web/ui/src/bridge.ts` mirrors them by hand;
//! `web/ui/src/bridge-fixtures.json` holds one example of every command and event, and both this
//! module's tests and the Vitest suite check it, so either side drifting fails a gate.
//!
//! Commands are UI input only: nothing posted here reaches the sim.

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::VecDeque;

/// Bumped on any incompatible change to [`Command`] or [`Event`].
pub const PROTOCOL: u32 = 1;

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
}

#[cfg(test)]
impl Command {
    /// Every wire tag, in declaration order.
    pub const TAGS: &'static [&'static str] = &["ping"];

    pub fn tag(&self) -> &'static str {
        match self {
            Command::Ping { .. } => "ping",
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
    /// The latest frame only: a newer frame replaces an undrained one.
    Frame {
        tick: u64,
        fps: f32,
    },
    Pong {
        id: u32,
    },
}

#[cfg(test)]
impl Event {
    /// Every wire tag, in declaration order.
    pub const TAGS: &'static [&'static str] = &["ready", "frame", "pong"];

    pub fn tag(&self) -> &'static str {
        match self {
            Event::Ready { .. } => "ready",
            Event::Frame { .. } => "frame",
            Event::Pong { .. } => "pong",
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

pub fn push_event(event: Event) {
    QUEUES.with_borrow_mut(|q| {
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
        assert_eq!(tags, Event::TAGS);
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
        push_event(Event::Frame { tick: 1, fps: 60.0 });
        push_event(Event::Pong { id: 7 });
        push_event(Event::Frame { tick: 2, fps: 60.0 });
        let out: Vec<Event> = serde_json::from_str(&drain_events_json()).unwrap();
        assert_eq!(
            out,
            vec![
                Event::Ready { protocol: PROTOCOL },
                Event::Pong { id: 7 },
                Event::Frame { tick: 2, fps: 60.0 },
            ]
        );
        assert_eq!(drain_events_json(), "[]");
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
