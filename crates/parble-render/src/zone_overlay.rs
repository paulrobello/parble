//! Debug overlay of the camera director: every camera zone as a world-space wireframe (amber,
//! pale blue while the followed marble is inside, white for the zone playing) and the playing
//! shot's target (a framing point or axis, an orbit centre, a fixed look point) in magenta. `V`
//! toggles it; `--zones` and `?zones=1` start with it on. Gizmos only, so it cannot touch the sim.

use bevy::prelude::*;

use crate::camera::{CameraDiag, CameraZones};
use crate::director::{Rule, Target, Volume};

/// Zone colours: idle, marble inside, playing; and the shot target.
pub const IDLE: Color = Color::srgb(1.0, 0.68, 0.2);
pub const INSIDE: Color = Color::srgb(0.6, 0.85, 1.0);
pub const PLAYING: Color = Color::srgb(1.0, 1.0, 1.0);
pub const TARGET: Color = Color::srgb(1.0, 0.15, 0.85);

/// Whether the overlay is on (`V`, `--zones`, `?zones=1`).
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ZoneOverlay(pub bool);

#[derive(Default, Reflect, GizmoConfigGroup)]
struct ZoneGizmos;

/// Registers the overlay, starting on when `on`.
pub struct ZoneOverlayPlugin {
    pub on: bool,
}

impl Plugin for ZoneOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ZoneOverlay(self.on))
            .init_gizmo_group::<ZoneGizmos>()
            .add_systems(Startup, configure)
            .add_systems(Update, (toggle, draw).chain());
    }
}

fn configure(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<ZoneGizmos>();
    config.depth_bias = -1.0;
    config.line.width = 2.0;
}

fn toggle(keys: Option<Res<ButtonInput<KeyCode>>>, mut overlay: ResMut<ZoneOverlay>) {
    if keys.is_some_and(|k| k.just_pressed(KeyCode::KeyV)) {
        overlay.0 = !overlay.0;
    }
}

fn draw(
    overlay: Res<ZoneOverlay>,
    zones: Res<CameraZones>,
    diag: Res<CameraDiag>,
    mut gizmos: Gizmos<ZoneGizmos>,
) {
    if !overlay.0 {
        return;
    }
    let d = &diag.director;
    for (i, z) in zones.0.iter().enumerate() {
        let colour = if d.zone == Some(i) {
            PLAYING
        } else if d.inside.contains(&i) {
            INSIDE
        } else {
            IDLE
        };
        match z.volume {
            Volume::Box { min, max } => {
                let (lo, hi) = (Vec3::from_array(min), Vec3::from_array(max));
                gizmos.cube(
                    Transform::from_translation((lo + hi) * 0.5).with_scale(hi - lo),
                    colour,
                );
            }
            Volume::Cylinder {
                base,
                height,
                radius,
            } => {
                let b = Vec3::from_array(base);
                let flat = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
                for y in [0.0, height] {
                    gizmos
                        .circle(Isometry3d::new(b + Vec3::Y * y, flat), radius, colour)
                        .resolution(32);
                }
                for k in 0..4 {
                    let a = k as f32 * std::f32::consts::FRAC_PI_2;
                    let o = Vec3::new(a.cos(), 0.0, a.sin()) * radius;
                    gizmos.line(b + o, b + o + Vec3::Y * height, colour);
                }
            }
        }
    }
    let cross = |g: &mut Gizmos<ZoneGizmos>, p: Vec3| {
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            g.line(p - axis * 0.012, p + axis * 0.012, TARGET);
        }
    };
    match d.shot {
        Some(Rule::Frame { target, .. }) => match target {
            Target::Point { at } => cross(&mut gizmos, Vec3::from_array(at)),
            Target::Axis { base, top } => {
                let (b, t) = (Vec3::from_array(base), Vec3::from_array(top));
                gizmos.line(b, t, TARGET);
                cross(&mut gizmos, b);
                cross(&mut gizmos, t);
            }
            Target::Marble => {}
        },
        Some(Rule::Orbit { center, .. }) => cross(&mut gizmos, Vec3::from_array(center)),
        Some(Rule::Fixed { look, .. }) => cross(&mut gizmos, Vec3::from_array(look)),
        _ => {}
    }
}
