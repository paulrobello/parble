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
    PortLevel,
};
use serde::Deserialize;

/// Sidecar schema version this loader reads (contract §11).
pub const SIDECAR_SCHEMA: u32 = 1;

/// Sidecars shipped in `assets/kit/`, in id order.
const BUILTIN_SIDECARS: [&str; 4] = [
    include_str!("../../../assets/kit/blk_curve90_l3/blk_curve90_l3.kit.json"),
    include_str!("../../../assets/kit/blk_end_l3/blk_end_l3.kit.json"),
    include_str!("../../../assets/kit/blk_slope_gentle_l3/blk_slope_gentle_l3.kit.json"),
    include_str!("../../../assets/kit/blk_straight_l3/blk_straight_l3.kit.json"),
];

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
    /// Trigger volumes, kept raw until mechanisms land (P2).
    pub sensors: Vec<serde_json::Value>,
    /// Kinematic parts, kept raw until mechanisms land (P2).
    pub animated_parts: Vec<serde_json::Value>,
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
    sensors: Vec<serde_json::Value>,
    animated_parts: Vec<serde_json::Value>,
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
        id,
        chiral: raw.chiral,
        footprint: raw.footprint,
        ports,
    };
    def.validate().map_err(KitError::Def)?;

    let mut collision = Vec::with_capacity(raw.collision.len());
    for (g, c) in raw.collision.into_iter().enumerate() {
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

    Ok(KitPiece {
        def,
        family: raw.family,
        piece_version: raw.piece_version,
        collision,
        sensors: raw.sensors,
        animated_parts: raw.animated_parts,
    })
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
    fn builtin_loads_every_shipped_sidecar() {
        let kit = builtin();
        let ids: Vec<&str> = kit.iter().map(|p| p.def.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "blk_curve90_l3",
                "blk_end_l3",
                "blk_slope_gentle_l3",
                "blk_straight_l3"
            ]
        );
        for p in kit.iter() {
            assert_eq!(p.collision.len(), 1, "{}", p.def.id);
            assert_eq!(p.collision[0].surface, Surface::Wood);
            assert!(!p.collision[0].triangles.is_empty());
        }
        assert!(kit.get(&id("blk_curve90_l3")).unwrap().chiral);
        assert!(!kit.get(&id("blk_straight_l3")).unwrap().chiral);
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
    fn refuses_bad_sidecars() {
        let good = BUILTIN_SIDECARS[3];
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
}
