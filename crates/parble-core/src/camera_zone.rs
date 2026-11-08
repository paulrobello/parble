//! Camera zones (PRD §5.6): trigger volumes with a shot rule. When the followed marble is inside a
//! zone the run camera's director blends to the zone's shot, and blends back when it leaves.
//!
//! A track carries authored zones in [`crate::Track::camera_zones`]; pieces add default zones from
//! [`piece_hint`] ([`hint_zones`]). Zones are camera data only: the sim never reads them, so they
//! cannot change a trajectory hash. Like the rest of the track model they are integer units:
//! micrometres (`_um`), millidegrees (`_mdeg`), milliseconds (`_ms`) and permille.
//!
//! # Piece hints
//!
//! [`piece_hint`] maps a piece id to a [`CameraHint`]:
//!
//! - `mech_checkpoint_*`: [`CameraHint::Beat`], a short framing beat on the arch as the marble
//!   passes (a box over the piece's cells, held [`BEAT_HOLD_MS`] after the marble leaves).
//! - `blk_redirect_*` and `blk_tower_*`: [`CameraHint::Axis`], a vertical cylinder over the piece's
//!   cell column whose shot pulls back and frames the column's centre line with the marble.
//!
//! A future helix (P7) declares its axis zone the same way: add its id prefix to [`piece_hint`] as
//! [`CameraHint::Axis`]. [`hint_zones`] then builds a cylinder over the helix footprint's column,
//! with the radius of its plan extent and the axis from the footprint's lowest cell floor to its
//! highest cell top, so the camera frames the whole helix axis while the marble winds round it.

use serde::{Deserialize, Serialize};

use crate::catalog::PieceCatalog;
use crate::contract::{CELL_UM, HALF_CELL_UM};
use crate::track::Track;

/// Most zones a track may carry.
pub const MAX_ZONES: usize = 256;
/// Longest zone name, in characters.
pub const MAX_ZONE_NAME_CHARS: usize = 120;
/// Longest blend or hold time.
pub const MAX_ZONE_MS: u32 = 10_000;
/// Framing margin range: 1000 frames the target edge to edge, 5000 leaves it a fifth of the view.
pub const MARGIN_PERMILLE: (u32, u32) = (1_000, 5_000);
/// Field of view range of a shot.
pub const FOV_MDEG: (u32, u32) = (5_000, 120_000);
/// View elevation range of a framing shot (positive looks down on the target).
pub const ELEVATION_MDEG: (i32, i32) = (-80_000, 85_000);

/// The trigger volume. Boxes are axis-aligned; cylinders stand vertically on `base_um`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "lowercase", deny_unknown_fields)]
pub enum ZoneVolume {
    Box {
        min_um: [i32; 3],
        max_um: [i32; 3],
    },
    Cylinder {
        /// Centre of the bottom face.
        base_um: [i32; 3],
        height_um: i32,
        radius_um: i32,
    },
}

/// What a framing shot keeps in view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum ShotTarget {
    /// The followed marble alone. A struct variant, so `deny_unknown_fields` applies to it (serde
    /// ignores extra fields on an internally tagged unit variant).
    Marble {},
    /// A point, with the marble.
    Point { at_um: [i32; 3] },
    /// A vertical segment from `base_um` up `height_um` (a helix or drop axis), with the marble.
    Axis { base_um: [i32; 3], height_um: i32 },
}

/// Where the depth of field focuses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FocusOn {
    #[default]
    Marble,
    /// The shot's target (a point, or the axis's nearest point to the camera).
    Target,
}

/// A camera rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "rule", rename_all = "lowercase", deny_unknown_fields)]
pub enum ShotRule {
    /// Tracking shot behind the marble: `back_um` behind, `up_um` above, looking `ahead_um` ahead.
    Follow {
        back_um: i32,
        up_um: i32,
        ahead_um: i32,
    },
    /// Pull back and fit the target and the marble in view, `margin_permille` loose, looking down
    /// at `elevation_mdeg`. Without `azimuth_mdeg` the view keeps the bearing the camera had when
    /// the shot started (0 looks along −Z, 90 000 along −X).
    Frame {
        target: ShotTarget,
        #[serde(default)]
        focus: FocusOn,
        margin_permille: u32,
        elevation_mdeg: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        azimuth_mdeg: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fov_mdeg: Option<u32>,
    },
    /// Circle `center_um` at `radius_um`, `up_um` above it, at `speed_mdeg_s`, looking at it.
    Orbit {
        center_um: [i32; 3],
        radius_um: i32,
        up_um: i32,
        speed_mdeg_s: i32,
        #[serde(default)]
        focus: FocusOn,
    },
    /// A fixed camera.
    Fixed {
        eye_um: [i32; 3],
        look_um: [i32; 3],
        #[serde(default)]
        focus: FocusOn,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fov_mdeg: Option<u32>,
    },
}

/// One camera zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraZone {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub volume: ZoneVolume,
    pub shot: ShotRule,
    /// Overlapping zones resolve to the highest priority; a tie goes to the zone listed first.
    pub priority: i32,
    pub blend_in_ms: u32,
    pub blend_out_ms: u32,
    /// The zone stays active this long after the marble leaves (a short beat outlasts its volume).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub hold_ms: u32,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

impl CameraZone {
    /// Rules the types cannot express; `Err` names the first broken one.
    pub fn check(&self) -> Result<(), String> {
        if self.name.chars().count() > MAX_ZONE_NAME_CHARS {
            return Err(format!(
                "zone names are limited to {MAX_ZONE_NAME_CHARS} characters"
            ));
        }
        match self.volume {
            ZoneVolume::Box { min_um, max_um } => {
                if (0..3).any(|i| min_um[i] >= max_um[i]) {
                    return Err("box min_um must be below max_um on every axis".into());
                }
            }
            ZoneVolume::Cylinder {
                height_um,
                radius_um,
                ..
            } => {
                if height_um <= 0 || radius_um <= 0 {
                    return Err("cylinder height_um and radius_um must be positive".into());
                }
            }
        }
        for (what, ms) in [
            ("blend_in_ms", self.blend_in_ms),
            ("blend_out_ms", self.blend_out_ms),
            ("hold_ms", self.hold_ms),
        ] {
            if ms > MAX_ZONE_MS {
                return Err(format!("{what} is limited to {MAX_ZONE_MS}"));
            }
        }
        let fov = |f: Option<u32>| match f {
            Some(f) if !(FOV_MDEG.0..=FOV_MDEG.1).contains(&f) => Err(format!(
                "fov_mdeg must be in {}..={}",
                FOV_MDEG.0, FOV_MDEG.1
            )),
            _ => Ok(()),
        };
        match self.shot {
            ShotRule::Follow { back_um, .. } => {
                if back_um <= 0 {
                    return Err("follow back_um must be positive".into());
                }
            }
            ShotRule::Frame {
                target,
                margin_permille,
                elevation_mdeg,
                fov_mdeg,
                ..
            } => {
                if let ShotTarget::Axis { height_um, .. } = target
                    && height_um <= 0
                {
                    return Err("axis height_um must be positive".into());
                }
                if !(MARGIN_PERMILLE.0..=MARGIN_PERMILLE.1).contains(&margin_permille) {
                    return Err(format!(
                        "margin_permille must be in {}..={}",
                        MARGIN_PERMILLE.0, MARGIN_PERMILLE.1
                    ));
                }
                if !(ELEVATION_MDEG.0..=ELEVATION_MDEG.1).contains(&elevation_mdeg) {
                    return Err(format!(
                        "elevation_mdeg must be in {}..={}",
                        ELEVATION_MDEG.0, ELEVATION_MDEG.1
                    ));
                }
                fov(fov_mdeg)?;
            }
            ShotRule::Orbit { radius_um, .. } => {
                if radius_um <= 0 {
                    return Err("orbit radius_um must be positive".into());
                }
            }
            ShotRule::Fixed {
                eye_um,
                look_um,
                fov_mdeg,
                ..
            } => {
                if eye_um == look_um {
                    return Err("fixed eye_um and look_um must differ".into());
                }
                fov(fov_mdeg)?;
            }
        }
        Ok(())
    }
}

/// A piece's default camera behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraHint {
    /// A short framing beat on the piece as the marble passes.
    Beat,
    /// Frame the vertical axis of the piece's cell column.
    Axis,
}

/// Default hint priority: below authored zones, which a track usually gives 10 or more.
pub const HINT_PRIORITY: i32 = 1;
/// How long a beat holds after the marble leaves its piece.
pub const BEAT_HOLD_MS: u32 = 900;

/// The camera hint of a piece id, if it has one.
pub fn piece_hint(id: &str) -> Option<CameraHint> {
    if id.starts_with("mech_checkpoint_") {
        Some(CameraHint::Beat)
    } else if id.starts_with("blk_redirect_") || id.starts_with("blk_tower_") {
        Some(CameraHint::Axis)
    } else {
        None
    }
}

/// The default zones of every hinted placement in `track`, in placement order. Placements whose
/// piece the catalog does not know are skipped.
pub fn hint_zones(track: &Track, catalog: &impl PieceCatalog) -> Vec<CameraZone> {
    let mut zones = Vec::new();
    for p in &track.placements {
        let (Some(hint), Some(def)) = (piece_hint(p.piece.as_str()), catalog.get(&p.piece)) else {
            continue;
        };
        let cells = p.world_cells(def);
        let Some(first) = cells.first() else {
            continue;
        };
        let (mut lo, mut hi) = ([first.x, first.y, first.z], [first.x, first.y, first.z]);
        for c in &cells {
            for (i, v) in [c.x, c.y, c.z].into_iter().enumerate() {
                lo[i] = lo[i].min(v);
                hi[i] = hi[i].max(v);
            }
        }
        // Cell (x, y, z) spans x·cell ± half a cell, y·cell up one cell, z·cell ± half a cell.
        let min_um = [
            lo[0] * CELL_UM - HALF_CELL_UM,
            lo[1] * CELL_UM,
            lo[2] * CELL_UM - HALF_CELL_UM,
        ];
        let max_um = [
            hi[0] * CELL_UM + HALF_CELL_UM,
            (hi[1] + 1) * CELL_UM,
            hi[2] * CELL_UM + HALF_CELL_UM,
        ];
        let centre = [
            (min_um[0] + max_um[0]) / 2,
            (min_um[1] + max_um[1]) / 2,
            (min_um[2] + max_um[2]) / 2,
        ];
        let name = format!(
            "{} {:?}",
            p.piece.as_str(),
            [p.anchor.x, p.anchor.y, p.anchor.z]
        );
        zones.push(match hint {
            CameraHint::Beat => CameraZone {
                name,
                volume: ZoneVolume::Box { min_um, max_um },
                shot: ShotRule::Frame {
                    target: ShotTarget::Point { at_um: centre },
                    focus: FocusOn::Marble,
                    margin_permille: 2_600,
                    elevation_mdeg: 20_000,
                    azimuth_mdeg: None,
                    fov_mdeg: None,
                },
                priority: HINT_PRIORITY,
                blend_in_ms: 450,
                blend_out_ms: 800,
                hold_ms: BEAT_HOLD_MS,
            },
            CameraHint::Axis => {
                let base_um = [centre[0], min_um[1], centre[2]];
                let height_um = max_um[1] - min_um[1];
                let radius_um = (max_um[0] - min_um[0]).max(max_um[2] - min_um[2]) / 2;
                CameraZone {
                    name,
                    volume: ZoneVolume::Cylinder {
                        base_um,
                        height_um,
                        radius_um,
                    },
                    shot: ShotRule::Frame {
                        target: ShotTarget::Axis { base_um, height_um },
                        focus: FocusOn::Marble,
                        margin_permille: 1_600,
                        elevation_mdeg: 18_000,
                        azimuth_mdeg: None,
                        fov_mdeg: None,
                    },
                    priority: HINT_PRIORITY,
                    blend_in_ms: 700,
                    blend_out_ms: 1_000,
                    hold_ms: 300,
                }
            }
        });
    }
    zones
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn zone() -> CameraZone {
        CameraZone {
            name: "tower".into(),
            volume: ZoneVolume::Cylinder {
                base_um: [1_440_000, 120_000, -420_000],
                height_um: 180_000,
                radius_um: 60_000,
            },
            shot: ShotRule::Frame {
                target: ShotTarget::Axis {
                    base_um: [1_440_000, 120_000, -420_000],
                    height_um: 180_000,
                },
                focus: FocusOn::Target,
                margin_permille: 1_400,
                elevation_mdeg: 15_000,
                azimuth_mdeg: Some(0),
                fov_mdeg: None,
            },
            priority: 10,
            blend_in_ms: 900,
            blend_out_ms: 1_200,
            hold_ms: 0,
        }
    }

    #[test]
    fn a_zone_round_trips_and_omits_its_defaults() {
        let z = zone();
        let v = serde_json::to_value(&z).unwrap();
        assert!(v.get("hold_ms").is_none());
        assert!(v["shot"].get("fov_mdeg").is_none());
        assert_eq!(v["volume"]["shape"], "cylinder");
        assert_eq!(v["shot"]["target"]["kind"], "axis");
        let back: CameraZone = serde_json::from_value(v).unwrap();
        assert_eq!(back, z);
        assert_eq!(z.check(), Ok(()));
    }

    #[test]
    fn unknown_fields_are_refused_at_every_level() {
        let good = serde_json::to_value(zone()).unwrap();
        for path in ["", "/volume", "/shot", "/shot/target"] {
            let mut v = good.clone();
            v.pointer_mut(path)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("extra".into(), json!(1));
            assert!(
                serde_json::from_value::<CameraZone>(v).is_err(),
                "accepted an extra field at {path:?}"
            );
        }
        let mut v = good;
        v["shot"]["target"] = json!({"kind": "marble", "at_um": [0, 0, 0]});
        assert!(serde_json::from_value::<CameraZone>(v).is_err());
    }

    #[test]
    fn degenerate_zones_fail_the_check() {
        let mut z = zone();
        z.volume = ZoneVolume::Box {
            min_um: [0, 0, 0],
            max_um: [10, 0, 10],
        };
        assert!(z.check().is_err());
        let mut z = zone();
        z.blend_in_ms = MAX_ZONE_MS + 1;
        assert!(z.check().is_err());
        let mut z = zone();
        if let ShotRule::Frame {
            margin_permille, ..
        } = &mut z.shot
        {
            *margin_permille = 900;
        }
        assert!(z.check().is_err());
    }

    #[test]
    fn hints_cover_checkpoints_redirects_and_towers_only() {
        assert_eq!(piece_hint("mech_checkpoint_l1"), Some(CameraHint::Beat));
        assert_eq!(piece_hint("blk_redirect_l3"), Some(CameraHint::Axis));
        assert_eq!(piece_hint("blk_tower_socket_l1"), Some(CameraHint::Axis));
        assert_eq!(piece_hint("blk_straight_l3"), None);
    }
}
