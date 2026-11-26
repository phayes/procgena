//! The Ecotope: one kind of ground cover, as Scatters laid over the same
//! ground in priority order.

mod file;
mod footprints;
mod presets;
mod scatters;

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use procgena_core::scatter::{Mask, ScatterInput};
use procgena_core::{ChildKey, child_seed};
use serde::{Deserialize, Serialize};

pub use footprints::{FootprintMask, Footprints};
pub use presets::EcotopePreset;
pub use scatters::{PLANT_CELL, ScatterCopies, ScatterFailure, ScatterKind};

/// The seed kind of a Scatter in an Ecotope: a Scatter keyed `key` in an
/// Ecotope with seed `seed` is placed with
/// `child_seed(seed, SCATTER_SEED_KIND, key)`. See [`scatter_seed`].
pub const SCATTER_SEED_KIND: &str = "ecotope.child.scatter";

/// The seed a Scatter keyed `key` is placed with in an Ecotope placed with
/// seed `ecotope_seed`.
///
/// It depends only on those two, so adding, removing, reordering or changing
/// other Scatters never changes it.
pub fn scatter_seed(ecotope_seed: u32, key: &ChildKey) -> u32 {
    child_seed(ecotope_seed, SCATTER_SEED_KIND, key)
}

/// One kind of ground cover with a single character, such as "pine forest
/// floor": a set of Scatters, each named by a frozen key and given a
/// priority, laid over the same ground.
///
/// Place it with [`place`](Self::place). The Scatter with the highest
/// priority is placed first. Each later Scatter keeps out of the footprints
/// of every Scatter above it, as far as its mask allows: a mask only thins
/// where copies are placed, so a wide copy, such as a log, can still reach
/// into a footprint above it.
///
/// This is an early Ecotope: it holds tree, stone, dead wood, mushroom and
/// plant Scatters, runs on any [`Ground`](procgena_core::Ground), and knows
/// nothing of Terrain. A plant Scatter leaves no footprints, and clears
/// every texel of [`PLANT_CELL`] metres that a footprint above it touches.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Ecotope {
    /// The seed to place this Ecotope with when it is placed on its own:
    /// pass it as the [`ScatterInput`]'s seed. Inside a Biome the seed comes
    /// from the Biome instead.
    pub default_seed: u32,
    /// The Scatters, by key. A key is the Scatter's identity: its seed comes
    /// from it, so renaming a key moves every copy of that Scatter.
    #[serde(with = "file::keyed")]
    pub scatters: BTreeMap<ChildKey, EcotopeScatter>,
}

/// One Scatter in an [`Ecotope`], with its place in the Ecotope's order.
#[derive(Debug, Clone, PartialEq)]
pub struct EcotopeScatter {
    /// Where the Scatter stands in the order: higher numbers are placed
    /// first, and lower ones keep out of them. No two Scatters of one
    /// Ecotope may share a priority.
    ///
    /// Priorities are on one scale for every Ecotope, so Ecotopes written
    /// apart still order their Scatters sensibly where they meet. For
    /// example: 900 and up for big obstacles such as trunks, the 800s for
    /// boulders and logs, the 600s for shrubs, and 100 to 300 for ground
    /// cover such as grass and mushrooms.
    pub priority: u32,
    /// How far, in metres, lower-priority Scatters keep from each copy,
    /// beyond the copy's own footprint. 0 or more.
    ///
    /// A tree's footprint is its trunk at the ground, a stone's is the
    /// widest part it rests on, and a piece of dead wood's is its outline
    /// seen from above. A mushroom's footprint is a point, so a mushroom
    /// Scatter keeps lower ones away only by its clearance.
    pub clearance: f32,
    /// What the Scatter spreads, and its settings.
    pub scatter: ScatterKind,
}

impl EcotopeScatter {
    /// A Scatter of `scatter` at `priority`, with no clearance.
    pub fn new(priority: u32, scatter: ScatterKind) -> Self {
        Self {
            priority,
            clearance: 0.0,
            scatter,
        }
    }

    /// The same Scatter with `clearance` metres kept clear around each copy.
    #[must_use]
    pub fn with_clearance(mut self, clearance: f32) -> Self {
        self.clearance = clearance;
        self
    }
}

impl Ecotope {
    /// The same Ecotope with `scatter` added under `key`, replacing any
    /// Scatter already there.
    #[must_use]
    pub fn with(mut self, key: ChildKey, scatter: EcotopeScatter) -> Self {
        self.scatters.insert(key, scatter);
        self
    }

    /// The Scatters in the order they are placed: highest priority first.
    pub fn order(&self) -> Vec<(&ChildKey, &EcotopeScatter)> {
        let mut order: Vec<_> = self.scatters.iter().collect();
        order.sort_by_key(|(_, scatter)| std::cmp::Reverse(scatter.priority));
        order
    }

    /// Checks every setting, including each Scatter's own.
    ///
    /// # Errors
    ///
    /// Returns the first problem found as an [`EcotopeError`].
    pub fn validate(&self) -> Result<(), EcotopeError> {
        self.validate_scatters()
    }

    /// Checks the Scatters' settings.
    fn validate_scatters(&self) -> Result<(), EcotopeError> {
        let order = self.order();
        for pair in order.windows(2) {
            if pair[0].1.priority == pair[1].1.priority {
                return Err(EcotopeError::SharedPriority {
                    priority: pair[0].1.priority,
                    keys: [pair[0].0.clone(), pair[1].0.clone()],
                });
            }
        }
        let keys: Vec<&ChildKey> = self.scatters.keys().collect();
        for (i, a) in keys.iter().enumerate() {
            if let Some(b) = keys[i + 1..].iter().find(|b| b.salt() == a.salt()) {
                return Err(EcotopeError::SharedSalt {
                    keys: [(*a).clone(), (*b).clone()],
                });
            }
        }
        for (key, scatter) in &self.scatters {
            if !(scatter.clearance.is_finite() && scatter.clearance >= 0.0) {
                return Err(EcotopeError::Clearance { key: key.clone() });
            }
            scatter
                .scatter
                .validate()
                .map_err(|error| EcotopeError::Scatter {
                    key: key.clone(),
                    error,
                })?;
        }
        Ok(())
    }

    /// Places every Scatter over `input`'s ground and area, highest
    /// priority first.
    ///
    /// The input's mask is the Ecotope's coverage, from 0 (none) to 1
    /// (full): every Scatter is thinned by it. The input's seed seeds the
    /// Ecotope; each Scatter gets [`scatter_seed`] of it and its key, and
    /// the Ecotope's own [`default_seed`](Self::default_seed) is not used.
    /// Each Scatter's mask is the coverage times
    /// [`EcotopePlacement::mask_above`]. The same Ecotope and input always
    /// give the same copies.
    ///
    /// # Errors
    ///
    /// Returns any error from [`validate`](Self::validate), and
    /// [`EcotopeError::Scatter`] for the first Scatter that can't be placed
    /// over the input, for example because its area is too large.
    pub fn place(&self, input: &ScatterInput<'_>) -> Result<EcotopePlacement, EcotopeError> {
        self.place_reusing(input, None)
    }

    /// Places every Scatter as [`place`](Self::place) does, and gives the
    /// same result, reusing what `previous` already placed where an edit
    /// can't have changed it.
    ///
    /// `previous` must have been placed over the same ground, area and
    /// coverage as `input`; with any other, the result is wrong. It may
    /// come from a different Ecotope, such as this one before an edit, and
    /// with a different seed.
    ///
    /// Scatters keep their copies, unplaced, from the top of the order down
    /// to the first Scatter whose key, place in the order, settings or seed
    /// changed. That Scatter and every one below it are placed again,
    /// because the footprints above them may have moved. A stone or dead
    /// wood Scatter placed again with unchanged settings and seed reuses
    /// its shapes rather than building them again.
    ///
    /// # Errors
    ///
    /// As for [`place`](Self::place).
    pub fn place_again(
        &self,
        input: &ScatterInput<'_>,
        previous: &EcotopePlacement,
    ) -> Result<EcotopePlacement, EcotopeError> {
        self.place_reusing(input, Some(previous))
    }

    /// [`place`](Self::place), reusing `previous` as
    /// [`place_again`](Self::place_again) describes.
    fn place_reusing(
        &self,
        input: &ScatterInput<'_>,
        previous: Option<&EcotopePlacement>,
    ) -> Result<EcotopePlacement, EcotopeError> {
        self.validate_scatters()?;
        let mut layers: Vec<PlacedScatter> = Vec::with_capacity(self.scatters.len());
        let mut above_unchanged = previous.is_some_and(|p| p.seed == input.seed);
        for (index, (key, scatter)) in self.order().into_iter().enumerate() {
            let seed = scatter_seed(input.seed, key);
            let earlier = previous.and_then(|p| p.get(key.as_str()));

            // Keep the layer as it was while nothing above it changed.
            let kept = previous
                .and_then(|p| p.layers.get(index))
                .filter(|old| above_unchanged && old.key == *key && old.settings == *scatter);
            if let Some(kept) = kept {
                layers.push(kept.clone());
                continue;
            }
            above_unchanged = false;

            // The mask: the coverage, kept out of every footprint above.
            let above = FootprintMask::new(layers.iter().map(|l| l.footprints.clone()).collect());
            let mask = Covered {
                coverage: input.mask,
                above: &above,
            };
            let scatter_input = ScatterInput {
                mask: Some(&mask),
                seed,
                ..*input
            };

            // Place the Scatter and keep its footprints for those below,
            // reusing its shapes if its own settings and seed didn't change.
            let prepared = earlier
                .filter(|old| old.seed == seed && old.settings.scatter == scatter.scatter)
                .map(|old| &*old.copies);
            let (copies, footprints) = scatter
                .scatter
                .place(&scatter_input, &above, scatter.clearance, prepared)
                .map_err(|error| EcotopeError::Scatter {
                    key: key.clone(),
                    error,
                })?;
            layers.push(PlacedScatter {
                key: key.clone(),
                priority: scatter.priority,
                seed,
                settings: scatter.clone(),
                copies: Arc::new(copies),
                footprints: Arc::new(footprints),
            });
        }
        Ok(EcotopePlacement {
            seed: input.seed,
            layers,
        })
    }
}

/// A coverage mask, kept out of footprints above.
struct Covered<'a> {
    coverage: Option<&'a dyn Mask>,
    above: &'a FootprintMask,
}

impl Mask for Covered<'_> {
    fn density(&self, p: glam::DVec2) -> f32 {
        if self.above.blocks(p) {
            return 0.0;
        }
        self.coverage.map_or(1.0, |coverage| coverage.density(p))
    }
}

/// Every Scatter of an Ecotope placed over one stretch of ground, from
/// [`Ecotope::place`].
#[derive(Debug, Clone)]
pub struct EcotopePlacement {
    seed: u32,
    layers: Vec<PlacedScatter>,
}

impl EcotopePlacement {
    /// The seed the Ecotope was placed with: the input's seed.
    pub fn seed(&self) -> u32 {
        self.seed
    }

    /// The placed Scatters, highest priority first.
    pub fn layers(&self) -> &[PlacedScatter] {
        &self.layers
    }

    /// The placed Scatter with this key, if the Ecotope has one.
    pub fn get(&self, key: &str) -> Option<&PlacedScatter> {
        self.layers.iter().find(|layer| layer.key.as_str() == key)
    }

    /// The mask the Scatter keyed `key` was placed with, leaving out the
    /// Ecotope's coverage: 0 inside the footprints of every Scatter above
    /// it. `None` if the Ecotope has no Scatter with this key.
    ///
    /// Placing that Scatter's settings again over the same ground, area and
    /// seed, with this mask times the same coverage, gives the same copies.
    pub fn mask_above(&self, key: &str) -> Option<FootprintMask> {
        let index = self.layers.iter().position(|l| l.key.as_str() == key)?;
        Some(FootprintMask::new(
            self.layers[..index]
                .iter()
                .map(|l| l.footprints.clone())
                .collect(),
        ))
    }
}

/// One Scatter of an Ecotope, placed.
#[derive(Debug, Clone)]
pub struct PlacedScatter {
    key: ChildKey,
    priority: u32,
    seed: u32,
    /// The settings it was placed with.
    settings: EcotopeScatter,
    copies: Arc<ScatterCopies>,
    footprints: Arc<Footprints>,
}

impl PlacedScatter {
    /// The Scatter's key in its Ecotope.
    pub fn key(&self) -> &ChildKey {
        &self.key
    }

    /// The Scatter's priority.
    pub fn priority(&self) -> u32 {
        self.priority
    }

    /// The seed the Scatter was placed with; see [`scatter_seed`].
    pub fn seed(&self) -> u32 {
        self.seed
    }

    /// The placed copies. Positions are relative to the centre of the area
    /// the Ecotope was placed over.
    pub fn copies(&self) -> &ScatterCopies {
        &self.copies
    }

    /// Where the copies stand, widened by the Scatter's clearance: the
    /// ground every lower-priority Scatter keeps out of.
    pub fn footprints(&self) -> &Footprints {
        &self.footprints
    }
}

/// Why an Ecotope is invalid, or could not be placed.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EcotopeError {
    /// Two Scatters share a priority, so their order is not settled.
    SharedPriority {
        /// The shared priority.
        priority: u32,
        /// The two Scatters' keys.
        keys: [ChildKey; 2],
    },
    /// Two different keys hash to the same salt, so their Scatters would
    /// share a seed. Rename one of them.
    SharedSalt {
        /// The two keys.
        keys: [ChildKey; 2],
    },
    /// The Scatter's clearance is negative or not a number.
    Clearance {
        /// The Scatter's key.
        key: ChildKey,
    },
    /// The Scatter's own settings are invalid, or it could not be placed.
    Scatter {
        /// The Scatter's key.
        key: ChildKey,
        /// The error from the Scatter's own crate.
        error: ScatterFailure,
    },
}

impl fmt::Display for EcotopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SharedPriority { priority, keys } => write!(
                f,
                "Scatters {} and {} share priority {priority}",
                keys[0], keys[1]
            ),
            Self::SharedSalt { keys } => write!(
                f,
                "Scatter keys {} and {} hash to the same seed; rename one",
                keys[0], keys[1]
            ),
            Self::Clearance { key } => {
                write!(f, "Scatter {key}: clearance must be 0 or more")
            }
            Self::Scatter { key, error } => write!(f, "Scatter {key}: {error}"),
        }
    }
}

impl std::error::Error for EcotopeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Scatter { error, .. } => Some(error),
            _ => None,
        }
    }
}
