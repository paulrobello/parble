//! Whether a marble at an open end can still reach a finish, given its estimated speed.
//!
//! The search prunes a step whose exit has no way forward under the speed rules of
//! [`crate::speed`] instead of finding out by backtracking. The walk here ignores space, so a `true` does not promise a route, but a `false` is a certain dead end.

use std::cell::RefCell;
use std::collections::BTreeMap;

use parble_core::{PortClass, PortLevel};

use crate::search::{FINISH_VALUE, MAX_FLAT_RUN};
use crate::speed::{MIN_FLOW_V2, TRANSFER_MAX_V2, enters, exit_v2};
use crate::template::{Shape, TemplateSet, end_value, rel_value};

/// Speeds within this many 1e-3 m²/s² share a memo entry.
const BUCKET: i64 = 4;

/// A chainable template reduced to what the walk needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Step {
    entry: Option<PortLevel>,
    exit: Option<PortLevel>,
    /// Exit value minus entry value, in port levels.
    delta: i32,
    shape: ShapeKey,
    /// Whether a marble falling from an `L1` exit may land on the entry.
    lands: bool,
}

/// `Shape` is not `Ord`, so the steps keep it as an index.
type ShapeKey = u8;

const SHAPES: [Shape; 10] = [
    Shape::Straight,
    Shape::Turn,
    Shape::RailTurn,
    Shape::Slope,
    Shape::Steep,
    Shape::Redirect,
    Shape::Drop,
    Shape::Catch,
    Shape::Special,
    Shape::Other,
];

fn shape_key(s: Shape) -> ShapeKey {
    SHAPES
        .iter()
        .position(|&x| x == s)
        .expect("every shape is listed") as ShapeKey
}

/// A finish reduced to the layer-0 value its entry sits at.
#[derive(Debug, Clone, Copy)]
struct Goal {
    entry: Option<PortLevel>,
    class: PortClass,
    value: i32,
}

type Key = (i32, i64, i32, u8, bool);

pub(crate) struct Viability {
    steps: Vec<Step>,
    goals: Vec<Goal>,
    memo: RefCell<BTreeMap<Key, bool>>,
}

impl Viability {
    pub fn new(set: &TemplateSet) -> Self {
        let mut steps: Vec<Step> = set
            .chain
            .iter()
            .filter_map(|t| {
                let exit = t.exits.first()?;
                Some(Step {
                    entry: t.entry.level,
                    exit: exit.level,
                    delta: end_value(exit) - rel_value(&t.entry),
                    shape: shape_key(t.shape),
                    lands: t.entry.class == PortClass::Channel
                        && t.cells.iter().all(|c| c.y <= t.entry.cell.y),
                })
            })
            .collect();
        steps.sort_unstable();
        steps.dedup();
        let goals = set
            .finish
            .iter()
            .map(|t| Goal {
                entry: t.entry.level,
                class: t.entry.class,
                value: rel_value(&t.entry) - 4 * t.entry.cell.y,
            })
            .collect();
        Self {
            steps,
            goals,
            memo: RefCell::new(BTreeMap::new()),
        }
    }

    /// Whether a marble leaving a port at `value` and `level` with squared speed `v2`, after
    /// `flat` level pieces in a row, can reach a finish.
    pub fn viable(
        &self,
        value: i32,
        level: Option<PortLevel>,
        v2: i64,
        flat: u8,
        catch: bool,
    ) -> bool {
        let key = (
            value,
            v2 / BUCKET,
            level.map_or(-1, PortLevel::index),
            flat,
            catch,
        );
        if let Some(&r) = self.memo.borrow().get(&key) {
            return r;
        }
        // A revisit while this state is still being expanded is a cycle, not a way forward.
        self.memo.borrow_mut().insert(key, false);
        let r = self.expand(value, level, v2, flat, catch);
        self.memo.borrow_mut().insert(key, r);
        r
    }

    fn expand(&self, value: i32, level: Option<PortLevel>, v2: i64, flat: u8, catch: bool) -> bool {
        let falls = level == Some(PortLevel::L1) && v2 <= TRANSFER_MAX_V2 && !catch;
        for g in &self.goals {
            let direct = g.entry == level && g.value == value;
            let landed = falls
                && g.entry == Some(PortLevel::L3)
                && g.class == PortClass::Channel
                && g.value == value - 2;
            if direct || landed {
                return true;
            }
        }
        for s in &self.steps {
            let shape = SHAPES[usize::from(s.shape)];
            let direct = s.entry == level;
            let landed = falls && s.lands && s.entry == Some(PortLevel::L3);
            if !direct && !landed {
                continue;
            }
            let ev = if direct {
                value + s.delta
            } else {
                value - 2 + s.delta
            };
            let d = value - ev;
            if d < 0 || ev < FINISH_VALUE || !enters(shape, v2) || (d == 0 && flat >= MAX_FLAT_RUN)
            {
                continue;
            }
            let out = exit_v2(v2, shape, d);
            let next_flat = if d == 0 { flat + 1 } else { 0 };
            if out >= MIN_FLOW_V2 && self.viable(ev, s.exit, out, next_flat, shape == Shape::Catch)
            {
                return true;
            }
        }
        false
    }
}
