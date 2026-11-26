//! Edge contracts kept once built, so the two chunks beside an edge sample
//! it once between them.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use crate::chunk::ChunkSource;

use super::contract::{ChunkEdgeKey, EdgeContract};
use super::numeric::CmHeight;
use super::probe::TinError;
use super::tolerance::TinTolerance;

/// An edge contract with the lattice points it sampled along its edge.
pub(crate) type SampledContract = Arc<(EdgeContract, Vec<(usize, CmHeight)>)>;

/// Edge contracts already built for one ground source and one tolerance,
/// so the chunks either side of an edge share one build of it (see
/// [`build_chunk_with`](super::build_chunk_with)).
///
/// Use one cache with one source and one tolerance only: it can't tell
/// sources apart, so a contract built from another source's ground would
/// be reused as if it were this one's. It may be shared between threads.
/// Two threads asking for the same new edge at once may both build it;
/// both builds are the same, and one is kept. It keeps every contract
/// until [`retain`](Self::retain) or [`clear`](Self::clear) drops it.
#[derive(Debug)]
pub struct ContractCache {
    /// The [`content_hash`](TinTolerance::content_hash) of the tolerance.
    tolerance: u64,
    edges: Mutex<HashMap<ChunkEdgeKey, SampledContract>>,
}

impl ContractCache {
    /// An empty cache for contracts built within `tolerance`.
    pub fn new(tolerance: &TinTolerance) -> Self {
        Self {
            tolerance: tolerance.content_hash(),
            edges: Mutex::new(HashMap::new()),
        }
    }

    /// How many contracts it holds.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Keeps only the contracts of the edges `keep` says to keep, such as
    /// the edges of the chunks still loaded.
    pub fn retain(&self, mut keep: impl FnMut(ChunkEdgeKey) -> bool) {
        self.lock().retain(|&edge, _| keep(edge));
    }

    /// Drops every contract.
    pub fn clear(&self) {
        self.lock().clear();
    }

    /// The contract for `edge` from `source` within `tolerance`, built
    /// now unless the cache holds it.
    ///
    /// # Panics
    ///
    /// If `tolerance` is not the one the cache was made for.
    pub(crate) fn get_or_build(
        &self,
        source: &dyn ChunkSource,
        edge: ChunkEdgeKey,
        tolerance: &TinTolerance,
    ) -> Result<SampledContract, TinError> {
        assert_eq!(
            tolerance.content_hash(),
            self.tolerance,
            "the cache holds contracts for another tolerance"
        );
        if let Some(found) = self.lock().get(&edge) {
            return Ok(found.clone());
        }
        // Built without holding the lock, so other edges aren't held up.
        let built = Arc::new(EdgeContract::build_sampled(source, edge, tolerance)?);
        Ok(self.lock().entry(edge).or_insert(built).clone())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<ChunkEdgeKey, SampledContract>> {
        // A panic while holding the lock leaves only whole entries behind.
        self.edges.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
