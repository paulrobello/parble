//! Shared light colours (PRD §5.5, decided 2026-11-08): every scene mixes a warm amber key with a
//! pale blue fill or rim, as the references do. Lights, lantern and lamp emissives and glow inlays
//! take their colour from here.
//!
//! The blue is kept desaturated: ACES fitted ([`crate::post::TONEMAPPING`]) pushes a saturated
//! blue towards purple, and a strong blue would grey the glass marbles.

use bevy::prelude::*;

/// Warm amber: paper lanterns, the lamp, and the key light's tint.
pub const WARM_AMBER: Color = Color::srgb(1.0, 0.78, 0.52);
/// A lighter warm yellow for broad key lights, so lit wood stays wood-coloured.
pub const WARM_KEY: Color = Color::srgb(1.0, 0.86, 0.68);
/// Pale blue: moonlight, window fill and rim light.
pub const PALE_BLUE: Color = Color::srgb(0.62, 0.74, 1.0);
/// Deep night blue for ambient fill and the space behind the scene.
pub const NIGHT_BLUE: Color = Color::srgb(0.35, 0.45, 1.0);

/// A glow or light preset in the shared palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Glow {
    #[default]
    WarmAmber,
    PaleBlue,
}

impl Glow {
    pub fn color(self) -> Color {
        match self {
            Self::WarmAmber => WARM_AMBER,
            Self::PaleBlue => PALE_BLUE,
        }
    }

    /// Emissive radiance at `nits` cd/m² per unit of colour.
    pub fn emissive(self, nits: f32) -> LinearRgba {
        LinearRgba::from(self.color()) * nits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warm_is_warm_and_blue_is_blue() {
        let w = WARM_AMBER.to_srgba();
        assert!(w.red > w.green && w.green > w.blue);
        let k = WARM_KEY.to_srgba();
        assert!(k.red > k.green && k.green > k.blue);
        let b = PALE_BLUE.to_srgba();
        assert!(b.blue > b.green && b.green > b.red);
        // Pale, not saturated: the red channel stays above half of the blue.
        assert!(b.red > 0.5 * b.blue);
        assert_eq!(
            Glow::PaleBlue.emissive(2.0),
            LinearRgba::from(PALE_BLUE) * 2.0
        );
    }
}
