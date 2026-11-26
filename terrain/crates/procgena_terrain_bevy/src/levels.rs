//! Which chunks a terrain builds and draws at each chunk level, in what
//! order, and when the drawn chunks change. Plain Rust, generic over what
//! a built chunk is, so it can be tested without a renderer or real
//! ground.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use glam::DVec2;
use procgena_terrain::{ChunkKey, ChunkSelection};

/// How far around the viewers each chunk level is built, whether it is
/// drawn there or not: the ground Scatters place on.
///
/// A chunk is built when it is drawn, or when its nearest point is within
/// its level's radius of a viewer, measured on the ground. Levels past the
/// last radius are built only where they are drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildRadii {
    /// The radius in metres for each chunk level, finest first, at the
    /// default quality. Lower qualities scale them down as they scale draw
    /// distances.
    pub radii: Vec<f32>,
}

impl Default for BuildRadii {
    /// Level 0 to 150 m, 1 to 500 m, 2 to 1.5 km and 3 to 4 km.
    fn default() -> Self {
        Self {
            radii: vec![150.0, 500.0, 1500.0, 4000.0],
        }
    }
}

/// How far past its radius a built chunk stays built, as a share of the
/// radius, so a viewer moving along a radius doesn't rebuild chunks.
const RADIUS_HYSTERESIS: f64 = 0.1;

impl BuildRadii {
    /// The chunks within their level's radius × `factor` of any of
    /// `viewers` (x and z), keeping those in `previous` until they are 10%
    /// past it.
    pub(crate) fn around(
        &self,
        factor: f32,
        viewers: &[DVec2],
        previous: &BTreeSet<ChunkKey>,
    ) -> BTreeSet<ChunkKey> {
        let mut chunks = BTreeSet::new();
        for (level, &radius) in self.radii.iter().enumerate() {
            let level = level as u8;
            let enter = f64::from(radius * factor);
            let stay = enter * (1.0 + RADIUS_HYSTERESIS);
            for &v in viewers {
                let lo = ChunkKey::containing(v - stay, level);
                let hi = ChunkKey::containing(v + stay, level);
                for z in lo.z..=hi.z {
                    for x in lo.x..=hi.x {
                        let key = ChunkKey::new(x, z, level);
                        let distance = key.distance_to(v);
                        if distance < enter || distance < stay && previous.contains(&key) {
                            chunks.insert(key);
                        }
                    }
                }
            }
        }
        chunks
    }
}

/// A built chunk, or `None` if it could not be built: it is then drawn as
/// a hole rather than holding up the chunks around it.
pub(crate) type Built<R> = Option<Arc<R>>;

/// A built chunk's ground, for heights under a point.
pub(crate) trait Ground {
    /// The ground's height under `p`, if `p` is in the chunk.
    fn height(&self, p: DVec2) -> Option<f32>;
}

/// What to change in the drawn chunks.
pub(crate) struct Swap<R> {
    /// Chunks no longer drawn, or drawn from another build.
    pub(crate) remove: Vec<ChunkKey>,
    /// Chunks to draw (replacing one in `remove` with the same key).
    pub(crate) add: Vec<(ChunkKey, Built<R>)>,
}

/// One terrain's chunks: which are wanted, built, being built and drawn.
///
/// The chunks to draw (the draw selection) and the chunks within a build
/// radius are both built; see [`wanted`](Self::wanted) for the order. The
/// drawn chunks never overlap. Until a chunk to draw is built, a coarser
/// chunk holding it is drawn instead: a chunk is replaced by its four
/// children only once all four can be drawn. After a reset, nothing changes
/// until the new top-level chunks can all be drawn, so old and new ground
/// are never drawn side by side.
pub(crate) struct LevelStream<R> {
    /// Counts resets, so builds started before one can be told apart.
    pub(crate) generation: u64,
    /// The generation the drawn chunks were built in.
    drawn_generation: u64,
    /// The chunks to draw once built.
    target: BTreeSet<ChunkKey>,
    /// Every chunk holding a chunk to draw, up to the top level, without
    /// being one.
    above: HashSet<ChunkKey>,
    /// The top-level chunks holding the chunks to draw.
    roots: BTreeSet<ChunkKey>,
    /// The chunks within a build radius.
    radius: BTreeSet<ChunkKey>,
    /// Chunks built in this generation.
    built: HashMap<ChunkKey, Built<R>>,
    /// Chunks being built.
    reading: HashSet<ChunkKey>,
    /// The chunks drawn now.
    drawn: BTreeMap<ChunkKey, Built<R>>,
    /// Whether the target or the builds changed since the drawn chunks were
    /// last chosen.
    stale: bool,
}

impl<R> Default for LevelStream<R> {
    fn default() -> Self {
        Self {
            generation: 0,
            drawn_generation: 0,
            target: BTreeSet::new(),
            above: HashSet::new(),
            roots: BTreeSet::new(),
            radius: BTreeSet::new(),
            built: HashMap::new(),
            reading: HashSet::new(),
            drawn: BTreeMap::new(),
            stale: false,
        }
    }
}

/// Where a chunk stands against the chunks to draw.
#[derive(Clone, Copy, PartialEq)]
enum Place {
    /// One of the chunks to draw.
    Target,
    /// Holds chunks to draw without being one.
    Above,
    /// Neither: nothing is drawn there.
    Outside,
}

impl<R> LevelStream<R> {
    /// Starts over with new ground: builds so far are dropped, and the drawn
    /// chunks stay until the new ones can replace them.
    pub(crate) fn reset(&mut self) {
        self.generation += 1;
        self.built.clear();
        self.reading.clear();
        self.stale = true;
    }

    /// Wants `drawn` drawn and `radius` built from now on.
    pub(crate) fn retarget(&mut self, drawn: &ChunkSelection, radius: BTreeSet<ChunkKey>) {
        let target: BTreeSet<ChunkKey> = drawn.iter().map(|(key, _)| key).collect();
        let top = target.iter().map(|k| k.level).max().unwrap_or(0);
        let mut above = HashSet::new();
        let mut roots = BTreeSet::new();
        for &key in &target {
            let mut at = key;
            while at.level < top {
                at = at.parent().expect("no chunk is above the top level");
                if !target.contains(&at) {
                    above.insert(at);
                }
            }
            roots.insert(at);
        }
        self.target = target;
        self.above = above;
        self.roots = roots;
        self.radius = radius;
        self.stale = true;
    }

    /// The chunks to build that are neither built nor being built, in the
    /// order to build them:
    ///
    /// 1. top-level chunks holding chunks to draw that aren't all built
    ///    yet, so a cold view is drawn soon, if coarsely;
    /// 2. chunks to draw, with those holding them that are within a build
    ///    radius, coarse and near first (by distance from the nearest of
    ///    `viewers` over chunk size);
    /// 3. the other chunks within a build radius, in the same order.
    pub(crate) fn wanted(&self, viewers: &[DVec2]) -> Vec<ChunkKey> {
        let missing = |key: &ChunkKey| !self.built.contains_key(key) && !self.reading.contains(key);

        // Top-level chunks are needed while a chunk to draw inside them
        // isn't built.
        let unbuilt_roots: HashSet<ChunkKey> = self
            .target
            .iter()
            .filter(|key| !self.built.contains_key(key))
            .map(|&key| self.root_of(key))
            .collect();
        let mut order: Vec<(u8, f64, ChunkKey)> = Vec::new();
        let rank = |key: ChunkKey| {
            let distance = viewers
                .iter()
                .map(|&v| key.distance_to(v))
                .fold(f64::INFINITY, f64::min);
            distance / key.size()
        };
        for &key in self.roots.iter().filter(|k| unbuilt_roots.contains(k)) {
            if missing(&key) {
                order.push((0, rank(key), key));
            }
        }
        let drawing = |key: &ChunkKey| {
            self.target.contains(key) || self.above.contains(key) && self.radius.contains(key)
        };
        for &key in self.target.iter().chain(&self.radius) {
            if missing(&key) && !(unbuilt_roots.contains(&key) && self.roots.contains(&key)) {
                let class = if drawing(&key) { 1 } else { 2 };
                order.push((class, rank(key), key));
            }
        }
        order.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)));
        order.dedup_by_key(|entry| entry.2);
        order.into_iter().map(|(_, _, key)| key).collect()
    }

    /// The top-level chunk holding `key`, which must be a chunk to draw or
    /// hold one.
    fn root_of(&self, key: ChunkKey) -> ChunkKey {
        let mut at = key;
        while !self.roots.contains(&at) {
            at = at.parent().expect("every chunk to draw has a root");
        }
        at
    }

    /// How many chunks are being built.
    pub(crate) fn reading(&self) -> usize {
        self.reading.len()
    }

    /// How many of the chunks wanted (to draw, or within a build radius)
    /// are built, and how many are wanted.
    pub(crate) fn builds(&self) -> (usize, usize) {
        let wanted = self.target.union(&self.radius);
        let (mut built, mut all) = (0, 0);
        for key in wanted {
            all += 1;
            built += usize::from(self.built.contains_key(key));
        }
        (built, all)
    }

    /// How many chunks to draw there are.
    pub(crate) fn target_len(&self) -> usize {
        self.target.len()
    }

    /// How many chunks to draw are drawn by a coarser chunk for now.
    pub(crate) fn standing_in(&self) -> usize {
        self.target
            .iter()
            .filter(|key| !self.drawn.contains_key(key))
            .count()
    }

    /// Whether the drawn chunks are the chunks to draw, every chunk within
    /// a build radius is built, and nothing is being built.
    pub(crate) fn is_settled(&self) -> bool {
        self.reading.is_empty()
            && self.drawn_generation == self.generation
            && self.drawn.len() == self.target.len()
            && self.target.iter().all(|key| self.drawn.contains_key(key))
            && self.radius.iter().all(|key| self.built.contains_key(key))
    }

    /// Marks `key` as being built.
    pub(crate) fn start(&mut self, key: ChunkKey) {
        self.reading.insert(key);
    }

    /// Marks `key` as no longer being built, its build dropped before it
    /// started. It is then wanted again.
    pub(crate) fn cancel(&mut self, key: ChunkKey) {
        self.reading.remove(&key);
    }

    /// Keeps the result of building `key` in `generation`; a result from an
    /// earlier generation is dropped.
    pub(crate) fn finish(&mut self, key: ChunkKey, generation: u64, built: Built<R>) {
        if generation != self.generation {
            return;
        }
        self.reading.remove(&key);
        self.built.insert(key, built);
        self.stale = true;
    }

    /// Where `key` stands against the chunks to draw.
    fn place(&self, key: ChunkKey) -> Place {
        if self.target.contains(&key) {
            Place::Target
        } else if self.above.contains(&key) {
            Place::Above
        } else {
            Place::Outside
        }
    }

    /// Whether every part of `key` holding chunks to draw can be drawn from
    /// this generation's builds: from `key` itself, or from finer chunks.
    fn drawable(&self, key: ChunkKey) -> bool {
        match self.place(key) {
            Place::Outside => true,
            Place::Target => self.built.contains_key(&key),
            Place::Above => self.built.contains_key(&key) || self.children_drawable(key),
        }
    }

    /// Whether all four of `key`'s children can be drawn.
    fn children_drawable(&self, key: ChunkKey) -> bool {
        key.children()
            .is_some_and(|children| children.iter().all(|&c| self.drawable(c)))
    }

    /// Adds the chunks to draw for `key` to `next`: the finest that can be
    /// drawn, or the chunks drawn inside it now if it can't be drawn yet.
    fn choose(&self, key: ChunkKey, next: &mut BTreeMap<ChunkKey, Built<R>>) {
        match self.place(key) {
            Place::Outside => {}
            Place::Above if self.children_drawable(key) => {
                for child in key.children().expect("a chunk above others has children") {
                    self.choose(child, next);
                }
            }
            Place::Target | Place::Above => match self.built.get(&key) {
                Some(built) => {
                    next.insert(key, built.clone());
                }
                // Keep what is drawn inside it until it can be drawn.
                None => next.extend(
                    self.drawn
                        .range(..)
                        .filter(|(k, _)| k.is_within(key))
                        .map(|(&k, b)| (k, b.clone())),
                ),
            },
        }
    }

    /// The changes that bring the drawn chunks closer to the chunks to
    /// draw. `None` when nothing changes, which it skips working out when
    /// neither the target nor the builds changed since the last call. After
    /// a reset, nothing changes until every top-level chunk can be drawn.
    /// Builds no longer wanted or drawn are dropped.
    pub(crate) fn swap(&mut self) -> Option<Swap<R>> {
        if !std::mem::take(&mut self.stale) {
            return None;
        }
        let fresh = self.drawn_generation != self.generation;
        if fresh && !self.roots.iter().all(|&root| self.drawable(root)) {
            return None;
        }
        let mut next = BTreeMap::new();
        for &root in &self.roots {
            self.choose(root, &mut next);
        }

        // Chunks gone or drawn from another build go; chunks new or drawn
        // from another build come. After a reset every chunk is new.
        let same = |a: &Built<R>, b: &Built<R>| match (a, b) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        let remove: Vec<ChunkKey> = self
            .drawn
            .iter()
            .filter(|(key, built)| fresh || !next.get(key).is_some_and(|n| same(n, built)))
            .map(|(&key, _)| key)
            .collect();
        let add: Vec<(ChunkKey, Built<R>)> = next
            .iter()
            .filter(|(key, built)| fresh || !self.drawn.get(key).is_some_and(|d| same(d, built)))
            .map(|(&key, built)| (key, built.clone()))
            .collect();
        self.drawn = next;
        self.drawn_generation = self.generation;

        // Keep what is wanted, drawn or still needed as a stand-in.
        let needed_roots: HashSet<ChunkKey> = self
            .target
            .iter()
            .filter(|key| !self.built.contains_key(key))
            .map(|&key| self.root_of(key))
            .collect();
        let (target, radius, drawn) = (&self.target, &self.radius, &self.drawn);
        let keep = |key: &ChunkKey| {
            target.contains(key)
                || radius.contains(key)
                || drawn.contains_key(key)
                || needed_roots.contains(key)
        };
        self.built.retain(|key, _| keep(key));

        (!remove.is_empty() || !add.is_empty()).then_some(Swap { remove, add })
    }

    /// The chunks drawn now, each with its build.
    pub(crate) fn drawn(&self) -> impl Iterator<Item = (ChunkKey, &Built<R>)> + '_ {
        self.drawn.iter().map(|(&key, built)| (key, built))
    }

    /// The build drawn for `key`, if `key` is drawn and built.
    pub(crate) fn drawn_of(&self, key: ChunkKey) -> Option<&Arc<R>> {
        self.drawn.get(&key)?.as_ref()
    }

    /// Every chunk built in this generation, drawn or not.
    pub(crate) fn built(&self) -> impl Iterator<Item = (ChunkKey, &Arc<R>)> + '_ {
        self.built
            .iter()
            .filter_map(|(&key, built)| Some((key, built.as_ref()?)))
    }

    /// The drawn chunks that touch `key` along a side or at a corner.
    pub(crate) fn touching(&self, key: ChunkKey) -> Vec<ChunkKey> {
        let (lo, hi) = (key.min_corner(), key.max_corner());
        self.drawn
            .keys()
            .copied()
            .filter(|&other| {
                let (a, b) = (other.min_corner(), other.max_corner());
                other != key && a.x <= hi.x && a.y <= hi.y && b.x >= lo.x && b.y >= lo.y
            })
            .collect()
    }
}

impl<R: Ground> LevelStream<R> {
    /// The drawn ground's height under `p`, if a built chunk is drawn
    /// there.
    pub(crate) fn ground_height(&self, p: DVec2) -> Option<f32> {
        let top = self.drawn.keys().map(|k| k.level).max()?;
        (0..=top).find_map(|level| self.drawn_of(ChunkKey::containing(p, level))?.height(p))
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use procgena_terrain::ChunkLevels;

    use super::*;

    /// A built chunk whose ground is flat at the height of its level, so a
    /// test can tell which chunk answered.
    struct Flat(ChunkKey);

    impl Ground for Flat {
        fn height(&self, p: DVec2) -> Option<f32> {
            let (lo, hi) = (self.0.min_corner(), self.0.max_corner());
            (p.cmpge(lo).all() && p.cmplt(hi).all()).then_some(f32::from(self.0.level))
        }
    }

    /// Chunk levels switching at 96, 192 and 384 m, drawn to 700 m.
    fn levels() -> ChunkLevels {
        ChunkLevels::new([96.0, 192.0, 384.0], 0.1, 700.0)
    }

    /// The chunks to draw around a viewer at `x`, 2 m above the ground.
    fn select(x: f64) -> ChunkSelection {
        ChunkSelection::around(&levels(), 1.0, &[DVec3::new(x, 2.0, 0.0)], None).unwrap()
    }

    /// A stream wanting the chunks around a viewer at the origin, with
    /// `radii`.
    fn stream(radii: &[f32]) -> LevelStream<Flat> {
        let mut stream = LevelStream::default();
        let radius = BuildRadii {
            radii: radii.to_vec(),
        }
        .around(1.0, &[DVec2::ZERO], &BTreeSet::new());
        stream.retarget(&select(0.0), radius);
        stream
    }

    /// Builds `key` now.
    fn build(stream: &mut LevelStream<Flat>, key: ChunkKey) {
        stream.start(key);
        let generation = stream.generation;
        stream.finish(key, generation, Some(Arc::new(Flat(key))));
    }

    /// Builds every wanted chunk, in order.
    fn build_all(stream: &mut LevelStream<Flat>) {
        for key in stream.wanted(&[DVec2::ZERO]) {
            build(stream, key);
        }
    }

    /// Whether no two drawn chunks overlap.
    fn no_overlaps(stream: &LevelStream<Flat>) -> bool {
        let drawn: Vec<ChunkKey> = stream.drawn().map(|(k, _)| k).collect();
        drawn
            .iter()
            .all(|a| drawn.iter().all(|b| a == b || !a.is_within(*b)))
    }

    #[test]
    fn radii_take_chunks_near_the_viewer_and_keep_them_a_little_past() {
        let radii = BuildRadii {
            radii: vec![100.0, 300.0],
        };
        let near = radii.around(1.0, &[DVec2::ZERO], &BTreeSet::new());
        assert!(near.iter().all(|k| k.level <= 1));
        assert!(
            near.iter()
                .all(|k| k.distance_to(DVec2::ZERO) < radii.radii[usize::from(k.level)].into())
        );
        assert!(near.contains(&ChunkKey::new(0, 0, 0)) && near.contains(&ChunkKey::new(-1, -1, 1)));

        // Half the factor takes fewer chunks.
        let low = radii.around(0.5, &[DVec2::ZERO], &BTreeSet::new());
        assert!(low.len() < near.len() && low.is_subset(&near));

        // Moving 105 m keeps a level-0 chunk now 105–110 m away, but
        // doesn't take one there afresh.
        let moved = DVec2::new(105.0, 0.0);
        let kept = radii.around(1.0, &[moved], &near);
        let fresh = radii.around(1.0, &[moved], &BTreeSet::new());
        let far = ChunkKey::new(-1, 0, 0);
        assert!((100.0..110.0).contains(&far.distance_to(moved)));
        assert!(kept.contains(&far) && !fresh.contains(&far));
    }

    #[test]
    fn a_cold_view_draws_its_top_level_first_and_then_refines() {
        let mut stream = stream(&[]);
        let wanted = stream.wanted(&[DVec2::ZERO]);
        let top = wanted[0].level;
        let roots = wanted.iter().take_while(|k| k.level == top).count();
        assert!(wanted[..roots].iter().all(|k| stream.roots.contains(k)));
        assert_eq!(roots, stream.roots.len());

        // On a first load each top-level chunk is drawn as soon as it is
        // built (after a reset, see below, they wait for each other).
        for (i, &key) in wanted[..roots].iter().enumerate() {
            build(&mut stream, key);
            let swap = stream.swap().unwrap();
            assert!(swap.remove.is_empty());
            assert_eq!(swap.add.len(), 1);
            assert_eq!(stream.drawn().count(), i + 1);
        }
        assert!(stream.drawn().all(|(k, _)| k.level == top));
        assert!(stream.standing_in() > 0);

        // Building a chunk alone changes nothing; its three siblings with it
        // replace their parent.
        let near = *stream.target.iter().find(|k| k.level == 0).unwrap();
        let siblings = near.parent().unwrap().children().unwrap();
        assert!(siblings.iter().all(|k| stream.target.contains(k)));
        let mut drawn_before: Vec<ChunkKey> = stream.drawn().map(|(k, _)| k).collect();
        build(&mut stream, siblings[0]);
        stream.swap();
        assert_eq!(
            stream.drawn().map(|(k, _)| k).collect::<Vec<_>>(),
            drawn_before
        );

        // Build the rest, refining as we go: the drawn chunks never overlap.
        for key in stream.wanted(&[DVec2::ZERO]) {
            build(&mut stream, key);
            stream.swap();
            assert!(no_overlaps(&stream));
            drawn_before = stream.drawn().map(|(k, _)| k).collect();
        }
        assert_eq!(
            drawn_before,
            stream.target.iter().copied().collect::<Vec<_>>()
        );
        assert!(stream.is_settled());
        assert_eq!(stream.standing_in(), 0);
        // Stand-in-only top-level chunks are dropped once settled.
        let built: BTreeSet<ChunkKey> = stream.built().map(|(k, _)| k).collect();
        assert_eq!(built, stream.target);
    }

    #[test]
    fn chunks_within_a_radius_are_built_after_those_drawn() {
        let mut stream = stream(&[100.0, 300.0]);
        let wanted = stream.wanted(&[DVec2::ZERO]);
        let first_radius_only = wanted
            .iter()
            .position(|k| {
                !stream.target.contains(k) && !stream.roots.contains(k) && !stream.above.contains(k)
            })
            .unwrap();
        assert!(
            wanted[first_radius_only..]
                .iter()
                .all(|k| !stream.target.contains(k))
        );
        build_all(&mut stream);
        stream.swap();
        assert!(stream.is_settled());
        // Built but not drawn: level-1 chunks within 300 m under the level-0
        // chunks drawn there.
        let hidden = stream
            .built()
            .filter(|(k, _)| !stream.drawn.contains_key(k))
            .count();
        assert!(hidden > 0);
        assert!(
            stream
                .built()
                .any(|(k, _)| k.level == 1 && !stream.drawn.contains_key(&k))
        );
    }

    #[test]
    fn a_failed_chunk_is_drawn_as_a_hole_and_holds_nothing_up() {
        let mut stream = stream(&[]);
        let wanted = stream.wanted(&[DVec2::ZERO]);
        let failed = *wanted
            .iter()
            .rev()
            .find(|k| stream.target.contains(k))
            .unwrap();
        for &key in &wanted {
            if key == failed {
                stream.start(key);
                let generation = stream.generation;
                stream.finish(key, generation, None);
            } else {
                build(&mut stream, key);
            }
        }
        stream.swap();
        assert!(stream.is_settled());
        assert!(stream.drawn.get(&failed).is_some_and(Option::is_none));
    }

    #[test]
    fn a_reset_keeps_the_old_ground_until_the_new_top_level_is_built() {
        let mut stream = stream(&[]);
        build_all(&mut stream);
        stream.swap();
        let old: Vec<ChunkKey> = stream.drawn().map(|(k, _)| k).collect();

        stream.reset();
        // An old build finishing now is dropped.
        let generation = stream.generation - 1;
        stream.finish(old[0], generation, Some(Arc::new(Flat(old[0]))));
        assert!(stream.built.is_empty());
        let wanted = stream.wanted(&[DVec2::ZERO]);
        let roots = stream.roots.len();
        for &key in &wanted[..roots - 1] {
            build(&mut stream, key);
        }
        assert!(stream.swap().is_none());
        assert_eq!(stream.drawn().map(|(k, _)| k).collect::<Vec<_>>(), old);

        // Then every old chunk goes at once.
        build(&mut stream, wanted[roots - 1]);
        let swap = stream.swap().unwrap();
        assert_eq!(swap.remove, old);
        assert_eq!(swap.add.len(), roots);
    }

    #[test]
    fn moving_on_keeps_drawn_ground_until_the_new_chunks_can_replace_it() {
        let mut stream = stream(&[]);
        build_all(&mut stream);
        stream.swap();
        assert!(stream.is_settled());

        // Move far enough that new top-level chunks come in.
        stream.retarget(&select(600.0), BTreeSet::new());
        let viewer = [DVec2::new(600.0, 0.0)];
        let wanted = stream.wanted(&viewer);
        assert!(!wanted.is_empty());
        stream.swap();
        assert!(no_overlaps(&stream));
        for key in wanted {
            build(&mut stream, key);
            stream.swap();
            assert!(no_overlaps(&stream));
        }
        assert!(stream.is_settled());
    }

    #[test]
    fn a_cancelled_build_is_wanted_again() {
        let mut stream = stream(&[]);
        let wanted = stream.wanted(&[DVec2::ZERO]);
        stream.start(wanted[0]);
        assert_eq!(stream.wanted(&[DVec2::ZERO]), wanted[1..]);
        stream.cancel(wanted[0]);
        assert_eq!(stream.wanted(&[DVec2::ZERO]), wanted);
        assert_eq!(stream.reading(), 0);
    }

    #[test]
    fn the_ground_height_is_the_drawn_chunks() {
        let mut stream = stream(&[100.0]);
        build_all(&mut stream);
        stream.swap();
        // Level 0 is drawn near the viewer, though level 0 and others are
        // built there too.
        assert_eq!(stream.ground_height(DVec2::new(5.0, 5.0)), Some(0.0));
        let far = stream
            .drawn()
            .map(|(k, _)| k)
            .max_by_key(|k| k.level)
            .unwrap();
        let inside = far.min_corner() + DVec2::splat(1.0);
        assert_eq!(stream.ground_height(inside), Some(f32::from(far.level)));
        assert_eq!(stream.ground_height(DVec2::new(1.0e6, 0.0)), None);
    }
}
