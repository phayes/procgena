//! Keeping count of region builds from a world's progress hook, so a
//! stream can say how far it has got.

use std::sync::{Arc, Mutex, PoisonError};

use procgena_terrain::{ProgressHook, RegionProgress, RegionStage};

/// Rough cost of one region stage, in units of a region plan. Measured in
/// the terrain crate's benchmark: the road search dominates.
pub(crate) fn stage_cost(stage: RegionStage) -> f32 {
    match stage {
        RegionStage::Plan => 1.0,
        RegionStage::Ground => 0.6,
        RegionStage::Roads => 5.0,
    }
}

/// Rough cost of reading one chunk, in the same units.
pub(crate) const CHUNK_COST: f32 = 0.7;

/// Counts a world's region builds, as told by its progress hook.
///
/// Make one, pass [`hook`](Self::hook) to
/// [`WorldTerrain::with_progress`](procgena_terrain::WorldTerrain::with_progress),
/// and give the tracker to the [`TerrainStream`](crate::TerrainStream);
/// [`TerrainStream::world`](crate::TerrainStream::world) does all three.
/// Counts start over each time a build starts with none running, so they
/// describe the current batch of work, not the world's whole history.
#[derive(Clone, Default)]
pub struct RegionTracker {
    state: Arc<Mutex<Counts>>,
}

/// The current batch of region work.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Counts {
    /// Cost of every stage started in this batch.
    pub(crate) total: f32,
    /// Cost of the stages finished.
    pub(crate) done: f32,
    /// Stages running, by stage, in [`RegionStage`] order.
    pub(crate) running: [usize; 3],
}

impl Counts {
    /// The costliest stage running, if any.
    pub(crate) fn stage(&self) -> Option<RegionStage> {
        [RegionStage::Roads, RegionStage::Ground, RegionStage::Plan]
            .into_iter()
            .find(|&s| self.running[index(s)] > 0)
    }
}

/// The position of `stage` in [`Counts::running`].
fn index(stage: RegionStage) -> usize {
    match stage {
        RegionStage::Plan => 0,
        RegionStage::Ground => 1,
        RegionStage::Roads => 2,
    }
}

impl RegionTracker {
    /// A hook that counts every region stage into this tracker.
    pub fn hook(&self) -> ProgressHook {
        let tracker = self.clone();
        Arc::new(move |progress| tracker.record(progress))
    }

    /// Counts one start or finish.
    pub fn record(&self, progress: &RegionProgress) {
        let mut counts = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        match *progress {
            RegionProgress::Started { stage, .. } => {
                if counts.running.iter().all(|&n| n == 0) {
                    *counts = Counts::default();
                }
                counts.total += stage_cost(stage);
                counts.running[index(stage)] += 1;
            }
            RegionProgress::Finished { stage, .. } => {
                counts.done += stage_cost(stage);
                let running = &mut counts.running[index(stage)];
                *running = running.saturating_sub(1);
            }
        }
    }

    /// The counts now.
    pub(crate) fn counts(&self) -> Counts {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use procgena_terrain::RegionId;

    use super::*;

    fn started(stage: RegionStage) -> RegionProgress {
        RegionProgress::Started {
            region: RegionId(1),
            stage,
        }
    }

    fn finished(stage: RegionStage) -> RegionProgress {
        RegionProgress::Finished {
            region: RegionId(1),
            stage,
            elapsed: Duration::ZERO,
            built: true,
        }
    }

    #[test]
    fn counts_follow_a_batch_and_start_over_after_it() {
        let tracker = RegionTracker::default();
        let hook = tracker.hook();
        hook(&started(RegionStage::Plan));
        hook(&started(RegionStage::Roads));
        let counts = tracker.counts();
        assert_eq!(counts.total, 6.0);
        assert_eq!(counts.stage(), Some(RegionStage::Roads));
        hook(&finished(RegionStage::Plan));
        hook(&finished(RegionStage::Roads));
        let counts = tracker.counts();
        assert_eq!((counts.done, counts.total), (6.0, 6.0));
        assert_eq!(counts.stage(), None);

        // A new batch starts from nothing.
        hook(&started(RegionStage::Ground));
        assert_eq!(tracker.counts().total, 0.6);
        assert_eq!(tracker.counts().done, 0.0);
    }
}
