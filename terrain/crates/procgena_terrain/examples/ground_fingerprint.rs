//! Prints fingerprints of painted ground weights, to check that a change
//! to how ground sites are read leaves the paint exactly the same.
//!
//! ```bash
//! cargo run -p procgena_terrain --example ground_fingerprint
//! ```
//!
//! Options: `--seed <n>` (default 1), `--chunks <n>` chunks per level
//! (default 4).
//!
//! For each shipped Terrain preset, as shipped (no rivers or roads) and with
//! rivers and roads switched on, it builds a world of that preset and
//! paints a row of chunks along x from the origin at chunk levels 0 to 3,
//! and the level 0 and 1 chunks holding the middle of each river, road and
//! pond of the origin's region, at the fine tier, with two stacks: bare
//! terrain's look, and the bare look
//! with a layer for every measure and label condition. It prints one hash
//! of every chunk's palette and weights per preset and stack. Run it before
//! and after a change and compare the lines.

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use glam::DVec2;
use procgena_core::ChildKey;
use procgena_terrain::ground::{
    BARE_SEED_KIND, Condition, FlagRule, GroundLayer, GroundStack, GroundWeights,
    MaterialCatalogue, Ramp, bare_layers,
};
use procgena_terrain::{
    ChunkKey, Elevation, FeatureTier, Flags, Landform, RegionId, TerrainPreset, WorldTerrain,
    WorldTerrainOptions,
};

fn main() {
    // Options.
    let args: Vec<String> = std::env::args().collect();
    let option = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let seed: u32 = option("--seed").map_or(1, |s| s.parse().expect("--seed <n>"));
    let chunks: i64 = option("--chunks").map_or(4, |s| s.parse().expect("--chunks <n>"));
    let stacks = [
        ("bare", GroundStack::bare(seed, 1.0)),
        ("every condition", every_condition(seed)),
    ];

    // Each preset as shipped, then with rivers and roads switched on.
    let variants = TerrainPreset::all().flat_map(|preset| {
        let shipped = (preset.key.to_string(), preset.terrain.clone());
        let mut options = preset.terrain.clone();
        options.features.rivers.density = river_density(&preset.key);
        options.features.roads.enabled = true;
        [shipped, (format!("{}+features", preset.key), options)]
    });
    for (name, options) in variants {
        // A world of this preset only.
        let source = move |_: RegionId, _: DVec2, _: &Elevation| options.clone();
        let world = WorldTerrain::new(
            WorldTerrainOptions {
                seed,
                ..WorldTerrainOptions::default()
            },
            Arc::new(source),
        )
        .expect("the default world options are valid");

        // A row of chunks at each level, then the chunks holding the middle
        // of each river, road and pond of the region at the origin, at
        // levels 0 and 1.
        let mut keys: Vec<ChunkKey> = (0..4u8)
            .flat_map(|level| (0..chunks).map(move |x| ChunkKey::new(x, 0, level)))
            .collect();
        let terrain = world
            .terrain_tier(world.region_at(DVec2::ZERO), FeatureTier::Fine)
            .expect("the origin's region builds");
        let middle = |points: &[DVec2]| points[points.len() / 2];
        let features: Vec<DVec2> = (terrain.rivers().iter().map(|r| middle(&r.points)))
            .chain(terrain.roads().iter().map(|r| middle(&r.points)))
            .chain(terrain.ponds().iter().map(|p| p.centre))
            .collect();
        for &p in &features {
            keys.extend([0, 1].map(|level| ChunkKey::containing(p, level)));
        }

        // Each stack's chunks, hashed in order.
        for (stack_name, stack) in &stacks {
            let mut hasher = DefaultHasher::new();
            let mut palette = BTreeMap::new();
            for &key in &keys {
                let weights = GroundWeights::chunk(stack, &world, key, FeatureTier::Fine)
                    .expect("the world has ground");
                weights.palette().hash(&mut hasher);
                weights.texels().hash(&mut hasher);
                for &id in weights.palette() {
                    *palette.entry(id.0).or_insert(0) += 1;
                }
            }
            println!(
                "{name:<24} {stack_name:<16} {:016x}  {} chunks, {} features, \
                 materials {palette:?}",
                hasher.finish(),
                keys.len(),
                features.len()
            );
        }
    }
}

/// The river sources per km² each shipped preset's other river settings
/// were tuned for.
fn river_density(preset: &str) -> f32 {
    match preset {
        "plains" => 0.6,
        "rolling_hills" => 1.0,
        "mountains" => 1.2,
        other => panic!("no river density for preset {other}"),
    }
}

/// Bare terrain's look with a layer on every measure and label condition:
/// moss in hollows, mud on banks and wet ground, gravel shoulders and rock
/// on cliffs, ridges and peaks.
fn every_condition(seed: u32) -> GroundStack {
    let key = |k: &'static str| ChildKey::from_static(k);
    let mut layers = bare_layers();
    layers.extend([
        (
            key("hollow_moss"),
            GroundLayer::new(key("moss"), 300).when(Condition::Curvature(Ramp::fall(-0.01, 0.0))),
        ),
        (
            key("bank_mud"),
            GroundLayer::new(key("mud"), 600)
                .when(Condition::WaterDistance(Ramp::fall(3.0, 8.0)))
                .when(Condition::HeightAboveWater(Ramp::fall(1.0, 2.0))),
        ),
        (
            key("wet_mud"),
            GroundLayer::new(key("mud"), 605).when(Condition::Flags(FlagRule {
                any: Flags::WET,
                none: Flags::ROAD,
            })),
        ),
        (
            key("road_shoulder"),
            GroundLayer::new(key("gravel_road"), 630)
                .when(Condition::RoadDistance(Ramp::fall(0.5, 2.0))),
        ),
        (
            key("cliff"),
            GroundLayer::new(key("cliff_rock"), 660).when(Condition::Landform(
                [Landform::Cliff, Landform::Ridge, Landform::Peak].into(),
            )),
        ),
    ]);
    GroundStack::new(
        &layers,
        Arc::new(MaterialCatalogue::standard()),
        BARE_SEED_KIND,
        seed,
        1.0,
    )
    .expect("the layers fit the standard catalogue")
}
