//! Marble looks (P1-T6, P5-T1): one visual per [`MarbleKind`], size and quality tier.
//!
//! - Cat's eye, High and above: a clear sphere with Bevy's screen-space specular transmission
//!   (IOR 1.5, roughness 0) around opaque inner vane geometry ([`vane_mesh`]). Thickness is the
//!   radius: Bevy offsets the background lookup along the refracted ray by the thickness, and the
//!   vanes sit at the centre. The sphere casts no shadow. The camera gets
//!   [`ScreenSpaceTransmission`].
//! - Cat's eye, Low and Medium: an opaque sphere with the [`VaneGlassExt`] fallback shader
//!   (`vane_glass.wesl`), which refracts the view ray into the marble and marches the same vane
//!   surfaces analytically (16 steps on Low, 32 on Medium).
//! - Swirl: the cat's eye structure with a different vane shape ([`SWIRL_VANES`]: six wide,
//!   tightly twisted ribbons) and the swirl ribbon texture on every blade. Same two paths.
//! - Onionskin, opal and glow: a textured core sphere inside the glass ([`CoreLook`]). High and
//!   above: a transmissive shell (clear for onionskin and glow, milky for opal) around an opaque
//!   core mesh. Low and Medium: an opaque sphere with the [`CoreGlassExt`] fallback shader
//!   (`core_glass.wesl`), which refracts the view ray, intersects the core analytically and samples
//!   the same texture. Glow's core is emissive on every tier, bright enough to bloom.
//! - Glazed: a glossy ceramic body under a clearcoat.
//! - Steel: metallic, reflecting the environment map.
//!
//! Every look scales with the marble's radius (`--marble-size`). The textures are the Higgsfield
//! swatches in `assets/marbles/` (provenance in `assets-source/marbles/`), embedded in the binary. A
//! camera without an environment map gets the procedural room from [`env`] until the P1-T5 probe
//! replaces it.

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
const CORE_SHADER: &str = "embedded://parble_render/marble/core_glass.wesl";
const VANE_RED_PNG: &[u8] = include_bytes!("../../../../assets/marbles/vane-red.png");
const VANE_GREEN_PNG: &[u8] = include_bytes!("../../../../assets/marbles/vane-green.png");
const SWIRL_PNG: &[u8] = include_bytes!("../../../../assets/marbles/swirl.png");
const ONIONSKIN_PNG: &[u8] = include_bytes!("../../../../assets/marbles/onionskin.png");
const OPAL_PNG: &[u8] = include_bytes!("../../../../assets/marbles/opal.png");
const GLOW_PNG: &[u8] = include_bytes!("../../../../assets/marbles/glow.png");

/// Cat's eye vane petal shape in unit-sphere units (shared by the mesh and the fallback shader).
pub const VANE_HEIGHT: f32 = 0.78;
pub const VANE_WIDTH: f32 = 0.62;
pub const VANE_TWIST: f32 = 0.9;
pub const VANE_CURL: f32 = 0.6;
pub const VANE_BLADES: u32 = 4;
/// Environment map intensity, cd/m² per unit of the procedural radiance.
pub const ENV_INTENSITY: f32 = 3_000.0;
/// Glow core emissive gain over its texture (pre-exposure, like the playroom lanterns). Bright
/// enough that the core blooms through the post stack's bloom.
pub const GLOW_EMISSIVE: f32 = 25.0;
/// Glow emissive colour (linear): a saturated green-cyan so the hue survives the tonemapper.
pub const GLOW_TINT: Vec3 = Vec3::new(0.25, 1.0, 0.7);

/// The inner vanes of a vane marble, in unit-sphere units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VaneShape {
    /// Petal half-height and maximum radial extent.
    pub height: f32,
    pub width: f32,
    /// Twist along y and curl along ρ, radians per unit.
    pub twist: f32,
    pub curl: f32,
    pub blades: u32,
}

/// The cat's eye: four gently twisted petals.
pub const CATS_EYE_VANES: VaneShape = VaneShape {
    height: VANE_HEIGHT,
    width: VANE_WIDTH,
    twist: VANE_TWIST,
    curl: VANE_CURL,
    blades: VANE_BLADES,
};

/// The swirl: six wide ribbons twisted about one and a half turns from pole to pole.
pub const SWIRL_VANES: VaneShape = VaneShape {
    height: 0.82,
    width: 0.66,
    twist: 5.5,
    curl: 0.4,
    blades: 6,
};

/// The core-marble looks (onionskin, opal, glow).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoreLook {
    /// Core radius as a fraction of the marble radius.
    pub core: f32,
    /// Opacity of the core layer over the glass body.
    pub layer_alpha: f32,
    /// Strength of the view-angle colour shift (opal play of colour).
    pub iridescence: f32,
    /// Emissive gain over the core texture (glow).
    pub emissive: f32,
    /// Glass body tint where the view ray misses the core (linear RGB).
    pub body: Vec3,
    /// High-tier shell: perceptual roughness and diffuse transmission (milkiness).
    pub shell_roughness: f32,
    pub shell_diffuse: f32,
    pub shell_tint: Color,
    /// High-tier shell thickness as a fraction of the radius. Small, so the screen-space lookup
    /// lands on the core just under the surface instead of shrinking it.
    pub shell_thickness: f32,
}

/// Onionskin: a coloured layer just under the surface.
pub const ONIONSKIN_LOOK: CoreLook = CoreLook {
    core: 0.9,
    layer_alpha: 0.88,
    iridescence: 0.0,
    emissive: 0.0,
    body: Vec3::new(0.06, 0.06, 0.065),
    shell_roughness: 0.0,
    shell_diffuse: 0.0,
    shell_tint: Color::srgb(0.97, 0.98, 1.0),
    shell_thickness: 0.1,
};

/// Opal: milky periwinkle blue (ref-04) with a play of colour that shifts with the view angle.
pub const OPAL_LOOK: CoreLook = CoreLook {
    core: 0.93,
    layer_alpha: 0.95,
    iridescence: 0.35,
    emissive: 0.0,
    body: Vec3::new(0.30, 0.38, 0.75),
    shell_roughness: 0.08,
    shell_diffuse: 0.2,
    shell_tint: Color::srgb(0.72, 0.78, 1.0),
    shell_thickness: 0.1,
};

/// Glow: an emissive core in clear glass.
pub const GLOW_LOOK: CoreLook = CoreLook {
    core: 0.7,
    layer_alpha: 1.0,
    iridescence: 0.0,
    emissive: GLOW_EMISSIVE,
    body: Vec3::new(0.03, 0.06, 0.06),
    shell_roughness: 0.0,
    shell_diffuse: 0.0,
    shell_tint: Color::srgb(0.95, 1.0, 0.98),
    shell_thickness: 0.3,
};

/// The marbles the scene shows, in sim spawn order: type and radius (`--marble`,
/// `--marble-size`, or the lineup). Read once at startup.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct MarbleLook(pub Vec<(MarbleKind, f32)>);

impl Default for MarbleLook {
    fn default() -> Self {
        Self(vec![(
            MarbleKind::CatsEye,
            scene::standard_marble_radius_m(),
        )])
    }
}

/// True when the tier renders glass marbles with real screen-space transmission.
pub fn uses_transmission(q: Quality) -> bool {
    !matches!(q, Quality::Low | Quality::Medium)
}

/// True when the type has a clear or milky glass body (everything but glazed and steel).
pub fn is_glass(kind: MarbleKind) -> bool {
    !matches!(kind, MarbleKind::Glazed | MarbleKind::Steel)
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

#[derive(Clone, Copy, Debug, ShaderType, Reflect)]
pub struct CoreParams {
    pub radius: f32,
    pub core: f32,
    pub layer_alpha: f32,
    pub iridescence: f32,
    pub emissive: f32,
    pub body: Vec4,
    pub glow_tint: Vec4,
}

/// The fallback core glass extension (onionskin, opal, glow): core parameters and its texture.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct CoreGlassExt {
    #[uniform(100)]
    pub params: CoreParams,
    #[texture(101)]
    #[sampler(102)]
    pub core_texture: Handle<Image>,
}

impl MaterialExtension for CoreGlassExt {
    fn fragment_shader() -> ShaderRef {
        CORE_SHADER.into()
    }
}

pub type CoreGlassMaterial = ExtendedMaterial<StandardMaterial, CoreGlassExt>;

pub struct MarblePlugin;

impl Plugin for MarblePlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "vane_glass.wesl");
        embedded_asset!(app, "core_glass.wesl");
        app.add_plugins(MaterialPlugin::<VaneGlassMaterial>::default())
            .add_plugins(MaterialPlugin::<CoreGlassMaterial>::default())
            .init_resource::<MarbleLook>()
            .add_systems(Startup, spawn_marbles)
            .add_systems(Update, equip_cameras);
    }
}

/// Kept in the main world too: a renderer rebuilt after a lost GPU device uploads it again.
fn vane_image(png: &[u8]) -> Image {
    Image::from_buffer(
        png,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::linear(),
        RenderAssetUsages::default(),
    )
    .expect("embedded marble PNG decodes")
}

/// Height of the cat's eye petal's radial extent at `y` (unit-sphere units).
pub fn petal_extent(y: f32) -> f32 {
    shape_extent(&CATS_EYE_VANES, y)
}

fn shape_extent(shape: &VaneShape, y: f32) -> f32 {
    let t = (1.0 - (y / shape.height).powi(2)).clamp(0.0, 1.0);
    shape.width * t.powf(0.6)
}

/// The cat's eye inner vane surfaces for `blades` (blade indices), scaled to `radius`.
pub fn vane_mesh(blades: &[u32], radius: f32) -> Mesh {
    shaped_vane_mesh(&CATS_EYE_VANES, blades, radius)
}

/// Inner vane surfaces of `shape` for `blades` (blade indices), scaled to `radius`. UV u runs from
/// the axis to the petal edge, v from top to bottom, matching the fallback shader's lookup.
pub fn shaped_vane_mesh(shape: &VaneShape, blades: &[u32], radius: f32) -> Mesh {
    const NS: u32 = 8;
    // 32 rows (the P1-T6 cat's eye); tighter twists get more so the ribbons stay smooth.
    let ny = 32u32.max((12.0 * shape.twist).round() as u32);
    let point = |b: u32, s: f32, y: f32| {
        let rho = s * shape_extent(shape, y);
        let a = b as f32 * std::f32::consts::TAU / shape.blades as f32
            + shape.twist * y
            + shape.curl * rho;
        Vec3::new(rho * a.cos(), y, rho * a.sin()) * radius
    };
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    for &b in blades {
        let base = positions.len() as u32;
        for j in 0..=ny {
            let v = j as f32 / ny as f32;
            let y = shape.height * (1.0 - 2.0 * v);
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
        for j in 0..ny {
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

/// Everything one marble's look needs, shared across the marbles of a scene.
struct Kit<'a, 'w> {
    quality: Quality,
    meshes: &'a mut Assets<Mesh>,
    images: &'a mut Assets<Image>,
    standard: &'a mut Assets<StandardMaterial>,
    vane_glass: &'a mut Assets<VaneGlassMaterial>,
    core_glass: &'a mut Assets<CoreGlassMaterial>,
    textures: &'w mut Vec<(&'static [u8], Handle<Image>)>,
}

impl Kit<'_, '_> {
    /// One handle per embedded texture, decoded on first use.
    fn texture(&mut self, png: &'static [u8]) -> Handle<Image> {
        if let Some((_, h)) = self
            .textures
            .iter()
            .find(|(p, _)| p.as_ptr() == png.as_ptr())
        {
            return h.clone();
        }
        let h = self.images.add(vane_image(png));
        self.textures.push((png, h.clone()));
        h
    }

    /// The clear (or milky) transmissive shell of the High tier.
    fn shell(
        &mut self,
        thickness: f32,
        tint: Color,
        roughness: f32,
        diffuse: f32,
    ) -> Handle<StandardMaterial> {
        self.standard.add(StandardMaterial {
            base_color: tint,
            perceptual_roughness: roughness,
            reflectance: 0.5,
            specular_transmission: 1.0 - diffuse,
            diffuse_transmission: diffuse,
            thickness,
            ior: 1.5,
            ..default()
        })
    }
}

/// Spawn the children of one marble entity for `kind` at radius `r`.
fn spawn_look(c: &mut ChildSpawnerCommands, kind: MarbleKind, r: f32, kit: &mut Kit) {
    let sphere = kit.meshes.add(Sphere::new(r).mesh().uv(48, 24));
    let transmission = uses_transmission(kit.quality);
    match kind {
        MarbleKind::CatsEye | MarbleKind::Swirl => {
            let (shape, a, b) = if kind == MarbleKind::CatsEye {
                (
                    CATS_EYE_VANES,
                    kit.texture(VANE_RED_PNG),
                    kit.texture(VANE_GREEN_PNG),
                )
            } else {
                let t = kit.texture(SWIRL_PNG);
                (SWIRL_VANES, t.clone(), t)
            };
            if transmission {
                let glass = kit.shell(r, Color::srgb(0.96, 0.98, 1.0), 0.0, 0.0);
                let vane = |tex: Handle<Image>| StandardMaterial {
                    base_color_texture: Some(tex),
                    perceptual_roughness: 0.3,
                    double_sided: true,
                    cull_mode: None,
                    ..default()
                };
                let mat_a = kit.standard.add(vane(a));
                let mat_b = kit.standard.add(vane(b));
                // Vanes sit just inside the glass so its refraction never clips them.
                let inner = 0.97 * r;
                let even: Vec<u32> = (0..shape.blades).step_by(2).collect();
                let odd: Vec<u32> = (1..shape.blades).step_by(2).collect();
                // Clear glass casts almost no shadow; as an opaque caster it would also shadow
                // its own vanes.
                c.spawn((Mesh3d(sphere), MeshMaterial3d(glass), NotShadowCaster));
                c.spawn((
                    Mesh3d(kit.meshes.add(shaped_vane_mesh(&shape, &even, inner))),
                    MeshMaterial3d(mat_a),
                ));
                c.spawn((
                    Mesh3d(kit.meshes.add(shaped_vane_mesh(&shape, &odd, inner))),
                    MeshMaterial3d(mat_b),
                ));
            } else {
                let mat = kit.vane_glass.add(VaneGlassMaterial {
                    base: StandardMaterial {
                        base_color: Color::WHITE,
                        perceptual_roughness: 0.04,
                        reflectance: 0.5,
                        ..default()
                    },
                    extension: VaneGlassExt {
                        params: VaneParams {
                            radius: r,
                            height: shape.height * 0.97,
                            width: shape.width * 0.97,
                            twist: shape.twist,
                            curl: shape.curl,
                            steps: fallback_steps(kit.quality),
                            vane_alpha: 0.92,
                            blades: shape.blades as f32,
                            body: Vec4::new(0.05, 0.065, 0.07, 1.0),
                        },
                        vane_a: a,
                        vane_b: b,
                    },
                });
                c.spawn((Mesh3d(sphere), MeshMaterial3d(mat)));
            }
        }
        MarbleKind::Onionskin | MarbleKind::Opal | MarbleKind::Glow => {
            let (look, png) = match kind {
                MarbleKind::Onionskin => (ONIONSKIN_LOOK, ONIONSKIN_PNG),
                MarbleKind::Opal => (OPAL_LOOK, OPAL_PNG),
                _ => (GLOW_LOOK, GLOW_PNG),
            };
            let tex = kit.texture(png);
            let emissive = LinearRgba::rgb(GLOW_TINT.x, GLOW_TINT.y, GLOW_TINT.z) * look.emissive;
            if transmission {
                let glass = kit.shell(
                    look.shell_thickness * r,
                    look.shell_tint,
                    look.shell_roughness,
                    look.shell_diffuse,
                );
                let core = kit.standard.add(StandardMaterial {
                    base_color_texture: Some(tex.clone()),
                    perceptual_roughness: 0.35,
                    emissive,
                    emissive_texture: (look.emissive > 0.0).then_some(tex),
                    ..default()
                });
                let core_mesh = kit.meshes.add(Sphere::new(look.core * r).mesh().uv(48, 24));
                c.spawn((Mesh3d(sphere), MeshMaterial3d(glass), NotShadowCaster));
                c.spawn((Mesh3d(core_mesh), MeshMaterial3d(core)));
            } else {
                let mat = kit.core_glass.add(CoreGlassMaterial {
                    base: StandardMaterial {
                        base_color: Color::WHITE,
                        perceptual_roughness: if kind == MarbleKind::Opal { 0.12 } else { 0.04 },
                        reflectance: 0.5,
                        ..default()
                    },
                    extension: CoreGlassExt {
                        params: CoreParams {
                            radius: r,
                            core: look.core,
                            layer_alpha: look.layer_alpha,
                            iridescence: look.iridescence,
                            emissive: look.emissive,
                            body: look.body.extend(1.0),
                            glow_tint: GLOW_TINT.extend(1.0),
                        },
                        core_texture: tex,
                    },
                });
                c.spawn((Mesh3d(sphere), MeshMaterial3d(mat)));
            }
        }
        MarbleKind::Glazed => {
            let mat = kit.standard.add(StandardMaterial {
                base_color: Color::srgb(0.10, 0.28, 0.70),
                perceptual_roughness: 0.45,
                reflectance: 0.5,
                clearcoat: 1.0,
                clearcoat_perceptual_roughness: 0.04,
                ..default()
            });
            c.spawn((Mesh3d(sphere), MeshMaterial3d(mat)));
        }
        MarbleKind::Steel => {
            // Iron F0 (linear 0.562/0.565/0.578), Real-Time Rendering 4th ed., Table 9.2.
            let mat = kit.standard.add(StandardMaterial {
                base_color: Color::linear_rgb(0.562, 0.565, 0.578),
                metallic: 1.0,
                perceptual_roughness: 0.06,
                ..default()
            });
            c.spawn((Mesh3d(sphere), MeshMaterial3d(mat)));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_marbles(
    mut commands: Commands,
    look: Res<MarbleLook>,
    quality: Res<Quality>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut vane_glass: ResMut<Assets<VaneGlassMaterial>>,
    mut core_glass: ResMut<Assets<CoreGlassMaterial>>,
) {
    let mut textures = Vec::new();
    let mut kit = Kit {
        quality: *quality,
        meshes: &mut meshes,
        images: &mut images,
        standard: &mut standard,
        vane_glass: &mut vane_glass,
        core_glass: &mut core_glass,
        textures: &mut textures,
    };
    let spawn = scene::marble_spawn_m();
    for (i, &(kind, r)) in look.0.iter().enumerate() {
        commands
            .spawn((
                Transform::from_xyz(spawn.x, spawn.y, spawn.z),
                Visibility::default(),
                MarbleVisual(i),
            ))
            .with_children(|c| spawn_look(c, kind, r, &mut kit));
    }
}

/// New 3D cameras get the procedural environment map (unless they already carry one) and, on
/// transmission tiers with a glass marble in the scene, [`ScreenSpaceTransmission`].
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
        if uses_transmission(*quality) && look.0.iter().any(|(k, _)| is_glass(*k)) {
            cam.insert(ScreenSpaceTransmission::default());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parble_sim::marble::MarbleSize;

    fn positions(mesh: &Mesh) -> Vec<[f32; 3]> {
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(ps)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("no positions");
        };
        ps.clone()
    }

    #[test]
    fn vanes_stay_inside_the_marble_at_every_size() {
        for size in MarbleSize::ALL {
            let r = size.radius_m();
            for shape in [CATS_EYE_VANES, SWIRL_VANES] {
                let all: Vec<u32> = (0..shape.blades).collect();
                for p in positions(&shaped_vane_mesh(&shape, &all, 0.97 * r)) {
                    assert!(Vec3::from_array(p).length() < r, "{size:?} {shape:?} {p:?}");
                }
            }
        }
    }

    #[test]
    fn cats_eye_vane_mesh_keeps_its_p1_t6_resolution() {
        // 2 blades × 33 rows × 9 columns, as in P1-T6: the committed P1-T7 shots used it.
        let mesh = vane_mesh(&[0, 2], scene::standard_marble_radius_m());
        assert_eq!(mesh.count_vertices(), 2 * 33 * 9);
    }

    #[test]
    fn core_looks_sit_inside_the_marble() {
        for look in [ONIONSKIN_LOOK, OPAL_LOOK, GLOW_LOOK] {
            assert!(look.core > 0.0 && look.core < 1.0);
            assert!(look.layer_alpha > 0.0 && look.layer_alpha <= 1.0);
        }
        const {
            assert!(
                GLOW_LOOK.emissive > 1.0,
                "glow must be bright enough to bloom"
            );
            // ref-04's opal is blue.
            assert!(OPAL_LOOK.body.z > OPAL_LOOK.body.x && OPAL_LOOK.body.z > OPAL_LOOK.body.y);
        }
        assert_eq!(ONIONSKIN_LOOK.emissive, 0.0);
        assert_eq!(OPAL_LOOK.emissive, 0.0);
    }

    #[test]
    fn tiers_split_between_transmission_and_the_fallback() {
        assert!(!uses_transmission(Quality::Low));
        assert!(!uses_transmission(Quality::Medium));
        assert!(uses_transmission(Quality::High));
        assert!(uses_transmission(Quality::Ultra));
        assert!(fallback_steps(Quality::Low) < fallback_steps(Quality::Medium));
        let glass: Vec<_> = MarbleKind::ALL
            .into_iter()
            .filter(|k| is_glass(*k))
            .collect();
        assert_eq!(glass.len(), 5);
    }

    #[test]
    fn embedded_marble_textures_decode_at_256() {
        for png in [
            VANE_RED_PNG,
            VANE_GREEN_PNG,
            SWIRL_PNG,
            ONIONSKIN_PNG,
            OPAL_PNG,
            GLOW_PNG,
        ] {
            let img = vane_image(png);
            assert_eq!(img.width(), 256);
            assert_eq!(img.height(), 256);
        }
    }
}
