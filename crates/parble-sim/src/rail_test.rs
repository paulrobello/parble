//! P2-T4 rail family: a cat's eye marble runs block → rail → S-rail → block at `L2` through the
//! real API (chunk merge, simplification, contact hooks), and the seam bump metrics of
//! `seam_test` are gated at every seam against the seamless `L2` block channel.

use crate::chunk::{ChunkMesh, placed_triangles};
use crate::marble::{CATS_EYE, MarbleKind, MarbleSize, MarbleSpec};
use crate::seam_test::{kick, upward};
use crate::{MarbleState, Sim};
use parble_core::{CellCoord, PieceId, Placement, Yaw4};
use rapier3d::prelude::*;

/// Launch speed along the channel, m/s (as `seam_test`).
const SPEED: f32 = 0.3;
/// Centre height of a standard marble resting on an `L2` floor (30 mm) in cell layer 0.
const REST_Y: f32 = 0.030 + 0.008;
/// `seam_test::SEAM_SPIKE_MM_S`: allowed peak |vy| (upward at a seam into a curve) above the
/// seamless channel's.
const SEAM_SPIKE_MM_S: f32 = 2.0;
/// `seam_test::CURVE_KICK_MARGIN_MM_S`: allowed kick above the seamless channel's.
const KICK_MARGIN_MM_S: f32 = 5.0;
const TICKS: u32 = 1_100;

fn place(piece: &str, x: i32, y: i32, z: i32) -> Placement {
    Placement::new(
        PieceId::new(piece).unwrap(),
        CellCoord::new(x, y, z),
        Yaw4::new(0),
        false.into(),
    )
}

fn launch(sim: &mut Sim, at: Vector, dir: Vector, speed: f32) {
    launch_with(sim, &CATS_EYE, at, dir, speed);
}

fn launch_with(sim: &mut Sim, spec: &MarbleSpec, at: Vector, dir: Vector, speed: f32) {
    let (mut body, collider) = spec.build(at);
    body.set_linvel(dir * speed, true);
    if speed > 0.0 {
        body.set_angvel(Vector::Y.cross(dir) * (speed / spec.radius_m), true);
    }
    let h = sim.world.bodies.insert(body);
    sim.world
        .colliders
        .insert_with_parent(collider, h, &mut sim.world.bodies);
    sim.marbles.push(h);
}

fn trace(sim: &mut Sim, ticks: u32) -> Vec<MarbleState> {
    (0..ticks)
        .map(|_| {
            sim.tick(1);
            sim.marbles()[0]
        })
        .collect()
}

fn track_sim(layout: &[Placement]) -> Sim {
    let kit = parble_kit::builtin();
    let mut sim = Sim::empty();
    for p in layout {
        sim.place_piece(kit, p.clone()).unwrap();
    }
    sim.rebuild_chunks(kit).unwrap();
    sim
}

/// Peak |vy| in mm/s while `sel` holds.
fn peak(tr: &[MarbleState], sel: impl Fn(&MarbleState) -> bool) -> f32 {
    tr.iter()
        .filter(|m| sel(m))
        .map(|m| m.linvel[1].abs())
        .fold(0.0, f32::max)
        * 1_000.0
}

/// `(peak |vy|, kick)` of the seamless reference: `blk_straight_l2` stretched over 20 cells, the
/// window from the first cell face to the last (as `seam_test::in_run`).
fn seamless_l2() -> (f32, f32) {
    let kit = parble_kit::builtin();
    let one = place("blk_straight_l2", 0, 0, 0);
    let tris: Vec<[[i64; 3]; 3]> = placed_triangles(kit.piece(&one.piece).unwrap(), &one)
        .into_iter()
        .map(|t| t.map(|v| [(v[0] + 30_000) * 20 - 30_000, v[1], v[2]]))
        .collect();
    let mut sim = Sim::empty();
    sim.world.integration_parameters.contact_clustering = false;
    sim.world.colliders.insert(
        ChunkMesh::from_triangles(&tris)
            .collider([0, 0, 0])
            .unwrap(),
    );
    launch(&mut sim, Vector::new(-0.02, REST_Y, 0.0), Vector::X, SPEED);
    let tr = trace(&mut sim, TICKS);
    let in_run = |m: &MarbleState| (0.03..1.14).contains(&m.position[0]);
    (peak(&tr, in_run), kick(&tr, in_run))
}

/// Two `L2` straights, two `L2` rail straights, the S-rail (cells x 4..6, z -1..0, offset 60 mm
/// toward -z), then two `L2` straights in row z = -1. The S-rail's anchor is in chunk (0, 0, 0)
/// and the straights after it in chunk (0, 0, -1), so its exit seam is also a chunk boundary.
fn rail_layout() -> Vec<Placement> {
    vec![
        place("blk_straight_l2", 0, 0, 0),
        place("blk_straight_l2", 1, 0, 0),
        place("rail_straight_l2", 2, 0, 0),
        place("rail_straight_l2", 3, 0, 0),
        place("rail_s_1x3_l2", 4, 0, 0),
        place("blk_straight_l2", 7, 0, -1),
        place("blk_straight_l2", 8, 0, -1),
    ]
}

/// A seam window: plan box (x, z) in metres, and whether the marble may be swinging there. A
/// marble on the S-rail's arcs swings smoothly up the outer wall and back and leaves still
/// swinging, so seams on and after the S-rail are gated on upward speed (a bump throws the marble
/// up) and kick, and the seams before it on peak |vy| and kick. `seam_test` gates the slope entry
/// after its curves the same way. Measured 2026-11-07: the block to block seam after the S-rail
/// peaks at 17.13 mm/s |vy| from the swing, with upward 6.91 and kick 17.27.
struct Window {
    name: &'static str,
    lo: [f32; 2],
    hi: [f32; 2],
    swinging: bool,
}

/// 15 mm either side of each seam, and the S-rail's interior between its seams.
const WINDOWS: [Window; 6] = [
    Window {
        name: "block to block (x 30 mm)",
        lo: [0.015, -0.03],
        hi: [0.045, 0.03],
        swinging: false,
    },
    Window {
        name: "block to rail (x 90 mm)",
        lo: [0.075, -0.03],
        hi: [0.105, 0.03],
        swinging: false,
    },
    Window {
        name: "rail to rail (x 150 mm)",
        lo: [0.135, -0.03],
        hi: [0.165, 0.03],
        swinging: false,
    },
    Window {
        name: "rail to S-rail (x 210 mm)",
        lo: [0.195, -0.03],
        hi: [0.225, 0.03],
        swinging: true,
    },
    Window {
        name: "S-rail to block (x 390 mm, chunk boundary)",
        lo: [0.375, -0.09],
        hi: [0.405, -0.03],
        swinging: true,
    },
    Window {
        name: "block to block (x 450 mm)",
        lo: [0.435, -0.09],
        hi: [0.465, -0.03],
        swinging: true,
    },
];

fn at(m: &MarbleState, lo: [f32; 2], hi: [f32; 2]) -> bool {
    (lo[0]..hi[0]).contains(&m.position[0]) && (lo[1]..hi[1]).contains(&m.position[2])
}

/// The S-rail interior, past the entry window and short of the exit window.
fn in_s_rail(m: &MarbleState) -> bool {
    (0.225..0.375).contains(&m.position[0])
}

#[test]
fn block_rail_s_rail_block_seams_stay_under_the_seam_thresholds() {
    let mut sim = track_sim(&rail_layout());
    launch(&mut sim, Vector::new(-0.02, REST_Y, 0.0), Vector::X, SPEED);
    let tr = trace(&mut sim, TICKS);
    let (p_base, k_base) = seamless_l2();
    let (p_limit, k_limit) = (p_base + SEAM_SPIKE_MM_S, k_base + KICK_MARGIN_MM_S);
    eprintln!("seamless L2 channel: peak {p_base:.2} mm/s, kick {k_base:.2} mm/s");
    assert!(
        tr.iter()
            .any(|m| m.position[0] > 0.50 && (m.position[2] + 0.06).abs() < 0.005),
        "marble must reach the last block, centred in row z = -60 mm"
    );
    assert!(
        tr.iter()
            .all(|m| m.position[0] > 0.51 || (m.position[1] - REST_Y).abs() < 0.004),
        "marble stays in the L2 channel"
    );
    for w in WINDOWS {
        let sel = |m: &MarbleState| at(m, w.lo, w.hi);
        assert!(tr.iter().any(sel), "{}: marble never crossed", w.name);
        let (p, u, k) = (peak(&tr, sel), upward(&tr, sel), kick(&tr, sel));
        eprintln!(
            "{}: peak |vy| {p:.2} mm/s, upward {u:.2} (limit {p_limit:.2}), kick {k:.2} \
             (limit {k_limit:.2})",
            w.name
        );
        let gated = if w.swinging { u } else { p };
        assert!(
            gated <= p_limit,
            "{}: {} {gated:.2} mm/s over {p_limit:.2}",
            w.name,
            if w.swinging { "upward" } else { "peak |vy|" }
        );
        assert!(
            k <= k_limit,
            "{}: kick {k:.2} mm/s over {k_limit:.2}",
            w.name
        );
    }
    let (p, k) = (peak(&tr, in_s_rail), kick(&tr, in_s_rail));
    eprintln!("S-rail interior: peak |vy| {p:.2} mm/s (ungated), kick {k:.2} (limit {k_limit:.2})");
    assert!(
        k <= k_limit,
        "S-rail interior: kick {k:.2} mm/s over {k_limit:.2}"
    );
}

#[test]
fn rail_joint_spigot_welds_to_the_tower_socket_channel() {
    // Contract §5.5: the joint's collision floor runs 10 mm past its own face to the socket's
    // port plane, where the tower's channel starts, so the 17 floor vertices of both rings are
    // the same integer µm points and weld in the chunk merge.
    let kit = parble_kit::builtin();
    let joint = place("rail_joint_l1", -1, 0, 0);
    let tower = place("blk_tower_socket_l1", 0, 0, 0);
    let verts = |p: &Placement| -> Vec<[i64; 3]> {
        placed_triangles(kit.piece(&p.piece).unwrap(), p)
            .into_iter()
            .flatten()
            .collect()
    };
    let (j, t) = (verts(&joint), verts(&tower));
    for &(u, dv) in &parble_core::contract::CANONICAL_RING_UM {
        if u.abs() == 14_000 && dv > 14_000 {
            continue; // wall tops: the rail's sit at floor + 18 mm, the tower's bore has none
        }
        let v = [-20_000, 15_000 + i64::from(dv), i64::from(u)];
        assert!(j.contains(&v), "joint lacks floor vertex {v:?}");
        assert!(t.contains(&v), "tower lacks floor vertex {v:?}");
    }
}

#[test]
fn a_marble_dropped_into_the_tower_leaves_by_the_joint_and_rail() {
    // ref-06: a marble dropped into the socket tower rolls out of the socket onto the joint's
    // spigot, along a rail and into a block channel, all at `L1`.
    let layout = vec![
        place("blk_tower_socket_l1", 0, 0, 0),
        place("rail_joint_l1", -1, 0, 0),
        place("rail_straight_l1", -2, 0, 0),
        place("blk_straight_l1", -3, 0, 0),
        place("blk_straight_l1", -4, 0, 0),
    ];
    let mut sim = track_sim(&layout);
    launch(&mut sim, Vector::new(0.0, 0.075, 0.0), Vector::X, 0.0);
    let tr = trace(&mut sim, 1_600);
    let rest = 0.015 + 0.008;
    let out = tr
        .iter()
        .find(|m| m.position[0] < -0.21)
        .expect("marble must reach the last block");
    eprintln!(
        "tower drop: past x = -210 mm at y {:.4} m, z {:.4} m, speed {:.3} m/s",
        out.position[1],
        out.position[2],
        (out.linvel[0].powi(2) + out.linvel[1].powi(2) + out.linvel[2].powi(2)).sqrt()
    );
    assert!((out.position[1] - rest).abs() < 0.004 && out.position[2].abs() < 0.008);
    for (name, x0) in [
        ("joint to rail (x -90 mm)", -0.105),
        ("rail to block (x -150 mm)", -0.165),
    ] {
        let sel = |m: &MarbleState| (x0..x0 + 0.03).contains(&m.position[0]);
        eprintln!(
            "tower drop, {name}: peak |vy| {:.2} mm/s, kick {:.2} mm/s (logged)",
            peak(&tr, sel),
            kick(&tr, sel)
        );
    }
}

#[test]
fn s_rail_without_the_arc_contact_kicks() {
    // Negative control: the same layout merged into one welded, simplified trimesh without the
    // contact hooks kicks over the limit at the S-rail entry and inside it, so the metric detects
    // the facet chatter that the exact arc contact ([`crate::rail_arc::ChannelArc`]) removes.
    let kit = parble_kit::builtin();
    let tris: Vec<[[i64; 3]; 3]> = rail_layout()
        .iter()
        .flat_map(|p| placed_triangles(kit.piece(&p.piece).unwrap(), p))
        .collect();
    let mut mesh = ChunkMesh::from_triangles(&tris);
    mesh.simplify_flat();
    let mut sim = Sim::empty();
    sim.world.integration_parameters.contact_clustering = false;
    sim.world
        .colliders
        .insert(mesh.collider([0, 0, 0]).unwrap());
    launch(&mut sim, Vector::new(-0.02, REST_Y, 0.0), Vector::X, SPEED);
    let tr = trace(&mut sim, TICKS);
    let (_, k_base) = seamless_l2();
    let limit = k_base + KICK_MARGIN_MM_S;
    let w = &WINDOWS[3];
    let (k_entry, k_in) = (kick(&tr, |m| at(m, w.lo, w.hi)), kick(&tr, in_s_rail));
    eprintln!(
        "without the arc contact: S-rail entry kick {k_entry:.2} mm/s, interior {k_in:.2} \
         (limit {limit:.2})"
    );
    assert!(
        k_entry > limit,
        "control entry kick {k_entry:.2} not over {limit:.2}"
    );
    assert!(
        k_in > limit,
        "control interior kick {k_in:.2} not over {limit:.2}"
    );
}

/// The 90° rail curve (2 × 2, radius 90 mm) between `L2` straights, in along +x at x = 90 mm and
/// out along -z at z = -90 mm, x = 180 mm, with `extra` triangles added (a control's step).
fn rail_curve_run(extra: Vec<[[i64; 3]; 3]>) -> Vec<MarbleState> {
    let turned = |x: i32, z: i32| {
        Placement::new(
            PieceId::new("blk_straight_l2").unwrap(),
            CellCoord::new(x, 0, z),
            Yaw4::new(1),
            false.into(),
        )
    };
    let layout = vec![
        place("blk_straight_l2", 0, 0, 0),
        place("blk_straight_l2", 1, 0, 0),
        place("rail_curve90_r90_l2", 2, 0, 0),
        turned(3, -2),
        turned(3, -3),
    ];
    let kit = parble_kit::builtin();
    let mut sim = Sim::empty();
    sim.pieces.extra_triangles = extra;
    for p in &layout {
        sim.place_piece(kit, p.clone()).unwrap();
    }
    sim.rebuild_chunks(kit).unwrap();
    launch(&mut sim, Vector::new(-0.02, REST_Y, 0.0), Vector::X, SPEED);
    trace(&mut sim, TICKS)
}

/// The rail curve's exit window into the block (z -90 mm).
const RAIL_CURVE_EXIT: ([f32; 2], [f32; 2]) = ([0.15, -0.105], [0.21, -0.075]);

#[test]
fn block_rail_curve_block_seams_stay_under_the_seam_thresholds() {
    // Gated like the S-rail's seams (upward and kick), except the two hand-overs between the
    // block channel and the rails and the curve's interior, gated on kick only as a bend inlet is
    // (`joiner_seams_stay_under_the_seam_thresholds`). On the way in the marble swings up the
    // curve's outer wall (upward 3.54 at the seam and 4.41 inside). Measured 2026-11-08, the old
    // solver against the vendored Rapier contact fix (PHYSICS_VERSION 3): both trajectories rise
    // the same 0.19 mm over x 92-108 mm (37.98 to 38.17 mm old, 38.00 to 38.20 mm new) as z swings
    // out. The old reading stayed negative under the rolling sink's -3 to -10 mm/s offset. On the
    // way out the mirror image: the marble rides about 1 mm outward and 0.1 mm high on the rails,
    // comes back down at 2.4 mm/s past the seam and the block channel takes it 6 mm in (upward
    // 4.04, kick 4.18, measured 2026-11-08; 4.57 / 5.10 with the solved rolling-resistance load). A real step there still fails the kick
    // (`rail_curve_exit_step_kicks`).
    let tr = rail_curve_run(Vec::new());
    let (p_base, k_base) = seamless_l2();
    let (p_limit, k_limit) = (p_base + SEAM_SPIKE_MM_S, k_base + KICK_MARGIN_MM_S);
    assert!(
        tr.iter()
            .any(|m| m.position[2] < -0.20 && (m.position[0] - 0.18).abs() < 0.005),
        "marble must leave the curve along -z, centred on x = 180 mm"
    );
    let windows: [(&str, [f32; 2], [f32; 2]); 4] = [
        (
            "block to rail curve (x 90 mm)",
            [0.075, -0.03],
            [0.105, 0.03],
        ),
        ("rail curve interior", [0.105, -0.075], [0.21, 0.03]),
        (
            "rail curve to block (z -90 mm)",
            RAIL_CURVE_EXIT.0,
            RAIL_CURVE_EXIT.1,
        ),
        ("block to block (z -150 mm)", [0.15, -0.165], [0.21, -0.135]),
    ];
    for (name, lo, hi) in windows {
        let sel = |m: &MarbleState| at(m, lo, hi);
        assert!(tr.iter().any(sel), "{name}: marble never crossed");
        let (p, u, k) = (peak(&tr, sel), upward(&tr, sel), kick(&tr, sel));
        let swing = name != "block to block (z -150 mm)";
        eprintln!(
            "{name}: peak |vy| {p:.2} mm/s, upward {u:.2} (limit {p_limit:.2}{}), kick {k:.2} \
             (limit {k_limit:.2})",
            if swing { ", swing, ungated" } else { "" }
        );
        assert!(
            swing || u <= p_limit,
            "{name}: upward {u:.2} mm/s over {p_limit:.2}"
        );
        assert!(k <= k_limit, "{name}: kick {k:.2} mm/s over {k_limit:.2}");
    }
}

/// A step up of `rise_um` in the block's floor just past the rail curve's exit (z -90 mm): a flat
/// plate 12 mm wide across the channel bottom at x = 180 mm, from the seam to 10 mm into the
/// block, `rise_um` over the `L2` floor (30 mm), facing up.
fn rail_curve_exit_step(rise_um: i64) -> Vec<[[i64; 3]; 3]> {
    let (x0, x1, y) = (174_000, 186_000, 30_000 + rise_um);
    let (z0, z1) = (-100_000, -90_000);
    vec![
        [[x0, y, z0], [x0, y, z1], [x1, y, z0]],
        [[x1, y, z0], [x0, y, z1], [x1, y, z1]],
    ]
}

#[test]
fn rail_curve_exit_step_kicks() {
    // Negative control for the kick-only exit gate: a 0.5 mm step at the seam still fails it.
    let tr = rail_curve_run(rail_curve_exit_step(500));
    let (_, k_base) = seamless_l2();
    let limit = k_base + KICK_MARGIN_MM_S;
    let (lo, hi) = RAIL_CURVE_EXIT;
    let k = kick(&tr, |m| at(m, lo, hi));
    eprintln!("rail curve to block with a 500 um step: kick {k:.2} (limit {limit:.2})");
    assert!(k > limit, "control kick {k:.2} not over {limit:.2}");
}

#[test]
fn rail_contacts_report_rail_and_block_contacts_wood_with_the_run_unchanged() {
    use crate::mechanism::{ContactOther, SimEvent, SurfaceId};
    use parble_kit::Surface;
    let rail = SurfaceId::Kit(Surface::Rail);
    let wood = SurfaceId::Kit(Surface::Wood);
    let run = |record: bool| {
        let mut sim = track_sim(&rail_layout());
        sim.set_event_recording(record);
        launch(&mut sim, Vector::new(-0.02, REST_Y, 0.0), Vector::X, SPEED);
        let tr = trace(&mut sim, TICKS);
        (tr, sim.drain_events(), sim.trajectory_hash())
    };
    let (tr, events, hash) = run(true);
    // Recording (and the surface lookup it runs) reads the world only.
    assert_eq!(run(false).2, hash);
    // The pieces under the marble's centre: blocks in cells x 0..1 and 7..8, rail straights in
    // 2..3, the S-rail in 4..6 (cell x spans x ± 30 mm).
    let x_at = |tick: u64| tr[tick as usize - 1].position[0];
    let over = |x: f32| match ((x + 0.03) / 0.06).floor() as i32 {
        0 | 1 | 7 | 8 => Some(wood),
        2..=6 => Some(rail),
        _ => None,
    };
    let (mut on_rail, mut on_wood) = (0, 0);
    for e in &events {
        let SimEvent::Roll(r) = e else { continue };
        let x = x_at(r.tick);
        // Away from the seams, where the marble may touch both pieces.
        let mid = (((x + 0.03) / 0.06).fract() - 0.5).abs() < 0.3;
        if let (Some(want), true, Some(got)) = (over(x), mid, r.surface) {
            assert_eq!(got, want, "tick {} at x {x:.4}", r.tick);
            if got == rail {
                on_rail += 1;
            } else {
                on_wood += 1;
            }
        }
    }
    assert!(on_rail > 20, "{on_rail} roll samples on the rails");
    assert!(on_wood > 20, "{on_wood} roll samples on the blocks");
    // A contact event reports the surface where the marble first touches the chunk. The run is one
    // chunk, so its marble starts on the blocks (wood) and keeps touching: it used to report a
    // rail contact only after a hop on the rails, which the exact straight-tube contact removed
    // (`chunk::ChannelStraight`, 2026-11-09). A marble dropped 2 mm onto the rail straight
    // (cell x 2) first touches the rails.
    let first_static = |events: &[SimEvent]| {
        events.iter().find_map(|e| match e {
            SimEvent::Contact(c) if c.other == ContactOther::Static => Some(c.surface),
            _ => None,
        })
    };
    assert_eq!(first_static(&events), Some(wood));
    let mut sim = track_sim(&rail_layout());
    sim.set_event_recording(true);
    launch(
        &mut sim,
        Vector::new(0.12, REST_Y + 0.002, 0.0),
        Vector::X,
        SPEED,
    );
    trace(&mut sim, 60);
    assert_eq!(first_static(&sim.drain_events()), Some(rail));
}

/// Centre height of a standard marble resting on an `L1`, `L2` or `L3` floor in cell layer 0.
const REST_AT: [f32; 4] = [0.0, 0.015 + 0.008, 0.030 + 0.008, 0.045 + 0.008];

/// [`seamless_l2`] at any level: `blk_straight_l<level>` stretched over 20 cells.
fn seamless_at(level: usize) -> (f32, f32) {
    let kit = parble_kit::builtin();
    let one = place(&format!("blk_straight_l{level}"), 0, 0, 0);
    let tris: Vec<[[i64; 3]; 3]> = placed_triangles(kit.piece(&one.piece).unwrap(), &one)
        .into_iter()
        .map(|t| t.map(|v| [(v[0] + 30_000) * 20 - 30_000, v[1], v[2]]))
        .collect();
    let mut sim = Sim::empty();
    sim.world.integration_parameters.contact_clustering = false;
    sim.world.colliders.insert(
        ChunkMesh::from_triangles(&tris)
            .collider([0, 0, 0])
            .unwrap(),
    );
    launch(
        &mut sim,
        Vector::new(-0.02, REST_AT[level], 0.0),
        Vector::X,
        SPEED,
    );
    let tr = trace(&mut sim, TICKS);
    let in_run = |m: &MarbleState| (0.03..1.14).contains(&m.position[0]);
    (peak(&tr, in_run), kick(&tr, in_run))
}

/// [`rail_layout`]'s run with every piece at `level`.
fn rail_layout_at(level: usize) -> Vec<Placement> {
    let at = |s: &str, x, z| place(&format!("{s}_l{level}"), x, 0, z);
    vec![
        at("blk_straight", 0, 0),
        at("blk_straight", 1, 0),
        at("rail_straight", 2, 0),
        at("rail_straight", 3, 0),
        at("rail_s_1x3", 4, 0),
        at("blk_straight", 7, -1),
        at("blk_straight", 8, -1),
    ]
}

/// [`rail_curve_run`]'s layout with every piece at `level`.
fn rail_curve_layout_at(level: usize) -> Vec<Placement> {
    let straight = format!("blk_straight_l{level}");
    let turned = |x: i32, z: i32| {
        Placement::new(
            PieceId::new(&straight).unwrap(),
            CellCoord::new(x, 0, z),
            Yaw4::new(1),
            false.into(),
        )
    };
    vec![
        place(&straight, 0, 0, 0),
        place(&straight, 1, 0, 0),
        place(&format!("rail_curve90_r90_l{level}"), 2, 0, 0),
        turned(3, -2),
        turned(3, -3),
    ]
}

#[test]
fn rails_at_l1_and_l3_keep_their_seams_under_the_thresholds() {
    // The L2 seam gates above, run with every piece of both layouts at L1 and at L3 against the
    // seamless block channel of the same level.
    for level in [1, 3] {
        let rest = REST_AT[level];
        let (p_base, k_base) = seamless_at(level);
        let (p_limit, k_limit) = (p_base + SEAM_SPIKE_MM_S, k_base + KICK_MARGIN_MM_S);
        eprintln!("L{level} seamless channel: peak {p_base:.2} mm/s, kick {k_base:.2} mm/s");

        let mut sim = track_sim(&rail_layout_at(level));
        launch(&mut sim, Vector::new(-0.02, rest, 0.0), Vector::X, SPEED);
        let tr = trace(&mut sim, TICKS);
        assert!(
            tr.iter()
                .any(|m| m.position[0] > 0.50 && (m.position[2] + 0.06).abs() < 0.005),
            "L{level}: marble must reach the last block, centred in row z = -60 mm"
        );
        assert!(
            tr.iter()
                .all(|m| m.position[0] > 0.51 || (m.position[1] - rest).abs() < 0.004),
            "L{level}: marble stays in the channel"
        );
        for w in WINDOWS {
            let sel = |m: &MarbleState| at(m, w.lo, w.hi);
            assert!(
                tr.iter().any(sel),
                "L{level} {}: marble never crossed",
                w.name
            );
            let (p, u, k) = (peak(&tr, sel), upward(&tr, sel), kick(&tr, sel));
            eprintln!(
                "L{level} {}: peak |vy| {p:.2} mm/s, upward {u:.2} (limit {p_limit:.2}), kick {k:.2} \
                 (limit {k_limit:.2})",
                w.name
            );
            let gated = if w.swinging { u } else { p };
            assert!(
                gated <= p_limit,
                "L{level} {}: {gated:.2} mm/s over {p_limit:.2}",
                w.name
            );
            assert!(
                k <= k_limit,
                "L{level} {}: kick {k:.2} mm/s over {k_limit:.2}",
                w.name
            );
        }
        let k = kick(&tr, in_s_rail);
        eprintln!("L{level} S-rail interior: kick {k:.2} (limit {k_limit:.2})");
        assert!(
            k <= k_limit,
            "L{level} S-rail interior: kick {k:.2} mm/s over {k_limit:.2}"
        );

        let mut sim = track_sim(&rail_curve_layout_at(level));
        launch(&mut sim, Vector::new(-0.02, rest, 0.0), Vector::X, SPEED);
        let tr = trace(&mut sim, TICKS);
        assert!(
            tr.iter()
                .any(|m| m.position[2] < -0.20 && (m.position[0] - 0.18).abs() < 0.005),
            "L{level}: marble must leave the curve along -z, centred on x = 180 mm"
        );
        let windows: [(&str, [f32; 2], [f32; 2], bool); 4] = [
            (
                "block to rail curve (x 90 mm)",
                [0.075, -0.03],
                [0.105, 0.03],
                true,
            ),
            ("rail curve interior", [0.105, -0.075], [0.21, 0.03], true),
            (
                "rail curve to block (z -90 mm)",
                RAIL_CURVE_EXIT.0,
                RAIL_CURVE_EXIT.1,
                true,
            ),
            (
                "block to block (z -150 mm)",
                [0.15, -0.165],
                [0.21, -0.135],
                false,
            ),
        ];
        for (name, lo, hi, swing) in windows {
            let sel = |m: &MarbleState| at(m, lo, hi);
            assert!(tr.iter().any(sel), "L{level} {name}: marble never crossed");
            let (p, u, k) = (peak(&tr, sel), upward(&tr, sel), kick(&tr, sel));
            eprintln!(
                "L{level} {name}: peak |vy| {p:.2} mm/s, upward {u:.2} (limit {p_limit:.2}{}), \
                 kick {k:.2} (limit {k_limit:.2})",
                if swing { ", swing, ungated" } else { "" }
            );
            assert!(
                swing || u <= p_limit,
                "L{level} {name}: upward {u:.2} mm/s over {p_limit:.2}"
            );
            assert!(
                k <= k_limit,
                "L{level} {name}: kick {k:.2} mm/s over {k_limit:.2}"
            );
        }
    }
}

#[test]
fn a_fast_shooter_stays_in_the_rail_curve_at_every_level() {
    // Kit contract §16 Q6: the rail lip is 5.5 mm above a shooter's centre. A shooter entering
    // each rail curve fast must still leave along -z rather than jump the outer wall.
    let shooter = MarbleKind::CatsEye.spec_sized(MarbleSize::Shooter);
    for level in [1, 2, 3] {
        for speed in [0.6, 1.0] {
            let mut sim = track_sim(&rail_curve_layout_at(level));
            let rest = 0.015 * level as f32 + shooter.radius_m;
            launch_with(
                &mut sim,
                &shooter,
                Vector::new(-0.02, rest, 0.0),
                Vector::X,
                speed,
            );
            let tr = trace(&mut sim, 600);
            let top = tr.iter().map(|m| m.position[1] - rest).fold(0.0, f32::max) * 1_000.0;
            let out = tr
                .iter()
                .any(|m| m.position[2] < -0.20 && (m.position[0] - 0.18).abs() < 0.012);
            eprintln!("L{level} shooter at {speed} m/s: rose {top:.2} mm, left along -z: {out}");
            assert!(
                out,
                "L{level}: a shooter at {speed} m/s left the rail curve"
            );
        }
    }
}

/// The rail slope chains, entered from two `L3` blocks at x 0..1 and left by two `L1` blocks:
/// gentle `L3` → `L2` → `L1` (cells x 2 and 3, then a rail straight at x 4), and steep `L3` →
/// `L1` (cell x 2, then a rail straight at x 3), and the gentle chain with one slope a block
/// either way round (rail `L3` slope onto a block `L2` slope, block `L3` onto rail `L2`). Each with
/// its seam windows (15 mm either side of
/// every seam from the first block one to the last, as plan x and z ranges) and slope interiors.
type SlopeRun = (
    &'static str,
    Vec<Placement>,
    Vec<(&'static str, [f32; 2])>,
    Vec<[f32; 2]>,
);

fn rail_slope_runs() -> [SlopeRun; 4] {
    let seam = |x: f32| [x - 0.015, x + 0.015];
    [
        (
            "gentle",
            vec![
                place("blk_straight_l3", 0, 0, 0),
                place("blk_straight_l3", 1, 0, 0),
                place("rail_slope_gentle_l3", 2, 0, 0),
                place("rail_slope_gentle_l2", 3, 0, 0),
                place("rail_straight_l1", 4, 0, 0),
                place("blk_straight_l1", 5, 0, 0),
                place("blk_straight_l1", 6, 0, 0),
            ],
            vec![
                ("block to rail slope (x 90 mm)", seam(0.09)),
                ("rail slope to rail slope (x 150 mm)", seam(0.15)),
                ("rail slope to rail (x 210 mm)", seam(0.21)),
                ("rail to block (x 270 mm)", seam(0.27)),
            ],
            vec![[0.105, 0.135], [0.165, 0.195]],
        ),
        (
            "steep",
            vec![
                place("blk_straight_l3", 0, 0, 0),
                place("blk_straight_l3", 1, 0, 0),
                place("rail_slope_steep_l3", 2, 0, 0),
                place("rail_straight_l1", 3, 0, 0),
                place("blk_straight_l1", 4, 0, 0),
                place("blk_straight_l1", 5, 0, 0),
            ],
            vec![
                ("block to rail slope (x 90 mm)", seam(0.09)),
                ("rail slope to rail (x 150 mm)", seam(0.15)),
                ("rail to block (x 210 mm)", seam(0.21)),
            ],
            vec![[0.105, 0.135]],
        ),
        (
            "rail then block slope",
            vec![
                place("blk_straight_l3", 0, 0, 0),
                place("blk_straight_l3", 1, 0, 0),
                place("rail_slope_gentle_l3", 2, 0, 0),
                place("blk_slope_gentle_l2", 3, 0, 0),
                place("blk_straight_l1", 4, 0, 0),
                place("blk_straight_l1", 5, 0, 0),
            ],
            vec![
                ("rail slope to block slope (x 150 mm)", seam(0.15)),
                ("block slope to block (x 210 mm)", seam(0.21)),
            ],
            vec![[0.165, 0.195]],
        ),
        (
            "block then rail slope",
            vec![
                place("blk_straight_l3", 0, 0, 0),
                place("blk_straight_l3", 1, 0, 0),
                place("blk_slope_gentle_l3", 2, 0, 0),
                place("rail_slope_gentle_l2", 3, 0, 0),
                place("rail_straight_l1", 4, 0, 0),
                place("blk_straight_l1", 5, 0, 0),
                place("blk_straight_l1", 6, 0, 0),
            ],
            vec![
                ("block slope to rail slope (x 150 mm)", seam(0.15)),
                ("rail slope to rail (x 210 mm)", seam(0.21)),
            ],
            vec![[0.165, 0.195]],
        ),
    ]
}

#[test]
fn rail_slopes_descend_to_l1_with_their_seams_under_the_thresholds() {
    // A marble entering each rail slope chain from an `L3` block leaves on the `L1` block,
    // centred and down in its channel. Every seam is gated on upward speed (the floor descends
    // across the slope seams, so only an upward throw is a bump) against the seamless `L3`
    // block channel's peak plus the seam spike, as `seam_test::straight_to_slope_join` gates the
    // block slope.
    //
    // Kick is gated against the same chain built from block pieces, plus the kick margin: the
    // smootherstep floor kicks on its own, near ½·vx³·DT²·|y'''| with |y'''| peaking at
    // 60·drop/cell³ at a slope's ends, so a fast exit is over the seamless margin with no seam
    // at all. Measured 2026-11-13, rail against block: 7.63 / 7.61 mm/s off the gentle L3 slope
    // onto the L2 one, 10.87 / 10.88 off the gentle L2 slope, 24.30 / 24.34 off the steep slope.
    let (p_base, _) = seamless_at(3);
    let p_limit = p_base + SEAM_SPIKE_MM_S;
    let roll = |layout: &[Placement]| {
        let mut sim = track_sim(layout);
        launch(
            &mut sim,
            Vector::new(-0.02, REST_AT[3], 0.0),
            Vector::X,
            SPEED,
        );
        trace(&mut sim, TICKS)
    };
    for (name, layout, windows, interiors) in rail_slope_runs() {
        let tr = roll(&layout);
        let blocks: Vec<Placement> = layout
            .iter()
            .map(|p| Placement {
                piece: PieceId::new(&p.piece.as_str().replace("rail_", "blk_")).unwrap(),
                ..p.clone()
            })
            .collect();
        let twin = roll(&blocks);
        let end = 0.06 * (layout.len() as f32 - 1.0) - 0.04;
        assert!(
            tr.iter().any(|m| m.position[0] > end
                && (m.position[1] - REST_AT[1]).abs() < 0.002
                && m.position[2].abs() < 0.005),
            "{name}: marble must reach the last L1 block, centred and down in its channel"
        );
        assert!(
            tr.iter()
                .all(|m| m.position[0] > 0.09 || (m.position[1] - REST_AT[3]).abs() < 0.004),
            "{name}: marble stays in the L3 channel up to the slope"
        );
        for (w, x) in &windows {
            let sel = |m: &MarbleState| at(m, [x[0], -0.03], [x[1], 0.03]);
            assert!(tr.iter().any(sel), "{name} {w}: marble never crossed");
            let (u, k) = (upward(&tr, sel), kick(&tr, sel));
            let k_limit = kick(&twin, sel) + KICK_MARGIN_MM_S;
            eprintln!(
                "{name} {w}: upward {u:.2} mm/s (limit {p_limit:.2}), kick {k:.2} (limit \
                 {k_limit:.2})"
            );
            assert!(
                u <= p_limit,
                "{name} {w}: upward {u:.2} mm/s over {p_limit:.2}"
            );
            // The block chain too: its slope-to-slope seam threw the ball up 19.6 mm/s while the
            // chunk weld left a strip across the seam untagged (see `ChunkMesh::mark_slopes`).
            let u = upward(&twin, sel);
            assert!(
                u <= p_limit,
                "{name} block twin {w}: upward {u:.2} mm/s over {p_limit:.2}"
            );
            assert!(
                k <= k_limit,
                "{name} {w}: kick {k:.2} mm/s over {k_limit:.2}"
            );
        }
        for x in &interiors {
            let sel = |m: &MarbleState| (x[0]..x[1]).contains(&m.position[0]);
            let (k, k_limit) = (kick(&tr, sel), kick(&twin, sel) + KICK_MARGIN_MM_S);
            eprintln!("{name} slope interior {x:?}: kick {k:.2} (limit {k_limit:.2})");
            assert!(
                k <= k_limit,
                "{name} slope interior {x:?}: kick {k:.2} mm/s over {k_limit:.2}"
            );
        }
    }
}
