//! Baking many impostors ahead of need, for example behind a loading
//! screen, with progress to show.

use bevy::prelude::*;

use crate::bake::{BakedImpostor, ImpostorRequest};

/// Bakes a set of impostors ahead of need. Spawn it on an entity of your
/// own; each request becomes a child [`ImpostorRequest`] entity.
///
/// The entity gets an [`ImpostorBatchProgress`] at once, updated as
/// impostors finish, and [`ImpostorBatchFinished`] is triggered on it when
/// all are ready. Requests already baked count as done at once. Despawn the
/// entity when you no longer need it; the impostors stay in
/// [`Impostors`](crate::Impostors), so anything that asks for them later
/// gets them at once.
#[derive(Component, Clone, Debug)]
pub struct ImpostorBatch {
    requests: Vec<ImpostorRequest>,
}

impl ImpostorBatch {
    /// A batch baking `requests`.
    pub fn new(requests: Vec<ImpostorRequest>) -> Self {
        Self { requests }
    }
}

/// How far an [`ImpostorBatch`] has got.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImpostorBatchProgress {
    /// Impostors ready.
    pub done: usize,
    /// Impostors in the batch.
    pub total: usize,
}

impl ImpostorBatchProgress {
    /// The share done, from 0 to 1; 1 for an empty batch.
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            1.0
        } else {
            self.done as f32 / self.total as f32
        }
    }
}

/// Triggered on an [`ImpostorBatch`]'s entity when every impostor in it is
/// ready.
#[derive(EntityEvent, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImpostorBatchFinished {
    /// The batch's entity.
    pub entity: Entity,
}

/// Spawns each new batch's requests as its children.
pub(crate) fn start_batches(
    mut commands: Commands,
    batches: Query<(Entity, &ImpostorBatch), Added<ImpostorBatch>>,
) {
    for (entity, batch) in &batches {
        for request in &batch.requests {
            commands.spawn((request.clone(), ChildOf(entity)));
        }
        commands.entity(entity).insert(ImpostorBatchProgress {
            done: 0,
            total: batch.requests.len(),
        });
    }
}

/// Counts each unfinished batch's ready impostors, and reports batches that
/// have just finished.
pub(crate) fn track_batches(
    mut commands: Commands,
    mut batches: Query<(Entity, &mut ImpostorBatchProgress, Option<&Children>)>,
    baked: Query<(), (With<ImpostorRequest>, With<BakedImpostor>)>,
) {
    for (entity, mut progress, children) in &mut batches {
        if progress.done == progress.total && progress.total > 0 {
            continue;
        }
        let done = children
            .into_iter()
            .flatten()
            .filter(|child| baked.contains(**child))
            .count();
        if done != progress.done {
            progress.done = done;
        }
        // An empty batch finishes in its first frame; others when the last
        // impostor arrives (after which they are skipped above).
        let finished = done == progress.total;
        if finished && (progress.total > 0 || progress.is_added()) {
            commands.trigger(ImpostorBatchFinished { entity });
        }
    }
}
