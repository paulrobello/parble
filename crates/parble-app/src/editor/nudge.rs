//! Nudge zone editing ([`parble_core::nudge_zone`], owner request 2026-11-09).
//!
//! Zones live in the track, so every change is an [`EditCommand`] with undo and redo, bumps
//! `edits` (the marble check goes stale, a run build starts over) and is handed to the live sim
//! for the next tick ([`super::finish_edit`]). The selected zone is [`parble_core::Editor::nudge`],
//! apart from the placement selection: selecting one clears the other.
//!
//! - `nudgeAdd`: a zone over `cells` (two corner cells), else over the selected placements' cells.
//! - `nudgeSelect`, `nudgeSet` (the whole zone, `mode` included, one undo step), `nudgeRemove`.
//! - With a zone selected and no pieces, `move` shifts it by whole cells, `rotate` turns its yaw a
//!   quarter turn per step, and `remove` deletes it.
//! - In the select tool a click that hits no piece selects the zone it passes through (nearest
//!   entry); in the delete tool such a click deletes it.
//!
//! `editor.nudges` lists the zones with their 8 projected box corners and a projected direction
//! arrow, refreshed every frame, for the overlay to draw. Nothing here renders in 3D: a run (and
//! every capture) never shows a zone.

use bevy::prelude::*;
use parble_core::nudge_zone::wrap_yaw;
use parble_core::{CellCoord, EditCommand, NudgeMode, NudgeZone, PieceCatalog};
use serde::Serialize;

use super::{Ctx, EditState, cell_arr, project_box, run_command};

/// Zones listed in the diag at most (`count` is always exact).
pub const MAX_LISTED_NUDGES: usize = 64;
/// One quarter turn, millidegrees.
const QUARTER_MDEG: i32 = 90_000;
/// The arrow spans this share of the box's chord through its centre along the direction.
const ARROW_SHARE: f32 = 0.7;

/// `editor.nudges`.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NudgeDiag {
    pub count: usize,
    /// The selected zone's index.
    pub selected: Option<u32>,
    /// The first [`MAX_LISTED_NUDGES`] zones, in track order.
    pub zones: Vec<NudgeZoneDiag>,
}

/// One zone: its track fields (as in the `.parble` file, in camelCase) and where it is on screen.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NudgeZoneDiag {
    pub index: u32,
    pub name: String,
    /// `"top_up"` or `"constant"`, as in the file (where top-up is omitted).
    pub mode: NudgeMode,
    pub min_um: [i32; 3],
    pub max_um: [i32; 3],
    pub yaw_mdeg: i32,
    pub pitch_mdeg: i32,
    pub min_speed_um_s: u32,
    pub max_accel_um_s2: u32,
    /// The cell holding the box's centre (the Show target).
    pub cell: [i32; 3],
    /// The box's 8 corners in CSS px (corner `i` takes the high x, y and z for bits 0, 1 and 2),
    /// or `null` off screen.
    pub outline: Option<Vec<[f32; 2]>>,
    /// The direction arrow's tail and head in CSS px, or `null` off screen.
    pub arrow: Option<[[f32; 2]; 2]>,
}

fn m(um: i32) -> f32 {
    (f64::from(um) * 1.0e-6) as f32
}

/// The box in metres.
fn box_m(z: &NudgeZone) -> (Vec3, Vec3) {
    (
        Vec3::new(m(z.min_um[0]), m(z.min_um[1]), m(z.min_um[2])),
        Vec3::new(m(z.max_um[0]), m(z.max_um[1]), m(z.max_um[2])),
    )
}

fn dir(z: &NudgeZone) -> Vec3 {
    Vec3::from_array(parble_sim::nudge::direction(z.yaw_mdeg, z.pitch_mdeg))
}

/// The arrow's tail and head, metres: through the box centre along the direction, over
/// [`ARROW_SHARE`] of the chord.
pub fn arrow_m(z: &NudgeZone) -> (Vec3, Vec3) {
    let (lo, hi) = box_m(z);
    let (c, size, d) = ((lo + hi) * 0.5, hi - lo, dir(z));
    let chord = (0..3)
        .filter(|&i| d[i].abs() > 1e-4)
        .map(|i| size[i] / d[i].abs())
        .fold(f32::INFINITY, f32::min);
    let half = d * (chord * ARROW_SHARE * 0.5);
    (c - half, c + half)
}

/// The cell holding the box's centre.
fn centre_cell(z: &NudgeZone) -> CellCoord {
    let (lo, hi) = box_m(z);
    let c = (lo + hi) * 0.5 / super::CELL_M;
    CellCoord::new(
        (c.x + 0.5).floor() as i32,
        c.y.floor() as i32,
        (c.z + 0.5).floor() as i32,
    )
}

/// The cell box of the selected placements' footprints.
fn selection_cells(st: &EditState) -> Option<(CellCoord, CellCoord)> {
    let placements = &st.editor.track().placements;
    let cells: Vec<CellCoord> = st
        .editor
        .selection
        .iter()
        .filter_map(|i| placements.get(i as usize))
        .flat_map(|p| {
            st.kit
                .get(&p.piece)
                .map_or_else(|| vec![p.anchor], |def| p.world_cells(def))
        })
        .collect();
    let first = *cells.first()?;
    Some(cells.iter().fold((first, first), |(lo, hi), c| {
        (
            CellCoord::new(lo.x.min(c.x), lo.y.min(c.y), lo.z.min(c.z)),
            CellCoord::new(hi.x.max(c.x), hi.y.max(c.y), hi.z.max(c.z)),
        )
    }))
}

/// `index`, or the selected zone, checked against the track.
fn target(st: &EditState, index: Option<u32>, op: &str) -> Result<u32, String> {
    let i = index
        .or(st.editor.nudge)
        .ok_or_else(|| format!("{op}: no nudge zone selected"))?;
    if (i as usize) < st.editor.track().nudge_zones.len() {
        Ok(i)
    } else {
        Err(format!("{op}: no nudge zone {i}"))
    }
}

/// `nudgeAdd`: a zone over `cells`, else over the selection, pointing at `yaw_mdeg` (0, +X, by
/// default) with the default speed and acceleration. It becomes the selection.
pub fn add(
    st: &mut EditState,
    ctx: &mut Ctx,
    cells: Option<[[i32; 3]; 2]>,
    yaw_mdeg: Option<i32>,
) -> Result<(), String> {
    let (lo, hi) = match cells {
        Some([a, b]) => (super::arr_cell(a), super::arr_cell(b)),
        None => selection_cells(st).ok_or("nudgeAdd: select pieces or give cells")?,
    };
    let zone = NudgeZone::over_cells(lo, hi, wrap_yaw(yaw_mdeg.unwrap_or(0)));
    run_command(st, ctx, "nudgeAdd", EditCommand::AddNudge(zone))?;
    st.editor.selection.clear();
    Ok(())
}

/// `nudgeSelect`: select zone `index` (clearing the placement selection), or none.
pub fn select(st: &mut EditState, index: Option<u32>) -> Result<(), String> {
    if let Some(i) = index {
        target(st, Some(i), "nudgeSelect")?;
        st.editor.selection.clear();
    }
    st.editor.nudge = index;
    Ok(())
}

/// `nudgeSet`: replace zone `index` (the selected one by default) with `zone`.
pub fn set(
    st: &mut EditState,
    ctx: &mut Ctx,
    index: Option<u32>,
    zone: NudgeZone,
) -> Result<(), String> {
    let index = target(st, index, "nudgeSet")?;
    run_command(st, ctx, "nudgeSet", EditCommand::SetNudge { index, zone })?;
    st.editor.nudge = Some(index);
    Ok(())
}

/// `nudgeRemove`: delete zone `index` (the selected one by default).
pub fn remove(st: &mut EditState, ctx: &mut Ctx, index: Option<u32>) -> Result<(), String> {
    let index = target(st, index, "nudgeRemove")?;
    run_command(st, ctx, "nudgeRemove", EditCommand::RemoveNudge(index))
}

/// The selected zone, when it is the whole selection (no pieces selected).
pub fn sole(st: &EditState) -> Option<u32> {
    st.editor
        .nudge
        .filter(|_| st.editor.selection.is_empty())
        .filter(|&i| (i as usize) < st.editor.track().nudge_zones.len())
}

/// `move` on the selected zone: shift its box by `offset` whole cells.
pub fn shift(
    st: &mut EditState,
    ctx: &mut Ctx,
    index: u32,
    offset: CellCoord,
) -> Result<(), String> {
    let mut z = st.editor.track().nudge_zones[index as usize].clone();
    let by =
        [offset.x, offset.y, offset.z].map(|v| v.saturating_mul(parble_core::contract::CELL_UM));
    for (i, d) in by.into_iter().enumerate() {
        z.min_um[i] = z.min_um[i].checked_add(d).ok_or("move: out of range")?;
        z.max_um[i] = z.max_um[i].checked_add(d).ok_or("move: out of range")?;
    }
    set(st, ctx, Some(index), z)
}

/// `rotate` on the selected zone: turn its yaw `by` quarter turns, as a placement turns.
pub fn turn(st: &mut EditState, ctx: &mut Ctx, index: u32, by: u8) -> Result<(), String> {
    let mut z = st.editor.track().nudge_zones[index as usize].clone();
    z.yaw_mdeg = wrap_yaw(z.yaw_mdeg + i32::from(by % 4) * QUARTER_MDEG);
    set(st, ctx, Some(index), z)
}

/// The zone a ray (world metres) enters first, if any: a slab test against each box.
pub fn pick(st: &EditState, ray: Ray3d) -> Option<u32> {
    let (o, d) = (ray.origin, *ray.direction);
    let mut best: Option<(f32, u32)> = None;
    for (i, z) in (0u32..).zip(&st.editor.track().nudge_zones) {
        let (lo, hi) = box_m(z);
        let (mut t0, mut t1) = (0.0f32, f32::INFINITY);
        let mut hit = true;
        for a in 0..3 {
            if d[a].abs() < 1e-9 {
                if o[a] < lo[a] || o[a] > hi[a] {
                    hit = false;
                    break;
                }
                continue;
            }
            let (mut ta, mut tb) = ((lo[a] - o[a]) / d[a], (hi[a] - o[a]) / d[a]);
            if ta > tb {
                std::mem::swap(&mut ta, &mut tb);
            }
            t0 = t0.max(ta);
            t1 = t1.min(tb);
            if t0 > t1 {
                hit = false;
                break;
            }
        }
        if hit && best.is_none_or(|(t, _)| t0 < t) {
            best = Some((t0, i));
        }
    }
    best.map(|(_, i)| i)
}

/// `editor.nudges` with every listed zone projected by `project`.
pub fn diag(st: &EditState, project: impl Fn(Vec3) -> Option<Vec2>) -> NudgeDiag {
    let zones = &st.editor.track().nudge_zones;
    NudgeDiag {
        count: zones.len(),
        selected: st.editor.nudge.filter(|&i| (i as usize) < zones.len()),
        zones: (0u32..)
            .zip(zones)
            .take(MAX_LISTED_NUDGES)
            .map(|(index, z)| {
                let (tail, head) = arrow_m(z);
                NudgeZoneDiag {
                    index,
                    name: z.name.clone(),
                    mode: z.mode,
                    min_um: z.min_um,
                    max_um: z.max_um,
                    yaw_mdeg: z.yaw_mdeg,
                    pitch_mdeg: z.pitch_mdeg,
                    min_speed_um_s: z.min_speed_um_s,
                    max_accel_um_s2: z.max_accel_um_s2,
                    cell: cell_arr(centre_cell(z)),
                    outline: project_box(box_m(z), &project),
                    arrow: project(tail)
                        .zip(project(head))
                        .map(|(a, b)| [[a.x, a.y], [b.x, b.y]]),
                }
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(yaw: i32) -> NudgeZone {
        NudgeZone::over_cells(CellCoord::new(1, 1, -6), CellCoord::new(4, 1, -6), yaw)
    }

    #[test]
    fn the_arrow_runs_through_the_centre_along_the_direction() {
        let (t, h) = arrow_m(&zone(0));
        // The box spans x 0.03..0.27 m: a 0.24 m chord along +X.
        assert!((h.x - t.x - 0.24 * ARROW_SHARE).abs() < 1e-5, "{t} {h}");
        assert!((t.y - 0.09).abs() < 1e-6 && (t.z + 0.36).abs() < 1e-6);
        let (t, h) = arrow_m(&zone(90_000));
        // Along −Z the chord is the box's 0.06 m depth.
        assert!((t.z - h.z - 0.06 * ARROW_SHARE).abs() < 1e-5, "{t} {h}");
        let odd = NudgeZone::over_cells(CellCoord::new(1, 1, -6), CellCoord::new(3, 1, -6), 0);
        assert_eq!(centre_cell(&odd), CellCoord::new(2, 1, -6));
    }

    #[test]
    fn a_ray_picks_the_zone_it_enters_first() {
        let mut st = EditState::new();
        let mut t = parble_core::Track::new(0);
        t.nudge_zones = vec![zone(0), {
            let mut z = zone(0);
            z.min_um[1] += 120_000;
            z.max_um[1] += 120_000;
            z
        }];
        st.editor = parble_core::Editor::new(t);
        let down = |x: f32| Ray3d::new(Vec3::new(x, 1.0, -0.36), Dir3::NEG_Y);
        assert_eq!(pick(&st, down(0.12)), Some(1), "the upper box first");
        assert_eq!(pick(&st, down(0.5)), None);
        let side = Ray3d::new(Vec3::new(-1.0, 0.09, -0.36), Dir3::X);
        assert_eq!(pick(&st, side), Some(0));
    }

    #[test]
    fn the_diag_reports_each_zone_mode() {
        let mut st = EditState::new();
        let mut t = parble_core::Track::new(0);
        t.nudge_zones = vec![
            zone(0),
            NudgeZone {
                mode: NudgeMode::Constant,
                ..zone(0)
            },
        ];
        st.editor = parble_core::Editor::new(t);
        let v = serde_json::to_value(diag(&st, |_| None)).unwrap();
        assert_eq!(v["zones"][0]["mode"], "top_up");
        assert_eq!(v["zones"][1]["mode"], "constant");
    }
}
