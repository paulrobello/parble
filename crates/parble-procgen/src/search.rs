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
    SeededRng, SubStream, Yaw4, clearance_cells, is_hopper,
};

use crate::params::{GenError, GenParams};
use crate::reach::Viability;
use crate::segment::{OpenPort, Space};
use crate::speed::{HOPPER_V2, TRANSFER_MAX_V2, enters, exit_v2, flows};
use crate::template::{Shape, Template, TemplateSet, mate_class};

/// Placements tried across all attempts before [`GenError::Exhausted`].
pub const NODE_BUDGET: u32 = 200_000;

/// Placements one attempt may try before it restarts with a fresh stream. Chronological
/// backtracking cannot undo an early mistake from deep in a long track, a restart can.
pub(crate) const ATTEMPT_NODES: u32 = 2_000;

/// Consecutive level pieces allowed before the track must descend again, for tracks whose
/// height leaves room to spare ([`max_flat_run`]).
pub(crate) const MAX_FLAT_RUN: u8 = 3;

/// The flat-run limit of a long track on a low hopper ([`max_flat_run`]).
pub(crate) const LONGEST_FLAT_RUN: u8 = 8;

/// Target pieces per layer of height above which a track gets [`LONGEST_FLAT_RUN`].
pub(crate) const FLAT_RUN_PIECES_PER_LAYER: u32 = 7;

/// The flat-run limit for `params`: [`MAX_FLAT_RUN`] up to [`FLAT_RUN_PIECES_PER_LAYER`] target
/// pieces a layer of height, [`LONGEST_FLAT_RUN`] above. At the old limit of 3 a walk ran out of
/// height above about seven pieces a layer (2026-11-15 `long_track_census`, 5 seeds a case: 90
/// pieces from layer 10 and 160 from layer 16 failed every seed); at 8, 100 from layer 10 and 160
/// from layer 16 pass all 5. The default params and every track under the ratio keep the limit
/// of 3 and generate exactly as before.
pub(crate) fn max_flat_run(params: &GenParams) -> u8 {
    if params.length > FLAT_RUN_PIECES_PER_LAYER * params.height {
        LONGEST_FLAT_RUN
    } else {
        MAX_FLAT_RUN
    }
}

/// Share of the target length at which [`Search::pace`] aims to reach layer 0, in per cent.
/// Steps that cannot stay level (a flat run at its limit, a blocked cell ahead) bring a track
/// down sooner than its pace, so the aim is past the full length: at 95 a 500-piece track from
/// layer 48 reached the ground near piece 410 of the 450 it needs and generated on 2 to 4 of 6
/// seeds, at 105 and above on 8 of 8 (2026-11-15 `long_track_census`).
pub(crate) const PACE_PER_CENT: i64 = 110;

/// The lowest port value a walk can still end from on layer 0: `L3`. A walk whose floor is
/// another layer ends from `L3` of that layer ([`Course::floor_value`]).
pub(crate) const FINISH_VALUE: i32 = 3;

/// How a walk ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ending {
    /// At a finish piece on the floor layer, with no open end left.
    Finish,
    /// At an open `L3` channel exit on the floor layer, for the next segment to continue from.
    Open,
}

/// The layers a walk may use and how it ends.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Course {
    /// Highest layer a piece may stand on: the layer the walk starts on.
    pub top: i32,
    /// Lowest layer a piece may stand on, and the layer the walk ends on.
    pub floor: i32,
    pub ending: Ending,
}

impl Course {
    /// The port value the walk ends at and never goes below: `L3` of the floor layer.
    pub fn floor_value(&self) -> i32 {
        4 * self.floor + FINISH_VALUE
    }
}

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

    fn from_open(o: &OpenPort, max_flat: u8) -> Self {
        Self {
            cell: o.cell,
            face: o.face,
            level: o.level,
            class: o.class,
            flow: PortFlow::Out,
            flat: o.flat.min(max_flat),
            v2: o.v2,
            catch: o.after_catch,
        }
    }

    fn open(&self) -> OpenPort {
        OpenPort {
            cell: self.cell,
            face: self.face,
            level: self.level,
            class: self.class,
            v2: self.v2,
            flat: self.flat,
            after_catch: self.catch,
        }
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
    /// Port levels the step descends.
    descent: i64,
}

struct Undo {
    cells: Vec<CellCoord>,
    clear: Vec<CellCoord>,
    reserved: Option<CellCoord>,
    nopiece: Option<CellCoord>,
    exits: usize,
}

struct Frame {
    end: End,
    /// Port levels per piece the rest of the track should descend, in thousandths.
    pace: i64,
    cands: Vec<Cand>,
    applied: Option<Undo>,
}

/// A finished walk: the track pieces in placement order and the cells it keeps clear.
pub(crate) struct Walk {
    pub placements: Vec<Placement>,
    pub occupied: BTreeSet<CellCoord>,
    /// Empty cells a transfer falls through. No support may stand in one.
    pub reserved: BTreeSet<CellCoord>,
    /// Cells over an `L2` or `L3` channel ([`parble_core::clearance_cells`]). No support may
    /// stand in one.
    pub clear: BTreeSet<CellCoord>,
    /// Cells under a transfer's source: no piece may take one, a support may.
    pub nopiece: BTreeSet<CellCoord>,
    /// The open end an [`Ending::Open`] walk stops at.
    pub end: Option<OpenPort>,
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
    /// Cells that stay empty over a placed piece's `L2` or `L3` channel.
    clear: BTreeSet<CellCoord>,
    ends: Vec<End>,
    max_flat: u8,
    course: Course,
    floor_value: i32,
}

impl<'a> Search<'a> {
    pub fn new(
        params: GenParams,
        set: &'a TemplateSet,
        via: &'a Viability,
        seed: u64,
        attempt: u32,
        course: Course,
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
            clear: BTreeSet::new(),
            ends: Vec::new(),
            max_flat: max_flat_run(&params),
            course,
            floor_value: course.floor_value(),
        }
    }

    /// Start from the cells of `space` instead of an empty plan.
    pub fn with_space(mut self, space: &Space) -> Self {
        self.occupied = space.occupied.clone();
        self.reserved = space.reserved.clone();
        self.clear = space.clear.clone();
        self.nopiece = space.no_piece.clone();
        self
    }

    /// Search from the open port `start` for a path to the course's ending, trying at most
    /// `node_cap` placements.
    pub fn run_from(mut self, start: &OpenPort, node_cap: u32) -> Result<Walk, GenError> {
        self.ends.push(End::from_open(start, self.max_flat));
        self.walk(node_cap)
    }

    /// Place the hopper on the top layer and search for a path to the course's ending, trying at
    /// most `node_cap` placements.
    pub fn run(mut self, hopper: &PieceDef, node_cap: u32) -> Result<Walk, GenError> {
        debug_assert!(is_hopper(&hopper.id));
        let orient = Orientation::new(Yaw4::new(self.rng.below(4) as u8), Default::default());
        let anchor = CellCoord::new(0, self.course.top, 0);
        let placed = hopper.transformed(orient);
        for c in &placed.footprint {
            self.occupied.insert(anchor + *c);
        }
        for c in clearance_cells(&placed.footprint, &placed.ports) {
            self.clear.insert(anchor + c);
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
        self.walk(node_cap)
    }

    /// Whether the walk has reached an [`Ending::Open`] course's end: one open end left, an `L3`
    /// channel exit on the floor layer, with the segment long enough ([`Search::may_finish`]).
    /// The two cells ahead of it
    /// must be free, so the next segment has room for a straight: an end facing the plan's edge
    /// one cell away leaves a marble too fast for a curve nowhere to go (3 of 30 ends, 2026-11-15).
    fn open_end_reached(&self) -> bool {
        self.course.ending == Ending::Open
            && self.ends.len() == 1
            && self.may_finish()
            && self.ends.last().is_some_and(|e| {
                let ahead = e.target() + e.face.normal();
                e.value() == self.floor_value
                    && e.level.is_some()
                    && e.face.is_side()
                    && e.class == PortClass::Channel
                    && self.in_plan(ahead)
                    && !self.taken(ahead)
                    && !self.clear.contains(&ahead)
            })
    }

    fn walk(mut self, node_cap: u32) -> Result<Walk, GenError> {
        let mut frames: Vec<Frame> = Vec::new();
        let mut nodes = 0u32;
        loop {
            if self.ends.is_empty() || self.open_end_reached() {
                return Ok(Walk {
                    end: self.ends.last().map(End::open),
                    placements: self.placements,
                    occupied: self.occupied,
                    reserved: self.reserved,
                    clear: self.clear,
                    nopiece: self.nopiece,
                });
            }
            let end = self.ends.last().cloned().expect("ends is not empty");
            let cands = self.candidates(&end);
            let pace = self.pace(&end);
            frames.push(Frame {
                end,
                pace,
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
                if let Some(c) = draw(
                    &mut self.rng,
                    self.params.special_per_mille,
                    top.pace,
                    &mut top.cands,
                ) {
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
        let clear: Vec<CellCoord> = t
            .clear
            .iter()
            .map(|rel| c.anchor + *rel)
            .filter(|w| self.clear.insert(*w))
            .collect();
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
            let flat = if d == 0 {
                (end.flat + 1).min(self.max_flat)
            } else {
                0
            };
            let v2 = exit_v2(end.v2, t.shape, d);
            let mut e = End::of(c.anchor, p, flat, v2);
            e.catch = t.shape == Shape::Catch;
            self.ends.push(e);
        }
        Undo {
            cells,
            clear,
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
        for c in u.clear {
            self.clear.remove(&c);
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
        c.x.abs() <= r && c.z.abs() <= r && (self.course.floor..=self.course.top).contains(&c.y)
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
                || self.clear.contains(&w)
                || blocked.contains(&w)
                || others.contains(&w)
                || under_gap
            {
                return false;
            }
        }
        // Nothing may stand over this piece's own L2 or L3 channel, now or later.
        if t.clear.iter().any(|rel| {
            let w = anchor + *rel;
            self.occupied.contains(&w) || others.contains(&w)
        }) {
            return false;
        }
        t.exits.iter().all(|p| {
            let f = anchor + p.cell + p.face.normal();
            self.in_plan(f)
                && !self.taken(f)
                && !self.clear.contains(&f)
                && !t.clear.iter().any(|rel| anchor + *rel == f)
                && !own.contains(&f)
                && !blocked.contains(&f)
                && !others.contains(&f)
        })
    }

    /// Whether the track is long enough to end: a finish on the floor layer ends it once the
    /// finish brings it to nine tenths of its target length, an open end once the pieces placed
    /// are nine tenths. A track that reaches the floor sooner runs level there until it is
    /// ([`Search::descends`]).
    fn may_finish(&self) -> bool {
        let closing = u32::from(self.course.ending == Ending::Finish);
        (self.placements.len() as u32 + closing) * 10 >= self.params.length * 9
    }

    /// Port levels per piece the track still has to descend from `end` to reach the floor layer
    /// at [`PACE_PER_CENT`] of its target length, in thousandths.
    fn pace(&self, end: &End) -> i64 {
        let need = i64::from((end.value() - self.floor_value).max(0));
        let target = i64::from(self.params.length) * PACE_PER_CENT / 100;
        let remaining = (target - self.placements.len() as i64).max(1);
        need * 1000 / remaining
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
        let want_finish = self.course.ending == Ending::Finish && self.may_finish();
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
            if !self.taken(gap)
                && landing.y >= self.course.floor
                && end.v2 <= TRANSFER_MAX_V2
                && !end.catch
            {
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
    /// still high enough to reach the ending. A finish is allowed only on the floor layer.
    fn descends(&self, t: &Template, anchor: CellCoord, end: &End, finish: bool) -> bool {
        if finish {
            return anchor.y + t.entry.cell.y == self.course.floor;
        }
        if !enters(t.shape, end.v2) {
            return false;
        }
        t.exits.iter().all(|p| {
            let v = End::of(anchor, p, 0, 0).value();
            let d = end.value() - v;
            let out = exit_v2(end.v2, t.shape, d);
            // On the ground a track that is still too short runs level past the flat-run limit.
            let grounded = v == self.floor_value && !self.may_finish();
            d >= 0
                && v >= self.floor_value
                && (d > 0 || end.flat < self.max_flat || grounded)
                && flows(out)
                && self.via.viable(
                    v,
                    p.level,
                    out,
                    if d == 0 {
                        (end.flat + 1).min(self.max_flat)
                    } else {
                        0
                    },
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
            Shape::Turn | Shape::TurnTop | Shape::RailTurn => 3 + diff / 15,
            Shape::Slope => 4,
            Shape::Steep => 1 + diff / 20,
            Shape::Redirect => 3,
            Shape::Drop | Shape::DropLow => 2,
            Shape::Catch => 8,
            Shape::Special | Shape::Arch => 4,
            Shape::Other => 2,
        };
        let descent = t.exits.first().map_or(0, |p| {
            i64::from(end.value() - End::of(anchor, p, 0, 0).value())
        });
        Cand {
            finish,
            tpl,
            anchor,
            transfer,
            weight: if finish { 1 } else { base.max(1) as u32 },
            special: matches!(t.shape, Shape::Special | Shape::Arch),
            descent,
        }
    }
}

fn sub(a: CellCoord, b: CellCoord) -> CellCoord {
    CellCoord::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

/// Weight of a descent one level off the two nearest the pace; it halves with each further level.
const OFF_PACE_WEIGHT: i64 = 30;

/// Weights for drawing one of `descents` (port levels a step descends, ascending) so the steps
/// average `pace` thousandths of a level per piece. The two whole descents either side of the
/// pace share 1000, and every other descent keeps a small weight ([`OFF_PACE_WEIGHT`]) so steep
/// slopes, transfers and drops still turn up on a slow track; the share of the nearer two is
/// shifted to make up for what those add. When the pace is too slow to make up for them, they
/// get no weight.
fn pace_weights(descents: &[i64], pace: i64) -> Vec<u32> {
    let below = pace.div_euclid(1000);
    let near = |d: i64| d == below || d == below + 1;
    let off = |d: i64| {
        let levels = if d < below { below - d } else { d - below - 1 };
        OFF_PACE_WEIGHT >> (levels - 1).min(5)
    };
    let others: i64 = descents
        .iter()
        .filter(|&&d| !near(d))
        .map(|&d| off(d))
        .sum();
    let others_levels: i64 = descents
        .iter()
        .filter(|&&d| !near(d))
        .map(|&d| off(d) * d)
        .sum();
    // Weight of `below + 1`, with `1000 - upper` for `below`.
    let upper = pace * (1000 + others) / 1000 - 1000 * below - others_levels;
    let keep_others = (0..=1000).contains(&upper) || !descents.iter().any(|&d| near(d));
    let upper = if keep_others {
        upper.clamp(0, 1000)
    } else {
        pace.rem_euclid(1000)
    };
    descents
        .iter()
        .map(|&d| {
            let w = if d == below {
                1000 - upper
            } else if d == below + 1 {
                upper
            } else if keep_others {
                off(d)
            } else {
                0
            };
            w as u32
        })
        .collect()
}

/// Draw an index into `weights`.
fn pick(rng: &mut SubStream, weights: &[u32]) -> usize {
    let total: u32 = weights.iter().sum();
    let mut r = rng.below(total.max(1));
    for (i, &w) in weights.iter().enumerate() {
        if r < w {
            return i;
        }
        r -= w;
    }
    weights.iter().rposition(|&w| w > 0).unwrap_or(0)
}

/// Remove and return the next candidate: a finish while one fits, else a special or a plain
/// piece by the density slider. Within that pool the descent is drawn first, by
/// [`pace_weights`], then a piece of that descent by its own weight.
fn draw(
    rng: &mut SubStream,
    special_per_mille: u32,
    pace: i64,
    cands: &mut Vec<Cand>,
) -> Option<Cand> {
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
        let mut pool =
            if rng.below(1000) < special_per_mille && !special.is_empty() || plain.is_empty() {
                special
            } else {
                plain
            };
        let mut descents: Vec<i64> = pool.iter().map(|&i| cands[i].descent).collect();
        descents.sort_unstable();
        descents.dedup();
        let descent = descents[pick(rng, &pace_weights(&descents, pace))];
        pool.retain(|&i| cands[i].descent == descent);
        pool
    } else {
        finish
    };
    let weights: Vec<u32> = pool.iter().map(|&i| cands[i].weight).collect();
    Some(cands.remove(pool[pick(rng, &weights)]))
}
