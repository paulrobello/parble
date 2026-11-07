use parble_core::{
    CellCoord, Mirror, Orientation, PieceDef, PieceDefError, PieceId, Port, PortClass, PortFace,
    PortFlow, PortLevel, Yaw4,
};

fn c(x: i32, y: i32, z: i32) -> CellCoord {
    CellCoord::new(x, y, z)
}

fn side(cell: CellCoord, face: PortFace, level: PortLevel, flow: PortFlow) -> Port {
    Port::side(cell, face, level, PortClass::Channel, flow)
}

fn all_orientations() -> Vec<Orientation> {
    let mut out = Vec::new();
    for mirror in [Mirror::Off, Mirror::On] {
        for k in 0..4 {
            out.push(Orientation::new(Yaw4::new(k), mirror));
        }
    }
    out
}

/// 2×2 rail curve: enters the anchor cell from −X, exits the far corner cell to +Z.
fn rail_curve() -> PieceDef {
    PieceDef {
        id: PieceId::new("rail_curve90_r90_l2").unwrap(),
        chiral: true,
        footprint: vec![c(0, 0, 0), c(1, 0, 0), c(0, 0, 1), c(1, 0, 1)],
        ports: vec![
            side(c(0, 0, 0), PortFace::NegX, PortLevel::L2, PortFlow::Both),
            side(c(1, 0, 1), PortFace::PosZ, PortLevel::L2, PortFlow::Both),
        ],
    }
}

/// 2×3 S-rail: lateral offset of one cell over three cells of length (contract §9, Q7).
fn s_rail() -> PieceDef {
    PieceDef {
        id: PieceId::new("rail_s_1x3_l2").unwrap(),
        chiral: true,
        footprint: vec![
            c(0, 0, 0),
            c(1, 0, 0),
            c(2, 0, 0),
            c(0, 0, 1),
            c(1, 0, 1),
            c(2, 0, 1),
        ],
        ports: vec![
            side(c(0, 0, 0), PortFace::NegX, PortLevel::L2, PortFlow::Both),
            side(c(2, 0, 1), PortFace::PosX, PortLevel::L2, PortFlow::Both),
        ],
    }
}

/// 1×1×2 gentle slope from L0 into L3 of the layer below; the exit is in the lower cell.
fn slope_two_tall() -> PieceDef {
    PieceDef {
        id: PieceId::new("blk_slope_gentle_l0").unwrap(),
        chiral: false,
        footprint: vec![c(0, 0, 0), c(0, -1, 0)],
        ports: vec![
            side(c(0, 0, 0), PortFace::NegX, PortLevel::L0, PortFlow::In),
            side(c(0, -1, 0), PortFace::PosX, PortLevel::L3, PortFlow::Out),
        ],
    }
}

#[test]
fn sample_pieces_validate() {
    for def in [rail_curve(), s_rail(), slope_two_tall()] {
        assert_eq!(def.validate(), Ok(()), "{}", def.id);
    }
}

#[test]
fn curve_2x2_footprint_under_yaw_one() {
    let o = Orientation::new(Yaw4::new(1), Mirror::Off);
    assert_eq!(
        rail_curve().footprint_for(o),
        vec![c(0, 0, -1), c(0, 0, 0), c(1, 0, -1), c(1, 0, 0)]
    );
}

#[test]
fn curve_2x2_footprint_mirrored_extends_to_negative_x() {
    let o = Orientation::new(Yaw4::new(0), Mirror::On);
    assert_eq!(
        rail_curve().footprint_for(o),
        vec![c(-1, 0, 0), c(-1, 0, 1), c(0, 0, 0), c(0, 0, 1)]
    );
}

#[test]
fn s_rail_2x3_footprint_under_yaw_two_and_three() {
    let s = s_rail();
    assert_eq!(
        s.footprint_for(Orientation::new(Yaw4::new(2), Mirror::Off)),
        vec![
            c(-2, 0, -1),
            c(-2, 0, 0),
            c(-1, 0, -1),
            c(-1, 0, 0),
            c(0, 0, -1),
            c(0, 0, 0)
        ]
    );
    // Yaw 3 maps (x, z) -> (-z, x).
    assert_eq!(
        s.footprint_for(Orientation::new(Yaw4::new(3), Mirror::Off)),
        vec![
            c(-1, 0, 0),
            c(-1, 0, 1),
            c(-1, 0, 2),
            c(0, 0, 0),
            c(0, 0, 1),
            c(0, 0, 2)
        ]
    );
}

#[test]
fn two_tall_footprint_keeps_lower_cell_below_anchor_in_all_orientations() {
    for o in all_orientations() {
        assert_eq!(
            slope_two_tall().footprint_for(o),
            vec![c(0, -1, 0), c(0, 0, 0)],
            "{o:?}"
        );
    }
}

#[test]
fn transformed_pieces_stay_valid_in_all_orientations() {
    for def in [rail_curve(), s_rail(), slope_two_tall()] {
        for o in all_orientations() {
            let t = def.transformed(o);
            assert_eq!(t.validate(), Ok(()), "{} {o:?}", def.id);
            assert_eq!(t.footprint, def.footprint_for(o));
            assert_eq!(t.ports[0].cell, CellCoord::ORIGIN);
        }
    }
}

#[test]
fn two_tall_ports_keep_levels_under_orientation() {
    let o = Orientation::new(Yaw4::new(3), Mirror::On);
    let ports = slope_two_tall().transformed(o).ports;
    assert_eq!(ports[0].level, Some(PortLevel::L0));
    assert_eq!(ports[1].level, Some(PortLevel::L3));
    assert_eq!(ports[1].cell, c(0, -1, 0));
}

#[test]
fn validation_rejects_port_on_internal_face() {
    let mut def = rail_curve();
    def.ports[1].face = PortFace::NegZ; // (1,0,1) -Z neighbours (1,0,0), inside the footprint.
    assert_eq!(def.validate(), Err(PieceDefError::PortOnInternalFace(1)));
}

#[test]
fn validation_rejects_anchor_port_outside_anchor_cell() {
    let mut def = rail_curve();
    def.ports.swap(0, 1);
    assert_eq!(def.validate(), Err(PieceDefError::AnchorPortNotAtOrigin));
}

#[test]
fn validation_rejects_disconnected_footprint() {
    let mut def = rail_curve();
    def.footprint = vec![c(0, 0, 0), c(2, 0, 0)];
    def.ports.truncate(1);
    assert_eq!(def.validate(), Err(PieceDefError::NotFaceConnected));
}

#[test]
fn validation_rejects_duplicate_cells_and_oversize() {
    let mut def = rail_curve();
    def.footprint.push(c(1, 0, 0));
    assert_eq!(def.validate(), Err(PieceDefError::DuplicateCell));
    let mut wide = rail_curve();
    wide.footprint = (0..5).map(|x| c(x, 0, 0)).collect();
    wide.ports.truncate(1);
    assert_eq!(wide.validate(), Err(PieceDefError::TooLarge));
}

#[test]
fn piece_id_accepts_contract_names() {
    for id in [
        "blk_straight_l1",
        "rail_s_1x3_l2",
        "mech_gate_oneway_l1",
        "blk_pillar",
        "deco_a",
    ] {
        assert!(PieceId::new(id).is_ok(), "{id}");
    }
}

#[test]
fn piece_id_rejects_bad_names() {
    let too_long = format!("blk_{}", "a".repeat(45));
    for id in [
        "",
        "blk",
        "blk_",
        "foo_straight",
        "blk__straight",
        "blk_straight_",
        "Blk_straight",
        "blk_Straight",
        "blk-straight",
        "blk_straight l1",
        too_long.as_str(),
    ] {
        assert!(PieceId::new(id).is_err(), "{id:?}");
    }
    assert!(PieceId::new(&format!("blk_{}", "a".repeat(44))).is_ok());
}

#[test]
fn piece_id_serde_rejects_invalid_names() {
    assert!(serde_json::from_str::<PieceId>("\"blk_ok\"").is_ok());
    assert!(serde_json::from_str::<PieceId>("\"nope\"").is_err());
}

#[test]
fn piece_def_serde_round_trip() {
    let def = rail_curve();
    let json = serde_json::to_string(&def).unwrap();
    assert_eq!(serde_json::from_str::<PieceDef>(&json).unwrap(), def);
}
