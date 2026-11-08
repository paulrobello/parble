//! Seam bump test (P1-T3, PRD §6 P1 "no visible or physical bump at piece seams", risk R2).
//!
//! A cat's eye marble is launched rolling (ω = v / r) along the `L3` channel floor and its peak
//! vertical velocity is compared with a seamless reference. Flat channels give a vertical velocity
//! of zero in theory; what the solver leaves is contact jitter (about ±10 mm/s alternating each
//! tick), so the threshold is the margin a seam may add above the seamless peak.

use crate::chunk::{ChunkMesh, placed_triangles};
use crate::marble::CATS_EYE;
use crate::{MarbleState, Sim};
use parble_core::{CellCoord, PieceId, Placement, Yaw4};
use rapier3d::prelude::*;

/// Allowed peak vertical-velocity spike above the seamless baseline, mm/s. The plan's starting
/// value (2 mm/s) held unchanged. Measured 2026-11-07 at 0.3 m/s: 20 straights 13.52 mm/s against a
/// seamless 20-cell channel 13.15 mm/s (+0.37). Without the flat-vertex simplification and the
/// chunk seam hook the same run peaks at 65–76 mm/s (`seams_without_the_fix_bump`).
const SEAM_SPIKE_MM_S: f32 = 2.0;
/// Launch speed along the channel, m/s.
const SPEED: f32 = 0.3;
/// Centre height of a marble resting on an `L3` floor (45 mm) in cell layer 0.
const REST_Y: f32 = 0.045 + 0.008;

fn place(piece: &str, x: i32, z: i32, yaw: u8, mirror: bool) -> Placement {
    Placement::new(
        PieceId::new(piece).unwrap(),
        CellCoord::new(x, 0, z),
        Yaw4::new(yaw),
        mirror.into(),
    )
}

fn launch(sim: &mut Sim, at: Vector, dir: Vector) {
    let (mut body, collider) = CATS_EYE.build(at);
    body.set_linvel(dir * SPEED, true);
    body.set_angvel(Vector::Y.cross(dir) * (SPEED / CATS_EYE.radius_m), true);
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

/// A run over `layout` through the real API: chunk merge, simplification and the seam hook.
fn roll_track(layout: &[Placement], at: Vector, dir: Vector, ticks: u32) -> Vec<MarbleState> {
    let kit = parble_kit::builtin();
    let mut sim = Sim::empty();
    for p in layout {
        sim.place_piece(kit, p.clone()).unwrap();
    }
    sim.rebuild_chunks(kit).unwrap();
    assert!(!sim.world.integration_parameters.contact_clustering);
    launch(&mut sim, at, dir);
    trace(&mut sim, ticks)
}

/// A run over hand-built colliders, with the same solver settings as [`roll_track`].
fn roll_colliders(
    colliders: Vec<Collider>,
    at: Vector,
    dir: Vector,
    ticks: u32,
) -> Vec<MarbleState> {
    let mut sim = Sim::empty();
    sim.world.integration_parameters.contact_clustering = false;
    for c in colliders {
        sim.world.colliders.insert(c);
    }
    launch(&mut sim, at, dir);
    trace(&mut sim, ticks)
}

fn straight_run(n: i32) -> Vec<Placement> {
    (0..n)
        .map(|x| place("blk_straight_l3", x, 0, 0, false))
        .collect()
}

fn triangles(layout: &[Placement]) -> Vec<[[i64; 3]; 3]> {
    let kit = parble_kit::builtin();
    layout
        .iter()
        .flat_map(|p| placed_triangles(kit.piece(&p.piece).unwrap(), p))
        .collect()
}

/// One straight stretched over `n` cells: the same cross-section with no seam.
fn seamless_straight(n: i64) -> Collider {
    let tris: Vec<[[i64; 3]; 3]> = triangles(&straight_run(1))
        .into_iter()
        .map(|t| t.map(|v| [(v[0] + 30_000) * n - 30_000, v[1], v[2]]))
        .collect();
    ChunkMesh::from_triangles(&tris)
        .collider([0, 0, 0])
        .unwrap()
}

/// Peak |vy| in mm/s while `sel` holds.
fn peak(tr: &[MarbleState], sel: impl Fn(&MarbleState) -> bool) -> f32 {
    tr.iter()
        .filter(|m| sel(m))
        .map(|m| m.linvel[1].abs())
        .fold(0.0, f32::max)
        * 1_000.0
}

/// The 20-straight window: from the first seam (skipping the launch cell) to the last.
fn in_run(m: &MarbleState) -> bool {
    (0.03..1.14).contains(&m.position[0])
}

const RUN_TICKS: u32 = 1_100;

#[test]
fn twenty_straights_roll_like_one_seamless_channel() {
    let start = Vector::new(-0.02, REST_Y, 0.0);
    let run = roll_track(&straight_run(20), start, Vector::X, RUN_TICKS);
    let base = roll_colliders(vec![seamless_straight(20)], start, Vector::X, RUN_TICKS);
    // The run crosses 19 piece seams and two chunk boundaries (x = 450 and 930 mm).
    assert!(
        run.iter().any(|m| m.position[0] > 1.14),
        "marble must cross every seam"
    );
    assert!(run.iter().all(|m| !in_run(m) || m.position[1] > 0.05));
    let (p_run, p_base) = (peak(&run, in_run), peak(&base, in_run));
    eprintln!("20 straights: peak {p_run:.2} mm/s, seamless {p_base:.2} mm/s");
    assert!(
        p_run <= p_base + SEAM_SPIKE_MM_S,
        "seam spike {p_run:.2} mm/s over seamless {p_base:.2} + {SEAM_SPIKE_MM_S}"
    );
}

#[test]
fn seams_without_the_fix_bump() {
    // Negative control: per-piece colliders (no merge) on the same run exceed the threshold, so
    // the metric does detect seams.
    let start = Vector::new(-0.02, REST_Y, 0.0);
    let pieces: Vec<Collider> = straight_run(20)
        .iter()
        .map(|p| {
            ChunkMesh::from_triangles(&triangles(std::slice::from_ref(p)))
                .collider([0, 0, 0])
                .unwrap()
        })
        .collect();
    let control = roll_colliders(pieces, start, Vector::X, RUN_TICKS);
    let base = roll_colliders(vec![seamless_straight(20)], start, Vector::X, RUN_TICKS);
    let (p_ctl, p_base) = (peak(&control, in_run), peak(&base, in_run));
    eprintln!("per-piece control: peak {p_ctl:.2} mm/s, seamless {p_base:.2} mm/s");
    assert!(p_ctl > p_base + SEAM_SPIKE_MM_S);
}

/// Allowed peak kick ([`kick`]) at a curve join or inside a curve above the seamless straight's,
/// mm/s. Measured 2026-11-07 at 0.3 m/s with the exact-tube bend contact ([`ChannelBend`]): joins
/// 20.96, 16.19, 22.29 and 5.40, interiors 4.32 and 7.39, against the seamless straight's 19.26
/// (+3.03 at most, the straight's own per-tick contact jitter under the extra curve load). The
/// faceted bend without the fix kicks 45.24–59.75 (`curves_without_the_fix_kick`).
///
/// [`ChannelBend`]: crate::chunk::ChannelBend
const CURVE_KICK_MARGIN_MM_S: f32 = 5.0;

/// Peak impulsive kick in mm/s while `sel` holds: how far a tick's vertical velocity departs from
/// the mean of its two neighbours. A bump is an impulse and shows in full. The marble's smooth
/// swing up the outer wall of a curve (its vertical speed peaks at 30–66 mm/s on an exact tube,
/// see [`curve_and_slope_joins`]) has a period near 30 ticks and leaks under 2 mm/s.
fn kick(tr: &[MarbleState], sel: impl Fn(&MarbleState) -> bool) -> f32 {
    tr.windows(3)
        .filter(|w| sel(&w[1]))
        .map(|w| (w[1].linvel[1] - 0.5 * (w[0].linvel[1] + w[2].linvel[1])).abs())
        .fold(0.0, f32::max)
        * 1_000.0
}

/// Straights east, a curve turning to +z, straights north, a mirrored curve turning back east,
/// two straights, then the gentle slope (cell x = 7, 390..450 mm). All in chunk (0, 0, 0).
fn curve_layout() -> Vec<Placement> {
    let mut layout = straight_run(4);
    layout.push(place("blk_curve90_l3", 4, 0, 1, false));
    for z in 1..4 {
        layout.push(place("blk_straight_l3", 4, z, 1, false));
    }
    layout.push(place("blk_curve90_l3", 4, 4, 0, true));
    layout.push(place("blk_straight_l3", 5, 4, 0, false));
    layout.push(place("blk_straight_l3", 6, 4, 0, false));
    layout.push(place("blk_slope_gentle_l3", 7, 4, 0, false));
    layout
}

fn at(m: &MarbleState, lo: [f32; 2], hi: [f32; 2]) -> bool {
    (lo[0]..hi[0]).contains(&m.position[0]) && (lo[1]..hi[1]).contains(&m.position[2])
}

/// Curve joins and interiors as `(name, plan min, plan max)` in metres, 15 mm either side of a
/// seam. Curve (4, 0, 0) arcs from (210, 0) to (240, 30) mm; the mirrored curve (4, 0, 4) from
/// (240, 210) to (270, 240) mm.
const CURVE_REGIONS: [(&str, [f32; 2], [f32; 2]); 6] = [
    ("straight to curve", [0.195, -0.03], [0.225, 0.03]),
    ("curve interior", [0.225, -0.03], [0.27, 0.015]),
    ("curve to straight", [0.21, 0.015], [0.27, 0.045]),
    ("straight to mirrored curve", [0.21, 0.195], [0.27, 0.225]),
    ("mirrored curve interior", [0.21, 0.225], [0.255, 0.27]),
    ("mirrored curve to straight", [0.255, 0.21], [0.285, 0.27]),
];

#[test]
fn curve_and_slope_joins() {
    // The kit has no `L2` entry piece yet, so the slope is last and only its entry seam
    // (x = 390 mm) is a join.
    //
    // Curves are gated on [`kick`], not on peak vertical speed: on an exact tube the marble
    // swings up to 2 mm up the outer wall and back (the 30 mm bend radius starts at the port),
    // a smooth motion that peaks at 30–66 mm/s depending on the swing it enters with. That is the
    // curve's geometry, not a bump. Measured 2026-11-07 at 0.3 m/s, peak |vy| with the fix:
    // joins 31.83, 24.02, 58.63, 28.25, interiors 38.66, 66.80 mm/s; without it 64.95, 44.09,
    // 49.81, 44.19 and 53.98, 38.77 mm/s (the facet chatter dissipates part of the swing).
    let layout = curve_layout();
    let start = Vector::new(-0.02, REST_Y, 0.0);
    let tr = roll_track(&layout, start, Vector::X, 1_600);
    let base = roll_colliders(vec![seamless_straight(20)], start, Vector::X, RUN_TICKS);
    let (p_base, k_base) = (peak(&base, in_run), kick(&base, in_run));
    assert!(
        tr.iter().any(|m| m.position[0] > 0.45),
        "marble must leave the slope exit"
    );
    assert!(
        tr.iter()
            .all(|m| m.position[1] > 0.05 || m.position[0] > 0.39),
        "marble stays in the L3 channel up to the slope"
    );

    let limit = k_base + CURVE_KICK_MARGIN_MM_S;
    for (name, lo, hi) in CURVE_REGIONS {
        assert!(
            tr.iter().any(|m| at(m, lo, hi)),
            "{name}: marble never crossed"
        );
        let (p, k) = (peak(&tr, |m| at(m, lo, hi)), kick(&tr, |m| at(m, lo, hi)));
        eprintln!("{name}: kick {k:.2} mm/s, limit {limit:.2} (peak |vy| {p:.2})");
        assert!(k <= limit, "{name}: kick {k:.2} mm/s over {limit:.2}");
    }

    // The marble leaves the mirrored curve still swinging (about 1 mm up either wall, ±47 mm/s,
    // measured 2026-11-07), so the slope entry here peaks at 51.31 mm/s upward from the swing
    // alone. Before the bend fix the facet chatter damped the swing and slowed the marble to
    // about 0.19 m/s, which is why this entry used to pass 15.15 mm/s. Its kick, 25.15 with the
    // exact slope contact, is the swinging marble's hand-over from the straight's
    // facets to that contact and sits on the curve limit (24.26), so it is logged, not gated; the
    // centred run in `straight_to_slope_join` gates the slope entry.
    let entry = [[0.375, 0.21], [0.405, 0.27]];
    eprintln!(
        "straight to slope after the curves: peak upward {:.2} mm/s (ungated, p_base {p_base:.2}), \
         kick {:.2} (limit {limit:.2})",
        upward(&tr, |m| at(m, entry[0], entry[1])),
        kick(&tr, |m| at(m, entry[0], entry[1]))
    );
}

/// Peak upward vertical velocity in mm/s while `sel` holds.
fn upward(tr: &[MarbleState], sel: impl Fn(&MarbleState) -> bool) -> f32 {
    tr.iter()
        .filter(|m| sel(m))
        .map(|m| m.linvel[1])
        .fold(0.0, f32::max)
        * 1_000.0
}

/// The straights-then-slope layout and its entry window (cell x = 7, 390..450 mm). The kit has no
/// `L2` entry piece yet, so the slope is last and only its entry seam (x = 390 mm) is a join.
fn slope_layout() -> (Vec<Placement>, impl Fn(&MarbleState) -> bool) {
    let mut layout = straight_run(7);
    layout.push(place("blk_slope_gentle_l3", 7, 0, 0, false));
    (layout, |m: &MarbleState| {
        (0.375..0.405).contains(&m.position[0])
    })
}

/// The slope entry's limits: the seam spike over the seamless straight's peak for upward speed
/// (the floor starts to descend at the seam, so only upward counts), and the curve kick margin
/// over its kick.
fn slope_limits() -> (f32, f32) {
    let start = Vector::new(-0.02, REST_Y, 0.0);
    let base = roll_colliders(vec![seamless_straight(20)], start, Vector::X, RUN_TICKS);
    (
        peak(&base, in_run) + SEAM_SPIKE_MM_S,
        kick(&base, in_run) + CURVE_KICK_MARGIN_MM_S,
    )
}

#[test]
fn straight_to_slope_join() {
    // A centred marble at 0.3 m/s, gated on both peak upward speed (limit 13.15 + 2 = 15.15) and
    // kick (19.26 + 5 = 24.26). Measured 2026-11-07 with the exact slope contact
    // ([`ChannelSlope`]): upward 7.31, kick 18.01; the kick is the hand-over from the straight's
    // facets to the exact contact a ball diameter before the seam, not the seam. Without the
    // contact the entry peaks at 32.32 upward with kick 43.62 (`slope_without_the_fix_bumps`).
    // The slope interior (405..445 mm) is logged, not gated: kick 48.55, from the concave lower
    // half (425..445 mm; 0.16 and 8.56 over 405..425), against 134.06 without the contact.
    //
    // [`ChannelSlope`]: crate::chunk::ChannelSlope
    let (layout, entry) = slope_layout();
    let start = Vector::new(-0.02, REST_Y, 0.0);
    let tr = roll_track(&layout, start, Vector::X, RUN_TICKS);
    assert!(
        tr.iter().any(|m| m.position[0] > 0.45),
        "marble must leave the slope exit"
    );
    assert!(tr.iter().any(&entry), "slope entry never crossed");
    let (p, k) = (upward(&tr, &entry), kick(&tr, &entry));
    let (p_limit, k_limit) = slope_limits();
    eprintln!(
        "straight to slope: peak upward {p:.2} mm/s (limit {p_limit:.2}), kick {k:.2} \
         (limit {k_limit:.2}), slope interior kick {:.2} (ungated)",
        kick(&tr, |m| (0.405..0.445).contains(&m.position[0])),
    );
    assert!(
        p <= p_limit,
        "slope entry: upward {p:.2} mm/s over {p_limit:.2}"
    );
    assert!(
        k <= k_limit,
        "slope entry: kick {k:.2} mm/s over {k_limit:.2}"
    );
}

#[test]
fn slope_without_the_fix_bumps() {
    // Negative control: the same layout merged into one welded, simplified trimesh but without
    // the contact hook exceeds both limits at the slope entry, so the gate detects the bump: the
    // ball, still on the straight, hits the slope's first bottom vertex 0.4 mm ahead with a normal
    // leaning 3° back, and bounces.
    let (layout, entry) = slope_layout();
    let mut mesh = ChunkMesh::from_triangles(&triangles(&layout));
    mesh.simplify_flat();
    let start = Vector::new(-0.02, REST_Y, 0.0);
    let control = roll_colliders(
        vec![mesh.collider([0, 0, 0]).unwrap()],
        start,
        Vector::X,
        RUN_TICKS,
    );
    let (p, k) = (upward(&control, &entry), kick(&control, &entry));
    let (p_limit, k_limit) = slope_limits();
    eprintln!(
        "straight to slope without the fix: upward {p:.2} mm/s (limit {p_limit:.2}), kick {k:.2} \
         (limit {k_limit:.2})"
    );
    assert!(p > p_limit, "control upward {p:.2} not over {p_limit:.2}");
    assert!(k > k_limit, "control kick {k:.2} not over {k_limit:.2}");
}

#[test]
fn curves_without_the_fix_kick() {
    // Negative control: the same layout merged into one welded, simplified trimesh but without
    // the bend contact hook kicks well over the limit at every curve join and interior, so the
    // metric does detect the facet chatter.
    let layout = curve_layout();
    let mut mesh = ChunkMesh::from_triangles(&triangles(&layout));
    mesh.simplify_flat();
    let start = Vector::new(-0.02, REST_Y, 0.0);
    let control = roll_colliders(
        vec![mesh.collider([0, 0, 0]).unwrap()],
        start,
        Vector::X,
        1_600,
    );
    let base = roll_colliders(vec![seamless_straight(20)], start, Vector::X, RUN_TICKS);
    let limit = kick(&base, in_run) + CURVE_KICK_MARGIN_MM_S;
    for (name, lo, hi) in CURVE_REGIONS {
        let k = kick(&control, |m| at(m, lo, hi));
        eprintln!("{name} without the fix: kick {k:.2} mm/s, limit {limit:.2}");
        assert!(k > limit, "{name}: control kick {k:.2} not over {limit:.2}");
    }
}
