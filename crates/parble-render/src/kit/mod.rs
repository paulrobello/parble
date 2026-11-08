//! Instanced, chunked kit-piece renderer (P1-T4).
//!
//! - Instancing: one [`Mesh`] per (piece type, LOD) and one shared wood material, so Bevy's GPU
//!   preprocessing bins every instance of a (type, LOD) into one indirect draw per view, below the
//!   P1-T4 bound of one per (type × LOD × chunk). Instances are grouped under one [`KitChunk`]
//!   entity per 8×8×8-cell chunk (kit contract §2).
//! - LOD: each piece spawns all three LODs with contiguous [`VisibilityRange`]s from the §12
//!   thresholds (one cell 160 px / 48 px tall at 1440p) and a ±15 % margin. Bevy draws both LODs in
//!   the margin with complementary screen-space dither (`VISIBILITY_RANGE_DITHER`, main pass and
//!   prepass/shadows), so there is no hard cut. The fade is measured from each piece's origin, which
//!   is finer than §12's per-chunk nearest point.
//! - Textures: the wood set loads as BC7 KTX2, else ASTC KTX2, else PNG ([`family`]), chosen once the
//!   render device exists (on the web it arrives after `Startup`).
//! - The draw counts in [`KitDiag`] are read from the render world's binned phases ([`count_draws`]).

pub mod family;
pub mod glb;

use std::{
    collections::{BTreeSet, HashSet},
    sync::{Arc, Mutex},
};

use bevy::{
    asset::{UntypedAssetId, io::AssetSourceBuilder},
    camera::visibility::VisibilityRange,
    core_pipeline::{core_3d::Opaque3d, prepass::Opaque3dPrepass},
    image::ImageLoaderSettings,
    math::Affine2,
    pbr::{RenderMaterialInstances, RenderMeshInstances, Shadow},
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        render_phase::{BinnedPhaseItem, BinnedRenderPhase, ViewBinnedRenderPhases},
        renderer::RenderDevice,
        settings::WgpuFeatures,
        sync_world::MainEntity,
        view::{ExtractedView, RenderVisibilityRanges, RenderVisibleEntities},
    },
};
use serde::Serialize;

use family::{TextureFamily, select_family_capped};
use glb::{LODS, LodMesh};

/// Cell edge, metres (kit contract §2).
pub const CELL_M: f32 = 0.06;
/// Chunk edge in cells (kit contract §2).
pub const CHUNK_CELLS: i32 = 8;
/// §12 switch thresholds: projected height of one cell at 1440p, LOD0→1 and LOD1→2.
pub const LOD_PX: [f32; LODS - 1] = [160.0, 48.0];
/// §12 crossfade margin around each threshold.
pub const LOD_FADE: f32 = 0.15;
pub const LOD_REF_HEIGHT_PX: f32 = 1440.0;
/// End of the last LOD's range (metres): far past any camera in the playroom.
const LOD_FAR_M: f32 = 1.0e4;

/// Kit GLBs, embedded (about 230 KB together).
pub const PIECES: [(&str, &[u8]); 4] = [
    (
        "blk_straight_l3",
        include_bytes!("../../../../assets/kit/blk_straight_l3/blk_straight_l3.glb"),
    ),
    (
        "blk_curve90_l3",
        include_bytes!("../../../../assets/kit/blk_curve90_l3/blk_curve90_l3.glb"),
    ),
    (
        "blk_slope_gentle_l3",
        include_bytes!("../../../../assets/kit/blk_slope_gentle_l3/blk_slope_gentle_l3.glb"),
    ),
    (
        "blk_end_l3",
        include_bytes!("../../../../assets/kit/blk_end_l3/blk_end_l3.glb"),
    ),
];

/// Asset source for the served `assets/` tree (web: `dist/assets`, copied by Trunk).
pub const ASSET_SOURCE: &str = "parble";
/// Wood set the kit material samples, and its maps.
pub const WOOD_SET: &str = "wood_warm_a";
pub const WOOD_MAPS: [&str; 2] = ["base_color", "orm"];
/// One wood repeat is 250 mm and a piece's UV0 atlas spans one 60 mm cell.
const WOOD_UV_SCALE: f32 = 0.24;

/// Path of a wood map in `family`, relative to the `assets/` root.
pub fn wood_path(map: &str, family: TextureFamily) -> String {
    format!("materials/{WOOD_SET}/{WOOD_SET}.{map}.{}", family.suffix())
}

/// Registers the [`ASSET_SOURCE`] source. Add it before `DefaultPlugins` (Bevy reads asset sources
/// when `AssetPlugin` builds).
pub struct KitAssetSourcePlugin;

impl Plugin for KitAssetSourcePlugin {
    fn build(&self, app: &mut App) {
        #[cfg(not(target_arch = "wasm32"))]
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
        #[cfg(target_arch = "wasm32")]
        let root = "assets";
        app.register_asset_source(
            ASSET_SOURCE,
            AssetSourceBuilder::platform_default(root, None),
        );
    }
}

/// Which kit layout to spawn (set by the app from `--scenario`).
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum KitScene {
    #[default]
    None,
    /// 2,000 pieces in a 50 × 10 × 4-cell wall beside the tray.
    Stress2000,
}

/// Agent cap on the texture family (`--tex`); see [`family::select_family_capped`].
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextureCap(pub Option<TextureFamily>);

/// One placed piece: index into [`PIECES`], anchor cell and yaw in quarter turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub piece: usize,
    pub cell: IVec3,
    pub yaw: u8,
}

/// Stress wall size in cells, and its first cell.
pub const STRESS_DIMS: IVec3 = IVec3::new(50, 10, 4);
pub const STRESS_ORIGIN: IVec3 = IVec3::new(0, 0, -16);

fn cell_hash(c: IVec3) -> u32 {
    let mut h = (c.x as u32).wrapping_mul(0x8da6_b343)
        ^ (c.y as u32).wrapping_mul(0xd816_3841)
        ^ (c.z as u32).wrapping_mul(0xcb1a_b31f);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^ (h >> 12)
}

/// The 2,000-piece layout: every cell of [`STRESS_DIMS`], piece type and yaw from a hash of the cell.
pub fn stress_layout() -> Vec<Placement> {
    let mut out = Vec::with_capacity((STRESS_DIMS.x * STRESS_DIMS.y * STRESS_DIMS.z) as usize);
    for y in 0..STRESS_DIMS.y {
        for z in 0..STRESS_DIMS.z {
            for x in 0..STRESS_DIMS.x {
                let cell = STRESS_ORIGIN + IVec3::new(x, y, z);
                let h = cell_hash(cell);
                out.push(Placement {
                    piece: (h % PIECES.len() as u32) as usize,
                    cell,
                    yaw: ((h >> 8) % 4) as u8,
                });
            }
        }
    }
    out
}

pub fn chunk_of(cell: IVec3) -> IVec3 {
    cell.div_euclid(IVec3::splat(CHUNK_CELLS))
}

/// World position of a cell's piece-local origin (bottom-face centre, kit contract §1).
pub fn cell_origin(cell: IVec3) -> Vec3 {
    cell.as_vec3() * CELL_M
}

pub fn placement_transform(p: &Placement) -> Transform {
    Transform::from_translation(cell_origin(p.cell)).with_rotation(Quat::from_rotation_y(
        f32::from(p.yaw) * std::f32::consts::FRAC_PI_2,
    ))
}

/// Occupied chunks and the contract bound of `LODS` draws per occupied (type, chunk).
pub fn chunk_stats(layout: &[Placement]) -> (usize, usize) {
    let chunks: BTreeSet<_> = layout.iter().map(|p| chunk_of(p.cell).to_array()).collect();
    let pairs: BTreeSet<_> = layout
        .iter()
        .map(|p| (p.piece, chunk_of(p.cell).to_array()))
        .collect();
    (chunks.len(), pairs.len() * LODS)
}

/// Camera distance (m) at which one cell projects to `px` pixels tall at 1440p.
pub fn lod_distance(px: f32, fov_y: f32) -> f32 {
    CELL_M * LOD_REF_HEIGHT_PX / (2.0 * (fov_y * 0.5).tan() * px)
}

/// Contiguous LOD0..LOD2 ranges: each threshold fades over ±[`LOD_FADE`] of its pixel height, and
/// a LOD's `end_margin` is exactly the next LOD's `start_margin` (Bevy's crossfade requirement).
pub fn lod_ranges(fov_y: f32) -> [VisibilityRange; LODS] {
    let band = |px: f32| {
        lod_distance(px * (1.0 + LOD_FADE), fov_y)..lod_distance(px * (1.0 - LOD_FADE), fov_y)
    };
    let b0 = band(LOD_PX[0]);
    let b1 = band(LOD_PX[1]);
    [
        VisibilityRange {
            start_margin: 0.0..0.0,
            end_margin: b0.clone(),
            use_aabb: false,
        },
        VisibilityRange {
            start_margin: b0,
            end_margin: b1.clone(),
            use_aabb: false,
        },
        VisibilityRange {
            start_margin: b1,
            end_margin: LOD_FAR_M..LOD_FAR_M,
            use_aabb: false,
        },
    ]
}

pub fn lod_render_mesh(m: &LodMesh) -> Mesh {
    Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, m.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, m.normals.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, m.uvs.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, m.tangents.clone())
    .with_inserted_indices(bevy::mesh::Indices::U32(m.indices.clone()))
}

/// A chunk's root entity; its children are the chunk's piece LOD instances.
#[derive(Component, Debug, Clone, Copy)]
pub struct KitChunk(pub IVec3);

/// One LOD instance of a placed piece.
#[derive(Component, Debug, Clone, Copy)]
pub struct KitInstance {
    pub piece: usize,
    pub lod: u8,
}

/// Draw counts read from the render world's binned phases (kit meshes only), summed over views.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KitDraws {
    /// Main opaque pass: one indirect draw per non-empty (mesh, material) bin.
    pub main: u32,
    pub main_views: u32,
    /// Instances in the main-pass bins (CPU-visible LOD instances).
    pub main_instances: u32,
    /// Main-pass instances whose visibility range crossfades (drawn with the dither pipeline).
    pub dithered_instances: u32,
    /// `None` when a pass's multidraw bins could not be rebuilt (shadow cascades carry no
    /// per-view visible list in the render world).
    pub prepass: Option<u32>,
    pub shadow: Option<u32>,
    pub shadow_views: u32,
    /// Multidraw batch sets in the main pass (their bins are not attributable; 0 on WebGPU).
    pub multidraw_sets: u32,
}

#[derive(Default)]
struct Shared {
    meshes: HashSet<UntypedAssetId>,
    draws: KitDraws,
}

#[derive(Resource, Clone, Default)]
struct KitShared(Arc<Mutex<Shared>>);

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextureDiag {
    pub path: String,
    pub state: &'static str,
    /// `wgpu::TextureFormat` of the loaded image.
    pub format: Option<String>,
}

/// Kit renderer state for the agent diag (`kit` in `--dump-state` and `window.__parble`).
#[derive(Resource, Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KitDiag {
    pub pieces: usize,
    pub chunks: usize,
    pub piece_types: usize,
    /// One draw per occupied (type × chunk) × LOD (the P1-T4 bound).
    pub draw_bound: usize,
    pub lod_triangles: Vec<[usize; LODS]>,
    /// Camera distances (m) of the LOD bands: `[lod0 end start, end, lod1 end start, end]`.
    pub lod_bands_m: [f32; 4],
    pub family: Option<TextureFamily>,
    /// The render device's BC and ASTC texture compression features, as read for `family`.
    pub device_bc: Option<bool>,
    pub device_astc: Option<bool>,
    pub textures: Vec<TextureDiag>,
    pub draws: KitDraws,
}

#[derive(Resource, Default)]
struct KitTextures {
    material: Handle<StandardMaterial>,
    images: Vec<(String, Handle<Image>)>,
}

pub struct KitPlugin;

impl Plugin for KitPlugin {
    fn build(&self, app: &mut App) {
        let shared = KitShared::default();
        app.init_resource::<KitScene>()
            .init_resource::<TextureCap>()
            .init_resource::<KitDiag>()
            .init_resource::<KitTextures>()
            .insert_resource(shared.clone())
            .add_systems(Startup, spawn_kit)
            .add_systems(Update, (load_textures, update_diag).chain());
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .insert_resource(shared)
                .add_systems(Render, count_draws.in_set(RenderSystems::PrepareBindGroups));
        }
    }
}

fn spawn_kit(
    mut commands: Commands,
    scene: Res<KitScene>,
    shared: Res<KitShared>,
    mut diag: ResMut<KitDiag>,
    mut tex: ResMut<KitTextures>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if *scene == KitScene::None {
        return;
    }
    let layout = stress_layout();
    let mut handles: Vec<[Handle<Mesh>; LODS]> = Vec::new();
    let mut ids = HashSet::new();
    for (id, bytes) in PIECES {
        let lods = glb::decode_lods(bytes, id).unwrap_or_else(|e| panic!("kit {id}: {e}"));
        diag.lod_triangles
            .push(std::array::from_fn(|l| lods[l].triangles()));
        let h: [Handle<Mesh>; LODS] =
            std::array::from_fn(|l| meshes.add(lod_render_mesh(&lods[l])));
        ids.extend(h.iter().map(|h| h.id().untyped()));
        handles.push(h);
    }
    shared.0.lock().expect("kit shared").meshes = ids;

    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.62, 0.40, 0.22),
        perceptual_roughness: 0.55,
        uv_transform: Affine2::from_scale(Vec2::splat(WOOD_UV_SCALE)),
        ..default()
    });
    tex.material = material.clone();

    let ranges = lod_ranges(crate::camera::FOV_DEG.to_radians());
    let (chunks, bound) = chunk_stats(&layout);
    let mut by_chunk: std::collections::BTreeMap<[i32; 3], Vec<&Placement>> = Default::default();
    for p in &layout {
        by_chunk
            .entry(chunk_of(p.cell).to_array())
            .or_default()
            .push(p);
    }
    for (chunk, placements) in by_chunk {
        commands
            .spawn((
                KitChunk(IVec3::from_array(chunk)),
                Transform::default(),
                Visibility::default(),
            ))
            .with_children(|c| {
                for p in placements {
                    let t = placement_transform(p);
                    for (lod, range) in ranges.iter().enumerate() {
                        c.spawn((
                            KitInstance {
                                piece: p.piece,
                                lod: lod as u8,
                            },
                            Mesh3d(handles[p.piece][lod].clone()),
                            MeshMaterial3d(material.clone()),
                            t,
                            range.clone(),
                        ));
                    }
                }
            });
    }
    diag.pieces = layout.len();
    diag.chunks = chunks;
    diag.piece_types = PIECES.len();
    diag.draw_bound = bound;
    diag.lod_bands_m = [
        ranges[0].end_margin.start,
        ranges[0].end_margin.end,
        ranges[1].end_margin.start,
        ranges[1].end_margin.end,
    ];
    info!(
        "kit: {} pieces in {chunks} chunks, draw bound {bound}",
        layout.len()
    );
}

/// Picks the texture family once the render device exists, then loads the wood maps into the kit
/// material.
fn load_textures(
    scene: Res<KitScene>,
    cap: Res<TextureCap>,
    device: Option<Res<RenderDevice>>,
    server: Res<AssetServer>,
    mut diag: ResMut<KitDiag>,
    mut tex: ResMut<KitTextures>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (KitScene::Stress2000, None, Some(device)) = (*scene, diag.family, device) else {
        return;
    };
    let f = device.features();
    let bc = f.contains(WgpuFeatures::TEXTURE_COMPRESSION_BC);
    let astc = f.contains(WgpuFeatures::TEXTURE_COMPRESSION_ASTC);
    let family = select_family_capped(bc, astc, cap.0);
    diag.family = Some(family);
    diag.device_bc = Some(bc);
    diag.device_astc = Some(astc);
    info!("kit: texture family {}", family.name());
    let mut load = |map: &str| {
        let path = format!("{ASSET_SOURCE}://{}", wood_path(map, family));
        let srgb = map == "base_color";
        let h: Handle<Image> = server
            .load_builder()
            .with_settings(move |s: &mut ImageLoaderSettings| s.is_srgb = srgb)
            .load(path.clone());
        tex.images.push((path, h.clone()));
        h
    };
    let base = load("base_color");
    let orm = load("orm");
    if let Some(mut m) = materials.get_mut(&tex.material) {
        m.base_color = Color::WHITE;
        m.base_color_texture = Some(base);
        m.perceptual_roughness = 1.0;
        m.metallic = 1.0;
        m.metallic_roughness_texture = Some(orm.clone());
        m.occlusion_texture = Some(orm);
    }
}

fn update_diag(
    shared: Res<KitShared>,
    server: Res<AssetServer>,
    images: Res<Assets<Image>>,
    tex: Res<KitTextures>,
    mut diag: ResMut<KitDiag>,
) {
    diag.draws = shared.0.lock().expect("kit shared").draws;
    diag.textures = tex
        .images
        .iter()
        .map(|(path, h)| TextureDiag {
            path: path.clone(),
            state: match server.load_state(h) {
                bevy::asset::LoadState::NotLoaded => "notLoaded",
                bevy::asset::LoadState::Loading => "loading",
                bevy::asset::LoadState::Loaded => "loaded",
                bevy::asset::LoadState::Failed(_) => "failed",
            },
            format: images
                .get(h)
                .map(|i| format!("{:?}", i.texture_descriptor.format)),
        })
        .collect();
}

#[derive(Default)]
struct PhaseCount {
    draws: u32,
    views: u32,
    instances: u32,
    dithered: u32,
    multidraw_sets: u32,
    unmeasured: bool,
}

impl PhaseCount {
    fn draws(&self) -> Option<u32> {
        (!self.unmeasured).then_some(self.draws)
    }
}

/// Render-world lookups that turn a visible entity into its bin: Bevy bins an opaque mesh instance
/// by (pipeline, material, mesh), and here the pipeline differs only by the dither key.
struct BinLookup<'a> {
    kit_meshes: &'a HashSet<UntypedAssetId>,
    ranges: Option<&'a RenderVisibilityRanges>,
    meshes: Option<&'a RenderMeshInstances>,
    materials: Option<&'a RenderMaterialInstances>,
}

impl BinLookup<'_> {
    fn crossfades(&self, e: MainEntity) -> bool {
        self.ranges
            .is_some_and(|r| r.entity_has_crossfading_visibility_ranges(e))
    }

    /// Bins of the kit instances in `visible`: `(draws, instances, dithered)`.
    fn visible_bins(&self, visible: &RenderVisibleEntities) -> (u32, u32, u32) {
        let (Some(meshes), Some(materials), Some(class)) =
            (self.meshes, self.materials, visible.get::<Mesh3d>())
        else {
            return (0, 0, 0);
        };
        let mut bins = HashSet::new();
        let (mut instances, mut dithered) = (0, 0);
        let entities = class
            .entities_cpu_culling
            .iter()
            .map(|(_, m)| *m)
            .chain(class.entities_gpu_culling.keys().copied());
        for e in entities {
            let Some(mesh) = meshes.mesh_asset_id(e).map(|m| m.untyped()) else {
                continue;
            };
            if !self.kit_meshes.contains(&mesh) {
                continue;
            }
            let material = materials.instances.get(&e).map(|m| m.asset_id);
            let fade = self.crossfades(e);
            bins.insert((material, mesh, fade));
            instances += 1;
            dithered += u32::from(fade);
        }
        (bins.len() as u32, instances, dithered)
    }

    /// One view's phase. Without GPU culling the bins are public: a non-empty batchable bin is one
    /// indirect draw and an unbatchable entity is one draw. With GPU culling (the WebGPU and Metal
    /// default) Bevy keeps its multidraw bins private, so they are rebuilt from the view's visible
    /// kit instances with the same (pipeline, material, mesh) key. Each bin is one indirect draw
    /// command of its batch set's multidraw.
    fn count<BPI: BinnedPhaseItem>(
        &self,
        c: &mut PhaseCount,
        phase: &BinnedRenderPhase<BPI>,
        visible: Option<&RenderVisibleEntities>,
        mesh_of: impl Fn(&BPI::BinKey) -> UntypedAssetId,
    ) {
        c.views += 1;
        if !phase.multidrawable_meshes.is_empty() {
            c.multidraw_sets += phase.multidrawable_meshes.len() as u32;
            if let Some(v) = visible {
                let (d, i, f) = self.visible_bins(v);
                c.draws += d;
                c.instances += i;
                c.dithered += f;
            } else {
                c.unmeasured = true;
            }
            return;
        }
        for ((_, key), bin) in &phase.batchable_meshes {
            if !self.kit_meshes.contains(&mesh_of(key)) || bin.entities().is_empty() {
                continue;
            }
            c.draws += 1;
            c.instances += bin.entities().len() as u32;
            c.dithered += bin
                .entities()
                .keys()
                .filter(|e| self.crossfades(**e))
                .count() as u32;
        }
        for ((_, key), bin) in &phase.unbatchable_meshes {
            if self.kit_meshes.contains(&mesh_of(key)) {
                c.draws += bin.entities.len() as u32;
                c.instances += bin.entities.len() as u32;
            }
        }
    }
}

/// Render world: kit draws per pass, summed over the views of each pass.
#[allow(clippy::too_many_arguments)]
fn count_draws(
    shared: Res<KitShared>,
    views: Query<(&ExtractedView, Option<&RenderVisibleEntities>)>,
    opaque: Option<Res<ViewBinnedRenderPhases<Opaque3d>>>,
    prepass: Option<Res<ViewBinnedRenderPhases<Opaque3dPrepass>>>,
    shadow: Option<Res<ViewBinnedRenderPhases<Shadow>>>,
    ranges: Option<Res<RenderVisibilityRanges>>,
    mesh_instances: Option<Res<RenderMeshInstances>>,
    material_instances: Option<Res<RenderMaterialInstances>>,
) {
    let mut s = shared.0.lock().expect("kit shared");
    if s.meshes.is_empty() {
        return;
    }
    let look = BinLookup {
        kit_meshes: &s.meshes,
        ranges: ranges.as_deref(),
        meshes: mesh_instances.as_deref(),
        materials: material_instances.as_deref(),
    };
    let mut main = PhaseCount::default();
    let mut pre = PhaseCount::default();
    let mut sh = PhaseCount::default();
    for (view, visible) in &views {
        let key = &view.retained_view_entity;
        if let Some(p) = opaque.as_ref().and_then(|p| p.get(key)) {
            look.count(&mut main, p, visible, |k| k.asset_id);
        }
        if let Some(p) = prepass.as_ref().and_then(|p| p.get(key)) {
            look.count(&mut pre, p, visible, |k| k.asset_id);
        }
        if let Some(p) = shadow.as_ref().and_then(|p| p.get(key)) {
            look.count(&mut sh, p, visible, |k| k.asset_id);
        }
    }
    s.draws = KitDraws {
        main: main.draws,
        main_views: main.views,
        main_instances: main.instances,
        dithered_instances: main.dithered,
        prepass: pre.draws(),
        shadow: sh.draws(),
        shadow_views: sh.views,
        multidraw_sets: main.multidraw_sets,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_piece_decodes_three_lods_within_the_block_budget() {
        // Kit contract §12: 1-cell block LOD0 ≤ 3,000, LOD1 ≤ 1,000, LOD2 ≤ 250 triangles.
        const BUDGET: [usize; LODS] = [3_000, 1_000, 250];
        for (id, bytes) in PIECES {
            let lods = glb::decode_lods(bytes, id).expect(id);
            for (l, m) in lods.iter().enumerate() {
                assert!(m.triangles() > 0, "{id} lod{l} empty");
                assert!(m.triangles() <= BUDGET[l], "{id} lod{l}: {}", m.triangles());
                let half = CELL_M / 2.0 + 1e-4;
                for p in &m.positions {
                    assert!(
                        p[0].abs() <= half && p[2].abs() <= half,
                        "{id} lod{l} {p:?}"
                    );
                    assert!((-1e-4..=CELL_M + 1e-4).contains(&p[1]), "{id} lod{l} {p:?}");
                }
            }
            assert!(lods[0].triangles() > lods[1].triangles());
            assert!(lods[1].triangles() > lods[2].triangles());
        }
    }

    #[test]
    fn stress_layout_is_2000_pieces_in_chunks() {
        let layout = stress_layout();
        assert_eq!(layout.len(), 2_000);
        let cells: HashSet<_> = layout.iter().map(|p| p.cell.to_array()).collect();
        assert_eq!(cells.len(), 2_000, "one piece per cell");
        for t in 0..PIECES.len() {
            assert!(layout.iter().any(|p| p.piece == t), "type {t} unused");
        }
        let (chunks, bound) = chunk_stats(&layout);
        // 50 × 10 × 4 cells from x 0, y 0, z −16: 7 × 2 × 1 chunks.
        assert_eq!(chunks, 14);
        assert_eq!(bound, 14 * PIECES.len() * LODS);
        assert_eq!(stress_layout(), layout, "deterministic");
    }

    #[test]
    fn chunks_floor_negative_cells() {
        assert_eq!(chunk_of(IVec3::new(-1, 0, -8)), IVec3::new(-1, 0, -1));
        assert_eq!(chunk_of(IVec3::new(7, 8, -9)), IVec3::new(0, 1, -2));
    }

    #[test]
    fn lod_ranges_are_contiguous_crossfades_at_the_contract_thresholds() {
        let fov = crate::camera::FOV_DEG.to_radians();
        let r = lod_ranges(fov);
        assert_eq!(r[0].start_margin, 0.0..0.0);
        for l in 0..LODS - 1 {
            assert_eq!(
                r[l].end_margin,
                r[l + 1].start_margin,
                "lod{l} → lod{}",
                l + 1
            );
            let m = &r[l].end_margin;
            assert!(m.start < m.end, "lod{l} fade margin is empty (hard cut)");
            assert!(r[l].start_margin.end <= r[l].end_margin.start);
            // At the margin ends one cell projects to threshold × (1 ± 15 %) pixels at 1440p.
            let px = |d: f32| CELL_M * LOD_REF_HEIGHT_PX / (2.0 * (fov * 0.5).tan() * d);
            assert!((px(m.start) - LOD_PX[l] * 1.15).abs() < 1e-2);
            assert!((px(m.end) - LOD_PX[l] * 0.85).abs() < 1e-2);
        }
        assert!(r[LODS - 1].end_margin.start >= 100.0);
    }

    #[test]
    fn lod_distance_shrinks_with_pixels() {
        let fov = 35f32.to_radians();
        assert!(lod_distance(160.0, fov) < lod_distance(48.0, fov));
        assert!((lod_distance(160.0, fov) - 0.8565).abs() < 1e-3);
    }
}
