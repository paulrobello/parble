//! Debug overlay of mechanism sensor volumes: one wireframe per sensor, drawn in world space with
//! gizmos, cyan while idle and magenta while a marble is inside. `G` toggles it; the app fills
//! [`SensorFrame`] from the sim each frame. Rendering only, so it cannot touch the sim or a hash.

use bevy::prelude::*;

/// Idle sensor colour.
pub const IDLE: Color = Color::srgb(0.1, 0.9, 1.0);
/// Occupied sensor colour.
pub const OCCUPIED: Color = Color::srgb(1.0, 0.15, 0.85);

/// One sensor volume in world metres. Boxes are axis-aligned.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SensorShape {
    Box { center: Vec3, half: Vec3 },
    Sphere { center: Vec3, radius: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SensorDraw {
    pub shape: SensorShape,
    pub occupied: bool,
}

/// The sensors to draw this frame.
#[derive(Resource, Debug, Default)]
pub struct SensorFrame(pub Vec<SensorDraw>);

/// Whether the overlay is on (`G`, `--sensors`, `?sensors=1`).
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SensorOverlay(pub bool);

#[derive(Default, Reflect, GizmoConfigGroup)]
struct SensorGizmos;

/// Registers the overlay, starting on when `on`.
pub struct SensorOverlayPlugin {
    pub on: bool,
}

impl Plugin for SensorOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SensorOverlay(self.on))
            .init_resource::<SensorFrame>()
            .init_gizmo_group::<SensorGizmos>()
            .add_systems(Startup, configure)
            .add_systems(Update, (toggle, draw).chain());
    }
}

fn configure(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<SensorGizmos>();
    // The sensors sit inside the wooden channels: draw over the wood.
    config.depth_bias = -1.0;
    config.line.width = 2.0;
}

fn toggle(keys: Option<Res<ButtonInput<KeyCode>>>, mut overlay: ResMut<SensorOverlay>) {
    if keys.is_some_and(|k| k.just_pressed(KeyCode::KeyG)) {
        overlay.0 = !overlay.0;
    }
}

fn draw(overlay: Res<SensorOverlay>, frame: Res<SensorFrame>, mut gizmos: Gizmos<SensorGizmos>) {
    if !overlay.0 {
        return;
    }
    for s in &frame.0 {
        let colour = if s.occupied { OCCUPIED } else { IDLE };
        match s.shape {
            SensorShape::Box { center, half } => {
                gizmos.cube(
                    Transform::from_translation(center).with_scale(half * 2.0),
                    colour,
                );
            }
            SensorShape::Sphere { center, radius } => {
                gizmos
                    .sphere(Isometry3d::from_translation(center), radius, colour)
                    .resolution(24);
            }
        }
    }
}
