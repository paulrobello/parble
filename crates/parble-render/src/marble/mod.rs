//! Marble looks (P1-T6): one visual per [`MarbleKind`] and quality tier.
//!
//! - Cat's eye, High and above: a clear sphere with Bevy's screen-space specular transmission
//!   (IOR 1.5, roughness 0) around opaque inner vane geometry ([`vane_mesh`]). Thickness is the
//!   radius: Bevy offsets the background lookup along the refracted ray by the thickness, and the
//!   vanes sit at the centre. The sphere casts no shadow. The camera gets
//!   [`ScreenSpaceTransmission`].
//! - Cat's eye, Low and Medium: an opaque sphere with the [`VaneGlassExt`] fallback shader
//!   (`vane_glass.wesl`), which refracts the view ray into the marble and marches the same vane
//!   surfaces analytically (16 steps on Low, 32 on Medium).
//! - Glazed: a glossy ceramic body under a clearcoat.
//! - Steel: metallic, reflecting the environment map.
//!
//! The vane colours are the Higgsfield textures in `assets/marbles/` (provenance in
//! `assets-source/marbles/`), embedded in the binary. A camera without an environment map gets the
//! procedural room from [`env`] until the P1-T5 probe replaces it.

pub mod env;

use bevy::{
    asset::{RenderAssetUsages, embedded_asset},
    image::{CompressedImageFormats, ImageSampler, ImageType},
    light::{EnvironmentMapLight, NotShadowCaster},
    mesh::{Indices, PrimitiveTopology},
    pbr::{ExtendedMaterial, MaterialExtension, ScreenSpaceTransmission},
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
};
use parble_sim::{marble::MarbleKind, scene};

use crate::{MarbleVisual, Quality};

const SHADER: &str = "embedded://parble_render/marble/vane_glass.wesl";
const VANE_RED_PNG: &[u8] = include_bytes!("../../../../assets/marbles/vane-red.png");
const VANE_GREEN_PNG: &[u8] = include_bytes!("../../../../assets/marbles/vane-green.png");

/// Vane petal shape in unit-sphere units (shared by the mesh and the fallback shader).
pub const VANE_HEIGHT: f32 = 0.78;
pub const VANE_WIDTH: f32 = 0.62;
pub const VANE_TWIST: f32 = 0.9;
pub const VANE_CURL: f32 = 0.6;
pub const VANE_BLADES: u32 = 4;
/// Environment map intensity, cd/m² per unit of the procedural radiance.
pub const ENV_INTENSITY: f32 = 3_000.0;

/// Which marble the scene shows (`--marble`). Read once at startup.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarbleLook(pub MarbleKind);

/// True when the tier renders the cat's eye with real screen-space transmission.
pub fn uses_transmission(q: Quality) -> bool {
    !matches!(q, Quality::Low | Quality::Medium)
}

/// Fallback march steps per tier.
pub fn fallback_steps(q: Quality) -> f32 {
    if q == Quality::Low { 16.0 } else { 32.0 }
}

#[derive(Clone, Copy, Debug, ShaderType, Reflect)]
pub struct VaneParams {
    pub radius: f32,
    pub height: f32,
    pub width: f32,
    pub twist: f32,
    pub curl: f32,
    pub steps: f32,
    pub vane_alpha: f32,
    pub blades: f32,
    pub body: Vec4,
}

/// The fallback glass extension: vane parameters and the two vane textures.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct VaneGlassExt {
    #[uniform(100)]
    pub params: VaneParams,
    #[texture(101)]
    #[sampler(102)]
    pub vane_a: Handle<Image>,
    #[texture(103)]
    #[sampler(104)]
    pub vane_b: Handle<Image>,
}

impl MaterialExtension for VaneGlassExt {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}

pub type VaneGlassMaterial = ExtendedMaterial<StandardMaterial, VaneGlassExt>;

pub struct MarblePlugin;

impl Plugin for MarblePlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "vane_glass.wesl");
        app.add_plugins(MaterialPlugin::<VaneGlassMaterial>::default())
            .init_resource::<MarbleLook>()
            .add_systems(Startup, spawn_marble)
            .add_systems(Update, equip_cameras);
    }
}

fn vane_image(png: &[u8]) -> Image {
    Image::from_buffer(
        png,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::linear(),
        RenderAssetUsages::RENDER_WORLD,
    )
    .expect("embedded vane PNG decodes")
}

/// Height of the petal's radial extent at `y` (unit-sphere units).
pub fn petal_extent(y: f32) -> f32 {
    let t = (1.0 - (y / VANE_HEIGHT).powi(2)).clamp(0.0, 1.0);
    VANE_WIDTH * t.powf(0.6)
}

/// The inner vane surfaces for `blades` (blade indices), scaled to `radius`. UV u runs from the
/// axis to the petal edge, v from top to bottom, matching the fallback shader's lookup.
pub fn vane_mesh(blades: &[u32], radius: f32) -> Mesh {
    const NS: u32 = 8;
    const NY: u32 = 32;
    let point = |b: u32, s: f32, y: f32| {
        let rho = s * petal_extent(y);
        let a = b as f32 * std::f32::consts::TAU / VANE_BLADES as f32
            + VANE_TWIST * y
            + VANE_CURL * rho;
        Vec3::new(rho * a.cos(), y, rho * a.sin()) * radius
    };
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    for &b in blades {
        let base = positions.len() as u32;
        for j in 0..=NY {
            let v = j as f32 / NY as f32;
            let y = VANE_HEIGHT * (1.0 - 2.0 * v);
            for i in 0..=NS {
                let s = i as f32 / NS as f32;
                let p = point(b, s, y);
                let e = 1e-3;
                let ds = point(b, (s + e).min(1.0), y) - point(b, (s - e).max(0.0), y);
                let dy = point(b, s, y + e) - point(b, s, y - e);
                let n = ds.cross(dy).normalize_or(Vec3::Z);
                positions.push(p.to_array());
                normals.push(n.to_array());
                uvs.push([s, v]);
            }
        }
        let row = NS + 1;
        for j in 0..NY {
            for i in 0..NS {
                let a = base + j * row + i;
                indices.extend_from_slice(&[a, a + row, a + 1, a + 1, a + row, a + row + 1]);
            }
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

#[allow(clippy::too_many_arguments)]
fn spawn_marble(
    mut commands: Commands,
    look: Res<MarbleLook>,
    quality: Res<Quality>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut vane_glass: ResMut<Assets<VaneGlassMaterial>>,
) {
    let r = scene::standard_marble_radius_m();
    let sphere = meshes.add(Sphere::new(r).mesh().uv(48, 24));
    let spawn = scene::marble_spawn_m();
    let mut marble = commands.spawn((
        Transform::from_xyz(spawn.x, spawn.y, spawn.z),
        Visibility::default(),
        MarbleVisual(0),
    ));
    match look.0 {
        MarbleKind::CatsEye => {
            let red = images.add(vane_image(VANE_RED_PNG));
            let green = images.add(vane_image(VANE_GREEN_PNG));
            if uses_transmission(*quality) {
                let glass = standard.add(StandardMaterial {
                    base_color: Color::srgb(0.96, 0.98, 1.0),
                    perceptual_roughness: 0.0,
                    reflectance: 0.5,
                    specular_transmission: 1.0,
                    diffuse_transmission: 0.0,
                    thickness: r,
                    ior: 1.5,
                    ..default()
                });
                let vane = |tex: Handle<Image>| StandardMaterial {
                    base_color_texture: Some(tex),
                    perceptual_roughness: 0.3,
                    double_sided: true,
                    cull_mode: None,
                    ..default()
                };
                let red_mat = standard.add(vane(red));
                let green_mat = standard.add(vane(green));
                // Vanes sit just inside the glass so its refraction never clips them.
                let inner = 0.97 * r;
                marble.with_children(|c| {
                    // Clear glass casts almost no shadow; as an opaque caster it would also
                    // shadow its own vanes.
                    c.spawn((Mesh3d(sphere), MeshMaterial3d(glass), NotShadowCaster));
                    c.spawn((
                        Mesh3d(meshes.add(vane_mesh(&[0, 2], inner))),
                        MeshMaterial3d(red_mat),
                    ));
                    c.spawn((
                        Mesh3d(meshes.add(vane_mesh(&[1, 3], inner))),
                        MeshMaterial3d(green_mat),
                    ));
                });
            } else {
                let mat = vane_glass.add(VaneGlassMaterial {
                    base: StandardMaterial {
                        base_color: Color::WHITE,
                        perceptual_roughness: 0.04,
                        reflectance: 0.5,
                        ..default()
                    },
                    extension: VaneGlassExt {
                        params: VaneParams {
                            radius: r,
                            height: VANE_HEIGHT * 0.97,
                            width: VANE_WIDTH * 0.97,
                            twist: VANE_TWIST,
                            curl: VANE_CURL,
                            steps: fallback_steps(*quality),
                            vane_alpha: 0.92,
                            blades: VANE_BLADES as f32,
                            body: Vec4::new(0.05, 0.065, 0.07, 1.0),
                        },
                        vane_a: red,
                        vane_b: green,
                    },
                });
                marble.with_children(|c| {
                    c.spawn((Mesh3d(sphere), MeshMaterial3d(mat)));
                });
            }
        }
        MarbleKind::Glazed => {
            let mat = standard.add(StandardMaterial {
                base_color: Color::srgb(0.10, 0.28, 0.70),
                perceptual_roughness: 0.45,
                reflectance: 0.5,
                clearcoat: 1.0,
                clearcoat_perceptual_roughness: 0.04,
                ..default()
            });
            marble.with_children(|c| {
                c.spawn((Mesh3d(sphere), MeshMaterial3d(mat)));
            });
        }
        MarbleKind::Steel => {
            // Iron F0 (linear 0.562/0.565/0.578), Real-Time Rendering 4th ed., Table 9.2.
            let mat = standard.add(StandardMaterial {
                base_color: Color::linear_rgb(0.562, 0.565, 0.578),
                metallic: 1.0,
                perceptual_roughness: 0.06,
                ..default()
            });
            marble.with_children(|c| {
                c.spawn((Mesh3d(sphere), MeshMaterial3d(mat)));
            });
        }
    }
}

/// New 3D cameras get the procedural environment map (unless they already carry one) and, on
/// transmission tiers with a cat's eye, [`ScreenSpaceTransmission`].
fn equip_cameras(
    mut commands: Commands,
    cameras: Query<(Entity, Has<EnvironmentMapLight>), Added<Camera3d>>,
    look: Res<MarbleLook>,
    quality: Res<Quality>,
    mut images: ResMut<Assets<Image>>,
    mut env_maps: Local<Option<(Handle<Image>, Handle<Image>)>>,
) {
    for (entity, has_env) in &cameras {
        let mut cam = commands.entity(entity);
        if !has_env {
            let (diffuse, specular) = env_maps
                .get_or_insert_with(|| {
                    (
                        images.add(env::diffuse_map()),
                        images.add(env::specular_map()),
                    )
                })
                .clone();
            cam.insert(EnvironmentMapLight {
                diffuse_map: diffuse,
                specular_map: specular,
                intensity: ENV_INTENSITY,
                ..default()
            });
        }
        if look.0 == MarbleKind::CatsEye && uses_transmission(*quality) {
            cam.insert(ScreenSpaceTransmission::default());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanes_stay_inside_the_marble() {
        let r = scene::standard_marble_radius_m();
        let mesh = vane_mesh(&[0, 1, 2, 3], 0.97 * r);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(ps)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("no positions");
        };
        for p in ps {
            assert!(Vec3::from_array(*p).length() < r, "{p:?}");
        }
    }

    #[test]
    fn tiers_split_between_transmission_and_the_fallback() {
        assert!(!uses_transmission(Quality::Low));
        assert!(!uses_transmission(Quality::Medium));
        assert!(uses_transmission(Quality::High));
        assert!(uses_transmission(Quality::Ultra));
        assert!(fallback_steps(Quality::Low) < fallback_steps(Quality::Medium));
    }

    #[test]
    fn embedded_vane_textures_decode() {
        for png in [VANE_RED_PNG, VANE_GREEN_PNG] {
            let img = vane_image(png);
            assert_eq!(img.width(), 256);
            assert_eq!(img.height(), 256);
        }
    }
}
