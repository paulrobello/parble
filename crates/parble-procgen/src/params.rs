//! Generator parameters and errors.

use parble_core::MarbleSize;

/// Which optional piece families the generator may draw from. The hopper, the finish and the
/// support pillar are always allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Families {
    /// Wooden block pieces (`blk_*`).
    pub blk: bool,
    /// Rail pieces (`rail_*`).
    pub rail: bool,
    /// Optional mechanisms (`mech_*` other than the hopper and the finish).
    pub mech: bool,
}

impl Families {
    pub const ALL: Self = Self {
        blk: true,
        rail: true,
        mech: true,
    };

    pub(crate) fn allows(self, family: &str) -> bool {
        match family {
            "blk" => self.blk,
            "rail" => self.rail,
            "mech" => self.mech,
            _ => false,
        }
    }
}

/// The generator sliders. A track is a pure function of `(catalog, seed, params)` and
/// [`crate::GENERATOR_VERSION`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GenParams {
    /// Half-extent of the plan area in cells: every piece lies in `-footprint..=footprint` on x
    /// and z around the hopper. `2..=64`.
    pub footprint: u32,
    /// Layer the hopper sits on. The track descends from it to the finish on layer 0. `1..=64`.
    pub height: u32,
    /// Target number of track pieces, hopper and finish included, supports not. The track ends
    /// at the first finish on layer 0 once it has three quarters of this many. `3..=1000`, and at
    /// most [`GenParams::max_length`] (12 a layer of `height`).
    pub length: u32,
    /// Chance, in per mille, that a step draws a special piece (gate, checkpoint, rope arch)
    /// instead of a plain one. `0..=1000`.
    pub special_per_mille: u32,
    /// Raises the weight of curves and steep slopes. `0..=100`.
    pub difficulty: u32,
    pub families: Families,
    /// Marbles in the hopper queue, all standard size. `1..=64`.
    pub marbles: u32,
}

impl Default for GenParams {
    fn default() -> Self {
        Self {
            footprint: 8,
            height: 6,
            length: 40,
            special_per_mille: 150,
            difficulty: 50,
            families: Families::ALL,
            marbles: 3,
        }
    }
}

impl GenParams {
    /// The marble size every generated hopper releases.
    pub const MARBLE_SIZE: MarbleSize = MarbleSize::Standard;

    /// Target pieces per layer of height a track can carry. Above it every walk runs out of
    /// height before it reaches the length, so [`crate::generate`] refuses at once instead of
    /// searching (2026-11-15 `long_track_census`, 5 seeds a case: 12 a layer passed 1 to 4 of 5,
    /// 14 none).
    pub const MAX_PIECES_PER_LAYER: u32 = 12;

    /// The longest target length the hopper's layer can carry.
    pub fn max_length(&self) -> u32 {
        Self::MAX_PIECES_PER_LAYER.saturating_mul(self.height)
    }

    pub(crate) fn check(&self) -> Result<(), GenError> {
        let range = |name: &'static str, v: u32, lo: u32, hi: u32| {
            if (lo..=hi).contains(&v) {
                Ok(())
            } else {
                Err(GenError::BadParams(name))
            }
        };
        range("footprint", self.footprint, 2, 64)?;
        range("height", self.height, 1, 64)?;
        range("length", self.length, 3, 1000)?;
        range("special_per_mille", self.special_per_mille, 0, 1000)?;
        range("difficulty", self.difficulty, 0, 100)?;
        range("marbles", self.marbles, 1, 64)?;
        if self.length > self.max_length() {
            return Err(GenError::TooLong {
                length: self.length,
                max_length: self.max_length(),
            });
        }
        Ok(())
    }
}

/// Why no track was generated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenError {
    /// The named parameter is out of range.
    BadParams(&'static str),
    /// The catalog has no hopper, finish or support pillar.
    MissingPiece(&'static str),
    /// The allowed families leave no piece the generator can chain.
    NoPieces,
    /// The search space was exhausted without reaching a finish.
    NoRoute,
    /// The target length needs more height than the hopper's layer gives: at most
    /// `max_length` pieces generate from it ([`GenParams::max_length`]).
    TooLong { length: u32, max_length: u32 },
    /// The node budget ran out first. A caller retries with the next sub-seed.
    Exhausted { nodes: u32 },
    /// The finished track failed validation. A generator bug, never shipped.
    Invalid { issues: usize, first: String },
}

impl core::fmt::Display for GenError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BadParams(p) => write!(f, "parameter {p} is out of range"),
            Self::MissingPiece(p) => write!(f, "the catalog has no {p}"),
            Self::NoPieces => f.write_str("the allowed families leave no chainable piece"),
            Self::NoRoute => f.write_str("no route from the hopper to a finish exists"),
            Self::TooLong { length, max_length } => write!(
                f,
                "a length of {length} needs a higher hopper: at most {max_length} pieces from this height"
            ),
            Self::Exhausted { nodes } => write!(f, "search budget of {nodes} nodes exhausted"),
            Self::Invalid { issues, first } => {
                write!(
                    f,
                    "the generated track has {issues} validation issues, first {first}"
                )
            }
        }
    }
}

impl std::error::Error for GenError {}
