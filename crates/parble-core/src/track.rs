//! The track model: piece placements and the hopper configuration.

use crate::camera_zone::CameraZone;
use crate::contract::KIT_CONTRACT_VERSION;
use crate::grid::{CellCoord, Mirror, Orientation, Yaw4};
use crate::piece::{PieceDef, PieceId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Version of the serialized track schema (`.parble` files).
pub const SCHEMA_VERSION: u32 = 1;

/// Version of the simulation behaviour. Bump whenever a golden trajectory hash changes.
pub const PHYSICS_VERSION: u32 = 6;

/// Per-placement mechanism settings, keyed by setting name. Integer values only, so a
/// config never introduces a float into the deterministic track model.
pub type MechConfig = BTreeMap<String, i64>;

/// One piece placed on the grid: its anchor cell and orientation (contract §9 rule 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub piece: PieceId,
    pub anchor: CellCoord,
    pub yaw: Yaw4,
    pub mirror: Mirror,
    /// Mechanism settings. Omitted from the file when empty, so tracks without
    /// mechanism config serialize exactly as they did before this field existed.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub config: MechConfig,
}

impl Placement {
    pub const fn new(piece: PieceId, anchor: CellCoord, yaw: Yaw4, mirror: Mirror) -> Self {
        Self {
            piece,
            anchor,
            yaw,
            mirror,
            config: BTreeMap::new(),
        }
    }

    pub const fn orientation(&self) -> Orientation {
        Orientation::new(self.yaw, self.mirror)
    }

    /// World cells covered by `def` placed here, sorted by `(x, y, z)`.
    pub fn world_cells(&self, def: &PieceDef) -> Vec<CellCoord> {
        let mut cells: Vec<CellCoord> = def
            .footprint_for(self.orientation())
            .into_iter()
            .map(|c| c + self.anchor)
            .collect();
        cells.sort_unstable();
        cells
    }
}

/// Marble size (contract §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarbleSize {
    Mini,
    Standard,
    Shooter,
}

/// How the hopper releases its queue. Intervals are in sim ticks (no wall-clock input).
///
/// `Sequential` is the default (PRD §6 P2 item 1: one marble in motion at a time). It releases the
/// first marble at tick 0 and each later one only once no marble it released is still moving: every
/// released marble is counted at a finish, lost, off the track on the tray floor, or at rest. The
/// run applies the rule and its thresholds (`parble_sim::track_run`). `One`, `All` and `Interval` release on fixed ticks.
///
/// Deserialized through [`RawRelease`]: serde's internally tagged unit variants ignore extra
/// fields, and a `.parble` file must refuse `{"mode": "one", "ticks": 5}`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase", try_from = "RawRelease")]
pub enum ReleaseMode {
    #[default]
    Sequential,
    One,
    All,
    Interval {
        ticks: u32,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRelease {
    mode: String,
    ticks: Option<u32>,
}

impl TryFrom<RawRelease> for ReleaseMode {
    type Error = String;

    fn try_from(r: RawRelease) -> Result<Self, String> {
        match (r.mode.as_str(), r.ticks) {
            ("sequential", None) => Ok(Self::Sequential),
            ("one", None) => Ok(Self::One),
            ("all", None) => Ok(Self::All),
            ("interval", Some(ticks)) => Ok(Self::Interval { ticks }),
            ("sequential" | "one" | "all", Some(_)) => {
                Err(format!("release mode {:?} takes no ticks", r.mode))
            }
            ("interval", None) => Err("release mode \"interval\" needs ticks".into()),
            (m, _) => Err(format!("unknown release mode {m:?}")),
        }
    }
}

/// Hopper configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HopperConfig {
    /// Index into [`Track::placements`] of the hopper piece, if one is placed.
    pub placement: Option<u32>,
    /// Marbles in release order.
    pub queue: Vec<MarbleSize>,
    pub release: ReleaseMode,
}

/// A track: versions, run seed, placements in a stable order and the hopper config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    pub schema: u32,
    pub physics: u32,
    pub kit_contract: u32,
    pub seed: u64,
    pub placements: Vec<Placement>,
    pub hopper: HopperConfig,
    /// Authored camera zones (PRD §5.6, [`crate::camera_zone`]). Camera data only: the sim never
    /// reads them. Omitted from the file when empty, so tracks without zones serialize exactly as
    /// they did before this field existed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub camera_zones: Vec<CameraZone>,
}

impl Track {
    /// An empty track stamped with the current versions.
    pub fn new(seed: u64) -> Self {
        Self {
            schema: SCHEMA_VERSION,
            physics: PHYSICS_VERSION,
            kit_contract: KIT_CONTRACT_VERSION,
            seed,
            placements: Vec::new(),
            hopper: HopperConfig::default(),
            camera_zones: Vec::new(),
        }
    }
}
