//! Support pillars under elevated pieces.

use std::collections::BTreeSet;

use parble_core::{CellCoord, MapCatalog, Mirror, PieceCatalog, PieceDef, Placement, Yaw4};

use crate::search::Walk;

/// A pillar column from under every piece cell down to layer 0 or the first occupied cell.
/// A column never enters a cell a transfer falls through.
pub(crate) fn pillars(walk: &Walk, catalog: &MapCatalog, pillar: &PieceDef) -> Vec<Placement> {
    let mut taken: BTreeSet<CellCoord> = walk.occupied.clone();
    let mut out = Vec::new();
    for p in &walk.placements {
        let Some(def) = catalog.get(&p.piece) else {
            continue;
        };
        for c in p.world_cells(def) {
            let mut y = c.y - 1;
            while y >= 0 {
                let cell = CellCoord::new(c.x, y, c.z);
                if taken.contains(&cell) || walk.reserved.contains(&cell) {
                    break;
                }
                taken.insert(cell);
                out.push(Placement::new(
                    pillar.id.clone(),
                    cell,
                    Yaw4::new(0),
                    Mirror::Off,
                ));
                y -= 1;
            }
        }
    }
    out
}
