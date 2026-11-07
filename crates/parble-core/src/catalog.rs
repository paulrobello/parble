//! Piece definition lookup shared by the edit command model and the validation engine.

use std::collections::BTreeMap;

use crate::piece::{PieceDef, PieceId};

/// Resolves a [`PieceId`] to its [`PieceDef`].
pub trait PieceCatalog {
    fn get(&self, id: &PieceId) -> Option<&PieceDef>;
}

/// An in-memory catalog with deterministic (sorted) iteration order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MapCatalog {
    defs: BTreeMap<PieceId, PieceDef>,
}

impl MapCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a definition, replacing any previous one with the same id.
    pub fn insert(&mut self, def: PieceDef) {
        self.defs.insert(def.id.clone(), def);
    }

    pub fn iter(&self) -> impl Iterator<Item = &PieceDef> {
        self.defs.values()
    }
}

impl PieceCatalog for MapCatalog {
    fn get(&self, id: &PieceId) -> Option<&PieceDef> {
        self.defs.get(id)
    }
}

impl FromIterator<PieceDef> for MapCatalog {
    fn from_iter<I: IntoIterator<Item = PieceDef>>(iter: I) -> Self {
        let mut c = Self::new();
        for d in iter {
            c.insert(d);
        }
        c
    }
}
