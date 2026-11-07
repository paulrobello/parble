//! Seeded RNG with named sub-streams over `rand_chacha`.
//!
//! A sub-stream depends only on `(root seed, name)`, never on other streams or on draw
//! order, so a mechanism's randomness stays stable when the rest of a track is edited.
//! The ChaCha key comes from the root seed (splitmix64) and the ChaCha stream id from the
//! name (FNV-1a 64). Changing either derivation changes every seeded run.

use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{Rng, SeedableRng};

/// Root of all seeded randomness for a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SeededRng {
    seed: u64,
}

/// One named, independent random stream.
#[derive(Debug, Clone)]
pub struct SubStream(ChaCha8Rng);

const fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    hash
}

impl SeededRng {
    pub const fn new(seed: u64) -> Self {
        Self { seed }
    }

    pub const fn seed(&self) -> u64 {
        self.seed
    }

    /// The sub-stream for `name`.
    pub fn stream(&self, name: &str) -> SubStream {
        let mut state = self.seed;
        let mut key = [0u8; 32];
        for chunk in key.as_chunks_mut::<8>().0 {
            *chunk = splitmix64(&mut state).to_le_bytes();
        }
        let mut rng = ChaCha8Rng::from_seed(key);
        rng.set_stream(fnv1a64(name.as_bytes()));
        SubStream(rng)
    }
}

impl SubStream {
    pub fn next_u32(&mut self) -> u32 {
        self.0.next_u32()
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }

    /// Uniform integer in `0..n`, unbiased (rejection sampling). Panics if `n == 0`.
    pub fn below(&mut self, n: u32) -> u32 {
        assert!(n > 0, "SubStream::below(0)");
        // Largest multiple of n that fits in u32's range; reject draws at or above it.
        let zone = u32::MAX - (u32::MAX - n + 1) % n;
        loop {
            let v = self.next_u32();
            if v <= zone {
                return v % n;
            }
        }
    }
}
