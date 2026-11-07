//! Piece identifiers and definitions (kit contract §9, §10).

use crate::contract::{MAX_FOOTPRINT_LAYERS, MAX_FOOTPRINT_PLAN_CELLS, MAX_PIECE_ID_LEN};
use crate::grid::{CellCoord, Orientation, PortFace};
use crate::port::{Port, PortError};
use serde::{Deserialize, Serialize};

const FAMILIES: [&str; 6] = ["blk", "rail", "mech", "terr", "music", "deco"];

/// A catalog piece ID matching `^(blk|rail|mech|terr|music|deco)_[a-z0-9]+(_[a-z0-9]+)*$`,
/// at most 48 characters (contract §10). Validated on construction and on deserialize.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PieceId(String);

/// Error for a string that does not match the §10 piece ID grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidPieceId(pub String);

impl core::fmt::Display for InvalidPieceId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "invalid piece id {:?}", self.0)
    }
}

impl PieceId {
    pub fn new(id: &str) -> Result<Self, InvalidPieceId> {
        if Self::is_valid(id) {
            Ok(Self(id.to_owned()))
        } else {
            Err(InvalidPieceId(id.to_owned()))
        }
    }

    fn is_valid(id: &str) -> bool {
        if id.len() > MAX_PIECE_ID_LEN {
            return false;
        }
        let mut segments = id.split('_');
        let family_ok = segments.next().is_some_and(|f| FAMILIES.contains(&f));
        let mut rest = segments.peekable();
        family_ok
            && rest.peek().is_some()
            && rest.all(|seg| {
                !seg.is_empty()
                    && seg
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            })
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The family prefix (`blk`, `rail`, …).
    pub fn family(&self) -> &str {
        self.0.split('_').next().unwrap_or_default()
    }
}

impl core::fmt::Display for PieceId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for PieceId {
    type Error = InvalidPieceId;

    fn try_from(s: String) -> Result<Self, InvalidPieceId> {
        Self::new(&s)
    }
}

impl From<PieceId> for String {
    fn from(id: PieceId) -> Self {
        id.0
    }
}

/// A piece definition in its piece-local frame: footprint cells relative to the anchor
/// cell `(0, 0, 0)` and an ordered port list whose index 0 sits in the anchor cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PieceDef {
    pub id: PieceId,
    /// Whether the editor offers mirror (contract §8).
    pub chiral: bool,
    pub footprint: Vec<CellCoord>,
    pub ports: Vec<Port>,
}

/// Why a [`PieceDef`] breaks the §9 footprint rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieceDefError {
    NoPorts,
    MissingAnchorCell,
    DuplicateCell,
    TooLarge,
    NotFaceConnected,
    AnchorPortNotAtOrigin,
    /// Port index whose cell is not in the footprint.
    PortOutsideFootprint(usize),
    /// Port index whose face neighbour is inside the footprint.
    PortOnInternalFace(usize),
    /// Port index that is malformed.
    Port(usize, PortError),
}

impl PieceDef {
    /// The footprint after orientation `o`, relative to the anchor, sorted by `(x, y, z)`.
    pub fn footprint_for(&self, o: Orientation) -> Vec<CellCoord> {
        let mut cells: Vec<CellCoord> = self.footprint.iter().map(|&c| o.apply_cell(c)).collect();
        cells.sort_unstable();
        cells
    }

    /// This definition as placed with orientation `o` (footprint sorted, port order kept).
    pub fn transformed(&self, o: Orientation) -> Self {
        Self {
            id: self.id.clone(),
            chiral: self.chiral,
            footprint: self.footprint_for(o),
            ports: self.ports.iter().map(|p| p.transformed(o)).collect(),
        }
    }

    /// Check the §9 footprint and port rules.
    pub fn validate(&self) -> Result<(), PieceDefError> {
        if self.ports.is_empty() {
            return Err(PieceDefError::NoPorts);
        }
        let mut cells = self.footprint.clone();
        cells.sort_unstable();
        if cells.binary_search(&CellCoord::ORIGIN).is_err() {
            return Err(PieceDefError::MissingAnchorCell);
        }
        if cells.windows(2).any(|w| w[0] == w[1]) {
            return Err(PieceDefError::DuplicateCell);
        }
        let extent = |axis: fn(&CellCoord) -> i32| {
            let lo = cells.iter().map(axis).min().unwrap_or(0);
            let hi = cells.iter().map(axis).max().unwrap_or(0);
            hi - lo + 1
        };
        if extent(|c| c.x) > MAX_FOOTPRINT_PLAN_CELLS
            || extent(|c| c.z) > MAX_FOOTPRINT_PLAN_CELLS
            || extent(|c| c.y) > MAX_FOOTPRINT_LAYERS
        {
            return Err(PieceDefError::TooLarge);
        }
        if !is_face_connected(&cells) {
            return Err(PieceDefError::NotFaceConnected);
        }
        for (i, port) in self.ports.iter().enumerate() {
            port.validate().map_err(|e| PieceDefError::Port(i, e))?;
            if i == 0 && port.cell != CellCoord::ORIGIN {
                return Err(PieceDefError::AnchorPortNotAtOrigin);
            }
            if cells.binary_search(&port.cell).is_err() {
                return Err(PieceDefError::PortOutsideFootprint(i));
            }
            if cells
                .binary_search(&(port.cell + port.face.normal()))
                .is_ok()
            {
                return Err(PieceDefError::PortOnInternalFace(i));
            }
        }
        Ok(())
    }
}

/// Flood fill over the six face neighbours. `cells` must be sorted and non-empty.
fn is_face_connected(cells: &[CellCoord]) -> bool {
    let mut seen = vec![false; cells.len()];
    let mut stack = vec![0usize];
    seen[0] = true;
    let mut reached = 1;
    while let Some(i) = stack.pop() {
        for face in PortFace::ALL {
            if let Ok(j) = cells.binary_search(&(cells[i] + face.normal()))
                && !seen[j]
            {
                seen[j] = true;
                reached += 1;
                stack.push(j);
            }
        }
    }
    reached == cells.len()
}
