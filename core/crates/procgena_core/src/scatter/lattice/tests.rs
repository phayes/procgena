use super::*;

/// A test candidate: a lattice spot with its own spacing.
#[derive(Clone, Copy, Debug)]
struct Spot {
    position: Fixed2,
    rank: Rank,
    spacing: i64,
}

impl Spaced for Spot {
    fn position(&self) -> Fixed2 {
        self.position
    }
    fn rank(&self) -> Rank {
        self.rank
    }
}

fn conflict(a: &Spot, b: &Spot) -> bool {
    let s = i128::from(a.spacing.max(b.spacing));
    a.position.distance_squared(b.position) < s * s
}

/// A dense mix of two species on 0.5 m cells: large ones 2.5 m apart and
/// small ones 0.6 m apart, under an uneven mask.
struct Mix {
    lattice: Lattice,
    seed: u32,
    chance: u32,
}

const LARGE: i64 = 640; // 2.5 m
const SMALL: i64 = 154; // 0.6 m

impl Mix {
    fn new() -> Self {
        Self {
            lattice: Lattice::new(0.5),
            seed: 11,
            chance: chance(0.6),
        }
    }

    fn local(p: Fixed2) -> u8 {
        let w = p.to_world();
        quantise((0.6 + 0.4 * (w.x * 0.05).sin() * (w.y * 0.07).cos()) as f32)
    }

    /// The candidates whose corner lies in `rect` that pass the density
    /// test.
    fn candidates(&self, rect: FixedRect) -> Vec<Spot> {
        self.lattice
            .cells_in(rect)
            .filter_map(|cell| {
                let h = self.lattice.hash(self.seed, cell);
                let position = self.lattice.position(h, cell);
                let local = Self::local(position);
                let r = rank(h);
                dense_enough(r, self.chance, local).then(|| Spot {
                    position,
                    rank: Rank {
                        tier: 0,
                        rank: r,
                        local,
                        cell,
                        index: 0,
                    },
                    spacing: if unit(h, 7) < 0.4 { LARGE } else { SMALL },
                })
            })
            .collect()
    }

    /// The kept candidates whose corner lies in `own`, from candidates
    /// spaced over `own` grown by `margin`.
    fn kept(&self, own: FixedRect, margin: i64, rounds: u32) -> Vec<CellId> {
        let window = self.candidates(own.grow(margin));
        let keep = keep_spaced(&window, rounds, LARGE, conflict);
        let mut cells: Vec<CellId> = window
            .iter()
            .zip(keep)
            .filter(|(s, k)| *k && own.contains(self.lattice.corner(s.rank.cell)))
            .map(|(s, _)| s.rank.cell)
            .collect();
        cells.sort();
        cells
    }
}

fn square(min: i64, side: i64) -> FixedRect {
    FixedRect::new(Fixed2::new(min, min), Fixed2::new(min + side, min + side))
}

/// Greedy spacing in rank order: the rule `K_d` approximates.
fn greedy(spots: &[Spot]) -> Vec<bool> {
    let mut order: Vec<usize> = (0..spots.len()).collect();
    order.sort_by_key(|&i| spots[i].rank);
    let mut kept = vec![false; spots.len()];
    let mut chosen: Vec<usize> = Vec::new();
    for i in order {
        if chosen.iter().all(|&j| !conflict(&spots[i], &spots[j])) {
            kept[i] = true;
            chosen.push(i);
        }
    }
    kept
}

#[test]
fn fixed_point_round_trips_through_areas() {
    let rect = square(-1000, 16_384);
    assert_eq!(rect.area().bounds(), rect);
    assert_eq!(
        Fixed2::from_world(DVec2::new(-1.5, 2.0)),
        Fixed2::new(-384, 512)
    );
    assert_eq!(to_metres(384), 1.5);
}

#[test]
fn cells_in_a_rect_are_those_whose_corner_it_holds() {
    let lattice = Lattice::from_fixed(100);
    let rect = FixedRect::new(Fixed2::new(-250, 0), Fixed2::new(100, 101));
    let cells: Vec<CellId> = lattice.cells_in(rect).collect();
    // Corners -200, -100, 0 along X; 0 and 100 along Z.
    assert_eq!(cells.len(), 6);
    assert_eq!(lattice.count_in(rect), 6);
    for cell in cells {
        assert!(rect.contains(lattice.corner(cell)));
    }
    assert_eq!(lattice.cell_of(Fixed2::new(-1, 99)), CellId::new(-1, 0));
}

#[test]
fn candidates_stand_inside_their_cell() {
    let lattice = Lattice::new(0.7);
    for x in -20..20 {
        for z in -3..3 {
            let cell = CellId::new(x, z);
            let p = lattice.position(lattice.hash(5, cell), cell);
            assert_eq!(lattice.cell_of(p), cell);
        }
    }
}

#[test]
fn a_lower_chance_or_share_keeps_a_subset() {
    for h in 0..20_000u32 {
        let r = rank(pcg(h));
        let full = dense_enough(r, chance(0.5), 255);
        assert!(!dense_enough(r, chance(0.3), 255) || full);
        assert!(!dense_enough(r, chance(0.5), 100) || full);
    }
    assert!(!dense_enough(0, 0, 255));
    assert!(dense_enough(CHANCE_ONE - 1, chance(1.0), 255));
    assert_eq!(quantise(f32::NAN), 0);
    assert_eq!(quantise(2.0), 255);
}

#[test]
fn ranks_order_by_tier_then_rank_over_local_then_cell() {
    let at = |tier, rank, local, x| Rank {
        tier,
        rank,
        local,
        cell: CellId::new(x, 0),
        index: 0,
    };
    // A thinner mask ranks a candidate higher: 100/255 < 60/100.
    assert!(at(0, 100, 255, 0) < at(0, 60, 100, 0));
    assert!(at(0, 9_000, 255, 0) < at(1, 1, 255, 0));
    assert!(at(0, 10, 100, 1) < at(0, 10, 100, 2));
    assert_eq!(at(0, 20, 200, 1).cmp(&at(0, 10, 100, 1)), Ordering::Equal);
}

#[test]
fn odd_rounds_never_break_spacing_and_keep_a_subset_of_greedy() {
    let mix = Mix::new();
    let spots = mix.candidates(square(0, 40 * 256));
    let greedy = greedy(&spots);
    for rounds in [1, 3, 5, 7] {
        let kept = keep_spaced(&spots, rounds, LARGE, conflict);
        for (i, a) in spots.iter().enumerate() {
            if !kept[i] {
                continue;
            }
            assert!(greedy[i], "K_{rounds} kept a spot greedy drops");
            for (j, b) in spots.iter().enumerate() {
                assert!(
                    i == j || !kept[j] || !conflict(a, b),
                    "K_{rounds} broke spacing"
                );
            }
        }
    }
    // K_5 is close to greedy.
    let k5 = keep_spaced(&spots, 5, LARGE, conflict);
    let (g, k) = (
        greedy.iter().filter(|k| **k).count(),
        k5.iter().filter(|k| **k).count(),
    );
    assert!(k as f32 > 0.99 * g as f32, "K_5 keeps {k} of greedy's {g}");
}

#[test]
#[should_panic(expected = "odd number")]
fn even_rounds_are_refused() {
    let _ = keep_spaced::<Spot>(&[], 4, 1, |_, _| false);
}

#[test]
fn chunks_with_the_margin_match_the_whole_area_in_any_order() {
    let mix = Mix::new();
    let rounds = 5;
    // The margin: rounds × R, plus a cell of jitter on each side.
    let margin = i64::from(rounds) * LARGE + 2 * mix.lattice.cell_fixed();
    let whole = square(-32 * 256, 64 * 256);
    let expected = mix.kept(whole, margin, rounds);
    assert!(expected.len() > 1000);

    for side in [16, 32] {
        let grid = ChunkGrid::new(side as f32);
        let mut chunks = grid.chunks_over(whole);
        // Build them in reverse, as a different order than row by row.
        chunks.reverse();
        let mut got: Vec<CellId> = chunks
            .iter()
            .flat_map(|&c| mix.kept(grid.rect(c), margin, rounds))
            .collect();
        got.sort();
        assert_eq!(got, expected, "chunks of {side} m");
    }
}

#[test]
fn chunks_without_the_margin_differ() {
    // The negative control for the test above: without a margin, spots near
    // chunk edges lose the neighbours that decide them.
    let mix = Mix::new();
    let whole = square(-32 * 256, 64 * 256);
    let margin = 5 * LARGE + 2 * mix.lattice.cell_fixed();
    let expected = mix.kept(whole, margin, 5);
    let grid = ChunkGrid::new(16.0);
    let mut got: Vec<CellId> = grid
        .chunks_over(whole)
        .iter()
        .flat_map(|&c| mix.kept(grid.rect(c), 0, 5))
        .collect();
    got.sort();
    assert_ne!(got, expected);
}

#[test]
fn chunks_tile_an_area_and_hold_its_corners() {
    let grid = ChunkGrid::new(64.0);
    let rect = FixedRect::new(Fixed2::new(-1, -1), Fixed2::new(64 * 256 + 1, 1));
    let chunks = grid.chunks_over(rect);
    assert_eq!(
        chunks,
        vec![
            ChunkId::new(-1, -1),
            ChunkId::new(0, -1),
            ChunkId::new(1, -1),
            ChunkId::new(-1, 0),
            ChunkId::new(0, 0),
            ChunkId::new(1, 0),
        ]
    );
    assert_eq!(grid.chunk_of(Fixed2::new(-1, 0)), ChunkId::new(-1, 0));
    assert_eq!(grid.area(ChunkId::new(0, 0)).centre, DVec2::new(32.0, 32.0));
}
