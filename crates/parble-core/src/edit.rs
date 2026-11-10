//! The track edit command model: commands, undo/redo and selection sets (P3-T1).
//!
//! Every command compiles into a list of primitive [`Change`]s that each hold both the old
//! and the new value. Undo applies the inverses in reverse order and redo replays the list,
//! so neither needs the catalog and undo restores the track exactly.

use std::collections::{BTreeMap, BTreeSet};

use crate::catalog::PieceCatalog;
use crate::grid::{CellCoord, Mirror, Orientation, Yaw4};
use crate::nudge_zone::{MAX_NUDGE_ZONES, NudgeZone};
use crate::piece::{PieceDef, PieceId};
use crate::track::{HopperConfig, MechConfig, Placement, Track};

/// A set of placement indices into [`Track::placements`], kept sorted and unique.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection(BTreeSet<u32>);

impl Selection {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `index`. Returns whether it was newly added.
    pub fn insert(&mut self, index: u32) -> bool {
        self.0.insert(index)
    }

    /// Removes `index`. Returns whether it was present.
    pub fn remove(&mut self, index: u32) -> bool {
        self.0.remove(&index)
    }

    /// Adds `index` if absent, removes it if present.
    pub fn toggle(&mut self, index: u32) {
        if !self.0.remove(&index) {
            self.0.insert(index);
        }
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn contains(&self, index: u32) -> bool {
        self.0.contains(&index)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Indices in ascending order.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = u32> + '_ {
        self.0.iter().copied()
    }

    /// Every placement with at least one footprint cell inside the inclusive box spanned
    /// by `a` and `b`. A placement whose piece is not in the catalog is tested by its
    /// anchor cell alone.
    pub fn boxed<C: PieceCatalog + ?Sized>(
        track: &Track,
        catalog: &C,
        a: CellCoord,
        b: CellCoord,
    ) -> Self {
        let lo = CellCoord::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z));
        let hi = CellCoord::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z));
        let inside = |c: &CellCoord| {
            (lo.x..=hi.x).contains(&c.x)
                && (lo.y..=hi.y).contains(&c.y)
                && (lo.z..=hi.z).contains(&c.z)
        };
        let mut sel = Self::new();
        for (i, p) in (0u32..).zip(&track.placements) {
            let hit = match catalog.get(&p.piece).map(|def| cells_of(def, p)) {
                Some(Ok(cells)) => cells.iter().any(inside),
                _ => inside(&p.anchor),
            };
            if hit {
                sel.insert(i);
            }
        }
        sel
    }
}

impl FromIterator<u32> for Selection {
    fn from_iter<I: IntoIterator<Item = u32>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

/// One edit. Commands that take a [`Selection`] act on those placement indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditCommand {
    /// Append a placement.
    Place(Placement),
    /// Remove the selected placements; later placements keep their relative order.
    Remove(Selection),
    /// Turn each selected placement by `by` quarter turns about its own anchor.
    Rotate { selection: Selection, by: Yaw4 },
    /// Toggle mirror on each selected placement in place. Every one must be chiral (§8).
    Mirror(Selection),
    /// Translate the selected placements by `offset`.
    MoveGroup {
        selection: Selection,
        offset: CellCoord,
    },
    /// Append copies of `placements` translated by `offset` (see [`Editor::copy`]).
    PasteGroup {
        placements: Vec<Placement>,
        offset: CellCoord,
    },
    /// Replace the mechanism config of a `mech_*` placement.
    SetMechConfig { index: u32, config: MechConfig },
    /// Replace the hopper configuration.
    SetHopper(HopperConfig),
    /// Append a nudge zone ([`crate::nudge_zone`]) and select it.
    AddNudge(NudgeZone),
    /// Replace nudge zone `index`.
    SetNudge { index: u32, zone: NudgeZone },
    /// Remove nudge zone `index`; later zones keep their order.
    RemoveNudge(u32),
}

/// Why a command was rejected. A rejected command changes nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    UnknownPiece(PieceId),
    /// A placement index that is not in the track.
    IndexOutOfRange(u32),
    EmptySelection,
    /// Mirror requested on an achiral piece (contract §8).
    NotChiral(PieceId),
    /// Mechanism config on a placement whose piece is not in the `mech` family.
    NotMechanism(u32),
    /// Two placements would share this cell (contract §9 rule 6).
    Collision(CellCoord),
    /// A cell coordinate would leave the `i32` range.
    CoordOverflow,
    /// A nudge zone index that is not in the track.
    NudgeOutOfRange(u32),
    /// A nudge zone that breaks a [`NudgeZone::check`] rule.
    InvalidNudge(String),
    /// The track already holds [`MAX_NUDGE_ZONES`].
    TooManyNudges,
}

impl core::fmt::Display for EditError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownPiece(id) => write!(f, "piece {id} is not in the catalog"),
            Self::IndexOutOfRange(i) => write!(f, "placement index {i} is out of range"),
            Self::EmptySelection => f.write_str("the selection is empty"),
            Self::NotChiral(id) => write!(f, "piece {id} is not chiral and cannot be mirrored"),
            Self::NotMechanism(i) => write!(f, "placement {i} is not a mechanism piece"),
            Self::Collision(c) => write!(f, "two placements would share cell {c:?}"),
            Self::CoordOverflow => f.write_str("cell coordinate overflow"),
            Self::NudgeOutOfRange(i) => write!(f, "nudge zone index {i} is out of range"),
            Self::InvalidNudge(e) => write!(f, "invalid nudge zone: {e}"),
            Self::TooManyNudges => write!(f, "a track holds at most {MAX_NUDGE_ZONES} nudge zones"),
        }
    }
}

impl std::error::Error for EditError {}

/// A primitive, self-inverting track change.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Change {
    Insert {
        index: u32,
        placement: Placement,
    },
    Remove {
        index: u32,
        placement: Placement,
    },
    Replace {
        index: u32,
        old: Placement,
        new: Placement,
    },
    SetHopper {
        old: HopperConfig,
        new: HopperConfig,
    },
    InsertNudge {
        index: u32,
        zone: NudgeZone,
    },
    RemoveNudge {
        index: u32,
        zone: NudgeZone,
    },
    ReplaceNudge {
        index: u32,
        old: NudgeZone,
        new: NudgeZone,
    },
}

impl Change {
    fn apply(&self, track: &mut Track) {
        match self {
            Self::Insert { index, placement } => {
                track.placements.insert(*index as usize, placement.clone());
            }
            Self::Remove { index, .. } => {
                track.placements.remove(*index as usize);
            }
            Self::Replace { index, new, .. } => track.placements[*index as usize] = new.clone(),
            Self::SetHopper { new, .. } => track.hopper = new.clone(),
            Self::InsertNudge { index, zone } => {
                track.nudge_zones.insert(*index as usize, zone.clone());
            }
            Self::RemoveNudge { index, .. } => {
                track.nudge_zones.remove(*index as usize);
            }
            Self::ReplaceNudge { index, new, .. } => {
                track.nudge_zones[*index as usize] = new.clone();
            }
        }
    }

    fn revert(&self, track: &mut Track) {
        match self {
            Self::Insert { index, .. } => {
                track.placements.remove(*index as usize);
            }
            Self::Remove { index, placement } => {
                track.placements.insert(*index as usize, placement.clone());
            }
            Self::Replace { index, old, .. } => track.placements[*index as usize] = old.clone(),
            Self::SetHopper { old, .. } => track.hopper = old.clone(),
            Self::InsertNudge { index, .. } => {
                track.nudge_zones.remove(*index as usize);
            }
            Self::RemoveNudge { index, zone } => {
                track.nudge_zones.insert(*index as usize, zone.clone());
            }
            Self::ReplaceNudge { index, old, .. } => {
                track.nudge_zones[*index as usize] = old.clone();
            }
        }
    }
}

/// A track under edit, with undo and redo stacks and the current selection.
#[derive(Debug, Clone)]
pub struct Editor {
    track: Track,
    undo: Vec<Vec<Change>>,
    redo: Vec<Vec<Change>>,
    /// The current selection. Structural commands and undo/redo update it.
    pub selection: Selection,
    /// The selected nudge zone, apart from the placement selection: [`EditCommand::AddNudge`]
    /// selects the zone it adds, [`EditCommand::RemoveNudge`], undo and redo clear it.
    pub nudge: Option<u32>,
}

impl Editor {
    pub fn new(track: Track) -> Self {
        Self {
            track,
            undo: Vec::new(),
            redo: Vec::new(),
            selection: Selection::new(),
            nudge: None,
        }
    }

    pub fn track(&self) -> &Track {
        &self.track
    }

    pub fn into_track(self) -> Track {
        self.track
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Clones of the selected placements, for a later [`EditCommand::PasteGroup`].
    pub fn copy(&self, selection: &Selection) -> Result<Vec<Placement>, EditError> {
        selection
            .iter()
            .map(|i| self.placement(i).cloned())
            .collect()
    }

    /// Validates and applies `cmd`. On success the redo stack is cleared; on error the
    /// track, both stacks and the selection are unchanged.
    pub fn apply<C: PieceCatalog + ?Sized>(
        &mut self,
        catalog: &C,
        cmd: EditCommand,
    ) -> Result<(), EditError> {
        let nudge = match &cmd {
            EditCommand::AddNudge(_) => Some(Some(
                u32::try_from(self.track.nudge_zones.len()).expect("nudge count fits in u32"),
            )),
            EditCommand::RemoveNudge(_) => Some(None),
            _ => None,
        };
        let (changes, selection) = self.plan(catalog, cmd)?;
        for change in &changes {
            change.apply(&mut self.track);
        }
        if let Some(n) = nudge {
            self.nudge = n;
        }
        self.undo.push(changes);
        self.redo.clear();
        if let Some(sel) = selection {
            self.selection = sel;
        }
        Ok(())
    }

    /// Whether `cmd` would apply, without changing the track, the stacks or the selection (the
    /// editor's ghost preview).
    pub fn check<C: PieceCatalog + ?Sized>(
        &self,
        catalog: &C,
        cmd: EditCommand,
    ) -> Result<(), EditError> {
        self.plan(catalog, cmd).map(|_| ())
    }

    /// Reverts the most recent command. Returns `false` when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        let Some(changes) = self.undo.pop() else {
            return false;
        };
        for change in changes.iter().rev() {
            change.revert(&mut self.track);
        }
        self.redo.push(changes);
        self.selection.clear();
        self.nudge = None;
        true
    }

    /// Re-applies the most recently undone command. Returns `false` when there is none.
    pub fn redo(&mut self) -> bool {
        let Some(changes) = self.redo.pop() else {
            return false;
        };
        for change in &changes {
            change.apply(&mut self.track);
        }
        self.undo.push(changes);
        self.selection.clear();
        self.nudge = None;
        true
    }

    fn placement(&self, index: u32) -> Result<&Placement, EditError> {
        self.track
            .placements
            .get(index as usize)
            .ok_or(EditError::IndexOutOfRange(index))
    }

    fn len(&self) -> u32 {
        u32::try_from(self.track.placements.len()).expect("placement count fits in u32")
    }

    /// The primitive changes for `cmd` and the selection to adopt after it (`None` keeps
    /// the current one).
    fn plan<C: PieceCatalog + ?Sized>(
        &self,
        catalog: &C,
        cmd: EditCommand,
    ) -> Result<(Vec<Change>, Option<Selection>), EditError> {
        match cmd {
            EditCommand::Place(p) => self.plan_append(catalog, vec![p]),
            EditCommand::PasteGroup { placements, offset } => {
                let moved = placements
                    .into_iter()
                    .map(|p| translated(p, offset))
                    .collect::<Result<Vec<_>, _>>()?;
                self.plan_append(catalog, moved)
            }
            EditCommand::Remove(sel) => self.plan_remove(&sel),
            EditCommand::Rotate { selection, by } => self.plan_replace(catalog, &selection, |p| {
                Ok(Placement {
                    yaw: p.yaw.then(by),
                    ..p.clone()
                })
            }),
            EditCommand::Mirror(selection) => self.plan_replace(catalog, &selection, |p| {
                Ok(Placement {
                    mirror: match p.mirror {
                        Mirror::Off => Mirror::On,
                        Mirror::On => Mirror::Off,
                    },
                    ..p.clone()
                })
            }),
            EditCommand::MoveGroup { selection, offset } => {
                self.plan_replace(catalog, &selection, |p| translated(p.clone(), offset))
            }
            EditCommand::SetMechConfig { index, config } => {
                let old = self.placement(index)?;
                if old.piece.family() != "mech" {
                    return Err(EditError::NotMechanism(index));
                }
                let new = Placement {
                    config,
                    ..old.clone()
                };
                let change = Change::Replace {
                    index,
                    old: old.clone(),
                    new,
                };
                Ok((vec![change], None))
            }
            EditCommand::SetHopper(new) => {
                if let Some(i) = new.placement {
                    self.placement(i)?;
                }
                let old = self.track.hopper.clone();
                Ok((vec![Change::SetHopper { old, new }], None))
            }
            EditCommand::AddNudge(zone) => {
                zone.check().map_err(EditError::InvalidNudge)?;
                if self.track.nudge_zones.len() >= MAX_NUDGE_ZONES {
                    return Err(EditError::TooManyNudges);
                }
                let index =
                    u32::try_from(self.track.nudge_zones.len()).expect("nudge count fits in u32");
                Ok((vec![Change::InsertNudge { index, zone }], None))
            }
            EditCommand::SetNudge { index, zone } => {
                zone.check().map_err(EditError::InvalidNudge)?;
                let old = self.nudge_zone(index)?.clone();
                Ok((
                    vec![Change::ReplaceNudge {
                        index,
                        old,
                        new: zone,
                    }],
                    None,
                ))
            }
            EditCommand::RemoveNudge(index) => {
                let zone = self.nudge_zone(index)?.clone();
                Ok((vec![Change::RemoveNudge { index, zone }], None))
            }
        }
    }

    fn nudge_zone(&self, index: u32) -> Result<&NudgeZone, EditError> {
        self.track
            .nudge_zones
            .get(index as usize)
            .ok_or(EditError::NudgeOutOfRange(index))
    }

    fn plan_append<C: PieceCatalog + ?Sized>(
        &self,
        catalog: &C,
        placements: Vec<Placement>,
    ) -> Result<(Vec<Change>, Option<Selection>), EditError> {
        if placements.is_empty() {
            return Err(EditError::EmptySelection);
        }
        let mut occupied = self.occupancy(catalog, &Selection::new())?;
        let start = self.len();
        let mut changes = Vec::with_capacity(placements.len());
        let mut selection = Selection::new();
        for (index, placement) in (start..).zip(placements) {
            occupy(&mut occupied, &checked_cells(catalog, &placement)?, index)?;
            selection.insert(index);
            changes.push(Change::Insert { index, placement });
        }
        Ok((changes, Some(selection)))
    }

    fn plan_remove(&self, sel: &Selection) -> Result<(Vec<Change>, Option<Selection>), EditError> {
        if sel.is_empty() {
            return Err(EditError::EmptySelection);
        }
        let mut changes = Vec::with_capacity(sel.len() + 1);
        let old = self.track.hopper.clone();
        let hopper = old.placement.and_then(|h| {
            if sel.contains(h) {
                None
            } else {
                let below = u32::try_from(sel.iter().take_while(|&i| i < h).count())
                    .expect("selection size fits in u32");
                Some(h - below)
            }
        });
        if hopper != old.placement {
            let new = HopperConfig {
                placement: hopper,
                ..old.clone()
            };
            changes.push(Change::SetHopper { old, new });
        }
        for index in sel.iter().rev() {
            let placement = self.placement(index)?.clone();
            changes.push(Change::Remove { index, placement });
        }
        Ok((changes, Some(Selection::new())))
    }

    /// Replaces each selected placement with `f(old)`, checking the new footprints against
    /// every unselected placement and against each other.
    fn plan_replace<C: PieceCatalog + ?Sized>(
        &self,
        catalog: &C,
        sel: &Selection,
        f: impl Fn(&Placement) -> Result<Placement, EditError>,
    ) -> Result<(Vec<Change>, Option<Selection>), EditError> {
        if sel.is_empty() {
            return Err(EditError::EmptySelection);
        }
        let mut occupied = self.occupancy(catalog, sel)?;
        let mut changes = Vec::with_capacity(sel.len());
        for index in sel.iter() {
            let old = self.placement(index)?;
            let new = f(old)?;
            occupy(&mut occupied, &checked_cells(catalog, &new)?, index)?;
            changes.push(Change::Replace {
                index,
                old: old.clone(),
                new,
            });
        }
        Ok((changes, None))
    }

    /// Cell → placement index for every placement not in `exclude`.
    fn occupancy<C: PieceCatalog + ?Sized>(
        &self,
        catalog: &C,
        exclude: &Selection,
    ) -> Result<BTreeMap<CellCoord, u32>, EditError> {
        let mut occupied = BTreeMap::new();
        for (i, p) in (0u32..).zip(&self.track.placements) {
            if !exclude.contains(i) {
                let def = lookup(catalog, &p.piece)?;
                for c in cells_of(def, p)? {
                    occupied.insert(c, i);
                }
            }
        }
        Ok(occupied)
    }
}

fn lookup<'a, C: PieceCatalog + ?Sized>(
    catalog: &'a C,
    id: &PieceId,
) -> Result<&'a PieceDef, EditError> {
    catalog
        .get(id)
        .ok_or_else(|| EditError::UnknownPiece(id.clone()))
}

/// World cells of `p`, after checking the piece exists and mirror is legal for it.
fn checked_cells<C: PieceCatalog + ?Sized>(
    catalog: &C,
    p: &Placement,
) -> Result<Vec<CellCoord>, EditError> {
    let def = lookup(catalog, &p.piece)?;
    if p.mirror == Mirror::On && !def.chiral {
        return Err(EditError::NotChiral(p.piece.clone()));
    }
    cells_of(def, p)
}

/// World cells covered by `def` placed as `p`, with overflow checking.
fn cells_of(def: &PieceDef, p: &Placement) -> Result<Vec<CellCoord>, EditError> {
    def.footprint_for(Orientation::new(p.yaw, p.mirror))
        .into_iter()
        .map(|c| checked_add(p.anchor, c))
        .collect()
}

fn occupy(
    occupied: &mut BTreeMap<CellCoord, u32>,
    cells: &[CellCoord],
    index: u32,
) -> Result<(), EditError> {
    for &c in cells {
        if occupied.insert(c, index).is_some() {
            return Err(EditError::Collision(c));
        }
    }
    Ok(())
}

fn translated(p: Placement, offset: CellCoord) -> Result<Placement, EditError> {
    Ok(Placement {
        anchor: checked_add(p.anchor, offset)?,
        ..p
    })
}

fn checked_add(a: CellCoord, b: CellCoord) -> Result<CellCoord, EditError> {
    match (
        a.x.checked_add(b.x),
        a.y.checked_add(b.y),
        a.z.checked_add(b.z),
    ) {
        (Some(x), Some(y), Some(z)) => Ok(CellCoord::new(x, y, z)),
        _ => Err(EditError::CoordOverflow),
    }
}
