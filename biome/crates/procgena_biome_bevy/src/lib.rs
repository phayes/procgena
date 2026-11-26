//! Bevy drawing for [`procgena_biome`]: draws one placed
//! [`Ecotope`] over a patch of ground, each Scatter
//! with its own asset's Bevy crate.
//!
//! Place the Ecotope first with
//! [`Ecotope::place`](procgena_biome::Ecotope::place), then call
//! [`spawn_ecotope`] with the same ground, area and coverage. Trees, stones,
//! dead wood, mushrooms and plants are drawn by `procgena_tree_bevy`,
//! `procgena_stone_bevy`, `procgena_deadwood_bevy`, `procgena_mushroom_bevy`
//! and `procgena_plant_bevy`, so add their plugins to the app:
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use bevy::prelude::*;
//! use procgena_biome::EcotopePreset;
//! use procgena_biome_bevy::spawn_ecotope;
//! use procgena_core::scatter::{ScatterArea, ScatterInput};
//! use procgena_core::{FlatGround, Ground};
//! use procgena_deadwood_bevy::DeadwoodPlugin;
//! use procgena_mushroom_bevy::MushroomPlugin;
//! use procgena_plant_bevy::PlantPlugins;
//! use procgena_stone_bevy::StonePlugin;
//! use procgena_tree_bevy::TreePlugin;
//!
//! fn setup(mut commands: Commands) {
//!     let ecotope = &EcotopePreset::pine_forest_floor().ecotope;
//!     let ground: Arc<dyn Ground> = Arc::new(FlatGround);
//!     let area = ScatterArea::new(bevy::math::DVec2::ZERO, Vec2::splat(30.0));
//!     let input = ScatterInput::new(&*ground, area, ecotope.default_seed);
//!     let placed = ecotope.place(&input).expect("the preset places");
//!     spawn_ecotope(&mut commands, ecotope, &placed, ground, area, None);
//! }
//!
//! App::new()
//!     .add_plugins((
//!         DefaultPlugins,
//!         TreePlugin { texture_root: None, ..default() },
//!         StonePlugin { texture_root: None, ..default() },
//!         DeadwoodPlugin { texture_root: None, ..default() },
//!         MushroomPlugin { texture_root: None, ..default() },
//!         PlantPlugins,
//!     ))
//!     .add_systems(Startup, setup)
//!     .run();
//! ```
//!
//! Each copy appears when its asset finishes generating in the background.
//! Copies with the same settings share meshes and materials.

use std::sync::Arc;

use bevy::math::DVec2;
use bevy::prelude::*;
use procgena_biome::{Ecotope, EcotopePlacement, FootprintMask, ScatterCopies, ScatterKind};
use procgena_core::Ground;
use procgena_core::scatter::{Mask, ScatterArea};
use procgena_deadwood_bevy::DeadwoodPlacedParams;
use procgena_mushroom_bevy::MushroomGroundScatterParams;
use procgena_plant_bevy::PlantScatter;
#[cfg(not(feature = "gpu"))]
use procgena_stone_bevy::StoneParams;
use procgena_tree_bevy::TreeParams;

mod ground;
mod placing;
#[cfg(feature = "gpu")]
mod stones;

pub use ground::{EcotopeGroundPatch, GroundPatch, ground_mesh, rolling_hills};
pub use placing::{
    EcotopeCopies, EcotopeFinished, EcotopeParams, EcotopePlacing, EcotopePlugin, EcotopeSummary,
    LayerSummary,
};

/// Marks the entity that holds an Ecotope drawn by [`spawn_ecotope`]. It
/// stands at the World origin and holds one [`EcotopeLayer`] child per
/// Scatter.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[require(Transform, Visibility)]
pub struct EcotopeRoot;

/// Marks the entity that holds one Scatter of a drawn Ecotope. It stands at
/// the centre of the area the Ecotope was placed over, at height 0. Its
/// children are the Scatter's copies: one [`TreeParams`] or
/// [`StoneParams`](procgena_stone_bevy::StoneParams) per copy, or one
/// [`DeadwoodPlacedParams`] or [`MushroomGroundScatterParams`] for the
/// whole Scatter.
///
/// With the `gpu` feature, stones are instead drawn through GPU-generated
/// pools: one child per stone class, holding one
/// [`PooledStone`](procgena_stone_bevy::PooledStone) per stone. Add the
/// stone plugin with its own `gpu` feature to draw them.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
#[require(Transform, Visibility)]
pub struct EcotopeLayer {
    /// The Scatter's key in its Ecotope.
    pub key: String,
    /// The Scatter's priority.
    pub priority: u32,
}

/// Spawns entities that draw `placed`, the copies of `ecotope` placed over
/// `area` of `ground` with `coverage` as the input's mask. Returns the
/// [`EcotopeRoot`] entity.
///
/// Pass the same Ecotope, ground, area and coverage the placement was made
/// with: mushrooms are placed again by `procgena_mushroom_bevy`, and come
/// out the same only from the same inputs. To place in the background and
/// redraw only what an edit changes, use [`EcotopeParams`] instead.
///
/// # Panics
///
/// Panics if `placed` holds a Scatter that `ecotope` doesn't, or of a
/// different kind.
pub fn spawn_ecotope(
    commands: &mut Commands,
    ecotope: &Ecotope,
    placed: &EcotopePlacement,
    ground: Arc<dyn Ground>,
    area: ScatterArea,
    coverage: Option<Arc<dyn Mask>>,
) -> Entity {
    let root = commands.spawn(EcotopeRoot).id();
    for index in 0..placed.layers().len() {
        spawn_layer(
            commands,
            root,
            ecotope,
            placed,
            index,
            ground.clone(),
            area,
            coverage.clone(),
        );
    }
    root
}

/// Spawns the [`EcotopeLayer`] drawing the layer at `index` of `placed` as
/// a child of `parent`, and returns it. The other arguments are as for
/// [`spawn_ecotope`].
#[allow(clippy::too_many_arguments)]
fn spawn_layer(
    commands: &mut Commands,
    parent: Entity,
    ecotope: &Ecotope,
    placed: &EcotopePlacement,
    index: usize,
    ground: Arc<dyn Ground>,
    area: ScatterArea,
    coverage: Option<Arc<dyn Mask>>,
) -> Entity {
    let layer = &placed.layers()[index];
    let settings = &ecotope.scatters[layer.key()];
    let centre = area.centre.as_vec2();
    let holder = commands
        .spawn((
            EcotopeLayer {
                key: layer.key().to_string(),
                priority: layer.priority(),
            },
            Transform::from_xyz(centre.x, 0.0, centre.y),
            ChildOf(parent),
        ))
        .id();
    let at = |transform: &bevy::math::Affine3A| {
        (
            Transform::from_matrix(Mat4::from(*transform)),
            ChildOf(holder),
        )
    };
    match (&settings.scatter, layer.copies()) {
        (ScatterKind::Tree(scatter), ScatterCopies::Tree(trees)) => {
            for tree in trees.instances() {
                let options = scatter.trees[tree.tree].variant(tree.variant);
                commands.spawn((TreeParams::new(options), at(&tree.transform)));
            }
        }
        #[cfg(feature = "gpu")]
        (ScatterKind::Stone(_), copies @ ScatterCopies::Stone(_)) => {
            // One GPU pool per class, drawing that class's stones.
            for class in stones::pooled_classes(copies) {
                commands.spawn((class, ChildOf(holder)));
            }
        }
        #[cfg(not(feature = "gpu"))]
        (ScatterKind::Stone(_), ScatterCopies::Stone(stones)) => {
            for stone in stones.instances() {
                let options = stones.variant(stone.class, stone.variant).clone();
                commands.spawn((StoneParams::new(options), at(&stone.transform)));
            }
        }
        (ScatterKind::Deadwood(_), ScatterCopies::Deadwood(wood)) => {
            // Each variant meshed once, every copy drawn instanced.
            commands.spawn((
                DeadwoodPlacedParams {
                    sticks: Arc::new(wood.clone()),
                    world_seed: layer.seed(),
                },
                ChildOf(holder),
            ));
        }
        (ScatterKind::Mushroom { mushroom, scatter }, ScatterCopies::Mushroom(_)) => {
            // Placed again by the mushroom crate, with the same input.
            let mut scatter = scatter.clone();
            scatter.default_seed = layer.seed();
            let above = placed
                .mask_above(layer.key().as_str())
                .expect("the layer is in the placement");
            commands.spawn((
                MushroomGroundScatterParams {
                    mushroom: Arc::new(mushroom.clone()),
                    scatter,
                    ground,
                    area,
                    mask: Some(Arc::new(Covered { above, coverage })),
                },
                ChildOf(holder),
            ));
        }
        (ScatterKind::Plant(options), ScatterCopies::Plant(bake)) => {
            // Plants are placed where they are drawn, over the ground and
            // density the Ecotope baked, with the Scatter's seed.
            let field = commands.spawn((Transform::IDENTITY, ChildOf(holder))).id();
            let (options, bake, seed) = (options.clone(), bake.clone(), layer.seed());
            let key = layer.key().to_string();
            commands.queue(move |world: &mut World| {
                let Some(mut images) = world.get_resource_mut::<Assets<Image>>() else {
                    warn!(
                        "procgena_biome_bevy: plant Scatter {key} is not drawn: the app has no \
                         image assets; add PlantPlugins"
                    );
                    return;
                };
                let scatter = PlantScatter {
                    seed: Some(seed),
                    ..PlantScatter::with_options(Vec2::ZERO, options).with_bake(&mut images, &bake)
                };
                if let Ok(mut entity) = world.get_entity_mut(field) {
                    entity.insert(scatter);
                }
            });
        }
        (kind, _) => panic!(
            "Scatter {} placed as another kind than its {} settings",
            layer.key(),
            kind.name()
        ),
    }
    holder
}

/// The mask a mushroom Scatter was placed with: the Ecotope's coverage,
/// kept out of the footprints above it.
struct Covered {
    above: FootprintMask,
    coverage: Option<Arc<dyn Mask>>,
}

impl Mask for Covered {
    fn density(&self, p: DVec2) -> f32 {
        if self.above.blocks(p) {
            return 0.0;
        }
        self.coverage.as_ref().map_or(1.0, |c| c.density(p))
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::AssetPlugin;
    use bevy::ecs::system::RunSystemOnce;
    use procgena_biome::EcotopePreset;
    use procgena_core::HeightGrid;
    use procgena_core::scatter::ScatterInput;
    use procgena_mushroom::MushroomScatter;

    use super::*;

    #[test]
    fn every_copy_is_spawned_under_its_layer() {
        // Place the pine forest floor over a small patch of hills.
        let ecotope = EcotopePreset::pine_forest_floor().ecotope.clone();
        let ground: Arc<dyn Ground> = Arc::new(HeightGrid::from_fn(
            DVec2::splat(-12.0),
            DVec2::splat(12.0),
            0.5,
            |p| 0.3 * (0.2 * p.x as f32).sin(),
        ));
        let area = ScatterArea::new(DVec2::ZERO, Vec2::splat(20.0));
        let input = ScatterInput::new(&*ground, area, ecotope.default_seed);
        let placed = ecotope.place(&input).unwrap();

        // Spawn it in a headless app.
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>();
        let (e, p) = (ecotope.clone(), placed.clone());
        let root = app
            .world_mut()
            .run_system_once(move |mut commands: Commands| {
                spawn_ecotope(&mut commands, &e, &p, ground.clone(), area, None)
            })
            .unwrap();
        app.update();

        // One layer per Scatter, in priority order, each holding its copies.
        let world = app.world_mut();
        assert!(world.get::<EcotopeRoot>(root).is_some());
        let layers: Vec<Entity> = world.get::<Children>(root).unwrap().to_vec();
        assert_eq!(layers.len(), placed.layers().len());
        for (entity, layer) in layers.iter().zip(placed.layers()) {
            let marker = world.get::<EcotopeLayer>(*entity).unwrap();
            assert_eq!(marker.key, layer.key().as_str());
            assert_eq!(marker.priority, layer.priority());
            let children: Vec<Entity> = world
                .get::<Children>(*entity)
                .map(|c| c.to_vec())
                .unwrap_or_default();
            let count = |has: fn(&World, Entity) -> bool| {
                children.iter().filter(|c| has(world, **c)).count()
            };
            match layer.copies() {
                ScatterCopies::Tree(_) => assert_eq!(
                    count(|w, e| w.get::<TreeParams>(e).is_some()),
                    layer.copies().len()
                ),
                #[cfg(not(feature = "gpu"))]
                ScatterCopies::Stone(_) => assert_eq!(
                    count(|w, e| w.get::<StoneParams>(e).is_some()),
                    layer.copies().len()
                ),
                // One pooled class per stone class.
                #[cfg(feature = "gpu")]
                ScatterCopies::Stone(placed) => assert_eq!(
                    count(|w, e| w.get::<crate::stones::PooledClass>(e).is_some()),
                    placed.options().classes.len()
                ),
                ScatterCopies::Deadwood(_) => {
                    assert_eq!(count(|w, e| w.get::<DeadwoodPlacedParams>(e).is_some()), 1)
                }
                ScatterCopies::Plant(bake) => {
                    // One field over the area, from the bake, with the
                    // Scatter's seed.
                    assert_eq!(count(|w, e| w.get::<PlantScatter>(e).is_some()), 1);
                    let field = world.get::<PlantScatter>(children[0]).unwrap();
                    assert_eq!(field.size, bake.size);
                    assert_eq!(field.seed, Some(layer.seed()));
                    assert!(field.density_map.is_some());
                }
                ScatterCopies::Mushroom(_) => {
                    assert_eq!(
                        count(|w, e| w.get::<MushroomGroundScatterParams>(e).is_some()),
                        1
                    );
                    // The mushroom crate places the same copies from it.
                    let params = world
                        .get::<MushroomGroundScatterParams>(children[0])
                        .unwrap();
                    let input = ScatterInput {
                        mask: params.mask.as_deref(),
                        ..ScatterInput::new(
                            &*params.ground,
                            params.area,
                            params.scatter.default_seed,
                        )
                    };
                    let again =
                        MushroomScatter::new(&params.mushroom, &params.scatter, &input).unwrap();
                    let ScatterCopies::Mushroom(original) = layer.copies() else {
                        unreachable!()
                    };
                    assert!(!original.copies().is_empty(), "{}", layer.key());
                    assert_eq!(again.copies(), original.copies(), "{}", layer.key());
                }
            }
        }
    }
}
