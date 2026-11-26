//! Base ground materials for a Biome. These layers provide the surface
//! where no Ecotope is present and combine with Ecotope layers elsewhere.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use procgena_core::ChildKey;
use procgena_terrain::Landform;
use procgena_terrain::ground::{
    Condition, GroundError, GroundLayer, GroundStack, MaterialCatalogue, Ramp, bare_layers,
};

use crate::Biome;

/// Seed namespace for reproducible noise in a Biome's base ground layers.
/// Each layer uses `child_seed(seed, BASE_GROUND_SEED_KIND, key)`, where
/// `seed` is the Biome instance seed from [`biome_seed`](crate::biome_seed).
pub const BASE_GROUND_SEED_KIND: &str = "biome.child.ground";

/// Chooses the base materials for a [`Biome`], including areas with no
/// Ecotopes. Ecotope layers combine with these layers by their order.
#[derive(Debug, Clone, PartialEq)]
pub enum BiomeGround {
    /// Uses the built-in materials and rules from [`standard_base_layers`].
    Standard,
    /// Replaces all standard layers with this keyed collection. Supply at
    /// least one layer; the lowest must cover every point at full opacity.
    /// See [`GroundStack::new`] for validation requirements.
    Layers(BTreeMap<ChildKey, GroundLayer>),
}

fn key(key: &'static str) -> ChildKey {
    ChildKey::from_static(key)
}

/// Returns the built-in Biome layers, using materials from
/// [`MaterialCatalogue::standard`]. Extends [`bare_layers`] with mud near
/// water, gravel beside roads and rock on labelled cliffs.
///
/// The distance and height limits below fade smoothly; other layers can
/// cover these materials according to their order.
///
/// | Key | Material | Order | Where |
/// |-----|----------|-------|-------|
/// | `bank_mud` | `mud` | 600 | full within 3 m of water and at most 1 m above it; fades to zero at 8 m away or 2 m above |
/// | `road_shoulder` | `gravel_road` | 630 | full on the road and within 0.5 m of its edge; fades to zero 2 m away |
/// | `cliff` | `cliff_rock` | 660 | where the terrain labels the ground a cliff |
///
/// These layers require [`SampleDetail::Labels`](procgena_terrain::SampleDetail::Labels):
/// terrain measurements plus landform classification. Sampling them costs
/// more than rules based only on height and slope. The mud rule also
/// matches underwater points; higher layers determine the visible surface.
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
    /// Returns an owned collection of layers by key: the built-in layers
    /// for `Standard`, or a copy of the supplied layers for `Layers`.
    pub fn layers(&self) -> BTreeMap<ChildKey, GroundLayer> {
        match self {
            Self::Standard => standard_base_layers(),
            Self::Layers(layers) => layers.clone(),
        }
    }
}

impl Biome {
    /// Builds a painter for this Biome's base ground using `catalogue`.
    /// Pass the Biome instance `seed` from [`biome_seed`](crate::biome_seed)
    /// and the World's positive, finite vertical scale as `vertical`.
    /// The catalogue must contain every material named by the layers.
    ///
    /// Use the stack to paint terrain chunks, and combine it with Ecotope
    /// layers where Ecotopes are present. When sampling sites yourself,
    /// request the stack's [`detail`](GroundStack::detail).
    ///
    /// # Errors
    ///
    /// Returns the validation errors documented by [`GroundStack::new`],
    /// including empty or invalid layers, an incomplete lowest layer,
    /// missing materials, conflicting orders or seed salts, unsupported
    /// Scatter influences, and a non-positive or non-finite `vertical`.
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
