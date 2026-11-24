//! A small Scatter that follows the shared placement rules, used to test the
//! trait's provided methods and the cache.

use glam::{Affine3A, DVec2, Vec2, Vec3};

use super::*;
use crate::address::{AddressPrefix, RegionId};
use crate::ground::FlatGround;
use crate::scatter::lattice::{
    Fixed2, Lattice, Rank, Spaced, chance, dense_enough, keep_spaced, rank,
};
use crate::scatter::placing::{Reach, ScatterSlot};
use crate::scatter::{Disc, ScatterArea};

/// Pegs 1.5 m apart on 1 m cells, each 0.3 m across.
struct Pegs;

struct Candidate {
    position: Fixed2,
    rank: Rank,
}

impl Spaced for Candidate {
    fn position(&self) -> Fixed2 {
        self.position
    }
    fn rank(&self) -> Rank {
        self.rank
    }
}

const RADIUS: f32 = 0.3;

impl Scatter for Pegs {
    fn lattice(&self) -> Lattice {
        Lattice::new(1.0)
    }

    fn reach(&self) -> Reach {
        Reach {
            rounds: 5,
            spacing: 1.5,
            displacement: 1.0,
            footprint: RADIUS,
        }
    }

    fn draw_range(&self) -> f32 {
        50.0
    }

    fn slots(&self, _world_seed: u32) -> Vec<ScatterSlot> {
        Vec::new()
    }

    fn place(&self, input: &ScatterInput<'_>) -> Result<Vec<ScatterCopy>, ScatterError> {
        let lattice = self.lattice();
        let own = input.area.bounds();
        let p = chance(0.5);
        let candidates: Vec<Candidate> = lattice
            .cells_in(self.reach().window(own))
            .filter_map(|cell| {
                let h = lattice.hash(input.seed, cell);
                let position = lattice.position(h, cell);
                let local = input.local_density(position);
                let r = rank(h);
                let disc = Disc::new(position.to_world(), RADIUS);
                (dense_enough(r, p, local) && !input.excluded(&[disc])).then_some(Candidate {
                    position,
                    rank: Rank {
                        tier: 0,
                        rank: r,
                        local,
                        cell,
                        index: 0,
                    },
                })
            })
            .collect();
        let spacing = self.reach().spacing_fixed();
        let keep = keep_spaced(&candidates, 5, spacing, |a, b| {
            a.position.distance_squared(b.position) < i128::from(spacing * spacing)
        });
        Ok(candidates
            .iter()
            .zip(keep)
            .filter(|(c, k)| *k && own.contains(lattice.corner(c.rank.cell)))
            .map(|(c, _)| {
                let local = c.position.relative_to(input.area.centre);
                ScatterCopy {
                    address: input.prefix.copy(c.rank.cell, 0),
                    slot: 0,
                    transform: Affine3A::from_translation(Vec3::new(local.x, 0.0, local.y)),
                    footprint: vec![Disc::new(c.position.to_world(), RADIUS)],
                }
            })
            .collect())
    }
}

fn input(area: ScatterArea) -> ScatterInput<'static> {
    ScatterInput::new(&FlatGround, area, 3)
}

fn world(copy: &ScatterCopy, area: &ScatterArea) -> DVec2 {
    let t = copy.transform.translation;
    area.centre + DVec2::new(f64::from(t.x), f64::from(t.z))
}

#[test]
fn chunks_in_any_order_give_the_copies_of_the_whole_area() {
    let whole = ScatterArea::new(DVec2::new(1000.5, -20.0), Vec2::splat(48.0));
    let expected = Pegs.place(&input(whole)).unwrap();
    assert!(expected.len() > 100);

    let grid = ChunkGrid::new(16.0);
    let mut chunks = grid.chunks_over(whole.bounds());
    chunks.reverse();
    let mut cache = ChunkCache::new(grid);
    let mut got = Vec::new();
    for chunk in chunks {
        let area = grid.area(chunk);
        for copy in cache.chunk(&Pegs, &input(whole), chunk).unwrap().iter() {
            if whole
                .bounds()
                .contains(Pegs.lattice().corner(copy.address.cell))
            {
                got.push((copy.address, world(copy, &area)));
            }
        }
    }
    got.sort_by_key(|(a, _)| *a);
    let mut want: Vec<_> = expected
        .iter()
        .map(|c| (c.address, world(c, &whole)))
        .collect();
    want.sort_by_key(|(a, _)| *a);
    assert_eq!(got.len(), want.len());
    for ((a, p), (b, q)) in got.iter().zip(&want) {
        assert_eq!(a, b);
        assert!(p.distance(*q) < 1e-4, "{p} vs {q}");
    }
}

#[test]
fn resolve_finds_each_copy_and_nothing_in_empty_cells() {
    let area = ScatterArea::new(DVec2::new(-30.0, 7.0), Vec2::splat(20.0));
    let placed = Pegs.place(&input(area)).unwrap();
    let mut cache = ChunkCache::default();
    let addresses: Vec<_> = placed.iter().map(|c| c.address).collect();
    let found = cache.resolve_all(&Pegs, &input(area), &addresses).unwrap();
    for (copy, found) in placed.iter().zip(found) {
        let found = found.expect("every placed copy resolves");
        assert_eq!(found.address, copy.address);
        let gap = found.transform.translation - copy.transform.translation;
        assert!(gap.length() < 1e-4);
        // One at a time gives the same.
        let one = Pegs.resolve(&input(area), &copy.address).unwrap().unwrap();
        assert!((one.transform.translation - copy.transform.translation).length() < 1e-4);
    }

    // An empty cell inside the area, and an address of another Scatter.
    let lattice = Pegs.lattice();
    let empty = lattice
        .cells_in(area.bounds())
        .find(|cell| placed.iter().all(|c| c.address.cell != *cell))
        .unwrap();
    let other = AddressPrefix {
        region: RegionId(4),
        ..AddressPrefix::ALONE
    };
    let missing = [
        AddressPrefix::ALONE.copy(empty, 0),
        other.copy(placed[0].address.cell, 0),
    ];
    let found = cache.resolve_all(&Pegs, &input(area), &missing).unwrap();
    assert_eq!(found, vec![None, None]);
}

#[test]
fn footprints_above_keep_copies_clear_exactly() {
    // Pegs below avoid the pegs of another seed above them.
    let area = ScatterArea::new(DVec2::ZERO, Vec2::splat(30.0));
    let above_input = ScatterInput::new(&FlatGround, area, 99);
    let above = Pegs.footprints(&above_input).unwrap();
    assert!(!above.is_empty());
    let free = Pegs.place(&input(area)).unwrap();
    let below = Pegs.place(&input(area).with_exclusion(&above)).unwrap();
    assert!(below.len() < free.len());
    for copy in &below {
        assert!(!above.overlaps_any(&copy.footprint));
    }
    // Without the exclusion, some would overlap.
    assert!(free.iter().any(|c| above.overlaps_any(&c.footprint)));
}

#[test]
fn the_mask_is_stamped_from_footprints_without_drawing() {
    let area = ScatterArea::new(DVec2::new(5.0, 5.0), Vec2::splat(10.0));
    let mask = Pegs.mask(&input(area)).unwrap();
    let copies = Pegs.place(&input(area)).unwrap();
    // Every 0.5 m texel of the area whose centre lies in a peg is cleared,
    // and only those, counting pegs placed just outside the area too.
    let footprints = Pegs.footprints(&input(area)).unwrap();
    assert!(footprints.discs().len() > copies.len());
    let mut cleared = 0;
    for x in 0..20 {
        for z in 0..20 {
            let centre = Fixed2::new(x * 128 + 64, z * 128 + 64);
            let inside = footprints.covers(centre);
            assert_eq!(mask.sample(centre) == 0, inside, "texel ({x}, {z})");
            cleared += usize::from(inside);
        }
    }
    assert!(cleared > 0);
    // The cache gives the same footprints from its chunks.
    let mut cache = ChunkCache::new(ChunkGrid::new(8.0));
    let cached = cache
        .footprints(&Pegs, &input(area), area.bounds())
        .unwrap();
    for disc in Pegs.footprints(&input(area)).unwrap().discs() {
        if area.bounds().grow(disc.radius).contains(disc.centre) {
            assert!(cached.discs().contains(disc));
        }
    }
}

#[test]
fn a_copy_past_its_chunk_edge_belongs_to_one_chunk() {
    // Chunks that the 1 m cells don't divide, so some cells straddle an edge.
    let grid = ChunkGrid::new(8.3);
    let mut cache = ChunkCache::new(grid);
    let whole = ScatterArea::new(DVec2::ZERO, Vec2::splat(32.0));
    let mut seen = std::collections::HashSet::new();
    let mut past_edge = 0;
    for chunk in grid.chunks_over(whole.bounds()) {
        let rect = grid.rect(chunk);
        for copy in cache.chunk(&Pegs, &input(whole), chunk).unwrap().iter() {
            assert!(seen.insert(copy.address), "a copy in two chunks");
            let at = Fixed2::from_world(world(copy, &grid.area(chunk)));
            if !rect.contains(at) {
                past_edge += 1;
            }
        }
    }
    assert!(past_edge > 0, "no copy stood past its chunk's edge");
}
