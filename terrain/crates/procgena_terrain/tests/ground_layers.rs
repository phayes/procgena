//! Ground layers: the bare-terrain look, stacking, and chunk weights that
//! agree along chunk edges whatever order chunks are built in.

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use common::{DEFAULT, RIVER_SEEDS, hills_into_plains};
use glam::{DVec2, Vec2};
use procgena_core::ChildKey;
use procgena_terrain::ground::{
    Condition, FlagRule, GROUND_TEXELS, GroundError, GroundLayer, GroundMaterial, GroundMix,
    GroundNoise, GroundOverlay, GroundPainter, GroundSite, GroundSource, GroundStack,
    GroundWeights, HeightRule, MaterialCatalogue, MaterialId, OnGround, PALETTE_SIZE, Ramp,
};
use procgena_terrain::{
    Boundary, ChunkKey, FeatureTier, Flags, Labels, Landform, Region, RegionId, RoadTags,
    SampleDetail, Terrain, TerrainGenerator, fixtures,
};

fn key(k: &str) -> ChildKey {
    ChildKey::new(k).unwrap()
}

/// The plains of [`hills_into_plains`], with rivers and roads.
fn plains() -> Terrain {
    let world = hills_into_plains(RIVER_SEEDS[0], DEFAULT);
    let plains = world.terrains()[0].clone();
    assert!(!plains.roads().is_empty() && !plains.rivers().is_empty());
    plains
}

/// The colour the terrain shader drew before ground layers
/// (`terrain_fragment.wesl` with the default `TerrainSurface`), without
/// its patchy variation.
fn g6_colour(height: f32, slope: f32) -> [f32; 3] {
    fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
        let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
    fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
        [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t)
    }
    let (grass, rock, snow) = ([0.14, 0.22, 0.06], [0.24, 0.22, 0.2], [0.85, 0.87, 0.9]);
    let colour = mix(grass, rock, smoothstep(0.45, 0.65, slope));
    let snow_line = smoothstep(900.0, 1100.0, height);
    mix(
        colour,
        snow,
        snow_line * (1.0 - smoothstep(0.55, 0.8, slope)),
    )
}

#[test]
fn the_bare_look_matches_the_terrain_colours_it_replaces() {
    let bare = GroundStack::bare(3, 1.0);
    for height in [-50.0, 0.0, 850.0, 950.0, 1000.0, 1050.0, 1200.0, 3000.0] {
        for g in [0.0_f32, 0.3, 0.5, 0.6, 0.75, 0.9, 1.2, 1.6, 3.0, 10.0] {
            let site = GroundSite::new(height, Vec2::new(g * 0.6, g * 0.8));
            let colour = bare
                .paint(DVec2::new(12.5, -3.0), &site)
                .colour(bare.catalogue());
            let expected = g6_colour(height, site.slope);
            for k in 0..3 {
                assert!(
                    (colour[k] - expected[k]).abs() < 1e-5,
                    "height {height}, gradient {g}: {colour:?} against {expected:?}"
                );
            }
        }
    }
}

#[test]
fn absolute_heights_scale_with_the_world() {
    let catalogue = MaterialCatalogue::standard();
    let snow = catalogue.id("snow").unwrap();
    let flat = |h: f32| GroundSite::new(h, Vec2::ZERO);
    let normal = GroundStack::bare(3, 1.0);
    let tall = GroundStack::bare(3, 2.0);
    assert_eq!(normal.paint(DVec2::ZERO, &flat(1500.0)).weight(snow), 1.0);
    assert_eq!(tall.paint(DVec2::ZERO, &flat(1500.0)).weight(snow), 0.0);
    assert_eq!(tall.paint(DVec2::ZERO, &flat(2300.0)).weight(snow), 1.0);

    // A relief height rule does nothing where the relief height is unknown.
    let layers = BTreeMap::from([
        (key("soil"), GroundLayer::new(key("grass"), 0)),
        (
            key("peaks"),
            GroundLayer::new(key("snow"), 900)
                .when(Condition::Height(HeightRule::Relief(Ramp::rise(0.7, 0.9)))),
        ),
    ]);
    let stack = GroundStack::new(&layers, Arc::new(catalogue), "test.ground", 1, 1.0).unwrap();
    assert_eq!(stack.paint(DVec2::ZERO, &flat(0.0)).weight(snow), 1.0);
    let low = flat(0.0).with_relief_height(0.5);
    assert_eq!(stack.paint(DVec2::ZERO, &low).weight(snow), 0.0);
}

#[test]
fn roads_and_riverbeds_show_on_a_terrain() {
    let plains = plains();
    let bare = GroundStack::bare(3, 1.0);
    let catalogue = bare.catalogue();

    // The middle of a road is gravel.
    let road = &plains.roads()[0].points;
    let on_road = road
        .iter()
        .copied()
        .find(|&p| {
            let site = plains.site(p, FeatureTier::Fine);
            site.road_tags == Some(RoadTags::ROAD) && site.slope < 0.3
        })
        .expect("a gentle stretch of road");
    let mix = bare.paint(on_road, &plains.site(on_road, FeatureTier::Fine));
    assert_eq!(mix.weight(catalogue.id("gravel_road").unwrap()), 1.0);

    // A riverbed lies under the river.
    let river = &plains.rivers()[0].points;
    let wet = river
        .iter()
        .copied()
        .find(|&p| {
            plains
                .site(p, FeatureTier::Fine)
                .height_above_water()
                .is_some_and(|h| h < -0.5)
        })
        .expect("a deep stretch of river");
    let mix = bare.paint(wet, &plains.site(wet, FeatureTier::Fine));
    assert!(mix.weight(catalogue.id("riverbed").unwrap()) > 0.99);

    // Plain ground has neither.
    let site = OnGround(&procgena_core::FlatGround).site(on_road, FeatureTier::Fine);
    assert_eq!(
        bare.paint(on_road, &site)
            .weight(catalogue.id("grass").unwrap()),
        1.0
    );
}

/// Every texel weight along the side of `weights` facing `side`, by
/// material id: `(texel index along the side, material, weight)`.
fn edge(
    weights: &GroundWeights,
    max_x: bool,
    catalogue: &MaterialCatalogue,
) -> Vec<(u32, MaterialId, u8)> {
    let i = if max_x { GROUND_TEXELS - 1 } else { 0 };
    let mut out = Vec::new();
    for j in 0..GROUND_TEXELS {
        for (id, _, _) in catalogue.iter() {
            out.push((j, id, weights.weight(i, j, id)));
        }
    }
    out
}

#[test]
fn neighbouring_chunks_agree_along_their_edge() {
    let plains = plains();
    let bare = GroundStack::bare(3, 1.0);
    let road = &plains.roads()[0].points;
    for level in [0, 1] {
        let west = ChunkKey::containing(road[road.len() / 2], level);
        let east = west.offset(1, 0);
        let a = GroundWeights::chunk(&bare, &plains, west, FeatureTier::Fine).unwrap();
        let b = GroundWeights::chunk(&bare, &plains, east, FeatureTier::Fine).unwrap();
        assert_eq!((a.dropped(), b.dropped()), (0, 0));
        assert!(
            a.palette().len() > 1,
            "the road chunk holds several materials"
        );
        assert_eq!(
            edge(&a, true, bare.catalogue()),
            edge(&b, false, bare.catalogue())
        );

        // Every texel's weights add up to 255.
        for j in 0..GROUND_TEXELS {
            for i in 0..GROUND_TEXELS {
                let sum: u32 = a.texel(i, j).iter().map(|&w| u32::from(w)).sum();
                assert_eq!(sum, 255);
            }
        }
    }
}

#[test]
fn blended_edges_agree_when_the_palettes_differ() {
    // Moss in noisy patches over grass everywhere, and bare dirt (the
    // lowest id) only well inside the western chunk, on ground that rises
    // gently eastwards.
    let layers = BTreeMap::from([
        (key("soil"), GroundLayer::new(key("grass"), 0)),
        (
            key("moss"),
            GroundLayer::new(key("moss"), 300).with_noise(GroundNoise {
                wavelength: 5.0,
                contrast: 1.0,
                threshold: 0.5,
            }),
        ),
        (
            key("dirt"),
            GroundLayer::new(key("bare_dirt"), 200).when(Condition::Height(HeightRule::Absolute(
                Ramp::fall(-0.3, -0.2),
            ))),
        ),
    ]);
    let catalogue = Arc::new(MaterialCatalogue::standard());
    let stack = GroundStack::new(&layers, catalogue.clone(), "test.ground", 9, 1.0).unwrap();
    let rising = |p: DVec2| p.x as f32 * 0.01;
    let source = OnGround(&rising);
    let build = |k| GroundWeights::chunk(&stack, &source, k, FeatureTier::Fine).unwrap();
    let dirt = catalogue.id("bare_dirt").unwrap();

    // Across an x edge: the west chunk has dirt, the east one doesn't.
    let (west, east) = (
        build(ChunkKey::new(-1, 0, 0)),
        build(ChunkKey::new(0, 0, 0)),
    );
    assert!(west.palette().contains(&dirt) && !east.palette().contains(&dirt));
    let mut blended = 0;
    for j in 0..GROUND_TEXELS {
        let moss = west.weight(GROUND_TEXELS - 1, j, catalogue.id("moss").unwrap());
        blended += usize::from(moss > 0 && moss < 255);
        for (id, _, _) in catalogue.iter() {
            assert_eq!(
                west.weight(GROUND_TEXELS - 1, j, id),
                east.weight(0, j, id),
                "texel {j}"
            );
        }
    }
    assert!(blended > 64, "{blended} blended edge texels");

    // Across a z edge.
    let south = build(ChunkKey::new(-1, -1, 0));
    for i in 0..GROUND_TEXELS {
        for (id, _, _) in catalogue.iter() {
            assert_eq!(
                south.weight(i, GROUND_TEXELS - 1, id),
                west.weight(i, 0, id)
            );
        }
    }
}

#[test]
fn chunks_are_the_same_whatever_order_they_are_built_in() {
    let plains = plains();
    let road = &plains.roads()[0].points;
    let start = ChunkKey::containing(road[road.len() / 2], 0);
    let keys: Vec<ChunkKey> = (-1..=1)
        .flat_map(|dx| (-1..=1).map(move |dz| start.offset(dx, dz)))
        .collect();
    let build = |keys: &mut dyn Iterator<Item = &ChunkKey>| {
        let bare = GroundStack::bare(3, 1.0);
        let mut built: BTreeMap<ChunkKey, GroundWeights> = BTreeMap::new();
        for &k in keys {
            let weights = GroundWeights::chunk(&bare, &plains, k, FeatureTier::Fine).unwrap();
            built.insert(k, weights);
        }
        built
    };
    let forwards = build(&mut keys.iter());
    let backwards = build(&mut keys.iter().rev());
    assert_eq!(forwards, backwards);
}

/// A painter of ten materials in stripes along x, each 10 m wide.
struct Stripes(MaterialCatalogue);

impl GroundPainter for Stripes {
    fn catalogue(&self) -> &MaterialCatalogue {
        &self.0
    }

    fn paint(&self, p: DVec2, _: &GroundSite) -> GroundMix {
        let stripe = (p.x / 10.0).floor().rem_euclid(10.0) as u16;
        GroundMix::single(MaterialId(stripe))
    }
}

#[test]
fn a_chunk_keeps_its_eight_heaviest_materials() {
    let catalogue = (0..10).fold(MaterialCatalogue::new(), |c, i| {
        c.with(
            key(&format!("m{i}")),
            GroundMaterial::new([i as f32 / 10.0, 0.5, 0.5], 0.5),
        )
    });
    let painter = Stripes(catalogue);
    let weights = GroundWeights::chunk(
        &painter,
        &OnGround(&procgena_core::FlatGround),
        ChunkKey::new(0, 0, 1),
        FeatureTier::Fine,
    )
    .unwrap();
    assert_eq!(weights.palette().len(), PALETTE_SIZE);
    assert_eq!(weights.dropped(), 2);
    // The 128 m chunk holds stripes 0 to 9, then 0, 1 and most of 2 again.
    // Stripes 3 to 9 tie, and the tie goes to the lower ids.
    let kept: Vec<u16> = weights.palette().iter().map(|id| id.0).collect();
    assert_eq!(kept, [0, 1, 2, 3, 4, 5, 6, 7]);
    for j in 0..GROUND_TEXELS {
        for i in 0..GROUND_TEXELS {
            let sum: u32 = weights.texel(i, j).iter().map(|&w| u32::from(w)).sum();
            assert_eq!(sum, 255);
        }
    }
}

#[test]
fn an_overlay_lies_on_top_at_equal_orders() {
    let bare = GroundStack::bare(3, 1.0);
    let catalogue = bare.catalogue().clone();
    let site = GroundSite::new(0.0, Vec2::ZERO);

    // Half moss over the soil; at the rock's order on steep ground, the
    // overlay's mud lies over the rock.
    let layers = BTreeMap::from([
        (
            key("moss"),
            GroundLayer::new(key("moss"), 300).with_opacity(0.5),
        ),
        (
            key("mud"),
            GroundLayer::new(key("mud"), 650).when(Condition::Slope(Ramp::rise(0.9, 0.95))),
        ),
    ]);
    let overlay = GroundOverlay::new(&layers, &bare, "test.ground", 5).unwrap();
    let mix = bare.paint_with(Some(&overlay), |_| 1.0, DVec2::ZERO, &site);
    assert_eq!(mix.weight(catalogue.id("moss").unwrap()), 0.5);
    assert_eq!(mix.weight(catalogue.id("grass").unwrap()), 0.5);
    let wall = GroundSite::new(0.0, Vec2::new(100.0, 0.0));
    let mix = bare.paint_with(Some(&overlay), |_| 1.0, DVec2::ZERO, &wall);
    assert_eq!(mix.weight(catalogue.id("mud").unwrap()), 1.0);
}

#[test]
fn invalid_stacks_are_refused() {
    let catalogue = Arc::new(MaterialCatalogue::standard());
    let stack = |layers: &[(&str, GroundLayer)]| {
        let layers = layers.iter().map(|(k, l)| (key(k), l.clone())).collect();
        GroundStack::new(&layers, catalogue.clone(), "test.ground", 1, 1.0)
    };
    let soil = GroundLayer::new(key("grass"), 0);
    assert_eq!(stack(&[]).unwrap_err(), GroundError::Empty);
    assert!(matches!(
        stack(&[("soil", soil.clone().with_opacity(0.5))]),
        Err(GroundError::BaseNotOpaque { .. })
    ));
    assert!(matches!(
        stack(&[
            ("soil", soil.clone()),
            ("other", GroundLayer::new(key("moss"), 0))
        ]),
        Err(GroundError::SharedOrder { order: 0, .. })
    ));
    assert!(matches!(
        stack(&[("soil", GroundLayer::new(key("lava"), 0))]),
        Err(GroundError::UnknownMaterial { .. })
    ));
    assert!(matches!(
        stack(&[
            ("soil", soil.clone()),
            (
                "bad",
                GroundLayer::new(key("moss"), 5).when(Condition::Slope(Ramp::rise(0.6, 0.2)))
            )
        ]),
        Err(GroundError::Layer { .. })
    ));
}

#[test]
fn a_grid_over_a_chunk_paints_the_chunks_weights() {
    let plains = plains();
    let bare = GroundStack::bare(3, 1.0);
    let road = &plains.roads()[0].points;
    let key = ChunkKey::containing(road[road.len() / 2], 1);
    let chunk = GroundWeights::chunk(&bare, &plains, key, FeatureTier::Fine).unwrap();
    let grid = GroundWeights::grid(
        &bare,
        &plains,
        key.min_corner(),
        key.max_corner(),
        glam::UVec2::splat(GROUND_TEXELS),
        FeatureTier::Fine,
    )
    .unwrap();
    assert_eq!(grid, chunk);

    // A smaller grid of another shape, its corners on the rectangle's.
    let (min, max) = (DVec2::new(3.0, -2.0), DVec2::new(13.0, 4.0));
    let small = GroundWeights::grid(
        &bare,
        &OnGround(&procgena_core::FlatGround),
        min,
        max,
        glam::UVec2::new(11, 4),
        FeatureTier::Fine,
    )
    .unwrap();
    assert_eq!(small.size(), glam::UVec2::new(11, 4));
    assert_eq!(small.texels().len(), 44);
    let grass = bare.catalogue().id("grass").unwrap();
    assert_eq!(small.weight(10, 3, grass), 255);
}

#[test]
fn retained_sites_repaint_without_reading_ground() {
    use procgena_terrain::ground::GroundGrid;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Counted(AtomicUsize);
    impl GroundSource for Counted {
        fn site(&self, p: DVec2, _: FeatureTier) -> GroundSite {
            self.0.fetch_add(1, Ordering::Relaxed);
            GroundSite::new(p.x as f32 * 20.0, Vec2::new(0.3, 0.5))
        }
    }
    let source = Counted(AtomicUsize::new(0));
    let key = ChunkKey::new(-1, 2, 0);
    let grid = GroundGrid::chunk(&source, key, FeatureTier::Fine, SampleDetail::Measures).unwrap();
    assert_eq!(grid.detail(), SampleDetail::Measures);
    let count = source.0.load(Ordering::Relaxed);
    for vertical in [0.5, 1.0, 2.0] {
        let painter = GroundStack::bare(7, vertical);
        let retained = grid.paint(&painter);
        assert_eq!(source.0.load(Ordering::Relaxed), count);
        let fresh = GroundWeights::chunk(&painter, &source, key, FeatureTier::Fine).unwrap();
        assert_eq!(retained, fresh);
        source.0.store(count, Ordering::Relaxed);
    }
    assert_eq!(count, (GROUND_TEXELS * GROUND_TEXELS) as usize);
}

#[test]
fn measure_and_label_conditions_read_the_site() {
    let mut site = GroundSite::new(5.0, Vec2::ZERO);
    site.water_distance = Some(2.0);
    site.road_distance = Some(10.0);
    site.curvature = Some(-0.05);
    let site = site.with_labels(Labels {
        landform: Landform::Riverbank,
        flags: Flags::RIVER | Flags::WET,
    });
    let factor = |c: Condition| c.factor(&site, 1.0);

    // Numeric measures produce the expected ramp weights.
    assert_eq!(
        factor(Condition::WaterDistance(Ramp::fall(3.0, 8.0))),
        Some(1.0)
    );
    assert_eq!(
        factor(Condition::RoadDistance(Ramp::fall(3.0, 8.0))),
        Some(0.0)
    );
    assert_eq!(
        factor(Condition::Curvature(Ramp::fall(-0.02, 0.0))),
        Some(1.0)
    );
    assert_eq!(
        factor(Condition::Curvature(Ramp::rise(0.0, 0.02))),
        Some(0.0)
    );

    // Any listed landform matches; an empty set matches nothing.
    let landforms = |set: &[Landform]| Condition::Landform(set.iter().copied().collect());
    assert_eq!(
        factor(landforms(&[Landform::Riverbank, Landform::Shore])),
        Some(1.0)
    );
    assert_eq!(factor(landforms(&[Landform::Cliff])), Some(0.0));
    assert_eq!(factor(landforms(&[])), Some(0.0));

    // Required flags allow a match unless an excluded flag is also present.
    let flags = |any, none| Condition::Flags(FlagRule { any, none });
    assert_eq!(factor(flags(Flags::NONE, Flags::NONE)), Some(1.0));
    assert_eq!(
        factor(flags(Flags::WET | Flags::POND, Flags::NONE)),
        Some(1.0)
    );
    assert_eq!(factor(flags(Flags::POND, Flags::NONE)), Some(0.0));
    assert_eq!(factor(flags(Flags::WET, Flags::RIVER)), Some(0.0));
    assert_eq!(factor(flags(Flags::NONE, Flags::ROAD)), Some(1.0));

    // Plain ground has no nearby water or roads: both distances are capped
    // at MEASURE_REACH. Curvature and labels are unknown, so their conditions
    // return None and do not reduce a layer's weight.
    let plain = GroundSite::new(5.0, Vec2::ZERO);
    let near_water = Condition::WaterDistance(Ramp::fall(3.0, 8.0));
    assert_eq!(near_water.factor(&plain, 1.0), Some(0.0));
    assert_eq!(
        Condition::Curvature(Ramp::rise(0.0, 0.02)).factor(&plain, 1.0),
        None
    );
    assert_eq!(landforms(&[Landform::Cliff]).factor(&plain, 1.0), None);
    assert_eq!(flags(Flags::WET, Flags::NONE).factor(&plain, 1.0), None);
    let layer = GroundLayer::new(key("moss"), 300).when(landforms(&[Landform::Cliff]));
    assert_eq!(layer.weight(DVec2::ZERO, &plain, 0, 1.0), 1.0);
}

/// Builds a test stack with bare terrain materials, mud near water,
/// gravel road shoulders and rock on ground labelled as a cliff.
fn banks_and_roads() -> GroundStack {
    let mut layers = procgena_terrain::ground::bare_layers();
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
                .when(Condition::Landform([Landform::Cliff].into())),
        ),
    ]);
    let catalogue = Arc::new(MaterialCatalogue::standard());
    GroundStack::new(&layers, catalogue, "test.ground", 3, 1.0).unwrap()
}

#[test]
fn painters_ask_for_the_detail_their_conditions_need() {
    let catalogue = Arc::new(MaterialCatalogue::standard());
    let stack = |layers: &[(&str, GroundLayer)]| {
        let layers = layers.iter().map(|(k, l)| (key(k), l.clone())).collect();
        GroundStack::new(&layers, catalogue.clone(), "test.ground", 1, 1.0).unwrap()
    };
    let soil = ("soil", GroundLayer::new(key("grass"), 0));
    let rock = (
        "rock",
        GroundLayer::new(key("cliff_rock"), 650).when(Condition::Slope(Ramp::rise(0.4, 0.6))),
    );
    assert_eq!(
        stack(std::slice::from_ref(&soil)).detail(),
        SampleDetail::Gradient
    );
    assert_eq!(
        stack(&[soil.clone(), rock]).detail(),
        SampleDetail::Gradient
    );
    let mud = (
        "mud",
        GroundLayer::new(key("mud"), 600).when(Condition::WaterDistance(Ramp::fall(1.0, 2.0))),
    );
    assert_eq!(stack(&[soil.clone(), mud]).detail(), SampleDetail::Measures);

    // The bare stack needs measures for height above nearby water.
    // Adding a cliff condition raises the requirement to labels.
    assert_eq!(GroundStack::bare(3, 1.0).detail(), SampleDetail::Measures);
    assert_eq!(banks_and_roads().detail(), SampleDetail::Labels);

    // An overlay reports its own requirements, independently of its base stack.
    let bare = GroundStack::bare(3, 1.0);
    let overlay = |layer: GroundLayer| {
        GroundOverlay::new(&BTreeMap::from([(key("wet"), layer)]), &bare, "test", 1).unwrap()
    };
    let wet = GroundLayer::new(key("mud"), 400).when(Condition::Flags(FlagRule {
        any: Flags::WET,
        none: Flags::NONE,
    }));
    assert_eq!(overlay(wet).detail(), SampleDetail::Labels);
    let empty = GroundOverlay::new(&BTreeMap::new(), &bare, "test", 1).unwrap();
    assert_eq!(empty.detail(), SampleDetail::Gradient);
}

/// Paints a chunk and returns its sampled sites. Checks that painting a
/// cached grid produces the same weights as sampling and painting together.
fn painted(
    terrain: &Terrain,
    stack: &GroundStack,
    key: ChunkKey,
) -> (GroundWeights, Vec<GroundSite>) {
    use procgena_terrain::ground::GroundGrid;
    let grid = GroundGrid::chunk(terrain, key, FeatureTier::Fine, stack.detail()).unwrap();
    let weights = GroundWeights::chunk(stack, terrain, key, FeatureTier::Fine).unwrap();
    assert_eq!(grid.paint(stack), weights, "the chunk reads the same sites");
    (weights, grid.sites().to_vec())
}

#[test]
fn mud_follows_river_banks_and_gravel_lines_roads() {
    let plains = plains();
    let stack = banks_and_roads();
    let catalogue = stack.catalogue();
    let (mud, gravel) = (
        catalogue.id("mud").unwrap(),
        catalogue.id("gravel_road").unwrap(),
    );
    let texel = |n: usize| (n as u32 % GROUND_TEXELS, n as u32 / GROUND_TEXELS);

    // Mud stays near water and below the height cutoff, including underwater points.
    let river = &plains.rivers()[0].points;
    let key = ChunkKey::containing(river[river.len() / 2], 0);
    let (weights, sites) = painted(&plains, &stack, key);
    let mut banks = 0;
    for (n, site) in sites.iter().enumerate() {
        let (i, j) = texel(n);
        if weights.weight(i, j, mud) > 128 {
            banks += 1;
            assert!(site.water_distance.unwrap() < 8.0, "{site:?}");
            assert!(site.height_above_water().unwrap() < 2.0, "{site:?}");
        }
        if site.water_distance.unwrap() >= 8.0 {
            assert_eq!(weights.weight(i, j, mud), 0, "{site:?}");
        }
    }
    assert!(banks > 50, "mud on {banks} texels of the river's chunk");

    // Gravel extends beyond the road surface and fades out 2 m from its edge.
    let road = &plains.roads()[0].points;
    let key = ChunkKey::containing(road[road.len() / 2], 0);
    let (weights, sites) = painted(&plains, &stack, key);
    let mut shoulders = 0;
    for (n, site) in sites.iter().enumerate() {
        let (i, j) = texel(n);
        let off_road = !site.road_tags.unwrap().contains(RoadTags::ROAD);
        let gentle = site.slope < 0.4;
        if off_road && gentle && weights.weight(i, j, gravel) > 128 {
            shoulders += 1;
            assert!(site.road_distance.unwrap() < 2.0, "{site:?}");
        }
        if site.road_distance.unwrap() >= 2.0 {
            assert_eq!(weights.weight(i, j, gravel), 0, "{site:?}");
        }
    }
    assert!(
        shoulders > 20,
        "gravel beside the road on {shoulders} texels"
    );
}

#[test]
fn a_cliff_label_picks_cliff_rock() {
    let stack = banks_and_roads();
    let rock = stack.catalogue().id("cliff_rock").unwrap();
    let labelled = |landform| {
        GroundSite::new(40.0, Vec2::new(0.2, 0.0)).with_labels(Labels {
            landform,
            flags: Flags::NONE,
        })
    };
    // At this gentle slope, only the cliff label can select rock.
    assert_eq!(
        stack
            .paint(DVec2::ZERO, &labelled(Landform::Slope))
            .weight(rock),
        0.0
    );
    assert_eq!(
        stack
            .paint(DVec2::ZERO, &labelled(Landform::Cliff))
            .weight(rock),
        1.0
    );

    // Check a generated mountain chunk too: every texel labelled as a cliff
    // should be covered in rock.
    let mountains = TerrainGenerator::new(fixtures::mountains_medium().terrain.clone())
        .unwrap()
        .build(
            Region {
                id: RegionId(1),
                boundary: Boundary::Polygon(vec![
                    DVec2::ZERO,
                    DVec2::new(1500.0, 0.0),
                    DVec2::new(1500.0, 1500.0),
                    DVec2::new(0.0, 1500.0),
                ]),
                edges: Vec::new(),
                corners: Vec::new(),
            },
            &[],
        )
        .unwrap();
    let cliff = a_cliff(&mountains).expect("the mountains have a cliff");
    let (weights, sites) = painted(&mountains, &stack, ChunkKey::containing(cliff, 0));
    let mut cliffs = 0;
    for (n, site) in sites.iter().enumerate() {
        if site.labels.unwrap().landform == Landform::Cliff {
            cliffs += 1;
            let (i, j) = (n as u32 % GROUND_TEXELS, n as u32 / GROUND_TEXELS);
            assert!(weights.weight(i, j, rock) >= 254, "{site:?}");
        }
    }
    assert!(cliffs > 0, "texels of the chunk are cliffs");
}

/// Searches a 4 m grid within the region for a point labelled as a cliff.
/// Returns None if no sampled point qualifies.
fn a_cliff(terrain: &Terrain) -> Option<DVec2> {
    let procgena_terrain::Boundary::Polygon(corners) = &terrain.region().boundary else {
        panic!("test regions have polygons");
    };
    let (lo, hi) = corners
        .iter()
        .fold((DVec2::INFINITY, DVec2::NEG_INFINITY), |(lo, hi), &q| {
            (lo.min(q), hi.max(q))
        });
    let steps = ((hi - lo) / 4.0).as_uvec2();
    (0..steps.y)
        .flat_map(|j| (0..steps.x).map(move |i| lo + DVec2::new(f64::from(i), f64::from(j)) * 4.0))
        .filter(|&p| terrain.region().boundary.contains(p))
        // Filter by slope or a road cliff tag before computing the more costly labels.
        .filter(|&p| {
            terrain.gradient(p).length() > 1.0 || terrain.road_tags(p).contains(RoadTags::CLIFF)
        })
        .find(|&p| terrain.annotate(p).landform == Landform::Cliff)
}

/// Sampling nearby water heights should preserve riverbed coverage under
/// visible water compared with using only the water covering each point.
/// Allows a small fraction of texels to lose coverage, since nearby waters
/// can have different surface heights.
#[test]
fn bare_riverbeds_lie_under_drawn_water_read_with_measures() {
    use procgena_terrain::ground::{GroundGrid, Water};
    let bare = GroundStack::bare(3, 1.0);
    let riverbed = bare.catalogue().id("riverbed").unwrap();
    let (mut wet, mut lost) = (0, 0);
    for seed in RIVER_SEEDS {
        let world = hills_into_plains(seed, DEFAULT);
        for terrain in world.terrains() {
            let waters = terrain
                .rivers()
                .iter()
                .map(|r| r.points[r.points.len() / 2]);
            let ponds = terrain.ponds().iter().map(|p| p.centre);
            for p in waters.chain(ponds) {
                let key = ChunkKey::containing(p, 0);
                let read = |detail| GroundGrid::chunk(terrain, key, FeatureTier::Fine, detail);
                let (rough, fine) = (
                    read(SampleDetail::Gradient).unwrap(),
                    read(SampleDetail::Measures).unwrap(),
                );
                let (a, b) = (rough.paint(&bare), fine.paint(&bare));
                for (n, site) in fine.sites().iter().enumerate() {
                    if !matches!(site.water, Some(Water::Level(_))) {
                        continue;
                    }
                    let (i, j) = (n as u32 % GROUND_TEXELS, n as u32 / GROUND_TEXELS);
                    let (was, now) = (a.weight(i, j, riverbed), b.weight(i, j, riverbed));
                    wet += 1;
                    if u32::from(now) + 64 < u32::from(was) {
                        lost += 1;
                    }
                }
            }
        }
    }
    eprintln!("{wet} texels under water, {lost} lost a quarter of their riverbed");
    assert!(wet > 10_000, "{wet} texels under water");
    assert!(lost * 1000 < wet, "{lost} of {wet} texels lost riverbed");
}
