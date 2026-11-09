//! The funnel catcher (`blk_funnel_3x3_l2`) links with its mating pieces as shipped: both rim
//! inlets take a block channel and a rail, the centre exit drops into `blk_catch_l1` (on to a
//! block channel) or `blk_tower_socket_l1` (on to a rail joint), and a drop from an `L3` exit
//! one layer up over the bowl is a controlled drop (`parble_core::validate`, funnel bowls).

use parble_core::{
    CellCoord, ControlledDrop, FUNNEL_MAX_FALL_UM, IssueKind, Mirror, PieceId, Placement, PortFace,
    Track, Yaw4, controlled_drops, validate_track,
};

fn place(id: &str, x: i32, y: i32, z: i32) -> Placement {
    Placement::new(
        PieceId::new(id).unwrap(),
        CellCoord::new(x, y, z),
        Yaw4::new(0),
        Mirror::Off,
    )
}

fn track(placements: Vec<Placement>) -> Track {
    let mut t = Track::new(7);
    t.placements = placements;
    t
}

/// The port issues of `t`: open ports and broken links, as (placement, port, face).
fn port_issues(t: &Track) -> Vec<(usize, usize, PortFace)> {
    let mut out: Vec<_> = validate_track(t, parble_kit::builtin())
        .iter()
        .filter_map(|i| match &i.kind {
            IssueKind::OpenPort { port, face, .. } => Some((i.placement?, *port, *face)),
            IssueKind::BrokenLink { port, .. } => Some((i.placement?, *port, PortFace::Top)),
            IssueKind::Overlap { .. } | IssueKind::LevelSuffixMismatch { .. } => {
                panic!("{i:?}")
            }
            _ => None,
        })
        .collect();
    out.sort_unstable();
    out
}

#[test]
fn the_rim_inlets_take_a_block_channel_and_a_rail_and_the_catch_exits_to_a_block_channel() {
    // Funnel at (1, 1, 0): port 0 at cell (1, 1, 0) -x, port 1 at (3, 1, -2) +x, both `L2`; the
    // bottom drop port at (2, 0, -1) over the catch at (2, -1, -1), whose `L1` exit faces -x.
    let t = track(vec![
        place("blk_straight_l2", 0, 1, 0),
        place("blk_funnel_3x3_l2", 1, 1, 0),
        place("rail_straight_l2", 4, 1, -2),
        place("blk_catch_l1", 2, -1, -1),
        place("blk_straight_l1", 1, -1, -1),
    ]);
    // Only the chain's three far ends are open.
    assert_eq!(
        port_issues(&t),
        vec![
            (0, 0, PortFace::NegX),
            (2, 1, PortFace::PosX),
            (4, 0, PortFace::NegX)
        ]
    );
}

#[test]
fn the_centre_exit_drops_into_a_tower_socket_and_on_to_a_rail_joint() {
    let t = track(vec![
        place("blk_straight_l2", 0, 1, 0),
        place("blk_funnel_3x3_l2", 1, 1, 0),
        place("rail_straight_l2", 4, 1, -2),
        place("blk_tower_socket_l1", 2, -1, -1),
        place("rail_joint_l1", 1, -1, -1),
    ]);
    assert_eq!(
        port_issues(&t),
        vec![
            (0, 0, PortFace::NegX),
            (2, 1, PortFace::PosX),
            (4, 0, PortFace::NegX)
        ]
    );
}

#[test]
fn an_unfed_funnel_reports_its_open_ports() {
    let t = track(vec![place("blk_funnel_3x3_l2", 1, 1, 0)]);
    assert_eq!(
        port_issues(&t),
        vec![
            (0, 0, PortFace::NegX),
            (0, 1, PortFace::PosX),
            (0, 2, PortFace::Bottom)
        ]
    );
}

#[test]
fn a_drop_over_the_bowl_links_and_one_off_the_catch_radius_or_too_high_does_not() {
    let base = || {
        vec![
            place("blk_straight_l2", 0, 1, 0),
            place("blk_funnel_3x3_l2", 1, 1, 0),
            place("rail_straight_l2", 4, 1, -2),
            place("blk_catch_l1", 2, -1, -1),
            place("blk_straight_l1", 1, -1, -1),
        ]
    };
    // An `L3` straight on the funnel's -x edge cell one layer up: its +x port plane is 30 mm
    // from the axis and the fastest exit lands 92.7 mm past it, 62.7 mm from the axis.
    let mut l = base();
    l.push(place("blk_straight_l3", 1, 2, -1));
    let t = track(l);
    assert_eq!(
        controlled_drops(&t, parble_kit::builtin()),
        vec![ControlledDrop {
            from: 5,
            from_port: 1,
            to: 1,
            to_port: None,
            fall_um: FUNNEL_MAX_FALL_UM,
        }]
    );
    // The drop port of the straight is linked; only its own -x end joins the chain's open ends,
    // and the funnel's ports stay as they were.
    assert_eq!(
        port_issues(&t),
        vec![
            (0, 0, PortFace::NegX),
            (2, 1, PortFace::PosX),
            (4, 0, PortFace::NegX),
            (5, 0, PortFace::NegX)
        ]
    );

    // One cell over in z: the port plane is 67.1 mm from the axis, off the 66 mm catch radius.
    let mut l = base();
    l.push(place("blk_straight_l3", 1, 2, 0));
    let t = track(l);
    assert!(controlled_drops(&t, parble_kit::builtin()).is_empty());
    assert!(port_issues(&t).contains(&(5, 1, PortFace::PosX)));

    // Two layers up: a 135 mm fall, over FUNNEL_MAX_FALL_UM.
    let mut l = base();
    l.push(place("blk_straight_l3", 1, 3, -1));
    let t = track(l);
    assert!(controlled_drops(&t, parble_kit::builtin()).is_empty());
}
