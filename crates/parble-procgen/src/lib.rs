//! parble-procgen: seeded procedural track generation (P6-T1).
//!
//! [`generate`] grows a track from a hopper on a high layer down to a finish on layer 0. The
//! result is a pure function of the catalog definitions, the seed, the [`GenParams`] and
//! [`GENERATOR_VERSION`]: the definitions are sorted by id inside, every collection is ordered,
//! randomness comes from `parble_core::SeededRng` and the search budget counts nodes, never time.
//!
//! The walker chains pieces by mating ports and falls through the controlled-drop transfer
//! (an `L1` exit onto an `L3` inlet one layer below) the validator accepts. Support pillars go
//! under every elevated cell afterwards. The finished track must pass
//! [`parble_core::validate_track`] with no issues. Static validity is not a physical run: the
//! headless sim check that accepts or retries a candidate is P6-T2.
//!
//! Not generated yet: lifts (`mech_stepper_*`), joins, funnels, one-way holes and socket/spigot
//! pieces, because each needs a second feed, an unmated inlet or a coupled partner.

mod params;
mod reach;
mod search;
mod speed;
mod support;
mod template;

use parble_core::{
    HopperConfig, MapCatalog, PieceDef, PortFlow, PortLevel, Track, is_finish, is_hopper,
    validate_track,
};

pub use params::{Families, GenError, GenParams};
pub use search::NODE_BUDGET;

/// Bump whenever a change alters the track generated for the same catalog, seed and params.
pub const GENERATOR_VERSION: u32 = 3;

/// Generate a track. `defs` is the catalog the track is built from, in any order.
pub fn generate(defs: &[PieceDef], seed: u64, params: &GenParams) -> Result<Track, GenError> {
    params.check()?;
    let mut sorted: Vec<PieceDef> = defs.to_vec();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    sorted.dedup_by(|a, b| a.id == b.id);

    let hopper = sorted
        .iter()
        .find(|d| is_hopper(&d.id))
        .ok_or(GenError::MissingPiece("hopper"))?;
    if !sorted.iter().any(|d| is_finish(&d.id)) {
        return Err(GenError::MissingPiece("finish"));
    }
    let pillar = sorted
        .iter()
        .find(|d| d.structural)
        .ok_or(GenError::MissingPiece("support pillar"))?;

    let set = template::build(&sorted, params.families, 1);
    if set.chain.is_empty() {
        return Err(GenError::NoPieces);
    }

    let start = hopper.ports.iter().find_map(|p| p.level);
    if !template::can_finish(&set, start) {
        return Err(GenError::NoRoute);
    }
    let via = reach::Viability::new(&set);
    let exit = hopper
        .ports
        .iter()
        .find(|p| matches!(p.flow, PortFlow::Out | PortFlow::Both))
        .ok_or(GenError::MissingPiece("hopper exit"))?;
    let exit_value =
        4 * (params.height as i32 + exit.cell.y) + exit.level.map_or(-3, PortLevel::index);
    if !via.viable(exit_value, exit.level, speed::HOPPER_V2, 0) {
        return Err(GenError::NoRoute);
    }
    let attempts = search::NODE_BUDGET / search::ATTEMPT_NODES;
    let walk = (0..attempts)
        .find_map(|a| {
            search::Search::new(*params, &set, &via, seed, a)
                .run(hopper, search::ATTEMPT_NODES)
                .ok()
        })
        .ok_or(GenError::Exhausted {
            nodes: search::NODE_BUDGET,
        })?;
    let catalog: MapCatalog = sorted.iter().cloned().collect();
    let mut track = Track::new(seed);
    track.placements = walk.placements.clone();
    track
        .placements
        .extend(support::pillars(&walk, &catalog, pillar));
    track.hopper = HopperConfig {
        placement: Some(0),
        queue: vec![GenParams::MARBLE_SIZE; params.marbles as usize],
        ..HopperConfig::default()
    };

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
