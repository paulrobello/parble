//! Editor-placed kit pieces (P3-T4): incremental chunk respawn, height slicing, the ghost preview
//! with port hints, and selection marks.
//!
//! The app owns the track. After an edit it writes the full new content of every chunk the edit
//! touched to [`KitEdits::pending`]; [`apply_edits`] despawns those [`KitChunk`] entities and
//! spawns them again, and leaves every other chunk alone. Render chunks are the cell chunks of the
//! anchors, the same 8 x 8 x 8 cells the sim's colliders use.

use std::collections::{BTreeMap, HashMap};

use bevy::prelude::*;
use serde::Serialize;

use super::{
    CHUNK_CELLS, KitChunk, KitDiag, KitInstance, KitMeshes, KitScene, KitTextures, LODS,
    cell_origin, pair_draws, spawn_parts, variant_of,
};

/// Floor under the editor area, min and max (x, z) corners in metres: room for the 2,000-piece
/// fill grid behind the tray.
pub const EDITOR_FLOOR_M: ([f32; 2], [f32; 2]) = ([-1.2, -3.2], [3.6, 1.2]);
/// Selection marks drawn at most (a larger selection still edits; only the marks are capped).
pub const MAX_MARKS: usize = 1024;
/// Port hint markers drawn at most.
const MAX_PORTS: usize = 8;

/// One editor-placed piece: index into [`super::PIECES`], anchor cell, quarter turns and mirror.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditPiece {
    pub piece: usize,
    pub cell: IVec3,
    pub yaw: u8,
    pub mirror: bool,
}

impl EditPiece {
    /// Mirror (x → −x) first, then yaw, like `Orientation::apply_cell`.
    pub fn transform(&self) -> Transform {
        let mut t = Transform::from_translation(cell_origin(self.cell)).with_rotation(
            Quat::from_rotation_y(f32::from(self.yaw) * std::f32::consts::FRAC_PI_2),
        );
        if self.mirror {
            t.scale.x = -1.0;
        }
        t
    }
}

/// Render chunk of an anchor cell.
pub fn chunk_of_cell(cell: IVec3) -> [i32; 3] {
    cell.div_euclid(IVec3::splat(CHUNK_CELLS)).to_array()
}

/// Edits from the app, and the spawned state.
#[derive(Resource, Debug, Default)]
pub struct KitEdits {
    /// Chunk → its complete new content. An empty list despawns the chunk.
    pub pending: BTreeMap<[i32; 3], Vec<EditPiece>>,
    /// Show only pieces whose anchor level is at most this (height slicing).
    pub slice: Option<i32>,
    entities: HashMap<[i32; 3], Entity>,
    /// Chunk → (piece, mirrored, anchor level) of its pieces.
    content: BTreeMap<[i32; 3], Vec<(usize, bool, i32)>>,
    applied_slice: Option<i32>,
}

/// An instance's anchor level, for slicing.
#[derive(Component, Debug, Clone, Copy)]
pub struct KitLayer(pub i32);

/// How a ghost port meets the cell across its face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PortHint {
    /// A mating port across: the two link.
    Linked,
    /// Nothing across.
    Open,
    /// A piece across that does not link here.
    Fault,
}

/// The ghost preview: the brush piece at the hovered cell, tinted by validity, with one marker per
/// port.
#[derive(Resource, Debug, Default, Clone)]
pub struct KitGhost {
    pub piece: Option<EditPiece>,
    pub valid: bool,
    /// World position and hint of each port.
    pub ports: Vec<(Vec3, PortHint)>,
}

/// Selection marks: the selected anchors, shown shifted by `offset` cells while a move drags.
#[derive(Resource, Debug, Default, Clone)]
pub struct KitMarks {
    pub cells: Vec<IVec3>,
    pub offset: IVec3,
    pub valid: bool,
}

/// Editor piece counts for the diag (`kit.edit`).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KitEditStats {
    pub pieces: usize,
    pub chunks: usize,
    /// Pieces hidden by the height slice.
    pub hidden_pieces: usize,
    /// Chunks respawned by the last frame that had any, and that frame's spawn time (ms).
    pub last_chunks: usize,
    pub last_ms: f32,
}

#[derive(Resource, Default)]
struct EditVisuals {
    ghost: Option<Entity>,
    ghost_ok: Handle<StandardMaterial>,
    ghost_bad: Handle<StandardMaterial>,
    ports: Vec<Entity>,
    port_mats: [Handle<StandardMaterial>; 3],
    marks: Vec<Entity>,
    mark_ok: Handle<StandardMaterial>,
    mark_bad: Handle<StandardMaterial>,
}

pub struct KitEditPlugin;

impl Plugin for KitEditPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<KitEdits>()
            .init_resource::<KitGhost>()
            .init_resource::<KitMarks>()
            .init_resource::<EditVisuals>()
            .add_systems(Startup, setup_visuals)
            .add_systems(
                PostUpdate,
                (apply_edits, apply_slice, show_ghost, show_marks).chain(),
            );
    }
}

fn tint(materials: &mut Assets<StandardMaterial>, c: Color) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: c,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    })
}

fn setup_visuals(
    mut commands: Commands,
    scene: Res<KitScene>,
    mut vis: ResMut<EditVisuals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if *scene != KitScene::Editor {
        return;
    }
    vis.ghost_ok = tint(&mut materials, Color::srgba(0.35, 0.95, 0.45, 0.45));
    vis.ghost_bad = tint(&mut materials, Color::srgba(1.0, 0.25, 0.2, 0.45));
    vis.port_mats = [
        tint(&mut materials, Color::srgba(0.3, 1.0, 0.4, 0.95)),
        tint(&mut materials, Color::srgba(1.0, 0.8, 0.2, 0.95)),
        tint(&mut materials, Color::srgba(1.0, 0.2, 0.2, 0.95)),
    ];
    vis.mark_ok = tint(&mut materials, Color::srgba(0.3, 0.6, 1.0, 0.28));
    vis.mark_bad = tint(&mut materials, Color::srgba(1.0, 0.25, 0.2, 0.28));
    let dot = meshes.add(Sphere::new(0.006).mesh().ico(2).expect("ico sphere"));
    vis.ports = (0..MAX_PORTS)
        .map(|_| {
            commands
                .spawn((
                    Mesh3d(dot.clone()),
                    MeshMaterial3d(vis.port_mats[1].clone()),
                    Transform::default(),
                    Visibility::Hidden,
                ))
                .id()
        })
        .collect();
    let s = super::CELL_M * 1.02;
    let cube = meshes.add(Cuboid::new(s, s, s));
    vis.marks = (0..MAX_MARKS)
        .map(|_| {
            commands
                .spawn((
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(vis.mark_ok.clone()),
                    Transform::default(),
                    Visibility::Hidden,
                ))
                .id()
        })
        .collect();
}

fn visible(slice: Option<i32>, level: i32) -> Visibility {
    match slice {
        Some(s) if level > s => Visibility::Hidden,
        _ => Visibility::Inherited,
    }
}

/// Respawns the pending chunks only.
fn apply_edits(
    mut commands: Commands,
    mut edits: ResMut<KitEdits>,
    kept: Res<KitMeshes>,
    mut tex: ResMut<KitTextures>,
    mut diag: ResMut<KitDiag>,
) {
    if edits.pending.is_empty() {
        return;
    }
    let Some(ranges) = kept.ranges.clone() else {
        return;
    };
    let start = bevy::platform::time::Instant::now();
    let pending = std::mem::take(&mut edits.pending);
    let slice = edits.slice;
    let n = pending.len();
    for (chunk, pieces) in pending {
        if let Some(e) = edits.entities.remove(&chunk) {
            commands.entity(e).despawn();
        }
        if pieces.is_empty() {
            edits.content.remove(&chunk);
            continue;
        }
        tex.wanted.extend(pieces.iter().map(|p| p.piece));
        let root = commands
            .spawn((
                KitChunk(IVec3::from_array(chunk)),
                Transform::default(),
                Visibility::default(),
            ))
            .with_children(|c| {
                for p in &pieces {
                    let t = p.transform();
                    let v = variant_of(p.cell);
                    let lods = if p.mirror {
                        &kept.mirrored[p.piece]
                    } else {
                        &kept.lods[p.piece]
                    };
                    for (lod, range) in ranges.iter().enumerate().take(LODS) {
                        c.spawn((
                            KitInstance {
                                piece: p.piece,
                                lod: lod as u8,
                            },
                            KitLayer(p.cell.y),
                            Mesh3d(lods[lod].clone()),
                            MeshMaterial3d(tex.materials[p.piece][lod][v].clone()),
                            t,
                            range.clone(),
                            visible(slice, p.cell.y),
                        ));
                    }
                    // The parts' parent slices with the piece; its children inherit.
                    let mats = &tex.materials[p.piece];
                    let root = (KitLayer(p.cell.y), visible(slice, p.cell.y));
                    spawn_parts(c, &kept, mats, p.piece, p.cell, v, t, p.mirror, root);
                }
            })
            .id();
        edits.entities.insert(chunk, root);
        edits.content.insert(
            chunk,
            pieces
                .iter()
                .map(|p| (p.piece, p.mirror, p.cell.y))
                .collect(),
        );
    }
    let ms = start.elapsed().as_secs_f32() * 1e3;
    let mut pairs = std::collections::BTreeSet::new();
    let mut pieces = 0;
    for (chunk, list) in &edits.content {
        pieces += list.len();
        // A mirrored piece draws its own (reversed winding) mesh.
        for &(piece, mirror, _) in list {
            pairs.insert((piece, mirror, *chunk));
        }
    }
    diag.pieces = pieces;
    diag.chunks = edits.content.len();
    diag.piece_types = pairs
        .iter()
        .map(|&(piece, _, _)| piece)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    diag.draw_bound = pairs.iter().map(|&(piece, _, _)| pair_draws(piece)).sum();
    let hidden = hidden_pieces(&edits);
    diag.edit = Some(KitEditStats {
        pieces,
        chunks: edits.content.len(),
        hidden_pieces: hidden,
        last_chunks: n,
        last_ms: ms,
    });
}

fn hidden_pieces(edits: &KitEdits) -> usize {
    let Some(s) = edits.slice else {
        return 0;
    };
    edits
        .content
        .values()
        .flatten()
        .filter(|&&(_, _, y)| y > s)
        .count()
}

/// Re-applies visibility to every editor instance when the slice changes.
fn apply_slice(
    mut edits: ResMut<KitEdits>,
    mut diag: ResMut<KitDiag>,
    mut q: Query<(&KitLayer, &mut Visibility)>,
) {
    if edits.applied_slice == edits.slice {
        return;
    }
    edits.applied_slice = edits.slice;
    for (layer, mut v) in &mut q {
        *v = visible(edits.slice, layer.0);
    }
    let hidden = hidden_pieces(&edits);
    if let Some(e) = diag.edit.as_mut() {
        e.hidden_pieces = hidden;
    }
}

fn show_ghost(
    mut commands: Commands,
    ghost: Res<KitGhost>,
    kept: Res<KitMeshes>,
    mut vis: ResMut<EditVisuals>,
    mut q: Query<(
        &mut Transform,
        &mut Visibility,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
    mut meshes: Query<&mut Mesh3d>,
) {
    if !ghost.is_changed() || vis.ports.is_empty() || kept.ghost.is_empty() {
        return;
    }
    match ghost.piece {
        Some(p) => {
            let mat = if ghost.valid {
                vis.ghost_ok.clone()
            } else {
                vis.ghost_bad.clone()
            };
            // The whole piece, parts at rest.
            let mesh = kept.ghost[p.piece][usize::from(p.mirror)].clone();
            let mut t = p.transform();
            // Slightly proud of the cell so it never z-fights a neighbour.
            t.scale *= 1.01;
            match vis.ghost {
                Some(e) => {
                    if let Ok((mut tf, mut v, mut m)) = q.get_mut(e) {
                        *tf = t;
                        *v = Visibility::Inherited;
                        m.0 = mat;
                    }
                    if let Ok(mut m3) = meshes.get_mut(e) {
                        m3.0 = mesh;
                    }
                }
                None => {
                    vis.ghost = Some(
                        commands
                            .spawn((Mesh3d(mesh), MeshMaterial3d(mat), t, Visibility::Inherited))
                            .id(),
                    );
                }
            }
        }
        None => {
            if let Some(e) = vis.ghost
                && let Ok((_, mut v, _)) = q.get_mut(e)
            {
                *v = Visibility::Hidden;
            }
        }
    }
    let ports = vis.ports.clone();
    for (i, e) in ports.into_iter().enumerate() {
        let Ok((mut tf, mut v, mut m)) = q.get_mut(e) else {
            continue;
        };
        match ghost.ports.get(i).filter(|_| ghost.piece.is_some()) {
            Some(&(pos, hint)) => {
                tf.translation = pos;
                *v = Visibility::Inherited;
                m.0 = vis.port_mats[hint as usize].clone();
            }
            None => *v = Visibility::Hidden,
        }
    }
}

fn show_marks(
    marks: Res<KitMarks>,
    vis: Res<EditVisuals>,
    mut q: Query<(
        &mut Transform,
        &mut Visibility,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    if !marks.is_changed() {
        return;
    }
    let mat = if marks.valid {
        &vis.mark_ok
    } else {
        &vis.mark_bad
    };
    for (i, &e) in vis.marks.iter().enumerate() {
        let Ok((mut tf, mut v, mut m)) = q.get_mut(e) else {
            continue;
        };
        match marks.cells.get(i) {
            Some(&c) => {
                tf.translation = cell_origin(c + marks.offset) + Vec3::Y * (super::CELL_M * 0.5);
                *v = Visibility::Inherited;
                if m.0 != *mat {
                    m.0 = mat.clone();
                }
            }
            None => {
                if *v != Visibility::Hidden {
                    *v = Visibility::Hidden;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_follow_the_sim_chunking() {
        assert_eq!(chunk_of_cell(IVec3::new(7, 0, 8)), [0, 0, 1]);
        assert_eq!(chunk_of_cell(IVec3::new(-1, -8, -9)), [-1, -1, -2]);
    }

    #[test]
    fn mirrored_transform_matches_apply_cell() {
        // Mirror then a quarter turn: local (1, 0, 0) → (−1, 0, 0) → (0, 0, 1).
        let p = EditPiece {
            piece: 0,
            cell: IVec3::ZERO,
            yaw: 1,
            mirror: true,
        };
        let v = p.transform().transform_point(Vec3::X);
        assert!((v - Vec3::Z).length() < 1e-5, "{v}");
    }

    #[test]
    fn slicing_hides_levels_above() {
        assert_eq!(visible(Some(1), 2), Visibility::Hidden);
        assert_eq!(visible(Some(1), 1), Visibility::Inherited);
        assert_eq!(visible(None, 9), Visibility::Inherited);
    }
}
