//! Which chunks a terrain wants, which are being read, and when the drawn
//! set can be swapped for a new one. Plain Rust, so it can be tested
//! without a renderer.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use glam::DVec2;
use procgena_terrain::{
    CHUNK_CELLS, ChunkKey, ChunkSamples, ChunkSelection, ChunkStage, Transitions,
};

/// A chunk's samples, or `None` if it could not be read (it is then drawn
/// as a hole rather than holding up every other chunk).
pub(crate) type Read = Option<Arc<ChunkSamples>>;

/// One terrain's streaming state.
///
/// The drawn chunks change all at once: a new selection replaces the drawn
/// one only when every chunk in it has been read, so the drawn chunks
/// always tile the ground and fit their neighbours. A drawn chunk read
/// from provisional ground can then be read again from canonical ground
/// and replaced on its own ([`upgrades`](Self::upgrades)), since that
/// changes its heights but not the tiling.
#[derive(Default)]
pub(crate) struct Stream {
    /// Counts resets, so reads started before one can be told apart.
    pub(crate) generation: u64,
    /// The generation the drawn chunks were read in.
    drawn_generation: u64,
    /// The chunks wanted now.
    pub(crate) target: ChunkSelection,
    /// The chunks drawn now.
    pub(crate) drawn: ChunkSelection,
    /// Samples read in this generation.
    samples: HashMap<ChunkKey, Read>,
    /// Chunks being read.
    reading: HashSet<ChunkKey>,
    /// Drawn chunks whose samples were upgraded, to be redrawn.
    refreshed: Vec<ChunkKey>,
    /// Chunks whose canonical read failed; they keep their provisional
    /// ground and aren't tried again in this generation.
    no_upgrade: HashSet<ChunkKey>,
}

/// What to change in the drawn chunks.
pub(crate) struct Swap {
    /// Chunks to remove.
    pub(crate) remove: Vec<ChunkKey>,
    /// Chunks to add (or replace, if also in `remove`), with their
    /// transitions and samples.
    pub(crate) add: Vec<(ChunkKey, Transitions, Read)>,
}

impl Stream {
    /// Starts over with new ground: earlier reads are dropped, and the
    /// drawn chunks stay until the new ones are ready.
    pub(crate) fn reset(&mut self) {
        self.generation += 1;
        self.samples.clear();
        self.reading.clear();
        self.refreshed.clear();
        self.no_upgrade.clear();
    }

    /// Wants `target` from now on.
    pub(crate) fn retarget(&mut self, target: ChunkSelection) {
        self.target = target;
    }

    /// Chunks of the target that are neither read nor being read, nearest
    /// to `viewers` first.
    pub(crate) fn missing(&self, viewers: &[DVec2]) -> Vec<ChunkKey> {
        let mut missing: Vec<(f64, ChunkKey)> = self
            .target
            .iter()
            .map(|(key, _)| key)
            .filter(|key| !self.samples.contains_key(key) && !self.reading.contains(key))
            .map(|key| {
                let d = viewers
                    .iter()
                    .map(|&v| key.distance_to(v))
                    .fold(f64::INFINITY, f64::min);
                (d, key)
            })
            .collect();
        missing.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        missing.into_iter().map(|(_, key)| key).collect()
    }

    /// How many chunks of the target are read.
    pub(crate) fn read_in_target(&self) -> usize {
        self.target
            .iter()
            .filter(|(key, _)| self.samples.contains_key(key))
            .count()
    }

    /// How many chunks are being read.
    pub(crate) fn reading(&self) -> usize {
        self.reading.len()
    }

    /// Whether nothing is being read and the drawn chunks are the target.
    pub(crate) fn is_settled(&self) -> bool {
        self.reading.is_empty()
            && self.drawn == self.target
            && self.drawn_generation == self.generation
    }

    /// Marks `key` as being read.
    pub(crate) fn start(&mut self, key: ChunkKey) {
        self.reading.insert(key);
    }

    /// Keeps the result of reading `key` in `generation`; a result from an
    /// earlier generation is dropped. A result for a chunk already read is
    /// an upgrade: it replaces the samples, and a drawn chunk is queued
    /// for [`take_refreshed`](Self::take_refreshed). A failed upgrade keeps
    /// the samples it would have replaced.
    pub(crate) fn finish(&mut self, key: ChunkKey, generation: u64, read: Read) {
        if generation != self.generation {
            return;
        }
        self.reading.remove(&key);
        match self.samples.get(&key) {
            Some(Some(_)) if read.is_none() => {
                self.no_upgrade.insert(key);
            }
            Some(Some(_)) => {
                self.samples.insert(key, read);
                if self.is_drawn(key) {
                    self.refreshed.push(key);
                }
            }
            _ => {
                self.samples.insert(key, read);
            }
        }
    }

    /// Whether `key` is drawn from samples of this generation.
    fn is_drawn(&self, key: ChunkKey) -> bool {
        self.drawn_generation == self.generation && self.drawn.get(key).is_some()
    }

    /// Drawn chunks read from provisional ground that `wants_canonical`
    /// says should be canonical, and that are not being read, nearest to
    /// `viewers` first.
    pub(crate) fn upgrades(
        &self,
        viewers: &[DVec2],
        wants_canonical: impl Fn(ChunkKey) -> bool,
    ) -> Vec<ChunkKey> {
        let mut wanted: Vec<(f64, ChunkKey)> = self
            .drawn
            .iter()
            .map(|(key, _)| key)
            .filter(|&key| {
                self.is_drawn(key)
                    && wants_canonical(key)
                    && !self.reading.contains(&key)
                    && !self.no_upgrade.contains(&key)
                    && matches!(
                        self.samples.get(&key),
                        Some(Some(s)) if s.stage() == ChunkStage::Provisional
                    )
            })
            .map(|key| {
                let d = viewers
                    .iter()
                    .map(|&v| key.distance_to(v))
                    .fold(f64::INFINITY, f64::min);
                (d, key)
            })
            .collect();
        wanted.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        wanted.into_iter().map(|(_, key)| key).collect()
    }

    /// The drawn chunks upgraded since the last call, with their
    /// transitions and new samples, to redraw one by one.
    pub(crate) fn take_refreshed(&mut self) -> Vec<(ChunkKey, Transitions, Read)> {
        std::mem::take(&mut self.refreshed)
            .into_iter()
            .filter_map(|key| {
                let transitions = self.drawn.get(key).filter(|_| self.is_drawn(key))?;
                Some((key, transitions, self.samples.get(&key)?.clone()))
            })
            .collect()
    }

    /// The changes that draw the target, once every chunk of it is read;
    /// `None` while some are still missing, or when nothing changes.
    /// Samples no longer needed are dropped.
    pub(crate) fn swap(&mut self) -> Option<Swap> {
        if self
            .target
            .iter()
            .any(|(key, _)| !self.samples.contains_key(&key))
        {
            return None;
        }
        let fresh = self.drawn_generation != self.generation;
        if !fresh && self.drawn == self.target {
            return None;
        }

        // Chunks gone or changed go; chunks new or changed come. After a
        // reset every chunk is new.
        let remove = self
            .drawn
            .iter()
            .filter(|&(key, t)| fresh || self.target.get(key) != Some(t))
            .map(|(key, _)| key)
            .collect();
        let add = self
            .target
            .iter()
            .filter(|&(key, t)| fresh || self.drawn.get(key) != Some(t))
            .map(|(key, t)| (key, t, self.samples[&key].clone()))
            .collect();
        self.drawn = self.target.clone();
        self.drawn_generation = self.generation;
        self.samples
            .retain(|key, _| self.drawn.get(*key).is_some() || self.reading.contains(key));
        Some(Swap { remove, add })
    }

    /// The ground height under `p` from the finest samples kept, if any:
    /// the height of the nearest vertex.
    pub(crate) fn ground_height(&self, p: DVec2) -> Option<f32> {
        let top = self.samples.keys().map(|k| k.level).max()?;
        (0..=top).find_map(|level| {
            let key = ChunkKey::containing(p, level);
            let samples = self.samples.get(&key)?.as_ref()?;
            let local = (p - key.min_corner()) / key.step();
            let cell = |x: f64| (x.round() as u32).min(CHUNK_CELLS);
            Some(samples.vertex(cell(local.x), cell(local.y)).height)
        })
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use procgena_core::{Lod, LodLevel};

    use super::*;

    /// Chunk levels switching at 96 and 192 m, drawn to 400 m.
    fn lod() -> Lod {
        let levels = [0.0, 96.0, 192.0]
            .map(|d| LodLevel::new(d, 0.1, ()))
            .to_vec();
        Lod::new(64.0, levels).with_cull(400.0)
    }

    fn select(x: f64, previous: Option<&ChunkSelection>) -> ChunkSelection {
        ChunkSelection::around(&lod(), 1.0, &[DVec3::new(x, 2.0, 0.0)], previous).unwrap()
    }

    /// Reads every missing chunk as a hole.
    fn read_all(stream: &mut Stream) {
        for key in stream.missing(&[DVec2::ZERO]) {
            stream.start(key);
            let generation = stream.generation;
            stream.finish(key, generation, None);
        }
    }

    /// Flat ground at height 1 from provisional reads and 2 from canonical
    /// ones.
    struct TwoStages;

    impl procgena_terrain::ChunkSource for TwoStages {
        fn surface(&self, _: DVec2, stage: ChunkStage) -> procgena_terrain::SurfacePoint {
            procgena_terrain::SurfacePoint {
                height: if stage == ChunkStage::Canonical {
                    2.0
                } else {
                    1.0
                },
                water: None,
            }
        }

        fn stage(&self, stage: ChunkStage) -> ChunkStage {
            stage
        }
    }

    /// Reads `key` from [`TwoStages`] at `stage` and finishes it.
    fn read(stream: &mut Stream, key: ChunkKey, stage: ChunkStage) {
        stream.start(key);
        let samples = ChunkSamples::sample(&TwoStages, key, stage).unwrap();
        let generation = stream.generation;
        stream.finish(key, generation, Some(Arc::new(samples)));
    }

    #[test]
    fn provisional_chunks_are_drawn_then_upgraded_one_by_one() {
        let mut stream = Stream::default();
        stream.retarget(select(0.0, None));
        for key in stream.missing(&[DVec2::ZERO]) {
            read(&mut stream, key, ChunkStage::Provisional);
        }
        assert!(stream.swap().is_some());

        // Only chunks wanted canonical are upgraded, nearest first.
        let fine = |key: ChunkKey| key.level == 0;
        let upgrades = stream.upgrades(&[DVec2::ZERO], fine);
        let level_0 = stream.drawn.iter().filter(|(k, _)| k.level == 0).count();
        assert_eq!(upgrades.len(), level_0);
        assert!(upgrades.len() < stream.drawn.len());

        // An upgraded chunk is redrawn alone, with canonical ground; a
        // failed upgrade keeps the provisional ground and isn't retried.
        read(&mut stream, upgrades[0], ChunkStage::Canonical);
        stream.start(upgrades[1]);
        let generation = stream.generation;
        stream.finish(upgrades[1], generation, None);
        let refreshed = stream.take_refreshed();
        assert_eq!(refreshed.len(), 1);
        let (key, transitions, samples) = &refreshed[0];
        assert_eq!(*key, upgrades[0]);
        assert_eq!(stream.drawn.get(*key), Some(*transitions));
        let samples = samples.as_ref().unwrap();
        assert_eq!(samples.stage(), ChunkStage::Canonical);
        assert_eq!(samples.vertex(3, 3).height, 2.0);
        let left = stream.upgrades(&[DVec2::ZERO], fine);
        assert_eq!(left.len(), upgrades.len() - 2);
        assert!(!left.contains(&upgrades[0]) && !left.contains(&upgrades[1]));
        assert!(stream.take_refreshed().is_empty());
        // Upgrades don't change the tiling.
        assert!(stream.swap().is_none());
    }

    #[test]
    fn the_drawn_chunks_change_only_when_the_whole_target_is_read() {
        let mut stream = Stream::default();
        stream.retarget(select(0.0, None));
        let missing = stream.missing(&[DVec2::ZERO]);
        assert_eq!(missing.len(), stream.target.len());
        // The chunk under the viewer comes first.
        assert_eq!(missing[0].level, 0);

        // Read all but one: nothing to swap yet.
        for &key in &missing[..missing.len() - 1] {
            stream.start(key);
            stream.finish(key, 0, None);
        }
        assert!(stream.swap().is_none());
        let last = missing[missing.len() - 1];
        stream.start(last);
        stream.finish(last, 0, None);
        let swap = stream.swap().unwrap();
        assert!(swap.remove.is_empty());
        assert_eq!(swap.add.len(), stream.target.len());
        assert!(stream.is_settled());
        assert!(stream.swap().is_none());

        // Moving a little changes the chunks near the viewer, but not the
        // far ones.
        let next = select(70.0, Some(&stream.target));
        stream.retarget(next);
        read_all(&mut stream);
        let swap = stream.swap().unwrap();
        assert!(!swap.remove.is_empty() && !swap.add.is_empty());
        assert!(swap.add.len() < stream.target.len());
        // Every chunk added is in the target, with its transitions.
        for (key, t, _) in &swap.add {
            assert_eq!(stream.drawn.get(*key), Some(*t));
        }
    }

    #[test]
    fn a_reset_replaces_every_chunk_and_drops_old_reads() {
        let mut stream = Stream::default();
        stream.retarget(select(0.0, None));
        read_all(&mut stream);
        let before = stream.swap().unwrap().add.len();

        // A read started before the reset finishes after it: dropped.
        stream.reset();
        let key = stream.missing(&[DVec2::ZERO])[0];
        stream.start(key);
        stream.finish(key, stream.generation - 1, None);
        assert!(stream.missing(&[DVec2::ZERO]).len() == before - 1);
        assert!(!stream.is_settled());

        stream.finish(key, stream.generation, None);
        read_all(&mut stream);
        let swap = stream.swap().unwrap();
        assert_eq!(swap.remove.len(), before);
        assert_eq!(swap.add.len(), before);
    }
}
