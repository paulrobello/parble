//! Track validation (P3-T2): open ports, broken links, overlaps, naming and reachability.
//!
//! [`validate_track`] returns a sorted, de-duplicated list of [`Issue`]s. Every issue tied to
//! a placement carries that placement's index, anchor and the cells involved. Track-level
//! issues ([`IssueKind::MissingHopper`], [`IssueKind::MissingFinish`], an out-of-range
//! [`IssueKind::HopperConfigInvalid`]) have no placement, no anchor and no cells.
//!
//! # Links (contract §3, §4, §5.5)
//!
//! A port faces the neighbouring cell across its face. Two ports mate when they sit on
//! opposite faces of that shared face, have the same absolute floor height (side ports,
//! [`PortLevel::absolute_floor_um`]; drop ports have none) and compatible classes
//! ([`PortClass::mates_with`]). A `Bottom` drop port therefore mates with the `Top` drop port
//! of the cell below. A link whose two ends are both `In` or both `Out` is a flow conflict.
//!
//! # Hopper and finish (contract §10)
//!
//! A piece is a hopper when its ID is in the `mech` family with shape segment `hopper`
//! (`mech_hopper`, `mech_hopper_<qualifier>…`), and a finish when the shape segment is
//! `finish`. When [`crate::HopperConfig::placement`] is set it names the only release source.
//! If it is out of range, unresolvable or not a hopper, that is an issue and every
//! hopper-named placement is used as a source instead.
//!
//! # Reachability
//!
//! Marbles flow along valid links only, from a port that allows outflow (`Out`, `Both`) into a
//! port that allows inflow (`In`, `Both`). Inside a piece, a marble entering any port can leave
//! by any outflow port (the validator does not model internal routing). Every resolved
//! placement not reached from a source is [`IssueKind::Unreachable`], except a finish, which is
//! reported once as [`IssueKind::FinishUnreachable`]. Without any source only
//! [`IssueKind::MissingHopper`] is reported.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::catalog::PieceCatalog;
use crate::grid::{CellCoord, PortFace};
use crate::piece::{PieceDef, PieceId};
use crate::port::{Port, PortClass, PortFlow, PortLevel};
use crate::track::Track;

/// One validation finding. Ordered by placement (track-level issues first), then anchor,
/// cells and kind, so a sorted list is deterministic.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Issue {
    /// Index into [`Track::placements`] of the placement the issue is reported on.
    pub placement: Option<usize>,
    /// Anchor cell of that placement.
    pub anchor: Option<CellCoord>,
    /// World cells involved, sorted.
    pub cells: Vec<CellCoord>,
    pub kind: IssueKind,
}

/// What is wrong.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IssueKind {
    /// The placement's piece is not in the catalog. Cells: the anchor.
    UnknownPiece { piece: PieceId },
    /// The placement shares cells with placement `other` (contract §9 rule 6). Reported on the
    /// lower index. Cells: the shared cells.
    Overlap { other: usize },
    /// The ID's `_l<n>` suffix does not name port 0's level (contract §10, §14 item 2).
    /// `port0` is `None` when port 0 is a drop port. Cells: the anchor.
    LevelSuffixMismatch {
        suffix: PortLevel,
        port0: Option<PortLevel>,
    },
    /// Nothing occupies the cell this port faces. Cells: the port's cell.
    OpenPort {
        port: usize,
        face: PortFace,
        flow: PortFlow,
    },
    /// The port faces placement `other` but does not form a valid link. Reported once per
    /// pair, on the lower `(placement, port)`. Cells: both cells of the shared face.
    BrokenLink {
        port: usize,
        other: usize,
        /// The facing port on `other`, if it has one on the shared face.
        other_port: Option<usize>,
        fault: LinkFault,
    },
    /// No marble released from a hopper can reach this placement. Cells: its footprint.
    Unreachable,
    /// No marble released from a hopper can reach this finish. Cells: its footprint.
    FinishUnreachable,
    /// `track.hopper.placement` does not name a hopper placement.
    HopperConfigInvalid { index: u32 },
    /// The track has no hopper to release from.
    MissingHopper,
    /// The track has no finish.
    MissingFinish,
}

/// Why two facing ports do not link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LinkFault {
    /// The neighbour has no port on the shared face.
    NoMatingPort,
    /// Different absolute floor heights (µm).
    LevelMismatch { floor_um: i32, other_floor_um: i32 },
    /// Classes that do not mate (§5.5).
    ClassMismatch {
        class: PortClass,
        other_class: PortClass,
    },
    /// Both ends `In` or both `Out`.
    FlowConflict {
        flow: PortFlow,
        other_flow: PortFlow,
    },
}

/// Whether `id` names a hopper: family `mech`, shape segment `hopper`.
pub fn is_hopper(id: &PieceId) -> bool {
    id.family() == "mech" && shape(id) == Some("hopper")
}

/// Whether `id` names a finish: family `mech`, shape segment `finish`.
pub fn is_finish(id: &PieceId) -> bool {
    id.family() == "mech" && shape(id) == Some("finish")
}

fn shape(id: &PieceId) -> Option<&str> {
    id.as_str().split('_').nth(1)
}

/// The level named by a trailing `_l0` … `_l3` segment, if any.
fn level_suffix(id: &PieceId) -> Option<PortLevel> {
    match id.as_str().rsplit('_').next()? {
        "l0" => Some(PortLevel::L0),
        "l1" => Some(PortLevel::L1),
        "l2" => Some(PortLevel::L2),
        "l3" => Some(PortLevel::L3),
        _ => None,
    }
}

/// A placement resolved against the catalog: world cells and world ports.
struct Resolved<'a> {
    def: &'a PieceDef,
    cells: Vec<CellCoord>,
    ports: Vec<Port>,
}

const fn allows_out(f: PortFlow) -> bool {
    matches!(f, PortFlow::Out | PortFlow::Both)
}

const fn allows_in(f: PortFlow) -> bool {
    matches!(f, PortFlow::In | PortFlow::Both)
}

/// Absolute floor height of a side port; `None` for a drop port.
fn floor_um(p: &Port) -> Option<i32> {
    p.level.map(|l| l.absolute_floor_um(p.cell.y))
}

/// Validate `track` against `catalog`. The result is sorted and free of duplicates.
pub fn validate_track(track: &Track, catalog: &impl PieceCatalog) -> Vec<Issue> {
    let mut issues: BTreeSet<Issue> = BTreeSet::new();
    let on = |i: usize, cells: Vec<CellCoord>, kind: IssueKind| Issue {
        placement: Some(i),
        anchor: Some(track.placements[i].anchor),
        cells,
        kind,
    };

    let resolved: Vec<Option<Resolved<'_>>> = track
        .placements
        .iter()
        .enumerate()
        .map(|(i, p)| match catalog.get(&p.piece) {
            Some(def) => {
                let o = p.orientation();
                let ports = def
                    .ports
                    .iter()
                    .map(|port| {
                        let mut w = port.transformed(o);
                        w.cell = w.cell + p.anchor;
                        w
                    })
                    .collect();
                Some(Resolved {
                    def,
                    cells: p.world_cells(def),
                    ports,
                })
            }
            None => {
                issues.insert(on(
                    i,
                    vec![p.anchor],
                    IssueKind::UnknownPiece {
                        piece: p.piece.clone(),
                    },
                ));
                None
            }
        })
        .collect();

    // Cell occupancy and overlaps.
    let mut occupancy: BTreeMap<CellCoord, Vec<usize>> = BTreeMap::new();
    for (i, r) in resolved.iter().enumerate() {
        for &cell in r.iter().flat_map(|r| &r.cells) {
            occupancy.entry(cell).or_default().push(i);
        }
    }
    let mut shared: BTreeMap<(usize, usize), Vec<CellCoord>> = BTreeMap::new();
    for (&cell, occ) in &occupancy {
        for (k, &a) in occ.iter().enumerate() {
            for &b in &occ[k + 1..] {
                shared.entry((a, b)).or_default().push(cell);
            }
        }
    }
    for ((a, b), cells) in shared {
        issues.insert(on(a, cells, IssueKind::Overlap { other: b }));
    }

    // Level suffix (§14 item 2).
    for (i, r) in resolved.iter().enumerate() {
        let Some(r) = r else { continue };
        if let Some(suffix) = level_suffix(&r.def.id) {
            let port0 = r.def.ports.first().and_then(|p| p.level);
            if port0 != Some(suffix) {
                issues.insert(on(
                    i,
                    vec![track.placements[i].anchor],
                    IssueKind::LevelSuffixMismatch { suffix, port0 },
                ));
            }
        }
    }

    // Links and the directed flow graph between placements.
    let mut edges: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    for (i, r) in resolved.iter().enumerate() {
        let Some(r) = r else { continue };
        for (pi, port) in r.ports.iter().enumerate() {
            let facing = port.cell + port.face.normal();
            let neighbours: Vec<usize> = occupancy
                .get(&facing)
                .map(|occ| occ.iter().copied().filter(|&q| q != i).collect())
                .unwrap_or_default();
            if neighbours.is_empty() {
                if !r.cells.contains(&facing) {
                    issues.insert(on(
                        i,
                        vec![port.cell],
                        IssueKind::OpenPort {
                            port: pi,
                            face: port.face,
                            flow: port.flow,
                        },
                    ));
                }
                continue;
            }
            let mut face_cells = vec![port.cell, facing];
            face_cells.sort_unstable();
            for q in neighbours {
                let Some(rq) = &resolved[q] else { continue };
                let candidates: Vec<(usize, &Port)> = rq
                    .ports
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.cell == facing && p.face == port.face.opposite())
                    .collect();
                let mate = candidates
                    .iter()
                    .find(|(_, p)| floor_um(p) == floor_um(port))
                    .or(candidates.first())
                    .copied();
                let Some((qi, m)) = mate else {
                    // One-sided: only this port exists on the shared face.
                    issues.insert(on(
                        i,
                        face_cells.clone(),
                        IssueKind::BrokenLink {
                            port: pi,
                            other: q,
                            other_port: None,
                            fault: LinkFault::NoMatingPort,
                        },
                    ));
                    continue;
                };
                match link_fault(port, m) {
                    Some(fault) => {
                        // Both ends see the fault; report it once, on the lower end.
                        if (i, pi) < (q, qi) {
                            issues.insert(on(
                                i,
                                face_cells.clone(),
                                IssueKind::BrokenLink {
                                    port: pi,
                                    other: q,
                                    other_port: Some(qi),
                                    fault,
                                },
                            ));
                        }
                    }
                    None => {
                        if allows_out(port.flow) && allows_in(m.flow) {
                            edges.entry(i).or_default().insert(q);
                        }
                    }
                }
            }
        }
    }

    // Sources and targets.
    let named = |pred: fn(&PieceId) -> bool| -> Vec<usize> {
        resolved
            .iter()
            .enumerate()
            .filter(|(_, r)| r.as_ref().is_some_and(|r| pred(&r.def.id)))
            .map(|(i, _)| i)
            .collect()
    };
    let hoppers = named(is_hopper);
    let finishes = named(is_finish);
    let sources = match track.hopper.placement {
        None => hoppers,
        Some(index) => {
            let idx = index as usize;
            let ok = resolved
                .get(idx)
                .and_then(Option::as_ref)
                .is_some_and(|r| is_hopper(&r.def.id));
            if ok {
                vec![idx]
            } else {
                let issue = if idx < track.placements.len() {
                    on(
                        idx,
                        vec![track.placements[idx].anchor],
                        IssueKind::HopperConfigInvalid { index },
                    )
                } else {
                    Issue {
                        placement: None,
                        anchor: None,
                        cells: Vec::new(),
                        kind: IssueKind::HopperConfigInvalid { index },
                    }
                };
                issues.insert(issue);
                hoppers
            }
        }
    };
    let track_issue = |kind| Issue {
        placement: None,
        anchor: None,
        cells: Vec::new(),
        kind,
    };
    if finishes.is_empty() {
        issues.insert(track_issue(IssueKind::MissingFinish));
    }
    if sources.is_empty() {
        issues.insert(track_issue(IssueKind::MissingHopper));
    } else {
        let mut reached = vec![false; resolved.len()];
        let mut queue: VecDeque<usize> = sources.iter().copied().collect();
        for &s in &sources {
            reached[s] = true;
        }
        while let Some(p) = queue.pop_front() {
            for &q in edges.get(&p).into_iter().flatten() {
                if !reached[q] {
                    reached[q] = true;
                    queue.push_back(q);
                }
            }
        }
        for (i, r) in resolved.iter().enumerate() {
            let Some(r) = r else { continue };
            if reached[i] {
                continue;
            }
            let kind = if is_finish(&r.def.id) {
                IssueKind::FinishUnreachable
            } else {
                IssueKind::Unreachable
            };
            issues.insert(on(i, r.cells.clone(), kind));
        }
    }

    issues.into_iter().collect()
}

/// Why `a` and `b`, which sit on the two sides of one face, do not link; `None` if they do.
pub fn link_fault(a: &Port, b: &Port) -> Option<LinkFault> {
    if let (Some(fa), Some(fb)) = (floor_um(a), floor_um(b))
        && fa != fb
    {
        return Some(LinkFault::LevelMismatch {
            floor_um: fa,
            other_floor_um: fb,
        });
    }
    if !a.class.mates_with(b.class) {
        return Some(LinkFault::ClassMismatch {
            class: a.class,
            other_class: b.class,
        });
    }
    let carries =
        (allows_out(a.flow) && allows_in(b.flow)) || (allows_out(b.flow) && allows_in(a.flow));
    if !carries {
        return Some(LinkFault::FlowConflict {
            flow: a.flow,
            other_flow: b.flow,
        });
    }
    None
}
