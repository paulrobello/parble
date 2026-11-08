//! parble-render: Bevy rendering of sim state. Reads [`MarbleFrame`] (written by the app after
//! each fixed-step advance) and never touches the Rapier world.
//!
//! P0 scene: the P0-T3 ramp tray built from [`parble_sim::scene::tray_mesh`], one standard marble,
//! a directional light and a floor. The tray triangles face into the tray, so the tray material is
//! double-sided with culling off.

use bevy::{
    asset::RenderAssetUsages,
    light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap},
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    transform::TransformSystems,
};
use parble_sim::{MarbleState, scene};

pub mod camera;
pub mod post;

/// Backdrop behind the scene (a dim warm room tone).
pub const BACKDROP: Color = Color::srgb(0.11, 0.10, 0.09);

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
    commands.spawn((Mesh3d(meshes.add(tray_render_mesh())), MeshMaterial3d(wood)));

    let floor = materials.add(StandardMaterial {
        base_color: Color::srgb(0.30, 0.24, 0.20),
        perceptual_roughness: 0.9,
        ..default()
    });
    let len = scene::tray_length_m();
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(4.0 * len, 4.0 * len))),
        MeshMaterial3d(floor),
        Transform::from_xyz(len / 2.0, -0.0005, 0.0),
    ));

    let glass = materials.add(StandardMaterial {
        base_color: Color::srgb(0.20, 0.45, 0.85),
        perceptual_roughness: 0.08,
        reflectance: 0.6,
        ..default()
    });
    let marble = meshes.add(
        Sphere::new(scene::standard_marble_radius_m())
            .mesh()
            .uv(32, 18),
    );
    let spawn = scene::marble_spawn_m();
    commands.spawn((
        Mesh3d(marble),
        MeshMaterial3d(glass),
        Transform::from_xyz(spawn.x, spawn.y, spawn.z),
        MarbleVisual(0),
    ));

    // Shadow cascades and biases sized to a sub-metre scene (Bevy's defaults assume tens of metres:
    // a 0.02 m depth bias is wider than the marble's radius).
    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
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
    if matches!(*quality, Quality::High | Quality::Ultra) {
        commands.insert_resource(DirectionalLightShadowMap { size: 4096 });
    }
    commands.insert_resource(GlobalAmbientLight {
        brightness: 400.0,
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

fn sync_marbles(frame: Res<MarbleFrame>, mut marbles: Query<(&MarbleVisual, &mut Transform)>) {
    for (MarbleVisual(i), mut transform) in &mut marbles {
        let Some(curr) = frame.curr.get(*i) else {
            continue;
        };
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
