//! Preparing groves ahead of time: generating every tree a grove can use,
//! and baking their impostors, before the grove is placed.

use std::sync::Arc;

use bevy::prelude::*;
use procgena_tree::{TreeOptions, TreeScatter, TreeScatterPreset};

use super::{TreeScatterAsset, TreeScatterSource};
use crate::spawn::{CacheState, SpawnSettings, TreeCache, TreeKey};

/// Every tree `tree_scatter` can place: each shape variant of each species,
/// in the order of [`TreeScatter::trees`]. Pass them to your own loading
/// code, or use [`PrepareTreeScatter`] to generate them.
pub fn tree_scatter_variants(tree_scatter: &TreeScatter) -> Vec<Arc<TreeOptions>> {
    tree_scatter
        .trees
        .iter()
        .flat_map(|species| (0..species.variants).map(|v| Arc::new(species.variant(v))))
        .collect()
}

/// Generates every tree a grove can use, and bakes their impostors, without
/// placing any, for example behind a loading screen.
///
/// Spawn it on an entity of its own. [`TreeScatterPreparing`] on the entity
/// tracks progress, and [`TreeScatterReady`] is triggered on it when
/// everything is ready. A [`TreeScatterParams`](super::TreeScatterParams)
/// with the same Scatter then finishes within a few frames, without
/// generating or baking anything. Groves sharing species share their trees,
/// so preparing one grove also speeds up others with the same species.
///
/// The trees stay cached while this entity exists. Despawn it, or remove
/// the component, once the groves using them have finished building
/// ([`TreeScatterFinished`](super::TreeScatterFinished)), so trees no grove
/// uses can be freed. Despawning it earlier, even in the frame a grove is
/// spawned, can free trees before the grove takes them, and they are then
/// generated again.
///
/// Changing the component, or the file it loads, prepares the new grove.
///
/// ```
/// use bevy::prelude::*;
/// use procgena_tree::TreeScatterPreset;
/// use procgena_tree_bevy::{
///     PrepareTreeScatter, TreeScatterFinished, TreeScatterParams, TreeScatterReady,
/// };
///
/// fn start_loading(mut commands: Commands) {
///     commands
///         .spawn(PrepareTreeScatter::preset(TreeScatterPreset::pine_stand()))
///         .observe(|ready: On<TreeScatterReady>, mut commands: Commands| {
///             let preparer = ready.entity;
///             let stand = TreeScatterPreset::pine_stand();
///             commands
///                 .spawn(TreeScatterParams::preset(stand, Vec2::splat(200.0)))
///                 // The grove holds its own trees once it is built.
///                 .observe(move |_: On<TreeScatterFinished>, mut commands: Commands| {
///                     commands.entity(preparer).despawn();
///                 });
///         });
/// }
/// ```
#[derive(Component, Debug, Clone, PartialEq)]
pub struct PrepareTreeScatter {
    /// The Scatter to prepare.
    pub tree_scatter: TreeScatterSource,
}

impl PrepareTreeScatter {
    /// Prepares `tree_scatter`.
    pub fn new(tree_scatter: TreeScatter) -> Self {
        Self {
            tree_scatter: TreeScatterSource::Options(Arc::new(tree_scatter)),
        }
    }

    /// Prepares a preset's Scatter.
    pub fn preset(preset: &TreeScatterPreset) -> Self {
        Self::new(preset.tree_scatter.clone())
    }
}

/// How far a [`PrepareTreeScatter`] has got, as steps done out of the
/// total: one step per tree for generating it, and one more for baking its
/// impostor when impostors are on. The plugin adds it once the Scatter is
/// known (a `.tree_scatter.yml` file must load first).
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TreeScatterPreparing {
    /// Steps finished, including trees whose settings were rejected.
    pub done: usize,
    /// Steps in all.
    pub total: usize,
}

impl TreeScatterPreparing {
    /// The share of steps done, from 0 to 1. A grove without trees is 1.
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            1.0
        } else {
            self.done as f32 / self.total as f32
        }
    }

    /// Whether every step is done.
    pub fn is_ready(&self) -> bool {
        self.done >= self.total
    }
}

/// Triggered on a [`PrepareTreeScatter`] entity when every tree of its
/// Scatter is generated and every impostor baked. Triggered again after the
/// component changes, once the new Scatter is ready.
#[derive(EntityEvent, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeScatterReady {
    /// The [`PrepareTreeScatter`] entity.
    pub entity: Entity,
}

/// The trees a [`PrepareTreeScatter`] entity keeps cached, and the grove
/// they are for.
#[derive(Component)]
pub(crate) struct PreparedTrees {
    pub(crate) keys: Vec<TreeKey>,
    grove: TreeScatter,
    /// Whether [`TreeScatterReady`] was triggered for these trees.
    reported: bool,
}

/// Starts generating the trees of every [`PrepareTreeScatter`] whose grove
/// is known and isn't the one it last prepared.
pub(crate) fn start_preparing(
    mut commands: Commands,
    groves: Query<(Entity, &PrepareTreeScatter, Option<&PreparedTrees>)>,
    files: Res<Assets<TreeScatterAsset>>,
    settings: Res<SpawnSettings>,
    mut cache: ResMut<TreeCache>,
) {
    for (entity, prepare, prepared) in &groves {
        let grove = match &prepare.tree_scatter {
            TreeScatterSource::Options(grove) => &**grove,
            TreeScatterSource::Asset(handle) => match files.get(handle) {
                Some(file) => &file.0.tree_scatter,
                None => continue,
            },
        };
        if prepared.is_some_and(|p| p.grove == *grove) {
            continue;
        }
        let keys = grove
            .trees
            .iter()
            .flat_map(|species| (0..species.variants).map(move |v| (species, v)))
            .map(|(species, v)| {
                let options = Arc::new(species.variant(v));
                cache.request_specimen(&options, species.specimen_id(v), &settings)
            })
            .collect();
        commands.entity(entity).insert(PreparedTrees {
            keys,
            grove: grove.clone(),
            reported: false,
        });
    }
}

/// Updates every [`PrepareTreeScatter`]'s progress, and reports those that
/// are ready.
pub(crate) fn track_preparing(
    mut commands: Commands,
    mut groves: Query<(
        Entity,
        &mut PreparedTrees,
        Option<&mut TreeScatterPreparing>,
    )>,
    settings: Res<SpawnSettings>,
    cache: Res<TreeCache>,
) {
    for (entity, mut prepared, progress) in &mut groves {
        let steps = if settings.impostor.is_some() { 2 } else { 1 };
        let done = prepared
            .keys
            .iter()
            .map(|key| steps_done(*key, &settings, &cache))
            .sum();
        let now = TreeScatterPreparing {
            done,
            total: prepared.keys.len() * steps,
        };
        match progress {
            Some(mut progress) => {
                progress.set_if_neq(now);
            }
            None => {
                commands.entity(entity).insert(now);
            }
        }
        if now.is_ready() && !prepared.reported {
            prepared.reported = true;
            commands.trigger(TreeScatterReady { entity });
        }
    }
}

/// How many of a tree's preparation steps are done: generating it, and
/// baking its impostor when impostors are on. A rejected tree has nothing
/// left to do.
fn steps_done(key: TreeKey, settings: &SpawnSettings, cache: &TreeCache) -> usize {
    let ready = match cache.state(key) {
        CacheState::Pending => return 0,
        CacheState::Failed => false,
        CacheState::Ready(_) => true,
    };
    if settings.impostor.is_none() {
        return 1;
    }
    let baked = !ready || cache.impostor(key).is_some();
    1 + usize::from(baked)
}

/// Lets go of the trees of entities that lost their [`PrepareTreeScatter`].
pub(crate) fn stop_preparing(
    mut commands: Commands,
    mut removed: RemovedComponents<PrepareTreeScatter>,
) {
    for entity in removed.read() {
        if let Ok(mut entity) = commands.get_entity(entity) {
            entity.remove::<(PreparedTrees, TreeScatterPreparing)>();
        }
    }
}
