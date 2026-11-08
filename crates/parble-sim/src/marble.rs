//! Marble types (P1-T6): the physical table for the three 16 mm marbles, plus the P0 legacy entry
//! the golden ramp run was recorded with.
//!
//! Every value below comes from a published source; `docs/marbles.md` holds the full table with
//! links and the derivations. Friction and restitution are **marble-on-wood pair** coefficients
//! (the kit is wood), applied through Rapier combine rules so the pair value wins against a wood
//! collider:
//!
//! - friction uses [`CoefficientCombineRule::Min`] (it outranks the wood collider's `Average`), so
//!   the contact uses the marble's value whenever it is at or below the wood's 0.5;
//! - restitution uses [`CoefficientCombineRule::Max`], so the contact uses the marble's value
//!   whenever it is at or above the wood's 0.4.
//!
//! Rolling resistance: Rapier has no rolling-resistance model, so the coefficient `c_rr` is applied
//! as angular damping matched on energy loss at a reference speed. A ball rolling without slip at
//! speed `v` loses `c_rr·m·g·v` W to rolling resistance and `k·I·ω²` W to angular damping `k`; with
//! `I = 2/5·m·r²` and `ω = v/r` the two agree when `k = 2.5·c_rr·g / v`. [`ROLLING_REF_SPEED`] is
//! the typical channel speed (0.5 m/s), so the damping is exact there and approximate elsewhere.

use rapier3d::prelude::*;

use crate::scene;

/// Gravity magnitude the sim runs at, m/s².
pub const G: f32 = 9.81;
/// Speed at which the rolling-resistance damping matches `c_rr` exactly, m/s.
pub const ROLLING_REF_SPEED: f32 = 0.5;

/// The selectable marble types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MarbleKind {
    /// Clear soda-lime glass with an inner vane.
    #[default]
    CatsEye,
    /// Solid glossy marble, modelled as glazed ceramic (porcelain body, glass glaze).
    Glazed,
    /// Chrome steel (AISI 52100 bearing ball).
    Steel,
}

impl MarbleKind {
    pub const ALL: [Self; 3] = [Self::CatsEye, Self::Glazed, Self::Steel];

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "cats-eye" => Some(Self::CatsEye),
            "glazed" => Some(Self::Glazed),
            "steel" => Some(Self::Steel),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::CatsEye => "cats-eye",
            Self::Glazed => "glazed",
            Self::Steel => "steel",
        }
    }

    pub fn spec(self) -> MarbleSpec {
        match self {
            Self::CatsEye => CATS_EYE,
            Self::Glazed => GLAZED,
            Self::Steel => STEEL,
        }
    }
}

/// Physical parameters of one marble type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarbleSpec {
    /// Radius, metres.
    pub radius_m: f32,
    /// Bulk density, kg/m³. Mass follows from the radius ([`MarbleSpec::mass_kg`]).
    pub density: f32,
    /// Sliding friction coefficient against wood.
    pub friction: f32,
    /// Coefficient of restitution against wood.
    pub restitution: f32,
    /// Rolling resistance coefficient (dimensionless, force / normal load).
    pub rolling_resistance: f32,
    /// Whether the pair-coefficient combine rules apply. Only the P0 legacy entry is `false`: it
    /// keeps Rapier's default `Average` rules so the golden run stays byte-identical.
    pub pair_rules: bool,
}

/// 16 mm diameter, the contract's standard marble.
const R16: f32 = 0.008;

/// Cat's eye: soda-lime glass, 2,520 kg/m³ (Wikipedia, "Soda–lime glass", density 2.52 g/cm³;
/// Engineering ToolBox "Densities of Solids", common glass 2.4–2.8). Friction 0.3: glass on wood is
/// not tabulated, so the nearest hard mineral pair is used (Engineering ToolBox "Friction and
/// Friction Coefficients", wood on stone 0.2–0.4, midpoint). Restitution 0.66 (Physics Factbook,
/// "Coefficients of Restitution", glass marble 0.658, dropped from 92 cm on a hard floor). Rolling
/// resistance 0.002 (Engineering ToolBox "Rolling Resistance", hard ball 0.001–0.002, upper end).
pub const CATS_EYE: MarbleSpec = MarbleSpec {
    radius_m: R16,
    density: 2_520.0,
    friction: 0.3,
    restitution: 0.66,
    rolling_resistance: 0.002,
    pair_rules: true,
};

/// Solid glossy, recorded as glazed ceramic: porcelain, 2,400 kg/m³ (Engineering ToolBox
/// "Densities of Solids", porcelain 2.3–2.5, midpoint). The glaze is a glass layer, so friction and
/// restitution take the glass values (same sources as [`CATS_EYE`]).
pub const GLAZED: MarbleSpec = MarbleSpec {
    radius_m: R16,
    density: 2_400.0,
    friction: 0.3,
    restitution: 0.66,
    rolling_resistance: 0.002,
    pair_rules: true,
};

/// Steel: AISI 52100 chrome bearing steel, 7,810 kg/m³ (AZoM, "AISI 52100 Alloy Steel", 7.81
/// g/cm³). Friction 0.4 (Engineering ToolBox, wood on clean metal 0.2–0.6, midpoint). Restitution
/// 0.60 (Physics Factbook, steel ball bearing 0.597). Rolling resistance 0.002 (Engineering
/// ToolBox, ball bearings 0.001–0.002).
pub const STEEL: MarbleSpec = MarbleSpec {
    radius_m: R16,
    density: 7_810.0,
    friction: 0.4,
    restitution: 0.60,
    rolling_resistance: 0.002,
    pair_rules: true,
};

/// The P0 marble the golden ramp hash `0c9206fc016d9b17` was recorded with (P0-T3 placeholder
/// values, not measured). Kept unchanged so the golden stays byte-identical; renders as a cat's eye.
pub const P0_LEGACY: MarbleSpec = MarbleSpec {
    radius_m: R16,
    density: scene::GLASS_DENSITY,
    friction: scene::GLASS_FRICTION,
    restitution: scene::GLASS_RESTITUTION,
    rolling_resistance: 0.0,
    pair_rules: false,
};

impl MarbleSpec {
    /// Volume, m³.
    pub fn volume_m3(&self) -> f32 {
        4.0 / 3.0 * core::f32::consts::PI * self.radius_m * self.radius_m * self.radius_m
    }

    /// Mass, kg.
    pub fn mass_kg(&self) -> f32 {
        self.density * self.volume_m3()
    }

    /// Angular damping that stands in for rolling resistance (see the module docs).
    pub fn angular_damping(&self) -> f32 {
        2.5 * self.rolling_resistance * G / ROLLING_REF_SPEED
    }

    /// A marble body (CCD on, never sleeps) at `at` and its collider.
    pub fn build(&self, at: Vector) -> (RigidBody, Collider) {
        let mut body = RigidBodyBuilder::dynamic()
            .translation(at)
            .ccd_enabled(true)
            .can_sleep(false);
        if self.rolling_resistance > 0.0 {
            body = body.angular_damping(self.angular_damping());
        }
        let mut collider = ColliderBuilder::ball(self.radius_m)
            .density(self.density)
            .friction(self.friction)
            .restitution(self.restitution);
        if self.pair_rules {
            collider = collider
                .friction_combine_rule(CoefficientCombineRule::Min)
                .restitution_combine_rule(CoefficientCombineRule::Max);
        }
        (body.build(), collider.build())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for k in MarbleKind::ALL {
            assert_eq!(MarbleKind::parse(k.name()), Some(k));
        }
        assert_eq!(MarbleKind::parse("opal"), None);
    }

    #[test]
    fn masses_match_the_published_densities() {
        // 16 mm sphere: 2.1447 cm³.
        let g = |k: MarbleKind| k.spec().mass_kg() * 1_000.0;
        assert!((g(MarbleKind::CatsEye) - 5.40).abs() < 0.01);
        assert!((g(MarbleKind::Glazed) - 5.15).abs() < 0.01);
        assert!((g(MarbleKind::Steel) - 16.75).abs() < 0.01);
    }

    #[test]
    fn pair_values_win_against_the_wood_collider() {
        for k in MarbleKind::ALL {
            let s = k.spec();
            assert!(s.friction <= scene::WOOD_FRICTION, "{k:?} friction");
            assert!(
                s.restitution >= scene::WOOD_RESTITUTION,
                "{k:?} restitution"
            );
        }
    }

    #[test]
    fn every_type_rolls_down_the_ramp_and_stays_in_the_tray() {
        let mut hashes = Vec::new();
        for k in MarbleKind::ALL {
            let mut sim = crate::Sim::ramp_scene_with(k.spec());
            sim.tick(crate::golden::RAMP_10S_TICKS);
            let m = sim.marbles()[0];
            assert!(
                m.position[0] > scene::ramp_run_m(),
                "{k:?} left the ramp: {m:?}"
            );
            assert!(
                m.position[0] < scene::tray_length_m(),
                "{k:?} escaped: {m:?}"
            );
            assert!(
                m.position[1] > 0.0 && m.position[1] < 0.02,
                "{k:?} height: {m:?}"
            );
            hashes.push(sim.trajectory_hash());
        }
        hashes.push(crate::golden::ramp_10s_hash());
        hashes.sort_unstable();
        hashes.dedup();
        assert_eq!(hashes.len(), 4, "each type must run its own trajectory");
    }

    #[test]
    fn legacy_entry_is_the_p0_marble() {
        let (_, c) = P0_LEGACY.build(Vector::ZERO);
        assert_eq!(c.friction(), 0.4);
        assert_eq!(c.restitution(), 0.6);
        assert_eq!(P0_LEGACY.radius_m, scene::standard_marble_radius_m());
    }
}
