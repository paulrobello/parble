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
    roll_track_with(layout, at, dir, ticks, false)
}

/// [`roll_track`], optionally keeping the bounce at each slope contact hand-over
/// (`ChunkColliders::slope_hand_over_bounces`).
fn roll_track_with(
    layout: &[Placement],
    at: Vector,
    dir: Vector,
    ticks: u32,
    hand_over_bounces: bool,
) -> Vec<MarbleState> {
    let mut sim = track_sim(layout);
    sim.pieces.slope_hand_over_bounces = hand_over_bounces;
    launch(&mut sim, at, dir);
    trace(&mut sim, ticks)
}

/// A sim holding `layout`'s chunk colliders and no marble.
fn track_sim(layout: &[Placement]) -> Sim {
    let kit = parble_kit::builtin();
    let mut sim = Sim::empty();
    for p in layout {
        sim.place_piece(kit, p.clone()).unwrap();
    }
    sim.rebuild_chunks(kit).unwrap();
    assert!(!sim.world.integration_parameters.contact_clustering);
    sim
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
/// mm/s. Measured 2026-11-07 at 0.3 m/s with the exact-tube bend contact ([`ChannelBend`]),
/// evaluated at the end of the step on one manifold without hand-over bounce: joins 2.14, 0.94,
/// 2.56 and 0.58, interiors 0.55 and 0.48, against the seamless straight's 19.26 (the
/// start-of-step contact copied onto every manifold gave 20.96, 16.19, 22.29, 5.40, 4.32 and
/// 7.39). The faceted bend without the fix kicks 45.24–59.75 (`curves_without_the_fix_kick`).
///
/// [`ChannelBend`]: crate::chunk::ChannelBend
const CURVE_KICK_MARGIN_MM_S: f32 = 5.0;

/// Peak impulsive kick in mm/s while `sel` holds: how far a tick's vertical velocity departs from
/// the mean of its two neighbours. A bump is an impulse and shows in full. The marble's smooth
/// swing up the outer wall of a curve (its vertical speed peaks at 30–66 mm/s on an exact tube,
/// see [`curve_and_slope_joins`]) has a period near 30 ticks and leaks under 2 mm/s.
pub(crate) fn kick(tr: &[MarbleState], sel: impl Fn(&MarbleState) -> bool) -> f32 {
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

/// The slope entry of [`curve_layout`], 15 mm either side of the seam at x = 390 mm.
const CURVE_SLOPE_ENTRY: [[f32; 2]; 2] = [[0.375, 0.21], [0.405, 0.27]];

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
    // joins 10.28, 27.61, 10.44, 16.78, interiors 35.90, 30.94 mm/s; without it 64.95, 44.09,
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

    // The marble used to leave the mirrored curve still swinging (about 1 mm up either wall,
    // ±47 mm/s), so the slope entry here peaked at 46.38 mm/s upward from the swing alone; with
    // the end-of-step bend contact it peaks at 0.01 (measured 2026-11-07). That stays logged, not
    // gated. Its kick is gated: 1.59 mm/s, measured 2026-11-07 (5.49 with the start-of-step bend
    // contact), against 34.25 without the contact hooks (`curves_without_the_fix_kick`). With the
    // slope's hand-over bounce kept it kicked 25.15, only 0.89 over the limit, too thin a margin
    // for a control.
    let entry = CURVE_SLOPE_ENTRY;
    assert!(
        tr.iter().any(|m| at(m, entry[0], entry[1])),
        "slope entry after the curves never crossed"
    );
    let k = kick(&tr, |m| at(m, entry[0], entry[1]));
    eprintln!(
        "straight to slope after the curves: peak upward {:.2} mm/s (ungated, p_base {p_base:.2}), \
         kick {k:.2} (limit {limit:.2})",
        upward(&tr, |m| at(m, entry[0], entry[1])),
    );
    assert!(
        k <= limit,
        "slope entry after the curves: kick {k:.2} mm/s over {limit:.2}"
    );
}

/// Peak upward vertical velocity in mm/s while `sel` holds.
pub(crate) fn upward(tr: &[MarbleState], sel: impl Fn(&MarbleState) -> bool) -> f32 {
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
    // kick (19.26 + 5 = 24.26) at the entry, and on kick inside the slope (405..445 mm). Measured
    // 2026-11-07 with the exact slope contact ([`ChannelSlope`]): entry upward 0.00, kick 8.66;
    // interior kick 4.28. With the contact but the bounce at each triangle hand-over kept, the
    // entry kicks 18.01 and the interior 48.55, all from the concave lower half
    // (`slope_hand_overs_without_the_fix_bounce`). Without the contact the entry peaks at 32.32
    // upward with kick 43.62 and the interior kicks 134.06 (`slope_without_the_fix_bumps`).
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
    let (p, k, k_in) = (upward(&tr, &entry), kick(&tr, &entry), kick(&tr, in_slope));
    let (p_limit, k_limit) = slope_limits();
    eprintln!(
        "straight to slope: peak upward {p:.2} mm/s (limit {p_limit:.2}), kick {k:.2} \
         (limit {k_limit:.2}), slope interior kick {k_in:.2} (limit {k_limit:.2})",
    );
    assert!(
        p <= p_limit,
        "slope entry: upward {p:.2} mm/s over {p_limit:.2}"
    );
    assert!(
        k <= k_limit,
        "slope entry: kick {k:.2} mm/s over {k_limit:.2}"
    );
    assert!(
        k_in <= k_limit,
        "slope interior: kick {k_in:.2} mm/s over {k_limit:.2}"
    );
}

/// The slope interior: cell x = 7 past the entry window, short of the exit seam.
fn in_slope(m: &MarbleState) -> bool {
    (0.405..0.445).contains(&m.position[0])
}

#[test]
fn slope_hand_overs_without_the_fix_bounce() {
    // Negative control: the exact slope contact with the marble's restitution kept on it. The
    // contact moves to a fresh manifold, which Rapier treats as a new contact, each time the ball
    // crosses a triangle edge; in the concave lower half the ball approaches it along the normal
    // and bounces, so the interior exceeds the kick limit.
    let (layout, _) = slope_layout();
    let start = Vector::new(-0.02, REST_Y, 0.0);
    let control = roll_track_with(&layout, start, Vector::X, RUN_TICKS, true);
    let k_in = kick(&control, in_slope);
    let (_, k_limit) = slope_limits();
    eprintln!("slope interior with the hand-over bounce: kick {k_in:.2} mm/s (limit {k_limit:.2})");
    assert!(
        k_in > k_limit,
        "control interior kick {k_in:.2} not over {k_limit:.2}"
    );
}

#[test]
fn a_marble_dropped_on_the_slope_still_bounces() {
    // The hand-over fix drops restitution only on a contact the ball already penetrates: a ball
    // falling onto the slope meets it first as a speculative contact and bounces as it would
    // with the bounce kept.
    let (layout, _) = slope_layout();
    let drop = |keep: bool| {
        let mut sim = track_sim(&layout);
        sim.pieces.slope_hand_over_bounces = keep;
        // 5 mm over the rest height at the middle of the slope (x = 420 mm).
        let (body, collider) = CATS_EYE.build(Vector::new(0.42, 0.0455 + 0.005, 0.0));
        let h = sim.world.bodies.insert(body);
        sim.world
            .colliders
            .insert_with_parent(collider, h, &mut sim.world.bodies);
        sim.marbles.push(h);
        let tr = trace(&mut sim, 60);
        upward(&tr, |_| true)
    };
    // Measured 2026-11-07: 116.47 mm/s both ways (impact at about 313 mm/s).
    let (fixed, kept) = (drop(false), drop(true));
    eprintln!("drop on the slope: peak upward {fixed:.2} mm/s, {kept:.2} with the bounce kept");
    assert!(
        fixed > 100.0,
        "dropped marble did not bounce: {fixed:.2} mm/s"
    );
    assert!(
        (fixed - kept).abs() < 0.01,
        "bounce {fixed:.2} mm/s differs from {kept:.2}"
    );
}

#[test]
fn slope_without_the_fix_bumps() {
    // Negative control: the same layout merged into one welded, simplified trimesh but without
    // the contact hook exceeds both limits at the slope entry, so the gate detects the bump: the
    // ball, still on the straight, hits the slope's first bottom vertex 0.4 mm ahead with a normal
    // leaning 3° back, and bounces. With Coulomb rolling resistance (2026-11-08) the ball arrives
    // at 279 mm/s instead of about 290 and the bounce no longer lifts it (upward 0.00, was 32.32),
    // but it still kicks 43.18 (was 43.62), so only the kick is the control; upward is logged.
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
    assert!(k > k_limit, "control kick {k:.2} not over {k_limit:.2}");
}

#[test]
fn curves_without_the_fix_kick() {
    // Negative control: the same layout merged into one welded, simplified trimesh but without
    // the bend and slope contact hooks kicks well over the limit at every curve join and
    // interior and at the slope entry after them, so the metric does detect the facet chatter.
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
    let [lo, hi] = CURVE_SLOPE_ENTRY;
    let k = kick(&control, |m| at(m, lo, hi));
    eprintln!("straight to slope after the curves without the fix: kick {k:.2} mm/s");
    assert!(
        k > limit,
        "slope entry: control kick {k:.2} not over {limit:.2}"
    );
}

/// Peak marble speed allowed over the last second of a 10 s rest, mm/s. Measured 2026-11-07 on the
/// playroom run's far leg: 0.037 (one marble 4 mm off centre), 0.005 (the same beside the curve,
/// on the exact tube contact) and 0.252 (the 4-marble train), against 189, 189 and 193 with the
/// start-of-step bend contact.
const REST_SPEED_MM_S: f32 = 1.0;
/// Largest rise allowed in the marbles' total energy above its start, µJ. Measured 0.00 in every
/// case, against 38.7, 38.1 and 411 with the start-of-step bend contact.
const REST_ENERGY_RISE_UJ: f32 = 1.0;

/// The playroom run with `starts` (centre, velocity) marbles of the run's types, ticked 10 s.
/// Returns the peak speed over the last second (mm/s) and the largest rise of the marbles' total
/// energy (kinetic + potential) above its value at tick 0 (µJ).
fn rest_in_playroom(starts: &[(Vector, Vector)], legacy_bend_contact: bool) -> (f32, f32) {
    let kit = parble_kit::builtin();
    let mut sim = Sim::empty();
    for p in crate::playroom_run::placements() {
        sim.place_piece(kit, p).unwrap();
    }
    sim.rebuild_chunks(kit).unwrap();
    sim.pieces.bend_contact_at_step_start = legacy_bend_contact;
    for (i, &(at, v)) in starts.iter().enumerate() {
        let (kind, size) = crate::playroom_run::MARBLES[i];
        let spec = kind.spec_sized(size);
        let (mut body, collider) = spec.build(at);
        body.set_linvel(v, true);
        body.set_angvel(Vector::Y.cross(v) / spec.radius_m, true);
        let h = sim.world.bodies.insert(body);
        sim.world
            .colliders
            .insert_with_parent(collider, h, &mut sim.world.bodies);
        sim.marbles.push(h);
    }
    let energy = |sim: &Sim| -> f32 {
        sim.marbles
            .iter()
            .map(|&h| {
                let b = &sim.world.bodies[h];
                b.kinetic_energy() + b.mass() * crate::marble::G * b.translation().y
            })
            .sum()
    };
    let e0 = energy(&sim);
    let (mut rise, mut peak) = (0.0f32, 0.0f32);
    for t in 1..=10 * crate::TICK_HZ {
        sim.tick(1);
        rise = rise.max(energy(&sim) - e0);
        if t > 9 * crate::TICK_HZ {
            for m in sim.marbles() {
                let [x, y, z] = m.linvel;
                peak = peak.max((x * x + y * y + z * z).sqrt());
            }
        }
    }
    (peak * 1_000.0, rise * 1.0e6)
}

/// Centre of a standard marble resting on the far leg's half-round `dz` metres off its centre
/// line, `x` metres along the run.
fn far_leg_at(x: f32, dz: f32) -> Vector {
    let r = parble_core::contract::MARBLE_STANDARD_DIAMETER_UM as f32 * 0.5e-6;
    let reach = 0.014 - r;
    let row = crate::playroom_run::ROW_FAR as f32 * crate::scene::cell_m();
    Vector::new(
        x,
        0.059 - (reach * reach - dz * dz).sqrt() + 0.0002,
        row + dz,
    )
}

/// One marble placed at rest 4 mm off the centre line of the far leg, three cells from the stop.
fn off_centre_marble() -> Vec<(Vector, Vector)> {
    vec![(far_leg_at(0.18, 0.004), Vector::ZERO)]
}

/// Four marbles rolling into the finish stop at 20 mm/s, 0.5 mm apart and 2 mm off centre on
/// alternating sides: they gather into a train against the stop.
fn train_into_the_stop() -> Vec<(Vector, Vector)> {
    (0..4)
        .map(|i| {
            let dz = if i % 2 == 0 { 0.002 } else { -0.002 };
            (
                far_leg_at(0.04 + 0.0165 * i as f32, dz),
                Vector::new(-0.02, 0.0, 0.0),
            )
        })
        .collect()
}

#[test]
fn marbles_at_rest_in_a_bend_row_stay_at_rest() {
    for (name, starts) in [
        ("off-centre marble", off_centre_marble()),
        // 10 mm short of the curve's cell, where the exact tube contact still holds.
        (
            "off-centre marble beside the curve",
            vec![(far_leg_at(0.38, 0.004), Vector::ZERO)],
        ),
        ("4-marble train at the stop", train_into_the_stop()),
    ] {
        let (fixed, fixed_rise) = rest_in_playroom(&starts, false);
        let (legacy, legacy_rise) = rest_in_playroom(&starts, true);
        eprintln!(
            "{name}: peak speed 9-10 s {fixed:.3} mm/s (limit {REST_SPEED_MM_S}), energy rise \
             {fixed_rise:.2} uJ; start-of-step bend contact: {legacy:.2} mm/s, {legacy_rise:.2} uJ"
        );
        assert!(
            fixed < REST_SPEED_MM_S,
            "{name}: still moving at {fixed:.3} mm/s after 9 s"
        );
        assert!(
            fixed_rise < REST_ENERGY_RISE_UJ,
            "{name}: gained {fixed_rise:.2} uJ"
        );
        // Negative control: the start-of-step bend contact keeps the marbles rocking.
        assert!(
            legacy > REST_SPEED_MM_S && legacy_rise > REST_ENERGY_RISE_UJ,
            "{name}: control {legacy:.2} mm/s, {legacy_rise:.2} uJ not over the limits"
        );
    }
}

/// Launch speed of the rolling-resistance rest test, m/s: a marble barely rolling.
const SLOW_SPEED: f32 = 0.02;
/// Ticks by which a [`SLOW_SPEED`] marble must be at rest on a flat channel. Coulomb rolling
/// resistance decelerates it at 5/7·c_rr·g = 14.0 mm/s², so it stops after 1.43 s (343 ticks).
const SLOW_REST_TICKS: usize = 480;
/// Speed under which a marble counts as at rest, mm/s.
const SLOW_REST_MM_S: f32 = 0.01;

/// A cat's eye launched rolling at `speed` along 20 straights: its speed (mm/s) each tick for
/// 4 s. `damping` puts the angular damping that stood in for rolling resistance before 2026-11-08
/// (2.5·c_rr·g / 0.5 m/s) on the marble instead of the contact model, `crr` overrides `c_rr`.
fn slow_roll(speed: f32, crr: f32, damping: bool) -> Vec<f32> {
    let mut sim = track_sim(&straight_run(20));
    let spec = crate::marble::MarbleSpec {
        rolling_resistance: crr,
        ..CATS_EYE
    };
    let (mut body, collider) = spec.build(Vector::new(0.03, REST_Y, 0.0));
    body.set_linvel(Vector::X * speed, true);
    body.set_angvel(Vector::Y.cross(Vector::X) * (speed / spec.radius_m), true);
    if damping {
        body.set_angular_damping(2.5 * CATS_EYE.rolling_resistance * crate::marble::G / 0.5);
    }
    let h = sim.world.bodies.insert(body);
    sim.world
        .colliders
        .insert_with_parent(collider, h, &mut sim.world.bodies);
    sim.marbles.push(h);
    (0..4 * crate::TICK_HZ)
        .map(|_| {
            sim.tick(1);
            let [x, y, z] = sim.marbles()[0].linvel;
            (x * x + y * y + z * z).sqrt() * 1_000.0
        })
        .collect()
}

#[test]
fn a_slow_marble_comes_to_rest_on_a_flat_channel() {
    // Measured 2026-11-08: at rest from tick 336 on (Coulomb rolling resistance). The damping it
    // replaced leaves the marble rolling at 17.39 mm/s after 4 s, and without rolling resistance
    // it still rolls at 19.48 mm/s.
    let speeds = slow_roll(SLOW_SPEED, CATS_EYE.rolling_resistance, false);
    let rest = speeds
        .iter()
        .rposition(|&v| v >= SLOW_REST_MM_S)
        .map_or(0, |i| i + 1);
    eprintln!(
        "{} mm/s marble: at rest from tick {rest} (limit {SLOW_REST_TICKS})",
        SLOW_SPEED * 1_000.0
    );
    assert!(
        rest <= SLOW_REST_TICKS,
        "still moving at tick {rest}: {:.4} mm/s",
        speeds[rest.saturating_sub(1)]
    );
    // Negative controls: the old damping and no rolling resistance both keep it rolling.
    for (name, crr, damping) in [
        ("angular damping", 0.0, true),
        ("no rolling resistance", 0.0, false),
    ] {
        let v = *slow_roll(SLOW_SPEED, crr, damping).last().unwrap();
        eprintln!("{name}: {v:.3} mm/s after 4 s");
        assert!(v > 10.0, "{name}: {v:.3} mm/s after 4 s");
    }
}
