//! Animated mechanism parts (P2-T6): each `anim_<name>` part is its own instanced mesh per (piece
//! type, LOD), drawn with the piece's wood under the [`super::PART_STAIN`] tint and posed every frame
//! from the sim's part angles.
//!
//! - The app writes the part angles of the last two physics ticks to [`PartFrame`] wherever it
//!   writes [`crate::MarbleFrame`]; parts interpolate with the marbles' `alpha` ([`interpolated_mdeg`]).
//! - A part with no sim instance behind it (editor-placed mechanisms, which have no behaviour)
//!   holds its sidecar rest angle.
//! - A part is posed in its piece's frame, under a per-piece parent entity carrying the placement
//!   transform: the editor mirrors with a negative x scale, which `Transform` composition cannot
//!   carry through a rotation, while `GlobalTransform` propagation (an affine product) can. The
//!   reflection also turns the part's rotation by θ into one by −θ about the image axis, which is
//!   the sim's mirrored sign, so the part takes the sim angle unsigned.
//! - A part's [`bevy::camera::visibility::VisibilityRange`] is measured from its pivot, not the
//!   piece origin, so inside a crossfade band a part can sit one LOD off its body (sub-cell).

use std::collections::BTreeMap;

use bevy::prelude::*;
use serde::Serialize;

use super::{KitDiag, KitMeshes, LODS, PIECES};
use crate::MarbleFrame;

/// Part angles (millidegrees, sidecar part order) per mechanism instance, keyed by its anchor
/// cell, at the last two physics ticks.
#[derive(Resource, Debug, Default, Clone)]
pub struct PartFrame {
    pub prev: BTreeMap<[i32; 3], Vec<i32>>,
    pub curr: BTreeMap<[i32; 3], Vec<i32>>,
}

/// One part of one piece type: its hinge and its meshes per LOD.
#[derive(Debug, Clone)]
pub struct PartKit {
    /// Index of the part in the sidecar's `animated_parts` (and the sim's part angles).
    pub index: usize,
    /// A slide part: its position is µm along `axis` instead of millidegrees about it.
    pub slide: bool,
    pub pivot: Vec3,
    /// Unit hinge (or slide) axis, piece frame.
    pub axis: Vec3,
    pub rest_mdeg: i32,
    /// The GLB node's rest-pose transform into the piece frame, per LOD.
    pub rest: [Mat4; LODS],
    pub lods: [Handle<Mesh>; LODS],
    /// [`Self::lods`] with reversed winding, for mirrored pieces.
    pub mirrored: [Handle<Mesh>; LODS],
}

/// One LOD instance of an animated part of the piece anchored at `cell`.
#[derive(Component, Debug, Clone, Copy)]
pub struct KitPart {
    pub cell: IVec3,
    pub piece: usize,
    /// [`PartKit`] slot of the piece (not the sidecar index).
    pub slot: usize,
    pub lod: u8,
}

/// One part's pose this frame, for the agent diag (`kit.parts`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartDiag {
    pub cell: [i32; 3],
    pub piece: &'static str,
    pub part: usize,
    /// The angle drawn this frame (interpolated between the two ticks).
    pub angle_mdeg: f32,
    /// The sim's angle at the previous and the current tick; `null` without a sim instance.
    pub prev_mdeg: Option<i32>,
    pub tick_mdeg: Option<i32>,
    pub alpha: f32,
    /// The LOD0 instance's rotation in the piece frame, `[x, y, z, w]`.
    pub rotation: [f32; 4],
}

/// The angle drawn at `alpha` in `[0, 1]` between the previous tick's `prev` (or `curr` when the
/// instance is new this tick) and the current tick's `curr`, like the marbles' `lerp_state`.
pub fn interpolated_mdeg(prev: Option<i32>, curr: i32, alpha: f32) -> f32 {
    let a = prev.unwrap_or(curr) as f32;
    a + (curr as f32 - a) * alpha
}

/// A part's transform into its piece frame at `mdeg`: the rest pose turned by `mdeg − rest_mdeg`
/// about the hinge, which is the sim's pose (`pivot + R(axis, mdeg) · local`) for a node at the
/// sidecar rest pose.
pub fn part_matrix(kit: &PartKit, lod: usize, mdeg: f32) -> Mat4 {
    if kit.slide {
        // `mdeg` is µm along the axis here: the rest pose moved by the difference.
        let d = (mdeg - kit.rest_mdeg as f32) * 1.0e-6;
        return Mat4::from_translation(kit.axis * d) * kit.rest[lod];
    }
    let turn = ((mdeg - kit.rest_mdeg as f32) * 1.0e-3).to_radians();
    Mat4::from_translation(kit.pivot)
        * Mat4::from_axis_angle(kit.axis, turn)
        * Mat4::from_translation(-kit.pivot)
        * kit.rest[lod]
}

/// Poses every part instance from [`PartFrame`] at the marbles' `alpha` and publishes `kit.parts`.
pub(super) fn animate_parts(
    frame: Res<PartFrame>,
    marbles: Res<MarbleFrame>,
    kept: Res<KitMeshes>,
    mut parts: Query<(&KitPart, &mut Transform)>,
    mut diag: ResMut<KitDiag>,
) {
    let alpha = marbles.alpha;
    let mut out = Vec::new();
    for (p, mut t) in &mut parts {
        let Some(kit) = kept.parts.get(p.piece).and_then(|k| k.get(p.slot)) else {
            continue;
        };
        let key = p.cell.to_array();
        let curr = frame.curr.get(&key).and_then(|a| a.get(kit.index)).copied();
        let prev = frame.prev.get(&key).and_then(|a| a.get(kit.index)).copied();
        let mdeg = curr.map_or(kit.rest_mdeg as f32, |c| interpolated_mdeg(prev, c, alpha));
        *t = Transform::from_matrix(part_matrix(kit, usize::from(p.lod), mdeg));
        if p.lod == 0 {
            out.push(PartDiag {
                cell: key,
                piece: PIECES[p.piece].0,
                part: kit.index,
                angle_mdeg: mdeg,
                prev_mdeg: prev,
                tick_mdeg: curr,
                alpha,
                rotation: t.rotation.to_array(),
            });
        }
    }
    out.sort_by_key(|a| (a.cell, a.part));
    if diag.parts != out {
        diag.parts = out;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::{glb, piece_index};

    fn kit_of(id: &str) -> Vec<PartKit> {
        let piece = parble_kit::builtin()
            .iter()
            .find(|p| p.def.id.as_str() == id)
            .unwrap();
        let split = glb::decode_split(PIECES[piece_index(id).unwrap()].1, id).unwrap();
        super::super::part_kits(piece, &split, |_| Handle::default())
    }

    #[test]
    fn the_angle_interpolates_between_ticks_at_alpha() {
        assert_eq!(interpolated_mdeg(Some(0), 80_000, 0.0), 0.0);
        assert_eq!(interpolated_mdeg(Some(0), 80_000, 0.25), 20_000.0);
        assert_eq!(interpolated_mdeg(Some(80_000), 73_334, 0.5), 76_667.0);
        assert_eq!(interpolated_mdeg(Some(-25_000), 25_000, 1.0), 25_000.0);
        // A new instance has no previous tick: it shows the current angle.
        assert_eq!(interpolated_mdeg(None, 7_917, 0.6), 7_917.0);

        // The rendered pose at a fractional alpha is the hinge turned that far between the two
        // ticks: the flap's free edge lies on the arc between its two tick positions.
        let flap = &kit_of("mech_gate_oneway_l1")[0];
        let (a, b) = (80_000, 73_334);
        let at = |mdeg: f32| part_matrix(flap, 0, mdeg);
        let half = at(interpolated_mdeg(Some(a), b, 0.5));
        let mid = Quat::from_mat4(&at(a as f32)).slerp(Quat::from_mat4(&at(b as f32)), 0.5);
        for v in [Vec3::X, Vec3::Y, Vec3::Z] {
            let d = half.transform_vector3(v).distance(mid * v);
            assert!(d < 1e-5, "{v}: {d}");
        }
        // The pivot stays put while the part turns.
        for m in [a as f32, b as f32, 76_667.0] {
            assert!(at(m).transform_point3(Vec3::ZERO).distance(flap.pivot) < 1e-6);
        }
    }

    /// At an angle, the drawn part is the sim's kinematic pose: `pivot + R(axis, mdeg) · local`
    /// on the part-frame vertices, for every mechanism part and LOD.
    #[test]
    fn part_poses_follow_the_sidecar_hinge() {
        let kit = parble_kit::builtin();
        let mut checked = 0;
        for (id, bytes) in PIECES {
            let piece = kit.iter().find(|p| p.def.id.as_str() == *id).unwrap();
            let split = glb::decode_split(bytes, id).unwrap();
            let kits = super::super::part_kits(piece, &split, |_| Handle::default());
            assert_eq!(kits.len(), piece.animated_parts.len(), "{id}");
            for (k, part) in kits.iter().zip(&piece.animated_parts) {
                let [lo, hi] = part.limits_mdeg;
                let slide = part.joint == parble_kit::Joint::Slide;
                for mdeg in [lo, part.rest_mdeg, hi, (lo + hi) / 2] {
                    let axis = Vec3::from_array(part.axis.map(|v| v as f32));
                    // A slide moves `mdeg` µm along the axis, unrotated.
                    let (q, shift) = if slide {
                        (Quat::IDENTITY, axis * mdeg as f32 * 1e-6)
                    } else {
                        (
                            Quat::from_axis_angle(axis, (mdeg as f32 * 1e-3).to_radians()),
                            Vec3::ZERO,
                        )
                    };
                    for (lod, l) in split.iter().enumerate() {
                        let m = part_matrix(k, lod, mdeg as f32);
                        let raw = &l.parts.iter().find(|p| p.name == part.name).unwrap().mesh;
                        for p in &raw.positions {
                            let local = Vec3::from_array(*p);
                            let got = m.transform_point3(local);
                            let want = k.pivot + shift + q * local;
                            assert!(got.distance(want) < 2e-5, "{id} {mdeg}: {got} vs {want}");
                        }
                    }
                }
                checked += 1;
            }
        }
        assert_eq!(checked, 6, "mechanism parts");
    }

    /// The split LODs at the rest angle are exactly the old merged rest-pose meshes.
    #[test]
    fn body_and_parts_at_rest_rebuild_the_merged_lods() {
        for (id, bytes) in PIECES {
            let merged = glb::decode_lods(bytes, id).unwrap();
            let split = glb::decode_split(bytes, id).unwrap();
            for (m, s) in merged.iter().zip(&split) {
                let mut rebuilt = s.body.clone();
                for p in &s.parts {
                    rebuilt.append(&p.mesh.transformed(&p.rest));
                }
                assert_eq!(&rebuilt, m, "{id}");
            }
        }
    }
}
