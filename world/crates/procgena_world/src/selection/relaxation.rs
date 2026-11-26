//! The regions a selection reads, and the passes of draws over them.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};

use procgena_terrain::{RegionId, RegionKey};

/// The six regions beside `region` on the hexagonal lattice (see
/// [`RegionKey::lattice_neighbours`]): the regions sharing an edge with it
/// in a weighted World.
pub(crate) fn lattice_neighbours(region: RegionId) -> [RegionId; 6] {
    RegionKey::from_id(region)
        .lattice_neighbours()
        .map(RegionKey::id)
}

/// Every region within some number of steps of a set of targets.
#[derive(Debug, Clone)]
pub(crate) struct Ball {
    /// The regions, in the order they were reached.
    pub ids: Vec<RegionId>,
    /// Each region's fewest steps to any target.
    pub dist: Vec<u32>,
    /// Each region's neighbours by index into `ids`, sorted by id. Empty
    /// for regions on the outer rim, whose neighbours are never read.
    pub neighbours: Vec<Vec<usize>>,
}

impl Ball {
    /// The regions at most `radius` steps from any of `targets`, with
    /// `neighbours` giving each region's neighbours. `neighbours` must be
    /// symmetric and give the same answer every time.
    pub fn new<N, I>(targets: &[RegionId], radius: u32, neighbours: N) -> Self
    where
        N: Fn(RegionId) -> I,
        I: IntoIterator<Item = RegionId>,
    {
        let mut ball = Self {
            ids: Vec::new(),
            dist: Vec::new(),
            neighbours: Vec::new(),
        };
        let mut index: HashMap<RegionId, usize> = HashMap::new();
        let mut queue = VecDeque::new();

        // Breadth first from every target at once, so each region gets
        // its fewest steps to any of them.
        for &t in targets {
            if let Entry::Vacant(slot) = index.entry(t) {
                slot.insert(ball.ids.len());
                queue.push_back(ball.ids.len());
                ball.ids.push(t);
                ball.dist.push(0);
                ball.neighbours.push(Vec::new());
            }
        }
        while let Some(i) = queue.pop_front() {
            if ball.dist[i] == radius {
                continue;
            }
            let mut around: Vec<RegionId> = neighbours(ball.ids[i]).into_iter().collect();
            around.sort_unstable();
            around.dedup();
            let mut links = Vec::with_capacity(around.len());
            for n in around {
                let j = *index.entry(n).or_insert_with(|| {
                    ball.ids.push(n);
                    ball.dist.push(ball.dist[i] + 1);
                    ball.neighbours.push(Vec::new());
                    queue.push_back(ball.ids.len() - 1);
                    ball.ids.len() - 1
                });
                links.push(j);
            }
            ball.neighbours[i] = links;
        }
        ball
    }

    /// Runs `passes` synchronous passes of draws over the ball and returns
    /// every region's last draw (meaningful only within `radius - passes`
    /// steps of a target; `passes` must not be above the ball's radius).
    ///
    /// `draw(i, pass, previous)` gives region `i`'s Biome on `pass`, from
    /// the previous pass's Biomes of every region (`previous` is empty on
    /// pass 0). Pass `t` draws only regions within `radius - t` steps, whose
    /// neighbours all drew on pass `t - 1`.
    pub fn relax(
        &self,
        radius: u32,
        passes: u32,
        mut draw: impl FnMut(usize, u32, &[usize]) -> usize,
    ) -> Vec<usize> {
        debug_assert!(passes <= radius);
        let mut previous: Vec<usize> = (0..self.ids.len()).map(|i| draw(i, 0, &[])).collect();
        let mut next = previous.clone();
        for pass in 1..=passes {
            for (i, (slot, &dist)) in next.iter_mut().zip(&self.dist).enumerate() {
                if dist <= radius - pass {
                    *slot = draw(i, pass, &previous);
                }
            }
            std::mem::swap(&mut previous, &mut next);
        }
        previous
    }
}

#[cfg(test)]
mod tests {
    use glam::IVec2;

    use super::*;

    fn key(x: i32, z: i32) -> RegionId {
        RegionKey(IVec2::new(x, z)).id()
    }

    #[test]
    fn the_ball_has_hexagonal_rings() {
        // 1 + 3r(r + 1) regions within r steps.
        for r in 0..6 {
            let ball = Ball::new(&[key(3, -7)], r, lattice_neighbours);
            assert_eq!(ball.ids.len() as u32, 1 + 3 * r * (r + 1));
            for (i, &d) in ball.dist.iter().enumerate() {
                assert_eq!(ball.neighbours[i].len(), if d < r { 6 } else { 0 });
            }
        }
    }

    #[test]
    fn a_ball_of_several_targets_takes_the_nearest() {
        let a = key(0, 0);
        let b = key(4, 0);
        let ball = Ball::new(&[a, b, a], 2, lattice_neighbours);
        let dist = |id| ball.dist[ball.ids.iter().position(|&x| x == id).unwrap()];
        assert_eq!(dist(a), 0);
        assert_eq!(dist(b), 0);
        assert_eq!(dist(key(2, 0)), 2);
        assert_eq!(dist(key(3, 0)), 1);
        assert!(!ball.ids.contains(&key(7, 0)));
    }
}
