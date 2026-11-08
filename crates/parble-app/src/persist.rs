//! Track persistence on the Rust side of the bridge (P3-T3): `loadTrack`, `runTrack` and
//! `exportTrack`. The file format and its migrations live in `parble_core::file`; the browser
//! library, download and file picker live in `web/ui/src/files.ts`.

use crate::bridge::{Command, Event};
use parble_core::ParbleFile;
use parble_core::file;
use parble_sim::hash::format_hash;
use parble_sim::track_run::run_hash;

/// Longest headless run `runTrack` accepts: 120 s of sim time.
pub const MAX_RUN_TICKS: u32 = 240 * 120;

/// What the app must do after a track command, besides sending its event.
#[derive(Debug, PartialEq, Eq)]
pub enum After {
    Nothing,
    /// Restart the live sim on the loaded track.
    Restart,
}

/// The loaded `.parble` file, if any.
#[derive(Debug, Default)]
pub struct Tracks {
    pub loaded: Option<ParbleFile>,
    /// `(ticks, hash)` of the last `runTrack`.
    pub last_run: Option<(u32, u64)>,
}

fn error(command: &str, message: impl Into<String>) -> (Event, After) {
    (
        Event::BridgeError {
            command: command.into(),
            message: message.into(),
        },
        After::Nothing,
    )
}

impl Tracks {
    /// Applies a track command. `Ping` is not a track command and is answered elsewhere.
    pub fn apply(&mut self, cmd: Command) -> Option<(Event, After)> {
        Some(match cmd {
            Command::Ping { .. } => return None,
            Command::LoadTrack { json } => match file::load(&json) {
                Ok(f) => {
                    let ev = Event::TrackLoaded {
                        name: f.metadata.name.clone(),
                        schema: f.track.schema,
                        placements: count(f.track.placements.len()),
                        marbles: count(f.track.hopper.queue.len()),
                    };
                    self.loaded = Some(f);
                    self.last_run = None;
                    (ev, After::Restart)
                }
                Err(e) => error("loadTrack", e.to_string()),
            },
            Command::RunTrack { ticks } => {
                let Some(f) = &self.loaded else {
                    return Some(error("runTrack", "no track loaded"));
                };
                if ticks > MAX_RUN_TICKS {
                    return Some(error(
                        "runTrack",
                        format!("ticks {ticks} exceeds {MAX_RUN_TICKS}"),
                    ));
                }
                let hash = run_hash(&f.track, ticks);
                self.last_run = Some((ticks, hash));
                (
                    Event::RunFinished {
                        ticks,
                        hash: format_hash(hash),
                    },
                    After::Restart,
                )
            }
            Command::ExportTrack {} => match &self.loaded {
                Some(f) => (
                    Event::TrackExported {
                        name: f.metadata.name.clone(),
                        json: f.to_json(),
                    },
                    After::Nothing,
                ),
                None => error("exportTrack", "no track loaded"),
            },
        })
    }
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const FIXTURES: &str = include_str!("../../../web/ui/src/parble-fixtures.json");

    fn fixtures() -> Value {
        serde_json::from_str(FIXTURES).expect("fixture JSON")
    }

    fn ticks() -> u32 {
        u32::try_from(fixtures()["runTicks"].as_u64().expect("runTicks")).unwrap()
    }

    /// The same runs the browser makes in e2e/tests/persistence.spec.mjs: one golden per fixture
    /// for every platform.
    #[test]
    fn every_valid_fixture_runs_to_its_committed_hash() {
        let mut wrong = Vec::new();
        for case in fixtures()["valid"].as_array().unwrap() {
            let f = file::load(&case["file"].to_string()).unwrap();
            let hash = format_hash(run_hash(&f.track, ticks()));
            if hash != case["runHash"].as_str().unwrap() {
                wrong.push(format!("{} runs to {hash}", case["name"]));
            }
        }
        assert!(wrong.is_empty(), "fixture run hashes changed: {wrong:?}");
    }

    fn run(t: &mut Tracks) -> String {
        match t.apply(Command::RunTrack { ticks: ticks() }) {
            Some((Event::RunFinished { hash, .. }, After::Restart)) => hash,
            other => panic!("unexpected {other:?}"),
        }
    }

    fn export(t: &mut Tracks) -> String {
        match t.apply(Command::ExportTrack {}) {
            Some((Event::TrackExported { json, .. }, After::Nothing)) => json,
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn load_run_export_reimport_run_reproduces_hash_and_bytes() {
        let case = &fixtures()["valid"][0];
        let mut t = Tracks::default();
        let loaded = t.apply(Command::LoadTrack {
            json: case["file"].to_string(),
        });
        assert!(matches!(
            loaded,
            Some((
                Event::TrackLoaded {
                    placements: 5,
                    marbles: 4,
                    ..
                },
                After::Restart
            ))
        ));
        let first = run(&mut t);
        assert_eq!(first, case["runHash"].as_str().unwrap());
        let bytes = export(&mut t);
        let mut again = Tracks::default();
        again.apply(Command::LoadTrack {
            json: bytes.clone(),
        });
        assert_eq!(run(&mut again), first);
        assert_eq!(export(&mut again), bytes);
    }

    #[test]
    fn commands_without_a_track_or_with_a_bad_file_are_errors() {
        let mut t = Tracks::default();
        for cmd in [
            Command::RunTrack { ticks: 10 },
            Command::ExportTrack {},
            Command::LoadTrack {
                json: "{\"format\":\"parble\",\"schema\":9}".into(),
            },
        ] {
            assert!(matches!(
                t.apply(cmd),
                Some((Event::BridgeError { .. }, After::Nothing))
            ));
        }
        assert!(t.loaded.is_none());
        assert_eq!(t.apply(Command::Ping { id: 1 }), None);
        t.apply(Command::LoadTrack {
            json: fixtures()["valid"][1]["file"].to_string(),
        });
        assert!(matches!(
            t.apply(Command::RunTrack {
                ticks: MAX_RUN_TICKS + 1
            }),
            Some((Event::BridgeError { .. }, After::Nothing))
        ));
    }
}
