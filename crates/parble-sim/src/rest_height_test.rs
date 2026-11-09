//! Rest heights at the exact channel contacts' hand-overs.
//!
//! A centred marble at rest on the two facets beside a straight's bottom crease (the faceted
//! contact every exact channel contact hands over to or from) is the reference. Every exact
//! channel contact must hold the same marble at that height: the exact straight tube, the bend's
//! and the slope's level continuations past their ports, a fork's and the joiner's inlet, a fork's
//! outlet and the redirect's landing floor at its exit. A step between two of them is a step in
//! the floor the marble rolls over: 34 µm at the exact straight's hand-over to the facets needed
//! about 22 mm/s to climb, and slow steel marbles stalled there on the all-pieces track.
//!
//! [`TOLERANCE_M`] is 2 µm: the sidecar rounds every vertex to 1 µm, a centred marble's rest
//! height in `f32` at y ≈ 0.06 m resolves to under 0.01 µm, and the solver settles it to under
//! 0.2 µm of the soft-contact model ([`crate::chunk::CREASE_SINK_COMPENSATION_M`]). A 2 µm
//! step is climbed from about 5 mm/s (rolling, `v = sqrt(10 g h / 7)`).

use crate::chunk::ChunkMesh;
use crate::marble::{MarbleKind, MarbleSize, MarbleSpec};
use crate::{MarbleState, Sim};
use parble_core::{CellCoord, PieceId, Placement, Yaw4};
use rapier3d::prelude::*;

/// Largest rest height difference between two contacts at a hand-over, metres.
const TOLERANCE_M: f32 = 2.0e-6;

/// Largest speed of a marble counted as at rest, m/s: on a level channel the height does not
/// depend on where along it the marble is, and at this speed it creeps 20 µm in the settle.
const REST_SPEED_M_S: f32 = 1.0e-5;

/// Ticks to settle (2 s): a marble placed 10 µm over its rest is still within 0.01 µm after 1 s.
const SETTLE_TICKS: u32 = 480;

fn place(piece: &str, x: i32, y: i32, z: i32, yaw: u8) -> Placement {
    Placement::new(
        PieceId::new(piece).unwrap(),
        CellCoord::new(x, y, z),
        Yaw4::new(yaw),
        false.into(),
    )
}

/// The marble `spec` set down at rest at `at` on `layout`, after [`SETTLE_TICKS`]. With
/// `facets` the straights keep their mesh contacts (`ChunkColliders::no_straight_contact`).
fn rest_at(layout: &[Placement], spec: MarbleSpec, at: Vector, facets: bool) -> MarbleState {
    let kit = parble_kit::builtin();
    let mut sim = Sim::empty();
    sim.pieces.no_straight_contact = facets;
    for p in layout {
        sim.place_piece(kit, p.clone()).unwrap();
    }
    sim.rebuild_chunks(kit).unwrap();
    let (body, collider) = spec.build(at);
    let h = sim.world.bodies.insert(body);
    sim.world
        .colliders
        .insert_with_parent(collider, h, &mut sim.world.bodies);
    sim.marbles.push(h);
    sim.tick(SETTLE_TICKS);
    sim.marbles()[0]
}

/// The marble `spec` set down at `at` on the two facets beside a straight's bottom crease at
/// `level`, each one triangle wide past the marble (the facets' planes, 16 segments over the
/// half-round, contract §5.4), so it touches exactly one triangle per facet: the clean faceted
/// contact, without the extra contacts a triangle edge under the marble adds.
fn rest_on_crease(level: u8, spec: MarbleSpec, at: Vector) -> MarbleState {
    let axis_um = i64::from(level) * 15_000 + 14_000;
    let half_seg = core::f64::consts::PI / 32.0;
    // The crease line and, for each side, one far point on that facet's plane: the facet runs
    // from the crease (angle 0) to the next vertex (angle 2·half_seg) of the 14 mm half-round.
    let crease = |x: i64| [x, axis_um - 14_000, 0];
    let far = |sign: i64| {
        let a = 2.0 * half_seg;
        let k = 4.0;
        let dz = 14_000.0 * libm::sin(a) * k;
        let dy = 14_000.0 * (1.0 - libm::cos(a)) * k;
        [
            0,
            axis_um - 14_000 + libm::round(dy) as i64,
            sign * libm::round(dz) as i64,
        ]
    };
    let l = 500_000;
    let tris = vec![
        [crease(-l), far(1), crease(l)],
        [crease(-l), crease(l), far(-1)],
    ];
    let mut sim = Sim::empty();
    sim.world.colliders.insert(
        ChunkMesh::from_triangles(&tris)
            .collider([0, 0, 0])
            .unwrap(),
    );
    let (body, collider) = spec.build(at);
    let h = sim.world.bodies.insert(body);
    sim.world
        .colliders
        .insert_with_parent(collider, h, &mut sim.world.bodies);
    sim.marbles.push(h);
    sim.tick(SETTLE_TICKS);
    sim.marbles()[0]
}

/// Five `L{level}` straights along +x in cells 0..=4 of layer 0, and `extra`.
fn straights(level: u8, cells: std::ops::Range<i32>, extra: &[Placement]) -> Vec<Placement> {
    let mut layout: Vec<Placement> = cells
        .map(|x| place(&format!("blk_straight_l{level}"), x, 0, 0, 0))
        .collect();
    layout.extend_from_slice(extra);
    layout
}

/// A hand-over case: the contact's name, the layout, and the marble centre's plan point for a
/// marble of radius `r`.
struct Case {
    name: &'static str,
    level: u8,
    layout: Vec<Placement>,
    at: fn(f32) -> (f32, f32),
}

fn cases() -> Vec<Case> {
    let fork = |id: &str| {
        straights(
            3,
            0..4,
            &[
                place(id, 4, 0, 0, 0),
                place("blk_straight_l3", 4, 0, 1, 1),
                place("blk_straight_l3", 4, 0, -1, 1),
            ],
        )
    };
    vec![
        Case {
            name: "exact straight",
            level: 3,
            layout: straights(3, 0..5, &[]),
            at: |_| (0.12, 0.0),
        },
        Case {
            name: "exact straight L1",
            level: 1,
            layout: straights(1, 0..5, &[]),
            at: |_| (0.12, 0.0),
        },
        Case {
            name: "bend continuation",
            level: 3,
            layout: straights(3, 0..4, &[place("blk_curve90_l3", 4, 0, 0, 1)]),
            at: |r| (0.21 - r, 0.0),
        },
        Case {
            name: "slope continuation",
            level: 3,
            layout: straights(3, 0..4, &[place("blk_slope_gentle_l3", 4, 0, 0, 0)]),
            at: |r| (0.21 - r, 0.0),
        },
        Case {
            name: "splitter inlet",
            level: 3,
            layout: fork("mech_splitter_l3"),
            at: |_| (0.211, 0.0),
        },
        Case {
            name: "toggle inlet",
            level: 3,
            layout: fork("mech_toggle_l3"),
            at: |_| (0.211, 0.0),
        },
        Case {
            name: "splitter outlet",
            level: 3,
            layout: fork("mech_splitter_l3"),
            at: |_| (0.24, -0.027),
        },
        Case {
            name: "joiner inlet",
            level: 3,
            layout: straights(3, 0..4, &[place("blk_join_l3", 4, 0, 0, 0)]),
            at: |_| (0.213, 0.0),
        },
        Case {
            name: "redirect landing exit",
            level: 3,
            layout: vec![
                place("blk_redirect_l3", 0, 1, 0, 0),
                place("blk_straight_l3", 1, 0, 0, 0),
                place("blk_straight_l3", 2, 0, 0, 0),
            ],
            at: |_| (0.027, 0.0),
        },
    ]
}

/// Rest height of a centred marble of each size on each exact channel contact against the
/// faceted crease of a straight of the same level, every pair within [`TOLERANCE_M`].
#[test]
fn every_exact_channel_contact_rests_a_centred_marble_at_the_facets_height() {
    let mut bad = Vec::new();
    let mut table = Vec::new();
    for size in MarbleSize::ALL {
        let spec = MarbleKind::CatsEye.spec_sized(size);
        let r = spec.radius_m;
        for case in cases() {
            let floor = 0.015 * f32::from(case.level);
            let start = floor + r + 1.0e-5;
            let reference = rest_on_crease(case.level, spec, Vector::new(0.0, start, 0.0));
            let (x, z) = (case.at)(r);
            let s = rest_at(&case.layout, spec, Vector::new(x, start + 4.0e-5, z), false);
            let d = s.position[1] - reference.position[1];
            let v = Vector::from(s.linvel).length();
            table.push(format!(
                "{:?} {}: facets {:.6} contact {:.6} diff {:+.2} µm, |v| {v:.1e}",
                size,
                case.name,
                reference.position[1],
                s.position[1],
                d * 1.0e6
            ));
            if d.abs() > TOLERANCE_M || v > REST_SPEED_M_S {
                bad.push(format!(
                    "{:?} {}: {:+.2} µm, |v| {v:.1e}",
                    size,
                    case.name,
                    d * 1.0e6
                ));
            }
        }
    }
    eprintln!("{}", table.join("\n"));
    assert!(
        bad.is_empty(),
        "rest height steps over 2 µm:\n{}",
        bad.join("\n")
    );
}
