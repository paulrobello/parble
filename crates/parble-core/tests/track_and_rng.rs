use parble_core::{
    CellCoord, HopperConfig, KIT_CONTRACT_VERSION, MarbleSize, Mirror, PHYSICS_VERSION, PieceDef,
    PieceId, Placement, Port, PortClass, PortFace, PortFlow, PortLevel, ReleaseMode,
    SCHEMA_VERSION, SeededRng, Track, Yaw4,
};

fn id(s: &str) -> PieceId {
    PieceId::new(s).unwrap()
}

fn sample_track() -> Track {
    let mut t = Track::new(u64::MAX - 7);
    t.placements = vec![
        Placement::new(
            id("mech_hopper"),
            CellCoord::new(0, 4, 0),
            Yaw4::new(0),
            Mirror::Off,
        ),
        Placement::new(
            id("blk_curve90_l3"),
            CellCoord::new(-3, 2, 7),
            Yaw4::new(3),
            Mirror::On,
        ),
        Placement::new(
            id("blk_slope_gentle_l0"),
            CellCoord::new(1, -1, -2),
            Yaw4::new(2),
            Mirror::Off,
        ),
    ];
    t.hopper = HopperConfig {
        placement: Some(0),
        queue: vec![MarbleSize::Standard, MarbleSize::Mini, MarbleSize::Shooter],
        release: ReleaseMode::Interval { ticks: 240 },
    };
    t
}

#[test]
fn version_constants_are_set() {
    assert_eq!(SCHEMA_VERSION, 1);
    assert_eq!(PHYSICS_VERSION, 4);
    assert_eq!(KIT_CONTRACT_VERSION, 1);
}

#[test]
fn new_track_stamps_versions() {
    let t = Track::new(42);
    assert_eq!(t.schema, SCHEMA_VERSION);
    assert_eq!(t.physics, PHYSICS_VERSION);
    assert_eq!(t.kit_contract, KIT_CONTRACT_VERSION);
    assert_eq!(t.seed, 42);
    assert!(t.placements.is_empty());
}

#[test]
fn track_json_round_trip_is_lossless() {
    let t = sample_track();
    let json = serde_json::to_string_pretty(&t).unwrap();
    let back: Track = serde_json::from_str(&json).unwrap();
    assert_eq!(back, t);
    // Re-serialising gives the same bytes.
    assert_eq!(serde_json::to_string_pretty(&back).unwrap(), json);
}

#[test]
fn track_json_uses_stable_field_shapes() {
    let json = serde_json::to_string(&sample_track()).unwrap();
    assert!(json.contains("\"anchor\":[-3,2,7]"), "{json}");
    assert!(json.contains("\"yaw\":3"), "{json}");
    assert!(json.contains("\"mirror\":true"), "{json}");
    assert!(
        json.contains("\"release\":{\"mode\":\"interval\",\"ticks\":240}"),
        "{json}"
    );
    assert!(
        json.contains("\"queue\":[\"standard\",\"mini\",\"shooter\"]"),
        "{json}"
    );
}

#[test]
fn track_json_rejects_out_of_range_yaw_and_bad_piece_id() {
    let good = serde_json::to_string(&sample_track()).unwrap();
    let bad_yaw = good.replace("\"yaw\":3", "\"yaw\":7");
    assert!(serde_json::from_str::<Track>(&bad_yaw).is_err());
    let bad_id = good.replace("blk_curve90_l3", "curve90");
    assert!(serde_json::from_str::<Track>(&bad_id).is_err());
}

#[test]
fn placement_world_cells_offset_by_anchor() {
    let def = PieceDef {
        id: id("rail_curve90_r90_l2"),
        chiral: true,
        footprint: vec![
            CellCoord::new(0, 0, 0),
            CellCoord::new(1, 0, 0),
            CellCoord::new(0, 0, 1),
            CellCoord::new(1, 0, 1),
        ],
        ports: vec![Port::side(
            CellCoord::ORIGIN,
            PortFace::NegX,
            PortLevel::L2,
            PortClass::Channel,
            PortFlow::Both,
        )],
    };
    let p = Placement::new(
        def.id.clone(),
        CellCoord::new(10, 3, 10),
        Yaw4::new(1),
        Mirror::Off,
    );
    assert_eq!(
        p.world_cells(&def),
        vec![
            CellCoord::new(10, 3, 9),
            CellCoord::new(10, 3, 10),
            CellCoord::new(11, 3, 9),
            CellCoord::new(11, 3, 10),
        ]
    );
}

fn take(rng: &mut parble_core::SubStream, n: usize) -> Vec<u64> {
    (0..n).map(|_| rng.next_u64()).collect()
}

#[test]
fn substream_same_seed_and_name_is_deterministic() {
    let a = take(&mut SeededRng::new(7).stream("splitter/12"), 16);
    let b = take(&mut SeededRng::new(7).stream("splitter/12"), 16);
    assert_eq!(a, b);
}

#[test]
fn substreams_with_different_names_differ() {
    let root = SeededRng::new(7);
    assert_ne!(
        take(&mut root.stream("splitter/12"), 8),
        take(&mut root.stream("splitter/13"), 8)
    );
    assert_ne!(
        take(&mut root.stream("a"), 8),
        take(&mut root.stream("b"), 8)
    );
}

#[test]
fn substreams_with_different_seeds_differ() {
    assert_ne!(
        take(&mut SeededRng::new(1).stream("x"), 8),
        take(&mut SeededRng::new(2).stream("x"), 8)
    );
}

#[test]
fn substream_is_independent_of_creation_order_and_sibling_draws() {
    let root = SeededRng::new(99);
    let alone = take(&mut root.stream("b"), 8);

    let mut a = root.stream("a");
    let _ = take(&mut a, 1000);
    let mut b = root.stream("b");
    let _ = take(&mut a, 1000);
    assert_eq!(take(&mut b, 8), alone);
}

#[test]
fn substream_golden_values() {
    // Pins the derivation (splitmix64 key + FNV-1a stream id over ChaCha8). A change here
    // changes every seeded run: bump PHYSICS_VERSION.
    let got = take(&mut SeededRng::new(0x5EED).stream("golden"), 4);
    assert_eq!(got, GOLDEN);
}

const GOLDEN: [u64; 4] = [
    16_589_852_132_465_626_343,
    2_069_644_067_978_860_110,
    11_605_491_000_381_437_618,
    3_841_449_556_148_423_331,
];

#[test]
fn below_stays_in_range_and_is_deterministic() {
    let root = SeededRng::new(3);
    let mut s = root.stream("dice");
    let draws: Vec<u32> = (0..1000).map(|_| s.below(6)).collect();
    assert!(draws.iter().all(|&d| d < 6));
    for face in 0..6 {
        assert!(draws.contains(&face));
    }
    let mut again = root.stream("dice");
    let draws2: Vec<u32> = (0..1000).map(|_| again.below(6)).collect();
    assert_eq!(draws, draws2);
}
