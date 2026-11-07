//! Stable trajectory hashing: 64-bit FNV-1a over quantized marble state.
//!
//! `std`'s `DefaultHasher` is unspecified across Rust releases, so the hash is implemented
//! here and fixed by the golden test.

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Positions are quantized to 1 µm (the parble-core length unit).
pub const POSITION_QUANTA_PER_M: f64 = 1_000_000.0;
/// Linear velocities are quantized to 1 µm/s.
pub const VELOCITY_QUANTA_PER_M_PER_S: f64 = 1_000_000.0;

/// 64-bit FNV-1a.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fnv1a64(u64);

impl Default for Fnv1a64 {
    fn default() -> Self {
        Self(FNV_OFFSET_BASIS)
    }
}

impl Fnv1a64 {
    pub fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(FNV_PRIME);
        }
    }

    pub fn write_u64(&mut self, v: u64) {
        self.write(&v.to_le_bytes());
    }

    pub fn write_i64(&mut self, v: i64) {
        self.write(&v.to_le_bytes());
    }

    pub fn finish(&self) -> u64 {
        self.0
    }
}

/// Round `value * quanta_per_unit` half away from zero to an integer (saturating).
pub fn quantize(value: f32, quanta_per_unit: f64) -> i64 {
    libm::round(f64::from(value) * quanta_per_unit) as i64
}

/// A hash as exactly 16 lowercase hex digits (the golden file format, minus its newline).
pub fn format_hash(hash: u64) -> String {
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_reference_vectors() {
        let h = Fnv1a64::default();
        assert_eq!(h.finish(), 0xcbf2_9ce4_8422_2325);
        let mut h = Fnv1a64::default();
        h.write(b"a");
        assert_eq!(h.finish(), 0xaf63_dc4c_8601_ec8c);
        let mut h = Fnv1a64::default();
        h.write(b"foobar");
        assert_eq!(h.finish(), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn quantize_rounds_half_away_from_zero() {
        assert_eq!(quantize(1.25, 2.0), 3);
        assert_eq!(quantize(-1.25, 2.0), -3);
        assert_eq!(quantize(0.06, POSITION_QUANTA_PER_M), 60_000);
    }

    #[test]
    fn format_is_sixteen_lowercase_hex_digits() {
        assert_eq!(format_hash(0xab), "00000000000000ab");
        assert_eq!(format_hash(u64::MAX), "ffffffffffffffff");
    }
}
