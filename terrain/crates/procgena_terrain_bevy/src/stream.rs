//! Which chunks a terrain wants, which are being read, and when the drawn
//! set can be swapped for a new one. Plain Rust, so it can be tested
//! without a renderer.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use glam::DVec2;
use procgena_terrain::{CHUNK_CELLS, ChunkKey, ChunkSelection, FeatureTier, Transitions};

use crate::ChunkRead;

/// A chunk as read, or `None` if it could not be read (it is then drawn
/// as a hole rather than holding up every other chunk).
pub(crate) type Read = Option<Arc<ChunkRead>>;

/// One terrain's streaming state.
///
/// The drawn chunks follow the reads: each part of the ground is swapped
/// to the wanted chunks once all of them there are read, and the drawn
/// chunks always tile the ground and fit their neighbours (see
/// [`ChunkSelection::blend`]). After a reset they change all at once, when
/// the whole new selection is read. A drawn chunk read
/// at an earlier feature tier than it wants can then be read again at its
/// own tier and replaced on its own ([`upgrades`](Self::upgrades)), since
/// that changes its heights but not the tiling.
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
    /// Chunks whose upgrade read failed; they keep the ground they have
    /// and aren't tried again in this generation.
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

    /// What becomes of a read of `key` finishing now: whether it is an
    /// upgrade of samples already kept.
    pub(crate) fn is_upgrade(&self, key: ChunkKey) -> bool {
        matches!(self.samples.get(&key), Some(Some(_)))
    }

    /// Keeps the result of reading `key` in `generation`; a result from an
    /// earlier generation is dropped. A result for a chunk already read is
    /// an upgrade (see [`is_upgrade`](Self::is_upgrade)): it replaces the
    /// samples, and a drawn chunk is queued for
    /// [`take_refreshed`](Self::take_refreshed). A failed upgrade keeps the
    /// samples it would have replaced.
    pub(crate) fn finish(&mut self, key: ChunkKey, generation: u64, read: Read) {
        if generation != self.generation {
            return;
        }
        self.reading.remove(&key);
        if !self.is_upgrade(key) {
            self.samples.insert(key, read);
        } else if read.is_none() {
            self.no_upgrade.insert(key);
        } else {
            self.samples.insert(key, read);
            if self.is_drawn(key) {
                self.refreshed.push(key);
            }
        }
    }

    /// Whether `key` is drawn from samples of this generation.
    fn is_drawn(&self, key: ChunkKey) -> bool {
        self.drawn_generation == self.generation && self.drawn.get(key).is_some()
    }

    /// The tier of the samples kept for `key`, if read.
    pub(crate) fn tier_of(&self, key: ChunkKey) -> Option<FeatureTier> {
        Some(self.samples.get(&key)?.as_ref()?.samples.tier())
    }

    /// The samples kept for `key`, if read.
    pub(crate) fn read_of(&self, key: ChunkKey) -> Read {
        self.samples.get(&key)?.clone()
    }

    /// The drawn chunks that touch `key` along a side or at a corner, with
    /// their transitions.
    pub(crate) fn touching(&self, key: ChunkKey) -> Vec<(ChunkKey, Transitions)> {
        let (lo, hi) = (key.min_corner(), key.max_corner());
        self.drawn
            .iter()
            .filter(|&(other, _)| {
                let (a, b) = (other.min_corner(), other.max_corner());
                other != key && a.x <= hi.x && a.y <= hi.y && b.x >= lo.x && b.y >= lo.y
            })
            .collect()
    }

    /// Drawn chunks read at an earlier tier than `wanted` gives them, and
    /// not being read, nearest to `viewers` first.
    pub(crate) fn upgrades(
        &self,
        viewers: &[DVec2],
        wanted: impl Fn(ChunkKey) -> FeatureTier,
    ) -> Vec<ChunkKey> {
        let mut wanted_keys: Vec<(f64, ChunkKey)> = self
            .drawn
            .iter()
            .map(|(key, _)| key)
            .filter(|&key| {
                self.is_drawn(key)
                    && !self.reading.contains(&key)
                    && !self.no_upgrade.contains(&key)
                    && self.tier_of(key).is_some_and(|tier| tier < wanted(key))
            })
            .map(|key| {
                let d = viewers
                    .iter()
                    .map(|&v| key.distance_to(v))
                    .fold(f64::INFINITY, f64::min);
                (d, key)
            })
            .collect();
        wanted_keys.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        wanted_keys.into_iter().map(|(_, key)| key).collect()
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

    /// The changes that bring the drawn chunks closer to the target: every
    /// part of the ground whose target chunks are all read is drawn from
    /// them, and the rest keeps the chunks drawn there (see
    /// [`ChunkSelection::blend`]). After a reset, nothing changes until the
    /// whole target is read, so old and new ground are never drawn side by
    /// side. `None` when nothing changes. Samples no longer needed are
    /// dropped.
    pub(crate) fn swap(&mut self) -> Option<Swap> {
        let fresh = self.drawn_generation != self.generation;
        let next = if fresh {
            let all_read = self
                .target
                .iter()
                .all(|(key, _)| self.samples.contains_key(&key));
            if !all_read {
                return None;
            }
            self.target.clone()
        } else {
            self.target
                .blend(&self.drawn, |key| self.samples.contains_key(&key))
        };
        if !fresh && self.drawn == next {
            return None;
        }

        // Chunks gone or changed go; chunks new or changed come. After a
        // reset every chunk is new.
        let remove = self
            .drawn
            .iter()
            .filter(|&(key, t)| fresh || next.get(key) != Some(t))
            .map(|(key, _)| key)
            .collect();
        let add = next
            .iter()
            .filter(|&(key, t)| fresh || self.drawn.get(key) != Some(t))
            .map(|(key, t)| (key, t, self.samples[&key].clone()))
            .collect();
        self.drawn = next;
        self.drawn_generation = self.generation;
        // Keep what is drawn, wanted or still coming.
        self.samples.retain(|key, _| {
            self.drawn.get(*key).is_some()
                || self.target.get(*key).is_some()
                || self.reading.contains(key)
        });
        Some(Swap { remove, add })
    }

    /// The ground height under `p` from the finest samples kept, if any:
    /// the height of the nearest vertex.
    pub(crate) fn ground_height(&self, p: DVec2) -> Option<f32> {
        let top = self.samples.keys().map(|k| k.level).max()?;
        (0..=top).find_map(|level| {
            let key = ChunkKey::containing(p, level);
            let samples = &self.samples.get(&key)?.as_ref()?.samples;
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
    use procgena_terrain::ground::{GroundSite, GroundSource, GroundStack};

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

    /// Flat ground at height 1 from rough reads and 2 from fine ones.
    struct TwoTiers;

    impl procgena_terrain::ChunkSource for TwoTiers {
        fn surface(&self, _: DVec2, tier: FeatureTier) -> procgena_terrain::SurfacePoint {
            procgena_terrain::SurfacePoint {
                height: if tier == FeatureTier::Fine { 2.0 } else { 1.0 },
                water: None,
            }
        }

        fn tier(&self, tier: FeatureTier) -> FeatureTier {
            tier
        }

        fn tier_difference(&self, _: DVec2, _: DVec2, from: FeatureTier) -> f32 {
            if from == FeatureTier::Fine { 0.0 } else { 1.0 }
        }
    }

    impl GroundSource for TwoTiers {
        fn site(&self, p: DVec2, tier: FeatureTier) -> GroundSite {
            use procgena_terrain::ChunkSource;
            GroundSite::new(self.surface(p, tier).height, glam::Vec2::ZERO)
        }
    }

    /// Reads `key` from [`TwoTiers`] at `tier`, painted bare, and finishes
    /// it.
    fn read(stream: &mut Stream, key: ChunkKey, tier: FeatureTier) {
        stream.start(key);
        let read = ChunkRead::read(&TwoTiers, &GroundStack::bare(0, 1.0), key, tier).unwrap();
        let generation = stream.generation;
        stream.finish(key, generation, Some(Arc::new(read)));
    }

    #[test]
    fn rough_chunks_are_drawn_then_upgraded_one_by_one() {
        let mut stream = Stream::default();
        stream.retarget(select(0.0, None));
        for key in stream.missing(&[DVec2::ZERO]) {
            read(&mut stream, key, FeatureTier::Rough);
        }
        assert!(stream.swap().is_some());

        // Only chunks wanted fine are upgraded, nearest first.
        let fine = |key: ChunkKey| {
            if key.level == 0 {
                FeatureTier::Fine
            } else {
                FeatureTier::Rough
            }
        };
        let upgrades = stream.upgrades(&[DVec2::ZERO], fine);
        let level_0 = stream.drawn.iter().filter(|(k, _)| k.level == 0).count();
        assert_eq!(upgrades.len(), level_0);
        assert!(upgrades.len() < stream.drawn.len());

        // An upgraded chunk is redrawn alone, with fine ground; a failed
        // upgrade keeps the rough ground and isn't retried.
        assert!(stream.is_upgrade(upgrades[0]));
        read(&mut stream, upgrades[0], FeatureTier::Fine);
        stream.start(upgrades[1]);
        let generation = stream.generation;
        stream.finish(upgrades[1], generation, None);
        let refreshed = stream.take_refreshed();
        assert_eq!(refreshed.len(), 1);
        let (key, transitions, samples) = &refreshed[0];
        assert_eq!(*key, upgrades[0]);
        assert_eq!(stream.drawn.get(*key), Some(*transitions));
        let samples = &samples.as_ref().unwrap().samples;
        assert_eq!(samples.tier(), FeatureTier::Fine);
        assert_eq!(stream.tier_of(upgrades[1]), Some(FeatureTier::Rough));
        assert_eq!(samples.vertex(3, 3).height, 2.0);
        let left = stream.upgrades(&[DVec2::ZERO], fine);
        assert_eq!(left.len(), upgrades.len() - 2);
        assert!(!left.contains(&upgrades[0]) && !left.contains(&upgrades[1]));
        assert!(stream.take_refreshed().is_empty());
        // Upgrades don't change the tiling.
        assert!(stream.swap().is_none());

        // The chunks touching one are those beside it and across its
        // corners, at any level.
        let key = upgrades[0];
        let touching = stream.touching(key);
        assert!(!touching.is_empty() && touching.iter().all(|(k, _)| *k != key));
        for (other, transitions) in &touching {
            assert_eq!(stream.drawn.get(*other), Some(*transitions));
            assert!(
                other.distance_to(key.min_corner() + DVec2::splat(key.size() / 2.0)) <= key.size()
            );
        }
        assert!(stream.read_of(key).is_some());
    }

    #[test]
    fn the_drawn_chunks_follow_the_reads_nearest_first() {
        let mut stream = Stream::default();
        stream.retarget(select(0.0, None));
        let missing = stream.missing(&[DVec2::ZERO]);
        assert_eq!(missing.len(), stream.target.len());
        // The chunk under the viewer comes first.
        assert_eq!(missing[0].level, 0);

        // Read all but the farthest: the ground near the viewer is drawn
        // already, but not all of it.
        for &key in &missing[..missing.len() - 1] {
            stream.start(key);
            stream.finish(key, 0, None);
        }
        let swap = stream.swap().unwrap();
        assert!(swap.remove.is_empty());
        assert!(swap.add.len() < stream.target.len());
        assert!(stream.drawn.get(missing[0]).is_some());
        assert!(
            stream
                .drawn
                .iter()
                .all(|(key, _)| stream.target.get(key).is_some())
        );
        assert!(!stream.is_settled());

        // Read the last: the rest is drawn.
        let last = missing[missing.len() - 1];
        stream.start(last);
        stream.finish(last, 0, None);
        assert!(stream.swap().is_some());
        assert_eq!(stream.drawn, stream.target);
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
