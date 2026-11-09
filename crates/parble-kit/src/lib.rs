//! parble-kit: the kit piece catalogue, loaded from the Blender pipeline's `.kit.json` sidecars
//! (kit contract §11). No Bevy, no Rapier.
//!
//! The sidecars are embedded with `include_str!`, so the same catalogue loads natively and in
//! wasm (no filesystem in the browser). [`builtin`] parses them once; [`parse_sidecar`] loads any
//! other sidecar text.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use parble_core::contract::KIT_CONTRACT_VERSION;
use parble_core::{
    CellCoord, PieceCatalog, PieceDef, PieceDefError, PieceId, Port, PortClass, PortFace, PortFlow,
    PortLevel, Route,
};
use serde::Deserialize;

/// Sidecar schema version this loader reads (contract §11).
pub const SIDECAR_SCHEMA: u32 = 1;

/// Calls `$m!` with the id of every piece shipped in `assets/kit/`, in id order, as bare
/// identifiers: the one list of builtin pieces. Each id names `assets/kit/<id>/<id>.*`, so a
/// caller can embed any file of every piece (this crate embeds the sidecars, `parble-render` the
/// GLBs) with `concat!` and `stringify!` in its own macro.
#[macro_export]
macro_rules! builtin_piece_ids {
    ($m:ident) => {
        $m!(
            blk_catch_l1,
            blk_curve90_l2,
            blk_curve90_l3,
            blk_drop_l2,
            blk_drop_l3,
            blk_end_l3,
            blk_funnel_3x3_l2,
            blk_join_l2,
            blk_join_l3,
            blk_pillar,
            blk_redirect_l2,
            blk_redirect_l3,
            blk_slope_gentle_l2,
            blk_slope_gentle_l3,
            blk_slope_steep_l3,
            blk_straight_l1,
            blk_straight_l2,
            blk_straight_l3,
            blk_tower_socket_l1,
            mech_arch_rope_l1,
            mech_checkpoint_l1,
            mech_finish,
            mech_gate_oneway_l1,
            mech_hole_oneway_l1,
            mech_hopper,
            mech_splitter_l3,
            mech_stepper_up1_l1,
            mech_stepper_up1_l3,
            mech_toggle_l3,
            rail_curve90_r90_l2,
            rail_joint_l1,
            rail_s_1x3_l2,
            rail_straight_l1,
            rail_straight_l2
        )
    };
}

macro_rules! sidecars {
    ($($id:ident),* $(,)?) => {
        [$(include_str!(concat!(
            "../../../assets/kit/",
            stringify!($id),
            "/",
            stringify!($id),
            ".kit.json"
        ))),*]
    };
}

/// Sidecars shipped in `assets/kit/`, in id order.
const BUILTIN_SIDECARS: [&str; 34] = builtin_piece_ids!(sidecars);

/// Pieces that carry no marble path of their own ([`PieceDef::structural`]): supports. The
/// sidecar schema has no field for it, so the catalogue sets it by id.
const STRUCTURAL: &[&str] = &["blk_pillar"];

/// Paths through pieces whose ports do not all connect ([`PieceDef::routes`]), as `(from, to)`
/// port indices; a piece not listed routes every inflow port to every outflow port. The one-way
/// hole: a marble from `+X` (port 1) rolls over the shut door to `-X` (port 0); one from `-X`
/// opens the door and falls out of the bottom (port 2).
const ROUTES: &[(&str, &[(usize, usize)])] = &[("mech_hole_oneway_l1", &[(1, 0), (0, 2)])];

/// [`BUILTIN_SIDECARS`] index of `blk_straight_l3`, the tests' reference sidecar.
#[cfg(test)]
const STRAIGHT_L3: usize = 17;

/// Contact surface of a collision group. Feeds `RollSample` and `ContactEvent` in P2-T1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    Wood,
    Rail,
    WoodEndGrain,
}

/// One static collision group in the piece-local frame, lengths in integer µm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollisionGroup {
    pub surface: Surface,
    pub vertices_um: Vec<[i32; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

/// A loaded kit piece: its grid definition plus the static collision mesh.
#[derive(Debug, Clone, PartialEq)]
pub struct KitPiece {
    pub def: PieceDef,
    pub family: String,
    pub piece_version: u32,
    pub collision: Vec<CollisionGroup>,
    /// Trigger volumes for mechanisms, in sidecar order.
    pub sensors: Vec<SensorDef>,
    /// Kinematic parts, in sidecar order.
    pub animated_parts: Vec<AnimatedPart>,
    /// The rail end that may leave the footprint into a socket (contract §5.5, §9 rule 3).
    pub spigot: Option<Cylinder>,
    /// The socket volume a spigot enters (contract §5.5).
    pub receptacle: Option<Cylinder>,
}

impl KitPiece {
    /// The surface of the static collision: the first group's, or wood for a piece without one.
    /// Every builtin piece's static groups share one surface (a rail is all rail, a block or a
    /// mechanism's frame all wood), so this is the surface of any static contact on the piece.
    pub fn static_surface(&self) -> Surface {
        self.collision.first().map_or(Surface::Wood, |g| g.surface)
    }
}

/// A cylinder volume in the piece-local frame (contract §11 `spigot` / `receptacle`): the centre of
/// its base, an axis-aligned unit axis, and its length and radius in integer µm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cylinder {
    pub axis: [i32; 3],
    pub center_um: [i32; 3],
    pub length_um: i32,
    pub radius_um: i32,
}

/// A mechanism trigger volume in the piece-local frame (contract §11 `sensors`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SensorDef {
    pub id: String,
    pub shape: SensorShape,
}

/// Trigger volume shape, lengths in integer µm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorShape {
    Box {
        center_um: [i32; 3],
        half_extents_um: [i32; 3],
    },
    Sphere {
        center_um: [i32; 3],
        radius_um: i32,
    },
}

/// How a part moves (contract §11 `joint`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Joint {
    /// Turns about `axis` through `pivot_um`; positions are millidegrees (sidecar `rest_mdeg`,
    /// `limits_mdeg`).
    #[default]
    Hinge,
    /// Slides along `axis` from `pivot_um`, unrotated; positions are µm (sidecar `rest_um`,
    /// `limits_um`).
    Slide,
}

/// One kinematic part (contract §11 `animated_parts`): a hinge about `axis` through `pivot_um`,
/// or a slide along `axis` ([`Joint`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnimatedPart {
    /// GLB node name, `anim_<name>`.
    pub name: String,
    pub joint: Joint,
    pub pivot_um: [i32; 3],
    /// Axis-aligned integer unit vector.
    pub axis: [i32; 3],
    /// Rest position in the joint's unit: millidegrees for a hinge, µm for a slide.
    pub rest_mdeg: i32,
    /// `[min, max]` in the joint's unit, with `rest_mdeg` inside.
    pub limits_mdeg: [i32; 2],
    /// The part's collision in the piece-local frame at position 0.
    pub collision: Vec<CollisionGroup>,
    /// Forward slant of the part's top faces, millidegrees (the stepper's slats, sidecar
    /// `slant_mdeg`); `None` for a part without one.
    pub slant_mdeg: Option<i32>,
}

/// Why a sidecar was refused.
#[derive(Debug)]
pub enum KitError {
    Json(serde_json::Error),
    Schema(u32),
    KitContract(u32),
    BadId(String),
    FamilyMismatch {
        id: String,
        family: String,
    },
    /// Port at this list position carries a different `index`.
    PortIndex {
        position: usize,
        index: u32,
    },
    /// Port at this position has a `kind` that disagrees with its face or level.
    PortKind(usize),
    BadLevel(usize),
    Def(PieceDefError),
    /// Collision group with a coordinate or index list that is not a whole number of triples.
    Ragged(usize),
    /// Collision group with an index past its vertex list.
    IndexOutOfRange(usize),
    Duplicate(String),
    /// Sensor at this position has extents that do not fit its shape, an empty id, or an id an
    /// earlier sensor already uses.
    BadSensor(usize),
    /// Animated part at this position has a non-unit axis, limits that do not hold its rest
    /// angle, a name without the `anim_` prefix, or a name an earlier part already uses. Errors in
    /// its collision groups use [`KitError::Ragged`] and [`KitError::IndexOutOfRange`].
    BadPart(usize),
    /// A `spigot` or `receptacle` volume with a non-unit axis or a non-positive length or radius.
    BadCylinder,
}

impl core::fmt::Display for KitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for KitError {}

impl From<serde_json::Error> for KitError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSidecar {
    schema: u32,
    kit_contract: u32,
    id: String,
    piece_version: u32,
    family: String,
    chiral: bool,
    footprint: Vec<CellCoord>,
    ports: Vec<RawPort>,
    collision: Vec<RawCollision>,
    sensors: Vec<RawSensor>,
    animated_parts: Vec<RawPart>,
    #[serde(default)]
    spigot: Option<Cylinder>,
    #[serde(default)]
    receptacle: Option<Cylinder>,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum RawShape {
    Box,
    Sphere,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSensor {
    id: String,
    shape: RawShape,
    center_um: [i32; 3],
    #[serde(default)]
    half_extents_um: Option<[i32; 3]>,
    #[serde(default)]
    radius_um: Option<i32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPart {
    name: String,
    #[serde(default)]
    joint: Joint,
    pivot_um: [i32; 3],
    axis: [i32; 3],
    #[serde(default)]
    rest_mdeg: Option<i32>,
    #[serde(default)]
    limits_mdeg: Option<[i32; 2]>,
    #[serde(default)]
    rest_um: Option<i32>,
    #[serde(default)]
    limits_um: Option<[i32; 2]>,
    #[serde(default)]
    slant_mdeg: Option<i32>,
    collision: Vec<RawCollision>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPort {
    cell: CellCoord,
    class: PortClass,
    face: PortFace,
    flow: PortFlow,
    index: u32,
    inset_um: i32,
    kind: RawKind,
    #[serde(default)]
    level: Option<u8>,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum RawKind {
    Side,
    Drop,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCollision {
    surface: Surface,
    vertices_um: Vec<i32>,
    indices: Vec<u32>,
}

/// Parse and check one `.kit.json` sidecar.
pub fn parse_sidecar(text: &str) -> Result<KitPiece, KitError> {
    let raw: RawSidecar = serde_json::from_str(text)?;
    if raw.schema != SIDECAR_SCHEMA {
        return Err(KitError::Schema(raw.schema));
    }
    if raw.kit_contract != KIT_CONTRACT_VERSION {
        return Err(KitError::KitContract(raw.kit_contract));
    }
    let id = PieceId::new(&raw.id).map_err(|_| KitError::BadId(raw.id.clone()))?;
    if id.family() != raw.family {
        return Err(KitError::FamilyMismatch {
            id: raw.id,
            family: raw.family,
        });
    }

    let mut ports = Vec::with_capacity(raw.ports.len());
    for (position, p) in raw.ports.into_iter().enumerate() {
        if usize::try_from(p.index).ok() != Some(position) {
            return Err(KitError::PortIndex {
                position,
                index: p.index,
            });
        }
        let side = p.kind == RawKind::Side;
        if side != p.face.is_side() || side != p.level.is_some() {
            return Err(KitError::PortKind(position));
        }
        let level = p
            .level
            .map(PortLevel::try_from)
            .transpose()
            .map_err(|_| KitError::BadLevel(position))?;
        ports.push(Port {
            cell: p.cell,
            face: p.face,
            level,
            class: p.class,
            flow: p.flow,
            inset_um: p.inset_um,
        });
    }
    let def = PieceDef {
        structural: STRUCTURAL.contains(&id.as_str()),
        routes: ROUTES
            .iter()
            .filter(|(pid, _)| *pid == id.as_str())
            .flat_map(|(_, routes)| routes.iter().map(|&(from, to)| Route { from, to }))
            .collect(),
        id,
        chiral: raw.chiral,
        footprint: raw.footprint,
        ports,
    };
    def.validate().map_err(KitError::Def)?;

    let collision = collision_groups(raw.collision)?;
    let sensors = sensors(raw.sensors)?;
    let animated_parts = animated_parts(raw.animated_parts)?;
    let well_formed = |c: &Cylinder| {
        c.axis.iter().map(|a| a.abs()).sum::<i32>() == 1 && c.length_um > 0 && c.radius_um > 0
    };
    if raw
        .spigot
        .iter()
        .chain(&raw.receptacle)
        .any(|c| !well_formed(c))
    {
        return Err(KitError::BadCylinder);
    }

    Ok(KitPiece {
        def,
        family: raw.family,
        piece_version: raw.piece_version,
        collision,
        sensors,
        animated_parts,
        spigot: raw.spigot,
        receptacle: raw.receptacle,
    })
}

fn collision_groups(raw: Vec<RawCollision>) -> Result<Vec<CollisionGroup>, KitError> {
    let mut collision = Vec::with_capacity(raw.len());
    for (g, c) in raw.into_iter().enumerate() {
        if !c.vertices_um.len().is_multiple_of(3) || !c.indices.len().is_multiple_of(3) {
            return Err(KitError::Ragged(g));
        }
        let vertices_um: Vec<[i32; 3]> = c.vertices_um.as_chunks::<3>().0.to_vec();
        let n = vertices_um.len();
        if c.indices.iter().any(|&i| i as usize >= n) {
            return Err(KitError::IndexOutOfRange(g));
        }
        let triangles = c.indices.as_chunks::<3>().0.to_vec();
        collision.push(CollisionGroup {
            surface: c.surface,
            vertices_um,
            triangles,
        });
    }
    Ok(collision)
}

fn sensors(raw: Vec<RawSensor>) -> Result<Vec<SensorDef>, KitError> {
    let mut out: Vec<SensorDef> = Vec::with_capacity(raw.len());
    for (i, s) in raw.into_iter().enumerate() {
        let shape = match (s.shape, s.half_extents_um, s.radius_um) {
            (RawShape::Box, Some(h), None) if h.iter().all(|&e| e > 0) => SensorShape::Box {
                center_um: s.center_um,
                half_extents_um: h,
            },
            (RawShape::Sphere, None, Some(r)) if r > 0 => SensorShape::Sphere {
                center_um: s.center_um,
                radius_um: r,
            },
            _ => return Err(KitError::BadSensor(i)),
        };
        if s.id.is_empty() || out.iter().any(|o| o.id == s.id) {
            return Err(KitError::BadSensor(i));
        }
        out.push(SensorDef { id: s.id, shape });
    }
    Ok(out)
}

fn animated_parts(raw: Vec<RawPart>) -> Result<Vec<AnimatedPart>, KitError> {
    let mut out: Vec<AnimatedPart> = Vec::with_capacity(raw.len());
    for (i, p) in raw.into_iter().enumerate() {
        let unit = p.axis.iter().map(|a| a.abs()).sum::<i32>() == 1;
        // A hinge carries only the mdeg fields, a slide only the µm ones.
        let (rest, limits) = match (p.joint, p.rest_mdeg, p.limits_mdeg, p.rest_um, p.limits_um) {
            (Joint::Hinge, Some(r), Some(l), None, None)
            | (Joint::Slide, None, None, Some(r), Some(l)) => (r, l),
            _ => return Err(KitError::BadPart(i)),
        };
        let [lo, hi] = limits;
        if !unit
            || !(lo <= rest && rest <= hi)
            || !p.name.starts_with("anim_")
            || out.iter().any(|o| o.name == p.name)
        {
            return Err(KitError::BadPart(i));
        }
        out.push(AnimatedPart {
            name: p.name,
            joint: p.joint,
            pivot_um: p.pivot_um,
            axis: p.axis,
            rest_mdeg: rest,
            limits_mdeg: limits,
            collision: collision_groups(p.collision)?,
            slant_mdeg: p.slant_mdeg,
        });
    }
    Ok(out)
}

/// Kit pieces by id, iterated in id order.
#[derive(Debug, Clone, Default)]
pub struct KitCatalog {
    pieces: BTreeMap<PieceId, KitPiece>,
}

impl KitCatalog {
    /// A catalogue from sidecar texts. Refuses a duplicate id.
    pub fn from_sidecars<'a>(texts: impl IntoIterator<Item = &'a str>) -> Result<Self, KitError> {
        let mut pieces = BTreeMap::new();
        for text in texts {
            let piece = parse_sidecar(text)?;
            let id = piece.def.id.clone();
            if pieces.insert(id.clone(), piece).is_some() {
                return Err(KitError::Duplicate(id.to_string()));
            }
        }
        Ok(Self { pieces })
    }

    pub fn piece(&self, id: &PieceId) -> Option<&KitPiece> {
        self.pieces.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &KitPiece> {
        self.pieces.values()
    }

    pub fn len(&self) -> usize {
        self.pieces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }
}

impl PieceCatalog for KitCatalog {
    fn get(&self, id: &PieceId) -> Option<&PieceDef> {
        self.pieces.get(id).map(|p| &p.def)
    }
}

/// The catalogue of the sidecars shipped in `assets/kit/`, parsed on first use.
pub fn builtin() -> &'static KitCatalog {
    static CATALOG: OnceLock<KitCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        KitCatalog::from_sidecars(BUILTIN_SIDECARS).expect("shipped kit sidecars are valid")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use parble_core::contract::CANONICAL_RING_UM;

    fn id(s: &str) -> PieceId {
        PieceId::new(s).unwrap()
    }

    #[test]
    fn every_builtin_piece_has_one_static_surface() {
        // `static_surface` (the sim's contact surface lookup) relies on this.
        for p in builtin().iter() {
            let s = p.static_surface();
            assert!(
                p.collision.iter().all(|g| g.surface == s),
                "{} mixes static surfaces",
                p.def.id.as_str()
            );
            assert_eq!(
                s == Surface::Rail,
                p.def.id.as_str().starts_with("rail_"),
                "{}",
                p.def.id.as_str()
            );
        }
    }

    #[test]
    fn builtin_loads_every_shipped_sidecar() {
        let kit = builtin();
        let ids: Vec<&str> = kit.iter().map(|p| p.def.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "blk_catch_l1",
                "blk_curve90_l2",
                "blk_curve90_l3",
                "blk_drop_l2",
                "blk_drop_l3",
                "blk_end_l3",
                "blk_funnel_3x3_l2",
                "blk_join_l2",
                "blk_join_l3",
                "blk_pillar",
                "blk_redirect_l2",
                "blk_redirect_l3",
                "blk_slope_gentle_l2",
                "blk_slope_gentle_l3",
                "blk_slope_steep_l3",
                "blk_straight_l1",
                "blk_straight_l2",
                "blk_straight_l3",
                "blk_tower_socket_l1",
                "mech_arch_rope_l1",
                "mech_checkpoint_l1",
                "mech_finish",
                "mech_gate_oneway_l1",
                "mech_hole_oneway_l1",
                "mech_hopper",
                "mech_splitter_l3",
                "mech_stepper_up1_l1",
                "mech_stepper_up1_l3",
                "mech_toggle_l3",
                "rail_curve90_r90_l2",
                "rail_joint_l1",
                "rail_s_1x3_l2",
                "rail_straight_l1",
                "rail_straight_l2"
            ]
        );
        assert!(kit.get(&id("blk_join_l3")).unwrap().chiral);
        let tower = kit.get(&id("blk_tower_socket_l1")).unwrap();
        assert_eq!(tower.ports[0].class, PortClass::Socket);
        assert_eq!(tower.ports[0].inset_um, 10_000);
        let redirect = kit.get(&id("blk_redirect_l3")).unwrap();
        assert_eq!(redirect.footprint.len(), 2);
        assert_eq!(redirect.ports[1].cell, CellCoord::new(0, -1, 0));
        for p in kit.iter() {
            assert_eq!(p.collision.len(), 1, "{}", p.def.id);
            let want = if p.family == "rail" {
                Surface::Rail
            } else {
                Surface::Wood
            };
            assert_eq!(p.collision[0].surface, want, "{}", p.def.id);
            assert!(!p.collision[0].triangles.is_empty());
        }
        assert!(kit.get(&id("blk_curve90_l3")).unwrap().chiral);
        assert!(!kit.get(&id("blk_straight_l3")).unwrap().chiral);
    }

    #[test]
    fn mechanism_pieces_carry_their_hinge_parts() {
        // P2-T3 (blender/kit/kitmech.py): part name, pivot µm, axis, rest and limits mdeg. The
        // finish cup and the checkpoint arch have no moving part.
        type Part = (&'static str, [i32; 3], [i32; 3], i32, [i32; 2]);
        let want: [(&str, &[Part]); 7] = [
            ("mech_checkpoint_l1", &[]),
            ("mech_finish", &[]),
            (
                "mech_gate_oneway_l1",
                &[("anim_flap", [0, 51_000, 0], [0, 0, 1], 0, [0, 85_000])],
            ),
            (
                "mech_hole_oneway_l1",
                &[(
                    "anim_trapdoor",
                    [16_000, 13_500, 0],
                    [0, 0, 1],
                    0,
                    [0, 90_000],
                )],
            ),
            (
                "mech_hopper",
                &[("anim_gate", [12_000, 51_000, 0], [0, 0, 1], 0, [0, 85_000])],
            ),
            (
                "mech_splitter_l3",
                &[(
                    "anim_flap",
                    [-2_000, 45_000, 0],
                    [0, 1, 0],
                    25_000,
                    [-25_000, 25_000],
                )],
            ),
            (
                "mech_toggle_l3",
                &[(
                    "anim_rocker",
                    [-2_000, 45_000, 0],
                    [0, 1, 0],
                    -25_000,
                    [-25_000, 25_000],
                )],
            ),
        ];
        for (pid, parts) in want {
            let p = builtin().piece(&id(pid)).unwrap();
            assert_eq!(p.family, "mech");
            assert!(!p.sensors.is_empty(), "{pid}");
            assert_eq!(p.animated_parts.len(), parts.len(), "{pid}");
            for (a, w) in p.animated_parts.iter().zip(parts) {
                assert_eq!(
                    (
                        a.name.as_str(),
                        a.pivot_um,
                        a.axis,
                        a.rest_mdeg,
                        a.limits_mdeg
                    ),
                    *w,
                    "{pid}"
                );
                assert_eq!(a.collision.len(), 1, "{pid}");
                assert_eq!(a.collision[0].surface, Surface::WoodEndGrain);
                assert!(!a.collision[0].triangles.is_empty(), "{pid}");
            }
        }
    }

    #[test]
    fn stepper_carries_one_slide_part_and_climbs_a_layer() {
        // blender/kit/kitstepper.py: the moving slats are one part sliding up +Y from rest 0.
        let p = builtin().piece(&id("mech_stepper_up1_l3")).unwrap();
        assert_eq!(p.family, "mech");
        assert_eq!(p.sensors.len(), 1);
        assert_eq!(p.animated_parts.len(), 1);
        let a = &p.animated_parts[0];
        assert_eq!(a.name, "anim_slats");
        assert_eq!(a.joint, Joint::Slide);
        assert_eq!((a.pivot_um, a.axis, a.rest_mdeg), ([0, 0, 0], [0, 1, 0], 0));
        assert_eq!(a.limits_mdeg, [0, 22_375]);
        assert_eq!(a.collision[0].surface, Surface::WoodEndGrain);
        // In at L3 of the anchor layer, out at L3 one layer up, two cells along +x.
        let (i, o) = (&p.def.ports[0], &p.def.ports[1]);
        assert_eq!(
            (i.face, i.level, i.flow),
            (PortFace::NegX, Some(PortLevel::L3), PortFlow::In)
        );
        assert_eq!(
            (o.face, o.level, o.flow),
            (PortFace::PosX, Some(PortLevel::L3), PortFlow::Out)
        );
        assert_eq!(o.cell, CellCoord::new(1, 1, 0));
        assert_eq!(p.def.footprint.len(), 4);
        for q in builtin()
            .iter()
            .filter(|q| !q.def.id.as_str().starts_with("mech_stepper_"))
        {
            assert!(q.animated_parts.iter().all(|a| a.joint == Joint::Hinge));
        }
    }

    #[test]
    fn the_l1_stepper_takes_an_l1_port_up_to_l3_one_layer_higher() {
        // The hopper's way up: in at L1 of the anchor layer, out at L3 one layer up, its slots
        // reaching one layer below the anchor.
        let p = builtin().piece(&id("mech_stepper_up1_l1")).unwrap();
        assert_eq!(p.animated_parts.len(), 1);
        let a = &p.animated_parts[0];
        assert_eq!((a.name.as_str(), a.joint), ("anim_slats", Joint::Slide));
        assert_eq!(a.limits_mdeg, [0, 29_875]);
        let (i, o) = (&p.def.ports[0], &p.def.ports[1]);
        assert_eq!(
            (i.face, i.level, i.flow, i.cell),
            (
                PortFace::NegX,
                Some(PortLevel::L1),
                PortFlow::In,
                CellCoord::new(0, 0, 0)
            )
        );
        assert_eq!(
            (o.face, o.level, o.flow, o.cell),
            (
                PortFace::PosX,
                Some(PortLevel::L3),
                PortFlow::Out,
                CellCoord::new(1, 1, 0)
            )
        );
        let mut ys: Vec<i32> = p.def.footprint.iter().map(|c| c.y).collect();
        ys.sort_unstable();
        ys.dedup();
        assert_eq!((p.def.footprint.len(), ys), (6, vec![-1, 0, 1]));
    }

    #[test]
    fn a_part_needs_the_fields_of_its_joint() {
        let base =
            include_str!("../../../assets/kit/mech_stepper_up1_l3/mech_stepper_up1_l3.kit.json");
        assert!(parse_sidecar(base).is_ok());
        for bad in [
            base.replacen("\"joint\": \"slide\",", "", 1),
            base.replacen("\"rest_um\": 0", "\"rest_mdeg\": 0", 1),
            base.replacen("\"rest_um\": 0", "\"rest_um\": 0, \"rest_mdeg\": 0", 1),
        ] {
            assert!(
                matches!(parse_sidecar(&bad), Err(KitError::BadPart(0))),
                "{bad:.80}"
            );
        }
    }

    #[test]
    fn rope_arch_hangs_two_rows_of_four_ropes() {
        // PRD P2 item 18 (blender/kit/kitmech.py): ropes hinged about +Z at the arch underside,
        // 35 mm above the L1 floor, rows at x = ±6 mm, ±45° each way.
        let p = builtin().piece(&id("mech_arch_rope_l1")).unwrap();
        assert_eq!(p.family, "mech");
        let sensors: Vec<&str> = p.sensors.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(sensors, ["curtain_nx", "curtain_px"]);
        assert_eq!(p.animated_parts.len(), 8);
        for (i, a) in p.animated_parts.iter().enumerate() {
            let x = if i < 4 { -6_000 } else { 6_000 };
            let z = [-10_500, -4_500, 4_500, 10_500][i % 4];
            assert_eq!(a.name, format!("anim_rope_{i}"));
            assert_eq!(a.pivot_um, [x, 50_000, z]);
            assert_eq!((a.axis, a.rest_mdeg), ([0, 0, 1], 0));
            assert_eq!(a.limits_mdeg, [-45_000, 45_000]);
            assert_eq!(a.collision.len(), 1);
            assert!(!a.collision[0].triangles.is_empty());
        }
    }

    #[test]
    fn every_sidecar_in_assets_is_embedded() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/kit");
        let mut on_disk: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| !n.starts_with('.'))
            .collect();
        on_disk.sort();
        let embedded: Vec<String> = builtin().iter().map(|p| p.def.id.to_string()).collect();
        assert_eq!(on_disk, embedded);
    }

    #[test]
    fn straight_port_rings_are_canonical() {
        // Contract §5.4: the collision vertices on each port plane are the canonical ring at the
        // port floor (L3 = 45 mm) with u along world Z for an X face.
        let p = builtin().piece(&id("blk_straight_l3")).unwrap();
        let verts = &p.collision[0].vertices_um;
        for x in [-30_000, 30_000] {
            for &(u, dv) in &CANONICAL_RING_UM {
                assert!(
                    verts.contains(&[x, 45_000 + dv, u]),
                    "missing ring vertex ({x}, {}, {u})",
                    45_000 + dv
                );
            }
        }
    }

    #[test]
    fn rail_pieces_carry_rail_rings_ports_and_the_joint_spigot() {
        // P2-T4 (blender/kit/kitrails.py): each port plane holds the canonical half-round with
        // the wall tops at floor + 18 mm (contract §5.3, §5.4); the joint's spigot port plane is
        // 10 mm past its +x face (§5.5), where the tower socket's channel starts.
        // (id, floor µm, per port: (plane axis 0 x / 2 z, plane µm, centre µm on the other axis,
        // spigot), footprint cells, chiral)
        type Rail = (&'static str, i32, [(usize, i32, i32, bool); 2], usize, bool);
        let want: [Rail; 5] = [
            (
                "rail_straight_l1",
                15_000,
                [(0, -30_000, 0, false), (0, 30_000, 0, false)],
                1,
                false,
            ),
            (
                "rail_straight_l2",
                30_000,
                [(0, -30_000, 0, false), (0, 30_000, 0, false)],
                1,
                false,
            ),
            (
                "rail_curve90_r90_l2",
                30_000,
                [(0, -30_000, 0, false), (2, -90_000, 60_000, false)],
                4,
                true,
            ),
            (
                "rail_s_1x3_l2",
                30_000,
                [(0, -30_000, 0, false), (0, 150_000, -60_000, false)],
                6,
                true,
            ),
            (
                "rail_joint_l1",
                15_000,
                [(0, -30_000, 0, false), (0, 40_000, 0, true)],
                1,
                false,
            ),
        ];
        for (pid, floor, ports, cells, chiral) in want {
            let p = builtin().piece(&id(pid)).unwrap();
            assert_eq!(p.family, "rail");
            assert_eq!(p.def.footprint.len(), cells, "{pid}");
            assert_eq!(p.def.chiral, chiral, "{pid}");
            let verts = &p.collision[0].vertices_um;
            for (i, &(axis, plane, centre, spigot)) in ports.iter().enumerate() {
                let port = &p.def.ports[i];
                let want_port = if spigot {
                    (PortClass::Spigot, 10_000)
                } else {
                    (PortClass::Channel, 0)
                };
                assert_eq!((port.class, port.inset_um), want_port, "{pid}");
                for &(u, dv) in &CANONICAL_RING_UM {
                    // The block ring's wall tops sit at the cell top; a rail's at floor + 18 mm.
                    let dv = if u.abs() == 14_000 && dv > 14_000 {
                        18_000
                    } else {
                        dv
                    };
                    let v = if axis == 0 {
                        [plane, floor + dv, centre + u]
                    } else {
                        [centre + u, floor + dv, plane]
                    };
                    assert!(
                        verts.contains(&v),
                        "{pid} port {i}: missing ring vertex {v:?}"
                    );
                }
            }
        }
        let joint = builtin().piece(&id("rail_joint_l1")).unwrap();
        assert_eq!(
            joint.spigot,
            Some(Cylinder {
                axis: [1, 0, 0],
                center_um: [30_000, 29_000, 0],
                length_um: 10_000,
                radius_um: 18_440
            })
        );
        assert!(builtin().iter().all(|p| p.receptacle.is_none()));
        assert!(
            builtin()
                .iter()
                .filter(|p| p.def.id.as_str() != "rail_joint_l1")
                .all(|p| p.spigot.is_none())
        );

        let mut v: serde_json::Value = serde_json::from_str(BUILTIN_SIDECARS[STRAIGHT_L3]).unwrap();
        v["spigot"] = serde_json::json!({
            "axis": [1, 1, 0], "center_um": [30000, 29000, 0], "length_um": 10000, "radius_um": 18440
        });
        assert!(matches!(
            parse_sidecar(&v.to_string()),
            Err(KitError::BadCylinder)
        ));
    }

    #[test]
    fn sensors_and_animated_parts_parse_to_typed_defs() {
        let mut v: serde_json::Value = serde_json::from_str(BUILTIN_SIDECARS[STRAIGHT_L3]).unwrap();
        v["sensors"] = serde_json::json!([
            { "center_um": [-20000, 27500, 0], "half_extents_um": [6000, 12500, 14000], "id": "approach", "shape": "box" },
            { "center_um": [0, 30000, 0], "id": "core", "radius_um": 9000, "shape": "sphere" }
        ]);
        v["animated_parts"] = serde_json::json!([{
            "axis": [0, 0, 1],
            "collision": [{
                "indices": [0, 1, 2, 0, 2, 3],
                "surface": "wood_end_grain",
                "vertices_um": [0, 17000, -14000, 0, 51000, -14000, 0, 51000, 14000, 0, 17000, 14000]
            }],
            "limits_mdeg": [0, 85000],
            "name": "anim_flap",
            "pivot_um": [0, 51000, 0],
            "rest_mdeg": 0
        }]);
        let p = parse_sidecar(&v.to_string()).unwrap();
        assert_eq!(
            p.sensors[0],
            SensorDef {
                id: "approach".into(),
                shape: SensorShape::Box {
                    center_um: [-20000, 27500, 0],
                    half_extents_um: [6000, 12500, 14000]
                }
            }
        );
        assert_eq!(
            p.sensors[1].shape,
            SensorShape::Sphere {
                center_um: [0, 30000, 0],
                radius_um: 9000
            }
        );
        let part = &p.animated_parts[0];
        assert_eq!(
            (part.name.as_str(), part.axis, part.limits_mdeg),
            ("anim_flap", [0, 0, 1], [0, 85000])
        );
        assert_eq!(part.collision[0].surface, Surface::WoodEndGrain);
        assert_eq!(part.collision[0].triangles, [[0, 1, 2], [0, 2, 3]]);

        let bad = |path: &str, val: serde_json::Value| {
            let mut w = v.clone();
            *w.pointer_mut(path).unwrap() = val;
            parse_sidecar(&w.to_string())
        };
        assert!(matches!(
            bad("/sensors/1/id", "approach".into()),
            Err(KitError::BadSensor(1))
        ));
        assert!(matches!(
            bad("/sensors/0/shape", "sphere".into()),
            Err(KitError::BadSensor(0))
        ));
        assert!(matches!(
            bad("/animated_parts/0/axis", serde_json::json!([1, 0, 1])),
            Err(KitError::BadPart(0))
        ));
        assert!(matches!(
            bad("/animated_parts/0/rest_mdeg", 90_000.into()),
            Err(KitError::BadPart(0))
        ));
        assert!(matches!(
            bad("/animated_parts/0/collision/0/indices/0", 7.into()),
            Err(KitError::IndexOutOfRange(0))
        ));
    }

    #[test]
    fn refuses_bad_sidecars() {
        let good = BUILTIN_SIDECARS[STRAIGHT_L3];
        let mut v: serde_json::Value = serde_json::from_str(good).unwrap();
        v["schema"] = 2.into();
        assert!(matches!(
            parse_sidecar(&v.to_string()),
            Err(KitError::Schema(2))
        ));

        let mut v: serde_json::Value = serde_json::from_str(good).unwrap();
        v["ports"][1]["index"] = 5.into();
        assert!(matches!(
            parse_sidecar(&v.to_string()),
            Err(KitError::PortIndex { position: 1, .. })
        ));

        let mut v: serde_json::Value = serde_json::from_str(good).unwrap();
        v["collision"][0]["indices"][0] = 9_999.into();
        assert!(matches!(
            parse_sidecar(&v.to_string()),
            Err(KitError::IndexOutOfRange(0))
        ));

        let mut v: serde_json::Value = serde_json::from_str(good).unwrap();
        v["extra"] = true.into();
        assert!(matches!(
            parse_sidecar(&v.to_string()),
            Err(KitError::Json(_))
        ));

        assert!(matches!(
            KitCatalog::from_sidecars([good, good]),
            Err(KitError::Duplicate(_))
        ));
    }

    #[test]
    fn only_the_pillar_is_structural_and_only_the_one_way_hole_has_routes() {
        let kit = builtin();
        let structural: Vec<&str> = kit
            .iter()
            .filter(|p| p.def.structural)
            .map(|p| p.def.id.as_str())
            .collect();
        assert_eq!(structural, ["blk_pillar"]);
        let routed: Vec<(&str, &[Route])> = kit
            .iter()
            .filter(|p| !p.def.routes.is_empty())
            .map(|p| (p.def.id.as_str(), p.def.routes.as_slice()))
            .collect();
        assert_eq!(
            routed,
            [(
                "mech_hole_oneway_l1",
                &[Route { from: 1, to: 0 }, Route { from: 0, to: 2 }][..]
            )]
        );
    }
}
