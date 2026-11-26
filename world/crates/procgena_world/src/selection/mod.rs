//! Choosing each region's Biome: the field values at a region, and weighted
//! selection over the hexagonal lattice of regions.

mod compatibility;
mod relaxation;
mod sampling;
mod suitability;

use std::collections::HashMap;

use glam::DVec2;
use procgena_terrain::{
    Elevation, NoiseLayer, RegionGraph, RegionGraphError, RegionGraphOptions, RegionId, RegionKey,
};

use crate::options::BiomeSelectionOptions;
use crate::world::FieldValues;

pub(crate) use relaxation::lattice_neighbours;
#[cfg(test)]
pub(crate) use sampling::SELECTION;

use compatibility::Affinity;
use relaxation::Ball;
use suitability::Candidate;

/// The World's fields: E and the selector fields, sampled anywhere.
#[derive(Debug, Clone)]
pub(crate) struct FieldSampler {
    /// The world elevation E, the same field the World's land stands on.
    pub elevation: Elevation,
    /// The ruggedness field's noise and seed.
    pub ruggedness: (NoiseLayer, u32),
    /// The moisture field's noise and seed.
    pub moisture: (NoiseLayer, u32),
}

impl FieldSampler {
    /// The field values at `p`.
    pub fn at(&self, p: DVec2) -> FieldValues {
        FieldValues {
            elevation: self.elevation.height(p),
            ruggedness: self.ruggedness.0.sample(p, self.ruggedness.1),
            moisture: self.moisture.0.sample(p, self.moisture.1),
        }
    }
}

/// The region graph of `regions` and `seed` without jitter: its sites are
/// the points of the hexagonal lattice.
///
/// # Errors
///
/// If `regions` aren't valid.
pub(crate) fn lattice(
    regions: RegionGraphOptions,
    seed: u32,
) -> Result<RegionGraph, RegionGraphError> {
    RegionGraph::new(
        RegionGraphOptions {
            jitter: 0.0,
            ..regions
        },
        seed,
    )
}

/// One region's Biome, as selection chose it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Selected {
    /// The Biome's index in the World's catalogue.
    pub biome: usize,
    /// Whether a pin chose it.
    pub pinned: bool,
    /// The field values selection read for the region.
    pub fields: FieldValues,
}

/// Weighted Biome selection, checked and ready to answer for any region.
#[derive(Debug, Clone)]
pub(crate) struct BiomeSelector {
    seed: u32,
    iterations: u32,
    temperature: f64,
    neighbour_strength: f64,
    /// The scale's vertical stretch, which elevation is divided by.
    vertical: f64,
    /// The Biomes free to be drawn, in key order: the order draws walk.
    candidates: Vec<Candidate>,
    affinity: Affinity,
    /// The catalogue index of each pinned region's Biome.
    pins: HashMap<RegionId, usize>,
    /// The region graph without jitter, whose sites are the lattice points
    /// the fields are read at.
    lattice: RegionGraph,
}

impl BiomeSelector {
    /// The selector for checked `options` in a World with seed `seed`,
    /// vertical scale `vertical`, a catalogue of `n` Biomes and `pins` by
    /// catalogue index. `lattice` is the World's region graph with no
    /// jitter (see [`lattice`]). `index` turns a Biome key into its
    /// catalogue index.
    ///
    /// # Errors
    ///
    /// The first key `index` refuses, with its error.
    pub fn new<E>(
        options: &BiomeSelectionOptions,
        seed: u32,
        lattice: RegionGraph,
        vertical: f64,
        n: usize,
        pins: HashMap<RegionId, usize>,
        mut index: impl FnMut(&str) -> Result<usize, E>,
    ) -> Result<Self, E> {
        let candidates = options
            .suitability
            .iter()
            .map(|(key, suits)| Ok(Candidate::new(index(key)?, suits)))
            .collect::<Result<Vec<_>, E>>()?;
        let affinity = Affinity::new(n, &options.compatibility, index)?;
        Ok(Self {
            seed,
            iterations: options.iterations,
            temperature: options.temperature,
            neighbour_strength: options.neighbour_strength,
            vertical,
            candidates,
            affinity,
            pins,
            lattice,
        })
    }

    /// The lattice point of `region`: where its fields are read. It
    /// doesn't depend on the region graph's jitter or on any Biome.
    pub fn site(&self, region: RegionId) -> DVec2 {
        self.lattice.site(RegionKey::from_id(region))
    }

    /// The Biome of `region`, with `fields` the World's fields.
    pub fn select(&self, region: RegionId, fields: &FieldSampler) -> Selected {
        self.select_many(&[region], fields)[0]
    }

    /// The Biome of each of `regions`, in the same order, repeats
    /// included. Each answer equals [`select`](Self::select) of that
    /// region alone; asking for many neighbouring regions at once shares
    /// the work.
    pub fn select_many(&self, regions: &[RegionId], fields: &FieldSampler) -> Vec<Selected> {
        let k = self.iterations;
        let ball = Ball::new(regions, k, lattice_neighbours);

        // Field values and the candidates' own scores, once per region.
        let values: Vec<FieldValues> = ball
            .ids
            .iter()
            .map(|&id| fields.at(self.site(id)))
            .collect();
        let own: Vec<Vec<f64>> = values
            .iter()
            .map(|v| {
                self.candidates
                    .iter()
                    .map(|c| c.score(v, self.vertical))
                    .collect()
            })
            .collect();

        // The passes: a pin always keeps its Biome; anything else draws
        // from its own scores plus its neighbours' previous draws.
        let mut scores = vec![0.0; self.candidates.len()];
        let last = ball.relax(k, k, |i, pass, previous| {
            let id = ball.ids[i];
            if let Some(&pinned) = self.pins.get(&id) {
                return pinned;
            }
            self.score(&own[i], &ball.neighbours[i], previous, &mut scores);
            self.draw(id, pass, &scores)
        });

        // Each target's place in the ball.
        let at: HashMap<RegionId, usize> = ball
            .ids
            .iter()
            .zip(&ball.dist)
            .enumerate()
            .filter(|(_, (_, d))| **d == 0)
            .map(|(i, (&id, _))| (id, i))
            .collect();
        regions
            .iter()
            .map(|id| {
                let i = at[id];
                Selected {
                    biome: last[i],
                    pinned: self.pins.contains_key(id),
                    fields: values[i],
                }
            })
            .collect()
    }

    /// Writes each candidate's score into `scores`: its own score `own`,
    /// plus the neighbour strength times the mean log weight of the
    /// candidate beside each of `neighbours`' Biomes in `previous`. With
    /// no neighbours (or none given, on pass 0) it is the own score.
    fn score(&self, own: &[f64], neighbours: &[usize], previous: &[usize], scores: &mut [f64]) {
        scores.copy_from_slice(own);
        if previous.is_empty() || neighbours.is_empty() || self.neighbour_strength == 0.0 {
            return;
        }
        let per = self.neighbour_strength / neighbours.len() as f64;
        for (score, candidate) in scores.iter_mut().zip(&self.candidates) {
            let sum: f64 = neighbours
                .iter()
                .map(|&j| self.affinity.ln(candidate.biome, previous[j]))
                .sum();
            *score += per * sum;
        }
    }

    /// The catalogue index drawn for `region` on `pass` from candidate
    /// `scores`.
    fn draw(&self, region: RegionId, pass: u32, scores: &[f64]) -> usize {
        let u = sampling::uniform(self.seed, region, pass);
        self.candidates[sampling::categorical(scores, self.temperature, u)].biome
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use glam::IVec2;
    use procgena_core::hash::seed_for;
    use procgena_terrain::{ElevationOptions, RegionGraphOptions, RegionId, RegionKey};

    use super::*;
    use crate::options::{
        BiomeSelectionOptions, BiomeSuitability, CompatibilityPair, SelectorFields,
    };

    /// The catalogue the tests choose from, in a deliberately unsorted
    /// order.
    const CATALOGUE: [&str; 4] = ["rolling_hills", "plains", "lakes", "mountains"];

    fn index(key: &str) -> Result<usize, String> {
        CATALOGUE
            .iter()
            .position(|&k| k == key)
            .ok_or_else(|| key.to_owned())
    }

    fn key(x: i32, z: i32) -> RegionId {
        RegionKey(IVec2::new(x, z)).id()
    }

    fn fields(seed: u32) -> FieldSampler {
        let f = SelectorFields::default();
        FieldSampler {
            elevation: Elevation::new(ElevationOptions::default(), seed).unwrap(),
            ruggedness: (
                f.ruggedness.layer(1.0),
                seed_for(seed, "test.field.ruggedness"),
            ),
            moisture: (f.moisture.layer(1.0), seed_for(seed, "test.field.moisture")),
        }
    }

    fn selector(
        options: &BiomeSelectionOptions,
        seed: u32,
        pins: HashMap<RegionId, usize>,
    ) -> BiomeSelector {
        options.validate().unwrap();
        let lattice = lattice(RegionGraphOptions::default(), seed).unwrap();
        BiomeSelector::new(options, seed, lattice, 1.0, CATALOGUE.len(), pins, index).unwrap()
    }

    /// Settings where neighbours count for a lot, so mistakes in the
    /// passes show.
    fn strong(iterations: u32) -> BiomeSelectionOptions {
        BiomeSelectionOptions {
            iterations,
            neighbour_strength: 4.0,
            temperature: 1.0,
            ..BiomeSelectionOptions::default()
        }
    }

    /// Every region of a square patch of lattice cells, drawn pass by pass
    /// over the whole patch at once, the plain way: a region with a
    /// neighbour outside the patch keeps its previous draw. Returns the
    /// last draws.
    fn simulate(
        s: &BiomeSelector,
        f: &FieldSampler,
        centre: IVec2,
        half: i32,
    ) -> HashMap<RegionId, usize> {
        let patch: Vec<RegionId> = (-half..=half)
            .flat_map(|dz| (-half..=half).map(move |dx| key(centre.x + dx, centre.y + dz)))
            .collect();
        let own: HashMap<RegionId, Vec<f64>> = patch
            .iter()
            .map(|&id| {
                let v = f.at(s.site(id));
                (
                    id,
                    s.candidates
                        .iter()
                        .map(|c| c.score(&v, s.vertical))
                        .collect(),
                )
            })
            .collect();
        let draw = |id: RegionId, pass: u32, scores: &[f64]| {
            let u = sampling::uniform(s.seed, id, pass);
            s.candidates[sampling::categorical(scores, s.temperature, u)].biome
        };
        let mut state: HashMap<RegionId, usize> = patch
            .iter()
            .map(|&id| {
                (
                    id,
                    s.pins
                        .get(&id)
                        .copied()
                        .unwrap_or_else(|| draw(id, 0, &own[&id])),
                )
            })
            .collect();
        for pass in 1..=s.iterations {
            let mut next = state.clone();
            for &id in &patch {
                let around = lattice_neighbours(id);
                if s.pins.contains_key(&id) || !around.iter().all(|n| state.contains_key(n)) {
                    continue;
                }
                let scores: Vec<f64> = s
                    .candidates
                    .iter()
                    .zip(&own[&id])
                    .map(|(c, &u)| {
                        let sum: f64 = around
                            .iter()
                            .map(|n| s.affinity.ln(c.biome, state[n]))
                            .sum();
                        u + s.neighbour_strength / 6.0 * sum
                    })
                    .collect();
                next.insert(id, draw(id, pass, &scores));
            }
            state = next;
        }
        state
    }

    #[test]
    fn random_access_matches_the_whole_patch_simulated() {
        for (seed, centre) in [
            (0, IVec2::ZERO),
            (17, IVec2::new(-5, 3)),
            (99, IVec2::new(250_000, -333_333)),
        ] {
            for k in [0, 1, 3, 5] {
                let f = fields(seed);
                let pins = HashMap::from([(key(centre.x + 1, centre.y), 3)]);
                let s = selector(&strong(k), seed, pins);
                let half = 12;
                let state = simulate(&s, &f, centre, half);
                // Rim regions go stale one step further in each pass, so
                // only regions well inside the halo are compared.
                let inner = half - k as i32 - 2;
                let ids: Vec<RegionId> = (-inner..=inner)
                    .flat_map(|dz| {
                        (-inner..=inner).map(move |dx| key(centre.x + dx, centre.y + dz))
                    })
                    .collect();
                let many = s.select_many(&ids, &f);
                let mut differ = 0;
                for (id, got) in ids.iter().zip(&many) {
                    assert_eq!(got.biome, state[id], "seed {seed} k {k}");
                    assert_eq!(*got, s.select(*id, &f));
                    differ += usize::from(got.biome != many[0].biome);
                }
                assert!(differ > 0, "the patch should not be all one Biome");
            }
        }
    }

    #[test]
    fn neighbours_change_the_result() {
        // A check that the simulation compares something: with strong
        // neighbours the final Biomes differ from the first draws.
        let f = fields(5);
        let first = selector(&strong(0), 5, HashMap::new());
        let last = selector(&strong(3), 5, HashMap::new());
        let ids: Vec<RegionId> = (0..20)
            .flat_map(|z| (0..20).map(move |x| key(x, z)))
            .collect();
        let a = first.select_many(&ids, &f);
        let b = last.select_many(&ids, &f);
        assert!(a.iter().zip(&b).any(|(a, b)| a.biome != b.biome));
    }

    #[test]
    fn batches_agree_with_single_queries_in_any_order() {
        let f = fields(3);
        let s = selector(&strong(3), 3, HashMap::new());
        let ids = [key(4, 4), key(-9, 2), key(4, 4), key(5, 4), key(40, -40)];
        let one: Vec<Selected> = ids.iter().map(|&id| s.select(id, &f)).collect();
        assert_eq!(s.select_many(&ids, &f), one);
        let reversed: Vec<RegionId> = ids.iter().rev().copied().collect();
        let mut back = s.select_many(&reversed, &f);
        back.reverse();
        assert_eq!(back, one);
    }

    #[test]
    fn zero_iterations_draws_from_the_fields_alone() {
        let f = fields(8);
        let s = selector(&strong(0), 8, HashMap::new());
        for x in -10..10 {
            let id = key(x, 2 * x);
            let v = f.at(s.site(id));
            let scores: Vec<f64> = s.candidates.iter().map(|c| c.score(&v, 1.0)).collect();
            let u = sampling::uniform(8, id, 0);
            let expected = s.candidates[sampling::categorical(&scores, 1.0, u)].biome;
            assert_eq!(s.select(id, &f).biome, expected);
        }
    }

    #[test]
    fn without_neighbour_strength_the_last_pass_is_an_independent_draw() {
        let f = fields(8);
        let options = BiomeSelectionOptions {
            neighbour_strength: 0.0,
            ..strong(4)
        };
        let s = selector(&options, 8, HashMap::new());
        for x in -10..10 {
            let id = key(3 * x, -x);
            let v = f.at(s.site(id));
            let scores: Vec<f64> = s.candidates.iter().map(|c| c.score(&v, 1.0)).collect();
            let u = sampling::uniform(8, id, 4);
            let expected = s.candidates[sampling::categorical(&scores, 1.0, u)].biome;
            assert_eq!(s.select(id, &f).biome, expected);
        }
    }

    #[test]
    fn a_pin_holds_and_reaches_no_further_than_the_iterations() {
        let f = fields(21);
        let k = 3;
        let pinned = key(2, -1);
        // "lakes" is pinned only: it is never drawn freely.
        let lakes = index("lakes").unwrap();
        // Lakes sit well only beside mountains, so the pin pulls its
        // neighbours towards mountains.
        let mut options = strong(k);
        for (other, weight) in [
            ("plains", 0.01),
            ("rolling_hills", 0.01),
            ("mountains", 10.0),
        ] {
            options.compatibility.pairs.push(CompatibilityPair {
                a: "lakes".to_owned(),
                b: other.to_owned(),
                weight,
            });
        }
        let free = selector(&options, 21, HashMap::new());
        let held = selector(&options, 21, HashMap::from([(pinned, lakes)]));
        let ball = Ball::new(&[pinned], 12, lattice_neighbours);
        let a = free.select_many(&ball.ids, &f);
        let b = held.select_many(&ball.ids, &f);
        let mut changed = 0;
        for i in 0..ball.ids.len() {
            if ball.ids[i] == pinned {
                assert_eq!(b[i].biome, lakes);
                assert!(b[i].pinned);
                continue;
            }
            assert_ne!(a[i].biome, lakes);
            assert_ne!(b[i].biome, lakes);
            if a[i].biome != b[i].biome {
                changed += 1;
                assert!(ball.dist[i] <= k, "changed {} steps away", ball.dist[i]);
            }
        }
        assert!(changed > 0, "the pin should sway some neighbour");
    }

    #[test]
    fn listing_order_doesnt_matter() {
        // The same pairs listed in another order and either way round.
        let f = fields(4);
        let mut options = strong(3);
        let a = selector(&options, 4, HashMap::new());
        options.compatibility.pairs.reverse();
        for pair in &mut options.compatibility.pairs {
            std::mem::swap(&mut pair.a, &mut pair.b);
        }
        let b = selector(&options, 4, HashMap::new());
        let ids: Vec<RegionId> = (0..15)
            .flat_map(|z| (0..15).map(move |x| key(x, z)))
            .collect();
        assert_eq!(a.select_many(&ids, &f), b.select_many(&ids, &f));
    }

    #[test]
    fn one_candidate_is_always_drawn() {
        let f = fields(1);
        let options = BiomeSelectionOptions {
            suitability: BTreeMap::from([("plains".to_owned(), BiomeSuitability::default())]),
            ..strong(2)
        };
        let s = selector(&options, 1, HashMap::new());
        let plains = index("plains").unwrap();
        for x in -5..5 {
            assert_eq!(s.select(key(x, x), &f).biome, plains);
        }
    }

    #[test]
    fn selection_is_frozen() {
        // Golden Biomes of a row of regions under the default settings: a
        // change here changes every weighted World.
        let f = fields(0);
        let s = selector(&BiomeSelectionOptions::default(), 0, HashMap::new());
        let ids: Vec<RegionId> = (-8..8).map(|x| key(x, -x / 2)).collect();
        let got: Vec<&str> = s
            .select_many(&ids, &f)
            .iter()
            .map(|sel| CATALOGUE[sel.biome])
            .collect();
        assert_eq!(got, GOLDEN_ROW, "{got:?}");
    }

    const GOLDEN_ROW: [&str; 16] = [
        "plains",
        "rolling_hills",
        "rolling_hills",
        "plains",
        "plains",
        "plains",
        "plains",
        "plains",
        "rolling_hills",
        "rolling_hills",
        "plains",
        "rolling_hills",
        "rolling_hills",
        "rolling_hills",
        "rolling_hills",
        "plains",
    ];
}
