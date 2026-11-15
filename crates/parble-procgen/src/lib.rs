//! parble-procgen: seeded procedural track generation (P6-T1).
//!
//! [`generate`] grows a track from a hopper on a high layer down to a finish on layer 0. The
//! result is a pure function of the catalog definitions, the seed, the [`GenParams`] and
//! [`GENERATOR_VERSION`]: the definitions are sorted by id inside, every collection is ordered,
//! randomness comes from `parble_core::SeededRng` and the search budget counts nodes, never time.
//!
//! The walker chains pieces by mating ports and falls through the controlled-drop transfer
//! (an `L1` exit onto an `L3` inlet one layer below) the validator accepts. Support pillars go
//! under every elevated cell afterwards, except where the column would come down onto an `L2` or
//! `L3` channel, each column stacked from the tallest pillar variant that fits. The finished track must pass
//! [`parble_core::validate_track`] with no issues. Static validity is not a physical run: the
//! headless sim check that accepts or retries a candidate is `parble_sim::procgen_job` (P6-T2).
//!
//! [`generate`] is one segment. The segment entry point ([`generate_segment`], [`start_segment`])
//! walks from any open port through cells already in use to a finish or to an open end on a lower
//! layer, and [`generate_chain`] builds a long track from several ([`segment`]).
//!
//! Not generated yet: lifts (`mech_stepper_*`), joins, funnels, one-way holes and socket/spigot
//! pieces, because each needs a second feed, an unmated inlet or a coupled partner.

mod params;
mod reach;
mod search;
pub mod segment;
mod speed;
mod support;
mod template;

use parble_core::file::Generator;
use parble_core::{ParbleFile, PieceDef, Track};

pub use params::{Families, GenError, GenParams};
pub use search::NODE_BUDGET;
pub use segment::{
    Goal, OpenPort, SEGMENT_LENGTH, Segment, SegmentSpec, Space, assemble, chain_segment,
    generate_chain, generate_segment, segment_plan, start_segment, supports,
};

/// Bump whenever a change alters the track generated for the same catalog, seed and params.
pub const GENERATOR_VERSION: u32 = 9;

/// Generate a track: one segment from the hopper to a finish on layer 0. `defs` is the catalog
/// the track is built from, in any order.
pub fn generate(defs: &[PieceDef], seed: u64, params: &GenParams) -> Result<Track, GenError> {
    let segment = start_segment(defs, seed, params, Goal::Finish)?;
    assemble(defs, seed, params, core::slice::from_ref(&segment))
}

/// The `.parble` file of a generated track, named `name`: its metadata records
/// [`GENERATOR_VERSION`] and, for a track of several segments ([`generate_chain`]), the seed of
/// each. A whole track ([`generate`]) passes no segment seeds: its own seed regenerates it.
pub fn generated_file(track: Track, name: &str, segment_seeds: &[u64]) -> ParbleFile {
    let mut file = ParbleFile::new(track, name);
    file.metadata.generator = Some(Generator {
        version: GENERATOR_VERSION,
        segments: segment_seeds.to_vec(),
    });
    file
}
