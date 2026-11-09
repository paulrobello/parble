//! Glow inlays (`ref-09`): thin light strips set into a mechanism's wood that light up from sim
//! state. Render-side only: the strips are not in the piece's GLB or collider, and the renderer
//! only reads the sim's [`parble_sim::inlay`] state.
//!
//! Each lit group is one [`GlowInlay`] entity: a strip mesh built from [`InlayStrip`]s in the piece
//! frame (or a part's frame, for a strip that rides a moving part), its own emissive material in
//! one of the [`InlayColor`] presets, and the anchor cell and inlay index whose state drives it.
//! The app writes every instance's levels to [`InlayFrame`] each frame ([`inlay_frame`]), and
//! [`drive_inlays`] sets the emissive from them. A level fades over [`INLAY_FADE_TICKS`] of sim
//! time from the tick the sim switched it, so a lockstep frame shows the same glow on every run.
//! With no sim instance behind it (an editor-placed piece) a group stays dark.
//!
//! Reuse: give the piece a strip table (like [`CHECKPOINT_STRIPS`]) and a branch in
//! [`piece_inlays`], have its behaviour save an [`parble_sim::inlay::Inlay`] per group, and
//! spawn [`GlowInlay`] (plus a `Transform`) as a child of the entity carrying the piece (or part)
//! transform. The mesh and material are attached on the next frame. The P7-T5 per-piece inlay
//! toggle and the stepper elevator's slats follow the same path; strips only go on mechanical
//! pieces, never on track channels (PRD §10, owner ruling 2026-11-07).

use std::collections::BTreeMap;

use bevy::{light::NotShadowCaster, prelude::*};
use parble_sim::{inlay::inlays, mechanism::MechanismView};

/// Sim ticks an inlay takes to fade fully on or off (0.25 s at 240 Hz).
pub const INLAY_FADE_TICKS: u64 = 60;

/// Emissive luminance of a fully lit strip (nits, times the preset colour). Bright enough to
/// bloom under the daylight key light of the piece showcases.
pub const INLAY_ON_NITS: f32 = 600.0;

/// The glow colour of an inlay group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InlayColor {
    /// Warm amber, the lamp light of `ref-09`.
    #[default]
    Amber,
    /// The pale blue of the cool strips in the reference photos.
    PaleBlue,
}

impl InlayColor {
    /// The lit glow colour (sRGB).
    pub fn glow(self) -> Color {
        match self {
            Self::Amber => Color::srgb(1.0, 0.70, 0.36),
            Self::PaleBlue => Color::srgb(0.62, 0.82, 1.0),
        }
    }

    /// The strip's unlit colour: a pale diffuser with a hint of its glow.
    pub fn dark(self) -> Color {
        match self {
            Self::Amber => Color::srgb(0.58, 0.51, 0.41),
            Self::PaleBlue => Color::srgb(0.52, 0.56, 0.60),
        }
    }
}

/// One strip: an axis-aligned box in the piece frame, metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InlayStrip {
    pub center: [f32; 3],
    pub half: [f32; 3],
}

const fn strip(center: [f32; 3], half: [f32; 3]) -> InlayStrip {
    InlayStrip { center, half }
}

/// Strip width and how far a strip stands proud of the wood face (half of it is sunk in).
const W: f32 = 0.0006;
const D: f32 = 0.0004;
/// The render body's faces (cell half-width minus the 0.5 mm seam inset).
const FACE: f32 = 0.0295;

/// `mech_checkpoint_l1`, inlay 0: three upright strips on each side face (±z), at x −15, 0 and
/// +15 mm. The arch faces (±x) carry none: in a track they abut the neighbouring pieces, so a strip
/// there would never be seen.
pub const CHECKPOINT_STRIPS: [InlayStrip; 6] = [
    strip([-0.015, 0.030, FACE], [W, 0.024, D]),
    strip([0.0, 0.030, FACE], [W, 0.024, D]),
    strip([0.015, 0.030, FACE], [W, 0.024, D]),
    strip([-0.015, 0.030, -FACE], [W, 0.024, D]),
    strip([0.0, 0.030, -FACE], [W, 0.024, D]),
    strip([0.015, 0.030, -FACE], [W, 0.024, D]),
];

/// The inlay groups of kit piece `id`: (inlay index, strips, colour).
pub fn piece_inlays(id: &str) -> &'static [(usize, &'static [InlayStrip], InlayColor)] {
    match id {
        "mech_checkpoint_l1" => &[(0, &CHECKPOINT_STRIPS, InlayColor::Amber)],
        _ => &[],
    }
}

/// One inlay group of the piece anchored at `cell`.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct GlowInlay {
    pub cell: IVec3,
    /// Inlay index in the behaviour's state.
    pub index: usize,
    pub strips: &'static [InlayStrip],
    pub color: InlayColor,
}

/// The level drawn last, so materials change only when a level does.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
struct Drawn {
    level: f32,
    color: InlayColor,
}

/// Inlay levels in `[0, 1]` per mechanism instance (inlay order), keyed by its anchor cell.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct InlayFrame(pub BTreeMap<[i32; 3], Vec<f32>>);

/// The level of an inlay at `tick`: a smoothstep fade over [`INLAY_FADE_TICKS`] from `since`.
pub fn inlay_level(on: bool, since: Option<u64>, tick: u64) -> f32 {
    let t = since.map_or(1.0, |s| {
        (tick.saturating_sub(s) as f32 / INLAY_FADE_TICKS as f32).clamp(0.0, 1.0)
    });
    let k = t * t * (3.0 - 2.0 * t);
    if on { k } else { 1.0 - k }
}

/// Every instance's inlay levels at `tick`, for [`InlayFrame`]. Instances without inlays are left
/// out.
pub fn inlay_frame(views: &[MechanismView], tick: u64) -> InlayFrame {
    InlayFrame(
        views
            .iter()
            .filter_map(|v| {
                let levels: Vec<f32> = inlays(&v.state)
                    .iter()
                    .map(|i| inlay_level(i.on, i.since, tick))
                    .collect();
                let c = v.id.0;
                (!levels.is_empty()).then_some(([c.x, c.y, c.z], levels))
            })
            .collect(),
    )
}

/// The strips as one flat-shaded box mesh.
pub fn strip_mesh(strips: &[InlayStrip]) -> Mesh {
    let mut out: Option<Mesh> = None;
    for s in strips {
        let h = Vec3::from_array(s.half);
        let m = Cuboid::new(2.0 * h.x, 2.0 * h.y, 2.0 * h.z)
            .mesh()
            .build()
            .translated_by(Vec3::from_array(s.center));
        match out.as_mut() {
            Some(o) => o.merge(&m).expect("cuboid meshes share attributes"),
            None => out = Some(m),
        }
    }
    out.unwrap_or_else(|| Cuboid::new(0.0, 0.0, 0.0).mesh().build())
}

/// The material of `color` at `level`.
pub fn inlay_material(color: InlayColor, level: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: color.dark().mix(&color.glow(), level),
        emissive: LinearRgba::from(color.glow()) * (INLAY_ON_NITS * level),
        perceptual_roughness: 0.6,
        ..default()
    }
}

/// Spawns the inlay groups of the piece `id` anchored at `cell` under a parent at the piece
/// transform `t`, with `visibility` and `root` on that parent (as for the animated parts, so the
/// editor's slicing hides them with the piece). No-op for a piece without inlays.
pub fn spawn_piece_inlays(
    c: &mut ChildSpawnerCommands<'_>,
    id: &str,
    cell: IVec3,
    t: Transform,
    visibility: Visibility,
    root: impl Bundle,
) {
    let groups = piece_inlays(id);
    if groups.is_empty() {
        return;
    }
    c.spawn((t, visibility, root)).with_children(|c| {
        for &(index, strips, color) in groups {
            c.spawn((
                GlowInlay {
                    cell,
                    index,
                    strips,
                    color,
                },
                Transform::default(),
                Visibility::Inherited,
            ));
        }
    });
}

pub struct InlayPlugin;

impl Plugin for InlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InlayFrame>()
            .add_systems(PostUpdate, (attach_inlays, drive_inlays).chain());
    }
}

/// Gives each new [`GlowInlay`] its strip mesh (shared per strip table) and its own material.
fn attach_inlays(
    mut commands: Commands,
    added: Query<(Entity, &GlowInlay), Without<Drawn>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: Local<Vec<(usize, Handle<Mesh>)>>,
) {
    for (e, g) in &added {
        let key = g.strips.as_ptr() as usize;
        let mesh = match cache.iter().find(|(k, _)| *k == key) {
            Some((_, h)) => h.clone(),
            None => {
                let h = meshes.add(strip_mesh(g.strips));
                cache.push((key, h.clone()));
                h
            }
        };
        let color = g.color;
        commands.entity(e).insert((
            Mesh3d(mesh),
            MeshMaterial3d(materials.add(inlay_material(color, 0.0))),
            NotShadowCaster,
            Drawn { level: 0.0, color },
        ));
    }
}

/// Sets each inlay's material from [`InlayFrame`].
fn drive_inlays(
    frame: Res<InlayFrame>,
    mut inlays: Query<(&GlowInlay, &MeshMaterial3d<StandardMaterial>, &mut Drawn)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (g, mat, mut drawn) in &mut inlays {
        let level = frame
            .0
            .get(&g.cell.to_array())
            .and_then(|l| l.get(g.index))
            .copied()
            .unwrap_or(0.0);
        if level != drawn.level
            && let Some(mut m) = materials.get_mut(&mat.0)
        {
            *m = inlay_material(drawn.color, level);
            drawn.level = level;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_level_fades_from_since_and_holds() {
        assert_eq!(inlay_level(false, None, 500), 0.0);
        assert_eq!(inlay_level(true, None, 0), 1.0);
        assert_eq!(inlay_level(true, Some(100), 100), 0.0);
        assert_eq!(
            inlay_level(true, Some(100), 100 + INLAY_FADE_TICKS / 2),
            0.5
        );
        assert_eq!(inlay_level(true, Some(100), 100 + INLAY_FADE_TICKS), 1.0);
        assert_eq!(inlay_level(true, Some(100), 10_000), 1.0);
        assert_eq!(inlay_level(false, Some(100), 100), 1.0);
        assert_eq!(inlay_level(false, Some(100), 100 + INLAY_FADE_TICKS), 0.0);
    }

    #[test]
    fn checkpoint_strips_sit_on_its_side_faces() {
        let face = 0.030;
        for s in CHECKPOINT_STRIPS {
            let c = Vec3::from_array(s.center);
            let h = Vec3::from_array(s.half);
            // On a side face (the arch faces abut the neighbours), proud of it by under a
            // millimetre, inside the cell.
            assert!((c.z.abs() - face).abs() < 0.001 && h.z < 0.001, "{s:?}");
            assert!(
                (c.x.abs() + h.x).max(c.z.abs() + h.z) <= face + 0.001,
                "{s:?}"
            );
            assert!((c - h).y >= 0.0 && (c + h).y <= 0.06, "{s:?}");
        }
        assert_eq!(piece_inlays("mech_checkpoint_l1").len(), 1);
        assert!(piece_inlays("blk_straight_l3").is_empty());
    }

    /// Kit contract §5.6: inlays only on mechanical pieces, never on track pieces.
    #[test]
    fn only_mechanical_pieces_carry_inlays() {
        for p in parble_kit::builtin().iter() {
            if !piece_inlays(p.def.id.as_str()).is_empty() {
                assert_eq!(p.family, "mech", "{}", p.def.id);
            }
        }
    }

    #[test]
    fn presets_glow_brighter_than_dark() {
        assert_eq!(InlayColor::default(), InlayColor::Amber);
        for c in [InlayColor::Amber, InlayColor::PaleBlue] {
            let lit = inlay_material(c, 1.0);
            assert!(lit.emissive.red + lit.emissive.green + lit.emissive.blue > 100.0);
            let dark = inlay_material(c, 0.0).emissive;
            assert_eq!(dark.red + dark.green + dark.blue, 0.0);
        }
    }
}
