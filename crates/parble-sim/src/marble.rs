//! Marble types (P1-T6, P5-T1): the physical table per type, plus the P0 legacy entry the golden
//! ramp run was recorded with. Size is a separate dimension ([`MarbleSize`]): every type comes in
//! mini 10 mm, standard 16 mm and shooter 25 mm, and mass follows from density × volume.
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
//! Rolling resistance: Rapier has no rolling-resistance model, so [`apply_rolling_resistance`]
//! applies one before each step. Each contact with a surface that is not another marble pushes on
//! the ball with a normal impulse `J` over the step, and resists its rolling with an angular impulse
//! `c_rr·r·J` against its spin; a ball whose spin that impulse would reverse is stopped instead
//! (Coulomb rolling resistance). A ball rolling on the level therefore decelerates at a constant
//! `5/7·c_rr·g` and comes to rest. Until 2026-11-08 it was angular damping matched on energy loss
//! at 0.5 m/s, which faded with speed: marble trains at the playroom finish crept back up the far
//! leg at 4–9 mm/s.

use parble_core::contract;
use rapier3d::prelude::*;

use crate::scene;

/// Gravity magnitude the sim runs at, m/s².
pub const G: f32 = 9.81;

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
    /// Clear soda-lime glass with twisted coloured ribbons inside.
    Swirl,
    /// Soda-lime glass with a layered translucent colour skin over the core.
    Onionskin,
    /// Milky blue opalescent glass (soda-lime glass with an opacifier).
    Opal,
    /// Clear soda-lime glass around an emissive (glow-in-the-dark) core.
    Glow,
}

impl MarbleKind {
    pub const ALL: [Self; 7] = [
        Self::CatsEye,
        Self::Glazed,
        Self::Steel,
        Self::Swirl,
        Self::Onionskin,
        Self::Opal,
        Self::Glow,
    ];

    /// The `--marble` values, for error messages.
    pub const NAMES: &'static str = "cats-eye|glazed|steel|swirl|onionskin|opal|glow";

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == s)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::CatsEye => "cats-eye",
            Self::Glazed => "glazed",
            Self::Steel => "steel",
            Self::Swirl => "swirl",
            Self::Onionskin => "onionskin",
            Self::Opal => "opal",
            Self::Glow => "glow",
        }
    }

    /// The standard 16 mm marble of this type.
    pub fn spec(self) -> MarbleSpec {
        match self {
            Self::CatsEye => CATS_EYE,
            Self::Glazed => GLAZED,
            Self::Steel => STEEL,
            Self::Swirl => SWIRL,
            Self::Onionskin => ONIONSKIN,
            Self::Opal => OPAL,
            Self::Glow => GLOW,
        }
    }

    /// This type at `size`: the same material, so only the radius (and with it the mass) changes.
    pub fn spec_sized(self, size: MarbleSize) -> MarbleSpec {
        MarbleSpec {
            radius_m: size.radius_m(),
            ..self.spec()
        }
    }
}

/// Marble size (P5-T1), from the kit contract's three diameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MarbleSize {
    /// 10 mm.
    Mini,
    /// 16 mm, the contract's standard marble.
    #[default]
    Standard,
    /// 25 mm, the largest marble the kit passes.
    Shooter,
}

impl MarbleSize {
    pub const ALL: [Self; 3] = [Self::Mini, Self::Standard, Self::Shooter];

    /// The `--marble-size` values, for error messages.
    pub const NAMES: &'static str = "mini|standard|shooter";

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == s)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Mini => "mini",
            Self::Standard => "standard",
            Self::Shooter => "shooter",
        }
    }

    /// Radius, metres.
    pub fn radius_m(self) -> f32 {
        let d = match self {
            Self::Mini => contract::MARBLE_MINI_DIAMETER_UM,
            Self::Standard => contract::MARBLE_STANDARD_DIAMETER_UM,
            Self::Shooter => contract::MARBLE_SHOOTER_DIAMETER_UM,
        };
        scene::um_to_m(d / 2)
    }
}

/// The physics a run uses for an optional `--marble` and a `--marble-size`. No type at the standard
/// size is the P0 legacy marble (the golden run); a size alone means a cat's eye at that size.
pub fn spec_for(kind: Option<MarbleKind>, size: MarbleSize) -> MarbleSpec {
    match (kind, size) {
        (None, MarbleSize::Standard) => P0_LEGACY,
        (k, s) => k.unwrap_or_default().spec_sized(s),
    }
}

/// The lineup scenario's marbles in spawn order (P5-T1): every type at the standard size, with a
/// mini and a shooter swirl at the two ends of the row for scale.
pub const LINEUP: [(MarbleKind, MarbleSize); 9] = [
    (MarbleKind::Swirl, MarbleSize::Mini),
    (MarbleKind::CatsEye, MarbleSize::Standard),
    (MarbleKind::Glazed, MarbleSize::Standard),
    (MarbleKind::Steel, MarbleSize::Standard),
    (MarbleKind::Swirl, MarbleSize::Standard),
    (MarbleKind::Onionskin, MarbleSize::Standard),
    (MarbleKind::Opal, MarbleSize::Standard),
    (MarbleKind::Glow, MarbleSize::Standard),
    (MarbleKind::Swirl, MarbleSize::Shooter),
];

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

/// Swirl: clear soda-lime glass with coloured glass ribbons twisted through it. The ribbons are
/// glass too, so every value is [`CATS_EYE`]'s (same sources).
pub const SWIRL: MarbleSpec = CATS_EYE;

/// Onionskin: soda-lime glass with a thin layer of coloured glass over the core. All glass, so
/// every value is [`CATS_EYE`]'s (same sources).
pub const ONIONSKIN: MarbleSpec = CATS_EYE;

/// Opal: opal (milk) glass, glass made translucent and milky by an opacifier (Wikipedia, "Milk
/// glass"). No source gives it a separate density, so it takes soda-lime's 2,520 kg/m³ (a proxy);
/// contact values are glass's (same sources as [`CATS_EYE`]).
pub const OPAL: MarbleSpec = CATS_EYE;

/// Glow: clear soda-lime glass around a phosphorescent core. The pigment is a small fraction of the
/// volume, so the body is taken as all glass: every value is [`CATS_EYE`]'s (same sources).
pub const GLOW: MarbleSpec = CATS_EYE;

/// The P0 marble the golden ramp hash `2813fade06774d5e` was recorded with (P0-T3 placeholder
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

    /// A marble body (CCD on, never sleeps) at `at` and its collider. The body's `user_data`
    /// carries the rolling-resistance coefficient's bits ([`apply_rolling_resistance`]).
    pub fn build(&self, at: Vector) -> (RigidBody, Collider) {
        let body = RigidBodyBuilder::dynamic()
            .translation(at)
            .ccd_enabled(true)
            .can_sleep(false)
            .user_data(u128::from(self.rolling_resistance.to_bits()));
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

/// Rolling resistance for every marble in `marbles` (see the module docs), from the contact
/// impulses of the last step. Call once before each world step.
pub fn apply_rolling_resistance(world: &mut PhysicsWorld, marbles: &[RigidBodyHandle]) {
    rolling_resistance(world, marbles, solved_impulse);
}

/// The normal impulse the solver applied in the last step on `m`'s solver contacts.
///
/// A manifold point's impulse is written back only while a solver contact refers to it. Where a
/// contact hook clears a manifold's solver contacts (every copy but the first of an exact bend,
/// slope, junction or arc contact, see [`crate::chunk::ChunkColliders`]) or truncates them, the
/// points keep the impulse of the last step they were solved, so summing every point
/// counted the exact contact's load again on the manifold it had moved from (measured 2026-11-08: 2.0 mg on some ticks).
pub(crate) fn solved_impulse(m: &ContactManifold) -> f32 {
    m.data
        .solver_contacts
        .iter()
        .filter_map(|c| m.points.get(c.contact_indices()[0] as usize))
        .map(|p| p.data.impulse)
        .sum()
}

/// [`apply_rolling_resistance`] with the per-manifold normal impulse `impulse`.
pub(crate) fn rolling_resistance(
    world: &mut PhysicsWorld,
    marbles: &[RigidBodyHandle],
    impulse: fn(&ContactManifold) -> f32,
) {
    for &h in marbles {
        let body = &world.bodies[h];
        let c_rr = f32::from_bits(body.user_data as u32);
        if c_rr <= 0.0 {
            continue;
        }
        let Some(&collider) = body.colliders().first() else {
            continue;
        };
        let Some(ball) = world.colliders[collider].shape().as_ball() else {
            continue;
        };
        let r = ball.radius;
        let load: f32 = world
            .narrow_phase
            .contact_pairs_with(collider)
            .filter(|pair| {
                let other = if pair.collider1 == collider {
                    pair.collider2
                } else {
                    pair.collider1
                };
                !is_marble(world, other)
            })
            .flat_map(|pair| pair.solver_manifolds())
            .map(impulse)
            .sum();
        if load <= 0.0 {
            continue;
        }
        let body = &mut world.bodies[h];
        let w = body.angvel();
        let speed = w.length();
        let i = 0.4 * body.mass() * r * r;
        let stop = c_rr * r * load / i;
        let w = if speed <= stop {
            Vector::ZERO
        } else {
            w * (1.0 - stop / speed)
        };
        body.set_angvel(w, true);
    }
}

/// Whether `c` is a marble: a ball on a dynamic body.
fn is_marble(world: &PhysicsWorld, c: ColliderHandle) -> bool {
    let collider = &world.colliders[c];
    collider.shape().as_ball().is_some()
        && collider
            .parent()
            .is_some_and(|b| world.bodies[b].is_dynamic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for k in MarbleKind::ALL {
            assert_eq!(MarbleKind::parse(k.name()), Some(k));
        }
        assert_eq!(MarbleKind::parse("agate"), None);
        assert_eq!(MarbleKind::NAMES.split('|').count(), MarbleKind::ALL.len());
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
    fn sizes_follow_the_contract_and_scale_mass_by_volume() {
        let mm = |s: MarbleSize| s.radius_m() * 2_000.0;
        assert!((mm(MarbleSize::Mini) - 10.0).abs() < 1e-4);
        assert!((mm(MarbleSize::Standard) - 16.0).abs() < 1e-4);
        assert!((mm(MarbleSize::Shooter) - 25.0).abs() < 1e-4);
        assert_eq!(MarbleSize::Standard.radius_m(), R16);
        for k in MarbleKind::ALL {
            let std_g = k.spec().mass_kg();
            assert_eq!(k.spec_sized(MarbleSize::Standard), k.spec());
            for s in MarbleSize::ALL {
                let spec = k.spec_sized(s);
                assert_eq!(spec.radius_m, s.radius_m(), "{k:?} {s:?}");
                // Same material at every size: mass ∝ r³.
                let ratio = (s.radius_m() / R16).powi(3);
                let m = spec.mass_kg();
                assert!((m / std_g - ratio).abs() < 1e-4, "{k:?} {s:?} mass {m}");
                assert_eq!(
                    (spec.density, spec.friction, spec.restitution),
                    (k.spec().density, k.spec().friction, k.spec().restitution)
                );
            }
        }
    }

    #[test]
    fn new_glass_types_have_published_glass_masses() {
        // Soda-lime glass, 2,520 kg/m³: 1.32 g mini, 5.40 g standard, 20.62 g shooter.
        for k in [
            MarbleKind::Swirl,
            MarbleKind::Onionskin,
            MarbleKind::Opal,
            MarbleKind::Glow,
        ] {
            let g = |s| k.spec_sized(s).mass_kg() * 1_000.0;
            assert!((g(MarbleSize::Mini) - 1.32).abs() < 0.01, "{k:?}");
            assert!((g(MarbleSize::Standard) - 5.40).abs() < 0.01, "{k:?}");
            assert!((g(MarbleSize::Shooter) - 20.62).abs() < 0.01, "{k:?}");
        }
        let steel_shooter = MarbleKind::Steel.spec_sized(MarbleSize::Shooter);
        assert!((steel_shooter.mass_kg() * 1_000.0 - 63.89).abs() < 0.01);
    }

    #[test]
    fn spec_for_keeps_the_legacy_default() {
        assert_eq!(spec_for(None, MarbleSize::Standard), P0_LEGACY);
        assert_eq!(
            spec_for(None, MarbleSize::Mini),
            MarbleKind::CatsEye.spec_sized(MarbleSize::Mini)
        );
        assert_eq!(spec_for(Some(MarbleKind::Opal), MarbleSize::Standard), OPAL);
        for s in MarbleSize::ALL {
            assert_eq!(MarbleSize::parse(s.name()), Some(s));
        }
        assert_eq!(MarbleSize::parse("huge"), None);
    }

    /// Every type × size: rolls down the ramp, stays in the tray for the whole run, and runs the
    /// same trajectory twice. Specs that differ give different trajectories; equal specs (the glass
    /// types share cat's eye's values) give the same one.
    #[test]
    fn every_type_and_size_rolls_down_the_ramp_and_stays_in_the_tray() {
        let half_w = scene::tray_width_m() / 2.0;
        let a = scene::ramp_run_m();
        let h = scene::ramp_height_m();
        let l = scene::tray_length_m();
        let tol = 1.0e-3;
        let mut runs: Vec<(MarbleSpec, u64)> = Vec::new();
        for k in MarbleKind::ALL {
            for size in MarbleSize::ALL {
                let spec = k.spec_sized(size);
                let r = spec.radius_m;
                let mut sim = crate::Sim::ramp_scene_with(spec);
                let mut max_x = f32::MIN;
                for _ in 0..crate::golden::RAMP_10S_TICKS {
                    sim.tick(1);
                    let [x, y, z] = sim.marbles()[0].position;
                    let surface = if x < a { h * (1.0 - x / a) } else { 0.0 };
                    let at = sim.tick_count();
                    assert!(y >= surface + r - tol, "{k:?} {size:?} tick {at}: y {y}");
                    assert!(y < surface + r + 0.01, "{k:?} {size:?} tick {at}: y {y}");
                    assert!(x >= r - tol && x <= l - r + tol, "{k:?} {size:?} x {x}");
                    assert!(z.abs() <= half_w - r + tol, "{k:?} {size:?} z {z}");
                    max_x = max_x.max(x);
                }
                assert!(
                    max_x > a + 4.0 * scene::cell_m(),
                    "{k:?} {size:?} max x {max_x}"
                );
                let hash = sim.trajectory_hash();
                let mut again = crate::Sim::ramp_scene_with(spec);
                again.tick(crate::golden::RAMP_10S_TICKS);
                assert_eq!(again.trajectory_hash(), hash, "{k:?} {size:?} rerun");
                runs.push((spec, hash));
            }
        }
        for (i, (sa, ha)) in runs.iter().enumerate() {
            for (sb, hb) in &runs[i + 1..] {
                assert_eq!(sa == sb, ha == hb, "{sa:?} vs {sb:?}");
            }
            assert_ne!(*ha, crate::golden::ramp_10s_hash(), "{sa:?} hit the golden");
        }
    }

    #[test]
    fn lineup_marbles_rest_in_a_row_without_touching() {
        let mut sim = crate::Sim::lineup_scene();
        sim.tick(crate::golden::RAMP_10S_TICKS);
        let ms = sim.marbles();
        assert_eq!(ms.len(), LINEUP.len());
        for (i, ((kind, size), m)) in LINEUP.iter().zip(&ms).enumerate() {
            let r = size.radius_m();
            let start = scene::lineup_marble_m(i, LINEUP.len(), r);
            let [x, y, z] = m.position;
            assert!(
                (x - start.x).abs() < 1e-3 && (z - start.z).abs() < 1e-3,
                "{kind:?} {m:?}"
            );
            assert!((y - r).abs() < 5e-4, "{kind:?} {size:?} y {y}");
        }
        for w in ms.windows(2) {
            assert!(
                w[1].position[0] - w[0].position[0] > 0.025,
                "lineup marbles overlap"
            );
        }
    }

    #[test]
    fn legacy_entry_is_the_p0_marble() {
        let (_, c) = P0_LEGACY.build(Vector::ZERO);
        assert_eq!(c.friction(), 0.4);
        assert_eq!(c.restitution(), 0.6);
        assert_eq!(P0_LEGACY.radius_m, scene::standard_marble_radius_m());
    }
}
