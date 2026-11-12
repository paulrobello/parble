//! parble-render: Bevy rendering of sim state. Reads [`MarbleFrame`] (written by the app after
//! each fixed-step advance) and never touches the Rapier world.
//!
//! P0 scene: the P0-T3 ramp tray built from [`parble_sim::scene::tray_mesh`], one marble (its look
//! lives in [`marble`]), a warm key light with a pale blue fill ([`palette`]) and a floor. The tray
//! triangles face into the tray, so the tray material is double-sided with culling off.

use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::NoFrustumCulling,
    light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap},
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    transform::TransformSystems,
};
use parble_sim::{MarbleState, scene};

pub mod camera;
pub mod director;
pub mod inlay;
pub mod kit;
pub mod marble;
pub mod palette;
pub mod pipelines;
pub mod playroom;
pub mod post;
pub mod present;
pub mod sensors;
pub mod zone_overlay;
pub use playroom::{Environment, PlayroomView, ShadowLights};

/// Backdrop behind the scene: a deep night blue, as behind the references' lit wood.
pub const BACKDROP: Color = Color::srgb(0.07, 0.11, 0.26);

/// Plain scenes' warm key (the only shadow caster) and its pale blue fill, lux.
pub const KEY_LUX: f32 = 13_500.0;
pub const FILL_LUX: f32 = 4_500.0;

/// Marble snapshots around the current render time: `prev` and `curr` are the sim states at the
/// last two fixed steps, `alpha` in `[0, 1)` is how far the frame lies past `curr`'s predecessor.
#[derive(Resource, Default, Debug, Clone)]
pub struct MarbleFrame {
    pub prev: Vec<MarbleState>,
    pub curr: Vec<MarbleState>,
    pub alpha: f32,
}

/// Render quality tier (`--quality`). Read once at startup. The post stack per tier is
/// [`Quality::post`] (`post.rs`).
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    /// No shadows, no AA, bloom only.
    Low,
    /// 2048² shadow map, 4× MSAA, bloom, Gaussian DoF.
    #[default]
    Medium,
    /// 4096² shadow map, TAA, SSAO, bloom, bokeh DoF.
    High,
    /// High with higher SSAO quality and a wider aperture.
    Ultra,
}

impl Quality {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "ultra" => Some(Self::Ultra),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Ultra => "ultra",
        }
    }

    pub fn msaa(self) -> Msaa {
        match self.post().anti_alias {
            post::AntiAlias::Msaa4 => Msaa::Sample4,
            post::AntiAlias::None | post::AntiAlias::Taa => Msaa::Off,
        }
    }
}

/// Marks a rendered marble; the index is its spawn order in the sim.
#[derive(Component, Debug, Clone, Copy)]
pub struct MarbleVisual(pub usize);

pub struct ParbleRenderPlugin;

impl Plugin for ParbleRenderPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(BACKDROP))
            .init_resource::<MarbleFrame>()
            .init_resource::<Quality>()
            .add_plugins(marble::MarblePlugin)
            .add_plugins(playroom::PlayroomPlugin)
            .add_plugins(kit::KitPlugin)
            .add_plugins(inlay::InlayPlugin)
            .add_plugins(pipelines::PipelineStatsPlugin)
            .add_plugins(present::PresentProbePlugin)
            .add_systems(Startup, spawn_scene)
            .add_systems(PostUpdate, sync_marbles.before(TransformSystems::Propagate));
    }
}

/// The tray trimesh with flat per-triangle normals (vertices duplicated per triangle).
pub fn tray_render_mesh() -> Mesh {
    let (vertices, tris) = scene::tray_mesh();
    let mut positions = Vec::with_capacity(tris.len() * 3);
    let mut normals = Vec::with_capacity(tris.len() * 3);
    for t in &tris {
        let [a, b, c] = t.map(|i| {
            let v = vertices[i as usize];
            Vec3::new(v.x, v.y, v.z)
        });
        let n = (b - a).cross(c - a).normalize_or_zero();
        for p in [a, b, c] {
            positions.push(p.to_array());
            normals.push(n.to_array());
        }
    }
    let count = positions.len() as u32;
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_indices(Indices::U32((0..count).collect()))
}

fn spawn_scene(
    mut commands: Commands,
    quality: Res<Quality>,
    env: Res<Environment>,
    kit_scene: Res<kit::KitScene>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let wood = materials.add(StandardMaterial {
        base_color: Color::srgb(0.62, 0.40, 0.22),
        perceptual_roughness: 0.6,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    // The playroom (P1-T5) lays its own rug and floor here, brings its own lights below, and runs
    // its kit-piece run instead of the tray.
    let plain = *env == Environment::Plain;
    // Drawn only where it is simulated: ramp, kit2000, closeup and lineup.
    if plain && matches!(*kit_scene, kit::KitScene::None | kit::KitScene::Stress2000) {
        commands.spawn((
            Name::new("tray"),
            Mesh3d(meshes.add(tray_render_mesh())),
            MeshMaterial3d(wood),
        ));
    }

    let floor = materials.add(StandardMaterial {
        base_color: Color::srgb(0.04, 0.10, 0.22),
        perceptual_roughness: 0.9,
        reflectance: 0.15,
        ..default()
    });
    let len = scene::tray_length_m();
    if plain {
        let (mut min, mut max) = (
            Vec2::new(-1.5 * len, -2.0 * len),
            Vec2::new(2.5 * len, 2.0 * len),
        );
        // The kit2000 showcase grid is far wider than the tray floor: floor under all of it.
        if *kit_scene == kit::KitScene::Stress2000 {
            let (a, b) = kit::grid_footprint_m();
            min = min.min(a - Vec2::splat(0.6));
            max = max.max(b + Vec2::splat(0.6));
        }
        if *kit_scene == kit::KitScene::Tower2000 {
            let (a, b) = kit::tower_footprint_m();
            min = min.min(a - Vec2::splat(0.6));
            max = max.max(b + Vec2::splat(0.6));
        }
        if *kit_scene == kit::KitScene::Editor {
            let (a, b) = kit::edit::EDITOR_FLOOR_M;
            min = min.min(Vec2::from_array(a));
            max = max.max(Vec2::from_array(b));
        }
        let (size, centre) = (max - min, (min + max) / 2.0);
        commands.spawn((
            Name::new("floor"),
            Mesh3d(meshes.add(Plane3d::default().mesh().size(size.x, size.y))),
            MeshMaterial3d(floor),
            Transform::from_xyz(centre.x, -0.0005, centre.y),
        ));
    }

    if !plain {
        return;
    }

    // Shadow cascades and biases sized to a sub-metre scene (Bevy's defaults assume tens of metres:
    // a 0.02 m depth bias is wider than the marble's radius).
    commands.spawn((
        DirectionalLight {
            color: palette::WARM_KEY,
            illuminance: KEY_LUX,
            shadow_maps_enabled: *quality != Quality::Low,
            shadow_depth_bias: 0.0004,
            shadow_normal_bias: 0.6,
            ..default()
        },
        // Steep and near the tray's long axis, so the channel floor is lit past the walls.
        Transform::from_xyz(len * 0.25, 1.0, 0.25)
            .looking_at(Vec3::new(len * 0.5, 0.0, 0.0), Vec3::Y),
        CascadeShadowConfigBuilder {
            num_cascades: 1,
            maximum_distance: 2.0,
            minimum_distance: 0.01,
            ..default()
        }
        .build(),
    ));
    // Pale blue fill, low and opposite the key's azimuth: cools the faces and edges the key misses.
    // No shadows, so it costs nothing against the tier's shadow budget.
    commands.spawn((
        DirectionalLight {
            color: palette::PALE_BLUE,
            illuminance: FILL_LUX,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(len * 0.5 + 1.0, 0.18, 0.6)
            .looking_at(Vec3::new(len * 0.5, 0.0, 0.0), Vec3::Y),
    ));
    if matches!(*quality, Quality::High | Quality::Ultra) {
        commands.insert_resource(DirectionalLightShadowMap { size: 4096 });
    }
    commands.insert_resource(GlobalAmbientLight {
        color: palette::PALE_BLUE,
        brightness: 500.0,
        ..default()
    });
}

fn lerp_state(a: &MarbleState, b: &MarbleState, t: f32) -> Transform {
    let pa = Vec3::from_array(a.position);
    let pb = Vec3::from_array(b.position);
    let qa = Quat::from_array(a.rotation);
    let qb = Quat::from_array(b.rotation);
    Transform::from_translation(pa.lerp(pb, t)).with_rotation(qa.slerp(qb, t))
}

/// Where the warm-up draws the marbles the sim has not released yet: far under the floor, out of
/// every camera's view, and past every light, so it shades and shadows nothing on screen.
const WARMUP_PARK_M: Vec3 = Vec3::new(0.0, -50.0, 0.0);

/// A visual with no sim marble yet (a hopper track releases its marbles over the run) is hidden,
/// except during the loading [`pipelines::Warmup`]: Bevy creates a material's pipelines only for
/// meshes some view draws, so a look first shown mid-run (the opal shell's diffuse-transmission
/// variant, released 2 s in) would compile then, on the main thread on wasm. Until the warm-up ends
/// such a visual is drawn at [`WARMUP_PARK_M`], its meshes marked [`NoFrustumCulling`] so the
/// main, prepass and every shadow view specialize them.
fn sync_marbles(
    mut commands: Commands,
    frame: Res<MarbleFrame>,
    warmup: Res<pipelines::Warmup>,
    mut warming: Local<Option<bool>>,
    mut marbles: Query<(&MarbleVisual, &mut Transform, &mut Visibility, &Children)>,
    meshes: Query<(), With<Mesh3d>>,
) {
    let warm = !warmup.done;
    if *warming != Some(warm) {
        for (.., children) in &marbles {
            for child in children.iter().filter(|c| meshes.contains(*c)) {
                if warm {
                    commands.entity(child).insert(NoFrustumCulling);
                } else {
                    commands.entity(child).remove::<NoFrustumCulling>();
                }
            }
        }
        // Wait for the marbles' children to exist before latching.
        if !marbles.is_empty() {
            *warming = Some(warm);
        }
    }
    for (MarbleVisual(i), mut transform, mut visibility, _) in &mut marbles {
        let Some(curr) = frame.curr.get(*i) else {
            if warm {
                visibility.set_if_neq(Visibility::Inherited);
                transform.set_if_neq(Transform::from_translation(WARMUP_PARK_M));
            } else {
                visibility.set_if_neq(Visibility::Hidden);
            }
            continue;
        };
        visibility.set_if_neq(Visibility::Inherited);
        let prev = frame.prev.get(*i).unwrap_or(curr);
        *transform = lerp_state(prev, curr, frame.alpha);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    #[test]
    fn tray_render_mesh_has_flat_unit_normals_per_sim_triangle() {
        let (_, tris) = scene::tray_mesh();
        let mesh = tray_render_mesh();
        assert_eq!(mesh.count_vertices(), tris.len() * 3);
        let Some(VertexAttributeValues::Float32x3(normals)) =
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            panic!("tray mesh has no Float32x3 normals");
        };
        for n in normals {
            assert!((Vec3::from_array(*n).length() - 1.0).abs() < 1e-5);
        }
    }
}
