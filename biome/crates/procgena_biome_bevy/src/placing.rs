//! Placing an Ecotope in the background from a component, and redrawing
//! only the Scatters an edit changed.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use procgena_biome::{Ecotope, EcotopeError, EcotopePlacement};
use procgena_core::scatter::{Mask, ScatterArea, ScatterInput};
use procgena_core::{ChildKey, Ground};
use procgena_terrain::ground::GroundWeights;
use procgena_terrain_bevy::{
    GroundTextureSettings, GroundTextures, TerrainExtension, TerrainMaterial, TerrainMaterialPlugin,
};

use crate::ground::{EcotopeGroundPatch, GroundPatch, ground_mesh, patch_weights};
use crate::spawn_layer;

/// Places and draws [`EcotopeParams`] entities. Add it once, with the tree,
/// stone, dead wood and mushroom plugins that draw the copies. It adds
/// `procgena_terrain_bevy`'s [`TerrainMaterialPlugin`] for the ground, if
/// it isn't added yet.
pub struct EcotopePlugin;

impl Plugin for EcotopePlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<TerrainMaterialPlugin>() {
            app.add_plugins(TerrainMaterialPlugin);
        }
        app.add_systems(Update, (start_placing, finish_placing).chain());
    }
}

/// Asks for an Ecotope to be placed over `area` of `ground` and drawn under
/// this entity, with [`EcotopePlugin`].
///
/// The Ecotope is placed in the background, and drawn as one
/// [`EcotopeLayer`](crate::EcotopeLayer) child per Scatter once that is done.
/// [`EcotopeFinished`] is triggered on the entity each time a placement ends.
/// Replace the component to change what is drawn: the Ecotope is placed
/// again, and only the Scatters whose copies may have moved are drawn
/// again. A Scatter is kept when its own settings and seed are unchanged and
/// every Scatter above it leaves the same footprints, over the same ground
/// (the same `Arc`), area and coverage. If a placement fails, the last
/// drawing stays.
///
/// The entity's transform moves the whole drawing; copies stand at their
/// World positions when it is the identity.
#[derive(Component, Clone)]
#[require(Transform, Visibility)]
pub struct EcotopeParams {
    /// The Ecotope to place.
    pub ecotope: Arc<Ecotope>,
    /// The seed to place it with, such as its
    /// [`default_seed`](Ecotope::default_seed).
    pub seed: u32,
    /// The ground its copies stand on.
    pub ground: Arc<dyn Ground>,
    /// The stretch of ground it covers.
    pub area: ScatterArea,
    /// How much of the area it covers, from 0 to 1, or `None` for all of it.
    pub coverage: Option<Arc<dyn Mask>>,
    /// Also draw the ground under the Ecotope, with its ground layers, as
    /// an [`EcotopeGroundPatch`] child; `None` draws only the copies. The
    /// ground is painted after each placement, from its copies, and stays
    /// as last drawn if it can't be painted.
    pub patch: Option<GroundPatch>,
}

impl EcotopeParams {
    /// Whether `other` asks for the same placement and drawing: an equal
    /// Ecotope and seed, the same ground and coverage (`Arc`s), an equal
    /// area and the same ground patch.
    fn same_as(&self, other: &EcotopeParams) -> bool {
        (Arc::ptr_eq(&self.ecotope, &other.ecotope) || self.ecotope == other.ecotope)
            && self.seed == other.seed
            && self.same_ground(other)
            && match (&self.patch, &other.patch) {
                (None, None) => true,
                (Some(a), Some(b)) => a.same_as(b),
                _ => false,
            }
    }

    /// Whether `other` places over the same ground, area and coverage.
    fn same_ground(&self, other: &EcotopeParams) -> bool {
        Arc::ptr_eq(&self.ground, &other.ground)
            && self.area == other.area
            && match (&self.coverage, &other.coverage) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
    }
}

/// Present on an [`EcotopeParams`] entity while its Ecotope is being placed.
/// The copies' own assets may still be generating after it is removed.
#[derive(Component, Debug, Default)]
pub struct EcotopePlacing;

/// Triggered on an [`EcotopeParams`] entity when a placement ends.
#[derive(EntityEvent, Debug, Clone, PartialEq)]
pub struct EcotopeFinished {
    /// The entity holding the [`EcotopeParams`].
    pub entity: Entity,
    /// What was placed and redrawn, or why the Ecotope could not be placed.
    /// After an error the previous drawing stays.
    pub result: Result<EcotopeSummary, EcotopeError>,
}

/// What one placement of an Ecotope gave.
#[derive(Debug, Clone, PartialEq)]
pub struct EcotopeSummary {
    /// Each Scatter, highest priority first.
    pub layers: Vec<LayerSummary>,
    /// How long placing took, not counting generating the copies' assets.
    pub place_time: Duration,
    /// How long painting the ground patch's layers took, if it was
    /// painted.
    pub ground_time: Option<Duration>,
}

impl EcotopeSummary {
    /// The number of copies of every Scatter together.
    pub fn copies(&self) -> usize {
        self.layers.iter().map(|layer| layer.copies).sum()
    }
}

/// One Scatter of a placed Ecotope.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerSummary {
    /// The Scatter's key.
    pub key: ChildKey,
    /// The Scatter's priority.
    pub priority: u32,
    /// How many copies were placed. A cluster of mushrooms counts as one.
    pub copies: usize,
    /// Whether the Scatter was drawn again; `false` when the drawing from
    /// the previous placement was kept.
    pub redrawn: bool,
}

/// What a placement task gives: the placement and how long it took, and
/// the ground patch's weights and how long painting them took, if asked
/// for and the placement succeeded.
type Placed = (
    Result<EcotopePlacement, EcotopeError>,
    Duration,
    Option<(Result<GroundWeights, String>, Duration)>,
);

/// The placement running for an entity, and what it was asked for.
#[derive(Component)]
struct Placing {
    params: EcotopeParams,
    task: Task<Placed>,
}

/// The last placement started for an entity.
#[derive(Component)]
struct Requested(EcotopeParams);

/// What an entity draws now: the placement and what it was made from, and
/// the entity of each Scatter's layer.
#[derive(Component)]
struct Drawn {
    params: EcotopeParams,
    placement: EcotopePlacement,
    layers: Vec<(ChildKey, Entity)>,
    /// The ground patch's entity, if one is drawn.
    patch: Option<Entity>,
}

/// An Ecotope entity's params, the last placement started for it, what it
/// draws now, and whether a placement is running.
type PlacingState<'a> = (
    Entity,
    &'a EcotopeParams,
    Option<&'a Requested>,
    Option<&'a Drawn>,
    Has<Placing>,
);

/// Starts placing every Ecotope whose params differ from the last placement
/// started for it. A placement already running is let finish first, so an
/// Ecotope edited every frame still updates.
fn start_placing(mut commands: Commands, ecotopes: Query<PlacingState>) {
    for (entity, params, requested, drawn, placing) in &ecotopes {
        if placing || requested.is_some_and(|r| r.0.same_as(params)) {
            continue;
        }
        // Reuse what the drawing on screen placed, if it is over the same
        // ground, so an edit places again only what it can change.
        let previous = drawn
            .filter(|drawn| drawn.params.same_ground(params))
            .map(|drawn| drawn.placement.clone());
        let input = params.clone();
        let task = AsyncComputeTaskPool::get().spawn(async move {
            let started = Instant::now();
            let scatter_input = ScatterInput {
                mask: input.coverage.as_deref(),
                ..ScatterInput::new(&*input.ground, input.area, input.seed)
            };
            let result = match &previous {
                Some(previous) => input.ecotope.place_again(&scatter_input, previous),
                None => input.ecotope.place(&scatter_input),
            };
            let place_time = started.elapsed();

            // Paint the ground patch from the copies just placed.
            let ground = match (&result, &input.patch) {
                (Ok(placement), Some(patch)) => {
                    let started = Instant::now();
                    let weights =
                        patch_weights(&input.ecotope, placement, &*input.ground, input.area, patch);
                    Some((weights, started.elapsed()))
                }
                _ => None,
            };
            (result, place_time, ground)
        });
        commands.entity(entity).insert((
            EcotopePlacing,
            Requested(params.clone()),
            Placing {
                params: params.clone(),
                task,
            },
        ));
    }
}

/// Draws each finished placement, keeping the layers it leaves unchanged,
/// and triggers [`EcotopeFinished`].
fn finish_placing(
    mut commands: Commands,
    mut placing: Query<(Entity, &mut Placing, &EcotopeParams, Option<&Drawn>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
    textures: Res<GroundTextures>,
    texture_settings: Res<GroundTextureSettings>,
) {
    for (entity, mut p, current, drawn) in &mut placing {
        let Some((result, place_time, ground)) = block_on(poll_once(&mut p.task)) else {
            continue;
        };
        let params = p.params.clone();
        commands.entity(entity).remove::<Placing>();
        if params.same_as(current) {
            commands.entity(entity).remove::<EcotopePlacing>();
        }
        let placement = match result {
            Ok(placement) => placement,
            Err(error) => {
                commands.trigger(EcotopeFinished {
                    entity,
                    result: Err(error),
                });
                continue;
            }
        };

        // Keep each layer that the previous drawing placed the same way;
        // draw the rest again.
        let old = drawn;
        let mut layers = Vec::with_capacity(placement.layers().len());
        let mut summaries = Vec::with_capacity(placement.layers().len());
        for index in 0..placement.layers().len() {
            let layer = &placement.layers()[index];
            let kept = old.and_then(|old| {
                unchanged(old, &params, &placement, index).then(|| {
                    old.layers
                        .iter()
                        .find(|(key, _)| key == layer.key())
                        .map(|(_, entity)| *entity)
                })?
            });
            let layer_entity = match kept {
                Some(kept) => kept,
                None => spawn_layer(
                    &mut commands,
                    entity,
                    &params.ecotope,
                    &placement,
                    index,
                    params.ground.clone(),
                    params.area,
                    params.coverage.clone(),
                ),
            };
            layers.push((layer.key().clone(), layer_entity));
            summaries.push(LayerSummary {
                key: layer.key().clone(),
                priority: layer.priority(),
                copies: layer.copies().len(),
                redrawn: kept.is_none(),
            });
        }

        // Remove the old layers that weren't kept.
        if let Some(old) = old {
            for (_, layer) in &old.layers {
                if !layers.iter().any(|(_, kept)| kept == layer) {
                    commands.entity(*layer).despawn();
                }
            }
        }
        // Draw the ground patch in its entity, made once and kept while a
        // patch is asked for; a patch that couldn't be painted keeps its
        // last drawing.
        let mut patch = old.and_then(|old| old.patch);
        let mut ground_time = None;
        match (&params.patch, ground) {
            (Some(settings), Some((Ok(weights), time))) => {
                ground_time = Some(time);
                let (min, max, _) = settings.rectangle(params.area);
                let mesh = ground_mesh(&*params.ground, min, max, settings.cell);
                let extension = TerrainExtension::new(
                    &weights,
                    settings.base.catalogue(),
                    Some((&textures, &texture_settings)),
                    &mut images,
                );
                let material = TerrainMaterial {
                    base: StandardMaterial {
                        reflectance: 0.2,
                        ..default()
                    },
                    extension,
                };
                let target = *patch.get_or_insert_with(|| {
                    commands.spawn((EcotopeGroundPatch, ChildOf(entity))).id()
                });
                commands.entity(target).insert((
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(materials.add(material)),
                ));
            }
            (Some(_), Some((Err(error), _))) => {
                warn!("the Ecotope's ground can't be painted: {error}");
            }
            (None, _) => {
                if let Some(old) = patch.take() {
                    commands.entity(old).despawn();
                }
            }
            (Some(_), None) => {}
        }

        commands.entity(entity).insert(Drawn {
            params,
            placement,
            layers,
            patch,
        });
        commands.trigger(EcotopeFinished {
            entity,
            result: Ok(EcotopeSummary {
                layers: summaries,
                place_time,
                ground_time,
            }),
        });
    }
}

/// Whether the layer at `index` of `placement`, made from `params`, would
/// be drawn the same as the layer with its key in `old`: its settings and
/// seed are equal, it is placed over the same ground, and the layers above
/// it leave the same footprints.
fn unchanged(
    old: &Drawn,
    params: &EcotopeParams,
    placement: &EcotopePlacement,
    index: usize,
) -> bool {
    let layer = &placement.layers()[index];
    let key = layer.key();
    let Some(old_index) = old.placement.layers().iter().position(|l| l.key() == key) else {
        return false;
    };
    let old_layer = &old.placement.layers()[old_index];
    let above = &placement.layers()[..index];
    let old_above = &old.placement.layers()[..old_index];
    old.params.same_ground(params)
        && old_layer.seed() == layer.seed()
        && old.params.ecotope.scatters.get(key) == params.ecotope.scatters.get(key)
        && above.len() == old_above.len()
        && above
            .iter()
            .zip(old_above)
            .all(|(new, old)| new.footprints() == old.footprints())
}

#[cfg(test)]
mod tests {
    use bevy::asset::AssetPlugin;
    use bevy::math::DVec2;
    use procgena_biome::{EcotopePreset, ScatterKind};
    use procgena_core::HeightGrid;
    use procgena_terrain::ground::GroundStack;

    use super::*;
    use crate::EcotopeLayer;

    /// The placements finished so far.
    #[derive(Resource, Default)]
    struct Finished(Vec<Result<EcotopeSummary, EcotopeError>>);

    /// An app drawing the pine forest floor over a small patch of hills,
    /// with its ground, and its Ecotope entity.
    fn app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::mesh::MeshPlugin,
        ))
        // The terrain material's shader modules load when the plugin is
        // added, so shaders must be an asset type by then.
        .init_asset::<bevy::shader::Shader>()
        .add_plugins(EcotopePlugin)
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .init_resource::<Finished>()
        .add_observer(|finished: On<EcotopeFinished>, mut all: ResMut<Finished>| {
            all.0.push(finished.result.clone());
        });
        let ground: Arc<dyn Ground> = Arc::new(HeightGrid::from_fn(
            DVec2::splat(-12.0),
            DVec2::splat(12.0),
            0.5,
            crate::rolling_hills,
        ));
        let ecotope = EcotopePreset::pine_forest_floor().ecotope.clone();
        let entity = app
            .world_mut()
            .spawn(EcotopeParams {
                seed: ecotope.default_seed,
                ecotope: Arc::new(ecotope),
                ground,
                area: ScatterArea::new(DVec2::ZERO, Vec2::splat(20.0)),
                coverage: None,
                patch: Some(GroundPatch {
                    base: Arc::new(GroundStack::bare(1, 1.0)),
                    margin: 2.0,
                    cell: 0.5,
                }),
            })
            .id();
        (app, entity)
    }

    /// The Ecotope entity's ground patch children.
    fn patches(app: &mut App, entity: Entity) -> Vec<Entity> {
        let world = app.world_mut();
        world
            .get::<Children>(entity)
            .map(|c| c.to_vec())
            .unwrap_or_default()
            .into_iter()
            .filter(|child| world.get::<EcotopeGroundPatch>(*child).is_some())
            .collect()
    }

    /// Runs `app` until `count` placements have finished, and returns the
    /// last.
    fn finish(app: &mut App, count: usize) -> Result<EcotopeSummary, EcotopeError> {
        for _ in 0..10_000 {
            app.update();
            let finished = &app.world().resource::<Finished>().0;
            if finished.len() >= count {
                return finished[count - 1].clone();
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("the placement did not finish");
    }

    /// Each layer's key and entity, in child order.
    fn layers(app: &mut App, entity: Entity) -> Vec<(String, Entity)> {
        let world = app.world_mut();
        let children: Vec<Entity> = world
            .get::<Children>(entity)
            .map(|c| c.to_vec())
            .unwrap_or_default();
        let mut layers: Vec<(String, Entity)> = children
            .into_iter()
            .filter_map(|child| {
                let layer = world.get::<EcotopeLayer>(child)?;
                Some((layer.key.clone(), child))
            })
            .collect();
        layers.sort();
        layers
    }

    /// The Ecotope entity's params with `edit` applied to its Ecotope.
    fn edit(app: &mut App, entity: Entity, edit: impl FnOnce(&mut Ecotope)) {
        let world = app.world_mut();
        let mut params = world.get::<EcotopeParams>(entity).unwrap().clone();
        let mut ecotope = (*params.ecotope).clone();
        edit(&mut ecotope);
        params.ecotope = Arc::new(ecotope);
        world.entity_mut(entity).insert(params);
    }

    #[test]
    fn an_edit_redraws_only_the_scatters_it_can_move() {
        // The first placement draws every Scatter.
        let (mut app, entity) = app();
        let first = finish(&mut app, 1).expect("the preset places");
        assert!(first.layers.iter().all(|layer| layer.redrawn));
        assert!(first.copies() > 0);
        assert!(!app.world().entity(entity).contains::<EcotopePlacing>());
        let before = layers(&mut app, entity);
        assert_eq!(before.len(), 6);

        // Thinning the fly agarics redraws them and the undergrowth below,
        // which keeps out of their footprints, and nothing above.
        edit(&mut app, entity, |ecotope| {
            let key = ChildKey::from_static("fly_agarics");
            let scatter = ecotope.scatters.get_mut(&key).unwrap();
            let ScatterKind::Mushroom { scatter, .. } = &mut scatter.scatter else {
                panic!("fly agarics are mushrooms");
            };
            scatter.density *= 0.5;
        });
        let second = finish(&mut app, 2).expect("the edit places");
        let redrawn: Vec<&str> = second
            .layers
            .iter()
            .filter(|layer| layer.redrawn)
            .map(|layer| layer.key.as_str())
            .collect();
        assert_eq!(redrawn, ["fly_agarics", "undergrowth"]);
        let after = layers(&mut app, entity);
        for ((key, old), (_, new)) in before.iter().zip(&after) {
            assert_eq!(old == new, !redrawn.contains(&key.as_str()), "{key}");
        }
        assert!(
            app.world().get_entity(before[1].1).is_err(),
            "old layer removed"
        );

        // Thinning the lowest Scatter, the undergrowth, redraws it alone.
        edit(&mut app, entity, |ecotope| {
            let key = ChildKey::from_static("undergrowth");
            let ScatterKind::Plant(options) = &mut ecotope.scatters.get_mut(&key).unwrap().scatter
            else {
                panic!("the undergrowth is plants");
            };
            options.density_scale *= 0.5;
        });
        let third = finish(&mut app, 3).expect("the edit places");
        let redrawn: Vec<&str> = third
            .layers
            .iter()
            .filter(|layer| layer.redrawn)
            .map(|layer| layer.key.as_str())
            .collect();
        assert_eq!(redrawn, ["undergrowth"]);
        let after = layers(&mut app, entity);

        // An Ecotope that can't be placed keeps the last drawing.
        edit(&mut app, entity, |ecotope| {
            let key = ChildKey::from_static("logs");
            ecotope.scatters.get_mut(&key).unwrap().clearance = -1.0;
        });
        assert!(finish(&mut app, 4).is_err());
        assert_eq!(layers(&mut app, entity), after);
    }

    /// The placement drawn now by the Ecotope entity.
    fn drawn(app: &App, entity: Entity) -> EcotopePlacement {
        app.world().get::<Drawn>(entity).unwrap().placement.clone()
    }

    /// The keys of the layers of `new` that still hold the same copies as
    /// `old`, not placed again.
    fn kept(old: &EcotopePlacement, new: &EcotopePlacement) -> Vec<String> {
        new.layers()
            .iter()
            .filter(|layer| {
                old.get(layer.key().as_str())
                    .is_some_and(|o| std::ptr::eq(o.copies(), layer.copies()))
            })
            .map(|layer| layer.key().to_string())
            .collect()
    }

    /// Whether `placement` matches placing the entity's Ecotope afresh.
    fn matches_fresh(app: &App, entity: Entity, placement: &EcotopePlacement) -> bool {
        let params = app.world().get::<EcotopeParams>(entity).unwrap();
        let input = ScatterInput::new(&*params.ground, params.area, params.seed);
        let fresh = params.ecotope.place(&input).unwrap();
        fresh.layers().len() == placement.layers().len()
            && fresh.layers().iter().zip(placement.layers()).all(|(a, b)| {
                a.key() == b.key()
                    && a.copies().len() == b.copies().len()
                    && a.footprints() == b.footprints()
            })
    }

    #[test]
    fn an_edit_places_again_only_the_scatters_it_can_move() {
        let (mut app, entity) = app();
        finish(&mut app, 1).expect("the preset places");
        let first = drawn(&app, entity);

        // Thinning the fly agarics keeps the placement of every Scatter
        // above them, and places them and the undergrowth again.
        edit(&mut app, entity, |ecotope| {
            let key = ChildKey::from_static("fly_agarics");
            let ScatterKind::Mushroom { scatter, .. } =
                &mut ecotope.scatters.get_mut(&key).unwrap().scatter
            else {
                panic!("fly agarics are mushrooms");
            };
            scatter.density *= 0.5;
        });
        finish(&mut app, 2).expect("the edit places");
        let second = drawn(&app, entity);
        assert_eq!(
            kept(&first, &second),
            ["pines", "boulders", "logs", "porcini"]
        );
        assert!(matches_fresh(&app, entity, &second));

        // Thinning the pines places everything again, and the result still
        // matches a fresh placement, though the stones and logs reuse their
        // shapes.
        edit(&mut app, entity, |ecotope| {
            let key = ChildKey::from_static("pines");
            let ScatterKind::Tree(trees) = &mut ecotope.scatters.get_mut(&key).unwrap().scatter
            else {
                panic!("pines are trees");
            };
            trees.density_scale *= 0.9;
        });
        finish(&mut app, 3).expect("the edit places");
        let third = drawn(&app, entity);
        assert!(kept(&second, &third).is_empty());
        assert!(matches_fresh(&app, entity, &third));

        // A new seed places everything afresh.
        let world = app.world_mut();
        let mut params = world.get::<EcotopeParams>(entity).unwrap().clone();
        params.seed += 1;
        world.entity_mut(entity).insert(params);
        finish(&mut app, 4).expect("the new seed places");
        let fourth = drawn(&app, entity);
        assert!(kept(&third, &fourth).is_empty());
        assert!(matches_fresh(&app, entity, &fourth));
    }

    #[test]
    fn the_ground_patch_is_painted_with_the_ecotopes_layers() {
        // The first placement paints the patch in one child.
        let (mut app, entity) = app();
        let first = finish(&mut app, 1).expect("the preset places");
        assert!(first.ground_time.is_some());
        let drawn = patches(&mut app, entity);
        assert_eq!(drawn.len(), 1);
        let material = app
            .world()
            .get::<MeshMaterial3d<TerrainMaterial>>(drawn[0])
            .expect("the patch has a terrain material")
            .clone();
        assert!(app.world().get::<Mesh3d>(drawn[0]).is_some());
        let catalogue = procgena_terrain::ground::MaterialCatalogue::standard();
        let colour = |key: &str| {
            let id = catalogue.id(key).unwrap();
            Vec3::from(catalogue.get(id).1.colour)
        };
        let surface = app
            .world()
            .resource::<Assets<TerrainMaterial>>()
            .get(&material)
            .expect("the patch's material is added")
            .extension
            .surface;
        // A texel on each vertex: 24 m at 0.5 m.
        assert_eq!(surface.texels, Vec2::splat(49.0));
        let palette = surface.palette;
        for key in ["forest_soil", "needle_litter", "bare_dirt"] {
            assert!(
                palette.iter().any(|slot| slot.truncate() == colour(key)),
                "{key} in the palette"
            );
        }

        // An edit paints it again in the same entity.
        edit(&mut app, entity, |ecotope| {
            ecotope.ground.remove(&ChildKey::from_static("moss"));
        });
        let second = finish(&mut app, 2).expect("the edit places");
        assert!(second.layers.iter().all(|layer| !layer.redrawn));
        assert_eq!(patches(&mut app, entity), drawn);

        // Without a patch, the ground is no longer drawn.
        let world = app.world_mut();
        let mut params = world.get::<EcotopeParams>(entity).unwrap().clone();
        params.patch = None;
        world.entity_mut(entity).insert(params);
        let third = finish(&mut app, 3).expect("the same Ecotope places");
        assert_eq!(third.ground_time, None);
        app.update();
        assert!(patches(&mut app, entity).is_empty());
    }
}
