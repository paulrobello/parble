//! Standalone repro of a Parry 0.31.1 point-projection false positive on a thin convex hull, for
//! an upstream report. The hull is the one-way gate flap's collider (34 mm plate, 2 mm thick, with
//! a 3 mm pin at the top), in its body frame (origin at the hinge), before the sim switched the
//! flap to a trimesh. The query point is the marble centre in that frame at tick 151 of a 0.13 m/s
//! run (measured 2026-11-08). `ConvexPolyhedron::project_local_point_and_get_feature` reports it
//! inside, while it lies 7.77 mm in front of one of the hull's face planes, so it is outside.
//! Rapier's ball contact then takes the far face's normal (dist -16.27 mm) and pushes the marble
//! into the flap. If this test fails, Parry has fixed it.
use rapier3d::parry::query::PointQuery;
use rapier3d::parry::shape::ConvexPolyhedron;
use rapier3d::prelude::Vector;

/// Hull points, f32 bit patterns of metres (x, y, z). In millimetres:
/// (-1, -22, -12), (-1, -26.592, -11.087), (-1, -30.485, -8.485), (-1, -33.087, -4.592), (-1, -34, 0)
/// (-1, -33.087, 4.592), (-1, -30.485, 8.485), (-1, -26.592, 11.087), (-1, -22, 12), (-1, 0, 12)
/// (-1, 0, -12), (1, -22, -12), (1, -26.592, -11.087), (1, -30.485, -8.485), (1, -33.087, -4.592)
/// (1, -34, 0), (1, -33.087, 4.592), (1, -30.485, 8.485), (1, -26.592, 11.087), (1, -22, 12)
/// (1, 0, 12), (1, 0, -12), (-1.5, -1.5, -14), (-1.5, -1.5, 14), (-1.5, 1.5, -14)
/// (-1.5, 1.5, 14), (1.5, -1.5, -14), (1.5, -1.5, 14), (1.5, 1.5, -14), (1.5, 1.5, 14)
const POINTS: [[u32; 3]; 30] = [
    [0xba83126f, 0xbcb43958, 0xbc449ba6],
    [0xba83126f, 0xbcd9d777, 0xbc35a640],
    [0xba83126f, 0xbcf9bbae, 0xbc0b04ab],
    [0xba83126f, 0xbd07863c, 0xbb96787d],
    [0xba83126f, 0xbd0b4396, 0x00000000],
    [0xba83126f, 0xbd07863c, 0x3b96787d],
    [0xba83126f, 0xbcf9bbae, 0x3c0b04ab],
    [0xba83126f, 0xbcd9d777, 0x3c35a640],
    [0xba83126f, 0xbcb43958, 0x3c449ba6],
    [0xba83126f, 0x00000000, 0x3c449ba6],
    [0xba83126f, 0x00000000, 0xbc449ba6],
    [0x3a83126f, 0xbcb43958, 0xbc449ba6],
    [0x3a83126f, 0xbcd9d777, 0xbc35a640],
    [0x3a83126f, 0xbcf9bbae, 0xbc0b04ab],
    [0x3a83126f, 0xbd07863c, 0xbb96787d],
    [0x3a83126f, 0xbd0b4396, 0x00000000],
    [0x3a83126f, 0xbd07863c, 0x3b96787d],
    [0x3a83126f, 0xbcf9bbae, 0x3c0b04ab],
    [0x3a83126f, 0xbcd9d777, 0x3c35a640],
    [0x3a83126f, 0xbcb43958, 0x3c449ba6],
    [0x3a83126f, 0x00000000, 0x3c449ba6],
    [0x3a83126f, 0x00000000, 0xbc449ba6],
    [0xbac49ba6, 0xbac49ba6, 0xbc656042],
    [0xbac49ba6, 0xbac49ba6, 0x3c656042],
    [0xbac49ba6, 0x3ac49ba6, 0xbc656042],
    [0xbac49ba6, 0x3ac49ba6, 0x3c656042],
    [0x3ac49ba6, 0xbac49ba6, 0xbc656042],
    [0x3ac49ba6, 0xbac49ba6, 0x3c656042],
    [0x3ac49ba6, 0x3ac49ba6, 0xbc656042],
    [0x3ac49ba6, 0x3ac49ba6, 0x3c656042],
];

/// Query point, f32 bits: (-8.898884, -26.585855, 0.011433) mm.
const QUERY: [u32; 3] = [0xbc11cca0, 0xbcd9ca94, 0x373fd094];

fn v(b: [u32; 3]) -> Vector {
    Vector::new(
        f32::from_bits(b[0]),
        f32::from_bits(b[1]),
        f32::from_bits(b[2]),
    )
}

#[test]
fn thin_hull_projection_reports_an_outside_point_inside() {
    let hull = ConvexPolyhedron::from_convex_hull(&POINTS.map(v)).unwrap();
    let q = v(QUERY);
    let (pts, adj) = (hull.points(), hull.vertices_adj_to_face());
    let separation = hull
        .faces()
        .iter()
        .map(|f| {
            f.normal
                .dot(q - pts[adj[f.first_vertex_or_edge as usize] as usize])
        })
        .fold(f32::MIN, f32::max);
    assert!(separation > 0.0077, "outside by {separation} m");
    let (proj, _) = hull.project_local_point_and_get_feature(q);
    assert!(
        proj.is_inside,
        "Parry no longer reports the point inside: {proj:?}"
    );
    // The projected point itself is the true closest point, 8.27 mm away.
    assert!(((q - proj.point).length() - 0.008_265).abs() < 1e-5);
}
