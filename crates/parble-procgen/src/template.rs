//! Oriented piece templates the search places: one per (piece, orientation, entry port).

use std::collections::BTreeMap;

use parble_core::{
    CellCoord, Mirror, Orientation, PieceDef, PieceId, Port, PortClass, PortFace, PortFlow,
    PortLevel, Yaw4, is_finish, is_hopper,
};

use crate::params::Families;

/// What a piece does to a marble's path. Drives the step weights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shape {
    Straight,
    Turn,
    /// A rail curve or S: it keeps the marble's speed, a block curve does not.
    RailTurn,
    Slope,
    Steep,
    Redirect,
    Drop,
    Catch,
    /// A mechanism the density slider controls (gate, checkpoint, rope arch).
    Special,
    Other,
}

impl Shape {
    fn of(id: &PieceId) -> Self {
        let mut seg = id.as_str().split('_').skip(1);
        match (seg.next(), seg.next()) {
            (Some("straight"), _) => Self::Straight,
            (Some("curve90" | "s"), _) if id.as_str().starts_with("rail_") => Self::RailTurn,
            (Some("curve90" | "s"), _) => Self::Turn,
            (Some("slope"), Some("steep")) => Self::Steep,
            (Some("slope"), _) => Self::Slope,
            (Some("redirect"), _) => Self::Redirect,
            (Some("drop"), _) => Self::Drop,
            (Some("catch"), _) => Self::Catch,
            (Some("gate" | "checkpoint" | "arch"), _) => Self::Special,
            _ => Self::Other,
        }
    }
}

/// A piece in one orientation, entered through `entry`. Ports and cells are relative to the
/// anchor, so the anchor is the target cell minus `entry.cell`.
#[derive(Debug, Clone)]
pub(crate) struct Template {
    pub id: PieceId,
    pub orient: Orientation,
    pub entry: Port,
    pub exits: Vec<Port>,
    pub cells: Vec<CellCoord>,
    pub shape: Shape,
}

/// What an open end needs of the entry port that mates with it.
pub(crate) type EntryKey = (PortFace, Option<PortLevel>, PortClass);

/// All templates the search may chain, the finish templates, and the hopper definition.
#[derive(Debug, Default)]
pub(crate) struct TemplateSet {
    pub chain: Vec<Template>,
    pub finish: Vec<Template>,
    pub chain_by_entry: BTreeMap<EntryKey, Vec<usize>>,
    pub finish_by_entry: BTreeMap<EntryKey, Vec<usize>>,
}

pub(crate) fn mate_class(c: PortClass) -> Option<PortClass> {
    match c {
        PortClass::Channel => Some(PortClass::Channel),
        PortClass::Drop => Some(PortClass::Drop),
        PortClass::Socket | PortClass::Spigot => None,
    }
}

fn orientations(chiral: bool) -> impl Iterator<Item = Orientation> {
    (0..4u8).flat_map(move |k| {
        let mirrors: &[Mirror] = if chiral {
            &[Mirror::Off, Mirror::On]
        } else {
            &[Mirror::Off]
        };
        mirrors
            .iter()
            .map(move |&m| Orientation::new(Yaw4::new(k), m))
    })
}

/// A port's height in port levels (four per layer) relative to the piece's anchor. A `Top` drop
/// port counts as the `L1` exit of the cell below the one that drops into it, a `Bottom` drop
/// port as the `L1` exit of the cell below it, the same convention as the search's open ends.
pub(crate) fn rel_value(p: &Port) -> i32 {
    match (p.level, p.face) {
        (Some(l), _) => 4 * p.cell.y + l.index(),
        (None, PortFace::Top) => 4 * p.cell.y + 1,
        (None, _) => 4 * p.cell.y - 3,
    }
}

/// A port's height as the search's open ends count it: a drop port is the `L1` exit of the catch
/// one layer below.
pub(crate) fn end_value(p: &Port) -> i32 {
    4 * p.cell.y + p.level.map_or(-3, PortLevel::index)
}

fn flows_in(f: PortFlow) -> bool {
    matches!(f, PortFlow::In | PortFlow::Both)
}

fn flows_out(f: PortFlow) -> bool {
    matches!(f, PortFlow::Out | PortFlow::Both)
}

/// Pieces the generator leaves out because the sim measured marbles failing on them, however
/// slowly or quickly they arrive (first marble of 100 to 300 seeds: `blk_drop_l3` 2 of 7 tracks,
/// `rail_s_1x3_l1` 5 of 59, `rail_curve90_r90_l1` 6 of 181 once the other failures were gone).
const UNRELIABLE: [&str; 3] = ["blk_drop_l3", "rail_s_1x3_l1", "rail_curve90_r90_l1"];

/// Orientations of `def` in the order the search enumerates them, with the entry port index of
/// every template. `max_exits` caps the outflow ports a chained piece may open.
pub(crate) fn build(defs: &[PieceDef], families: Families, max_exits: usize) -> TemplateSet {
    let mut set = TemplateSet::default();
    for def in defs {
        let hopper = is_hopper(&def.id);
        let finish = is_finish(&def.id);
        if hopper
            || def.structural
            || !def.routes.is_empty()
            || UNRELIABLE.contains(&def.id.as_str())
        {
            continue;
        }
        if !finish && !families.allows(def.id.family()) {
            continue;
        }
        let classes_ok = def
            .ports
            .iter()
            .all(|p| matches!(p.class, PortClass::Channel | PortClass::Drop));
        if !classes_ok {
            continue;
        }
        let shape = Shape::of(&def.id);
        for orient in orientations(def.chiral) {
            let placed = def.transformed(orient);
            for (ei, entry) in placed.ports.iter().enumerate() {
                if !flows_in(entry.flow) {
                    continue;
                }
                let exits: Vec<Port> = placed
                    .ports
                    .iter()
                    .enumerate()
                    .filter(|&(i, _)| i != ei)
                    .map(|(_, p)| *p)
                    .collect();
                let usable = if finish {
                    exits.is_empty()
                } else {
                    !exits.is_empty()
                        && exits.len() <= max_exits
                        && exits.iter().all(|p| flows_out(p.flow))
                        && exits.iter().all(|p| rel_value(p) <= rel_value(entry))
                };
                if !usable {
                    continue;
                }
                let Some(class) = mate_class(entry.class) else {
                    continue;
                };
                let key: EntryKey = (entry.face, entry.level, class);
                let t = Template {
                    id: def.id.clone(),
                    orient,
                    entry: *entry,
                    exits,
                    cells: placed.footprint.clone(),
                    shape,
                };
                let (list, index) = if finish {
                    (&mut set.finish, &mut set.finish_by_entry)
                } else {
                    (&mut set.chain, &mut set.chain_by_entry)
                };
                index.entry(key).or_default().push(list.len());
                list.push(t);
            }
        }
    }
    set
}

/// A level a marble's path can be at, ignoring the layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Lvl {
    Side(PortLevel),
    Drop,
}

fn lvl(p: &Port) -> Lvl {
    p.level.map_or(Lvl::Drop, Lvl::Side)
}

/// Whether the chainable pieces can lead from a hopper exit at `start` to a finish: by mating
/// ports, and by transfers (an `L1` exit onto an `L3` inlet one layer down, which only a piece
/// no taller than its entry cell can take). The check ignores space and layers, so a `true`
/// does not promise a route, but a `false` rules every seed out.
pub(crate) fn can_finish(set: &TemplateSet, start: Option<PortLevel>) -> bool {
    let mut seen: Vec<Lvl> = start.map(Lvl::Side).into_iter().collect();
    let mut i = 0;
    while i < seen.len() {
        let here = seen[i];
        i += 1;
        let mut reach = |next: Lvl| {
            if !seen.contains(&next) {
                seen.push(next);
            }
        };
        for t in &set.chain {
            let enters = lvl(&t.entry) == here;
            let transfers = here == Lvl::Side(PortLevel::L1)
                && lvl(&t.entry) == Lvl::Side(PortLevel::L3)
                && t.entry.class == PortClass::Channel
                && t.cells.iter().all(|c| c.y <= t.entry.cell.y);
            if enters || transfers {
                for p in &t.exits {
                    reach(lvl(p));
                }
            }
        }
    }
    seen.contains(&Lvl::Side(PortLevel::L3))
        && set
            .finish
            .iter()
            .any(|t| lvl(&t.entry) == Lvl::Side(PortLevel::L3))
}
