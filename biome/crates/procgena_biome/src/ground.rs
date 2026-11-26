//! A Biome's base ground: the ground layers under its Ecotopes, and all of
//! its ground where no Ecotope covers it.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use procgena_core::ChildKey;
use procgena_terrain::Landform;
use procgena_terrain::ground::{
    Condition, GroundError, GroundLayer, GroundStack, MaterialCatalogue, Ramp, bare_layers,
};

use crate::Biome;

/// The seed kind of a Biome's base ground layers: layer `key` of a Biome
/// instance seeded `seed` (see [`biome_seed`](crate::biome_seed)) gets
/// `child_seed(seed, BASE_GROUND_SEED_KIND, key)` for its noise.
pub const BASE_GROUND_SEED_KIND: &str = "biome.child.ground";

/// The base ground of a [`Biome`]: the layers every point of its region
/// starts from, before any Ecotope's layers.
#[derive(Debug, Clone, PartialEq)]
pub enum BiomeGround {
    /// The built-in base ground, [`standard_base_layers`].
    Standard,
    /// Layers given in full, by key. The lowest must cover everything (see
    /// [`GroundStack::new`]).
    Layers(BTreeMap<ChildKey, GroundLayer>),
}

fn key(key: &'static str) -> ChildKey {
    ChildKey::from_static(key)
}

/// The built-in base ground of a Biome, over
/// [`MaterialCatalogue::standard`]: bare terrain's look
/// ([`bare_layers`]) with river banks, road sides and cliffs of their own.
/// Besides the bare layers, it has:
///
/// | Key | Material | Order | Where |
/// |-----|----------|-------|-------|
/// | `bank_mud` | `mud` | 600 | within about 3 to 8 m of a river or pond, up to about 1 to 2 m above it |
/// | `road_shoulder` | `gravel_road` | 630 | within about 0.5 to 2 m of a road's surface |
/// | `cliff` | `cliff_rock` | 660 | where the terrain labels the ground a cliff |
///
/// It reads labels, so painting it costs the most detail
/// ([`SampleDetail::Labels`](procgena_terrain::SampleDetail::Labels)).
pub fn standard_base_layers() -> BTreeMap<ChildKey, GroundLayer> {
    let mut layers = bare_layers();
    layers.extend([
        (
            key("bank_mud"),
            GroundLayer::new(key("mud"), 600)
                .when(Condition::WaterDistance(Ramp::fall(3.0, 8.0)))
                .when(Condition::HeightAboveWater(Ramp::fall(1.0, 2.0))),
        ),
        (
            key("road_shoulder"),
            GroundLayer::new(key("gravel_road"), 630)
                .when(Condition::RoadDistance(Ramp::fall(0.5, 2.0))),
        ),
        (
            key("cliff"),
            GroundLayer::new(key("cliff_rock"), 660)
                .when(Condition::Landform(BTreeSet::from([Landform::Cliff]))),
        ),
    ]);
    layers
}

impl BiomeGround {
    /// The layers, by key.
    pub fn layers(&self) -> BTreeMap<ChildKey, GroundLayer> {
        match self {
            Self::Standard => standard_base_layers(),
            Self::Layers(layers) => layers.clone(),
        }
    }
}

impl Biome {
    /// This Biome's base ground as a stack over `catalogue`, for the Biome
    /// instance seeded `seed` (see [`biome_seed`](crate::biome_seed)) in a
    /// World of vertical scale `vertical`. Paint a Terrain's chunks with
    /// it where no Ecotope covers them; lay Ecotope layers over it.
    ///
    /// # Errors
    ///
    /// Any error of [`GroundStack::new`]: unusable layers, a lowest layer
    /// that doesn't cover everything, a material `catalogue` lacks, or a
    /// `vertical` that isn't more than 0.
    pub fn ground(
        &self,
        catalogue: Arc<MaterialCatalogue>,
        seed: u32,
        vertical: f32,
    ) -> Result<GroundStack, GroundError> {
        GroundStack::new(
            &self.ground.layers(),
            catalogue,
            BASE_GROUND_SEED_KIND,
            seed,
            vertical,
        )
    }
}
