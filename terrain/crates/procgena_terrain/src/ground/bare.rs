//! The look of bare terrain: grass, rock on steep ground, snow up high,
//! road surfaces and riverbeds.

use std::collections::BTreeMap;
use std::sync::Arc;

use procgena_core::ChildKey;

use super::{Condition, GroundLayer, GroundStack, HeightRule, MaterialCatalogue, Ramp};
use crate::RoadTags;

/// The seed kind of the bare-terrain stack's layers: layer `key` of a bare
/// stack seeded `seed` gets `child_seed(seed, BARE_SEED_KIND, key)`.
pub const BARE_SEED_KIND: &str = "terrain.child.ground";

fn key(key: &'static str) -> ChildKey {
    ChildKey::from_static(key)
}

/// The layers of bare terrain's look, by key, over
/// [`MaterialCatalogue::standard`]:
///
/// | Key | Material | Order | Where |
/// |-----|----------|-------|-------|
/// | `soil` | `grass` | 0 | everywhere |
/// | `riverbed` | `riverbed` | 610 | under water |
/// | `road_banks` | `bare_dirt` | 620 | banks cut or built up beside roads |
/// | `rock` | `cliff_rock` | 650 | slopes from about 27° to 40° and steeper |
/// | `road` | `gravel_road` | 680 | road surfaces |
/// | `snow` | `snow` | 900 | above 900 to 1,100 m, on slopes below about 34° to 53° |
///
/// Without roads and water, these are the colours the terrain was drawn
/// with before ground layers.
pub fn bare_layers() -> BTreeMap<ChildKey, GroundLayer> {
    BTreeMap::from([
        (key("soil"), GroundLayer::new(key("grass"), 0)),
        (
            key("riverbed"),
            GroundLayer::new(key("riverbed"), 610)
                .when(Condition::HeightAboveWater(Ramp::fall(-0.4, 0.0))),
        ),
        (
            key("road_banks"),
            GroundLayer::new(key("bare_dirt"), 620)
                .when(Condition::RoadTags(RoadTags::CUT | RoadTags::EMBANKMENT)),
        ),
        (
            key("rock"),
            GroundLayer::new(key("cliff_rock"), 650).when(Condition::Slope(Ramp::rise(0.45, 0.65))),
        ),
        (
            key("road"),
            GroundLayer::new(key("gravel_road"), 680).when(Condition::RoadTags(RoadTags::ROAD)),
        ),
        (
            key("snow"),
            GroundLayer::new(key("snow"), 900)
                .when(Condition::Height(HeightRule::Absolute(Ramp::rise(
                    900.0, 1100.0,
                ))))
                .when(Condition::Slope(Ramp::fall(0.55, 0.8))),
        ),
    ])
}

impl GroundStack {
    /// Bare terrain's look ([`bare_layers`]) over the standard catalogue,
    /// for a World seeded `seed` with vertical scale `vertical`.
    ///
    /// # Panics
    ///
    /// If `vertical` isn't more than 0.
    pub fn bare(seed: u32, vertical: f32) -> Self {
        Self::new(
            &bare_layers(),
            Arc::new(MaterialCatalogue::standard()),
            BARE_SEED_KIND,
            seed,
            vertical,
        )
        .expect("the bare layers fit the standard catalogue; vertical must be more than 0")
    }
}
