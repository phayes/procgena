//! Which chunks a terrain wants, which are being read, and when the drawn
//! set can be swapped for a new one. Plain Rust, so it can be tested
//! without a renderer.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use glam::DVec2;
use procgena_terrain::{CHUNK_CELLS, ChunkKey, ChunkSamples, ChunkSelection, Transitions};

/// A chunk's samples, or `None` if it could not be read (it is then drawn
/// as a hole rather than holding up every other chunk).
pub(crate) type Read = Option<Arc<ChunkSamples>>;

/// One terrain's streaming state.
///
/// The drawn chunks change all at once: a new selection replaces the drawn
/// one only when every chunk in it has been read, so the drawn chunks
/// always tile the ground and fit their neighbours.
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
    /// earlier generation is dropped.
    pub(crate) fn finish(&mut self, key: ChunkKey, generation: u64, read: Read) {
        if generation != self.generation {
            return;
        }
        self.reading.remove(&key);
        self.samples.insert(key, read);
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
