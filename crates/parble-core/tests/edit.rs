use std::collections::BTreeMap;

use parble_core::{
    CellCoord, EditCommand, EditError, Editor, HopperConfig, MapCatalog, MarbleSize, MechConfig,
    Mirror, PieceDef, PieceId, Placement, Port, PortClass, PortFace, PortFlow, PortLevel,
    ReleaseMode, Selection, Track, Yaw4,
};
use proptest::prelude::*;

fn c(x: i32, y: i32, z: i32) -> CellCoord {
    CellCoord::new(x, y, z)
}

fn id(s: &str) -> PieceId {
    PieceId::new(s).unwrap()
}

fn side(cell: CellCoord, face: PortFace, level: PortLevel) -> Port {
    Port::side(cell, face, level, PortClass::Channel, PortFlow::Both)
}

const STRAIGHT: &str = "rail_straight_l2";
const CURVE: &str = "rail_curve90_r90_l2";
const S_RAIL: &str = "rail_s_1x3_l2";
const SLOPE: &str = "blk_slope_gentle_l0";
const HOPPER: &str = "mech_hopper";
const TOGGLE: &str = "mech_toggle_l3";
const PIECES: [&str; 6] = [STRAIGHT, CURVE, S_RAIL, SLOPE, HOPPER, TOGGLE];

fn def(name: &str, chiral: bool, footprint: Vec<CellCoord>, exit: CellCoord) -> PieceDef {
    PieceDef {
        structural: false,
        routes: Vec::new(),
        id: id(name),
        chiral,
        footprint,
        ports: vec![
            side(c(0, 0, 0), PortFace::NegX, PortLevel::L2),
            side(exit, PortFace::PosX, PortLevel::L2),
        ],
    }
}

fn catalog() -> MapCatalog {
    let defs = [
        def(STRAIGHT, false, vec![c(0, 0, 0)], c(0, 0, 0)),
        def(
            CURVE,
            true,
            vec![c(0, 0, 0), c(1, 0, 0), c(0, 0, 1), c(1, 0, 1)],
            c(1, 0, 1),
        ),
        def(
            S_RAIL,
            true,
            vec![
                c(0, 0, 0),
                c(1, 0, 0),
                c(2, 0, 0),
                c(0, 0, 1),
                c(1, 0, 1),
                c(2, 0, 1),
            ],
            c(2, 0, 1),
        ),
        def(SLOPE, false, vec![c(0, 0, 0), c(0, -1, 0)], c(0, -1, 0)),
        def(HOPPER, false, vec![c(0, 0, 0)], c(0, 0, 0)),
        def(TOGGLE, false, vec![c(0, 0, 0)], c(0, 0, 0)),
    ];
    for d in &defs {
        assert_eq!(d.validate(), Ok(()), "{}", d.id);
    }
    defs.into_iter().collect()
}

fn at(piece: &str, anchor: CellCoord, yaw: u8, mirror: Mirror) -> Placement {
    Placement::new(id(piece), anchor, Yaw4::new(yaw), mirror)
}

fn place(piece: &str, anchor: CellCoord, yaw: u8, mirror: Mirror) -> EditCommand {
    EditCommand::Place(at(piece, anchor, yaw, mirror))
}

fn sel(indices: &[u32]) -> Selection {
    indices.iter().copied().collect()
}

fn bytes(t: &Track) -> Vec<u8> {
    serde_json::to_vec(t).unwrap()
}

fn editor_with(cmds: Vec<EditCommand>) -> (Editor, MapCatalog) {
    let cat = catalog();
    let mut ed = Editor::new(Track::new(7));
    for cmd in cmds {
        ed.apply(&cat, cmd).unwrap();
    }
    (ed, cat)
}

/// Applies `cmd`, expects `err`, and checks that the track and both stacks are unchanged.
fn assert_rejected(ed: &mut Editor, cat: &MapCatalog, cmd: EditCommand, err: EditError) {
    let before = bytes(ed.track());
    let (undo, redo) = (ed.can_undo(), ed.can_redo());
    assert_eq!(ed.apply(cat, cmd), Err(err));
    assert_eq!(bytes(ed.track()), before);
    assert_eq!((ed.can_undo(), ed.can_redo()), (undo, redo));
}

// ---- footprint collision (contract §9 rule 6) ----

#[test]
fn curve_2x2_rejects_overlap_on_non_anchor_cell() {
    let (mut ed, cat) = editor_with(vec![place(CURVE, c(0, 0, 0), 0, Mirror::Off)]);
    assert_rejected(
        &mut ed,
        &cat,
        place(STRAIGHT, c(1, 0, 1), 0, Mirror::Off),
        EditError::Collision(c(1, 0, 1)),
    );
    ed.apply(&cat, place(STRAIGHT, c(2, 0, 1), 0, Mirror::Off))
        .unwrap();
}

#[test]
fn two_yawed_curves_overlap_with_distinct_anchors() {
    // Yaw 1 maps (x, z) -> (z, -x): cells (0,0,0) (1,0,0) (0,0,-1) (1,0,-1).
    let (mut ed, cat) = editor_with(vec![place(CURVE, c(0, 0, 0), 1, Mirror::Off)]);
    // Yaw 2 at (2,0,0) covers (2,0,0) (1,0,0) (2,0,-1) (1,0,-1).
    assert_rejected(
        &mut ed,
        &cat,
        place(CURVE, c(2, 0, 0), 2, Mirror::Off),
        EditError::Collision(c(1, 0, -1)),
    );
    // Yaw 0 at (2,0,0) covers (2,0,0) (3,0,0) (2,0,1) (3,0,1): clear.
    ed.apply(&cat, place(CURVE, c(2, 0, 0), 0, Mirror::Off))
        .unwrap();
}

#[test]
fn mirrored_s_rail_2x3_footprint_collides() {
    // Mirror then yaw 3, (x, z) -> (-z, x): cells x in {0, -1}, z in {0, -1, -2}.
    let (mut ed, cat) = editor_with(vec![place(S_RAIL, c(0, 0, 0), 3, Mirror::On)]);
    assert_rejected(
        &mut ed,
        &cat,
        place(STRAIGHT, c(-1, 0, -2), 0, Mirror::Off),
        EditError::Collision(c(-1, 0, -2)),
    );
    // The unmirrored footprint would cover (-1, 0, 2); the mirrored one does not.
    ed.apply(&cat, place(STRAIGHT, c(-1, 0, 2), 0, Mirror::Off))
        .unwrap();
}

#[test]
fn two_tall_1x1x2_collides_in_lower_cell() {
    let (mut ed, cat) = editor_with(vec![place(SLOPE, c(0, 0, 0), 0, Mirror::Off)]);
    assert_rejected(
        &mut ed,
        &cat,
        place(STRAIGHT, c(0, -1, 0), 0, Mirror::Off),
        EditError::Collision(c(0, -1, 0)),
    );
    // A second slope stacked on top reaches down into (0, 0, 0).
    assert_rejected(
        &mut ed,
        &cat,
        place(SLOPE, c(0, 1, 0), 0, Mirror::Off),
        EditError::Collision(c(0, 0, 0)),
    );
    ed.apply(&cat, place(STRAIGHT, c(0, -2, 0), 0, Mirror::Off))
        .unwrap();
    ed.apply(&cat, place(SLOPE, c(0, 2, 0), 0, Mirror::Off))
        .unwrap();
}

#[test]
fn rotate_into_neighbour_is_rejected() {
    let (mut ed, cat) = editor_with(vec![
        place(CURVE, c(0, 0, 0), 0, Mirror::Off),
        place(STRAIGHT, c(0, 0, -1), 0, Mirror::Off),
    ]);
    assert_rejected(
        &mut ed,
        &cat,
        EditCommand::Rotate {
            selection: sel(&[0]),
            by: Yaw4::new(1),
        },
        EditError::Collision(c(0, 0, -1)),
    );
    // Yaw 2 covers (0,0,0) (-1,0,0) (0,0,-1) (-1,0,-1): still blocked.
    assert_rejected(
        &mut ed,
        &cat,
        EditCommand::Rotate {
            selection: sel(&[0]),
            by: Yaw4::new(2),
        },
        EditError::Collision(c(0, 0, -1)),
    );
    ed.apply(
        &cat,
        EditCommand::Rotate {
            selection: sel(&[0]),
            by: Yaw4::new(3),
        },
    )
    .unwrap();
    assert_eq!(ed.track().placements[0].yaw, Yaw4::new(3));
}

#[test]
fn move_group_may_slide_over_its_own_cells() {
    let (mut ed, cat) = editor_with(vec![
        place(STRAIGHT, c(0, 0, 0), 0, Mirror::Off),
        place(STRAIGHT, c(1, 0, 0), 0, Mirror::Off),
        place(STRAIGHT, c(3, 0, 0), 0, Mirror::Off),
    ]);
    ed.apply(
        &cat,
        EditCommand::MoveGroup {
            selection: sel(&[0, 1]),
            offset: c(1, 0, 0),
        },
    )
    .unwrap();
    let anchors: Vec<_> = ed.track().placements.iter().map(|p| p.anchor).collect();
    assert_eq!(anchors, vec![c(1, 0, 0), c(2, 0, 0), c(3, 0, 0)]);
    assert_rejected(
        &mut ed,
        &cat,
        EditCommand::MoveGroup {
            selection: sel(&[0, 1]),
            offset: c(1, 0, 0),
        },
        EditError::Collision(c(3, 0, 0)),
    );
}

#[test]
fn paste_group_collides_with_source_and_itself() {
    let (mut ed, cat) = editor_with(vec![
        place(CURVE, c(0, 0, 0), 0, Mirror::On),
        place(STRAIGHT, c(2, 0, 0), 0, Mirror::Off),
    ]);
    let clip = ed.copy(&sel(&[0, 1])).unwrap();
    assert_rejected(
        &mut ed,
        &cat,
        EditCommand::PasteGroup {
            placements: clip.clone(),
            offset: c(1, 0, 0),
        },
        EditError::Collision(c(0, 0, 0)),
    );
    // The two clipboard pieces overlap each other once the straight sits inside the curve.
    let overlapping = vec![clip[0].clone(), at(STRAIGHT, c(-1, 0, 1), 0, Mirror::Off)];
    assert_rejected(
        &mut ed,
        &cat,
        EditCommand::PasteGroup {
            placements: overlapping,
            offset: c(0, 0, 5),
        },
        EditError::Collision(c(-1, 0, 1 + 5)),
    );
    ed.apply(
        &cat,
        EditCommand::PasteGroup {
            placements: clip,
            offset: c(0, 0, 5),
        },
    )
    .unwrap();
    assert_eq!(ed.track().placements.len(), 4);
    assert_eq!(ed.selection, sel(&[2, 3]));
    assert_eq!(ed.track().placements[2].anchor, c(0, 0, 5));
    assert_eq!(ed.track().placements[2].mirror, Mirror::On);
}

#[test]
fn unknown_piece_is_rejected() {
    let (mut ed, cat) = editor_with(vec![]);
    assert_rejected(
        &mut ed,
        &cat,
        place("rail_helix_l2", c(0, 0, 0), 0, Mirror::Off),
        EditError::UnknownPiece(id("rail_helix_l2")),
    );
}

// ---- mirror (contract §8) ----

#[test]
fn mirror_only_for_chiral_pieces() {
    let (mut ed, cat) = editor_with(vec![
        place(STRAIGHT, c(0, 0, 0), 0, Mirror::Off),
        place(CURVE, c(5, 0, 0), 0, Mirror::Off),
    ]);
    assert_rejected(
        &mut ed,
        &cat,
        place(SLOPE, c(9, 0, 0), 0, Mirror::On),
        EditError::NotChiral(id(SLOPE)),
    );
    assert_rejected(
        &mut ed,
        &cat,
        EditCommand::Mirror(sel(&[0, 1])),
        EditError::NotChiral(id(STRAIGHT)),
    );
    ed.apply(&cat, EditCommand::Mirror(sel(&[1]))).unwrap();
    assert_eq!(ed.track().placements[1].mirror, Mirror::On);
    ed.apply(&cat, EditCommand::Mirror(sel(&[1]))).unwrap();
    assert_eq!(ed.track().placements[1].mirror, Mirror::Off);
}

// ---- remove, hopper index, configs ----

#[test]
fn remove_keeps_hopper_index_pointing_at_the_hopper() {
    let (mut ed, cat) = editor_with(vec![
        place(STRAIGHT, c(0, 0, 0), 0, Mirror::Off),
        place(STRAIGHT, c(1, 0, 0), 0, Mirror::Off),
        place(HOPPER, c(2, 0, 0), 0, Mirror::Off),
        place(STRAIGHT, c(3, 0, 0), 0, Mirror::Off),
        EditCommand::SetHopper(HopperConfig {
            placement: Some(2),
            queue: vec![MarbleSize::Mini],
            release: ReleaseMode::All,
        }),
    ]);
    let original = bytes(ed.track());
    ed.apply(&cat, EditCommand::Remove(sel(&[0, 1, 3])))
        .unwrap();
    assert_eq!(ed.track().hopper.placement, Some(0));
    assert_eq!(ed.track().placements[0].piece, id(HOPPER));
    assert!(ed.undo());
    assert_eq!(bytes(ed.track()), original);

    ed.apply(&cat, EditCommand::Remove(sel(&[2]))).unwrap();
    assert_eq!(ed.track().hopper.placement, None);
    assert_eq!(ed.track().hopper.queue, vec![MarbleSize::Mini]);
    assert!(ed.undo());
    assert_eq!(bytes(ed.track()), original);
}

#[test]
fn bad_indices_and_empty_selections_are_rejected() {
    let (mut ed, cat) = editor_with(vec![place(STRAIGHT, c(0, 0, 0), 0, Mirror::Off)]);
    assert_rejected(
        &mut ed,
        &cat,
        EditCommand::Remove(sel(&[0, 1])),
        EditError::IndexOutOfRange(1),
    );
    assert_rejected(
        &mut ed,
        &cat,
        EditCommand::Remove(Selection::new()),
        EditError::EmptySelection,
    );
    assert_rejected(
        &mut ed,
        &cat,
        EditCommand::SetHopper(HopperConfig {
            placement: Some(1),
            ..HopperConfig::default()
        }),
        EditError::IndexOutOfRange(1),
    );
}

#[test]
fn mech_config_is_stored_on_mech_placements_only() {
    let (mut ed, cat) = editor_with(vec![
        place(STRAIGHT, c(0, 0, 0), 0, Mirror::Off),
        place(TOGGLE, c(1, 0, 0), 0, Mirror::Off),
    ]);
    let config: MechConfig = BTreeMap::from([("initial_state".to_owned(), 1)]);
    assert_rejected(
        &mut ed,
        &cat,
        EditCommand::SetMechConfig {
            index: 0,
            config: config.clone(),
        },
        EditError::NotMechanism(0),
    );
    let before = bytes(ed.track());
    ed.apply(
        &cat,
        EditCommand::SetMechConfig {
            index: 1,
            config: config.clone(),
        },
    )
    .unwrap();
    assert_eq!(ed.track().placements[1].config, config);
    // The config moves with its placement.
    ed.apply(
        &cat,
        EditCommand::MoveGroup {
            selection: sel(&[1]),
            offset: c(0, 0, 4),
        },
    )
    .unwrap();
    assert_eq!(ed.track().placements[1].config, config);
    assert!(ed.undo() && ed.undo());
    assert_eq!(bytes(ed.track()), before);
}

#[test]
fn empty_config_is_not_serialized() {
    let p = at(STRAIGHT, c(0, 0, 0), 0, Mirror::Off);
    let json = serde_json::to_string(&p).unwrap();
    assert!(!json.contains("config"), "{json}");
    let back: Placement = serde_json::from_str(&json).unwrap();
    assert_eq!(back, p);
}

// ---- undo / redo / selection ----

#[test]
fn redo_after_undo_reproduces_state() {
    let (mut ed, cat) = editor_with(vec![
        place(CURVE, c(0, 0, 0), 0, Mirror::Off),
        place(SLOPE, c(4, 1, 0), 0, Mirror::Off),
    ]);
    ed.apply(
        &cat,
        EditCommand::Rotate {
            selection: sel(&[0]),
            by: Yaw4::new(2),
        },
    )
    .unwrap();
    let after = bytes(ed.track());
    assert!(ed.undo());
    assert_ne!(bytes(ed.track()), after);
    // A rejected command must not drop the redo stack.
    assert_rejected(
        &mut ed,
        &cat,
        place(STRAIGHT, c(4, 0, 0), 0, Mirror::Off),
        EditError::Collision(c(4, 0, 0)),
    );
    assert!(ed.redo());
    assert_eq!(bytes(ed.track()), after);
    assert!(!ed.redo());
}

#[test]
fn successful_command_clears_redo() {
    let (mut ed, cat) = editor_with(vec![place(STRAIGHT, c(0, 0, 0), 0, Mirror::Off)]);
    assert!(ed.undo());
    assert!(ed.can_redo());
    ed.apply(&cat, place(STRAIGHT, c(1, 0, 0), 0, Mirror::Off))
        .unwrap();
    assert!(!ed.can_redo());
    assert!(ed.undo());
    assert!(!ed.undo());
    assert!(ed.track().placements.is_empty());
}

#[test]
fn selection_sets_and_box_select() {
    let (ed, cat) = editor_with(vec![
        place(CURVE, c(0, 0, 0), 0, Mirror::Off),
        place(STRAIGHT, c(5, 0, 0), 0, Mirror::Off),
        place(SLOPE, c(2, 1, 2), 0, Mirror::Off),
    ]);
    assert_eq!(ed.selection, sel(&[2]), "place selects the new piece");
    // The curve's far corner (1,0,1) is inside; its anchor is not. The slope's lower cell
    // (2,0,2) is inside.
    let boxed = Selection::boxed(ed.track(), &cat, c(2, 0, 2), c(1, 0, 1));
    assert_eq!(boxed, sel(&[0, 2]));

    let mut s = Selection::new();
    s.toggle(3);
    s.insert(1);
    s.toggle(3);
    assert!(s.contains(1) && !s.contains(3));
    assert_eq!(s.iter().collect::<Vec<_>>(), vec![1]);
    s.clear();
    assert!(s.is_empty());
}

// ---- property: random op sequence then full undo is byte-identical ----

#[derive(Debug, Clone)]
enum Op {
    Place(usize, (i32, i32, i32), u8, bool),
    Remove(Vec<u32>),
    Rotate(Vec<u32>, u8),
    Mirror(Vec<u32>),
    Move(Vec<u32>, (i32, i32, i32)),
    Paste(Vec<u32>, (i32, i32, i32)),
    MechConfig(u32, i64),
    Hopper(Option<u32>, u8),
    Undo,
    Redo,
}

fn idx() -> impl Strategy<Value = u32> {
    0u32..64
}

fn idxs() -> impl Strategy<Value = Vec<u32>> {
    prop::collection::vec(idx(), 0..4)
}

fn coord(r: i32) -> impl Strategy<Value = (i32, i32, i32)> {
    (-r..=r, -r..=r, -r..=r)
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (0..PIECES.len(), coord(4), 0u8..4, any::<bool>())
            .prop_map(|(p, a, y, m)| Op::Place(p, a, y, m)),
        1 => idxs().prop_map(Op::Remove),
        1 => (idxs(), 0u8..4).prop_map(|(s, y)| Op::Rotate(s, y)),
        1 => idxs().prop_map(Op::Mirror),
        1 => (idxs(), coord(3)).prop_map(|(s, d)| Op::Move(s, d)),
        1 => (idxs(), coord(3)).prop_map(|(s, d)| Op::Paste(s, d)),
        1 => (idx(), -3i64..3).prop_map(|(i, v)| Op::MechConfig(i, v)),
        1 => (prop::option::of(idx()), 0u8..4).prop_map(|(i, n)| Op::Hopper(i, n)),
        1 => Just(Op::Undo),
        1 => Just(Op::Redo),
    ]
}

fn to_sel(raw: &[u32], len: usize) -> Selection {
    if len == 0 {
        return raw.iter().copied().collect();
    }
    let len = u32::try_from(len).unwrap();
    raw.iter().map(|i| i % len).collect()
}

fn run(ed: &mut Editor, cat: &MapCatalog, op: &Op) {
    let len = ed.track().placements.len();
    let wrap = |i: u32| if len == 0 { i } else { i % len as u32 };
    let cmd = match op {
        Op::Undo => {
            ed.undo();
            return;
        }
        Op::Redo => {
            ed.redo();
            return;
        }
        Op::Place(p, (x, y, z), yaw, mirror) => {
            place(PIECES[*p], c(*x, *y, *z), *yaw, Mirror::from(*mirror))
        }
        Op::Remove(s) => EditCommand::Remove(to_sel(s, len)),
        Op::Rotate(s, y) => EditCommand::Rotate {
            selection: to_sel(s, len),
            by: Yaw4::new(*y),
        },
        Op::Mirror(s) => EditCommand::Mirror(to_sel(s, len)),
        Op::Move(s, (x, y, z)) => EditCommand::MoveGroup {
            selection: to_sel(s, len),
            offset: c(*x, *y, *z),
        },
        Op::Paste(s, (x, y, z)) => match ed.copy(&to_sel(s, len)) {
            Ok(placements) => EditCommand::PasteGroup {
                placements,
                offset: c(*x, *y, *z),
            },
            Err(_) => return,
        },
        Op::MechConfig(i, v) => EditCommand::SetMechConfig {
            index: wrap(*i),
            config: BTreeMap::from([("mode".to_owned(), *v)]),
        },
        Op::Hopper(i, n) => EditCommand::SetHopper(HopperConfig {
            placement: i.map(wrap),
            queue: vec![MarbleSize::Standard; usize::from(*n)],
            release: ReleaseMode::Interval {
                ticks: u32::from(*n) * 60,
            },
        }),
    };
    let before = bytes(ed.track());
    if ed.apply(cat, cmd).is_err() {
        assert_eq!(bytes(ed.track()), before, "rejected op mutated the track");
    }
}

fn seeded_track() -> Track {
    let mut t = Track::new(0xDEAD_BEEF);
    t.placements = vec![
        at(HOPPER, c(0, 3, 0), 0, Mirror::Off),
        at(CURVE, c(2, 0, 2), 1, Mirror::On),
        at(SLOPE, c(-3, 1, -3), 2, Mirror::Off),
        at(S_RAIL, c(-2, 0, 3), 0, Mirror::Off),
    ];
    t.placements[0].config = BTreeMap::from([("rate".to_owned(), 2)]);
    t.hopper = HopperConfig {
        placement: Some(0),
        queue: vec![MarbleSize::Shooter, MarbleSize::Mini],
        release: ReleaseMode::One,
    };
    t
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn full_undo_restores_original_bytes(ops in prop::collection::vec(op(), 0..48)) {
        let cat = catalog();
        let original = seeded_track();
        let original_bytes = bytes(&original);
        let mut ed = Editor::new(original);
        for op in &ops {
            run(&mut ed, &cat, op);
        }
        let edited = bytes(ed.track());

        let mut depth = 0;
        while ed.can_undo() {
            prop_assert!(ed.undo());
            depth += 1;
        }
        prop_assert_eq!(&bytes(ed.track()), &original_bytes);

        for _ in 0..depth {
            prop_assert!(ed.redo());
        }
        prop_assert_eq!(&bytes(ed.track()), &edited);
    }
}

#[test]
fn editor_accepts_a_trait_object_catalog() {
    let cat = catalog();
    let dyn_cat: &dyn parble_core::PieceCatalog = &cat;
    let mut ed = Editor::new(Track::new(1));
    ed.apply(dyn_cat, place(CURVE, c(0, 0, 0), 0, Mirror::Off))
        .unwrap();
    assert_eq!(
        Selection::boxed(ed.track(), dyn_cat, c(1, 0, 1), c(1, 0, 1)),
        sel(&[0])
    );
}

#[test]
fn check_matches_apply_and_changes_nothing() {
    let (mut ed, cat) = editor_with(vec![place(CURVE, c(0, 0, 0), 0, Mirror::Off)]);
    ed.undo();
    let before = (bytes(ed.track()), ed.can_undo(), ed.can_redo());
    assert_eq!(
        ed.check(&cat, place(STRAIGHT, c(5, 0, 0), 0, Mirror::Off)),
        Ok(())
    );
    ed.redo();
    let after_redo = bytes(ed.track());
    ed.undo();
    assert_eq!((bytes(ed.track()), ed.can_undo(), ed.can_redo()), before);
    ed.redo();
    assert_eq!(bytes(ed.track()), after_redo);
    assert_eq!(
        ed.check(&cat, place(STRAIGHT, c(1, 0, 1), 0, Mirror::Off)),
        Err(EditError::Collision(c(1, 0, 1)))
    );
    assert!(ed.can_undo() && !ed.can_redo());
}
