//! The P0 primitive scene: a straight steep ramp into a flat floor, walled into a tray.
//!
//! One trimesh (`FIX_INTERNAL_EDGES`) built from a shared vertex list so the ramp, floor and
//! walls meet at exact shared vertices. Every triangle faces into the tray.

use parble_core::contract::{CELL_UM, CHANNEL_WIDTH_UM, MARBLE_STANDARD_DIAMETER_UM};
use rapier3d::prelude::*;

/// Micrometres to metres (the sim works in metres, the contract in integer µm).
pub fn um_to_m(um: i32) -> f32 {
    um as f32 * 1.0e-6
}

pub fn cell_m() -> f32 {
    um_to_m(CELL_UM)
}

pub fn standard_marble_radius_m() -> f32 {
    um_to_m(MARBLE_STANDARD_DIAMETER_UM / 2)
}

/// Ramp drop: 2 cells.
pub fn ramp_height_m() -> f32 {
    2.0 * cell_m()
}

/// Ramp run: 4 cells (50 % gradient, the contract's steep nominal slope).
pub fn ramp_run_m() -> f32 {
    4.0 * cell_m()
}

/// Tray end wall x: ramp run plus 8 cells of flat floor.
pub fn tray_length_m() -> f32 {
    ramp_run_m() + 8.0 * cell_m()
}

/// Tray inner width: the 28 mm channel class.
pub fn tray_width_m() -> f32 {
    um_to_m(CHANNEL_WIDTH_UM)
}

/// Wall height above the floor profile: 1 cell.
pub fn wall_height_m() -> f32 {
    cell_m()
}

/// Glass, kg/m³.
pub const GLASS_DENSITY: f32 = 2_500.0;
pub const GLASS_FRICTION: f32 = 0.4;
pub const GLASS_RESTITUTION: f32 = 0.6;
pub const WOOD_FRICTION: f32 = 0.5;
pub const WOOD_RESTITUTION: f32 = 0.4;

/// Vertex and index buffers of the tray trimesh.
pub fn tray_mesh() -> (Vec<Vector>, Vec<[u32; 3]>) {
    let h = ramp_height_m();
    let a = ramp_run_m();
    let l = tray_length_m();
    let half_w = tray_width_m() / 2.0;
    let wall = wall_height_m();
    // Floor profile in XY: top of ramp, ramp foot, end of floor.
    let profile = [(0.0_f32, h), (a, 0.0), (l, 0.0)];
    let zs = [-half_w, half_w];

    let mut vertices = Vec::with_capacity(profile.len() * 4);
    for &(x, y) in &profile {
        for &z in &zs {
            vertices.push(Vector::new(x, y, z));
            vertices.push(Vector::new(x, y + wall, z));
        }
    }
    // Index of profile point `i`, side `s` (0 = −Z, 1 = +Z), `top` (0 = floor, 1 = wall top).
    let v = |i: usize, s: usize, top: usize| (i * 4 + s * 2 + top) as u32;

    let mut tris = Vec::new();
    let mut quad = |q: [u32; 4], inward: Vector| {
        for t in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
            let [p0, p1, p2] = t.map(|i| vertices[i as usize]);
            let n = (p1 - p0).cross(p2 - p0);
            tris.push(if n.dot(inward) < 0.0 {
                [t[0], t[2], t[1]]
            } else {
                t
            });
        }
    };
    for i in 0..profile.len() - 1 {
        // Running surface.
        quad(
            [v(i, 0, 0), v(i + 1, 0, 0), v(i + 1, 1, 0), v(i, 1, 0)],
            Vector::Y,
        );
        // Side walls.
        quad(
            [v(i, 0, 0), v(i + 1, 0, 0), v(i + 1, 0, 1), v(i, 0, 1)],
            Vector::Z,
        );
        quad(
            [v(i, 1, 0), v(i + 1, 1, 0), v(i + 1, 1, 1), v(i, 1, 1)],
            -Vector::Z,
        );
    }
    let last = profile.len() - 1;
    // Back wall at the top of the ramp, end wall at the far end of the floor.
    quad([v(0, 0, 0), v(0, 1, 0), v(0, 1, 1), v(0, 0, 1)], Vector::X);
    quad(
        [v(last, 0, 0), v(last, 1, 0), v(last, 1, 1), v(last, 0, 1)],
        -Vector::X,
    );
    (vertices, tris)
}

/// The static tray collider.
pub fn tray_collider() -> Collider {
    let (vertices, indices) = tray_mesh();
    ColliderBuilder::trimesh_with_flags(vertices, indices, TriMeshFlags::FIX_INTERNAL_EDGES)
        .expect("tray trimesh is manifold and consistently oriented")
        .friction(WOOD_FRICTION)
        .restitution(WOOD_RESTITUTION)
        .build()
}

/// Spawn point of the marble: near the back wall, just above the ramp surface.
pub fn marble_spawn_m() -> Vector {
    let r = standard_marble_radius_m();
    let x = r + 0.004;
    let surface_y = ramp_height_m() * (1.0 - x / ramp_run_m());
    // Touching height is r / cos θ ≈ 1.118 r on a 50 % slope, so this drops the marble ~1 mm.
    Vector::new(x, surface_y + 1.25 * r, 0.0)
}

/// Resting point of the close-up marble: on the flat floor, 4 cells past the ramp foot.
pub fn closeup_marble_m(radius: f32) -> Vector {
    Vector::new(ramp_run_m() + 4.0 * cell_m(), radius, 0.0)
}

/// A standard 16 mm glass marble body (CCD on, never sleeps) and its collider, with the
/// [`crate::marble::P0_LEGACY`] parameters.
pub fn standard_marble(at: Vector) -> (RigidBody, Collider) {
    crate::marble::P0_LEGACY.build(at)
}
