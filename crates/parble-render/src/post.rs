//! Camera post stack per [`Quality`] tier: anti-aliasing (MSAA or TAA), SSAO, bloom, depth of field
//! and tonemapping. The table and the reasons behind it are in `docs/adr/0001-post-stack-and-tiers.md`.
//!
//! Bevy constraints: TAA and SSAO both require `Msaa::Off`, so MSAA is only used on Medium. Bloom
//! requires `Hdr` (pulled in by its `#[require]`), TAA requires the depth and motion-vector
//! prepasses, SSAO the depth and normal prepasses. `Option<Component>` is not a `Bundle` in 0.20,
//! so the optional effects are inserted one by one in [`insert_post`].

use bevy::{
    anti_alias::taa::TemporalAntiAliasing,
    core_pipeline::tonemapping::Tonemapping,
    pbr::{ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel},
    post_process::{
        bloom::Bloom,
        dof::{DepthOfField, DepthOfFieldMode},
    },
    prelude::*,
};
use serde::Serialize;

use crate::Quality;

/// Anti-aliasing per tier. TAA and MSAA are mutually exclusive in Bevy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AntiAlias {
    None,
    Msaa4,
    Taa,
}

/// Depth of field per tier.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DofTier {
    pub bokeh: bool,
    /// Aperture in f-stops; smaller is a shallower focus.
    pub aperture_f_stops: f32,
    /// Largest circle of confusion, in pixels.
    pub max_coc_px: f32,
}

/// The post settings for one tier.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostTier {
    pub anti_alias: AntiAlias,
    /// SSAO quality, `None` when off.
    pub ssao: Option<&'static str>,
    pub bloom: bool,
    pub dof: Option<DofTier>,
    pub shadow_map: Option<u32>,
}

/// Chosen during P1-T7 look-dev (ADR 0001): ACES fitted gives ref-01's contrast, deep shadows and
/// warm wood. `--tonemap tony|agx` stay available for the P1-T9 sign-off.
pub const TONEMAPPING: Tonemapping = Tonemapping::AcesFitted;

/// Bloom on every tier: a low-intensity natural bloom so lamp and glass highlights glow.
pub const BLOOM_INTENSITY: f32 = 0.08;

/// Depth past which the background is clamped for the DoF (the backdrop is at infinity).
pub const DOF_MAX_DEPTH_M: f32 = 4.0;

impl Quality {
    pub fn post(self) -> PostTier {
        match self {
            Self::Low => PostTier {
                anti_alias: AntiAlias::None,
                ssao: None,
                bloom: true,
                dof: None,
                shadow_map: None,
            },
            Self::Medium => PostTier {
                anti_alias: AntiAlias::Msaa4,
                ssao: None,
                bloom: true,
                dof: Some(DofTier {
                    bokeh: false,
                    aperture_f_stops: 4.0,
                    max_coc_px: 24.0,
                }),
                shadow_map: Some(2048),
            },
            Self::High => PostTier {
                anti_alias: AntiAlias::Taa,
                ssao: Some("medium"),
                bloom: true,
                dof: Some(DofTier {
                    bokeh: true,
                    aperture_f_stops: 2.8,
                    max_coc_px: 48.0,
                }),
                shadow_map: Some(4096),
            },
            Self::Ultra => PostTier {
                anti_alias: AntiAlias::Taa,
                ssao: Some("high"),
                bloom: true,
                dof: Some(DofTier {
                    bokeh: true,
                    aperture_f_stops: 2.0,
                    max_coc_px: 64.0,
                }),
                shadow_map: Some(4096),
            },
        }
    }
}

/// Look-dev and test overrides on top of the tier (`--taa 0`, `--dof 0`, `--tonemap`).
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
pub struct PostOverrides {
    /// `Some(false)` turns TAA off on a TAA tier (no AA then, never MSAA, so SSAO still runs).
    pub taa: Option<bool>,
    pub dof: Option<bool>,
    /// Aperture override, f-stops.
    pub aperture: Option<f32>,
    pub tonemapping: Option<Tonemapping>,
}

/// Parse a `--tonemap` name.
pub fn parse_tonemapping(s: &str) -> Option<Tonemapping> {
    Some(match s {
        "tony" => Tonemapping::TonyMcMapface,
        "agx" => Tonemapping::AgX,
        "aces" => Tonemapping::AcesFitted,
        "filmic" => Tonemapping::BlenderFilmic,
        "sbdt" => Tonemapping::SomewhatBoringDisplayTransform,
        "reinhard" => Tonemapping::ReinhardLuminance,
        _ => return None,
    })
}

/// The effective tier after overrides.
pub fn effective(quality: Quality, o: &PostOverrides) -> PostTier {
    let mut t = quality.post();
    if o.taa == Some(false) && t.anti_alias == AntiAlias::Taa {
        t.anti_alias = AntiAlias::None;
    }
    if o.dof == Some(false) {
        t.dof = None;
    }
    if let (Some(f), Some(dof)) = (o.aperture, t.dof.as_mut()) {
        dof.aperture_f_stops = f;
    }
    t
}

fn ssao_level(name: &str) -> ScreenSpaceAmbientOcclusionQualityLevel {
    match name {
        "low" => ScreenSpaceAmbientOcclusionQualityLevel::Low,
        "medium" => ScreenSpaceAmbientOcclusionQualityLevel::Medium,
        "ultra" => ScreenSpaceAmbientOcclusionQualityLevel::Ultra,
        _ => ScreenSpaceAmbientOcclusionQualityLevel::High,
    }
}

/// Give a camera its tier's post stack.
pub fn insert_post(cam: &mut EntityCommands, quality: Quality, o: &PostOverrides) {
    let t = effective(quality, o);
    cam.insert((
        match t.anti_alias {
            AntiAlias::Msaa4 => Msaa::Sample4,
            AntiAlias::None | AntiAlias::Taa => Msaa::Off,
        },
        o.tonemapping.unwrap_or(TONEMAPPING),
    ));
    if t.anti_alias == AntiAlias::Taa {
        cam.insert(TemporalAntiAliasing::default());
    }
    if let Some(level) = t.ssao {
        cam.insert(ScreenSpaceAmbientOcclusion {
            quality_level: ssao_level(level),
            ..default()
        });
    }
    if t.bloom {
        cam.insert(Bloom {
            intensity: BLOOM_INTENSITY,
            ..Bloom::NATURAL
        });
    }
    if let Some(dof) = t.dof {
        cam.insert(DepthOfField {
            mode: if dof.bokeh {
                DepthOfFieldMode::Bokeh
            } else {
                DepthOfFieldMode::Gaussian
            },
            focal_distance: 0.15,
            aperture_f_stops: dof.aperture_f_stops,
            max_circle_of_confusion_diameter: dof.max_coc_px,
            max_depth: DOF_MAX_DEPTH_M,
            ..default()
        });
    }
}

/// What the live camera actually carries, for the state JSON (`camera.post`).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LivePost {
    pub msaa: u32,
    pub taa: bool,
    pub ssao: Option<String>,
    pub bloom: Option<f32>,
    pub dof: Option<String>,
    pub aperture: Option<f32>,
    pub focus_m: Option<f32>,
    pub tonemapping: String,
}

#[allow(clippy::type_complexity)]
pub fn live_post(
    q: (
        &Msaa,
        Option<&TemporalAntiAliasing>,
        Option<&ScreenSpaceAmbientOcclusion>,
        Option<&Bloom>,
        Option<&DepthOfField>,
        Option<&Tonemapping>,
    ),
) -> LivePost {
    let (msaa, taa, ssao, bloom, dof, tm) = q;
    LivePost {
        msaa: msaa.samples(),
        taa: taa.is_some(),
        ssao: ssao.map(|s| format!("{:?}", s.quality_level).to_lowercase()),
        bloom: bloom.map(|b| b.intensity),
        dof: dof.map(|d| format!("{:?}", d.mode).to_lowercase()),
        aperture: dof.map(|d| d.aperture_f_stops),
        focus_m: dof.map(|d| d.focal_distance),
        tonemapping: tm.map(|t| format!("{t:?}")).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taa_and_ssao_tiers_never_use_msaa() {
        for q in [Quality::Low, Quality::Medium, Quality::High, Quality::Ultra] {
            let t = q.post();
            if t.anti_alias == AntiAlias::Taa || t.ssao.is_some() {
                assert_ne!(t.anti_alias, AntiAlias::Msaa4, "{q:?}");
            }
        }
    }

    #[test]
    fn high_and_ultra_run_the_full_stack() {
        for q in [Quality::High, Quality::Ultra] {
            let t = q.post();
            assert_eq!(t.anti_alias, AntiAlias::Taa);
            assert!(t.ssao.is_some() && t.bloom);
            assert!(t.dof.is_some_and(|d| d.bokeh));
        }
        assert!(
            Quality::Ultra.post().dof.unwrap().aperture_f_stops
                < Quality::High.post().dof.unwrap().aperture_f_stops
        );
    }

    #[test]
    fn taa_override_drops_to_no_aa_not_msaa() {
        let o = PostOverrides {
            taa: Some(false),
            ..default()
        };
        let t = effective(Quality::High, &o);
        assert_eq!(t.anti_alias, AntiAlias::None);
        assert!(t.ssao.is_some());
        assert_eq!(effective(Quality::Medium, &o).anti_alias, AntiAlias::Msaa4);
    }
}
