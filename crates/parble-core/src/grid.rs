//! Cell grid, faces and orientation (kit contract §1, §8).

use core::ops::Add;
use serde::{Deserialize, Serialize};

/// Integer cell index. Cell `(x, y, z)` spans world `[60x − 30, 60x + 30]` mm in X,
/// `[60y, 60y + 60]` mm in Y and `[60z − 30, 60z + 30]` mm in Z (contract §1).
///
/// Field order is x, y, z so the derived `Ord` sorts like the sidecar `[x, y, z]` lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(from = "[i32; 3]", into = "[i32; 3]")]
pub struct CellCoord {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl CellCoord {
    pub const ORIGIN: Self = Self::new(0, 0, 0);

    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
}

impl Add for CellCoord {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl From<[i32; 3]> for CellCoord {
    fn from([x, y, z]: [i32; 3]) -> Self {
        Self::new(x, y, z)
    }
}

impl From<CellCoord> for [i32; 3] {
    fn from(c: CellCoord) -> Self {
        [c.x, c.y, c.z]
    }
}

/// A cell face. Side faces carry horizontal ports, `Top`/`Bottom` carry drop ports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PortFace {
    #[serde(rename = "+x")]
    PosX,
    #[serde(rename = "-x")]
    NegX,
    #[serde(rename = "+z")]
    PosZ,
    #[serde(rename = "-z")]
    NegZ,
    #[serde(rename = "top")]
    Top,
    #[serde(rename = "bottom")]
    Bottom,
}

impl PortFace {
    pub const ALL: [Self; 6] = [
        Self::PosX,
        Self::NegX,
        Self::PosZ,
        Self::NegZ,
        Self::Top,
        Self::Bottom,
    ];

    /// Outward unit normal as a cell offset.
    pub const fn normal(self) -> CellCoord {
        match self {
            Self::PosX => CellCoord::new(1, 0, 0),
            Self::NegX => CellCoord::new(-1, 0, 0),
            Self::PosZ => CellCoord::new(0, 0, 1),
            Self::NegZ => CellCoord::new(0, 0, -1),
            Self::Top => CellCoord::new(0, 1, 0),
            Self::Bottom => CellCoord::new(0, -1, 0),
        }
    }

    /// The face with the given outward normal, if it is a unit axis vector.
    pub const fn from_normal(n: CellCoord) -> Option<Self> {
        match (n.x, n.y, n.z) {
            (1, 0, 0) => Some(Self::PosX),
            (-1, 0, 0) => Some(Self::NegX),
            (0, 0, 1) => Some(Self::PosZ),
            (0, 0, -1) => Some(Self::NegZ),
            (0, 1, 0) => Some(Self::Top),
            (0, -1, 0) => Some(Self::Bottom),
            _ => None,
        }
    }

    pub const fn is_side(self) -> bool {
        !matches!(self, Self::Top | Self::Bottom)
    }

    /// The face of the neighbouring cell that this face touches.
    pub const fn opposite(self) -> Self {
        match self {
            Self::PosX => Self::NegX,
            Self::NegX => Self::PosX,
            Self::PosZ => Self::NegZ,
            Self::NegZ => Self::PosZ,
            Self::Top => Self::Bottom,
            Self::Bottom => Self::Top,
        }
    }
}

/// Quarter-turn yaw about +Y, `k × 90°` right-handed (k = 1 maps +X to −Z).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(try_from = "u8", into = "u8")]
pub struct Yaw4(u8);

impl Yaw4 {
    /// `k` quarter turns, reduced modulo 4.
    pub const fn new(k: u8) -> Self {
        Self(k % 4)
    }

    pub const fn quarter_turns(self) -> u8 {
        self.0
    }

    /// The yaw of applying `self` then `other`.
    pub const fn then(self, other: Self) -> Self {
        Self::new(self.0 + other.0)
    }
}

/// Error for a serialized yaw outside `0..=3`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidYaw(pub u8);

impl core::fmt::Display for InvalidYaw {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "yaw quarter turns must be 0..=3, got {}", self.0)
    }
}

impl TryFrom<u8> for Yaw4 {
    type Error = InvalidYaw;

    fn try_from(k: u8) -> Result<Self, InvalidYaw> {
        if k < 4 {
            Ok(Self(k))
        } else {
            Err(InvalidYaw(k))
        }
    }
}

impl From<Yaw4> for u8 {
    fn from(y: Yaw4) -> Self {
        y.0
    }
}

/// Reflection of the piece-local X axis (`x → −x`). Serialized as a bool.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(from = "bool", into = "bool")]
pub enum Mirror {
    #[default]
    Off,
    On,
}

impl From<bool> for Mirror {
    fn from(b: bool) -> Self {
        if b { Self::On } else { Self::Off }
    }
}

impl From<Mirror> for bool {
    fn from(m: Mirror) -> Self {
        m == Mirror::On
    }
}

/// A placement orientation: mirror first, then yaw (contract §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Orientation {
    pub yaw: Yaw4,
    pub mirror: Mirror,
}

impl Orientation {
    pub const IDENTITY: Self = Self::new(Yaw4::new(0), Mirror::Off);

    pub const fn new(yaw: Yaw4, mirror: Mirror) -> Self {
        Self { yaw, mirror }
    }

    /// Transform a piece-local cell offset (relative to the anchor cell).
    pub const fn apply_cell(self, c: CellCoord) -> CellCoord {
        let mut x = match self.mirror {
            Mirror::Off => c.x,
            Mirror::On => -c.x,
        };
        let mut z = c.z;
        let mut k = self.yaw.0;
        while k > 0 {
            // One quarter turn: (x, z) -> (z, -x).
            let nx = z;
            z = -x;
            x = nx;
            k -= 1;
        }
        CellCoord::new(x, c.y, z)
    }

    /// Transform a piece-local face. Derived from the face normal so it can never
    /// disagree with [`Self::apply_cell`].
    pub const fn apply_face(self, f: PortFace) -> PortFace {
        match PortFace::from_normal(self.apply_cell(f.normal())) {
            Some(face) => face,
            None => unreachable!(),
        }
    }
}
