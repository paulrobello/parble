//! The `.parble` track file: JSON schema v1 and its migration chain.
//!
//! A file is one JSON object:
//!
//! ```json
//! { "format": "parble", "schema": 1, "physics": 1, "kit_contract": 1, "seed": 7,
//!   "placements": [...], "hopper": {...}, "mechanisms": [...], "metadata": {...},
//!   "camera_zones": [...] }
//! ```
//!
//! The version, seed, placement, hopper and camera zone fields are [`Track`]'s own. `camera_zones`
//! ([`crate::camera_zone`]) is optional and left out when empty. `web/ui/src/parble.ts`
//! mirrors the shape by hand; `web/ui/src/parble-fixtures.json` holds valid and rejected files that
//! both this module's tests and the Vitest suite check.
//!
//! Every integer in a file must survive a JavaScript `number`: seeds, mechanism config and state
//! values are limited to ±[`MAX_SAFE_INTEGER`] so the browser library never rounds a track.
//!
//! Loading goes through [`load`]: the raw JSON is read as a [`serde_json::Value`], its `schema` is
//! checked, the [`MIGRATIONS`] chain lifts it to [`SCHEMA_VERSION`], then it is deserialized and
//! validated. A file newer than [`SCHEMA_VERSION`] is refused.

use crate::track::{MechConfig, SCHEMA_VERSION, Track};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The `format` tag of every `.parble` file.
pub const FORMAT: &str = "parble";

/// `Number.MAX_SAFE_INTEGER`: the largest integer a JavaScript `number` holds exactly.
pub const MAX_SAFE_INTEGER: i64 = (1 << 53) - 1;

/// Longest metadata `name` or `author`, in characters.
pub const MAX_NAME_CHARS: usize = 120;

/// Longest metadata `description`, in characters.
pub const MAX_DESCRIPTION_CHARS: usize = 2_000;

/// Saved runtime state of one mechanism placement (toggle position, splitter phase, …), keyed by
/// state name. Integer values only, like [`MechConfig`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MechanismState {
    /// Index into [`Track::placements`].
    pub placement: u32,
    pub state: MechConfig,
}

/// Descriptive fields. None of them reach the sim.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub author: String,
}

/// A loaded `.parble` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParbleFile {
    pub track: Track,
    /// Sorted by placement, at most one per placement.
    pub mechanisms: Vec<MechanismState>,
    pub metadata: Metadata,
}

/// The wire shape of schema v1.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireV1 {
    format: String,
    schema: u32,
    physics: u32,
    kit_contract: u32,
    seed: u64,
    placements: Vec<crate::track::Placement>,
    hopper: crate::track::HopperConfig,
    mechanisms: Vec<MechanismState>,
    metadata: Metadata,
    /// Optional since it was added: a file without zones has no `camera_zones` key, so every file
    /// written before zones existed loads and saves byte for byte, still as schema 1.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    camera_zones: Vec<crate::camera_zone::CameraZone>,
}

/// Why a file was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileError {
    /// Not JSON, or JSON that does not match the schema.
    Parse(String),
    /// `format` is missing or not `"parble"`.
    NotParble,
    /// `schema` is missing or not an integer.
    NoSchema,
    /// Written by a newer parble.
    TooNew { schema: u32 },
    /// No migrator lifts this version.
    NoMigration { from: u32 },
    /// Parsed but breaks a rule the types cannot express.
    Invalid(String),
}

impl core::fmt::Display for FileError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "not a valid .parble file: {e}"),
            Self::NotParble => write!(f, "not a .parble file (format must be \"parble\")"),
            Self::NoSchema => write!(f, "missing or non-integer schema version"),
            Self::TooNew { schema } => write!(
                f,
                "schema {schema} is newer than this build supports ({SCHEMA_VERSION})"
            ),
            Self::NoMigration { from } => write!(f, "no migration from schema {from}"),
            Self::Invalid(e) => write!(f, "invalid .parble file: {e}"),
        }
    }
}

/// One step of the migration chain: rewrites a raw schema-`from` document into schema `from + 1`.
/// The step sets nothing itself; [`migrate`] stamps the new `schema` after it returns.
pub struct Migration {
    pub from: u32,
    pub apply: fn(Value) -> Result<Value, FileError>,
}

/// The production chain, ascending by `from`, one step per version below [`SCHEMA_VERSION`].
/// Schema 1 is the first version, so the chain is empty. A v2 bump appends
/// `Migration { from: 1, apply: v1_to_v2 }` here (see the tests for the shape).
pub const MIGRATIONS: &[Migration] = &[];

/// Lifts `doc` from its `schema` to `target` through `chain`. Refuses a document newer than
/// `target` and a version with no step.
pub fn migrate(mut doc: Value, target: u32, chain: &[Migration]) -> Result<Value, FileError> {
    let mut version = schema_of(&doc)?;
    if version > target {
        return Err(FileError::TooNew { schema: version });
    }
    while version < target {
        let step = chain
            .iter()
            .find(|m| m.from == version)
            .ok_or(FileError::NoMigration { from: version })?;
        doc = (step.apply)(doc)?;
        version += 1;
        if let Some(obj) = doc.as_object_mut() {
            obj.insert("schema".into(), Value::from(version));
        }
    }
    Ok(doc)
}

fn schema_of(doc: &Value) -> Result<u32, FileError> {
    let obj = doc
        .as_object()
        .ok_or_else(|| FileError::Parse("top level is not an object".into()))?;
    if obj.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(FileError::NotParble);
    }
    obj.get("schema")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or(FileError::NoSchema)
}

/// Parses `.parble` JSON text of any supported schema.
pub fn load(json: &str) -> Result<ParbleFile, FileError> {
    let doc: Value = serde_json::from_str(json).map_err(|e| FileError::Parse(e.to_string()))?;
    load_value(doc)
}

/// [`load`] for an already parsed document.
pub fn load_value(doc: Value) -> Result<ParbleFile, FileError> {
    let doc = migrate(doc, SCHEMA_VERSION, MIGRATIONS)?;
    let wire: WireV1 = serde_json::from_value(doc).map_err(|e| FileError::Parse(e.to_string()))?;
    let file = ParbleFile {
        track: Track {
            schema: wire.schema,
            physics: wire.physics,
            kit_contract: wire.kit_contract,
            seed: wire.seed,
            placements: wire.placements,
            hopper: wire.hopper,
            camera_zones: wire.camera_zones,
        },
        mechanisms: wire.mechanisms,
        metadata: wire.metadata,
    };
    file.validate()?;
    Ok(file)
}

fn safe(v: i64) -> bool {
    (-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(&v)
}

impl ParbleFile {
    /// A new file for `track`, named `name`.
    pub fn new(track: Track, name: &str) -> Self {
        Self {
            track,
            mechanisms: Vec::new(),
            metadata: Metadata {
                name: name.to_owned(),
                ..Metadata::default()
            },
        }
    }

    /// The rules beyond the types. Run by [`load`]; call it before saving a hand-built file.
    pub fn validate(&self) -> Result<(), FileError> {
        let bad = |m: String| Err(FileError::Invalid(m));
        let t = &self.track;
        let n = t.placements.len();
        if t.seed > MAX_SAFE_INTEGER as u64 {
            return bad(format!("seed {} exceeds 2^53 - 1", t.seed));
        }
        for (i, p) in t.placements.iter().enumerate() {
            if let Some((k, v)) = p.config.iter().find(|(_, v)| !safe(**v)) {
                return bad(format!(
                    "placement {i} config {k:?} = {v} exceeds ±(2^53 - 1)"
                ));
            }
        }
        if let Some(h) = t.hopper.placement
            && h as usize >= n
        {
            return bad(format!(
                "hopper placement {h} is out of range ({n} placements)"
            ));
        }
        if let crate::track::ReleaseMode::Interval { ticks: 0 } = t.hopper.release {
            return bad("release interval must be at least 1 tick".into());
        }
        let mut last: Option<u32> = None;
        for m in &self.mechanisms {
            if m.placement as usize >= n {
                return bad(format!(
                    "mechanism placement {} is out of range",
                    m.placement
                ));
            }
            if last.is_some_and(|l| l >= m.placement) {
                return bad("mechanisms must be sorted by placement without repeats".into());
            }
            last = Some(m.placement);
            if let Some((k, v)) = m.state.iter().find(|(_, v)| !safe(**v)) {
                return bad(format!("mechanism state {k:?} = {v} exceeds ±(2^53 - 1)"));
            }
        }
        if t.camera_zones.len() > crate::camera_zone::MAX_ZONES {
            return bad(format!(
                "at most {} camera zones",
                crate::camera_zone::MAX_ZONES
            ));
        }
        for (i, z) in t.camera_zones.iter().enumerate() {
            z.check()
                .or_else(|e| bad(format!("camera zone {i}: {e}")))?;
        }
        let md = &self.metadata;
        if md.name.chars().count() > MAX_NAME_CHARS || md.author.chars().count() > MAX_NAME_CHARS {
            return bad(format!(
                "name and author are limited to {MAX_NAME_CHARS} characters"
            ));
        }
        if md.description.chars().count() > MAX_DESCRIPTION_CHARS {
            return bad(format!(
                "description is limited to {MAX_DESCRIPTION_CHARS} characters"
            ));
        }
        Ok(())
    }

    /// The file as a JSON value, stamped with the current [`FORMAT`].
    pub fn to_value(&self) -> Value {
        let t = &self.track;
        serde_json::to_value(WireV1 {
            format: FORMAT.into(),
            schema: t.schema,
            physics: t.physics,
            kit_contract: t.kit_contract,
            seed: t.seed,
            placements: t.placements.clone(),
            hopper: t.hopper.clone(),
            mechanisms: self.mechanisms.clone(),
            metadata: self.metadata.clone(),
            camera_zones: t.camera_zones.clone(),
        })
        .expect(".parble files serialize")
    }

    /// Canonical file text: pretty JSON with a trailing newline. Saving a loaded file reproduces
    /// these bytes exactly.
    pub fn to_json(&self) -> String {
        let mut s =
            serde_json::to_string_pretty(&self.to_value()).expect(".parble files serialize");
        s.push('\n');
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::PHYSICS_VERSION;

    const FIXTURES: &str = include_str!("../../../web/ui/src/parble-fixtures.json");

    fn fixtures() -> Value {
        serde_json::from_str(FIXTURES).expect("fixture JSON")
    }

    fn list(key: &str) -> Vec<Value> {
        fixtures()[key].as_array().expect("fixture array").clone()
    }

    #[test]
    fn versions_match_the_fixture() {
        let all = fixtures();
        assert_eq!(all["schema"], SCHEMA_VERSION);
        assert_eq!(all["physics"], PHYSICS_VERSION);
        assert_eq!(all["maxSafeInteger"], MAX_SAFE_INTEGER);
    }

    #[test]
    fn every_valid_fixture_loads_and_round_trips() {
        let valid = list("valid");
        assert!(!valid.is_empty());
        for case in valid {
            let doc = &case["file"];
            let file = load(&doc.to_string()).unwrap_or_else(|e| panic!("{}: {e}", case["name"]));
            assert_eq!(&file.to_value(), doc, "{} round trip", case["name"]);
            let again = load(&file.to_json()).expect("canonical text reloads");
            assert_eq!(again, file);
            assert_eq!(
                again.to_json(),
                file.to_json(),
                "canonical bytes are stable"
            );
        }
    }

    #[test]
    fn every_rejected_fixture_is_refused() {
        let rejected = list("rejected");
        assert!(!rejected.is_empty());
        for case in rejected {
            assert!(
                load(&case["file"].to_string()).is_err(),
                "accepted {}",
                case["why"]
            );
        }
        assert!(load("not json").is_err());
    }

    #[test]
    fn new_tracks_default_to_sequential_and_every_mode_saves_and_reloads() {
        use crate::track::ReleaseMode;
        assert_eq!(Track::new(0).hopper.release, ReleaseMode::Sequential);
        for mode in [
            ReleaseMode::Sequential,
            ReleaseMode::One,
            ReleaseMode::All,
            ReleaseMode::Interval { ticks: 120 },
        ] {
            let mut t = Track::new(5);
            t.hopper.queue = vec![crate::track::MarbleSize::Standard; 3];
            t.hopper.release = mode;
            let f = ParbleFile::new(t, "modes");
            let back = load(&f.to_json()).expect("saved file reloads");
            assert_eq!(back.track.hopper.release, mode);
            assert_eq!(back, f);
        }
        let doc = list("valid")
            .into_iter()
            .find(|c| c["name"] == "release-sequential")
            .expect("sequential fixture")["file"]
            .clone();
        assert_eq!(
            load(&doc.to_string()).unwrap().track.hopper.release,
            ReleaseMode::Sequential
        );
        let one = list("valid")[1]["file"].clone();
        assert_eq!(one["hopper"]["release"]["mode"], "one");
        assert_eq!(
            load(&one.to_string()).unwrap().track.hopper.release,
            ReleaseMode::One,
            "a saved mode is kept, not replaced by the default"
        );
    }

    /// The splitter's mode (`mode`: 0 Random, 1 Alternate) and first side (`first`) are placement
    /// config, so they save and reload in schema 1 byte for byte, and a splitter saved before the
    /// mode existed loads with no `mode` key (Random).
    #[test]
    fn splitter_mode_saves_and_reloads_and_old_files_load_as_random() {
        use crate::{CellCoord, Mirror, PieceId, Placement, Yaw4};
        let splitter = || {
            Placement::new(
                PieceId::new("mech_splitter_l3").unwrap(),
                CellCoord::new(1, 0, 0),
                Yaw4::new(1),
                Mirror::Off,
            )
        };
        let mut t = Track::new(7);
        let mut alt = splitter();
        alt.config.insert("mode".into(), 1);
        alt.config.insert("first".into(), 1);
        t.placements = vec![alt, splitter()];
        let f = ParbleFile::new(t, "splitter modes");
        let text = f.to_json();
        let back = load(&text).expect("saved file reloads");
        assert_eq!(back, f);
        assert_eq!(back.to_json(), text, "canonical bytes are stable");
        assert_eq!(back.track.schema, 1);
        let cfg = &back.track.placements[0].config;
        assert_eq!((cfg.get("mode"), cfg.get("first")), (Some(&1), Some(&1)));
        // The default splitter writes no config at all, as before the mode existed.
        let doc: Value = serde_json::from_str(&text).unwrap();
        assert!(doc["placements"][1].get("config").is_none());
        assert!(!back.track.placements[1].config.contains_key("mode"));
    }

    #[test]
    fn v1_loads_through_the_production_chain() {
        assert!(MIGRATIONS.is_empty(), "schema 1 needs no steps");
        let doc = list("valid")[0]["file"].clone();
        assert_eq!(migrate(doc.clone(), SCHEMA_VERSION, MIGRATIONS), Ok(doc));
    }

    fn identity(doc: Value) -> Result<Value, FileError> {
        Ok(doc)
    }

    #[test]
    fn a_registered_step_lifts_v1_to_v2_and_stamps_the_schema() {
        let chain = [Migration {
            from: 1,
            apply: identity,
        }];
        let doc = list("valid")[0]["file"].clone();
        let out = migrate(doc.clone(), 2, &chain).unwrap();
        assert_eq!(out["schema"], 2);
        let mut expect = doc;
        expect["schema"] = Value::from(2);
        assert_eq!(out, expect);
    }

    #[test]
    fn the_chain_refuses_newer_and_unbridged_versions() {
        let mut doc = list("valid")[0]["file"].clone();
        doc["schema"] = Value::from(SCHEMA_VERSION + 1);
        assert_eq!(
            load(&doc.to_string()),
            Err(FileError::TooNew {
                schema: SCHEMA_VERSION + 1
            })
        );
        doc["schema"] = Value::from(0);
        assert_eq!(
            migrate(doc, SCHEMA_VERSION, MIGRATIONS),
            Err(FileError::NoMigration { from: 0 })
        );
    }
}
