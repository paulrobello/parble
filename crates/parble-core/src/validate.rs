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
//! # Controlled drops (PRD §5.4, owner ruling 2026-11-08)
//!
//! A marble may leave a side port into an empty cell and fall a short way when its landing is
//! controlled. The validator models the exit as a ballistic fall: the marble crosses the source
//! port's plane along the face normal at a speed in
//! [[`CONTROLLED_DROP_MIN_EXIT_SPEED_UM_S`], [`CONTROLLED_DROP_MAX_EXIT_SPEED_UM_S`]] and falls
//! the floor difference `Δ`, so it travels `v·√(2Δ/g)` past the plane before it lands. A
//! [`ControlledDrop`] links the source to a landing port when:
//!
//! - the source port is a side port that allows outflow and faces an empty cell;
//! - the landing port is a `Channel` side port that allows inflow, on the face opposite the
//!   source face, in the first occupied cell of the column below that empty cell (a channel
//!   entry under the exit: centreline offset 0, inside
//!   [`CONTROLLED_DROP_LATERAL_TOLERANCE_UM`]);
//! - its floor is lower than the source's by `0 < Δ ≤` [`CONTROLLED_DROP_MAX_FALL_UM`];
//! - the whole predicted landing range lies over the landing cell, `0 … CELL_UM` past the plane.
//!
//! A drop into a funnel catcher's bowl (shape segment `funnel`, [`is_funnel`]) is controlled too:
//! when the first occupied cell under the exit belongs to a funnel, the source floor is higher
//! than the funnel's rim floor (its highest side port) by `0 < Δ ≤` [`FUNNEL_MAX_FALL_UM`], and
//! the whole predicted landing range, the segment from the port plane to `v·√(2Δ/g)` past it
//! along the source's centreline, lies within [`FUNNEL_CATCH_RADIUS_UM`] of the bowl's axis in
//! plan (the centre of the funnel's `Bottom` drop port cell). Such a drop has no landing port
//! (`to_port` is `None`).
//!
//! A controlled drop is a link, not an issue: the source port is not open, the landing port is
//! not open and not one-sided (a closed channel end behind a landing is fine), and marbles flow
//! from source to landing only. Any other free fall stays an [`IssueKind::OpenPort`].
//! [`controlled_drops`] lists the drops of a track.
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
//! port that allows inflow (`In`, `Both`). Placement reachability ignores internal routing: a
//! marble entering a piece can reach every piece its outflow ports link to. Every resolved
//! placement not reached from a source is [`IssueKind::Unreachable`], except a finish, which is
//! reported once as [`IssueKind::FinishUnreachable`], and an idle structural piece (below).
//! Without any source only [`IssueKind::MissingHopper`] is reported.
//!
//! # Fed ports, routes and structural pieces (owner ruling 2026-11-09)
//!
//! A port is fed when a marble can enter by it: the port allows inflow and faces a port that
//! allows outflow (whether or not the two link), or it is a controlled drop's landing port.
//!
//! [`PieceDef::routes`] lists the paths through a piece; an empty list routes every inflow port
//! to every outflow port. An open `Out` port of a piece with routes is reported only when a route
//! to it starts at a fed port (or the piece is a funnel drop's target, which is entered without a
//! port): no marble can leave by a port no fed route leads to. Open `In` and `Both` ports are
//! reported as before.
//!
//! A [`PieceDef::structural`] piece (a support) that no marble can enter (no fed port, no funnel
//! drop into it) is idle: its open and one-sided ports and its unreachability are not issues. Once
//! a marble can enter it, every rule applies to it as to any other piece.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::catalog::PieceCatalog;
use crate::contract::{
    CELL_UM, CHANNEL_RADIUS_UM, CHANNEL_WIDTH_UM, HALF_CELL_UM, LEVEL_STEP_UM,
    MARBLE_STANDARD_DIAMETER_UM,
};
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

/// Whether `id` names a funnel catcher: shape segment `funnel` (`blk_funnel_3x3_l2`).
pub fn is_funnel(id: &PieceId) -> bool {
    shape(id) == Some("funnel")
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

/// Highest controlled drop, floor to floor: two port levels (30 mm). It is the fall of all three
/// free-fall transfers of the all-pieces track (`L1` onto `L3` of the layer below), which deliver
/// 10 of 10 marbles (`parble_sim::allpieces`). Longer falls were not measured as controlled: a
/// 120 mm drop → pillar → tower fall threw marbles off the rails.
pub const CONTROLLED_DROP_MAX_FALL_UM: i32 = 2 * LEVEL_STEP_UM;

/// Slowest exit speed the landing check assumes, µm/s: a marble may trickle off a port edge, so
/// the prediction starts at rest, landing at the port plane.
pub const CONTROLLED_DROP_MIN_EXIT_SPEED_UM_S: i64 = 0;

/// Fastest exit speed the landing check assumes, µm/s. Measured 2026-11-08 at the three transfer
/// planes of the all-pieces track (10 marbles each): hopper exit 0.243 m/s, one-way hole roll-over
/// 0.062–0.146 m/s, steep slope to `L1` 0.636–0.669 m/s. 0.75 m/s is the fastest plus about 12 %;
/// over the highest fall it still lands 58.7 mm past the plane, inside one cell. The measured first
/// landings matched the ballistic prediction (52–56 mm at 0.64–0.67 m/s).
pub const CONTROLLED_DROP_MAX_EXIT_SPEED_UM_S: i64 = 750_000;

/// Largest sideways offset between the predicted path (the source port's centreline) and the
/// landing port's centreline: the standard marble's play in the 28 mm channel, (28 − 16) / 2 =
/// 6 mm (contract §5.1, §6). Ports sit on cell centrelines (§3), so only the landing column under
/// the exit is within it; the next column is a whole cell away.
pub const CONTROLLED_DROP_LATERAL_TOLERANCE_UM: i32 =
    (CHANNEL_WIDTH_UM - MARBLE_STANDARD_DIAMETER_UM) / 2;

/// Plan radius of a funnel catcher's rim floor about its axis (blender/kit/kitfunnel.py
/// `RIM_R`): the bowl falls from there to the centre bore, and the rim gutter's half-round runs
/// outside it.
pub const FUNNEL_RIM_RADIUS_UM: i32 = 60_000;

/// Largest plan distance from a funnel's axis at which a falling marble's centre is caught: the
/// bowl opening (rim plus the gutter's channel radius, 74 mm) less the standard marble's radius.
pub const FUNNEL_CATCH_RADIUS_UM: i32 =
    FUNNEL_RIM_RADIUS_UM + CHANNEL_RADIUS_UM - MARBLE_STANDARD_DIAMETER_UM / 2;

/// Highest controlled fall into a funnel's bowl, source floor to rim floor: five port levels
/// (75 mm), an `L3` exit one layer over an `L2` rim. Measured in `parble_sim` (funnel_test): a
/// marble leaving such an exit over the bowl at 0.02, 0.3 and 0.75 m/s is caught every time and
/// leaves by the centre bore.
pub const FUNNEL_MAX_FALL_UM: i32 = 5 * LEVEL_STEP_UM;

/// Gravity the prediction uses, µm/s² (the sim's 9.81 m/s²).
pub const GRAVITY_UM_S2: i64 = 9_810_000;

const _: () = assert!(CONTROLLED_DROP_LATERAL_TOLERANCE_UM < CELL_UM);
const _: () = assert!(CONTROLLED_DROP_MAX_FALL_UM < CELL_UM);
const _: () = assert!(CONTROLLED_DROP_MAX_FALL_UM <= FUNNEL_MAX_FALL_UM);
const _: () = assert!(FUNNEL_MAX_FALL_UM < 2 * CELL_UM);
const _: () = assert!(
    0 <= CONTROLLED_DROP_MIN_EXIT_SPEED_UM_S
        && CONTROLLED_DROP_MIN_EXIT_SPEED_UM_S <= CONTROLLED_DROP_MAX_EXIT_SPEED_UM_S
);

/// A controlled free fall from an open side port onto a channel entry below (module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ControlledDrop {
    /// Placement and port index the marble leaves by.
    pub from: usize,
    pub from_port: usize,
    /// Placement it lands in, and the landing port; `None` for a funnel's bowl.
    pub to: usize,
    pub to_port: Option<usize>,
    /// Floor-to-floor fall, µm.
    pub fall_um: i32,
}

/// Whether a ballistic exit at every speed in the assumed range, falling `fall_um`, lands
/// `0 … CELL_UM` past the port plane. Distance `d = v·√(2Δ/g)`, compared squared in integers; the
/// slowest exit lands at `d ≥ 0` because the minimum speed is not negative.
fn lands_in_catch(fall_um: i32) -> bool {
    let fall = i128::from(fall_um);
    let g = i128::from(GRAVITY_UM_S2);
    let reach_sq = |v: i64| i128::from(v) * i128::from(v) * 2 * fall;
    let cell_sq = i128::from(CELL_UM) * i128::from(CELL_UM);
    reach_sq(CONTROLLED_DROP_MAX_EXIT_SPEED_UM_S) <= cell_sq * g
}

/// Squared plan distance, µm².
fn dist_sq(a: [i64; 2], b: [i64; 2]) -> i128 {
    let (dx, dz) = (i128::from(a[0] - b[0]), i128::from(a[1] - b[1]));
    dx * dx + dz * dz
}

/// Plan centre of a cell, µm.
fn plan_centre(c: CellCoord) -> [i64; 2] {
    [
        i64::from(c.x) * i64::from(CELL_UM),
        i64::from(c.z) * i64::from(CELL_UM),
    ]
}

/// Whether a ballistic exit from the side `port`, falling `fall_um` into the bowl of the funnel
/// `f`, lands within [`FUNNEL_CATCH_RADIUS_UM`] of its axis at every assumed speed: both ends of
/// the landing range (a straight segment along the exit normal) inside the catch disc.
fn lands_in_bowl(port: &Port, f: &Resolved<'_>, fall_um: i32) -> bool {
    let Some(bottom) = f
        .ports
        .iter()
        .find(|p| p.level.is_none() && p.face == PortFace::Bottom)
    else {
        return false;
    };
    let axis = plan_centre(bottom.cell);
    let n = port.face.normal();
    let (nx, nz) = (i64::from(n.x), i64::from(n.z));
    let centre = plan_centre(port.cell);
    let half = i64::from(HALF_CELL_UM);
    let plane = [centre[0] + nx * half, centre[1] + nz * half];
    // d = v·√(2Δ/g), rounded up so the far end is never under-estimated.
    let v = i128::from(CONTROLLED_DROP_MAX_EXIT_SPEED_UM_S);
    let d_sq = v * v * 2 * i128::from(fall_um) / i128::from(GRAVITY_UM_S2) + 1;
    let mut d: i64 = 0;
    while i128::from(d) * i128::from(d) < d_sq {
        d += 1;
    }
    let far = [plane[0] + nx * d, plane[1] + nz * d];
    let r_sq = i128::from(FUNNEL_CATCH_RADIUS_UM) * i128::from(FUNNEL_CATCH_RADIUS_UM);
    dist_sq(plane, axis) <= r_sq && dist_sq(far, axis) <= r_sq
}

/// Resolve every placement against the catalog; `None` for an unknown piece.
fn resolve<'a>(track: &Track, catalog: &'a impl PieceCatalog) -> Vec<Option<Resolved<'a>>> {
    track
        .placements
        .iter()
        .map(|p| {
            catalog.get(&p.piece).map(|def| {
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
                Resolved {
                    def,
                    cells: p.world_cells(def),
                    ports,
                }
            })
        })
        .collect()
}

/// Placements occupying each world cell, in index order.
fn occupancy(resolved: &[Option<Resolved<'_>>]) -> BTreeMap<CellCoord, Vec<usize>> {
    let mut occupancy: BTreeMap<CellCoord, Vec<usize>> = BTreeMap::new();
    for (i, r) in resolved.iter().enumerate() {
        for &cell in r.iter().flat_map(|r| &r.cells) {
            occupancy.entry(cell).or_default().push(i);
        }
    }
    occupancy
}

fn find_drops(
    resolved: &[Option<Resolved<'_>>],
    occupancy: &BTreeMap<CellCoord, Vec<usize>>,
) -> Vec<ControlledDrop> {
    let mut drops = Vec::new();
    for (i, r) in resolved.iter().enumerate() {
        let Some(r) = r else { continue };
        for (pi, port) in r.ports.iter().enumerate() {
            let Some(src_floor) = floor_um(port) else {
                continue;
            };
            let exit = port.cell + port.face.normal();
            if !allows_out(port.flow) || occupancy.contains_key(&exit) {
                continue;
            }
            // The marble falls down the empty column under `exit` onto the first occupied cell.
            let mut below = exit;
            let landing_cell = loop {
                below = below + PortFace::Bottom.normal();
                if CELL_UM * (below.y + 1) <= src_floor - FUNNEL_MAX_FALL_UM {
                    break None;
                }
                if occupancy.contains_key(&below) {
                    break Some(below);
                }
            };
            let Some(cell) = landing_cell else { continue };
            for &q in &occupancy[&cell] {
                let Some(rq) = &resolved[q] else { continue };
                if is_funnel(&rq.def.id) {
                    let rim = rq.ports.iter().filter_map(floor_um).max();
                    let fall_um = rim.map_or(0, |rim| src_floor - rim);
                    if 0 < fall_um
                        && fall_um <= FUNNEL_MAX_FALL_UM
                        && lands_in_bowl(port, rq, fall_um)
                    {
                        drops.push(ControlledDrop {
                            from: i,
                            from_port: pi,
                            to: q,
                            to_port: None,
                            fall_um,
                        });
                    }
                    continue;
                }
                for (qi, m) in rq.ports.iter().enumerate() {
                    let Some(floor) = floor_um(m) else { continue };
                    let fall_um = src_floor - floor;
                    if m.cell == cell
                        && m.face == port.face.opposite()
                        && m.class == PortClass::Channel
                        && allows_in(m.flow)
                        && 0 < fall_um
                        && fall_um <= CONTROLLED_DROP_MAX_FALL_UM
                        && lands_in_catch(fall_um)
                    {
                        drops.push(ControlledDrop {
                            from: i,
                            from_port: pi,
                            to: q,
                            to_port: Some(qi),
                            fall_um,
                        });
                    }
                }
            }
        }
    }
    drops
}

/// The controlled drops of `track` (module docs), sorted. Placements not in `catalog` take part
/// in none.
pub fn controlled_drops(track: &Track, catalog: &impl PieceCatalog) -> Vec<ControlledDrop> {
    let resolved = resolve(track, catalog);
    let mut drops = find_drops(&resolved, &occupancy(&resolved));
    drops.sort_unstable();
    drops
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

    let resolved = resolve(track, catalog);
    for (i, p) in track.placements.iter().enumerate() {
        if resolved[i].is_none() {
            issues.insert(on(
                i,
                vec![p.anchor],
                IssueKind::UnknownPiece {
                    piece: p.piece.clone(),
                },
            ));
        }
    }

    // Cell occupancy and overlaps.
    let occupancy = occupancy(&resolved);
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

    // Links and the directed flow graph between placements. Controlled drops carry flow from
    // source to landing only, and neither of their ports is open or one-sided.
    let mut edges: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    let drops = find_drops(&resolved, &occupancy);
    let mut drop_ports: BTreeSet<(usize, usize)> = BTreeSet::new();
    // Ports a marble can enter by: a link from a port that allows outflow, or a drop landing.
    let mut fed: BTreeSet<(usize, usize)> = BTreeSet::new();
    // Placements a marble falls into without a port (a funnel's bowl).
    let mut bowls: BTreeSet<usize> = BTreeSet::new();
    for d in &drops {
        edges.entry(d.from).or_default().insert(d.to);
        drop_ports.insert((d.from, d.from_port));
        match d.to_port {
            Some(to_port) => {
                drop_ports.insert((d.to, to_port));
                fed.insert((d.to, to_port));
            }
            None => {
                bowls.insert(d.to);
            }
        }
    }
    // Open ports and one-sided ports of this placement's own, decided once every link is known
    // (structural pieces and routes, module docs): (placement, port, issue).
    let mut unlinked: Vec<(usize, usize, Issue)> = Vec::new();
    for (i, r) in resolved.iter().enumerate() {
        let Some(r) = r else { continue };
        for (pi, port) in r.ports.iter().enumerate() {
            let facing = port.cell + port.face.normal();
            let neighbours: Vec<usize> = occupancy
                .get(&facing)
                .map(|occ| occ.iter().copied().filter(|&q| q != i).collect())
                .unwrap_or_default();
            if neighbours.is_empty() {
                if !r.cells.contains(&facing) && !drop_ports.contains(&(i, pi)) {
                    unlinked.push((
                        i,
                        pi,
                        on(
                            i,
                            vec![port.cell],
                            IssueKind::OpenPort {
                                port: pi,
                                face: port.face,
                                flow: port.flow,
                            },
                        ),
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
                    // One-sided: only this port exists on the shared face. A landing port's
                    // closed end is fine: the marble arrives from above.
                    if drop_ports.contains(&(i, pi)) {
                        continue;
                    }
                    unlinked.push((
                        i,
                        pi,
                        on(
                            i,
                            face_cells.clone(),
                            IssueKind::BrokenLink {
                                port: pi,
                                other: q,
                                other_port: None,
                                fault: LinkFault::NoMatingPort,
                            },
                        ),
                    ));
                    continue;
                };
                // A facing port that lets marbles out feeds this one, linked or not.
                if allows_out(m.flow) && allows_in(port.flow) {
                    fed.insert((i, pi));
                }
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

    // A structural piece no marble can enter keeps its ports unlinked. A port that only routes
    // lead to (it takes no marble in) is open only when a route to it starts at a fed port.
    let entered =
        |i: usize| bowls.contains(&i) || fed.range((i, 0)..=(i, usize::MAX)).next().is_some();
    let idle_support = |i: usize| {
        resolved[i]
            .as_ref()
            .is_some_and(|r| r.def.structural && !entered(i))
    };
    for (i, pi, issue) in unlinked {
        let Some(r) = &resolved[i] else { continue };
        if idle_support(i) {
            continue;
        }
        let routed_out = matches!(issue.kind, IssueKind::OpenPort { .. })
            && r.ports[pi].flow == PortFlow::Out
            && !r.def.routes.is_empty()
            && !bowls.contains(&i);
        if routed_out
            && !r
                .def
                .routes
                .iter()
                .any(|rt| rt.to == pi && fed.contains(&(i, rt.from)))
        {
            continue;
        }
        issues.insert(issue);
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
            if reached[i] || idle_support(i) {
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
