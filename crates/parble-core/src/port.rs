//! Port levels and ports (kit contract §3, §4, §5.5).

use crate::contract::{CELL_UM, LEVEL_COUNT, LEVEL_STEP_UM};
use crate::grid::{CellCoord, Orientation, PortFace};
use serde::{Deserialize, Serialize};

/// Height of a side port's channel floor above its cell's bottom face, in 15 mm steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum PortLevel {
    L0,
    L1,
    L2,
    L3,
}

impl PortLevel {
    pub const ALL: [Self; 4] = [Self::L0, Self::L1, Self::L2, Self::L3];

    pub const fn index(self) -> i32 {
        self as i32
    }

    /// Floor height above the cell's bottom face.
    pub const fn floor_um(self) -> i32 {
        self.index() * LEVEL_STEP_UM
    }

    /// Absolute floor height for this level in cell layer `layer`.
    pub const fn absolute_floor_um(self, layer: i32) -> i32 {
        layer * CELL_UM + self.floor_um()
    }

    /// Move `delta` levels from this level in `layer`, carrying across cell layers
    /// (one below `L0` of layer `y` is `L3` of layer `y − 1`). Returns `(layer, level)`.
    pub const fn offset(self, layer: i32, delta: i32) -> (i32, Self) {
        let total = layer * LEVEL_COUNT + self.index() + delta;
        let level = match total.rem_euclid(LEVEL_COUNT) {
            0 => Self::L0,
            1 => Self::L1,
            2 => Self::L2,
            _ => Self::L3,
        };
        (total.div_euclid(LEVEL_COUNT), level)
    }
}

/// Error for a serialized level outside `0..=3`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidLevel(pub u8);

impl core::fmt::Display for InvalidLevel {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "port level must be 0..=3, got {}", self.0)
    }
}

impl TryFrom<u8> for PortLevel {
    type Error = InvalidLevel;

    fn try_from(v: u8) -> Result<Self, InvalidLevel> {
        match v {
            0 => Ok(Self::L0),
            1 => Ok(Self::L1),
            2 => Ok(Self::L2),
            3 => Ok(Self::L3),
            _ => Err(InvalidLevel(v)),
        }
    }
}

impl From<PortLevel> for u8 {
    fn from(l: PortLevel) -> Self {
        l as u8
    }
}

/// Port class (contract §5.5). `Channel` mates `Channel`, `Socket` mates `Spigot`, `Drop` mates `Drop`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortClass {
    Channel,
    Socket,
    Spigot,
    Drop,
}

impl PortClass {
    pub const fn mates_with(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::Channel, Self::Channel)
                | (Self::Socket, Self::Spigot)
                | (Self::Spigot, Self::Socket)
                | (Self::Drop, Self::Drop)
        )
    }
}

/// Marble flow direction through a port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortFlow {
    In,
    Out,
    Both,
}

/// A piece port in the piece-local frame. Side ports carry a level, drop ports
/// (on `Top`/`Bottom`) do not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Port {
    pub cell: CellCoord,
    pub face: PortFace,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<PortLevel>,
    pub class: PortClass,
    pub flow: PortFlow,
    /// 0 except for socket and spigot ports ([`crate::contract::SOCKET_PORT_INSET_UM`]).
    #[serde(default)]
    pub inset_um: i32,
}

/// Why a [`Port`] is not well formed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortError {
    SideWithoutLevel,
    DropWithLevel,
    DropClassOnSideFace,
    SideClassOnDropFace,
}

impl Port {
    /// A horizontal side port.
    pub const fn side(
        cell: CellCoord,
        face: PortFace,
        level: PortLevel,
        class: PortClass,
        flow: PortFlow,
    ) -> Self {
        Self {
            cell,
            face,
            level: Some(level),
            class,
            flow,
            inset_um: 0,
        }
    }

    /// A vertical drop port on `Top` or `Bottom`.
    pub const fn drop(cell: CellCoord, face: PortFace, flow: PortFlow) -> Self {
        Self {
            cell,
            face,
            level: None,
            class: PortClass::Drop,
            flow,
            inset_um: 0,
        }
    }

    pub fn validate(&self) -> Result<(), PortError> {
        let is_drop_class = self.class == PortClass::Drop;
        if self.face.is_side() {
            if is_drop_class {
                return Err(PortError::DropClassOnSideFace);
            }
            if self.level.is_none() {
                return Err(PortError::SideWithoutLevel);
            }
        } else {
            if !is_drop_class {
                return Err(PortError::SideClassOnDropFace);
            }
            if self.level.is_some() {
                return Err(PortError::DropWithLevel);
            }
        }
        Ok(())
    }

    /// The port after placing its piece with `o`. Level and class are unchanged (§8).
    pub const fn transformed(self, o: Orientation) -> Self {
        Self {
            cell: o.apply_cell(self.cell),
            face: o.apply_face(self.face),
            ..self
        }
    }
}

const _: () = assert!(LEVEL_STEP_UM * LEVEL_COUNT == CELL_UM);
