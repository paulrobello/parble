//! Instanced, chunked kit-piece renderer (P1-T4).
//!
//! - Instancing: one [`Mesh`] per (piece type, LOD) and one [`KitWoodMaterial`] per (piece type,
//!   LOD, wood variant), so Bevy's GPU preprocessing bins every instance of a (type, LOD, variant)
//!   into one indirect draw per view, below the P1-T4 bound of one per (type × LOD × chunk).
//!   Instances are grouped under one [`KitChunk`] entity per 8×8×8-cell chunk (kit contract §2) of
//!   their world-space origin.
//! - LOD: each piece spawns all three LODs with contiguous [`VisibilityRange`]s from the §12
//!   thresholds (one cell 160 px / 48 px tall at 1440p) and a ±15 % margin. Bevy draws both LODs in
//!   the margin with complementary screen-space dither (`VISIBILITY_RANGE_DITHER`, main pass and
//!   prepass/shadows), so there is no hard cut. The fade is measured from each piece's origin, which
//!   is finer than §12's per-chunk nearest point.
//! - Wood (§12): each piece picks one of [`WOOD_SETS`] by a hash of its layout key
//!   ([`variant_of`]). `wood.wesl` projects the tileable wood in world space, triplanar, at one
//!   1,024² repeat per [`WOOD_TILE_M`] (4,096 texels per metre; Low samples mip 1, 2,048), and the
//!   StandardMaterial half applies the piece's baked AO and bevel normal maps at UV0.
//! - Textures: the wood sets load as BC7 KTX2, else ASTC KTX2, else PNG ([`family`]), chosen once
//!   the render device exists (on the web it arrives after `Startup`). Baked maps are PNG only.
//! - The draw counts in [`KitDiag`] are read from the render world's binned phases ([`count_draws`]).
//! - Mechanism parts (P2-T6, [`parts`]): each animated part is its own mesh per (type, LOD), drawn
//!   with the piece's material and posed from the sim each frame, so a mechanism type adds one bin
//!   per part per (LOD, variant) next to its body's.

pub mod edit;
pub mod family;
pub mod glb;
pub mod parts;

use std::{
    borrow::Cow,
    collections::{BTreeSet, HashMap, HashSet},
    sync::{Arc, Mutex},
};

use bevy::{
    asset::{UntypedAssetId, embedded_asset, io::AssetSourceBuilder},
    camera::visibility::VisibilityRange,
    core_pipeline::{core_3d::Opaque3d, prepass::Opaque3dPrepass},
    image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor},
    pbr::{
        ExtendedMaterial, MaterialExtension, RenderMaterialInstances, RenderMeshInstances, Shadow,
    },
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        render_phase::{BinnedPhaseItem, BinnedRenderPhase, ViewBinnedRenderPhases},
        render_resource::{AsBindGroup, ShaderType},
        renderer::RenderDevice,
        settings::WgpuFeatures,
        sync_world::MainEntity,
        view::{ExtractedView, RenderVisibilityRanges, RenderVisibleEntities},
    },
    shader::ShaderRef,
};
use serde::Serialize;

use family::{TextureFamily, select_family_capped};
use glb::{LODS, LodMesh, SplitLod};
use parts::{KitPart, PartFrame, PartKit};

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

macro_rules! glbs {
    ($($id:ident),* $(,)?) => {
        [$((
            stringify!($id),
            include_bytes!(concat!(
                "../../../../assets/kit/",
                stringify!($id),
                "/",
                stringify!($id),
                ".glb"
            )) as &[u8],
        )),*]
    };
}

/// Every builtin kit piece's GLB, embedded (about 1.9 MB together), in the catalogue's id order
/// ([`parble_kit::builtin_piece_ids`]), so the renderer and the editor palette cover exactly the
/// pieces the sim loads.
pub const PIECES: &[(&str, &[u8])] = &parble_kit::builtin_piece_ids!(glbs);

/// The four 1-cell pieces of the P1-T4 showcase grid ([`stress_layout`]) and the editor's `fill`
/// layout, in their original order: both pick a piece by `hash % 4`, so the 2,000-piece layouts,
/// their chunk and draw bounds stay those of P1-T4 as the catalogue grows. Multi-cell pieces
/// cannot sit on the grid's one-cell pitch.
pub const SHOWCASE: [&str; 4] = [
    "blk_straight_l3",
    "blk_curve90_l3",
    "blk_slope_gentle_l3",
    "blk_end_l3",
];

/// Index into [`PIECES`] of piece `id`.
pub fn piece_index(id: &str) -> Option<usize> {
    PIECES.iter().position(|(p, _)| *p == id)
}

/// [`PIECES`] index of [`SHOWCASE`] entry `i`.
pub fn showcase_piece(i: usize) -> usize {
    piece_index(SHOWCASE[i]).expect("showcase pieces are builtin")
}

/// Asset source for the served `assets/` tree (web: `dist/assets`, copied by Trunk).
pub const ASSET_SOURCE: &str = "parble";
/// Wood variants (P1-T1), picked per piece by [`variant_of`], and the maps the kit material loads
/// from each. The sets' own normal maps are not loaded: the bevel normal is the piece's baked map.
pub const WOOD_SETS: [&str; 3] = ["wood_warm_a", "wood_warm_b", "wood_warm_c"];
pub const WOOD_MAPS: [&str; 2] = ["base_color", "orm"];
/// One wood repeat in metres (`assets/materials/provenance.json` `contract.tileM`).
pub const WOOD_TILE_M: f32 = 0.25;
/// Per-piece, per-LOD baked maps (P1-T2), PNG only, sampled at UV0.
pub const BAKED_MAPS: [&str; 2] = ["ao", "normal"];

const SHADER: &str = "embedded://parble_render/kit/wood.wesl";

/// Path of a wood map in `family`, relative to the `assets/` root.
pub fn wood_path(set: &str, map: &str, family: TextureFamily) -> String {
    format!("materials/{set}/{set}.{map}.{}", family.suffix())
}

/// Path of a piece's baked map for `lod`, relative to the `assets/` root.
pub fn baked_path(piece: &str, lod: usize, map: &str) -> String {
    format!("kit/{piece}/{piece}_lod{lod}_{map}.png")
}

/// Every file the kit renderer can fetch from [`ASSET_SOURCE`], over all texture families, sorted.
/// `index.html` ships exactly these under `assets/`.
pub fn fetchable_paths() -> Vec<String> {
    let mut out = Vec::new();
    for f in [TextureFamily::Bc7, TextureFamily::Astc, TextureFamily::Png] {
        for set in WOOD_SETS {
            for map in WOOD_MAPS {
                out.push(wood_path(set, map, f));
            }
        }
    }
    for (piece, _) in PIECES {
        for lod in 0..LODS {
            for map in BAKED_MAPS {
                out.push(baked_path(piece, lod, map));
            }
        }
    }
    out.sort();
    out
}

/// Wood uniform for `wood.wesl` (binding 100).
#[derive(Clone, Copy, Debug, Default, ShaderType, Reflect)]
pub struct KitWoodParams {
    /// 1 / [`WOOD_TILE_M`].
    pub inv_tile_m: f32,
    /// 1 once the wood maps are bound.
    pub enabled: f32,
    /// Wood mip bias: 1 on Low (2,048 texels per metre), else 0.
    pub mip_bias: f32,
    pub pad: f32,
}

/// The world-space wood of one variant, layered over StandardMaterial's baked AO and normal.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct KitWoodExt {
    #[uniform(100)]
    pub params: KitWoodParams,
    #[texture(101)]
    #[sampler(102)]
    pub base: Option<Handle<Image>>,
    #[texture(103)]
    #[sampler(104)]
    pub orm: Option<Handle<Image>>,
}

impl MaterialExtension for KitWoodExt {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}

pub type KitWoodMaterial = ExtendedMaterial<StandardMaterial, KitWoodExt>;

/// Wood variant of the piece keyed by `cell` ([`Placement::cell`]): hash bits disjoint from the
/// stress layout's piece type (`h % 4`) and yaw (`h >> 8`), so variants do not correlate with either.
pub fn variant_of(cell: IVec3) -> usize {
    ((cell_hash(cell) >> 16) % WOOD_SETS.len() as u32) as usize
}

/// The URL the web build fetches for site path `path` (`assets/...`): `path?v=<hash>` with the
/// content hash `scripts/web/version-dist.mjs` gave the file, or `path` unchanged when it has none.
pub fn versioned_url<'a>(path: &'a str, versions: &HashMap<String, String>) -> Cow<'a, str> {
    match versions.get(path) {
        Some(v) => Cow::Owned(format!("{path}?v={v}")),
        None => Cow::Borrowed(path),
    }
}

/// Registers the [`ASSET_SOURCE`] source. Add it before `DefaultPlugins` (Bevy reads asset sources
/// when `AssetPlugin` builds).
#[derive(Default)]
pub struct KitAssetSourcePlugin {
    /// Site path → content hash (`window.__PARBLE_HASHES` on the web, empty natively). The web reader
    /// appends `?v=<hash>` to every fetch, like the rest of the versioned site.
    pub versions: HashMap<String, String>,
}

impl Plugin for KitAssetSourcePlugin {
    fn build(&self, app: &mut App) {
        #[cfg(not(target_arch = "wasm32"))]
        app.register_asset_source(
            ASSET_SOURCE,
            AssetSourceBuilder::platform_default(
                concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"),
                None,
            ),
        );
        #[cfg(target_arch = "wasm32")]
        {
            use bevy::asset::io::wasm::HttpWasmAssetReader;
            let versions = Arc::new(self.versions.clone());
            if versions.is_empty() {
                warn!("kit: no window.__PARBLE_HASHES, asset fetches are unversioned");
            }
            app.register_asset_source(
                ASSET_SOURCE,
                AssetSourceBuilder::new(move || {
                    let v = versions.clone();
                    Box::new(
                        HttpWasmAssetReader::new("assets")
                            .with_request_mapper(move |p| versioned_url(p, &v)),
                    )
                }),
            );
        }
    }
}

/// Which kit layout to spawn (set by the app from `--scenario`).
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum KitScene {
    #[default]
    None,
    /// 2,000 pieces in one flat, raised 50 × 40 showcase grid with half-cell gaps, beside the tray.
    Stress2000,
    /// The playroom marble run ([`parble_sim::playroom_run::RUN`]), the same placements the sim
    /// runs as chunk colliders.
    Playroom,
    /// The editor (P3-T4): no fixed layout. The app places pieces chunk by chunk through
    /// [`edit::KitEdits`].
    Editor,
    /// The P2-T6 mechanism showcase ([`parble_sim::mech_showcase`]): a toggle and a one-way gate
    /// lane, the same placements the sim runs with their behaviours.
    Mechanisms,
}

impl KitScene {
    /// The placements this scene draws.
    pub fn layout(self) -> Vec<Placement> {
        match self {
            Self::None | Self::Editor => Vec::new(),
            Self::Stress2000 => stress_layout(),
            Self::Playroom => playroom_layout(),
            Self::Mechanisms => run_layout(&parble_sim::mech_showcase::LAYOUT),
        }
    }
}

/// Agent cap on the texture family (`--tex`); see [`family::select_family_capped`].
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextureCap(pub Option<TextureFamily>);

/// One placed piece: index into [`PIECES`], hash key, world origin and yaw in quarter turns.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub piece: usize,
    /// The anchor cell of a cell-grid layout, or the (column, 0, row) index of a showcase grid
    /// piece. Keys the type, yaw and wood variant hashes.
    pub cell: IVec3,
    /// World position of the piece-local origin (bottom-face centre, kit contract §1).
    pub origin: Vec3,
    pub yaw: u8,
}

/// Showcase grid: columns along +x, rows along −z, one flat layer.
pub const GRID_COLS: i32 = 50;
pub const GRID_ROWS: i32 = 40;
/// Neighbour spacing: one 1-cell piece plus a half-cell gap.
pub const GRID_PITCH_M: f32 = 1.5 * CELL_M;
/// Height of every grid piece's bottom face above the floor (y = 0).
pub const GRID_RAISE_M: f32 = 2.0 * CELL_M;
/// Origin of grid piece (0, 0). The near row stands 0.13 m behind the tray (−z) and the columns
/// straddle its length. No origin lands on a chunk boundary (0.48 m multiples), so chunking is
/// not at the mercy of float rounding.
pub const GRID_ORIGIN_M: Vec3 = Vec3::new(-1.88, GRID_RAISE_M, -0.16);

fn cell_hash(c: IVec3) -> u32 {
    let mut h = (c.x as u32).wrapping_mul(0x8da6_b343)
        ^ (c.y as u32).wrapping_mul(0xd816_3841)
        ^ (c.z as u32).wrapping_mul(0xcb1a_b31f);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^ (h >> 12)
}

/// World origin of showcase grid piece (`col`, `row`).
pub fn grid_origin(col: i32, row: i32) -> Vec3 {
    GRID_ORIGIN_M + Vec3::new(col as f32 * GRID_PITCH_M, 0.0, -(row as f32) * GRID_PITCH_M)
}

/// The 2,000-piece showcase: a [`GRID_COLS`] × [`GRID_ROWS`] grid at [`GRID_PITCH_M`], raised
/// [`GRID_RAISE_M`], piece type and yaw from a hash of the grid index.
pub fn stress_layout() -> Vec<Placement> {
    let mut out = Vec::with_capacity((GRID_COLS * GRID_ROWS) as usize);
    for row in 0..GRID_ROWS {
        for col in 0..GRID_COLS {
            let cell = IVec3::new(col, 0, row);
            let h = cell_hash(cell);
            out.push(Placement {
                piece: showcase_piece((h % SHOWCASE.len() as u32) as usize),
                cell,
                origin: grid_origin(col, row),
                yaw: ((h >> 8) % 4) as u8,
            });
        }
    }
    out
}

/// The showcase grid's floor-plan footprint, min and max corners in (x, z) metres.
pub fn grid_footprint_m() -> (Vec2, Vec2) {
    let half = CELL_M / 2.0;
    let a = grid_origin(0, 0);
    let b = grid_origin(GRID_COLS - 1, GRID_ROWS - 1);
    (
        Vec2::new(a.x.min(b.x) - half, a.z.min(b.z) - half),
        Vec2::new(a.x.max(b.x) + half, a.z.max(b.z) + half),
    )
}

/// The playroom run's placements, from the sim's own list so the drawn and simulated runs agree.
pub fn playroom_layout() -> Vec<Placement> {
    run_layout(&parble_sim::playroom_run::RUN)
}

/// Cell-grid placements (id, anchor cell, yaw) as render placements.
pub fn run_layout(run: &[(&str, [i32; 3], u8)]) -> Vec<Placement> {
    run.iter()
        .map(|&(id, cell, yaw)| Placement {
            piece: PIECES
                .iter()
                .position(|(p, _)| *p == id)
                .unwrap_or_else(|| panic!("run piece {id} has no GLB")),
            cell: IVec3::from_array(cell),
            origin: cell_origin(IVec3::from_array(cell)),
            yaw,
        })
        .collect()
}

/// Cell edge in integer µm (kit contract §2).
const CELL_UM: i32 = 60_000;

/// Chunk of a world-space piece origin. Rounded to integer µm first, so a cell-grid origin on a
/// chunk boundary lands in the chunk it starts, as `cell.div_euclid(CHUNK_CELLS)` would.
pub fn chunk_of(origin: Vec3) -> IVec3 {
    (origin * 1.0e6)
        .round()
        .as_ivec3()
        .div_euclid(IVec3::splat(CHUNK_CELLS * CELL_UM))
}

/// World position of a cell's piece-local origin (bottom-face centre, kit contract §1).
pub fn cell_origin(cell: IVec3) -> Vec3 {
    cell.as_vec3() * CELL_M
}

pub fn placement_transform(p: &Placement) -> Transform {
    Transform::from_translation(p.origin).with_rotation(Quat::from_rotation_y(
        f32::from(p.yaw) * std::f32::consts::FRAC_PI_2,
    ))
}

/// Animated parts of [`PIECES`] entry `piece` (its sidecar's `animated_parts`).
pub fn piece_parts(piece: usize) -> usize {
    parble_kit::builtin()
        .iter()
        .find(|p| p.def.id.as_str() == PIECES[piece].0)
        .map_or(0, |p| p.animated_parts.len())
}

/// Draws one occupied (type, chunk) may cost: `LODS` for the body and `LODS` per animated part
/// ([`parts`]), so the P1-T4 bound of one draw per (type × LOD × chunk) holds per drawn mesh.
pub fn pair_draws(piece: usize) -> usize {
    LODS * (1 + piece_parts(piece))
}

/// Occupied chunks and the draw bound: [`pair_draws`] per occupied (type, chunk). A layout with no
/// mechanism parts keeps the P1-T4 bound of `LODS` per pair.
pub fn chunk_stats(layout: &[Placement]) -> (usize, usize) {
    let chunks: BTreeSet<_> = layout
        .iter()
        .map(|p| chunk_of(p.origin).to_array())
        .collect();
    let pairs: BTreeSet<_> = layout
        .iter()
        .map(|p| (p.piece, chunk_of(p.origin).to_array()))
        .collect();
    let bound = pairs.iter().map(|&(piece, _)| pair_draws(piece)).sum();
    (chunks.len(), bound)
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
    /// `wood` (a [`WOOD_SETS`] map in the device's family) or `baked` (a piece's PNG map).
    pub kind: &'static str,
    pub state: &'static str,
    /// `wgpu::TextureFormat` of the loaded image.
    pub format: Option<String>,
    /// The asset server's load error when `state` is `failed`.
    pub error: Option<String>,
}

/// Kit renderer state for the agent diag (`kit` in `--dump-state` and `window.__parble`).
#[derive(Resource, Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KitDiag {
    pub pieces: usize,
    pub chunks: usize,
    pub piece_types: usize,
    /// Wood variants in use ([`WOOD_SETS`]); each (type, LOD, variant) is one material.
    pub variants: usize,
    /// Pieces per wood variant in the layout.
    pub variant_pieces: Vec<usize>,
    /// One draw per occupied (type × chunk) × LOD (the P1-T4 bound), and one more per animated
    /// part of the type (P2-T6, [`pair_draws`]).
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
    /// Editor-placed pieces ([`KitScene::Editor`] only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit: Option<edit::KitEditStats>,
    /// Every drawn mechanism part's pose this frame (P2-T6), by anchor cell and part.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<parts::PartDiag>,
}

/// Per-piece LOD meshes and their visibility ranges, kept for pieces spawned after startup.
#[derive(Resource, Default)]
struct KitMeshes {
    /// Each piece's static body per LOD (without its animated parts).
    lods: Vec<[Handle<Mesh>; LODS]>,
    /// [`Self::lods`] with reversed winding, for mirrored pieces ([`LodMesh::reversed_winding`]).
    mirrored: Vec<[Handle<Mesh>; LODS]>,
    /// LOD0 with the parts merged at rest, plain and mirrored, for the editor ghost.
    ghost: Vec<[Handle<Mesh>; 2]>,
    /// Each piece's animated parts.
    parts: Vec<Vec<PartKit>>,
    ranges: Option<[VisibilityRange; LODS]>,
}

/// `piece`'s animated parts from its split LODs, in sidecar order, with `add` turning each part
/// mesh into a handle (plain, then mirrored, per LOD).
fn part_kits(
    piece: &parble_kit::KitPiece,
    split: &[SplitLod; LODS],
    mut add: impl FnMut(&LodMesh) -> Handle<Mesh>,
) -> Vec<PartKit> {
    piece
        .animated_parts
        .iter()
        .enumerate()
        .map(|(index, part)| {
            let mesh = |lod: usize| {
                split[lod]
                    .parts
                    .iter()
                    .find(|p| p.name == part.name)
                    .unwrap_or_else(|| panic!("{} has no {}_lod{lod}", piece.def.id, part.name))
            };
            let mut lods: [Option<Handle<Mesh>>; LODS] = Default::default();
            let mut mirrored: [Option<Handle<Mesh>>; LODS] = Default::default();
            for lod in 0..LODS {
                lods[lod] = Some(add(&mesh(lod).mesh));
                mirrored[lod] = Some(add(&mesh(lod).mesh.reversed_winding()));
            }
            PartKit {
                index,
                pivot: Vec3::from_array(part.pivot_um.map(|v| v as f32 * 1.0e-6)),
                axis: Vec3::from_array(part.axis.map(|v| v as f32)).normalize(),
                rest_mdeg: part.rest_mdeg,
                rest: std::array::from_fn(|lod| mesh(lod).rest),
                lods: lods.map(|h| h.expect("set per LOD")),
                mirrored: mirrored.map(|h| h.expect("set per LOD")),
            }
        })
        .collect()
}

/// Spawns the animated parts of one placed piece under a parent at the placement transform `t`
/// (see [`parts`] for why a parent), with `root` added to that parent. No-op for a piece without
/// parts.
#[allow(clippy::too_many_arguments)]
fn spawn_parts(
    c: &mut ChildSpawnerCommands<'_>,
    kept: &KitMeshes,
    materials: &[[Handle<KitWoodMaterial>; 3]; LODS],
    piece: usize,
    cell: IVec3,
    variant: usize,
    t: Transform,
    mirror: bool,
    root: impl Bundle,
) {
    let (Some(kits), Some(ranges)) = (kept.parts.get(piece), kept.ranges.as_ref()) else {
        return;
    };
    if kits.is_empty() {
        return;
    }
    c.spawn((t, Visibility::Inherited, root))
        .with_children(|c| {
            for (slot, k) in kits.iter().enumerate() {
                let meshes = if mirror { &k.mirrored } else { &k.lods };
                for (lod, range) in ranges.iter().enumerate() {
                    c.spawn((
                        KitPart {
                            cell,
                            piece,
                            slot,
                            lod: lod as u8,
                        },
                        Mesh3d(meshes[lod].clone()),
                        MeshMaterial3d(materials[lod][variant].clone()),
                        Transform::from_matrix(k.rest[lod]),
                        range.clone(),
                    ));
                }
            }
        });
}

#[derive(Resource, Default)]
struct KitTextures {
    /// `materials[piece][lod][variant]`.
    materials: Vec<[[Handle<KitWoodMaterial>; 3]; LODS]>,
    images: Vec<(String, &'static str, Handle<Image>)>,
    /// Each [`WOOD_SETS`] variant's [`WOOD_MAPS`], once the family is known.
    wood: Vec<[Handle<Image>; 2]>,
    /// Pieces whose baked maps are wanted: every piece of the layout, and every piece the editor
    /// has placed. Baked maps load per piece on first use, so a scene fetches only its own.
    wanted: BTreeSet<usize>,
    /// Pieces whose baked maps are loading or loaded.
    baked: BTreeSet<usize>,
}

pub struct KitPlugin;

impl Plugin for KitPlugin {
    fn build(&self, app: &mut App) {
        let shared = KitShared::default();
        embedded_asset!(app, "wood.wesl");
        app.add_plugins(MaterialPlugin::<KitWoodMaterial>::default())
            .init_resource::<KitScene>()
            .init_resource::<TextureCap>()
            .init_resource::<KitDiag>()
            .init_resource::<KitTextures>()
            .init_resource::<KitMeshes>()
            .init_resource::<PartFrame>()
            .insert_resource(shared.clone())
            .add_plugins(edit::KitEditPlugin)
            .add_systems(Startup, spawn_kit)
            .add_systems(Update, (load_textures, update_diag).chain())
            .add_systems(
                PostUpdate,
                parts::animate_parts.before(bevy::transform::TransformSystems::Propagate),
            );
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .insert_resource(shared)
                .add_systems(Render, count_draws.in_set(RenderSystems::PrepareBindGroups));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_kit(
    mut commands: Commands,
    scene: Res<KitScene>,
    shared: Res<KitShared>,
    mut diag: ResMut<KitDiag>,
    mut tex: ResMut<KitTextures>,
    mut kept: ResMut<KitMeshes>,
    quality: Option<Res<crate::Quality>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<KitWoodMaterial>>,
) {
    if *scene == KitScene::None {
        return;
    }
    let low = quality.is_some_and(|q| *q == crate::Quality::Low);
    let layout = scene.layout();
    let mut handles: Vec<[Handle<Mesh>; LODS]> = Vec::new();
    let mut mirrored: Vec<[Handle<Mesh>; LODS]> = Vec::new();
    let mut ids = HashSet::new();
    let catalogue = parble_kit::builtin();
    for (id, bytes) in PIECES {
        let split = glb::decode_split(bytes, id).unwrap_or_else(|e| panic!("kit {id}: {e}"));
        let piece = catalogue
            .iter()
            .find(|p| p.def.id.as_str() == *id)
            .unwrap_or_else(|| panic!("kit {id} is not in the catalogue"));
        // A piece draws its body and every part.
        diag.lod_triangles.push(std::array::from_fn(|l| {
            let parts: usize = split[l].parts.iter().map(|p| p.mesh.triangles()).sum();
            split[l].body.triangles() + parts
        }));
        let h: [Handle<Mesh>; LODS] =
            std::array::from_fn(|l| meshes.add(lod_render_mesh(&split[l].body)));
        let m: [Handle<Mesh>; LODS] =
            std::array::from_fn(|l| meshes.add(lod_render_mesh(&split[l].body.reversed_winding())));
        ids.extend(h.iter().chain(&m).map(|h| h.id().untyped()));
        let kits = part_kits(piece, &split, |mesh| {
            let h = meshes.add(lod_render_mesh(mesh));
            ids.insert(h.id().untyped());
            h
        });
        kept.ghost.push(if kits.is_empty() {
            [h[0].clone(), m[0].clone()]
        } else {
            let mut rest = split[0].body.clone();
            for p in &split[0].parts {
                rest.append(&p.mesh.transformed(&p.rest));
            }
            [
                meshes.add(lod_render_mesh(&rest)),
                meshes.add(lod_render_mesh(&rest.reversed_winding())),
            ]
        });
        kept.parts.push(kits);
        handles.push(h);
        mirrored.push(m);
    }
    shared.0.lock().expect("kit shared").meshes = ids;

    // Until the maps load: the GLB placeholder wood colour, flat.
    tex.materials = (0..PIECES.len())
        .map(|_| {
            std::array::from_fn(|_| {
                std::array::from_fn(|_| {
                    materials.add(KitWoodMaterial {
                        base: StandardMaterial {
                            base_color: Color::srgb(0.62, 0.40, 0.22),
                            perceptual_roughness: 0.55,
                            ..default()
                        },
                        extension: KitWoodExt {
                            params: KitWoodParams {
                                inv_tile_m: 1.0 / WOOD_TILE_M,
                                enabled: 0.0,
                                mip_bias: if low { 1.0 } else { 0.0 },
                                pad: 0.0,
                            },
                            ..default()
                        },
                    })
                })
            })
        })
        .collect();
    tex.wanted.extend(layout.iter().map(|p| p.piece));
    let mut variant_pieces = vec![0; WOOD_SETS.len()];

    let ranges = lod_ranges(crate::camera::FOV_DEG.to_radians());
    kept.lods = handles.clone();
    kept.mirrored = mirrored;
    kept.ranges = Some(ranges.clone());
    let (chunks, bound) = chunk_stats(&layout);
    let mut by_chunk: std::collections::BTreeMap<[i32; 3], Vec<&Placement>> = Default::default();
    for p in &layout {
        by_chunk
            .entry(chunk_of(p.origin).to_array())
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
                    let v = variant_of(p.cell);
                    variant_pieces[v] += 1;
                    for (lod, range) in ranges.iter().enumerate() {
                        c.spawn((
                            KitInstance {
                                piece: p.piece,
                                lod: lod as u8,
                            },
                            Mesh3d(handles[p.piece][lod].clone()),
                            MeshMaterial3d(tex.materials[p.piece][lod][v].clone()),
                            t,
                            range.clone(),
                        ));
                    }
                    let mats = &tex.materials[p.piece];
                    spawn_parts(c, &kept, mats, p.piece, p.cell, v, t, false, ());
                }
            });
    }
    diag.pieces = layout.len();
    diag.chunks = chunks;
    diag.piece_types = layout
        .iter()
        .map(|p| p.piece)
        .collect::<BTreeSet<_>>()
        .len();
    diag.variants = WOOD_SETS.len();
    diag.variant_pieces = variant_pieces;
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

/// Picks the texture family once the render device exists and loads every wood variant's maps,
/// then loads each wanted piece's baked maps into its kit materials the first frame it is wanted.
fn load_textures(
    scene: Res<KitScene>,
    cap: Res<TextureCap>,
    device: Option<Res<RenderDevice>>,
    server: Res<AssetServer>,
    mut diag: ResMut<KitDiag>,
    mut tex: ResMut<KitTextures>,
    mut materials: ResMut<Assets<KitWoodMaterial>>,
) {
    if *scene == KitScene::None {
        return;
    }
    let family = match (diag.family, device) {
        (Some(f), _) => f,
        (None, Some(device)) => {
            let f = device.features();
            let bc = f.contains(WgpuFeatures::TEXTURE_COMPRESSION_BC);
            let astc = f.contains(WgpuFeatures::TEXTURE_COMPRESSION_ASTC);
            let family = select_family_capped(bc, astc, cap.0);
            diag.family = Some(family);
            diag.device_bc = Some(bc);
            diag.device_astc = Some(astc);
            info!("kit: texture family {}", family.name());
            family
        }
        (None, None) => return,
    };
    let tex = &mut *tex;
    let mut load = |rel: String, kind: &'static str, srgb: bool, repeat: bool| {
        let path = format!("{ASSET_SOURCE}://{rel}");
        let h: Handle<Image> = server
            .load_builder()
            .with_settings(move |s: &mut ImageLoaderSettings| {
                s.is_srgb = srgb;
                if repeat {
                    // World-space tiling wraps; the default sampler clamps.
                    let mut d = ImageSamplerDescriptor {
                        address_mode_u: ImageAddressMode::Repeat,
                        address_mode_v: ImageAddressMode::Repeat,
                        ..ImageSamplerDescriptor::linear()
                    };
                    d.set_anisotropic_filter(8);
                    s.sampler = ImageSampler::Descriptor(d);
                }
            })
            .load(path.clone());
        tex.images.push((path, kind, h.clone()));
        h
    };
    if tex.wood.is_empty() {
        tex.wood = WOOD_SETS
            .iter()
            .map(|set| {
                WOOD_MAPS.map(|map| {
                    load(
                        wood_path(set, map, family),
                        "wood",
                        map == "base_color",
                        true,
                    )
                })
            })
            .collect();
    }
    let todo: Vec<usize> = tex.wanted.difference(&tex.baked).copied().collect();
    for p in todo {
        let piece = PIECES[p].0;
        for lod in 0..LODS {
            let [ao, normal] =
                BAKED_MAPS.map(|map| load(baked_path(piece, lod, map), "baked", false, false));
            for (v, [base, orm]) in tex.wood.iter().enumerate() {
                let Some(mut m) = materials.get_mut(&tex.materials[p][lod][v]) else {
                    continue;
                };
                m.base.base_color = Color::WHITE;
                m.base.occlusion_texture = Some(ao.clone());
                m.base.normal_map_texture = Some(normal.clone());
                m.extension.base = Some(base.clone());
                m.extension.orm = Some(orm.clone());
                m.extension.params.enabled = 1.0;
            }
        }
        tex.baked.insert(p);
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
        .map(|(path, kind, h)| {
            let load = server.load_state(h);
            TextureDiag {
                path: path.clone(),
                kind,
                state: match load {
                    bevy::asset::LoadState::NotLoaded => "notLoaded",
                    bevy::asset::LoadState::Loading => "loading",
                    bevy::asset::LoadState::Loaded => "loaded",
                    bevy::asset::LoadState::Failed(_) => "failed",
                },
                error: match load {
                    bevy::asset::LoadState::Failed(e) => Some(e.to_string()),
                    _ => None,
                },
                format: images
                    .get(h)
                    .map(|i| format!("{:?}", i.texture_descriptor.format)),
            }
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
    fn mirrored_lods_wind_outward_under_the_mirror_transform() {
        // Share of triangles whose winding normal agrees with their vertex normals, with every
        // point and normal mapped by `f` (the mirror is x → −x, determinant −1).
        fn outward(m: &LodMesh, f: impl Fn([f32; 3]) -> Vec3) -> f32 {
            let agree = m
                .indices
                .as_chunks::<3>()
                .0
                .iter()
                .filter(|t| {
                    let p = t.map(|i| f(m.positions[i as usize]));
                    let n: Vec3 = t.iter().map(|&i| f(m.normals[i as usize])).sum();
                    (p[1] - p[0]).cross(p[2] - p[0]).dot(n) > 0.0
                })
                .count();
            agree as f32 / m.triangles() as f32
        }
        let id = |p: [f32; 3]| Vec3::from_array(p);
        let mirror = |p: [f32; 3]| Vec3::new(-p[0], p[1], p[2]);
        for (name, bytes) in PIECES {
            for (l, m) in glb::decode_lods(bytes, name).unwrap().iter().enumerate() {
                let plain = outward(m, id);
                assert!(plain > 0.9, "{name} lod{l}: {plain}");
                assert_eq!(
                    outward(&m.reversed_winding(), mirror),
                    plain,
                    "{name} lod{l}"
                );
                assert!(
                    outward(m, mirror) < 0.1,
                    "{name} lod{l}: the unreversed mirror is inside-out"
                );
            }
        }
    }

    #[test]
    fn every_piece_decodes_three_lods_within_its_footprint_and_budget() {
        // Kit contract §12 budgets: a block 3,000 / 1,000 / 250 triangles per cell, a rail
        // 1,500 / 500 / 120 per cell of centreline length, a mechanism the block budget plus
        // 1,500 / 500 / 120 for its animated parts. kit-check holds the exact rail lengths; here a
        // rail is bounded by its footprint cell count, which is never shorter.
        const BLOCK: [usize; LODS] = [3_000, 1_000, 250];
        const RAIL: [usize; LODS] = [1_500, 500, 120];
        let kit = parble_kit::builtin();
        assert_eq!(PIECES.len(), kit.len());
        for (id, bytes) in PIECES {
            let piece = kit
                .iter()
                .find(|p| p.def.id.as_str() == *id)
                .unwrap_or_else(|| panic!("{id} is not in the catalogue"));
            let cells = &piece.def.footprint;
            let lo = cells
                .iter()
                .fold(IVec3::MAX, |a, c| a.min(IVec3::new(c.x, c.y, c.z)));
            let hi = cells
                .iter()
                .fold(IVec3::MIN, |a, c| a.max(IVec3::new(c.x, c.y, c.z)));
            // Cell (x, y, z) spans x ± half a cell, y .. y + 1 cell, z ± half a cell (§1, §2).
            let mut min = lo.as_vec3() * CELL_M - Vec3::new(CELL_M / 2.0, 0.0, CELL_M / 2.0);
            let mut max = hi.as_vec3() * CELL_M + Vec3::new(CELL_M / 2.0, CELL_M, CELL_M / 2.0);
            // A spigot may leave the footprint into a socket (§5.5).
            if let Some(s) = piece.spigot {
                let a = Vec3::from_array(s.axis.map(|v| v as f32));
                let c = Vec3::from_array(s.center_um.map(|v| v as f32 * 1e-6));
                let end = c + a * (s.length_um as f32 * 1e-6);
                let r = Vec3::splat(s.radius_um as f32 * 1e-6);
                min = min.min(c.min(end) - r);
                max = max.max(c.max(end) + r);
            }
            let n = cells.len();
            let budget: [usize; LODS] = std::array::from_fn(|l| {
                if id.starts_with("rail_") {
                    RAIL[l] * n
                } else if id.starts_with("mech_") {
                    BLOCK[l] * n + RAIL[l]
                } else {
                    BLOCK[l] * n
                }
            });
            let lods = glb::decode_lods_with_parts(bytes, id).expect(id);
            let names: Vec<String> = piece
                .animated_parts
                .iter()
                .map(|p| p.name.clone())
                .collect();
            for (l, (m, parts)) in lods.iter().enumerate() {
                assert_eq!(parts, &names, "{id} lod{l}: animated parts");
                assert!(m.triangles() > 0, "{id} lod{l} empty");
                assert!(m.triangles() <= budget[l], "{id} lod{l}: {}", m.triangles());
                for p in &m.positions {
                    let p = Vec3::from_array(*p);
                    assert!(
                        p.cmpge(min - 1e-4).all() && p.cmple(max + 1e-4).all(),
                        "{id} lod{l} {p} outside {min} .. {max}"
                    );
                }
            }
            assert!(lods[0].0.triangles() > lods[1].0.triangles(), "{id}");
            assert!(lods[1].0.triangles() > lods[2].0.triangles(), "{id}");
        }
    }

    #[test]
    fn pieces_are_the_builtin_catalogue_in_id_order() {
        let ids: Vec<&str> = parble_kit::builtin()
            .iter()
            .map(|p| p.def.id.as_str())
            .collect();
        assert_eq!(PIECES.iter().map(|(id, _)| *id).collect::<Vec<_>>(), ids);
        for (i, id) in SHOWCASE.iter().enumerate() {
            assert_eq!(PIECES[showcase_piece(i)].0, *id);
        }
    }

    #[test]
    fn stress_layout_is_a_flat_raised_2000_piece_grid_with_half_cell_gaps() {
        let layout = stress_layout();
        assert_eq!(layout.len(), 2_000);
        assert_eq!((GRID_COLS * GRID_ROWS) as usize, layout.len());
        let keys: HashSet<_> = layout.iter().map(|p| p.cell.to_array()).collect();
        assert_eq!(keys.len(), 2_000, "one piece per grid slot");
        for t in (0..SHOWCASE.len()).map(showcase_piece) {
            assert!(layout.iter().any(|p| p.piece == t), "type {t} unused");
        }
        // One flat layer, every bottom face at the raise height.
        // The undersides clear the floor by at least a cell.
        assert!(
            layout
                .iter()
                .all(|p| p.origin.y == GRID_RAISE_M && p.origin.y >= CELL_M)
        );
        // Neighbours on both axes are 1.5 cells apart, so the 1-cell pieces leave half a cell of
        // gap on every side (a quarter-turn yaw about the bottom-face centre keeps the footprint).
        let pitch = 1.5 * CELL_M;
        for row in 0..GRID_ROWS {
            for col in 0..GRID_COLS {
                let o = grid_origin(col, row);
                if col + 1 < GRID_COLS {
                    let d = grid_origin(col + 1, row) - o;
                    assert!((d.x - pitch).abs() < 1e-5 && d.z.abs() < 1e-5, "{d}");
                }
                if row + 1 < GRID_ROWS {
                    let d = grid_origin(col, row + 1) - o;
                    assert!((d.z + pitch).abs() < 1e-5 && d.x.abs() < 1e-5, "{d}");
                }
            }
        }
        for p in &layout {
            assert_eq!(p.origin, grid_origin(p.cell.x, p.cell.z));
        }
        // The grid stays clear of the tray and ramp (x 0 .. tray length, z within ±5 cm).
        let (min, max) = grid_footprint_m();
        assert!(max.y < -0.10, "grid near edge {} overlaps the tray", max.y);
        assert!(min.x < 0.0 && max.x > parble_sim::scene::tray_length_m());
        let (chunks, bound) = chunk_stats(&layout);
        // Origins span x −1.88 .. 2.53 m and z −3.67 .. −0.16 m in one y layer: 10 × 8 chunks.
        assert_eq!(chunks, 80);
        // 317 occupied (type, chunk) pairs: three thin edge chunks lack one piece type.
        assert_eq!(bound, 317 * LODS);
        assert!(bound <= chunks * SHOWCASE.len() * LODS);
        assert_eq!(stress_layout(), layout, "deterministic");
    }

    #[test]
    fn playroom_layout_draws_the_sim_run_at_the_sim_poses() {
        let kit = parble_kit::builtin();
        let layout = playroom_layout();
        let sim = parble_sim::playroom_run::placements();
        assert_eq!(layout.len(), sim.len());
        for (r, p) in layout.iter().zip(&sim) {
            assert_eq!(PIECES[r.piece].0, p.piece.as_str());
            assert_eq!(r.cell.to_array(), [p.anchor.x, p.anchor.y, p.anchor.z]);
            // Every port position the sim places matches the render transform of the piece.
            let t = placement_transform(r);
            for port in &kit.piece(&p.piece).unwrap().def.ports {
                let n = port.face.normal();
                let local = [n.x * 30_000, 0, n.z * 30_000];
                let w = parble_sim::chunk::place_point_um(p.orientation(), p.anchor, local);
                let want = Vec3::new(w[0] as f32, w[1] as f32, w[2] as f32) * 1e-6;
                let got = t.transform_point(Vec3::from_array(local.map(|v| v as f32 * 1e-6)));
                assert!(got.distance(want) < 1e-6, "{}: {got} vs {want}", p.piece);
            }
        }
    }

    #[test]
    fn mechanism_parts_raise_the_draw_bound_per_part() {
        let layout = KitScene::Mechanisms.layout();
        let sim = parble_sim::mech_showcase::placements();
        assert_eq!(layout.len(), sim.len());
        for (r, p) in layout.iter().zip(&sim) {
            assert_eq!(PIECES[r.piece].0, p.piece.as_str());
            assert_eq!(r.cell.to_array(), [p.anchor.x, p.anchor.y, p.anchor.z]);
        }
        // One chunk: two blocks at 3 draws each, the toggle and the gate at 3 for the body and 3
        // for their one part each.
        assert_eq!(chunk_stats(&layout), (1, 3 + 3 + 6 + 6));
        // No parts, no change: the P1-T4 bounds stay.
        for l in [playroom_layout(), stress_layout()] {
            assert!(l.iter().all(|p| piece_parts(p.piece) == 0));
        }
    }

    #[test]
    fn every_piece_type_uses_every_wood_variant_deterministically() {
        let layout = stress_layout();
        for t in (0..SHOWCASE.len()).map(showcase_piece) {
            for v in 0..WOOD_SETS.len() {
                assert!(
                    layout
                        .iter()
                        .any(|p| p.piece == t && variant_of(p.cell) == v),
                    "type {t} never uses variant {v}"
                );
            }
        }
        let counts: Vec<usize> = (0..WOOD_SETS.len())
            .map(|v| layout.iter().filter(|p| variant_of(p.cell) == v).count())
            .collect();
        // Roughly a third each (2,000 / 3 ≈ 667).
        assert!(
            counts.iter().all(|&n| (550..=780).contains(&n)),
            "{counts:?}"
        );
        assert_eq!(
            variant_of(IVec3::new(3, -2, 7)),
            variant_of(IVec3::new(3, -2, 7))
        );
    }

    /// §12: tileable wood at 4,096 texels per metre on High, 1,024² per [`WOOD_TILE_M`] repeat.
    #[test]
    fn wood_texel_density_is_the_contract_high_tier() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/");
        for set in WOOD_SETS {
            for map in WOOD_MAPS {
                for f in [TextureFamily::Bc7, TextureFamily::Astc] {
                    let p = format!("{root}{}", wood_path(set, map, f));
                    let b = std::fs::read(&p).expect(&p);
                    let dim = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
                    // KTX2 header: pixelWidth at 20, pixelHeight at 24.
                    assert_eq!((dim(20), dim(24)), (1024, 1024), "{p}");
                    assert_eq!(dim(20) as f32 / WOOD_TILE_M, 4096.0);
                }
            }
        }
    }

    /// `index.html` copies exactly the files the kit renderer can fetch (no wood normal maps, GLBs or
    /// sidecars), and each exists.
    #[test]
    fn index_html_ships_exactly_the_fetchable_kit_files() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../");
        let html = std::fs::read_to_string(format!("{root}index.html")).expect("index.html");
        let mut shipped: Vec<String> = html
            .split("href=\"assets/")
            .skip(1)
            .map(|s| s[..s.find('"').expect("closing quote")].to_owned())
            .collect();
        shipped.sort();
        assert_eq!(shipped, fetchable_paths());
        for p in &shipped {
            assert!(
                std::path::Path::new(&format!("{root}assets/{p}")).is_file(),
                "missing {p}"
            );
        }
        assert!(
            !html.contains("rel=\"copy-dir\" href=\"assets/"),
            "assets ship per file"
        );
    }

    #[test]
    fn versioned_url_appends_the_content_hash_only_when_known() {
        let map: HashMap<String, String> = fetchable_paths()
            .into_iter()
            .map(|p| (format!("assets/{p}"), "0123456789".to_owned()))
            .collect();
        for p in fetchable_paths() {
            let site = format!("assets/{p}");
            assert_eq!(versioned_url(&site, &map), format!("{site}?v=0123456789"));
        }
        assert_eq!(versioned_url("assets/other.png", &map), "assets/other.png");
        assert_eq!(
            versioned_url("assets/other.png", &HashMap::new()),
            "assets/other.png"
        );
    }

    #[test]
    fn chunks_floor_negative_origins_and_keep_cell_boundaries() {
        let at = |x, y, z| chunk_of(cell_origin(IVec3::new(x, y, z)));
        assert_eq!(at(-1, 0, -8), IVec3::new(-1, 0, -1));
        assert_eq!(at(7, 8, -9), IVec3::new(0, 1, -2));
        // Every cell origin chunks as its cell does, boundaries included.
        for c in -40..40 {
            let cell = IVec3::new(c, c / 2, -c);
            assert_eq!(
                chunk_of(cell_origin(cell)),
                cell.div_euclid(IVec3::splat(CHUNK_CELLS))
            );
        }
        assert!((CELL_M * 1.0e6 - CELL_UM as f32).abs() < 1e-2);
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
