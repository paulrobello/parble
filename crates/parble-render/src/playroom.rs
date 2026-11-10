//! P1-T5 night playroom environment: rug, room shell, paper lanterns, a warm lamp, toy-block set
//! dressing, an out-of-focus backdrop plate and a baked environment probe. Render-only: nothing here
//! touches the sim, so the ramp's trajectory hash is unchanged.
//!
//! Runtime assets live in `assets/playroom/` and are embedded in the binary (`include_bytes!`), so
//! native and web load them synchronously with no fetches. Sources and generators:
//! `assets-source/playroom/` and `blender/playroom/`. The placement of every piece comes from
//! `assets/playroom/layout.json`, which the probe bake reads too.
//!
//! The shadow budget ([`shadow_light_cap`], at most 3 on High) is enforced for every scenario by
//! [`enforce_shadow_cap`], and the live count is published as [`ShadowLights`].

use std::{f32::consts::FRAC_PI_2, path::Path, sync::OnceLock};

use bevy::{
    asset::{RenderAssetUsages, embedded_asset, io::embedded::EmbeddedAssetRegistry},
    gltf::GltfAssetLabel,
    image::{
        CompressedImageFormats, ImageAddressMode, ImageFilterMode, ImageSampler,
        ImageSamplerDescriptor, ImageType,
    },
    light::{EnvironmentMapLight, NotShadowCaster, NotShadowReceiver},
    math::Affine2,
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::{
        AsBindGroup, Extent3d, ShaderType, TextureDataOrder, TextureDimension, TextureFormat,
        TextureViewDescriptor, TextureViewDimension,
    },
    shader::ShaderRef,
};
use serde::Deserialize;

use crate::{
    Quality,
    camera::MainCamera,
    palette::{LANTERN_PAPER, NIGHT_BLUE, PALE_BLUE, WARM_AMBER as WARM},
};

/// Which environment surrounds the P0 tray. Chosen by the app from the scenario.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    /// The P0 scene: a plain floor, a warm key sun with a pale blue fill, a night-blue backdrop.
    #[default]
    Plain,
    /// The night playroom (`--scenario playroom`).
    Playroom,
}

/// Fixed playroom camera framings (`--view`), matched to the look-dev references. The director
/// plays one as a fixed shot ([`crate::camera::Cues::playroom`]).
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayroomView {
    /// Low across the rug towards the big lantern (ref-07).
    #[default]
    Rug,
    /// The toy-block arches and towers around the small lantern (ref-08).
    Blocks,
}

impl PlayroomView {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "rug" => Some(Self::Rug),
            "blocks" => Some(Self::Blocks),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Rug => "rug",
            Self::Blocks => "blocks",
        }
    }

    /// Depth of field focus distance: the big lantern (rug) or the small lantern (blocks).
    pub fn focus_m(self) -> f32 {
        let lay = layout();
        let (eye, _) = self.eye_target();
        let lantern = match self {
            Self::Rug => &lay.lanterns[0],
            Self::Blocks => &lay.lanterns[1],
        };
        eye.distance(v3(lantern.center))
    }

    /// Camera eye and look-at target, metres.
    pub fn eye_target(self) -> (Vec3, Vec3) {
        match self {
            Self::Rug => (Vec3::new(0.98, 0.15, 0.52), Vec3::new(0.0, 0.04, -0.5)),
            Self::Blocks => (Vec3::new(0.46, 0.085, -0.18), Vec3::new(0.46, 0.06, -1.0)),
        }
    }
}

/// Most lights allowed to cast shadows at a tier. High and Ultra are capped at 3 (P1-T5).
pub const fn shadow_light_cap(quality: Quality) -> usize {
    match quality {
        Quality::Low => 0,
        Quality::Medium => 1,
        Quality::High | Quality::Ultra => 3,
    }
}

/// Shadow-casting lights after [`enforce_shadow_cap`] ran this frame, and the tier's cap.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShadowLights {
    pub casting: usize,
    pub cap: usize,
}

/// Lower keeps its shadow first when the cap bites. Lights without one rank last (255).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ShadowPriority(pub u8);

/// Turns off shadows on the lowest-priority lights past the tier's cap and records the count.
pub fn enforce_shadow_cap(
    quality: Res<Quality>,
    mut stats: ResMut<ShadowLights>,
    mut dir: Query<(Entity, &mut DirectionalLight, Option<&ShadowPriority>)>,
    mut point: Query<(Entity, &mut PointLight, Option<&ShadowPriority>)>,
    mut spot: Query<(Entity, &mut SpotLight, Option<&ShadowPriority>)>,
) {
    let rank = |p: Option<&ShadowPriority>| p.map_or(255, |p| p.0);
    // (priority, kind, entity): directional before spot before point at equal priority.
    let mut on: Vec<(u8, u8, Entity)> = Vec::new();
    on.extend(
        dir.iter()
            .filter(|(_, l, _)| l.shadow_maps_enabled)
            .map(|(e, _, p)| (rank(p), 0, e)),
    );
    on.extend(
        spot.iter()
            .filter(|(_, l, _)| l.shadow_maps_enabled)
            .map(|(e, _, p)| (rank(p), 1, e)),
    );
    on.extend(
        point
            .iter()
            .filter(|(_, l, _)| l.shadow_maps_enabled)
            .map(|(e, _, p)| (rank(p), 2, e)),
    );
    on.sort();
    let cap = shadow_light_cap(*quality);
    for &(_, kind, e) in on.iter().skip(cap) {
        match kind {
            0 => dir
                .get_mut(e)
                .map(|(_, mut l, _)| l.shadow_maps_enabled = false),
            1 => spot
                .get_mut(e)
                .map(|(_, mut l, _)| l.shadow_maps_enabled = false),
            _ => point
                .get_mut(e)
                .map(|(_, mut l, _)| l.shadow_maps_enabled = false),
        }
        .ok();
    }
    let now = ShadowLights {
        casting: on.len().min(cap),
        cap,
    };
    if *stats != now {
        *stats = now;
    }
}

#[derive(Deserialize, Debug)]
pub struct Layout {
    pub room: Room,
    pub floor: Floor,
    pub rug: Rug,
    pub backdrop: Backdrop,
    pub lanterns: Vec<Lantern>,
    pub lamp: Lamp,
    pub moon: Moon,
    pub blocks: Vec<Block>,
}

#[derive(Deserialize, Debug)]
pub struct Room {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub color: [f32; 3],
}

#[derive(Deserialize, Debug)]
pub struct Floor {
    pub color: [f32; 3],
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Rug {
    pub center: [f32; 2],
    pub size: [f32; 2],
    pub tile_m: f32,
}

#[derive(Deserialize, Debug)]
pub struct Backdrop {
    pub center: [f32; 3],
    pub size: [f32; 2],
}

#[derive(Deserialize, Debug)]
pub struct Lantern {
    pub center: [f32; 3],
    pub radius: f32,
    pub lumens: f32,
    pub shadows: bool,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Lamp {
    pub base: [f32; 3],
    pub height: f32,
    pub shade_radius: f32,
    pub lumens: f32,
    pub shadows: bool,
}

#[derive(Deserialize, Debug)]
pub struct Moon {
    pub from: [f32; 3],
    pub lux: f32,
    pub shadows: bool,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub piece: String,
    pub at: [f32; 3],
    pub yaw_deg: f32,
}

pub fn layout() -> &'static Layout {
    static LAYOUT: OnceLock<Layout> = OnceLock::new();
    LAYOUT.get_or_init(|| {
        serde_json::from_str(include_str!("../../../assets/playroom/layout.json"))
            .expect("assets/playroom/layout.json parses")
    })
}

/// Toy-block GLBs (blender/playroom/blocks.py), embedded and served from `embedded://`.
const PIECES: [(&str, &[u8]); 6] = [
    (
        "cube",
        include_bytes!("../../../assets/playroom/toy_cube.glb"),
    ),
    (
        "brick",
        include_bytes!("../../../assets/playroom/toy_brick.glb"),
    ),
    (
        "plank",
        include_bytes!("../../../assets/playroom/toy_plank.glb"),
    ),
    (
        "pillar",
        include_bytes!("../../../assets/playroom/toy_pillar.glb"),
    ),
    (
        "roof",
        include_bytes!("../../../assets/playroom/toy_roof.glb"),
    ),
    (
        "arch",
        include_bytes!("../../../assets/playroom/toy_arch.glb"),
    ),
];

/// Footprint radius (xz, metres) of each piece, for the keep-out test.
pub fn piece_radius(piece: &str) -> Option<f32> {
    Some(match piece {
        "cube" => 0.022,
        "brick" | "roof" => 0.034,
        "plank" => 0.062,
        "pillar" => 0.015,
        "arch" => 0.048,
        _ => return None,
    })
}

const RUG_BASE: &[u8] = include_bytes!("../../../assets/playroom/rug.base_color.ktx2");
const RUG_NORMAL: &[u8] = include_bytes!("../../../assets/playroom/rug.normal.ktx2");
const BACKDROP: &[u8] = include_bytes!("../../../assets/playroom/backdrop.ktx2");
const ENV_SPECULAR: &[u8] = include_bytes!("../../../assets/playroom/env_specular.rgba16f");
const ENV_DIFFUSE: &[u8] = include_bytes!("../../../assets/playroom/env_diffuse.rgba16f");
/// Must match blender/playroom/probe.py (SPEC_SIZE, DIFF_SIZE).
const ENV_SPECULAR_SIZE: u32 = 64;
const ENV_DIFFUSE_SIZE: u32 = 16;
/// Scales the probe's relative radiance to cd/m². Raised from 15 with the ambient, moon, rim and
/// lamp when the lantern lights were dimmed (ref-12 floor pool, 2026-11-10).
const ENV_INTENSITY: f32 = 22.5;
/// Backdrop plate luminance scale, cd/m² at texel value 1.
const BACKDROP_NITS: f32 = 5.0;

/// Night sky behind everything (the room shell normally hides it).
pub const NIGHT: Color = Color::srgb(0.012, 0.018, 0.045);
/// Pale blue window rim from behind the set (the backdrop side), lux. No shadows.
pub const RIM_LUX: f32 = 1_800.0;
/// Where the rim comes from: high over the backdrop, left of the window.
const RIM_FROM: Vec3 = Vec3::new(-0.8, 1.6, -2.0);

pub struct PlayroomPlugin;

impl Plugin for PlayroomPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Environment>()
            .init_resource::<ShadowLights>()
            .add_systems(
                Startup,
                (spawn_playroom_lights, spawn_playroom_set).run_if(is_playroom),
            )
            .add_systems(PostStartup, attach_environment_map.run_if(is_playroom))
            .add_systems(PostUpdate, enforce_shadow_cap);
        embedded_asset!(app, "lantern.wesl");
        app.add_plugins(MaterialPlugin::<LanternMaterial>::default());
    }
}

const LANTERN_SHADER: &str = "embedded://parble_render/lantern.wesl";

/// Paper shade emissive face on, cd/m² per unit of [`LANTERN_PAPER`] (ref-12, 2026-11-10).
pub const LANTERN_PAPER_NITS: f32 = 1.2;
/// Bamboo rib emissive, cd/m² per unit of warm amber: darker lines on the lit paper.
pub const LANTERN_RIB_NITS: f32 = 0.45;
/// Shade emissive scale at the silhouette, and the falloff exponent on N.V towards it.
pub const LANTERN_EDGE: f32 = 0.15;
pub const LANTERN_FALLOFF_POWER: f32 = 1.0;
/// The inner glow seen through the paper, added on top: `LANTERN_HOT * (N.V)^LANTERN_HOT_POWER`.
pub const LANTERN_HOT: f32 = 1.6;
pub const LANTERN_HOT_POWER: f32 = 5.0;
/// Lantern point lights against `layout.json`'s lumens, which stay as the probe bake's values
/// (`blender/playroom/probe.py`): a modest pool on the rug, as in ref-12.
pub const LANTERN_LIGHT_SCALE: f32 = 0.25;
/// Moon and lamp against `layout.json`, raised to hold the room's light level with the lanterns
/// dimmed.
pub const MOON_LUX_SCALE: f32 = 2.2;
pub const LAMP_LUMENS_SCALE: f32 = 3.0;

/// Shade falloff for `lantern.wesl` (binding 100).
#[derive(ShaderType, Reflect, Debug, Clone, Copy, Default)]
pub struct LanternParams {
    /// Emissive scale at the silhouette, where the view grazes the paper.
    pub edge: f32,
    /// Falloff exponent on N.V.
    pub power: f32,
    /// Inner glow added on top, and its exponent on N.V.
    pub hot: f32,
    pub hot_power: f32,
}

/// Paper lantern shade and ribs: the emissive dims from face on towards the silhouette.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct LanternExt {
    #[uniform(100)]
    pub params: LanternParams,
}

impl MaterialExtension for LanternExt {
    fn fragment_shader() -> ShaderRef {
        LANTERN_SHADER.into()
    }
}

pub type LanternMaterial = ExtendedMaterial<StandardMaterial, LanternExt>;

fn is_playroom(env: Res<Environment>) -> bool {
    *env == Environment::Playroom
}

fn v3(a: [f32; 3]) -> Vec3 {
    Vec3::from_array(a)
}

/// The playroom's lights. No asset parameters, so tests can run it in a bare world.
pub fn spawn_playroom_lights(mut commands: Commands, quality: Res<Quality>) {
    let lay = layout();
    let shadows = *quality != Quality::Low;
    commands.insert_resource(ClearColor(NIGHT));
    commands.insert_resource(GlobalAmbientLight {
        color: NIGHT_BLUE,
        brightness: 6.0,
        ..default()
    });
    commands.spawn((
        DirectionalLight {
            color: PALE_BLUE,
            illuminance: lay.moon.lux * MOON_LUX_SCALE,
            shadow_maps_enabled: shadows && lay.moon.shadows,
            shadow_depth_bias: 0.0004,
            shadow_normal_bias: 0.6,
            ..default()
        },
        Transform::from_translation(v3(lay.moon.from))
            .looking_at(Vec3::new(0.36, 0.0, 0.0), Vec3::Y),
        bevy::light::CascadeShadowConfigBuilder {
            num_cascades: 1,
            maximum_distance: 2.5,
            minimum_distance: 0.01,
            ..default()
        }
        .build(),
        ShadowPriority(1),
    ));
    commands.spawn((
        DirectionalLight {
            color: PALE_BLUE,
            illuminance: RIM_LUX,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_translation(RIM_FROM).looking_at(Vec3::new(0.36, 0.0, -0.3), Vec3::Y),
    ));
    for (i, lan) in lay.lanterns.iter().enumerate() {
        commands.spawn((
            PointLight {
                color: WARM,
                intensity: lan.lumens * LANTERN_LIGHT_SCALE,
                range: 4.0,
                radius: lan.radius * 0.6,
                shadow_maps_enabled: shadows && lan.shadows,
                shadow_depth_bias: 0.004,
                shadow_normal_bias: 0.3,
                ..default()
            },
            Transform::from_translation(v3(lan.center)),
            ShadowPriority(if i == 0 { 0 } else { 3 }),
        ));
    }
    let lamp = &lay.lamp;
    commands.spawn((
        PointLight {
            color: WARM,
            intensity: lamp.lumens * LAMP_LUMENS_SCALE,
            range: 6.0,
            radius: lamp.shade_radius * 0.5,
            shadow_maps_enabled: shadows && lamp.shadows,
            shadow_depth_bias: 0.01,
            shadow_normal_bias: 0.4,
            ..default()
        },
        Transform::from_translation(v3(lamp.base) + Vec3::Y * (lamp.height - 0.02)),
        ShadowPriority(2),
    ));
}

fn sampler(repeat: bool) -> ImageSampler {
    let mode = if repeat {
        ImageAddressMode::Repeat
    } else {
        ImageAddressMode::ClampToEdge
    };
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: mode,
        address_mode_v: mode,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    })
}

/// An embedded KTX2 (RGBA8, zstd, full mip chain; blender/playroom/encode.sh).
pub fn ktx2_image(bytes: &[u8], srgb: bool, repeat: bool) -> Image {
    Image::from_buffer(
        bytes,
        ImageType::Extension("ktx2"),
        CompressedImageFormats::NONE,
        srgb,
        sampler(repeat),
        RenderAssetUsages::RENDER_WORLD,
    )
    .expect("embedded playroom KTX2 decodes")
}

/// Bytes in an RGBA16F cubemap with `levels` mips from `size`.
pub const fn cube_bytes(size: u32, levels: u32) -> usize {
    let mut total = 0;
    let mut l = 0;
    while l < levels {
        let s = size >> l;
        total += (s * s) as usize * 8 * 6;
        l += 1;
    }
    total
}

/// A raw RGBA16F cubemap in layer-major order (blender/playroom/probe.py).
pub fn cube_image(bytes: &[u8], size: u32, levels: u32) -> Image {
    assert_eq!(bytes.len(), cube_bytes(size, levels), "probe cubemap size");
    let mut img = Image {
        data: Some(bytes.to_vec()),
        data_order: TextureDataOrder::LayerMajor,
        ..default()
    };
    img.texture_descriptor.size = Extent3d {
        width: size,
        height: size,
        depth_or_array_layers: 6,
    };
    img.texture_descriptor.mip_level_count = levels;
    img.texture_descriptor.format = TextureFormat::Rgba16Float;
    img.texture_descriptor.dimension = TextureDimension::D2;
    img.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    img.asset_usage = RenderAssetUsages::RENDER_WORLD;
    img
}

#[allow(clippy::too_many_arguments)]
fn spawn_playroom_set(
    mut commands: Commands,
    registry: Res<EmbeddedAssetRegistry>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut lantern_materials: ResMut<Assets<LanternMaterial>>,
    mut images: ResMut<Assets<Image>>,
    kit_scene: Option<Res<crate::kit::KitScene>>,
) {
    let lay = layout();
    let kit = kit_scene.map_or_else(Vec::new, |k| k.layout());

    // Room shell: an inside-out box, dark blue, lit only by the room's own lights.
    let lo = v3(lay.room.min);
    let hi = v3(lay.room.max);
    let wall = materials.add(StandardMaterial {
        base_color: Color::srgb_from_array(lay.room.color),
        perceptual_roughness: 1.0,
        reflectance: 0.2,
        cull_mode: None,
        double_sided: true,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::from_corners(lo, hi))),
        MeshMaterial3d(wall),
        // The cuboid mesh is centred on its own origin.
        Transform::from_translation((lo + hi) / 2.0),
        NotShadowCaster,
    ));
    let size = hi - lo;
    let floor = materials.add(StandardMaterial {
        base_color: Color::srgb_from_array(lay.floor.color),
        perceptual_roughness: 0.75,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(size.x, size.z))),
        MeshMaterial3d(floor),
        Transform::from_xyz((lo.x + hi.x) / 2.0, -0.002, (lo.z + hi.z) / 2.0),
    ));

    // Rug: the scripted square-in-square material, tiled at its recorded physical size.
    let rug = &lay.rug;
    let rug_mat = materials.add(StandardMaterial {
        base_color_texture: Some(images.add(ktx2_image(RUG_BASE, true, true))),
        normal_map_texture: Some(images.add(ktx2_image(RUG_NORMAL, false, true))),
        perceptual_roughness: 0.92,
        reflectance: 0.3,
        uv_transform: Affine2::from_scale(Vec2::new(
            rug.size[0] / rug.tile_m,
            rug.size[1] / rug.tile_m,
        )),
        ..default()
    });
    let mut rug_mesh = Plane3d::default()
        .mesh()
        .size(rug.size[0], rug.size[1])
        .build();
    rug_mesh
        .generate_tangents()
        .expect("plane mesh has normals and uvs");
    commands.spawn((
        Mesh3d(meshes.add(rug_mesh)),
        MeshMaterial3d(rug_mat),
        Transform::from_xyz(rug.center[0], -0.0005, rug.center[1]),
    ));

    // Backdrop plate: self-lit, never shadowed, standing in front of the back wall.
    let bd = &lay.backdrop;
    // Emissive rather than unlit, so the plate sits on the same exposure as the lit scene.
    let plate = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive: LinearRgba::WHITE * BACKDROP_NITS,
        emissive_texture: Some(images.add(ktx2_image(BACKDROP, true, false))),
        perceptual_roughness: 1.0,
        reflectance: 0.0,
        cull_mode: None,
        double_sided: true,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(bd.size[0], bd.size[1]))),
        MeshMaterial3d(plate),
        Transform::from_translation(v3(bd.center)).with_rotation(Quat::from_rotation_x(FRAC_PI_2)),
        NotShadowCaster,
        NotShadowReceiver,
    ));

    // Paper lanterns: an emissive shade with darker bamboo ribs. The light is spawned separately.
    // The emissive stays below ACES's shoulder so the paper is not clipped and the ribs show. It
    // fades towards the silhouette with an inner glow at the centre, and the paper reflects little
    // and takes no shadows, so it reads as lit from inside rather than by the room (ref-12).
    let falloff = LanternExt {
        params: LanternParams {
            edge: LANTERN_EDGE,
            power: LANTERN_FALLOFF_POWER,
            hot: LANTERN_HOT,
            hot_power: LANTERN_HOT_POWER,
        },
    };
    let paper = lantern_materials.add(LanternMaterial {
        base: StandardMaterial {
            base_color: Color::linear_rgb(0.312, 0.276, 0.223),
            emissive: LinearRgba::from(LANTERN_PAPER) * LANTERN_PAPER_NITS,
            perceptual_roughness: 0.9,
            ..default()
        },
        extension: falloff.clone(),
    });
    let rib = lantern_materials.add(LanternMaterial {
        base: StandardMaterial {
            base_color: Color::linear_rgb(0.086, 0.059, 0.036),
            emissive: LinearRgba::from(WARM) * LANTERN_RIB_NITS,
            perceptual_roughness: 0.9,
            ..default()
        },
        extension: falloff,
    });
    let sphere = meshes.add(Sphere::new(1.0).mesh().uv(48, 24));
    for lan in &lay.lanterns {
        let c = v3(lan.center);
        let r = lan.radius;
        commands.spawn((
            Mesh3d(sphere.clone()),
            MeshMaterial3d(paper.clone()),
            Transform::from_translation(c).with_scale(Vec3::new(r, r * 0.94, r)),
            NotShadowCaster,
            NotShadowReceiver,
        ));
        for k in -3i32..=3 {
            let lat = k as f32 * 0.38;
            let ring = r * lat.cos() * 1.004;
            commands.spawn((
                Mesh3d(
                    meshes.add(
                        Torus::new(ring - 0.0012, ring + 0.0012)
                            .mesh()
                            .major_resolution(48),
                    ),
                ),
                MeshMaterial3d(rib.clone()),
                Transform::from_translation(c + Vec3::Y * (r * 0.94 * lat.sin())),
                NotShadowCaster,
            ));
        }
    }

    // Warm lamp: base, stem and a glowing conical shade, in the back-left corner.
    let lamp = &lay.lamp;
    let base = v3(lamp.base);
    let dark_wood = materials.add(StandardMaterial {
        base_color: Color::srgb(0.18, 0.11, 0.07),
        perceptual_roughness: 0.6,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(0.08, 0.02))),
        MeshMaterial3d(dark_wood.clone()),
        Transform::from_translation(base + Vec3::Y * 0.01),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(0.008, lamp.height))),
        MeshMaterial3d(dark_wood),
        Transform::from_translation(base + Vec3::Y * (lamp.height / 2.0)),
    ));
    let shade = materials.add(StandardMaterial {
        base_color: Color::srgb(0.95, 0.85, 0.7),
        emissive: LinearRgba::from(WARM) * 40.0,
        cull_mode: None,
        double_sided: true,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(ConicalFrustum {
            radius_top: lamp.shade_radius * 0.6,
            radius_bottom: lamp.shade_radius,
            height: 0.18,
        })),
        MeshMaterial3d(shade),
        Transform::from_translation(base + Vec3::Y * lamp.height),
        NotShadowCaster,
    ));

    // Toy blocks: Blender-generated GLBs served from the embedded asset source.
    for (name, bytes) in PIECES {
        let path = format!("parble_render/playroom/toy_{name}.glb");
        registry.insert_asset(Path::new(&path).to_path_buf(), Path::new(&path), bytes);
    }
    for b in lay.blocks.iter().filter(|b| block_clear_of(b, &kit)) {
        let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(format!(
            "embedded://parble_render/playroom/toy_{}.glb",
            b.piece
        )));
        commands.spawn((
            bevy::world_serialization::WorldAssetRoot(scene),
            Transform::from_translation(v3(b.at))
                .with_rotation(Quat::from_rotation_y(b.yaw_deg.to_radians())),
        ));
    }
}

/// Whether toy block `b` stands clear of every kit piece's cell (plus a 12 mm margin). The
/// playroom run is laid clear of all of them; the showcase puts the all-pieces track on the rug and
/// leaves out the blocks it would go through.
pub fn block_clear_of(b: &Block, kit: &[crate::kit::Placement]) -> bool {
    let r = piece_radius(&b.piece).unwrap_or(0.0);
    let half = crate::kit::CELL_M / 2.0 + 0.012;
    let p = Vec2::new(b.at[0], b.at[2]);
    kit.iter().all(|k| {
        let c = Vec2::new(k.origin.x, k.origin.z);
        (p - p.clamp(c - half, c + half)).length() > r
    })
}

/// Attaches the baked probe to the main camera as its environment light (browser WebGPU has no
/// binding arrays, so a per-region reflection probe would be ignored there).
fn attach_environment_map(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    cams: Query<Entity, With<MainCamera>>,
) {
    let levels = ENV_SPECULAR_SIZE.ilog2() + 1;
    let env = EnvironmentMapLight {
        diffuse_map: images.add(cube_image(ENV_DIFFUSE, ENV_DIFFUSE_SIZE, 1)),
        specular_map: images.add(cube_image(ENV_SPECULAR, ENV_SPECULAR_SIZE, levels)),
        intensity: ENV_INTENSITY,
        ..default()
    };
    for e in &cams {
        commands.entity(e).insert(env.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    fn count(world: &mut World) -> usize {
        let mut n = 0;
        n += world
            .query::<&DirectionalLight>()
            .iter(world)
            .filter(|l| l.shadow_maps_enabled)
            .count();
        n += world
            .query::<&PointLight>()
            .iter(world)
            .filter(|l| l.shadow_maps_enabled)
            .count();
        n += world
            .query::<&SpotLight>()
            .iter(world)
            .filter(|l| l.shadow_maps_enabled)
            .count();
        n
    }

    fn world(q: Quality) -> World {
        let mut w = World::new();
        w.insert_resource(q);
        w.init_resource::<ShadowLights>();
        w
    }

    #[test]
    fn cap_is_three_on_high() {
        assert_eq!(shadow_light_cap(Quality::High), 3);
        assert_eq!(shadow_light_cap(Quality::Ultra), 3);
        assert!(shadow_light_cap(Quality::Medium) <= 3);
        assert_eq!(shadow_light_cap(Quality::Low), 0);
    }

    #[test]
    fn enforcement_turns_off_extra_shadow_lights_by_priority() {
        let mut w = world(Quality::High);
        let keep = w
            .spawn((
                PointLight {
                    shadow_maps_enabled: true,
                    ..default()
                },
                ShadowPriority(0),
            ))
            .id();
        for _ in 0..4 {
            w.spawn(PointLight {
                shadow_maps_enabled: true,
                ..default()
            });
        }
        w.spawn(SpotLight {
            shadow_maps_enabled: true,
            ..default()
        });
        w.spawn(DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        });
        w.run_system_once(enforce_shadow_cap).unwrap();
        assert_eq!(count(&mut w), 3);
        assert!(w.get::<PointLight>(keep).unwrap().shadow_maps_enabled);
        assert_eq!(
            *w.resource::<ShadowLights>(),
            ShadowLights { casting: 3, cap: 3 }
        );
    }

    #[test]
    fn playroom_lights_respect_the_cap_on_every_tier() {
        for q in [Quality::Low, Quality::Medium, Quality::High, Quality::Ultra] {
            let mut w = world(q);
            w.run_system_once(spawn_playroom_lights).unwrap();
            w.run_system_once(enforce_shadow_cap).unwrap();
            let n = count(&mut w);
            assert!(n <= shadow_light_cap(q), "{q:?}: {n} shadow lights");
            assert_eq!(w.resource::<ShadowLights>().casting, n);
        }
        // High uses the full budget: moon, big lantern, lamp.
        let mut w = world(Quality::High);
        w.run_system_once(spawn_playroom_lights).unwrap();
        w.run_system_once(enforce_shadow_cap).unwrap();
        assert_eq!(count(&mut w), 3);
    }

    #[test]
    fn playroom_mixes_warm_lights_with_a_pale_blue_moon_and_rim() {
        let mut w = world(Quality::High);
        w.run_system_once(spawn_playroom_lights).unwrap();
        let dirs: Vec<DirectionalLight> =
            w.query::<&DirectionalLight>().iter(&w).cloned().collect();
        assert_eq!(dirs.len(), 2, "moon and rim");
        assert!(dirs.iter().all(|l| l.color == PALE_BLUE));
        // The rim is free: no shadow map, so the moon keeps the directional shadow slot.
        assert_eq!(dirs.iter().filter(|l| l.shadow_maps_enabled).count(), 1);
        let points: Vec<PointLight> = w.query::<&PointLight>().iter(&w).cloned().collect();
        assert_eq!(points.len(), 3, "two lanterns and the lamp");
        assert!(points.iter().all(|l| l.color == WARM));
    }

    #[test]
    fn set_dressing_stays_clear_of_the_tray() {
        use parble_sim::scene;
        let lay = layout();
        let half_w = scene::tray_width_m() / 2.0 + 0.012;
        let tray_min = Vec2::new(-0.012, -half_w);
        let tray_max = Vec2::new(scene::tray_length_m() + 0.012, half_w);
        let clear = |p: Vec2, r: f32| {
            let d = p - p.clamp(tray_min, tray_max);
            d.length() > r
        };
        for b in &lay.blocks {
            let r = piece_radius(&b.piece).unwrap_or_else(|| panic!("unknown piece {}", b.piece));
            assert!(
                clear(Vec2::new(b.at[0], b.at[2]), r),
                "{b:?} overlaps the tray"
            );
        }
        for l in &lay.lanterns {
            assert!(clear(Vec2::new(l.center[0], l.center[2]), l.radius));
        }
    }

    #[test]
    fn set_dressing_stays_clear_of_the_kit_run() {
        use parble_sim::{playroom_run::RUN, scene};
        let cell = scene::cell_m();
        let half = cell / 2.0 + 0.012;
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for &(_, [x, _, z], _) in &RUN {
            let c = Vec2::new(x as f32, z as f32) * cell;
            lo = lo.min(c - half);
            hi = hi.max(c + half);
        }
        let clear = |p: Vec2, r: f32| (p - p.clamp(lo, hi)).length() > r;
        let lay = layout();
        for b in &lay.blocks {
            let r = piece_radius(&b.piece).unwrap();
            assert!(
                clear(Vec2::new(b.at[0], b.at[2]), r),
                "{b:?} overlaps the run"
            );
        }
        for l in &lay.lanterns {
            assert!(clear(Vec2::new(l.center[0], l.center[2]), l.radius));
        }
        // The run stands on the rug.
        let (c, s) = (
            Vec2::from_array(lay.rug.center),
            Vec2::from_array(lay.rug.size),
        );
        assert!(lo.cmpge(c - s / 2.0).all() && hi.cmple(c + s / 2.0).all());
    }

    #[test]
    fn the_showcase_leaves_out_only_the_blocks_on_the_allpieces_track() {
        use crate::kit::{KitScene, run_layout};
        let lay = layout();
        let playroom = run_layout(&parble_sim::playroom_run::RUN);
        assert!(lay.blocks.iter().all(|b| block_clear_of(b, &playroom)));
        let track = KitScene::AllPieces.layout();
        let dropped: Vec<_> = lay
            .blocks
            .iter()
            .filter(|b| !block_clear_of(b, &track))
            .map(|b| (b.piece.as_str(), b.at))
            .collect();
        assert_eq!(dropped, [("cube", [0.68, 0.0, -0.42])]);
        // The track stands on the rug, clear of both lanterns.
        let (c, s) = (
            Vec2::from_array(lay.rug.center),
            Vec2::from_array(lay.rug.size),
        );
        for k in &track {
            let p = Vec2::new(k.origin.x, k.origin.z);
            assert!(p.cmpge(c - s / 2.0).all() && p.cmple(c + s / 2.0).all());
            for l in &lay.lanterns {
                assert!(p.distance(Vec2::new(l.center[0], l.center[2])) > l.radius + 0.03);
            }
        }
    }

    #[test]
    fn embedded_assets_decode() {
        let rug = ktx2_image(RUG_BASE, true, true);
        assert!(rug.texture_descriptor.mip_level_count > 1);
        assert!(
            ktx2_image(RUG_NORMAL, false, true)
                .texture_descriptor
                .mip_level_count
                > 1
        );
        assert!(
            ktx2_image(BACKDROP, true, false)
                .texture_descriptor
                .mip_level_count
                > 1
        );
        let spec = cube_image(
            ENV_SPECULAR,
            ENV_SPECULAR_SIZE,
            ENV_SPECULAR_SIZE.ilog2() + 1,
        );
        assert_eq!(spec.texture_descriptor.size.depth_or_array_layers, 6);
        cube_image(ENV_DIFFUSE, ENV_DIFFUSE_SIZE, 1);
        assert_eq!(PIECES.len(), 6);
        for b in &layout().blocks {
            assert!(PIECES.iter().any(|(n, _)| *n == b.piece));
        }
    }
}
