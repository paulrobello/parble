//! The segment entry point: a walk from the hopper or from any open port, through a set of cells
//! already in use, to a finish or to an open end on a lower layer.
//!
//! [`crate::generate`] is one segment from the hopper to a finish. A long track is a chain of
//! segments ([`segment_plan`], [`generate_chain`]): each starts at the open end of the one before
//! and keeps out of its cells, so a segment that fails its physical run is regenerated with
//! another seed while the segments above it stay (`parble_sim::procgen_job::SegmentedJob`).
//! Fill-between (P6-T4) and infinite descent (P6-T5) start from the same call.
//!
//! A segment is a pure function of the catalog, its seed, the [`GenParams`], the start port, the
//! [`Space`] and the [`Goal`], and of [`crate::GENERATOR_VERSION`].

use std::collections::BTreeSet;

use parble_core::{
    CellCoord, HopperConfig, MapCatalog, PieceDef, Placement, PortClass, PortFace, PortFlow,
    PortLevel, Track, is_finish, is_hopper, validate_track,
};

use crate::params::{GenError, GenParams};
use crate::reach::Viability;
use crate::search::{self, Course, Ending, Walk};
use crate::{speed, support, template};

/// An open port a segment starts from or ends at: the piece cell it sits in, its face, and what
/// the generator knows of a marble leaving it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenPort {
    pub cell: CellCoord,
    pub face: PortFace,
    /// `None` for a drop port.
    pub level: Option<PortLevel>,
    pub class: PortClass,
    /// Estimated squared speed of a marble leaving the port, in 1e-3 m²/s²
    /// ([`OpenPort::v2_of_mm_s`]).
    pub v2: i64,
    /// Level pieces in a row before the port.
    pub flat: u8,
    /// Whether the port's piece is a catch: no transfer starts straight from one.
    pub after_catch: bool,
}

impl OpenPort {
    /// A side channel port a marble leaves at `mm_s` millimetres a second.
    pub fn channel(cell: CellCoord, face: PortFace, level: PortLevel, mm_s: u32) -> Self {
        Self {
            cell,
            face,
            level: Some(level),
            class: PortClass::Channel,
            v2: Self::v2_of_mm_s(mm_s),
            flat: 0,
            after_catch: false,
        }
    }

    /// The speed estimate of a marble at `mm_s` millimetres a second.
    pub fn v2_of_mm_s(mm_s: u32) -> i64 {
        i64::from(mm_s) * i64::from(mm_s) / 1000
    }

    /// The cell the port faces: the first cell of the segment that continues from it.
    pub fn target(&self) -> CellCoord {
        self.cell + self.face.normal()
    }
}

/// The cells a segment must keep out of, and adds its own to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Space {
    /// Cells a placement stands in.
    pub occupied: BTreeSet<CellCoord>,
    /// Cells that stay empty over an `L2` or `L3` channel ([`parble_core::clearance_cells`]).
    pub clear: BTreeSet<CellCoord>,
    /// Empty cells a transfer falls through.
    pub reserved: BTreeSet<CellCoord>,
    /// Cells under a transfer's source: no piece may take one, a support may.
    pub no_piece: BTreeSet<CellCoord>,
}

/// Where a segment ends.
///
/// A given target port (fill-between, P6-T4) is not a goal yet. The walk descends at a pace and
/// does not steer across the plan, so reaching one port needs its own router; it would be a third
/// variant here, an ending in `search::Ending` and a goal test in `reach::Viability`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Goal {
    /// A finish on layer 0.
    Finish,
    /// An open `L3` channel exit on `layer`, for the next segment. `layer` is also the lowest
    /// layer the segment uses, so the end is never below it.
    OpenEnd { layer: i32 },
}

/// A generated segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// The track pieces in placement order, supports not included ([`supports`]).
    pub pieces: Vec<Placement>,
    /// The open end, for [`Goal::OpenEnd`].
    pub end: Option<OpenPort>,
    /// The space the segment was generated in, with its own cells added.
    pub space: Space,
}

struct Kit {
    sorted: Vec<PieceDef>,
    hopper: usize,
}

impl Kit {
    fn new(defs: &[PieceDef]) -> Result<Self, GenError> {
        let mut sorted: Vec<PieceDef> = defs.to_vec();
        sorted.sort_by(|a, b| a.id.cmp(&b.id));
        sorted.dedup_by(|a, b| a.id == b.id);
        let hopper = sorted
            .iter()
            .position(|d| is_hopper(&d.id))
            .ok_or(GenError::MissingPiece("hopper"))?;
        if !sorted.iter().any(|d| is_finish(&d.id)) {
            return Err(GenError::MissingPiece("finish"));
        }
        if support::variants(&sorted).is_none() {
            return Err(GenError::MissingPiece("support pillar"));
        }
        Ok(Self { sorted, hopper })
    }
}

/// A segment from the hopper, placed on layer `params.height` over the plan centre, in an empty
/// space.
pub fn start_segment(
    defs: &[PieceDef],
    seed: u64,
    params: &GenParams,
    goal: Goal,
) -> Result<Segment, GenError> {
    walk(defs, seed, params, None, goal)
}

/// A segment from the open port `start` through `space` to `goal`.
///
/// `params.length` is the target number of pieces of this segment and `params.height` is not
/// read: the segment uses the layers from the start port's down to the goal's. The plan is
/// `params.footprint` cells either side of the origin, as for a whole track.
///
/// Errors: [`GenError::GoalAbove`] when the goal layer is above the start port,
/// [`GenError::NoSegmentRoute`] when no walk from the port can reach the goal (the port's cell
/// ahead is taken, or its level and speed lead nowhere), [`GenError::Exhausted`] when the node
/// budget ran out, which another seed may get past.
pub fn generate_segment(
    defs: &[PieceDef],
    seed: u64,
    params: &GenParams,
    start: &OpenPort,
    space: &Space,
    goal: Goal,
) -> Result<Segment, GenError> {
    walk(defs, seed, params, Some((start, space)), goal)
}

fn walk(
    defs: &[PieceDef],
    seed: u64,
    params: &GenParams,
    from: Option<(&OpenPort, &Space)>,
    goal: Goal,
) -> Result<Segment, GenError> {
    if from.is_none() {
        params.check()?;
    }
    let top = from.map_or(params.height as i32, |(p, _)| p.cell.y);
    let (floor, ending) = match goal {
        Goal::Finish => (0, Ending::Finish),
        Goal::OpenEnd { layer } => (layer, Ending::Open),
    };
    if floor > top {
        return Err(GenError::GoalAbove {
            start_layer: top,
            goal_layer: floor,
        });
    }
    // The flat-run limit and the longest length follow the layers this segment descends.
    let eff = GenParams {
        height: (top - floor).clamp(1, 64) as u32,
        ..*params
    };
    eff.check()?;
    let kit = Kit::new(defs)?;
    let hopper = &kit.sorted[kit.hopper];

    let set = template::build(
        &kit.sorted,
        params.families,
        params.special_per_mille > 0,
        1,
    );
    if set.chain.is_empty() {
        return Err(GenError::NoPieces);
    }
    let no_route = if from.is_some() {
        GenError::NoSegmentRoute
    } else {
        GenError::NoRoute
    };
    let start_level = match from {
        Some((p, _)) => p.level,
        None => hopper.ports.iter().find_map(|p| p.level),
    };
    if ending == Ending::Finish && !template::can_finish(&set, start_level) {
        return Err(no_route);
    }
    let via = Viability::new(
        &set,
        search::max_flat_run(&eff),
        floor,
        ending == Ending::Open,
    );
    let viable = match from {
        Some((p, _)) => {
            template::mate_class(p.class).is_some()
                && via.viable(
                    4 * p.cell.y + p.level.map_or(-3, PortLevel::index),
                    p.level,
                    p.v2,
                    p.flat.min(search::max_flat_run(&eff)),
                    p.after_catch,
                )
        }
        None => {
            let exit = hopper
                .ports
                .iter()
                .find(|p| matches!(p.flow, PortFlow::Out | PortFlow::Both))
                .ok_or(GenError::MissingPiece("hopper exit"))?;
            let exit_value = 4 * (top + exit.cell.y) + exit.level.map_or(-3, PortLevel::index);
            via.viable(exit_value, exit.level, speed::HOPPER_V2, 0, false)
        }
    };
    if !viable {
        return Err(no_route);
    }

    let course = Course { top, floor, ending };
    let attempts = search::NODE_BUDGET / search::ATTEMPT_NODES;
    // Whether an attempt stopped at its node cap: if none did, every walk was tried to its end
    // and no seed finds a route from this start.
    let mut capped = false;
    let found = (0..attempts).find_map(|a| {
        let s = search::Search::new(eff, &set, &via, seed, a, course);
        let r = match from {
            Some((p, space)) => s.with_space(space).run_from(p, search::ATTEMPT_NODES),
            None => s.run(hopper, search::ATTEMPT_NODES),
        };
        capped |= matches!(r, Err(GenError::Exhausted { .. }));
        r.ok()
    });
    let Some(w) = found else {
        return Err(if from.is_some() && !capped {
            GenError::NoSegmentRoute
        } else {
            GenError::Exhausted {
                nodes: search::NODE_BUDGET,
            }
        });
    };
    Ok(Segment {
        pieces: w.placements,
        end: w.end,
        space: Space {
            occupied: w.occupied,
            clear: w.clear,
            reserved: w.reserved,
            no_piece: w.nopiece,
        },
    })
}

/// Support pillars for `pieces`, which stand in `space`: a column under every elevated cell down
/// to layer 0 or the first occupied cell, left out where it would come down onto an `L2` or `L3`
/// channel. The pillars' own cells are not added to the space.
pub fn supports(
    defs: &[PieceDef],
    pieces: &[Placement],
    space: &Space,
) -> Result<Vec<Placement>, GenError> {
    let kit = Kit::new(defs)?;
    Ok(pillars(&kit, pieces, space))
}

fn pillars(kit: &Kit, pieces: &[Placement], space: &Space) -> Vec<Placement> {
    let catalog: MapCatalog = kit.sorted.iter().cloned().collect();
    let walk = Walk {
        placements: pieces.to_vec(),
        occupied: space.occupied.clone(),
        reserved: space.reserved.clone(),
        clear: space.clear.clone(),
        nopiece: space.no_piece.clone(),
        end: None,
    };
    let variants = support::variants(&kit.sorted).expect("Kit::new found a one-cell pillar");
    support::pillars(&walk, &catalog, &variants)
}

/// The track of a chain of segments: every segment's pieces in order, then the supports of the
/// whole chain, with the hopper queue of `params`. `segments` must be a chain: the first from
/// [`start_segment`], each next one generated from the end and in the space of the one before,
/// the last ending at a finish. `seed` becomes the track's seed, the root of its mechanisms'
/// random streams. The track must pass [`validate_track`] with no issues.
pub fn assemble(
    defs: &[PieceDef],
    seed: u64,
    params: &GenParams,
    segments: &[Segment],
) -> Result<Track, GenError> {
    let kit = Kit::new(defs)?;
    let pieces: Vec<Placement> = segments
        .iter()
        .flat_map(|s| s.pieces.iter().cloned())
        .collect();
    let empty = Space::default();
    let space = segments.last().map_or(&empty, |s| &s.space);
    let mut track = Track::new(seed);
    track.placements = pieces;
    let supports = pillars(&kit, &track.placements, space);
    track.placements.extend(supports);
    track.hopper = HopperConfig {
        placement: Some(0),
        queue: vec![GenParams::MARBLE_SIZE; params.marbles as usize],
        ..HopperConfig::default()
    };

    let catalog: MapCatalog = kit.sorted.iter().cloned().collect();
    let issues = validate_track(&track, &catalog);
    if issues.is_empty() {
        Ok(track)
    } else {
        Err(GenError::Invalid {
            issues: issues.len(),
            first: format!("{:?}", issues[0]),
        })
    }
}

/// Target pieces of one segment of a chain. Short enough that a rejected segment costs little of
/// the track, long enough that the level run a segment makes on its floor layer stays a small
/// share of it.
pub const SEGMENT_LENGTH: u32 = 64;

/// One segment of a chain: its share of the track's length and where it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentSpec {
    /// The segment's share of the target length. Its own target follows what the segments
    /// above it delivered ([`chain_segment`]).
    pub length: u32,
    pub goal: Goal,
}

/// How a track of `params` splits into segments: about [`SEGMENT_LENGTH`] pieces each, the
/// layers shared out evenly and the length in step with them, every segment but the last ending
/// at an open end and the last at the finish. Fewer segments when the height has fewer layers than that, or when a segment
/// would be longer than its layers carry ([`GenParams::max_length`]). A track up to
/// [`SEGMENT_LENGTH`] pieces is one segment.
pub fn segment_plan(params: &GenParams) -> Vec<SegmentSpec> {
    let height = params.height.max(1);
    let mut n = params.length.div_ceil(SEGMENT_LENGTH).clamp(1, height);
    loop {
        let floor = |i: u32| (height * (n - 1 - i) / n) as i32;
        // Pieces down to the floor of segment `i`: the length in step with the layers used.
        let upto = |i: u32| params.length * (height - floor(i) as u32) / height;
        let specs: Vec<SegmentSpec> = (0..n)
            .map(|i| SegmentSpec {
                length: upto(i) - if i == 0 { 0 } else { upto(i - 1) },
                goal: if i + 1 == n {
                    Goal::Finish
                } else {
                    Goal::OpenEnd { layer: floor(i) }
                },
            })
            .collect();
        let fits = (0..n).all(|i| {
            let top = if i == 0 { height as i32 } else { floor(i - 1) };
            let drop = (top - floor(i)).max(1) as u32;
            specs[i as usize].length <= GenParams::MAX_PIECES_PER_LAYER * drop
                && specs[i as usize].length >= 3
        });
        if fits || n == 1 {
            return specs;
        }
        n -= 1;
    }
}

/// The next segment of a chain of `plan` ([`segment_plan`]`(params)`): segment `chain.len()`,
/// from the hopper when `chain` is empty, else from the end of its last segment.
///
/// A segment that comes down to its goal layer late is longer than its target. The target of
/// each segment is therefore an even share of the pieces the track still needs, at most what
/// the segment's layers carry ([`GenParams::MAX_PIECES_PER_LAYER`]).
pub fn chain_segment(
    defs: &[PieceDef],
    seed: u64,
    params: &GenParams,
    plan: &[SegmentSpec],
    chain: &[Segment],
) -> Result<Segment, GenError> {
    params.check()?;
    let spec = plan
        .get(chain.len())
        .ok_or(GenError::BadParams("segment"))?;
    let placed: usize = chain.iter().map(|s| s.pieces.len()).sum();
    let left = (plan.len() - chain.len()) as u32;
    let top = match chain.last() {
        None => params.height as i32,
        Some(prev) => prev.end.as_ref().ok_or(GenError::NoSegmentRoute)?.cell.y,
    };
    let floor = match spec.goal {
        Goal::Finish => 0,
        Goal::OpenEnd { layer } => layer,
    };
    let carries = GenParams::MAX_PIECES_PER_LAYER * (top - floor).clamp(1, 64) as u32;
    let p = GenParams {
        length: (params.length.saturating_sub(placed as u32) / left).clamp(3, carries.max(3)),
        ..*params
    };
    match chain.last() {
        None => start_segment(defs, seed, &p, spec.goal),
        Some(prev) => {
            let start = prev.end.as_ref().ok_or(GenError::NoSegmentRoute)?;
            generate_segment(defs, seed, &p, start, &prev.space, spec.goal)
        }
    }
}

/// The track of [`segment_plan`]`(params)` generated with one seed a segment. `track_seed` is the
/// track's own seed. This is how a segmented track regenerates from the seeds its file records
/// (`metadata.generator.segments`).
pub fn generate_chain(
    defs: &[PieceDef],
    track_seed: u64,
    params: &GenParams,
    seeds: &[u64],
) -> Result<Track, GenError> {
    params.check()?;
    let plan = segment_plan(params);
    if seeds.len() != plan.len() {
        return Err(GenError::BadParams("segment seeds"));
    }
    let mut segments: Vec<Segment> = Vec::with_capacity(plan.len());
    for &seed in seeds {
        let s = chain_segment(defs, seed, params, &plan, &segments)?;
        segments.push(s);
    }
    assemble(defs, track_seed, params, &segments)
}
