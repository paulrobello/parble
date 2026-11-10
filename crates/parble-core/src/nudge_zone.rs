//! Nudge zones (PRD §6 P3 editor, owner request 2026-11-09): invisible editor-placed boxes that
//! gently push a marble along a direction while its centre is inside, to keep marbles moving past
//! weak spots.
//!
//! A track carries them in [`crate::Track::nudge_zones`], omitted from the file when empty. Unlike
//! camera zones ([`crate::camera_zone`]) the sim reads them: `parble-sim` applies them every tick
//! before the physics step. A track with no nudge zones runs exactly as it did before they
//! existed, so its trajectory hash does not change.
//!
//! # The push ("top up")
//!
//! Each tick, for each marble in spawn order and each zone in track order: when the marble's
//! centre (quantized to 1 µm, like the trajectory hash) is inside the zone's box, and its speed
//! along the zone's direction is below `min_speed_um_s`, its velocity gains
//! `min(max_accel_um_s2 · dt, min_speed − speed along)` along the direction. A marble already at
//! or above the minimum speed along the direction is left alone, so a zone never slows a marble,
//! never pushes one that is moving well, and adds nothing sideways. Overlapping zones each top up
//! on their own, in track order.
//!
//! # Constant mode
//!
//! A zone with `mode: "constant"` pushes every marble whose centre is inside by `max_accel_um_s2
//! · dt` along its direction every tick, whatever the marble's speed: a steady force, like a
//! steeper incline over the box, not a top-up. `min_speed_um_s` is kept (and still checked) but
//! unused. The default mode is top-up, and the file omits `mode` when it is top-up, so files and
//! trajectory hashes from before the field existed are unchanged.
//!
//! # The acceleration cap
//!
//! The default acceleration, [`DEFAULT_MAX_ACCEL_UM_S2`], is 1 m/s²: the pull gravity gives a
//! marble on a 1 in 10 incline (9.81 m/s² · sin 5.7°). At 240 Hz that is 4.2 mm/s per tick, and a
//! stopped marble takes 0.2 s and 2 cm to reach the default 0.2 m/s minimum, so a nudged marble
//! reads as one on a slightly steeper track, not as one being shoved. A file may raise it to
//! [`MAX_ACCEL_UM_S2`], 2 m/s² (a 1 in 5 incline), and no further.
//!
//! # Units
//!
//! Integer, like the rest of the track model: the box in micrometres (`_um`), the direction as a
//! yaw and a pitch in millidegrees (`_mdeg`), speed in µm/s and acceleration in µm/s².
//!
//! - `yaw_mdeg`, `0..360_000`: the heading in plan. 0 points along +X; a positive yaw turns the
//!   way a placement's quarter turn does, so 90 000 points along −Z, 180 000 along −X and
//!   270 000 along +Z.
//! - `pitch_mdeg`, `-90_000..=90_000`: above (positive) or below the horizontal.
//!
//! The direction is `(cos p · cos y, sin p, −cos p · sin y)`.

use serde::{Deserialize, Serialize};

use crate::contract::{CELL_UM, HALF_CELL_UM};
use crate::grid::CellCoord;

/// Most nudge zones a track may carry.
pub const MAX_NUDGE_ZONES: usize = 256;
/// Longest nudge zone name, in characters.
pub const MAX_NUDGE_NAME_CHARS: usize = 120;
/// Highest minimum speed a zone may ask for, µm/s (2 m/s).
pub const MAX_MIN_SPEED_UM_S: u32 = 2_000_000;
/// Highest acceleration cap a zone may set, µm/s² (2 m/s²).
pub const MAX_ACCEL_UM_S2: u32 = 2_000_000;
/// The editor's default minimum speed for a new zone, µm/s (0.2 m/s).
pub const DEFAULT_MIN_SPEED_UM_S: u32 = 200_000;
/// The editor's default acceleration cap for a new zone, µm/s² (1 m/s², see the module docs).
pub const DEFAULT_MAX_ACCEL_UM_S2: u32 = 1_000_000;
/// One full turn of yaw, millidegrees.
pub const FULL_TURN_MDEG: i32 = 360_000;
/// Pitch range, millidegrees.
pub const PITCH_MDEG: (i32, i32) = (-90_000, 90_000);

/// How a zone pushes (module docs). Written `"top_up"` or `"constant"`; omitted when top-up.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NudgeMode {
    /// Push only while the speed along the direction is below the minimum, up to it.
    #[default]
    TopUp,
    /// Push at the zone's acceleration every tick, whatever the speed.
    Constant,
}

impl NudgeMode {
    pub fn is_top_up(&self) -> bool {
        *self == Self::TopUp
    }
}

/// One nudge zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NudgeZone {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Top-up (the default, omitted from the file) or constant.
    #[serde(default, skip_serializing_if = "NudgeMode::is_top_up")]
    pub mode: NudgeMode,
    /// The axis-aligned box, µm: a marble whose centre is in `min_um..=max_um` on every axis is
    /// inside.
    pub min_um: [i32; 3],
    pub max_um: [i32; 3],
    /// Heading in plan, `0..360_000` (module docs).
    pub yaw_mdeg: i32,
    /// Above or below the horizontal, `-90_000..=90_000`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pitch_mdeg: i32,
    /// The speed along the direction the zone tops a marble up to, µm/s, `1..=2_000_000`. Unused
    /// in constant mode.
    pub min_speed_um_s: u32,
    /// The most the zone adds per second, µm/s², `1..=2_000_000`: the cap in top-up mode, the
    /// steady push in constant mode.
    pub max_accel_um_s2: u32,
}

fn is_zero(v: &i32) -> bool {
    *v == 0
}

impl NudgeZone {
    /// A zone over the box of whole cells from `lo` to `hi` (inclusive, any order), pointing at
    /// `yaw_mdeg` with the default speed and acceleration. A cell spans its centre ± half a cell
    /// in x and z and one cell up from its floor in y.
    pub fn over_cells(lo: CellCoord, hi: CellCoord, yaw_mdeg: i32) -> Self {
        let (a, b) = (
            [lo.x.min(hi.x), lo.y.min(hi.y), lo.z.min(hi.z)],
            [lo.x.max(hi.x), lo.y.max(hi.y), lo.z.max(hi.z)],
        );
        Self {
            name: String::new(),
            mode: NudgeMode::TopUp,
            min_um: [
                a[0] * CELL_UM - HALF_CELL_UM,
                a[1] * CELL_UM,
                a[2] * CELL_UM - HALF_CELL_UM,
            ],
            max_um: [
                b[0] * CELL_UM + HALF_CELL_UM,
                (b[1] + 1) * CELL_UM,
                b[2] * CELL_UM + HALF_CELL_UM,
            ],
            yaw_mdeg,
            pitch_mdeg: 0,
            min_speed_um_s: DEFAULT_MIN_SPEED_UM_S,
            max_accel_um_s2: DEFAULT_MAX_ACCEL_UM_S2,
        }
    }

    /// Whether a point, µm, is inside the box (faces included).
    pub fn contains_um(&self, p: [i64; 3]) -> bool {
        (0..3).all(|i| i64::from(self.min_um[i]) <= p[i] && p[i] <= i64::from(self.max_um[i]))
    }

    /// Rules the types cannot express; `Err` names the first broken one.
    pub fn check(&self) -> Result<(), String> {
        if self.name.chars().count() > MAX_NUDGE_NAME_CHARS {
            return Err(format!(
                "nudge zone names are limited to {MAX_NUDGE_NAME_CHARS} characters"
            ));
        }
        if (0..3).any(|i| self.min_um[i] >= self.max_um[i]) {
            return Err("min_um must be below max_um on every axis".into());
        }
        if !(0..FULL_TURN_MDEG).contains(&self.yaw_mdeg) {
            return Err(format!("yaw_mdeg must be in 0..{FULL_TURN_MDEG}"));
        }
        if !(PITCH_MDEG.0..=PITCH_MDEG.1).contains(&self.pitch_mdeg) {
            return Err(format!(
                "pitch_mdeg must be in {}..={}",
                PITCH_MDEG.0, PITCH_MDEG.1
            ));
        }
        if !(1..=MAX_MIN_SPEED_UM_S).contains(&self.min_speed_um_s) {
            return Err(format!(
                "min_speed_um_s must be in 1..={MAX_MIN_SPEED_UM_S}"
            ));
        }
        if !(1..=MAX_ACCEL_UM_S2).contains(&self.max_accel_um_s2) {
            return Err(format!("max_accel_um_s2 must be in 1..={MAX_ACCEL_UM_S2}"));
        }
        Ok(())
    }
}

/// `yaw_mdeg` brought into `0..360_000`.
pub fn wrap_yaw(yaw_mdeg: i32) -> i32 {
    yaw_mdeg.rem_euclid(FULL_TURN_MDEG)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn zone() -> NudgeZone {
        NudgeZone::over_cells(CellCoord::new(2, 1, -6), CellCoord::new(4, 1, -6), 0)
    }

    #[test]
    fn a_zone_round_trips_and_omits_its_defaults() {
        let z = zone();
        assert_eq!(z.min_um, [90_000, 60_000, -390_000]);
        assert_eq!(z.max_um, [270_000, 120_000, -330_000]);
        let v = serde_json::to_value(&z).unwrap();
        assert!(v.get("name").is_none());
        assert!(v.get("pitch_mdeg").is_none());
        assert!(
            v.get("mode").is_none(),
            "top-up is the default and is omitted"
        );
        let back: NudgeZone = serde_json::from_value(v).unwrap();
        assert_eq!(back, z);
        assert_eq!(z.check(), Ok(()));
        let named = NudgeZone {
            name: "lift".into(),
            pitch_mdeg: 10_000,
            ..zone()
        };
        let back: NudgeZone =
            serde_json::from_value(serde_json::to_value(&named).unwrap()).unwrap();
        assert_eq!(back, named);
    }

    #[test]
    fn constant_mode_is_written_and_read_back() {
        let z = NudgeZone {
            mode: NudgeMode::Constant,
            ..zone()
        };
        let v = serde_json::to_value(&z).unwrap();
        assert_eq!(v["mode"], json!("constant"));
        assert_eq!(serde_json::from_value::<NudgeZone>(v).unwrap(), z);
        let mut v = serde_json::to_value(zone()).unwrap();
        v.as_object_mut()
            .unwrap()
            .insert("mode".into(), json!("top_up"));
        assert_eq!(
            serde_json::from_value::<NudgeZone>(v.clone()).unwrap(),
            zone()
        );
        v["mode"] = json!("sideways");
        assert!(serde_json::from_value::<NudgeZone>(v).is_err());
    }

    #[test]
    fn unknown_fields_are_refused() {
        let mut v = serde_json::to_value(zone()).unwrap();
        v.as_object_mut().unwrap().insert("extra".into(), json!(1));
        assert!(serde_json::from_value::<NudgeZone>(v).is_err());
    }

    #[test]
    fn out_of_range_zones_fail_the_check() {
        let bad = [
            NudgeZone {
                max_um: [90_000, 120_000, 0],
                ..zone()
            },
            NudgeZone {
                yaw_mdeg: FULL_TURN_MDEG,
                ..zone()
            },
            NudgeZone {
                yaw_mdeg: -1,
                ..zone()
            },
            NudgeZone {
                pitch_mdeg: 90_001,
                ..zone()
            },
            NudgeZone {
                min_speed_um_s: 0,
                ..zone()
            },
            NudgeZone {
                min_speed_um_s: MAX_MIN_SPEED_UM_S + 1,
                ..zone()
            },
            NudgeZone {
                max_accel_um_s2: MAX_ACCEL_UM_S2 + 1,
                ..zone()
            },
            NudgeZone {
                max_accel_um_s2: 0,
                ..zone()
            },
            NudgeZone {
                name: "n".repeat(MAX_NUDGE_NAME_CHARS + 1),
                ..zone()
            },
        ];
        for z in bad {
            assert!(z.check().is_err(), "{z:?}");
        }
    }

    #[test]
    fn the_box_holds_its_faces_and_nothing_past_them() {
        let z = zone();
        assert!(z.contains_um([90_000, 60_000, -390_000]));
        assert!(z.contains_um([270_000, 120_000, -330_000]));
        assert!(!z.contains_um([89_999, 90_000, -360_000]));
        assert!(!z.contains_um([180_000, 120_001, -360_000]));
    }

    #[test]
    fn yaw_wraps_into_one_turn() {
        assert_eq!(wrap_yaw(-45_000), 315_000);
        assert_eq!(wrap_yaw(360_000), 0);
        assert_eq!(wrap_yaw(405_000), 45_000);
    }
}
