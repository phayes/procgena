//! Growing a cluster from an entity's settings and spawning its levels.

use std::sync::Arc;

use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use procgena_core::Quality;
use procgena_core_bevy::{LodQuality, visibility_range};
use procgena_fruit::{Fruit, FruitOptions, FruitPreset, OptionsError, default_lod};

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

/// One detail level's mesh of a drawn cluster, a child of the entity with
/// [`FruitParams`].
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct FruitPart {
    /// The detail level, from 0 for the nearest.
    pub level: usize,
}

/// Triggered on an entity with [`FruitParams`] after its cluster was grown
/// and its levels spawned, or with the settings' error.
#[derive(EntityEvent, Debug, Clone)]
pub struct FruitFinished {
    /// The entity whose cluster was grown.
    pub entity: Entity,
    /// The number of vertices and triangles at each level, nearest first,
    /// or why the cluster couldn't be grown. On an error the old levels stay.
    pub result: Result<Vec<(usize, usize)>, OptionsError>,
}

/// Grows the clusters whose settings changed, replacing their levels.
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

        // Replace the old levels.
        for child in children.into_iter().flatten() {
            if parts.contains(*child) {
                commands.entity(*child).despawn();
            }
        }

        // One child per level, fading by distance for this cluster's size.
        let lod = default_lod();
        let ranges = lod.ranges(fruit.lod_factor(&lod, 1.0, quality));
        let material = materials.add(fruit_material(fruit.options()));
        let mut sizes = Vec::new();
        for (level, (_, mesh)) in fruit.lod_meshes(&lod).into_iter().enumerate() {
            sizes.push((mesh.vertex_count(), mesh.triangle_count()));
            if mesh.indices.is_empty() {
                continue;
            }
            let mut part = commands.spawn((
                FruitPart { level },
                Mesh3d(meshes.add(fruit_mesh(&mesh))),
                MeshMaterial3d(material.clone()),
                ChildOf(entity),
            ));
            if !ranges[level].is_always() {
                let range: VisibilityRange = visibility_range(&ranges[level]);
                part.insert(range);
            }
        }
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

    fn parts(app: &mut App) -> Vec<usize> {
        let mut levels: Vec<usize> = app
            .world_mut()
            .query::<&FruitPart>()
            .iter(app.world())
            .map(|p| p.level)
            .collect();
        levels.sort_unstable();
        levels
    }

    #[test]
    fn every_level_spawns_once_and_is_replaced_on_change() {
        let mut app = app();
        let entity = app
            .world_mut()
            .spawn(FruitParams::preset(FruitPreset::apple()))
            .id();
        app.update();
        let levels = default_lod().levels.len();
        assert_eq!(parts(&mut app), (0..levels).collect::<Vec<_>>());

        app.world_mut().entity_mut(entity).insert(FruitParams::new(
            procgena_fruit::FlowerPreset::dogwood_bloom().flower.clone(),
        ));
        app.update();
        app.update();
        assert_eq!(parts(&mut app), (0..levels).collect::<Vec<_>>());
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
