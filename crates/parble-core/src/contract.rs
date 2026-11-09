//! Kit contract dimensions in integer micrometres (`docs/kit-contract.md`).
//!
//! Changing any collision or port value here is a contract change: bump [`KIT_CONTRACT_VERSION`],
//! rebuild the catalog and bump [`crate::PHYSICS_VERSION`]. The render-only values
//! ([`VISUAL_SEAM_INSET_UM`], [`BLOCK_EDGE_BEVEL_UM`], the lip fillets) change only the GLBs and
//! baked maps: regenerate the kit, keep both versions and every sidecar byte (§10).

/// Version of `docs/kit-contract.md` these constants implement.
pub const KIT_CONTRACT_VERSION: u32 = 1;

// §2 Cell and chunk.
pub const CELL_UM: i32 = 60_000;
pub const HALF_CELL_UM: i32 = CELL_UM / 2;
pub const CHUNK_CELLS: i32 = 8;
pub const CHUNK_UM: i32 = CHUNK_CELLS * CELL_UM;
pub const VISUAL_SEAM_INSET_UM: i32 = 200;
pub const COLLISION_SEAM_INSET_UM: i32 = 0;

// §3 Port levels and tolerances.
pub const LEVEL_STEP_UM: i32 = 15_000;
pub const LEVEL_COUNT: i32 = 4;
pub const WELD_EPSILON_UM: i32 = 1;
pub const RENDER_PORT_TOLERANCE_UM: i32 = 100;

// §4 Vertical drop port.
pub const DROP_HOLE_DIAMETER_UM: i32 = 32_000;
pub const DROP_BOWL_DIAMETER_UM: i32 = 36_000;
pub const DROP_RIM_CHAMFER_UM: i32 = 2_000;

// §5.1 Channel class.
pub const CHANNEL_WIDTH_UM: i32 = 28_000;
pub const CHANNEL_RADIUS_UM: i32 = 14_000;
pub const OVERHEAD_CLEARANCE_UM: i32 = 34_000;
pub const TUNNEL_HEIGHT_UM: i32 = 34_000;

// §5.2 Block family.
pub const BLOCK_SIDE_WALL_UM: i32 = 16_000;
pub const BLOCK_MIN_WOOD_UNDER_FLOOR_UM: i32 = 6_000;
pub const BLOCK_LIP_RADIUS_UM: i32 = 3_000;
pub const BLOCK_EDGE_BEVEL_UM: i32 = 400;

// §5.3 Rail family.
pub const RAIL_WALL_UM: i32 = 4_000;
pub const RAIL_OUTER_WIDTH_UM: i32 = 36_000;
pub const RAIL_LIP_HEIGHT_UM: i32 = 18_000;
pub const RAIL_LIP_RADIUS_UM: i32 = 2_000;

// §5.5 Rail-to-block joint.
pub const SOCKET_BORE_DIAMETER_UM: i32 = 40_000;
pub const SOCKET_DEPTH_UM: i32 = 10_500;
pub const SPIGOT_LENGTH_UM: i32 = 10_000;
/// `inset_um` of socket and spigot ports: the collision port plane sits 10 mm inside the socket face.
pub const SOCKET_PORT_INSET_UM: i32 = 10_000;

// §6 Marbles and size gates (diameters).
pub const MARBLE_MINI_DIAMETER_UM: i32 = 10_000;
pub const MARBLE_STANDARD_DIAMETER_UM: i32 = 16_000;
pub const MARBLE_SHOOTER_DIAMETER_UM: i32 = 25_000;
pub const MARBLE_MAX_DIAMETER_UM: i32 = MARBLE_SHOOTER_DIAMETER_UM;
pub const GATE_MINI_DIAMETER_UM: i32 = 12_000;
pub const GATE_STANDARD_DIAMETER_UM: i32 = 19_000;

// §9 Footprint limits.
pub const MAX_FOOTPRINT_PLAN_CELLS: i32 = 4;
pub const MAX_FOOTPRINT_LAYERS: i32 = 8;

// §10 Piece naming.
pub const MAX_PIECE_ID_LEN: usize = 48;

// §5.4 Collision mesh.
pub const COLLISION_HALF_ROUND_SEGMENTS: usize = 16;
pub const RING_VERTEX_COUNT: usize = COLLISION_HALF_ROUND_SEGMENTS + 1;

/// Canonical collision port ring (§5.4): `(u, v − floor)` in µm for `θ = −90° … +90°`
/// in 11.25° steps, `u = 14000 sin θ`, `v = floor + 14000 − 14000 cos θ`, rounded half
/// away from zero. The two wall-top vertices are family-specific and not included.
pub const CANONICAL_RING_UM: [(i32, i32); RING_VERTEX_COUNT] = [
    (-14_000, 14_000),
    (-13_731, 11_269),
    (-12_934, 8_642),
    (-11_641, 6_222),
    (-9_899, 4_101),
    (-7_778, 2_359),
    (-5_358, 1_066),
    (-2_731, 269),
    (0, 0),
    (2_731, 269),
    (5_358, 1_066),
    (7_778, 2_359),
    (9_899, 4_101),
    (11_641, 6_222),
    (12_934, 8_642),
    (13_731, 11_269),
    (14_000, 14_000),
];
