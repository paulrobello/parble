use parble_core::{
    CONTROLLED_DROP_MAX_FALL_UM, CellCoord, ControlledDrop, Issue, IssueKind, LinkFault,
    MapCatalog, Mirror, PieceDef, PieceId, Placement, Port, PortClass, PortFace, PortFlow,
    PortLevel, Track, Yaw4, controlled_drops, validate_track,
};

fn c(x: i32, y: i32, z: i32) -> CellCoord {
    CellCoord::new(x, y, z)
}

fn ch(cell: CellCoord, face: PortFace, level: PortLevel, flow: PortFlow) -> Port {
    Port::side(cell, face, level, PortClass::Channel, flow)
}

fn def(id: &str, chiral: bool, footprint: Vec<CellCoord>, ports: Vec<Port>) -> PieceDef {
    PieceDef {
        id: PieceId::new(id).unwrap(),
        chiral,
        footprint,
        ports,
    }
}

const O: CellCoord = CellCoord::ORIGIN;

/// The small test catalog. Every definition passes the §9 footprint rules.
fn catalog() -> MapCatalog {
    use PortFace::*;
    use PortFlow::*;
    use PortLevel::*;
    let defs = vec![
        def("mech_hopper", false, vec![O], vec![ch(O, PosX, L3, Out)]),
        def("mech_hopper_l1", false, vec![O], vec![ch(O, PosX, L1, Out)]),
        def("mech_finish", false, vec![O], vec![Port::drop(O, Top, In)]),
        def(
            "blk_straight_l1",
            false,
            vec![O],
            vec![ch(O, NegX, L1, Both), ch(O, PosX, L1, Both)],
        ),
        def(
            "blk_straight_l3",
            false,
            vec![O],
            vec![ch(O, NegX, L3, Both), ch(O, PosX, L3, Both)],
        ),
        def(
            "blk_straight_l2",
            false,
            vec![O],
            vec![ch(O, NegX, L2, Both), ch(O, PosX, L2, Both)],
        ),
        def(
            "blk_curve90_l3",
            true,
            vec![O],
            vec![ch(O, NegX, L3, Both), ch(O, PosZ, L3, Both)],
        ),
        def(
            "blk_slope_gentle_l3",
            false,
            vec![O],
            vec![ch(O, NegX, L3, In), ch(O, PosX, L2, Out)],
        ),
        def(
            "rail_curve90_r90_l2",
            true,
            vec![c(0, 0, 0), c(1, 0, 0), c(0, 0, 1), c(1, 0, 1)],
            vec![
                ch(c(0, 0, 0), NegX, L2, Both),
                ch(c(1, 0, 1), PosZ, L2, Both),
            ],
        ),
        def(
            "blk_drop_l2",
            false,
            vec![O],
            vec![ch(O, NegX, L2, In), Port::drop(O, Bottom, Out)],
        ),
        def(
            "blk_tower_socket_l3",
            false,
            vec![O],
            vec![Port::side(O, NegX, L3, PortClass::Socket, In)],
        ),
        def(
            "mech_gate_oneway_l3",
            false,
            vec![O],
            vec![ch(O, NegX, L3, In), ch(O, PosX, L3, Out)],
        ),
        // Deliberately misnamed: suffix says L1, port 0 is L2.
        def(
            "blk_bad_l1",
            false,
            vec![O],
            vec![ch(O, NegX, L2, Both), ch(O, PosX, L2, Both)],
        ),
    ];
    for d in &defs {
        d.validate().unwrap_or_else(|e| panic!("{}: {e:?}", d.id));
    }
    defs.into_iter().collect()
}

fn place(id: &str, anchor: CellCoord, yaw: u8) -> Placement {
    Placement::new(
        PieceId::new(id).unwrap(),
        anchor,
        Yaw4::new(yaw),
        Mirror::Off,
    )
}

fn track(placements: Vec<Placement>) -> Track {
    let mut t = Track::new(7);
    t.placements = placements;
    t
}

fn kinds(issues: &[Issue]) -> Vec<&IssueKind> {
    issues.iter().map(|i| &i.kind).collect()
}

/// Hopper → straight → curve → slope → 2×2 rail curve → drop → finish (drop into its top).
fn clean_placements() -> Vec<Placement> {
    vec![
        place("mech_hopper", c(0, 1, 0), 0),
        place("blk_straight_l3", c(1, 1, 0), 0),
        place("blk_curve90_l3", c(2, 1, 0), 0),
        // Yaw 3 maps −X → −Z and +X → +Z: enters from the curve, exits toward +Z at L2.
        place("blk_slope_gentle_l3", c(2, 1, 1), 3),
        // Yaw 3 maps (x, z) → (−z, x): cells (2,1,2) (2,1,3) (1,1,2) (1,1,3), exit −X at (1,1,3).
        place("rail_curve90_r90_l2", c(2, 1, 2), 3),
        // Yaw 2 maps −X → +X: side entry from the rail, drop out of the bottom.
        place("blk_drop_l2", c(0, 1, 3), 2),
        place("mech_finish", c(0, 0, 3), 0),
    ]
}

#[test]
fn clean_multi_piece_track_has_no_issues() {
    let cat = catalog();
    let t = track(clean_placements());

    // Pin the hand-derived geometry before trusting the validator.
    let world_port = |i: usize, j: usize| {
        let p = &t.placements[i];
        let d = cat.iter().find(|d| d.id == p.piece).unwrap();
        let port = d.ports[j].transformed(p.orientation());
        (port.cell + p.anchor, port.face, port.level)
    };
    assert_eq!(
        world_port(3, 0),
        (c(2, 1, 1), PortFace::NegZ, Some(PortLevel::L3))
    );
    assert_eq!(
        world_port(3, 1),
        (c(2, 1, 1), PortFace::PosZ, Some(PortLevel::L2))
    );
    assert_eq!(
        world_port(4, 1),
        (c(1, 1, 3), PortFace::NegX, Some(PortLevel::L2))
    );
    assert_eq!(
        world_port(5, 0),
        (c(0, 1, 3), PortFace::PosX, Some(PortLevel::L2))
    );
    assert_eq!(world_port(5, 1), (c(0, 1, 3), PortFace::Bottom, None));
    let rail = &t.placements[4];
    let rail_def = cat.iter().find(|d| d.id == rail.piece).unwrap();
    assert_eq!(
        rail.world_cells(rail_def),
        vec![c(1, 1, 2), c(1, 1, 3), c(2, 1, 2), c(2, 1, 3)]
    );

    assert_eq!(validate_track(&t, &cat), vec![]);
}

#[test]
fn open_port_is_reported() {
    let cat = catalog();
    let t = track(vec![
        place("mech_hopper", c(0, 0, 0), 0),
        place("blk_straight_l3", c(1, 0, 0), 0),
    ]);
    let issues = validate_track(&t, &cat);
    assert!(issues.contains(&Issue {
        placement: Some(1),
        anchor: Some(c(1, 0, 0)),
        cells: vec![c(1, 0, 0)],
        kind: IssueKind::OpenPort {
            port: 1,
            face: PortFace::PosX,
            flow: PortFlow::Both,
        },
    }));
}

#[test]
fn open_drop_port_is_reported() {
    let cat = catalog();
    let t = track(vec![place("blk_drop_l2", c(0, 3, 0), 0)]);
    let issues = validate_track(&t, &cat);
    assert!(kinds(&issues).contains(&&IssueKind::OpenPort {
        port: 1,
        face: PortFace::Bottom,
        flow: PortFlow::Out,
    }));
}

#[test]
fn level_mismatch_is_reported_once_per_pair() {
    let cat = catalog();
    let t = track(vec![
        place("mech_hopper", c(0, 0, 0), 0),
        place("blk_straight_l2", c(1, 0, 0), 0),
    ]);
    let issues = validate_track(&t, &cat);
    let broken: Vec<&Issue> = issues
        .iter()
        .filter(|i| matches!(i.kind, IssueKind::BrokenLink { .. }))
        .collect();
    assert_eq!(
        broken,
        vec![&Issue {
            placement: Some(0),
            anchor: Some(c(0, 0, 0)),
            cells: vec![c(0, 0, 0), c(1, 0, 0)],
            kind: IssueKind::BrokenLink {
                port: 0,
                other: 1,
                other_port: Some(0),
                fault: LinkFault::LevelMismatch {
                    floor_um: 45_000,
                    other_floor_um: 30_000,
                },
            },
        }]
    );
}

#[test]
fn class_mismatch_is_reported() {
    let cat = catalog();
    let t = track(vec![
        place("mech_hopper", c(0, 0, 0), 0),
        place("blk_tower_socket_l3", c(1, 0, 0), 0),
    ]);
    let issues = validate_track(&t, &cat);
    assert!(kinds(&issues).contains(&&IssueKind::BrokenLink {
        port: 0,
        other: 1,
        other_port: Some(0),
        fault: LinkFault::ClassMismatch {
            class: PortClass::Channel,
            other_class: PortClass::Socket,
        },
    }));
}

#[test]
fn port_facing_a_solid_face_is_reported() {
    let cat = catalog();
    // Yaw 1 puts the straight's ports on ±Z, so the hopper's +X port meets a wall.
    let t = track(vec![
        place("mech_hopper", c(0, 0, 0), 0),
        place("blk_straight_l3", c(1, 0, 0), 1),
    ]);
    let issues = validate_track(&t, &cat);
    assert!(kinds(&issues).contains(&&IssueKind::BrokenLink {
        port: 0,
        other: 1,
        other_port: None,
        fault: LinkFault::NoMatingPort,
    }));
}

#[test]
fn drop_onto_piece_without_top_port_is_reported() {
    let cat = catalog();
    let t = track(vec![
        place("blk_drop_l2", c(0, 1, 0), 0),
        place("blk_straight_l2", c(0, 0, 0), 0),
    ]);
    let issues = validate_track(&t, &cat);
    assert!(kinds(&issues).contains(&&IssueKind::BrokenLink {
        port: 1,
        other: 1,
        other_port: None,
        fault: LinkFault::NoMatingPort,
    }));
}

#[test]
fn flow_conflict_is_reported() {
    let cat = catalog();
    // Two hoppers facing each other: Out meets Out.
    let t = track(vec![
        place("mech_hopper", c(0, 0, 0), 0),
        place("mech_hopper", c(1, 0, 0), 2),
    ]);
    let issues = validate_track(&t, &cat);
    assert!(kinds(&issues).contains(&&IssueKind::BrokenLink {
        port: 0,
        other: 1,
        other_port: Some(0),
        fault: LinkFault::FlowConflict {
            flow: PortFlow::Out,
            other_flow: PortFlow::Out,
        },
    }));
}

#[test]
fn overlap_is_reported() {
    let cat = catalog();
    let t = track(vec![
        place("blk_straight_l3", c(0, 0, 0), 0),
        place("rail_curve90_r90_l2", c(-1, 0, -1), 0),
    ]);
    let issues = validate_track(&t, &cat);
    assert!(issues.contains(&Issue {
        placement: Some(0),
        anchor: Some(c(0, 0, 0)),
        cells: vec![c(0, 0, 0)],
        kind: IssueKind::Overlap { other: 1 },
    }));
}

#[test]
fn unknown_piece_is_reported() {
    let cat = catalog();
    let t = track(vec![place("deco_gnome", c(4, 0, 4), 0)]);
    let issues = validate_track(&t, &cat);
    assert!(issues.contains(&Issue {
        placement: Some(0),
        anchor: Some(c(4, 0, 4)),
        cells: vec![c(4, 0, 4)],
        kind: IssueKind::UnknownPiece {
            piece: PieceId::new("deco_gnome").unwrap(),
        },
    }));
}

#[test]
fn level_suffix_mismatch_is_reported() {
    let cat = catalog();
    let t = track(vec![place("blk_bad_l1", c(0, 0, 0), 0)]);
    let issues = validate_track(&t, &cat);
    assert!(kinds(&issues).contains(&&IssueKind::LevelSuffixMismatch {
        suffix: PortLevel::L1,
        port0: Some(PortLevel::L2),
    }));
    // The rail id's `r90` segment is not a level suffix, and `_l2` matches its port 0.
    let clean = validate_track(&track(clean_placements()), &cat);
    assert!(
        !clean
            .iter()
            .any(|i| matches!(i.kind, IssueKind::LevelSuffixMismatch { .. }))
    );
}

#[test]
fn disconnected_piece_is_unreachable() {
    let cat = catalog();
    let mut p = clean_placements();
    p.push(place("blk_straight_l3", c(10, 0, 10), 0));
    let issues = validate_track(&track(p), &cat);
    assert!(issues.contains(&Issue {
        placement: Some(7),
        anchor: Some(c(10, 0, 10)),
        cells: vec![c(10, 0, 10)],
        kind: IssueKind::Unreachable,
    }));
}

#[test]
fn unreachable_finish_is_reported_once() {
    let cat = catalog();
    let t = track(vec![
        place("mech_hopper", c(0, 0, 0), 0),
        place("blk_straight_l3", c(1, 0, 0), 0),
        place("mech_finish", c(5, 0, 5), 0),
    ]);
    let issues = validate_track(&t, &cat);
    let finish: Vec<&IssueKind> = issues
        .iter()
        .filter(|i| i.placement == Some(2))
        .map(|i| &i.kind)
        .filter(|k| matches!(k, IssueKind::FinishUnreachable | IssueKind::Unreachable))
        .collect();
    assert_eq!(finish, vec![&IssueKind::FinishUnreachable]);
}

#[test]
fn reachability_follows_flow_direction() {
    let cat = catalog();
    // The one-way gate is placed backwards (yaw 2): its Out port faces the straight. The link is
    // legal (Out meets Both) but carries flow only from the gate, so the gate and the finish
    // beyond it are unreachable from the hopper.
    let t = track(vec![
        place("mech_hopper", c(0, 0, 0), 0),
        place("blk_straight_l3", c(1, 0, 0), 0),
        place("mech_gate_oneway_l3", c(2, 0, 0), 2),
        place("blk_straight_l3", c(3, 0, 0), 0),
    ]);
    let issues = validate_track(&t, &cat);
    assert!(
        !issues
            .iter()
            .any(|i| matches!(i.kind, IssueKind::BrokenLink { .. })),
        "{issues:?}"
    );
    let unreachable: Vec<Option<usize>> = issues
        .iter()
        .filter(|i| i.kind == IssueKind::Unreachable)
        .map(|i| i.placement)
        .collect();
    assert_eq!(unreachable, vec![Some(2), Some(3)]);
}

#[test]
fn missing_hopper_and_finish_are_track_level_issues() {
    let cat = catalog();
    let t = track(vec![place("blk_straight_l3", c(0, 0, 0), 0)]);
    let issues = validate_track(&t, &cat);
    let track_level: Vec<&Issue> = issues.iter().filter(|i| i.placement.is_none()).collect();
    assert_eq!(
        track_level,
        vec![
            &Issue {
                placement: None,
                anchor: None,
                cells: vec![],
                kind: IssueKind::MissingHopper,
            },
            &Issue {
                placement: None,
                anchor: None,
                cells: vec![],
                kind: IssueKind::MissingFinish,
            },
        ]
    );
    // Without a hopper nothing is flagged unreachable.
    assert!(!issues.iter().any(|i| i.kind == IssueKind::Unreachable));
}

#[test]
fn hopper_config_must_point_at_a_hopper() {
    let cat = catalog();
    let mut t = track(clean_placements());
    t.hopper.placement = Some(1);
    let issues = validate_track(&t, &cat);
    assert!(issues.contains(&Issue {
        placement: Some(1),
        anchor: Some(c(1, 1, 0)),
        cells: vec![c(1, 1, 0)],
        kind: IssueKind::HopperConfigInvalid { index: 1 },
    }));

    t.hopper.placement = Some(99);
    let issues = validate_track(&t, &cat);
    assert!(kinds(&issues).contains(&&IssueKind::HopperConfigInvalid { index: 99 }));

    // A valid config pointing at the hopper is clean.
    t.hopper.placement = Some(0);
    assert_eq!(validate_track(&t, &cat), vec![]);
}

#[test]
fn output_is_deterministic_and_sorted() {
    let cat = catalog();
    let t = track(vec![
        place("blk_bad_l1", c(9, 0, 9), 0),
        place("mech_hopper", c(0, 0, 0), 0),
        place("blk_straight_l2", c(1, 0, 0), 0),
        place("deco_gnome", c(3, 0, 3), 0),
        place("blk_straight_l3", c(9, 0, 9), 0),
        place("blk_drop_l2", c(5, 2, 5), 1),
    ]);
    let a = validate_track(&t, &cat);
    let b = validate_track(&t, &cat);
    assert_eq!(a, b);
    assert!(a.len() > 5, "{a:?}");
    assert!(
        a.windows(2).all(|w| w[0] < w[1]),
        "sorted and unique: {a:?}"
    );
}

/// An `L1` hopper exit 30 mm above the `L3` channel entry `landing` of the layer below, then on
/// to a finish.
fn drop_placements(landing: Placement) -> Vec<Placement> {
    vec![
        place("mech_hopper_l1", c(0, 1, 0), 0),
        landing,
        place("blk_slope_gentle_l3", c(2, 0, 0), 0),
        place("blk_drop_l2", c(3, 0, 0), 0),
        place("mech_finish", c(3, -1, 0), 0),
    ]
}

const HOPPER_EXIT_OPEN: IssueKind = IssueKind::OpenPort {
    port: 0,
    face: PortFace::PosX,
    flow: PortFlow::Out,
};

#[test]
fn controlled_drop_within_limits_links() {
    let cat = catalog();
    let t = track(drop_placements(place("blk_straight_l3", c(1, 0, 0), 0)));
    assert_eq!(
        controlled_drops(&t, &cat),
        vec![ControlledDrop {
            from: 0,
            from_port: 0,
            to: 1,
            to_port: 0,
            fall_um: CONTROLLED_DROP_MAX_FALL_UM,
        }]
    );
    // Neither end of the drop is open, and the finish is reached through it.
    assert_eq!(validate_track(&t, &cat), vec![]);
}

#[test]
fn controlled_drop_carries_flow_down_only() {
    let cat = catalog();
    // The L1 straight's +X exit drops onto the L3 straight below, which the hopper feeds from
    // the other side (yaw 2 turns the hopper's exit to −X). No marble climbs the drop, so the
    // L1 straight is unreachable, and its other port is open.
    let t = track(vec![
        place("blk_straight_l1", c(0, 1, 0), 0),
        place("blk_straight_l3", c(1, 0, 0), 0),
        place("mech_hopper", c(2, 0, 0), 2),
    ]);
    assert_eq!(controlled_drops(&t, &cat).len(), 1);
    let issues = validate_track(&t, &cat);
    let at_source: Vec<&IssueKind> = issues
        .iter()
        .filter(|i| i.placement == Some(0))
        .map(|i| &i.kind)
        .collect();
    assert_eq!(
        at_source,
        vec![
            &IssueKind::OpenPort {
                port: 0,
                face: PortFace::NegX,
                flow: PortFlow::Both,
            },
            &IssueKind::Unreachable,
        ]
    );
    assert!(!issues.iter().any(|i| i.placement == Some(1)), "{issues:?}");
}

#[test]
fn drop_too_high_stays_open() {
    let cat = catalog();
    // An L2 entry below the L1 exit is a 45 mm fall.
    let t = track(drop_placements(place("blk_straight_l2", c(1, 0, 0), 0)));
    assert!(controlled_drops(&t, &cat).is_empty());
    let issues = validate_track(&t, &cat);
    assert!(issues.contains(&Issue {
        placement: Some(0),
        anchor: Some(c(0, 1, 0)),
        cells: vec![c(0, 1, 0)],
        kind: HOPPER_EXIT_OPEN,
    }));
    assert!(issues.contains(&Issue {
        placement: Some(1),
        anchor: Some(c(1, 0, 0)),
        cells: vec![c(1, 0, 0)],
        kind: IssueKind::OpenPort {
            port: 0,
            face: PortFace::NegX,
            flow: PortFlow::Both,
        },
    }));
    assert!(kinds(&issues).contains(&&IssueKind::FinishUnreachable));
}

#[test]
fn drop_too_far_sideways_stays_open() {
    let cat = catalog();
    // The landing straight is one cell off the exit's centreline (60 mm, over the 6 mm
    // tolerance).
    let t = track(vec![
        place("mech_hopper_l1", c(0, 1, 0), 0),
        place("blk_straight_l3", c(1, 0, 1), 0),
    ]);
    assert!(controlled_drops(&t, &cat).is_empty());
    assert!(kinds(&validate_track(&t, &cat)).contains(&&HOPPER_EXIT_OPEN));
}

#[test]
fn drop_landing_beyond_the_predicted_range_stays_open() {
    let cat = catalog();
    // One cell further out than the exit column: the fastest exit lands less than 60 mm past
    // the port plane, short of that entry.
    let t = track(vec![
        place("mech_hopper_l1", c(0, 1, 0), 0),
        place("blk_straight_l3", c(2, 0, 0), 0),
    ]);
    assert!(controlled_drops(&t, &cat).is_empty());
    assert!(kinds(&validate_track(&t, &cat)).contains(&&HOPPER_EXIT_OPEN));
}

#[test]
fn drop_onto_a_piece_without_an_entry_stays_open() {
    let cat = catalog();
    // Yaw 1 turns the straight's ports to ±Z: the marble would land across the channel.
    let t = track(vec![
        place("mech_hopper_l1", c(0, 1, 0), 0),
        place("blk_straight_l3", c(1, 0, 0), 1),
    ]);
    assert!(controlled_drops(&t, &cat).is_empty());
    assert!(kinds(&validate_track(&t, &cat)).contains(&&HOPPER_EXIT_OPEN));
}

#[test]
fn drop_with_no_landing_piece_stays_open() {
    let cat = catalog();
    let t = track(vec![place("mech_hopper_l1", c(0, 1, 0), 0)]);
    assert!(controlled_drops(&t, &cat).is_empty());
    assert!(kinds(&validate_track(&t, &cat)).contains(&&HOPPER_EXIT_OPEN));
}
