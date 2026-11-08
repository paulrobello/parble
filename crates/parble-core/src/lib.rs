//! parble-core: the cell grid, the kit contract types, the track model and seeded RNG.
//!
//! No Bevy, no Rapier, no floats in transforms. All lengths are integer micrometres (µm)
//! and every number traces to `docs/kit-contract.md`.

pub mod catalog;
pub mod contract;
pub mod edit;
pub mod file;
pub mod grid;
pub mod piece;
pub mod port;
pub mod rng;
pub mod track;
pub mod validate;

pub use catalog::{MapCatalog, PieceCatalog};
pub use contract::KIT_CONTRACT_VERSION;
pub use edit::{EditCommand, EditError, Editor, Selection};
pub use file::{FileError, MechanismState, Metadata, ParbleFile};
pub use grid::{CellCoord, InvalidYaw, Mirror, Orientation, PortFace, Yaw4};
pub use piece::{InvalidPieceId, PieceDef, PieceDefError, PieceId};
pub use port::{InvalidLevel, Port, PortClass, PortError, PortFlow, PortLevel};
pub use rng::{SeededRng, SubStream};
pub use track::{
    HopperConfig, MarbleSize, MechConfig, PHYSICS_VERSION, Placement, ReleaseMode, SCHEMA_VERSION,
    Track,
};
pub use validate::{Issue, IssueKind, LinkFault, is_finish, is_hopper, validate_track};
