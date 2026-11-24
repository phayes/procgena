//! Placing an Ecotope in the background from a component, and redrawing
//! only the Scatters an edit changed.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use procgena_biome::{Ecotope, EcotopeError, EcotopePlacement};
use procgena_core::scatter::{Mask, ScatterArea, ScatterInput};
use procgena_core::{ChildKey, Ground};

use crate::spawn_layer;

/// Places and draws [`EcotopeParams`] entities. Add it once, with the tree,
/// stone, dead wood and mushroom plugins that draw the copies.
pub struct EcotopePlugin;

impl Plugin for EcotopePlugin {
    fn build(&self, app: &mut App) {
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
}

impl EcotopeParams {
    /// Whether `other` asks for the same placement: an equal Ecotope and
    /// seed, the same ground and coverage (`Arc`s), and an equal area.
    fn same_as(&self, other: &EcotopeParams) -> bool {
        (Arc::ptr_eq(&self.ecotope, &other.ecotope) || self.ecotope == other.ecotope)
            && self.seed == other.seed
            && self.same_ground(other)
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

/// The placement running for an entity, and what it was asked for.
#[derive(Component)]
struct Placing {
    params: EcotopeParams,
    task: Task<(Result<EcotopePlacement, EcotopeError>, Duration)>,
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
}

/// Starts placing every Ecotope whose params differ from the last placement
/// started for it. A placement already running is let finish first, so an
/// Ecotope edited every frame still updates.
fn start_placing(
    mut commands: Commands,
    ecotopes: Query<(Entity, &EcotopeParams, Option<&Requested>, Has<Placing>)>,
) {
    for (entity, params, requested, placing) in &ecotopes {
        if placing || requested.is_some_and(|r| r.0.same_as(params)) {
            continue;
        }
        let input = params.clone();
        let task = AsyncComputeTaskPool::get().spawn(async move {
            let started = Instant::now();
            let scatter_input = ScatterInput {
                mask: input.coverage.as_deref(),
                ..ScatterInput::new(&*input.ground, input.area, input.seed)
            };
            let result = input.ecotope.place(&scatter_input);
            (result, started.elapsed())
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
) {
    for (entity, mut p, current, drawn) in &mut placing {
        let Some((result, place_time)) = block_on(poll_once(&mut p.task)) else {
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
        commands.entity(entity).insert(Drawn {
            params,
            placement,
            layers,
        });
        commands.trigger(EcotopeFinished {
            entity,
            result: Ok(EcotopeSummary {
                layers: summaries,
                place_time,
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

    use super::*;
    use crate::EcotopeLayer;

    /// The placements finished so far.
    #[derive(Resource, Default)]
    struct Finished(Vec<Result<EcotopeSummary, EcotopeError>>);

    /// An app drawing the pine forest floor over a small patch of hills,
    /// and its Ecotope entity.
    fn app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), EcotopePlugin))
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
            })
            .id();
        (app, entity)
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
        assert_eq!(before.len(), 5);

        // Thinning the lowest Scatter redraws it alone.
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
        assert_eq!(redrawn, ["fly_agarics"]);
        let after = layers(&mut app, entity);
        for ((key, old), (_, new)) in before.iter().zip(&after) {
            assert_eq!(old == new, key != "fly_agarics", "{key}");
        }
        assert!(
            app.world().get_entity(before[1].1).is_err(),
            "old layer removed"
        );

        // An Ecotope that can't be placed keeps the last drawing.
        edit(&mut app, entity, |ecotope| {
            let key = ChildKey::from_static("logs");
            ecotope.scatters.get_mut(&key).unwrap().clearance = -1.0;
        });
        assert!(finish(&mut app, 3).is_err());
        assert_eq!(layers(&mut app, entity), after);
    }
}
