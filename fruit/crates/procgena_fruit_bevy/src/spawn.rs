//! Growing a cluster from an entity's settings and spawning its game-level
//! and lower meshes.

use std::sync::Arc;

use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use procgena_core::chain::{DistanceChain, Step, ViewScale};
use procgena_core::{LodRange, Quality};
use procgena_core_bevy::{LodQuality, visibility_range};
use procgena_fruit::{Fruit, FruitOptions, FruitPreset, OptionsError};

use crate::{fruit_material, fruit_mesh};

/// The settings of the cluster this entity draws. Changing or replacing
/// the component grows the cluster again.
///
/// The cluster is the first specimen of its pool, grown with its settings'
/// own [`default_seed`](FruitOptions::default_seed).
#[derive(Component, Clone, Debug, PartialEq)]
#[require(Transform, Visibility)]
pub struct FruitParams(pub Arc<FruitOptions>);

impl FruitParams {
    /// Draws a cluster with `options`.
    pub fn new(options: FruitOptions) -> Self {
        Self(Arc::new(options))
    }

    /// Draws the cluster of a built-in or loaded preset.
    pub fn preset(preset: &FruitPreset) -> Self {
        Self::new(preset.fruit.clone())
    }
}

/// One mesh step of a drawn cluster, a child of the entity with
/// [`FruitParams`].
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct FruitPart {
    /// The step: [`Step::Game`] or [`Step::Lower`].
    pub step: Step,
}

/// Triggered on an entity with [`FruitParams`] after its cluster was grown
/// and its meshes spawned, or with the settings' error.
#[derive(EntityEvent, Debug, Clone)]
pub struct FruitFinished {
    /// The entity whose cluster was grown.
    pub entity: Entity,
    /// The number of vertices and triangles of the game-level mesh, then
    /// of the lower mesh, or why the cluster couldn't be grown. On an error
    /// the old meshes stay.
    pub result: Result<[(usize, usize); 2], OptionsError>,
}

/// The fade bands of a cluster's game-level and lower meshes, for a
/// cluster `size` metres across seen through a [`ViewScale::DEFAULT`]
/// camera at `quality`: the shared [`DistanceChain`]'s, with the lower mesh
/// drawn until the cull, as a cluster on its own has no impostor.
fn mesh_ranges(size: f32, quality: Quality) -> [LodRange; 2] {
    let factor = DistanceChain::factor(size, ViewScale::DEFAULT);
    let [game, lower, impostor] = DistanceChain::DEFAULT.ranges(factor, quality);
    let lower = LodRange {
        fade_in: lower.fade_in,
        fade_out: impostor.fade_out,
    };
    [game, lower]
}

/// Grows the clusters whose settings changed, replacing their meshes.
pub(crate) fn build_changed(
    changed: Query<(Entity, &FruitParams, Option<&Children>), Changed<FruitParams>>,
    parts: Query<(), With<FruitPart>>,
    quality: Option<Res<LodQuality>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let quality = quality.map_or(Quality::High, |q| q.0);
    for (entity, params, children) in &changed {
        let fruit = match Fruit::new((*params.0).clone(), params.0.default_seed, 0) {
            Ok(fruit) => fruit,
            Err(error) => {
                commands.trigger(FruitFinished {
                    entity,
                    result: Err(error),
                });
                continue;
            }
        };

        // Replace the old meshes.
        for child in children.into_iter().flatten() {
            if parts.contains(*child) {
                commands.entity(*child).despawn();
            }
        }

        // One child per mesh step, fading by distance for this cluster's size.
        let ranges = mesh_ranges(fruit.size(), quality);
        let material = materials.add(fruit_material(fruit.options()));
        let lod = fruit.meshes();
        for (step, mesh) in lod.meshes() {
            if mesh.indices.is_empty() {
                continue;
            }
            let mut part = commands.spawn((
                FruitPart { step },
                Mesh3d(meshes.add(fruit_mesh(mesh))),
                MeshMaterial3d(material.clone()),
                ChildOf(entity),
            ));
            let range = &ranges[step.index()];
            if !range.is_always() {
                let range: VisibilityRange = visibility_range(range);
                part.insert(range);
            }
        }
        let sizes = lod
            .meshes()
            .map(|(_, mesh)| (mesh.vertex_count(), mesh.triangle_count()));
        commands.trigger(FruitFinished {
            entity,
            result: Ok(sizes),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An app that grows clusters, without a window or renderer.
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<bevy::shader::Shader>()
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .add_plugins(crate::FruitPlugin);
        app
    }

    /// The steps of every spawned part, game level first.
    fn parts(app: &mut App) -> Vec<Step> {
        let mut steps: Vec<Step> = app
            .world_mut()
            .query::<&FruitPart>()
            .iter(app.world())
            .map(|p| p.step)
            .collect();
        steps.sort_unstable_by_key(|step| step.index());
        steps
    }

    #[test]
    fn both_mesh_steps_spawn_once_and_are_replaced_on_change() {
        let mut app = app();
        let entity = app
            .world_mut()
            .spawn(FruitParams::preset(FruitPreset::apple()))
            .id();
        app.update();
        let both = vec![Step::Game, Step::Lower];
        assert_eq!(parts(&mut app), both);

        app.world_mut().entity_mut(entity).insert(FruitParams::new(
            procgena_fruit::FlowerPreset::dogwood_bloom().flower.clone(),
        ));
        app.update();
        app.update();
        assert_eq!(parts(&mut app), both);
    }

    /// The lower mesh takes over where the shared chain switches for the
    /// cluster's size, and is drawn until the cull.
    #[test]
    fn the_lower_mesh_is_drawn_from_the_chain_switch_to_the_cull() {
        let size = 0.1;
        let factor = DistanceChain::factor(size, ViewScale::DEFAULT);
        let [game, lower] = mesh_ranges(size, Quality::High);
        let [chain_game, chain_lower, impostor] =
            DistanceChain::DEFAULT.ranges(factor, Quality::High);
        assert_eq!(game, chain_game);
        assert_eq!(lower.fade_in, chain_lower.fade_in);
        assert_eq!(lower.fade_out, impostor.fade_out);
    }

    #[test]
    fn invalid_settings_report_an_error_and_keep_the_old_cluster() {
        let mut app = app();
        let entity = app
            .world_mut()
            .spawn(FruitParams::preset(FruitPreset::apple()))
            .id();
        app.update();
        let before = parts(&mut app);

        let errors = std::sync::Arc::new(std::sync::Mutex::new(0));
        let seen = errors.clone();
        app.world_mut()
            .entity_mut(entity)
            .observe(move |finished: On<FruitFinished>| {
                if finished.result.is_err() {
                    *seen.lock().unwrap() += 1;
                }
            });
        let mut bad = FruitPreset::apple().fruit.clone();
        bad.maturity = 2.0;
        app.world_mut()
            .entity_mut(entity)
            .insert(FruitParams::new(bad));
        app.update();
        app.update();
        assert_eq!(*errors.lock().unwrap(), 1);
        assert_eq!(parts(&mut app), before);
    }
}
