//! Support pillars under elevated pieces.

use std::collections::BTreeSet;

use parble_core::{CellCoord, MapCatalog, Mirror, PieceCatalog, PieceDef, Placement, Yaw4};

use crate::search::Walk;

/// The catalog's support pillars as `(height in cells, piece)`, tallest first: every structural
/// piece whose footprint is one cell column hanging from its anchor (the kit's `blk_pillar` and
/// its `_h2`, `_h4` and `_h8` variants), one per height. `None` without a one-cell pillar, since
/// only that one fills a column of any height.
pub(crate) fn variants(defs: &[PieceDef]) -> Option<Vec<(usize, &PieceDef)>> {
    let mut out: Vec<(usize, &PieceDef)> = Vec::new();
    for def in defs.iter().filter(|d| d.structural) {
        let h = def.footprint.len();
        let column = (0..h as i32).all(|k| def.footprint.contains(&CellCoord::new(0, -k, 0)));
        if h > 0 && column && !out.iter().any(|(o, _)| *o == h) {
            out.push((h, def));
        }
    }
    out.sort_by_key(|&(h, _)| std::cmp::Reverse(h));
    (out.last()?.0 == 1).then_some(out)
}

/// A pillar column from under every piece cell down to layer 0 or the first occupied cell.
/// A column never enters a cell a transfer falls through. A column that would come down onto an
/// `L2` or `L3` channel is left out whole, since a pillar there stands in the channel's overhead
/// clearance (`parble_core::clearance_cells`): the piece above spans the gap unsupported.
///
/// A column is built from its foot up, each time with the tallest of `pillars` ([`variants`])
/// that fits what is left, so the tall pieces stand at the bottom and the column's cells are the
/// ones a stack of one-cell pillars would fill.
pub(crate) fn pillars(
    walk: &Walk,
    catalog: &MapCatalog,
    pillars: &[(usize, &PieceDef)],
) -> Vec<Placement> {
    let mut taken: BTreeSet<CellCoord> = walk.occupied.clone();
    let mut out = Vec::new();
    for p in &walk.placements {
        let Some(def) = catalog.get(&p.piece) else {
            continue;
        };
        for c in p.world_cells(def) {
            let mut column = Vec::new();
            let mut blocks_channel = false;
            let mut y = c.y - 1;
            while y >= 0 {
                let cell = CellCoord::new(c.x, y, c.z);
                if taken.contains(&cell) || walk.reserved.contains(&cell) {
                    break;
                }
                if walk.clear.contains(&cell) {
                    blocks_channel = true;
                    break;
                }
                column.push(cell);
                y -= 1;
            }
            if blocks_channel {
                continue;
            }
            taken.extend(column.iter().copied());
            // `column` runs top to bottom; a pillar's anchor is its top cell.
            let mut left = column.len();
            while left > 0 {
                let Some((h, pillar)) = pillars.iter().find(|(h, _)| *h <= left) else {
                    break;
                };
                out.push(Placement::new(
                    pillar.id.clone(),
                    column[left - h],
                    Yaw4::new(0),
                    Mirror::Off,
                ));
                left -= h;
            }
        }
    }
    out
}
