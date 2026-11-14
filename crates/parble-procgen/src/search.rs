//! The depth-first search that grows a track from the hopper to a finish.
//!
//! State is a stack of open ends (ports whose mate is not placed yet). Each step extends the
//! last open end with a template whose entry port mates with it, or with a transfer: an `L1`
//! exit that drops 30 mm through an empty cell onto an `L3` inlet one layer below, the link
//! `parble_core::controlled_drops` accepts. A dead end backtracks. The budget is a node count,
//! never a clock, so a run is a pure function of its inputs.

use std::collections::BTreeSet;

use parble_core::{
    CellCoord, Orientation, PieceDef, Placement, Port, PortClass, PortFace, PortFlow, PortLevel,
    SeededRng, SubStream, Yaw4, is_hopper,
};

use crate::params::{GenError, GenParams};
use crate::reach::Viability;
use crate::speed::{HOPPER_V2, MIN_FLOW_V2, TRANSFER_MAX_V2, enters, exit_v2};
use crate::template::{Shape, Template, TemplateSet, mate_class};

/// Placements tried across all attempts before [`GenError::Exhausted`].
pub const NODE_BUDGET: u32 = 200_000;

/// Placements one attempt may try before it restarts with a fresh stream. Chronological
/// backtracking cannot undo an early mistake from deep in a long track, a restart can.
pub(crate) const ATTEMPT_NODES: u32 = 2_000;

/// Consecutive level pieces allowed before the track must descend again.
pub(crate) const MAX_FLAT_RUN: u8 = 3;

/// The lowest port value a track can still finish from: `L3` of layer 0.
pub(crate) const FINISH_VALUE: i32 = 3;

/// An open port: the piece cell it sits in, its face and what a mate must offer.
#[derive(Debug, Clone)]
struct End {
    cell: CellCoord,
    face: PortFace,
    level: Option<PortLevel>,
    class: PortClass,
    flow: PortFlow,
    /// Level pieces since the last descent.
    flat: u8,
    /// Estimated squared speed of a marble leaving this port, in 1e-3 m²/s².
    v2: i64,
    /// Whether the piece this port belongs to is a catch.
    catch: bool,
}

impl End {
    fn of(anchor: CellCoord, p: &Port, flat: u8, v2: i64) -> Self {
        Self {
            cell: anchor + p.cell,
            face: p.face,
            level: p.level,
            class: p.class,
            flow: p.flow,
            flat,
            v2,
            catch: false,
        }
    }

    /// The cell the port faces.
    fn target(&self) -> CellCoord {
        self.cell + self.face.normal()
    }

    /// Height in port levels (four per layer). A drop port counts as the `L1` exit of the catch
    /// one layer below it.
    fn value(&self) -> i32 {
        4 * self.cell.y + self.level.map_or(-3, PortLevel::index)
    }
}

#[derive(Debug, Clone)]
struct Cand {
    finish: bool,
    tpl: usize,
    anchor: CellCoord,
    /// The empty cell a transfer falls through.
    transfer: Option<CellCoord>,
    weight: u32,
    special: bool,
}

struct Undo {
    cells: Vec<CellCoord>,
    reserved: Option<CellCoord>,
    nopiece: Option<CellCoord>,
    exits: usize,
}

struct Frame {
    end: End,
    cands: Vec<Cand>,
    applied: Option<Undo>,
}

/// A finished walk: the track pieces in placement order and the cells it keeps clear.
pub(crate) struct Walk {
    pub placements: Vec<Placement>,
    pub occupied: BTreeSet<CellCoord>,
    /// Empty cells a transfer falls through. No support may stand in one.
    pub reserved: BTreeSet<CellCoord>,
}

pub(crate) struct Search<'a> {
    params: GenParams,
    set: &'a TemplateSet,
    via: &'a Viability,
    rng: SubStream,
    placements: Vec<Placement>,
    occupied: BTreeSet<CellCoord>,
    reserved: BTreeSet<CellCoord>,
    /// Cells no piece may take, but a support may: under a transfer's source.
    nopiece: BTreeSet<CellCoord>,
    ends: Vec<End>,
}

impl<'a> Search<'a> {
    pub fn new(
        params: GenParams,
        set: &'a TemplateSet,
        via: &'a Viability,
        seed: u64,
        attempt: u32,
    ) -> Self {
        Self {
            params,
            set,
            via,
            rng: SeededRng::new(seed).stream(&format!("procgen/walk/{attempt}")),
            placements: Vec::new(),
            occupied: BTreeSet::new(),
            reserved: BTreeSet::new(),
            nopiece: BTreeSet::new(),
            ends: Vec::new(),
        }
    }

    /// Place the hopper on the top layer and search for a path to a finish, trying at most
    /// `node_cap` placements.
    pub fn run(mut self, hopper: &PieceDef, node_cap: u32) -> Result<Walk, GenError> {
        debug_assert!(is_hopper(&hopper.id));
        let orient = Orientation::new(Yaw4::new(self.rng.below(4) as u8), Default::default());
        let anchor = CellCoord::new(0, self.params.height as i32, 0);
        let placed = hopper.transformed(orient);
        for c in &placed.footprint {
            self.occupied.insert(anchor + *c);
        }
        self.placements.push(Placement::new(
            hopper.id.clone(),
            anchor,
            orient.yaw,
            orient.mirror,
        ));
        for p in placed
            .ports
            .iter()
            .filter(|p| matches!(p.flow, PortFlow::Out | PortFlow::Both))
        {
            self.ends.push(End::of(anchor, p, 0, HOPPER_V2));
        }

        let mut frames: Vec<Frame> = Vec::new();
        let mut nodes = 0u32;
        loop {
            if self.ends.is_empty() {
                return Ok(Walk {
                    placements: self.placements,
                    occupied: self.occupied,
                    reserved: self.reserved,
                });
            }
            let end = self.ends.last().cloned().expect("ends is not empty");
            let cands = self.candidates(&end);
            frames.push(Frame {
                end,
                cands,
                applied: None,
            });
            loop {
                let Some(top) = frames.last_mut() else {
                    return Err(GenError::NoRoute);
                };
                if let Some(u) = top.applied.take() {
                    let end = top.end.clone();
                    self.undo(u, end);
                }
                let top = frames.last_mut().expect("frame still on the stack");
                if let Some(c) = draw(&mut self.rng, self.params.special_per_mille, &mut top.cands)
                {
                    nodes += 1;
                    if nodes > node_cap {
                        return Err(GenError::Exhausted { nodes: node_cap });
                    }
                    let end = top.end.clone();
                    top.applied = Some(self.apply(&c, &end));
                    break;
                }
                frames.pop();
            }
        }
    }

    fn template(&self, c: &Cand) -> &'a Template {
        if c.finish {
            &self.set.finish[c.tpl]
        } else {
            &self.set.chain[c.tpl]
        }
    }

    fn apply(&mut self, c: &Cand, end: &End) -> Undo {
        let t = self.template(c);
        self.ends.pop();
        let mut cells = Vec::new();
        for rel in &t.cells {
            let w = c.anchor + *rel;
            self.occupied.insert(w);
            cells.push(w);
        }
        self.placements.push(Placement::new(
            t.id.clone(),
            c.anchor,
            t.orient.yaw,
            t.orient.mirror,
        ));
        let mut reserved = None;
        let mut nopiece = None;
        if let Some(gap) = c.transfer {
            self.reserved.insert(gap);
            reserved = Some(gap);
            let under = CellCoord::new(end.cell.x, end.cell.y - 1, end.cell.z);
            self.nopiece.insert(under);
            nopiece = Some(under);
        }
        let descent = |p: &Port| end.value() - End::of(c.anchor, p, 0, 0).value();
        for p in t.exits.iter().rev() {
            let d = descent(p);
            let flat = if d == 0 { end.flat + 1 } else { 0 };
            let v2 = exit_v2(end.v2, t.shape, d);
            let mut e = End::of(c.anchor, p, flat, v2);
            e.catch = t.shape == Shape::Catch;
            self.ends.push(e);
        }
        Undo {
            cells,
            reserved,
            nopiece,
            exits: t.exits.len(),
        }
    }

    fn undo(&mut self, u: Undo, end: End) {
        for _ in 0..u.exits {
            self.ends.pop();
        }
        self.ends.push(end);
        self.placements.pop();
        for c in u.cells {
            self.occupied.remove(&c);
        }
        if let Some(c) = u.reserved {
            self.reserved.remove(&c);
        }
        if let Some(c) = u.nopiece {
            self.nopiece.remove(&c);
        }
    }

    fn in_plan(&self, c: CellCoord) -> bool {
        let r = self.params.footprint as i32;
        c.x.abs() <= r && c.z.abs() <= r && (0..=self.params.height as i32).contains(&c.y)
    }

    fn taken(&self, c: CellCoord) -> bool {
        self.occupied.contains(&c) || self.reserved.contains(&c) || self.nopiece.contains(&c)
    }

    /// Whether `t` placed at `anchor` leaves the track consistent: its cells are free, its exits
    /// face free cells, and no other open end loses its mating cell.
    fn fits(&self, t: &Template, anchor: CellCoord, blocked: &[CellCoord]) -> bool {
        let own: BTreeSet<CellCoord> = t.cells.iter().map(|c| anchor + *c).collect();
        let others: Vec<CellCoord> = self
            .ends
            .iter()
            .take(self.ends.len().saturating_sub(1))
            .map(End::target)
            .collect();
        for &w in &own {
            let under_gap = self
                .reserved
                .iter()
                .any(|g| g.x == w.x && g.z == w.z && w.y > g.y);
            if !self.in_plan(w)
                || self.taken(w)
                || blocked.contains(&w)
                || others.contains(&w)
                || under_gap
            {
                return false;
            }
        }
        t.exits.iter().all(|p| {
            let f = anchor + p.cell + p.face.normal();
            self.in_plan(f)
                && !self.taken(f)
                && !own.contains(&f)
                && !blocked.contains(&f)
                && !others.contains(&f)
        })
    }

    fn candidates(&self, end: &End) -> Vec<Cand> {
        let mut out = Vec::new();
        let placed = self.placements.len() as u32;
        if placed > 2 * self.params.length + 16 {
            return out;
        }
        let Some(mclass) = mate_class(end.class) else {
            return out;
        };
        if !matches!(end.flow, PortFlow::Out | PortFlow::Both) {
            return out;
        }
        // A finish on layer 0 ends the track once it has three quarters of its target length:
        // the descent cannot always be timed to land on the last piece.
        let want_finish = placed * 4 >= self.params.length * 3;
        let target = end.target();
        let key = (end.face.opposite(), end.level, mclass);
        for finish in [false, true] {
            if finish && !want_finish {
                continue;
            }
            let (list, index) = if finish {
                (&self.set.finish, &self.set.finish_by_entry)
            } else {
                (&self.set.chain, &self.set.chain_by_entry)
            };
            for &i in index.get(&key).into_iter().flatten() {
                let t = &list[i];
                let anchor = sub(target, t.entry.cell);
                if self.fits(t, anchor, &[]) && self.descends(t, anchor, end, finish) {
                    out.push(self.cand(finish, i, anchor, None, t, end));
                }
            }
        }
        if end.face.is_side() && end.class == PortClass::Channel && end.level == Some(PortLevel::L1)
        {
            let gap = target;
            let landing = CellCoord::new(gap.x, gap.y - 1, gap.z);
            let under = CellCoord::new(end.cell.x, end.cell.y - 1, end.cell.z);
            let key = (end.face.opposite(), Some(PortLevel::L3), PortClass::Channel);
            if !self.taken(gap) && landing.y >= 0 && end.v2 <= TRANSFER_MAX_V2 && !end.catch {
                for finish in [false, true] {
                    if finish && !want_finish {
                        continue;
                    }
                    let (list, index) = if finish {
                        (&self.set.finish, &self.set.finish_by_entry)
                    } else {
                        (&self.set.chain, &self.set.chain_by_entry)
                    };
                    for &i in index.get(&key).into_iter().flatten() {
                        let t = &list[i];
                        let anchor = sub(landing, t.entry.cell);
                        if self.fits(t, anchor, &[gap, under])
                            && self.descends(t, anchor, end, finish)
                        {
                            out.push(self.cand(finish, i, anchor, Some(gap), t, end));
                        }
                    }
                }
            }
        }
        out
    }

    /// Whether every exit of `t` stays at or below the end, within the flat-run limit, and is
    /// still high enough to reach a finish. A finish is allowed only on layer 0.
    fn descends(&self, t: &Template, anchor: CellCoord, end: &End, finish: bool) -> bool {
        if finish {
            return anchor.y + t.entry.cell.y == 0;
        }
        if !enters(t.shape, end.v2) {
            return false;
        }
        t.exits.iter().all(|p| {
            let v = End::of(anchor, p, 0, 0).value();
            let d = end.value() - v;
            let out = exit_v2(end.v2, t.shape, d);
            d >= 0
                && v >= FINISH_VALUE
                && (d > 0 || end.flat < MAX_FLAT_RUN)
                && out >= MIN_FLOW_V2
                && self.via.viable(
                    v,
                    p.level,
                    out,
                    if d == 0 { end.flat + 1 } else { 0 },
                    t.shape == Shape::Catch,
                )
        })
    }

    fn cand(
        &self,
        finish: bool,
        tpl: usize,
        anchor: CellCoord,
        transfer: Option<CellCoord>,
        t: &Template,
        end: &End,
    ) -> Cand {
        let diff = i64::from(self.params.difficulty);
        let base: i64 = match t.shape {
            Shape::Straight => 6,
            Shape::Turn | Shape::RailTurn => 3 + diff / 15,
            Shape::Slope => 4,
            Shape::Steep => 1 + diff / 20,
            Shape::Redirect => 3,
            Shape::Drop => 2,
            Shape::Catch => 8,
            Shape::Special => 4,
            Shape::Other => 2,
        };
        // Steer the descent so the track reaches layer 0 as the length target is met.
        let need = i64::from((end.value() - FINISH_VALUE).max(0));
        let remaining = i64::from(
            self.params
                .length
                .saturating_sub(self.placements.len() as u32)
                .max(1),
        );
        let want_milli = need * 1000 / remaining;
        let d = t.exits.first().map_or(0, |p| {
            i64::from(end.value() - End::of(anchor, p, 0, 0).value())
        });
        let diff_milli = (d * 1000 - want_milli).abs();
        let mult = 1 + 16_000 / (1000 + diff_milli);
        Cand {
            finish,
            tpl,
            anchor,
            transfer,
            weight: if finish {
                1
            } else {
                (base * mult).clamp(1, 10_000) as u32
            },
            special: t.shape == Shape::Special,
        }
    }
}

fn sub(a: CellCoord, b: CellCoord) -> CellCoord {
    CellCoord::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

/// Remove and return the next candidate: a finish while one fits, else a special or a plain
/// piece by the density slider, weighted within the pool.
fn draw(rng: &mut SubStream, special_per_mille: u32, cands: &mut Vec<Cand>) -> Option<Cand> {
    if cands.is_empty() {
        return None;
    }
    let indices = |f: &dyn Fn(&Cand) -> bool| -> Vec<usize> {
        cands
            .iter()
            .enumerate()
            .filter(|&(_, c)| f(c))
            .map(|(i, _)| i)
            .collect()
    };
    let finish = indices(&|c| c.finish);
    let pool = if finish.is_empty() {
        let special = indices(&|c| !c.finish && c.special);
        let plain = indices(&|c| !c.finish && !c.special);
        if rng.below(1000) < special_per_mille && !special.is_empty() || plain.is_empty() {
            special
        } else {
            plain
        }
    } else {
        finish
    };
    let total: u32 = pool.iter().map(|&i| cands[i].weight).sum();
    let mut r = rng.below(total.max(1));
    for &i in &pool {
        if r < cands[i].weight {
            return Some(cands.remove(i));
        }
        r -= cands[i].weight;
    }
    pool.last().map(|&i| cands.remove(i))
}
