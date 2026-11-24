//! A cache of one Scatter's placed copies, chunk by chunk, for queries.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use super::ScatterInput;
use super::footprint::Footprints;
use super::lattice::{ChunkGrid, ChunkId, FixedRect, to_fixed};
use super::placing::{Scatter, ScatterCopy, ScatterError};
use crate::address::CopyAddress;

/// The copies of one Scatter, placed on the CPU one chunk at a time and
/// kept for later queries.
///
/// Every call must pass the same Scatter and the same input (ground, mask,
/// seed, address prefix and exclusions); only the input's area may differ.
/// Call [`clear`](Self::clear) when any of them changes.
#[derive(Clone, Debug, Default)]
pub struct ChunkCache {
    grid: ChunkGrid,
    chunks: HashMap<ChunkId, Arc<[ScatterCopy]>>,
}

impl ChunkCache {
    /// An empty cache on the chunk grid `grid`.
    pub fn new(grid: ChunkGrid) -> Self {
        Self {
            grid,
            chunks: HashMap::new(),
        }
    }

    /// The chunk grid it places on.
    pub fn grid(&self) -> ChunkGrid {
        self.grid
    }

    /// How many chunks it holds.
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    /// Whether it holds no chunks.
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Forgets every chunk.
    pub fn clear(&mut self) {
        self.chunks.clear();
    }

    /// Forgets `chunk`, such as when it unloads.
    pub fn forget(&mut self, chunk: ChunkId) {
        self.chunks.remove(&chunk);
    }

    /// The copies `scatter` owns in `chunk`, placed now unless the cache
    /// already holds them. Their transforms are relative to the chunk's
    /// centre ([`ChunkGrid::area`]).
    ///
    /// # Errors
    ///
    /// As [`Scatter::place`]; nothing is cached then.
    pub fn chunk(
        &mut self,
        scatter: &dyn Scatter,
        input: &ScatterInput<'_>,
        chunk: ChunkId,
    ) -> Result<Arc<[ScatterCopy]>, ScatterError> {
        if let Some(copies) = self.chunks.get(&chunk) {
            return Ok(copies.clone());
        }
        let copies: Arc<[ScatterCopy]> = scatter.place(&input.over(self.grid.area(chunk)))?.into();
        self.chunks.insert(chunk, copies.clone());
        Ok(copies)
    }

    /// Looks up many addresses at once: the copy at each, or `None` where its
    /// spot is empty or it names another Scatter, in the order given.
    ///
    /// Each chunk holding one of the addresses is placed once, and kept.
    /// Transforms are relative to `input.area.centre`.
    ///
    /// # Errors
    ///
    /// As [`Scatter::place`].
    pub fn resolve_all(
        &mut self,
        scatter: &dyn Scatter,
        input: &ScatterInput<'_>,
        addresses: &[CopyAddress],
    ) -> Result<Vec<Option<ScatterCopy>>, ScatterError> {
        // Group the addresses by the chunk holding their cell.
        let lattice = scatter.lattice();
        let mut by_chunk: BTreeMap<ChunkId, Vec<usize>> = BTreeMap::new();
        for (i, address) in addresses.iter().enumerate() {
            if address.prefix() == input.prefix {
                let chunk = self.grid.chunk_of(lattice.corner(address.cell));
                by_chunk.entry(chunk).or_default().push(i);
            }
        }

        // Place each chunk once and find its addresses in it.
        let mut found = vec![None; addresses.len()];
        for (chunk, wanted) in by_chunk {
            let copies = self.chunk(scatter, input, chunk)?;
            let index: HashMap<CopyAddress, &ScatterCopy> =
                copies.iter().map(|c| (c.address, c)).collect();
            let centre = self.grid.area(chunk).centre;
            for i in wanted {
                found[i] = index
                    .get(&addresses[i])
                    .map(|&copy| copy.clone().moved(centre, input.area.centre));
            }
        }
        Ok(found)
    }

    /// The footprints of `scatter`'s copies that can reach into `rect`, from
    /// the chunks around it, placing any the cache lacks.
    ///
    /// This is how a lower Scatter's chunk reads the finished copies of the
    /// Scatters above it: pass the result as its input's exclusions.
    ///
    /// # Errors
    ///
    /// As [`Scatter::place`].
    pub fn footprints(
        &mut self,
        scatter: &dyn Scatter,
        input: &ScatterInput<'_>,
        rect: FixedRect,
    ) -> Result<Footprints, ScatterError> {
        let reach = scatter.reach();
        let grown = rect.grow(to_fixed(f64::from(reach.footprint + reach.displacement)));
        let mut footprints = Footprints::new();
        for chunk in self.grid.chunks_over(grown) {
            let copies = self.chunk(scatter, input, chunk)?;
            footprints.extend(copies.iter().flat_map(|c| c.footprint.iter().copied()));
        }
        Ok(footprints)
    }
}

#[cfg(test)]
mod tests;
