//! Preparing groves ahead of time: generating every tree a grove can use,
//! and baking their impostors, before the grove is placed.

use std::sync::Arc;

use bevy::prelude::*;
use procgena_tree::{GrovePreset, TreeGrove, TreeOptions};

use super::{GroveSource, TreeGroveAsset};
use crate::spawn::{CacheState, SpawnSettings, TreeCache, TreeKey};

/// Every tree `grove` can place: each shape variant of each species, in the
/// order of [`TreeGrove::trees`]. Pass them to your own loading code, or
/// use [`PrepareGrove`] to generate them.
pub fn grove_variants(grove: &TreeGrove) -> Vec<Arc<TreeOptions>> {
    grove
        .trees
        .iter()
        .flat_map(|species| (0..species.variants).map(|v| Arc::new(species.variant(v))))
        .collect()
}

/// Generates every tree a grove can use, and bakes their impostors, without
/// placing any, for example behind a loading screen.
///
/// Spawn it on an entity of its own. [`GrovePreparing`] on the entity
/// tracks progress, and [`GroveReady`] is triggered on it when everything
/// is ready. A [`TreeGroveParams`](super::TreeGroveParams) with the same
/// grove then finishes within a few frames, without generating or baking
/// anything. Groves sharing species share their trees, so preparing one
/// grove also speeds up others with the same species.
///
/// The trees stay cached while this entity exists. Despawn it, or remove
/// the component, once the groves using them have finished building
/// ([`TreeGroveFinished`](super::TreeGroveFinished)), so trees no grove
/// uses can be freed. Despawning it earlier, even in the frame a grove is
/// spawned, can free trees before the grove takes them, and they are then
/// generated again.
///
/// Changing the component, or the file it loads, prepares the new grove.
///
/// ```
/// use bevy::prelude::*;
/// use procgena_tree::GrovePreset;
/// use procgena_tree_bevy::{GroveReady, PrepareGrove, TreeGroveFinished, TreeGroveParams};
///
/// fn start_loading(mut commands: Commands) {
///     commands
///         .spawn(PrepareGrove::preset(GrovePreset::PineStand))
///         .observe(|ready: On<GroveReady>, mut commands: Commands| {
///             let preparer = ready.entity;
///             commands
///                 .spawn(TreeGroveParams::preset(GrovePreset::PineStand, Vec2::splat(200.0)))
///                 // The grove holds its own trees once it is built.
///                 .observe(move |_: On<TreeGroveFinished>, mut commands: Commands| {
///                     commands.entity(preparer).despawn();
///                 });
///         });
/// }
/// ```
#[derive(Component, Debug, Clone, PartialEq)]
pub struct PrepareGrove {
    /// The grove to prepare.
    pub grove: GroveSource,
}

impl PrepareGrove {
    /// Prepares `grove`.
    pub fn new(grove: TreeGrove) -> Self {
        Self {
            grove: GroveSource::Options(Arc::new(grove)),
        }
    }

    /// Prepares a preset's grove.
    pub fn preset(preset: GrovePreset) -> Self {
        Self::new(preset.grove())
    }
}

/// How far a [`PrepareGrove`] has got, as steps done out of the total: one
/// step per tree for generating it, and one more for baking its impostor
/// when impostors are on. The plugin adds it once the grove is known (a
/// grove file must load first).
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GrovePreparing {
    /// Steps finished, including trees whose settings were rejected.
    pub done: usize,
    /// Steps in all.
    pub total: usize,
}

impl GrovePreparing {
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

/// Triggered on a [`PrepareGrove`] entity when every tree of its grove is
/// generated and every impostor baked. Triggered again after the component
/// changes, once the new grove is ready.
#[derive(EntityEvent, Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroveReady {
    /// The [`PrepareGrove`] entity.
    pub entity: Entity,
}

/// The trees a [`PrepareGrove`] entity keeps cached, and the grove they are
/// for.
#[derive(Component)]
pub(crate) struct PreparedTrees {
    pub(crate) keys: Vec<TreeKey>,
    grove: TreeGrove,
    /// Whether [`GroveReady`] was triggered for these trees.
    reported: bool,
}

/// Starts generating the trees of every [`PrepareGrove`] whose grove is
/// known and isn't the one it last prepared.
pub(crate) fn start_preparing(
    mut commands: Commands,
    groves: Query<(Entity, &PrepareGrove, Option<&PreparedTrees>)>,
    files: Res<Assets<TreeGroveAsset>>,
    settings: Res<SpawnSettings>,
    mut cache: ResMut<TreeCache>,
) {
    for (entity, prepare, prepared) in &groves {
        let grove = match &prepare.grove {
            GroveSource::Options(grove) => &**grove,
            GroveSource::Asset(handle) => match files.get(handle) {
                Some(file) => &file.0,
                None => continue,
            },
        };
        if prepared.is_some_and(|p| p.grove == *grove) {
            continue;
        }
        let keys = grove_variants(grove)
            .iter()
            .map(|options| cache.request(options, &settings))
            .collect();
        commands.entity(entity).insert(PreparedTrees {
            keys,
            grove: grove.clone(),
            reported: false,
        });
    }
}

/// Updates every [`PrepareGrove`]'s progress, and reports those that are
/// ready.
pub(crate) fn track_preparing(
    mut commands: Commands,
    mut groves: Query<(Entity, &mut PreparedTrees, Option<&mut GrovePreparing>)>,
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
        let now = GrovePreparing {
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
            commands.trigger(GroveReady { entity });
        }
    }
}

/// How many of a tree's preparation steps are done: generating it, and
/// baking its impostor when impostors are on. A rejected tree, or one
/// without the level impostors are baked from, has nothing left to do.
fn steps_done(key: TreeKey, settings: &SpawnSettings, cache: &TreeCache) -> usize {
    let generated = match cache.state(key) {
        CacheState::Pending => return 0,
        CacheState::Failed => None,
        CacheState::Ready(generated) => Some(generated),
    };
    let Some(impostor) = settings.impostor else {
        return 1;
    };
    let baked = generated.is_none_or(|generated| {
        impostor.baked_level >= generated.level_count() || cache.impostor(key).is_some()
    });
    1 + usize::from(baked)
}

/// Lets go of the trees of entities that lost their [`PrepareGrove`].
pub(crate) fn stop_preparing(mut commands: Commands, mut removed: RemovedComponents<PrepareGrove>) {
    for entity in removed.read() {
        if let Ok(mut entity) = commands.get_entity(entity) {
            entity.remove::<(PreparedTrees, GrovePreparing)>();
        }
    }
}
