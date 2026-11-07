use parble_core::{
    CellCoord, Mirror, Orientation, Port, PortClass, PortFace, PortFlow, PortLevel, Yaw4,
};
use proptest::prelude::*;

const SIDES: [PortFace; 4] = [
    PortFace::PosX,
    PortFace::NegX,
    PortFace::PosZ,
    PortFace::NegZ,
];

fn all_orientations() -> Vec<Orientation> {
    let mut out = Vec::new();
    for mirror in [Mirror::Off, Mirror::On] {
        for k in 0..4 {
            out.push(Orientation::new(Yaw4::new(k), mirror));
        }
    }
    out
}

/// Kit contract §8 face-mapping table, rows in contract order. Columns are the
/// images of +X, −X, +Z, −Z.
#[test]
fn face_mapping_matches_contract_section_8_table() {
    use PortFace::{NegX as NX, NegZ as NZ, PosX as PX, PosZ as PZ};
    let table: [(Mirror, u8, [PortFace; 4]); 8] = [
        (Mirror::Off, 0, [PX, NX, PZ, NZ]),
        (Mirror::Off, 1, [NZ, PZ, PX, NX]),
        (Mirror::Off, 2, [NX, PX, NZ, PZ]),
        (Mirror::Off, 3, [PZ, NZ, NX, PX]),
        (Mirror::On, 0, [NX, PX, PZ, NZ]),
        (Mirror::On, 1, [PZ, NZ, PX, NX]),
        (Mirror::On, 2, [PX, NX, NZ, PZ]),
        (Mirror::On, 3, [NZ, PZ, NX, PX]),
    ];
    for (mirror, k, expected) in table {
        let o = Orientation::new(Yaw4::new(k), mirror);
        for (face, want) in SIDES.iter().zip(expected) {
            assert_eq!(
                o.apply_face(*face),
                want,
                "mirror={mirror:?} k={k} face={face:?}"
            );
            for level in PortLevel::ALL {
                let port = Port::side(
                    CellCoord::ORIGIN,
                    *face,
                    level,
                    PortClass::Channel,
                    PortFlow::Out,
                );
                let t = port.transformed(o);
                assert_eq!(t.face, want, "port mirror={mirror:?} k={k} face={face:?}");
                assert_eq!(t.cell, CellCoord::ORIGIN);
                assert_eq!(t.level, Some(level));
                assert_eq!(t.class, PortClass::Channel);
                assert_eq!(t.flow, PortFlow::Out);
                assert_eq!(t.inset_um, 0);
            }
        }
    }
}

#[test]
fn top_and_bottom_faces_never_change() {
    for o in all_orientations() {
        assert_eq!(o.apply_face(PortFace::Top), PortFace::Top);
        assert_eq!(o.apply_face(PortFace::Bottom), PortFace::Bottom);
    }
}

#[test]
fn yaw_one_maps_offset_x_to_negative_z() {
    let o = Orientation::new(Yaw4::new(1), Mirror::Off);
    assert_eq!(
        o.apply_cell(CellCoord::new(1, 0, 0)),
        CellCoord::new(0, 0, -1)
    );
    assert_eq!(
        o.apply_cell(CellCoord::new(0, 0, 1)),
        CellCoord::new(1, 0, 0)
    );
}

#[test]
fn mirror_is_applied_before_yaw() {
    // Mirror x -> -x gives (-1,0,0); yaw 1 maps (x,z) -> (z,-x) giving (0,0,1).
    let o = Orientation::new(Yaw4::new(1), Mirror::On);
    assert_eq!(
        o.apply_cell(CellCoord::new(1, 0, 0)),
        CellCoord::new(0, 0, 1)
    );
}

#[test]
fn yaw4_new_wraps_modulo_four() {
    assert_eq!(Yaw4::new(5), Yaw4::new(1));
    assert_eq!(Yaw4::new(4).quarter_turns(), 0);
}

fn arb_orientation() -> impl Strategy<Value = Orientation> {
    (0u8..4, any::<bool>())
        .prop_map(|(k, m)| Orientation::new(Yaw4::new(k), if m { Mirror::On } else { Mirror::Off }))
}

fn arb_cell() -> impl Strategy<Value = CellCoord> {
    (-1000i32..1000, -1000i32..1000, -1000i32..1000).prop_map(|(x, y, z)| CellCoord::new(x, y, z))
}

fn arb_face() -> impl Strategy<Value = PortFace> {
    prop::sample::select(PortFace::ALL.to_vec())
}

proptest! {
    #[test]
    fn yaw_four_times_is_identity(c in arb_cell(), f in arb_face(), k in 0u8..4) {
        let o = Orientation::new(Yaw4::new(k), Mirror::Off);
        let (mut cc, mut ff) = (c, f);
        for _ in 0..4 {
            cc = o.apply_cell(cc);
            ff = o.apply_face(ff);
        }
        // Applying a yaw-k rotation four times is a 4k quarter-turn rotation.
        prop_assert_eq!(cc, c);
        prop_assert_eq!(ff, f);
        let one = Orientation::new(Yaw4::new(1), Mirror::Off);
        let mut c1 = c;
        for _ in 0..4 { c1 = one.apply_cell(c1); }
        prop_assert_eq!(c1, c);
    }

    #[test]
    fn mirror_twice_is_identity(c in arb_cell(), f in arb_face()) {
        let m = Orientation::new(Yaw4::new(0), Mirror::On);
        prop_assert_eq!(m.apply_cell(m.apply_cell(c)), c);
        prop_assert_eq!(m.apply_face(m.apply_face(f)), f);
    }

    #[test]
    fn face_and_cell_transforms_agree(o in arb_orientation(), c in arb_cell(), f in arb_face()) {
        // T(cell + normal(F)) == T(cell) + normal(T(F))
        prop_assert_eq!(o.apply_cell(c + f.normal()), o.apply_cell(c) + o.apply_face(f).normal());
    }

    #[test]
    fn orientation_preserves_cell_layer(o in arb_orientation(), c in arb_cell()) {
        prop_assert_eq!(o.apply_cell(c).y, c.y);
    }
}
