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
//! - A chain part (the rope arch's ropes, [`parble_sim::rope`]) is drawn as [`ROPE_LOD_BANDS`]
//!   bands per LOD, cut from its mesh by height, each posed rigidly along the sim's segment
//!   polyline ([`band_matrix`]), so the strand bends where the sim bends it.

use std::collections::BTreeMap;

use bevy::prelude::*;
use serde::Serialize;

use super::glb::LodMesh;
use super::{KitDiag, KitMeshes, LODS, PIECES};
use crate::MarbleFrame;

/// One mechanism instance's part poses at one tick: part angles (millidegrees, sidecar part
/// order) and, per part, its segment angles when it is a chain (else empty).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PartPoses {
    pub angles: Vec<i32>,
    pub segments: Vec<Vec<i32>>,
}

/// Part poses per mechanism instance, keyed by its anchor cell, at the last two physics ticks.
#[derive(Resource, Debug, Default, Clone)]
pub struct PartFrame {
    pub prev: BTreeMap<[i32; 3], PartPoses>,
    pub curr: BTreeMap<[i32; 3], PartPoses>,
}

/// Bands a rope of the rope arch is cut into per LOD: each covers `SEGMENTS / bands` sim
/// segments. The generator puts strand rings at these heights (`blender/kit/kitmech.py`). A band
/// over several segments is drawn as their chord, which cuts the inside of a sharp bend, where the
/// marble sits, through the clearance the sim keeps, so LOD0 and LOD1 draw one band per segment.
/// LOD2 keeps two: the §12 part budget (120 triangles for all eight ropes) has no room for six,
/// and its chords are held to at most a pixel inside the marble where LOD2 starts (`parble_sim`
/// test `rope_arch_ropes_bend_round_the_marble_and_never_pass_through_it`).
pub const ROPE_LOD_BANDS: [usize; LODS] =
    [parble_sim::rope::SEGMENTS, parble_sim::rope::SEGMENTS, 2];

/// Bands per LOD of part `part` of piece `piece`: [`ROPE_LOD_BANDS`] for the rope arch's ropes, 1
/// for a rigid part.
pub fn part_bands(piece: &str) -> [usize; LODS] {
    if piece == parble_sim::behaviour::ROPE_ARCH {
        ROPE_LOD_BANDS
    } else {
        [1; LODS]
    }
}

/// `mesh` (a part's mesh in its part frame, the pivot at the origin and the strand hanging down
/// −Y) cut into `bands` equal height bands of the rope: each triangle goes to the band holding
/// its centroid, the top band also taking anything above the pivot and the bottom band anything
/// below the rope's end.
pub fn split_bands(mesh: &LodMesh, bands: usize) -> Vec<LodMesh> {
    if bands <= 1 {
        return vec![mesh.clone()];
    }
    let band_m = parble_sim::rope::LENGTH_UM as f32 * 1.0e-6 / bands as f32;
    let mut out = vec![
        LodMesh {
            indices: Vec::new(),
            ..mesh.clone()
        };
        bands
    ];
    for t in mesh.indices.as_chunks::<3>().0 {
        let y: f32 = t
            .iter()
            .map(|&i| mesh.positions[i as usize][1])
            .sum::<f32>()
            / 3.0;
        let b = ((-y / band_m).floor().max(0.0) as usize).min(bands - 1);
        out[b].indices.extend_from_slice(t);
    }
    out
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
    /// Per LOD, the part's mesh, or its bands top first for a chain part ([`split_bands`]).
    pub lods: [Vec<Handle<Mesh>>; LODS],
    /// [`Self::lods`] with reversed winding, for mirrored pieces.
    pub mirrored: [Vec<Handle<Mesh>>; LODS],
}

/// One LOD instance of an animated part of the piece anchored at `cell`.
#[derive(Component, Debug, Clone, Copy)]
pub struct KitPart {
    pub cell: IVec3,
    pub piece: usize,
    /// [`PartKit`] slot of the piece (not the sidecar index).
    pub slot: usize,
    pub lod: u8,
    /// Band of a chain part (0 for a rigid part).
    pub band: u8,
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
    /// The LOD0 instance's rotation in the piece frame, `[x, y, z, w]` (a chain part's top band).
    pub rotation: [f32; 4],
    /// A chain part's segment angles drawn this frame (interpolated between the two ticks), top
    /// first; empty for a rigid part.
    pub segments_mdeg: Vec<f32>,
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

/// Band `band` of chain part `kit` at LOD `lod` posed along segment angles `segments`
/// (millidegrees, top first): the band's rest strand (straight down from its top joint) turned
/// about the pivot axis to the chord of the segments it covers (and stretched to its length), its
/// top joint moved to the polyline's ([`parble_sim::rope::polyline_mdeg`]). With every segment at one angle it is
/// [`part_matrix`] at that angle.
pub fn band_matrix(kit: &PartKit, lod: usize, band: usize, bands: usize, segments: &[f32]) -> Mat4 {
    use parble_sim::rope::{SEGMENT_M, SEGMENTS};
    let per = SEGMENTS / bands.max(1);
    let (s0, s1) = (band * per, (band + 1) * per);
    let mut rad = [0.0_f32; SEGMENTS];
    for (r, &m) in rad.iter_mut().zip(segments) {
        *r = ((m - kit.rest_mdeg as f32) * 1.0e-3).to_radians();
    }
    // Joints in the swing plane: `u` along the swing, `y` up, from the pivot.
    let mut joints = [(0.0_f32, 0.0_f32); SEGMENTS + 1];
    for j in 0..SEGMENTS {
        let (u, y) = joints[j];
        joints[j + 1] = (u + SEGMENT_M * rad[j].sin(), y - SEGMENT_M * rad[j].cos());
    }
    let (a, b) = (joints[s0], joints[s1]);
    let chord = (b.0 - a.0).atan2(-(b.1 - a.1));
    // A band over several segments is stretched along itself to the chord, so it ends on the
    // next band's top joint (1 for a band of one segment).
    let stretch = (b.0 - a.0).hypot(b.1 - a.1) / (SEGMENT_M * (s1 - s0) as f32);
    // A positive turn about the axis swings −Y toward `axis × −Y`.
    let swing = kit.axis.cross(Vec3::NEG_Y);
    let top = swing * a.0 + Vec3::Y * a.1;
    let rest_top = Vec3::NEG_Y * (s0 as f32 * SEGMENT_M);
    Mat4::from_translation(kit.pivot + top)
        * Mat4::from_axis_angle(kit.axis, chord)
        * Mat4::from_scale(Vec3::new(1.0, stretch, 1.0))
        * Mat4::from_translation(-rest_top - kit.pivot)
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
        let (now, before) = (frame.curr.get(&key), frame.prev.get(&key));
        let curr = now.and_then(|a| a.angles.get(kit.index)).copied();
        let prev = before.and_then(|a| a.angles.get(kit.index)).copied();
        let mdeg = curr.map_or(kit.rest_mdeg as f32, |c| interpolated_mdeg(prev, c, alpha));
        let lod = usize::from(p.lod);
        let bands = kit.lods[lod].len();
        let seg_now = now.and_then(|a| a.segments.get(kit.index));
        let seg_before = before.and_then(|a| a.segments.get(kit.index));
        let segments: Vec<f32> = match seg_now {
            Some(c) if !c.is_empty() => c
                .iter()
                .enumerate()
                .map(|(j, &m)| {
                    interpolated_mdeg(seg_before.and_then(|b| b.get(j)).copied(), m, alpha)
                })
                .collect(),
            _ => Vec::new(),
        };
        let m = if bands > 1 {
            // A chain part with no sim instance behind it hangs at its rest angle.
            let rest = vec![kit.rest_mdeg as f32; parble_sim::rope::SEGMENTS];
            let s = if segments.is_empty() {
                &rest
            } else {
                &segments
            };
            band_matrix(kit, lod, usize::from(p.band), bands, s)
        } else {
            part_matrix(kit, lod, mdeg)
        };
        *t = Transform::from_matrix(m);
        if p.lod == 0 && p.band == 0 {
            out.push(PartDiag {
                cell: key,
                piece: PIECES[p.piece].0,
                part: kit.index,
                angle_mdeg: mdeg,
                prev_mdeg: prev,
                tick_mdeg: curr,
                alpha,
                rotation: t.rotation.to_array(),
                segments_mdeg: segments,
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
        assert_eq!(checked, 15, "mechanism parts");
    }

    /// A rope's bands: every band of every LOD holds triangles, the bands together are the whole
    /// strand, and with every segment at one angle each band is posed exactly as the rigid part
    /// at that angle. Bent, consecutive bands meet at their shared joint.
    #[test]
    fn rope_bands_cover_the_strand_and_follow_the_segments() {
        let piece = parble_sim::behaviour::ROPE_ARCH;
        let kits = kit_of(piece);
        let split = glb::decode_split(PIECES[piece_index(piece).unwrap()].1, piece).unwrap();
        assert_eq!(kits.len(), 8);
        for (lod, l) in split.iter().enumerate() {
            let bands = part_bands(piece)[lod];
            for p in &l.parts {
                let cut = split_bands(&p.mesh, bands);
                assert_eq!(cut.len(), bands);
                assert!(
                    cut.iter().all(|b| !b.indices.is_empty()),
                    "{} lod{lod}",
                    p.name
                );
                let n: usize = cut.iter().map(LodMesh::triangles).sum();
                assert_eq!(n, p.mesh.triangles());
            }
        }
        let k = &kits[1];
        for (lod, &bands) in ROPE_LOD_BANDS.iter().enumerate() {
            if lod < 2 {
                assert_eq!(
                    bands,
                    parble_sim::rope::SEGMENTS,
                    "lod{lod} draws a band per segment"
                );
            }
            assert_eq!(k.lods[lod].len(), bands);
            for mdeg in [-30_000.0_f32, 0.0, 12_500.0] {
                let seg = [mdeg; parble_sim::rope::SEGMENTS];
                for b in 0..bands {
                    let got = band_matrix(k, lod, b, bands, &seg);
                    let want = part_matrix(k, lod, mdeg);
                    assert!(got.abs_diff_eq(want, 1e-5), "lod{lod} band {b} at {mdeg}");
                }
            }
            // Bent: each band's bottom joint is the next band's top joint.
            let seg = [0.0, 5_000.0, 15_000.0, 40_000.0, 70_000.0, 95_000.0];
            let per = parble_sim::rope::SEGMENTS / bands;
            let len = parble_sim::rope::SEGMENT_M * per as f32;
            let joint = |m: Mat4, b: usize| {
                let local = Vec3::NEG_Y * (b as f32 * len);
                (m * k.rest[lod].inverse()).transform_point3(k.pivot + local)
            };
            for b in 0..bands - 1 {
                let end = joint(band_matrix(k, lod, b, bands, &seg), b + 1);
                let start = joint(band_matrix(k, lod, b + 1, bands, &seg), b + 1);
                assert!(
                    end.distance(start) < 1e-5,
                    "lod{lod} joint {b}: {end} vs {start}"
                );
            }
        }
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
