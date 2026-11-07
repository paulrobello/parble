//! The track model: piece placements and the hopper configuration.

use crate::contract::KIT_CONTRACT_VERSION;
use crate::grid::{CellCoord, Mirror, Orientation, Yaw4};
use crate::piece::{PieceDef, PieceId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Version of the serialized track schema (`.parble` files).
pub const SCHEMA_VERSION: u32 = 1;

/// Version of the simulation behaviour. Bump whenever a golden trajectory hash changes.
pub const PHYSICS_VERSION: u32 = 1;

/// Per-placement mechanism settings, keyed by setting name. Integer values only, so a
/// config never introduces a float into the deterministic track model.
pub type MechConfig = BTreeMap<String, i64>;

/// One piece placed on the grid: its anchor cell and orientation (contract §9 rule 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum ReleaseMode {
    One,
    All,
    Interval { ticks: u32 },
}

/// Hopper configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HopperConfig {
    /// Index into [`Track::placements`] of the hopper piece, if one is placed.
    pub placement: Option<u32>,
    /// Marbles in release order.
    pub queue: Vec<MarbleSize>,
    pub release: ReleaseMode,
}

impl Default for HopperConfig {
    fn default() -> Self {
        Self {
            placement: None,
            queue: Vec::new(),
            release: ReleaseMode::One,
        }
    }
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
        }
    }
}
