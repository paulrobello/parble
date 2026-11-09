//! P2-T4 rail family: a cat's eye marble runs block → rail → S-rail → block at `L2` through the
//! real API (chunk merge, simplification, contact hooks), and the seam bump metrics of
//! `seam_test` are gated at every seam against the seamless `L2` block channel.

use crate::chunk::{ChunkMesh, placed_triangles};
use crate::marble::CATS_EYE;
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
    let (mut body, collider) = CATS_EYE.build(at);
    body.set_linvel(dir * speed, true);
    if speed > 0.0 {
        body.set_angvel(Vector::Y.cross(dir) * (speed / CATS_EYE.radius_m), true);
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

#[test]
fn block_rail_curve_block_seams_stay_under_the_seam_thresholds() {
    // The 90° rail curve (2 × 2, radius 90 mm) between `L2` straights: in along +x at x = 90 mm,
    // out along -z at z = -90 mm, x = 180 mm. Gated like the S-rail's seams (upward and kick),
    // except the block to rail curve seam and the curve's interior, gated on kick only as a bend
    // inlet is (`joiner_seams_stay_under_the_seam_thresholds`): their upward speed (3.54 and
    // 4.41 mm/s) is the swing up the curve's outer wall. Measured 2026-11-08, the old solver against the vendored Rapier
    // contact fix (PHYSICS_VERSION 3): both trajectories rise the same 0.19 mm over x 92-108 mm
    // (37.98 to 38.17 mm old, 38.00 to 38.20 mm new) as z swings out. The old reading stayed
    // negative under the rolling sink's -3 to -10 mm/s offset.
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
    let mut sim = track_sim(&layout);
    launch(&mut sim, Vector::new(-0.02, REST_Y, 0.0), Vector::X, SPEED);
    let tr = trace(&mut sim, TICKS);
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
            [0.15, -0.105],
            [0.21, -0.075],
        ),
        ("block to block (z -150 mm)", [0.15, -0.165], [0.21, -0.135]),
    ];
    for (name, lo, hi) in windows {
        let sel = |m: &MarbleState| at(m, lo, hi);
        assert!(tr.iter().any(sel), "{name}: marble never crossed");
        let (p, u, k) = (peak(&tr, sel), upward(&tr, sel), kick(&tr, sel));
        let swing = name.starts_with("block to rail curve") || name == "rail curve interior";
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
    assert!(events.iter().any(|e| matches!(
        e,
        SimEvent::Contact(c) if c.other == ContactOther::Static && c.surface == rail
    )));
}
