use parble_core::contract::{CANONICAL_RING_UM, CELL_UM, LEVEL_STEP_UM, RING_VERTEX_COUNT};
use parble_core::{
    CellCoord, Mirror, Orientation, Port, PortClass, PortFace, PortFlow, PortLevel, Yaw4,
};

#[test]
fn level_floor_heights_follow_contract_section_3() {
    assert_eq!(PortLevel::L0.floor_um(), 0);
    assert_eq!(PortLevel::L1.floor_um(), 15_000);
    assert_eq!(PortLevel::L2.floor_um(), 30_000);
    assert_eq!(PortLevel::L3.floor_um(), 45_000);
    assert_eq!(PortLevel::L3.absolute_floor_um(2), 2 * 60_000 + 45_000);
    assert_eq!(PortLevel::L1.absolute_floor_um(-1), -60_000 + 15_000);
}

#[test]
fn level_below_l0_carries_to_l3_of_layer_below() {
    assert_eq!(PortLevel::L0.offset(5, -1), (4, PortLevel::L3));
}

#[test]
fn level_above_l3_carries_to_l0_of_layer_above() {
    assert_eq!(PortLevel::L3.offset(5, 1), (6, PortLevel::L0));
}

#[test]
fn level_offset_spans_several_layers() {
    // Steep slope L1 -> two levels down -> L3 of the layer below (contract §7).
    assert_eq!(PortLevel::L1.offset(0, -2), (-1, PortLevel::L3));
    assert_eq!(PortLevel::L2.offset(0, 9), (2, PortLevel::L3));
    for delta in -20..20 {
        let (layer, level) = PortLevel::L2.offset(3, delta);
        assert_eq!(
            level.absolute_floor_um(layer),
            PortLevel::L2.absolute_floor_um(3) + delta * LEVEL_STEP_UM
        );
    }
}

#[test]
fn level_serializes_as_integer_and_rejects_four() {
    assert_eq!(serde_json::to_string(&PortLevel::L3).unwrap(), "3");
    assert!(serde_json::from_str::<PortLevel>("4").is_err());
}

#[test]
fn port_transform_keeps_level_and_maps_face_and_cell() {
    let p = Port::side(
        CellCoord::new(1, 0, 0),
        PortFace::PosX,
        PortLevel::L2,
        PortClass::Channel,
        PortFlow::Out,
    );
    let o = Orientation::new(Yaw4::new(1), Mirror::Off);
    let t = p.transformed(o);
    assert_eq!(t.cell, CellCoord::new(0, 0, -1));
    assert_eq!(t.face, PortFace::NegZ);
    assert_eq!(t.level, Some(PortLevel::L2));
    assert_eq!(t.class, PortClass::Channel);
    assert_eq!(t.flow, PortFlow::Out);
}

#[test]
fn port_validation_rejects_level_mismatch_by_face_kind() {
    let ok = Port::drop(CellCoord::ORIGIN, PortFace::Bottom, PortFlow::Out);
    assert!(ok.validate().is_ok());
    let mut bad = ok;
    bad.level = Some(PortLevel::L1);
    assert!(bad.validate().is_err());
    let mut side_no_level = Port::side(
        CellCoord::ORIGIN,
        PortFace::NegX,
        PortLevel::L1,
        PortClass::Channel,
        PortFlow::In,
    );
    side_no_level.level = None;
    assert!(side_no_level.validate().is_err());
    let mut drop_on_side = ok;
    drop_on_side.face = PortFace::PosZ;
    assert!(drop_on_side.validate().is_err());
}

#[test]
fn port_serde_uses_sidecar_spellings() {
    let p = Port::side(
        CellCoord::ORIGIN,
        PortFace::NegX,
        PortLevel::L1,
        PortClass::Channel,
        PortFlow::In,
    );
    let json = serde_json::to_string(&p).unwrap();
    assert!(json.contains("\"face\":\"-x\""), "{json}");
    assert!(json.contains("\"class\":\"channel\""), "{json}");
    assert!(json.contains("\"flow\":\"in\""), "{json}");
    assert!(json.contains("\"cell\":[0,0,0]"), "{json}");
    assert_eq!(serde_json::from_str::<Port>(&json).unwrap(), p);
}

#[test]
fn contract_cell_and_level_step_in_micrometres() {
    assert_eq!(CELL_UM, 60_000);
    assert_eq!(LEVEL_STEP_UM * 4, CELL_UM);
}

#[test]
fn canonical_ring_matches_contract_worked_value() {
    // §11 worked example: θ = 11.25° gives u = 2731 µm, floor offset 269 µm.
    assert_eq!(RING_VERTEX_COUNT, 17);
    assert_eq!(CANONICAL_RING_UM[8], (0, 0));
    assert_eq!(CANONICAL_RING_UM[9], (2731, 269));
    assert_eq!(CANONICAL_RING_UM[0], (-14_000, 14_000));
    assert_eq!(CANONICAL_RING_UM[16], (14_000, 14_000));
}

#[test]
fn canonical_ring_matches_formula_rounded_half_away_from_zero() {
    for (i, &(u, v)) in CANONICAL_RING_UM.iter().enumerate() {
        let theta = (-90.0 + 11.25 * i as f64).to_radians();
        let want_u = (14_000.0 * theta.sin()).round() as i32;
        let want_v = (14_000.0 - 14_000.0 * theta.cos()).round() as i32;
        assert_eq!((u, v), (want_u, want_v), "vertex {i}");
        // Symmetric about the centreline.
        assert_eq!(CANONICAL_RING_UM[16 - i], (-u, v));
    }
}
