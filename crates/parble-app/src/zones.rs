//! Camera zones for the director: the loaded track's authored zones and its pieces' default camera
//! hints (`parble_core::camera_zone`), converted to the renderer's metre units, and the points of
//! interest (mechanism centres) the follow shot's action window takes in. Without a loaded track
//! the scenario's own track is used: `allpieces` carries one authored demo zone ([`allpieces_demo`]).
//!
//! Read-only on the track model: nothing here reaches the sim, so no hash changes.

use bevy::prelude::*;
use parble_core::camera_zone::{self, CameraZone, FocusOn, ShotRule, ShotTarget, ZoneVolume};
use parble_core::contract::{CELL_UM, HALF_CELL_UM};
use parble_core::{PieceCatalog, PortFace, PortFlow, Track};
use parble_render::camera::{CameraInterest, CameraObstacles, CameraZones};
use parble_render::director::{Focus, HiddenPath, Rule, Solid, Target, Volume, Zone, ZoneSource};
use std::collections::{BTreeMap, BTreeSet};

use crate::SimClock;
use crate::agent::Scenario;

fn m(um: i32) -> f32 {
    um as f32 * 1.0e-6
}

fn m3(um: [i32; 3]) -> [f32; 3] {
    um.map(m)
}

fn rad(mdeg: i32) -> f32 {
    (mdeg as f32 * 1.0e-3).to_radians()
}

fn focus(f: FocusOn) -> Focus {
    match f {
        FocusOn::Marble => Focus::Marble,
        FocusOn::Target => Focus::Target,
    }
}

/// A track-model rule in renderer units.
pub fn rule(r: &ShotRule) -> Rule {
    let fov = |f: Option<u32>| f.map(|f| rad(f as i32));
    match *r {
        ShotRule::Follow {
            back_um,
            up_um,
            ahead_um,
        } => Rule::Follow {
            back: m(back_um),
            up: m(up_um),
            ahead: m(ahead_um),
        },
        ShotRule::Frame {
            target,
            focus: f,
            margin_permille,
            elevation_mdeg,
            azimuth_mdeg,
            fov_mdeg,
        } => Rule::Frame {
            target: match target {
                ShotTarget::Marble {} => Target::Marble,
                ShotTarget::Point { at_um } => Target::Point { at: m3(at_um) },
                ShotTarget::Axis { base_um, height_um } => {
                    let base = m3(base_um);
                    Target::Axis {
                        base,
                        top: [base[0], base[1] + m(height_um), base[2]],
                    }
                }
            },
            focus: focus(f),
            margin: margin_permille as f32 / 1000.0,
            elevation: rad(elevation_mdeg),
            azimuth: azimuth_mdeg.map(rad),
            fov: fov(fov_mdeg),
        },
        ShotRule::Orbit {
            center_um,
            radius_um,
            up_um,
            speed_mdeg_s,
            focus: f,
        } => Rule::Orbit {
            center: m3(center_um),
            radius: m(radius_um),
            up: m(up_um),
            speed: rad(speed_mdeg_s),
            focus: focus(f),
        },
        ShotRule::Fixed {
            eye_um,
            look_um,
            focus: f,
            fov_mdeg,
        } => Rule::Fixed {
            eye: m3(eye_um),
            look: m3(look_um),
            focus: focus(f),
            fov: fov(fov_mdeg),
        },
    }
}

/// A track-model zone in renderer units.
pub fn zone(z: &CameraZone, source: ZoneSource) -> Zone {
    Zone {
        name: z.name.clone(),
        volume: match z.volume {
            ZoneVolume::Box { min_um, max_um } => Volume::Box {
                min: m3(min_um),
                max: m3(max_um),
            },
            ZoneVolume::Cylinder {
                base_um,
                height_um,
                radius_um,
            } => Volume::Cylinder {
                base: m3(base_um),
                height: m(height_um),
                radius: m(radius_um),
            },
        },
        rule: rule(&z.shot),
        priority: z.priority,
        blend_in_s: z.blend_in_ms as f32 / 1000.0,
        blend_out_s: z.blend_out_ms as f32 / 1000.0,
        hold_s: z.hold_ms as f32 / 1000.0,
        source,
    }
}

/// The director's zones for `track`: its authored zones first, then its pieces' hints.
pub fn zones_for(track: &Track) -> Vec<Zone> {
    let hints = camera_zone::hint_zones(track, parble_kit::builtin());
    track
        .camera_zones
        .iter()
        .map(|z| zone(z, ZoneSource::Track))
        .chain(hints.iter().map(|z| zone(z, ZoneSource::Piece)))
        .collect()
}

/// The centres of the track's mechanism placements' anchor cells (metres).
pub fn interest_for(track: &Track) -> Vec<Vec3> {
    track
        .placements
        .iter()
        .filter(|p| p.piece.family() == "mech")
        .map(|p| {
            let a = p.anchor;
            Vec3::new(
                m(a.x * CELL_UM),
                m(a.y * CELL_UM + HALF_CELL_UM),
                m(a.z * CELL_UM),
            )
        })
        .collect()
}

/// Pieces that hide the marble on its way through: drop shafts, redirects (a drop that turns),
/// the tower socket and the one-way hole.
pub fn hides_marble(id: &str) -> bool {
    [
        "blk_drop_",
        "blk_redirect_",
        "blk_tower_",
        "mech_hole_oneway_",
    ]
    .iter()
    .any(|p| id.starts_with(p))
}

/// Inset of a solid's box from its cells (metres), so a marble in a groove at a block's top edge
/// does not count as hidden by the block beside it.
const SOLID_INSET_M: f32 = 0.004;

/// The track's hidden paths and solid columns, for the follow shot.
///
/// A hidden path is a placement of a [`hides_marble`] piece: its cells' box, and its exit, the
/// world position of its `Out` port (the last port when none is `Out`) on the port's face, at the
/// port's channel floor for a side port. Solids are the cells of every vertical run of two or more
/// occupied cells in one cell column (a tower, a pillar stack, a block over a rail), one box per
/// cell: low track the marble rolls over never counts, so only tall pieces turn the camera, and a
/// marble running under a block (the rail joint below `blk_straight_l2` on `allpieces`) is hidden
/// by the block's own cell, not excused by sharing its column's box.
pub fn obstacles_for(track: &Track) -> CameraObstacles {
    let catalog = parble_kit::builtin();
    let half = m(HALF_CELL_UM);
    let cell = m(CELL_UM);
    let mut hidden = Vec::new();
    let mut columns: BTreeMap<(i32, i32), BTreeSet<i32>> = BTreeMap::new();
    for p in &track.placements {
        let Some(def) = catalog.get(&p.piece) else {
            continue;
        };
        let cells = p.world_cells(def);
        for c in &cells {
            columns.entry((c.x, c.z)).or_default().insert(c.y);
        }
        if !hides_marble(p.piece.as_str()) || cells.is_empty() {
            continue;
        }
        let def = def.transformed(p.orientation());
        let Some(port) = def
            .ports
            .iter()
            .find(|q| q.flow == PortFlow::Out)
            .or(def.ports.last())
        else {
            continue;
        };
        let c = port.cell + p.anchor;
        let (cx, cz) = (m(c.x * CELL_UM), m(c.z * CELL_UM));
        let floor = m(c.y * CELL_UM);
        let n = port.face.normal();
        let exit = match port.face {
            PortFace::Bottom => Vec3::new(cx, floor, cz),
            PortFace::Top => Vec3::new(cx, floor + cell, cz),
            _ => Vec3::new(
                cx + n.x as f32 * half,
                floor + port.level.map_or(0.0, |l| m(l.floor_um())) + 0.008,
                cz + n.z as f32 * half,
            ),
        };
        let (lo, hi) = cells
            .iter()
            .fold((cells[0], cells[0]), |(lo, hi), c| (lo.min(*c), hi.max(*c)));
        hidden.push(HiddenPath {
            min: Vec3::new(
                m(lo.x * CELL_UM) - half,
                m(lo.y * CELL_UM),
                m(lo.z * CELL_UM) - half,
            ),
            max: Vec3::new(
                m(hi.x * CELL_UM) + half,
                m((hi.y + 1) * CELL_UM),
                m(hi.z * CELL_UM) + half,
            ),
            exit,
        });
    }
    let mut solids = Vec::new();
    for ((x, z), ys) in &columns {
        let ys: Vec<i32> = ys.iter().copied().collect();
        let mut i = 0;
        while i < ys.len() {
            let mut j = i;
            while j + 1 < ys.len() && ys[j + 1] == ys[j] + 1 {
                j += 1;
            }
            if j > i {
                let (cx, cz) = (m(x * CELL_UM), m(z * CELL_UM));
                let inset = half - SOLID_INSET_M;
                for y in &ys[i..=j] {
                    solids.push(Solid {
                        min: Vec3::new(cx - inset, m(y * CELL_UM), cz - inset),
                        max: Vec3::new(
                            cx + inset,
                            m((y + 1) * CELL_UM) - SOLID_INSET_M,
                            cz + inset,
                        ),
                    });
                }
            }
            i = j + 1;
        }
    }
    CameraObstacles { hidden, solids }
}

/// The all-pieces track's vertical drop: the marble leaves the L2 run through `blk_drop_l2` at
/// cell (24, 3, −7) and falls into the tower socket below it. The zone is a cylinder round that
/// column; its shot pulls back and frames the column's axis, from the tower socket's floor to
/// half a cell above the drop, with the marble, focused on the axis. It holds 0.8 s after the
/// marble leaves the column and blends out over 1.4 s, so the pull-back reads before the camera
/// returns to the follow shot along the rail joint.
///
/// The shot looks from the +z side at a set bearing. The marble leaves the socket onto the rail
/// joint (23, 2, −7) under `blk_straight_l2`, and runs on under `blk_curve90_l2`: from above or
/// along the run (−x) those blocks and the rail cells hide it, so only a side view sees it there.
pub fn allpieces_demo() -> CameraZone {
    let (x, z) = (24 * CELL_UM, -7 * CELL_UM);
    let base_um = [x, 2 * CELL_UM, z];
    CameraZone {
        name: "tower drop".into(),
        volume: ZoneVolume::Cylinder {
            base_um,
            height_um: 2 * CELL_UM,
            radius_um: 90_000,
        },
        shot: ShotRule::Frame {
            target: ShotTarget::Axis {
                base_um,
                height_um: 2 * CELL_UM + HALF_CELL_UM,
            },
            focus: FocusOn::Target,
            margin_permille: 1_500,
            elevation_mdeg: 30_000,
            azimuth_mdeg: Some(0),
            fov_mdeg: None,
        },
        priority: 10,
        blend_in_ms: 800,
        blend_out_ms: 1_800,
        hold_ms: 800,
    }
}

/// The scenario's track as the director sees it, when the scenario is a kit track.
pub fn scenario_track(scenario: Scenario) -> Option<Track> {
    match scenario {
        Scenario::AllPieces | Scenario::Showcase => {
            let mut t = parble_sim::allpieces::track();
            t.camera_zones.push(allpieces_demo());
            Some(t)
        }
        _ => None,
    }
}

/// Refreshes [`CameraZones`] and [`CameraInterest`] when the loaded track changes.
pub fn publish_zones(
    clock: Res<SimClock>,
    mut zones: ResMut<CameraZones>,
    mut interest: ResMut<CameraInterest>,
    mut obstacles: ResMut<CameraObstacles>,
    mut seen: Local<Option<(u64, bool)>>,
) {
    let key = (clock.restarts, clock.tracks.loaded.is_some());
    if *seen == Some(key) {
        return;
    }
    *seen = Some(key);
    let track = match &clock.tracks.loaded {
        Some(f) => Some(f.track.clone()),
        None => scenario_track(clock.scenario),
    };
    let (z, i, o) = track.map_or_else(
        || (Vec::new(), Vec::new(), CameraObstacles::default()),
        |t| (zones_for(&t), interest_for(&t), obstacles_for(&t)),
    );
    zones.0 = z;
    interest.0 = i;
    *obstacles = o;
}

#[cfg(test)]
mod tests {
    use super::*;
    use parble_render::director::{Director, Frame, MarbleIn};

    /// The all-pieces hints: three redirects and the tower socket frame their axis, the
    /// checkpoint gives a beat, and the demo zone comes first.
    #[test]
    fn allpieces_has_the_demo_zone_and_its_piece_hints() {
        let t = scenario_track(Scenario::AllPieces).unwrap();
        let zones = zones_for(&t);
        assert_eq!(zones[0].name, "tower drop");
        assert_eq!(zones[0].source, ZoneSource::Track);
        let hints: Vec<&str> = zones[1..].iter().map(|z| z.name.as_str()).collect();
        assert_eq!(hints.len(), 5, "{hints:?}");
        assert_eq!(
            hints
                .iter()
                .filter(|n| n.starts_with("blk_redirect"))
                .count(),
            3
        );
        assert!(hints.iter().any(|n| n.starts_with("mech_checkpoint_l1")));
        assert!(hints.iter().any(|n| n.starts_with("blk_tower_socket_l1")));
        for z in &t.camera_zones {
            assert_eq!(z.check(), Ok(()));
        }
        assert!(interest_for(&t).len() >= 8);
    }

    /// The sim's own marble positions, read only: marble 0 enters the demo zone on its way down
    /// and leaves it again, and the director plays the zone and then returns to the cue.
    #[test]
    fn the_first_marble_drops_through_the_demo_zone() {
        let t = scenario_track(Scenario::AllPieces).unwrap();
        let zones = zones_for(&t);
        let o = obstacles_for(&t);
        let interest = interest_for(&t);
        let mut run = parble_sim::allpieces::run().unwrap();
        let mut d = Director::default();
        let (mut entered, mut left) = (None, None);
        let mut was_zone = false;
        for _ in 0..(parble_sim::allpieces::RUN_TICKS / 4) {
            run.tick(4);
            let marbles: Vec<MarbleIn> = run
                .marbles()
                .iter()
                .map(|s| MarbleIn {
                    p: Vec3::from_array(s.position),
                    v: Vec3::from_array(s.linvel),
                })
                .collect();
            d.update(
                &Frame {
                    marbles: &marbles,
                    dt: 1.0 / 60.0,
                    aspect: 16.0 / 9.0,
                    run: 0,
                    interest: &interest,
                    hidden: &o.hidden,
                    solids: &o.solids,
                },
                Some((0, Rule::FOLLOW)),
                &zones,
            );
            let in_demo = d.diag.zone == Some(0);
            if in_demo && !was_zone && entered.is_none() {
                entered = Some(run.tick_count());
            }
            if !in_demo && was_zone && left.is_none() {
                left = Some(run.tick_count());
            }
            was_zone = in_demo;
            if left.is_some() {
                break;
            }
        }
        let (entered, left) = (entered.expect("entered"), left.expect("left"));
        println!("demo zone: marble 0 enters at tick {entered}, the zone ends at tick {left}");
        assert!(left > entered + 240, "the beat lasts over a second");
        assert_eq!(d.diag.followed, Some(0));
    }

    /// The all-pieces track's hidden paths and solids: the drop, the redirects and the tower socket
    /// hide the marble, and the tall columns (the tower, the pillars) are solids.
    #[test]
    fn allpieces_obstacles_hide_the_drops_and_block_the_view_at_the_tower() {
        let t = scenario_track(Scenario::AllPieces).unwrap();
        let o = obstacles_for(&t);
        assert_eq!(o.hidden.len(), 6, "{:?}", o.hidden);
        assert!(!o.solids.is_empty());
        for h in &o.hidden {
            assert!(h.min.cmplt(h.max).all());
            // The exit sits on the piece's box.
            let grown = HiddenPath {
                min: h.min - Vec3::splat(1e-4),
                max: h.max + Vec3::splat(1e-4),
                exit: h.exit,
            };
            assert!(grown.contains(h.exit), "{h:?}");
        }
    }

    /// The whole all-pieces run, read from the sim: outside hidden paths the camera sees the
    /// followed marble past the track's solids in more frames than without avoidance, and in at
    /// least 95% of them.
    #[test]
    fn the_allpieces_run_keeps_the_marble_in_sight() {
        let t = scenario_track(Scenario::AllPieces).unwrap();
        let zones = zones_for(&t);
        let o = obstacles_for(&t);
        let interest = interest_for(&t);
        let measure = |avoid: bool| {
            let mut run = parble_sim::allpieces::run().unwrap();
            let mut d = Director::default();
            let (mut frames, mut clear) = (0u32, 0u32);
            for _ in 0..(parble_sim::allpieces::RUN_TICKS / 4) {
                run.tick(4);
                let marbles: Vec<MarbleIn> = run
                    .marbles()
                    .iter()
                    .map(|s| MarbleIn {
                        p: Vec3::from_array(s.position),
                        v: Vec3::from_array(s.linvel),
                    })
                    .collect();
                let Some(pose) = d.update(
                    &Frame {
                        marbles: &marbles,
                        dt: 1.0 / 60.0,
                        aspect: 16.0 / 9.0,
                        run: 0,
                        interest: &interest,
                        hidden: &o.hidden,
                        solids: if avoid { &o.solids } else { &[] },
                    },
                    Some((0, Rule::FOLLOW)),
                    &zones,
                ) else {
                    continue;
                };
                let Some(m) = d.diag.followed.and_then(|i| marbles.get(i)) else {
                    continue;
                };
                if o.hidden.iter().any(|h| h.contains(m.p)) {
                    continue;
                }
                frames += 1;
                if parble_render::director::line_of_sight(pose.eye, m.p, &o.solids) {
                    clear += 1;
                }
            }
            clear as f32 / frames.max(1) as f32
        };
        let (plain, share) = (measure(false), measure(true));
        println!(
            "allpieces: clear line of sight in {share:.3} of frames ({plain:.3} without avoidance)"
        );
        assert!(share > plain, "avoidance helps: {share} vs {plain}");
        assert!(share >= 0.95, "clear in {share} of frames");
    }

    /// The tower drop zone, every pass of every marble: on each frame it plays, outside hidden
    /// paths, the followed marble projects inside the frame (its hold lets the marble go rather
    /// than lose it), and no camera view looks down more steeply than 60°. The one exception is
    /// the second after the director moves on to a new marble that is already at the tower: that
    /// blend starts from the old marble's view, so the new one comes into frame during it.
    #[test]
    fn the_tower_zone_keeps_the_marble_in_frame() {
        let t = scenario_track(Scenario::AllPieces).unwrap();
        let zones = zones_for(&t);
        let o = obstacles_for(&t);
        let interest = interest_for(&t);
        let aspect = 16.0 / 9.0;
        let mut run = parble_sim::allpieces::run().unwrap();
        let mut d = Director::default();
        let (mut frames, mut steepest) = (0u32, 0f32);
        let (mut followed, mut since_retarget) = (None, u32::MAX);
        for _ in 0..(parble_sim::allpieces::RUN_TICKS / 4) {
            run.tick(4);
            let marbles: Vec<MarbleIn> = run
                .marbles()
                .iter()
                .map(|s| MarbleIn {
                    p: Vec3::from_array(s.position),
                    v: Vec3::from_array(s.linvel),
                })
                .collect();
            let Some(pose) = d.update(
                &Frame {
                    marbles: &marbles,
                    dt: 1.0 / 60.0,
                    aspect,
                    run: 0,
                    interest: &interest,
                    hidden: &o.hidden,
                    solids: &o.solids,
                },
                Some((0, Rule::FOLLOW)),
                &zones,
            ) else {
                continue;
            };
            steepest = steepest.max((-pose.forward.y).asin());
            since_retarget = if d.diag.followed != followed && followed.is_some() {
                0
            } else {
                since_retarget.saturating_add(1)
            };
            followed = d.diag.followed;
            if d.diag.zone != Some(0) || since_retarget < 60 {
                continue;
            }
            let Some(m) = d.diag.followed.and_then(|i| marbles.get(i)) else {
                continue;
            };
            if o.hidden.iter().any(|h| h.contains(m.p)) {
                continue;
            }
            frames += 1;
            assert!(
                parble_render::director::in_frame(&pose, aspect, m.p, 1.0),
                "tick {}: marble {} out of frame",
                run.tick_count(),
                m.p
            );
        }
        println!(
            "tower zone: {frames} frames with the marble in frame; steepest view {:.1}°",
            steepest.to_degrees()
        );
        assert!(frames > 30, "the zone plays with the marble in sight");
        assert!(steepest <= std::f32::consts::FRAC_PI_3 + 0.01, "{steepest}");
    }

    /// The showcase director stays calm: in the 30 s all-pieces run it changes shot at most 12
    /// times (22 before piece hints waited [`parble_render::director::MIN_SHOT_S`]) and the view never turns
    /// faster than 200 °/s from one frame to the next (a retarget once flipped 32° in a frame).
    #[test]
    fn the_allpieces_director_changes_shot_rarely_and_turns_smoothly() {
        assert_eq!(
            parble_render::director::HINT_PRIORITY,
            camera_zone::HINT_PRIORITY
        );
        let t = scenario_track(Scenario::AllPieces).unwrap();
        let zones = zones_for(&t);
        let o = obstacles_for(&t);
        let interest = interest_for(&t);
        let mut run = parble_sim::allpieces::run().unwrap();
        let mut d = Director::default();
        let mut prev: Option<parble_render::director::Pose> = None;
        let mut max_turn = 0.0_f32;
        for _ in 0..(parble_sim::allpieces::RUN_TICKS / 4) {
            run.tick(4);
            let marbles: Vec<MarbleIn> = run
                .marbles()
                .iter()
                .map(|s| MarbleIn {
                    p: Vec3::from_array(s.position),
                    v: Vec3::from_array(s.linvel),
                })
                .collect();
            let frame = Frame {
                marbles: &marbles,
                dt: 1.0 / 60.0,
                aspect: 16.0 / 9.0,
                run: 0,
                interest: &interest,
                hidden: &o.hidden,
                solids: &o.solids,
            };
            let Some(pose) = d.update(&frame, Some((0, Rule::FOLLOW)), &zones) else {
                continue;
            };
            if let Some(p) = prev {
                max_turn = max_turn.max(p.forward.angle_between(pose.forward).to_degrees() * 60.0);
            }
            prev = Some(pose);
        }
        println!(
            "allpieces director: {} shot changes, peak turn {max_turn:.0} deg/s",
            d.diag.blends
        );
        assert!(d.diag.blends <= 12, "{} shot changes", d.diag.blends);
        assert!(max_turn < 200.0, "turned {max_turn} deg/s in one frame");
    }
}
