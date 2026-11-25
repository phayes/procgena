//! Impostors for pooled rocks: each variant of a [`StonePool`] is baked into
//! an impostor from its game-level mesh and drawn as one quad when small on
//! screen, the last step of the drawing chain, as single rocks are.
//!
//! The pool's own meshes never reach the CPU, so the bake draws the
//! variant's game-level mesh built on the CPU, the reference path, which the
//! GPU meshes match.

use std::collections::{HashMap, HashSet};

use bevy::asset::UntypedAssetId;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use procgena_core::chain::Step;
use procgena_core::content_hash;
use procgena_core_bevy::instanced::{InstancedPart, PartMesh};
use procgena_core_bevy::{BakeAhead, ImpostorGate};
use procgena_impostor::{
    BakeOutput, BakePart, BakeSettings, ImpostorKey, ImpostorQuad, ImpostorRequest, Impostors,
};
use procgena_stone::Stone;

use super::{PoolDefinitions, PoolSizes, PooledStone, StonePool, renderer_lod};
use crate::material::{StoneMaterial, bake_copy};
use crate::mesh::stone_mesh;
use crate::spawn::SpawnSettings;

/// Where each variant of each defined pool stands on its way to an
/// impostor, by pool and the [`PoolDefinition`](super::PoolDefinition)
/// version it was started for.
#[derive(Resource, Default)]
pub(crate) struct PoolImpostors(HashMap<AssetId<StonePool>, (u64, Vec<VariantImpostor>)>);

/// One pool variant on its way to an impostor.
enum VariantImpostor {
    /// Its game-level mesh is being built on the CPU in the background.
    Building(Task<Option<Mesh>>),
    /// Its game-level mesh is built, and its impostor asked for.
    Baking(Handle<Mesh>),
    /// Its impostor is drawn as the slot's last level, or it has none.
    Done,
}

/// Starts, follows and finishes the impostor of every variant of every
/// defined pool, and gives each finished one's slot an impostor level.
/// A variant no camera can be far enough from to show its impostor (see
/// [`ImpostorGate`]) waits, unless a rock drawing it is marked
/// [`BakeAhead`], or its impostor is already baked. Does nothing without
/// impostor bake settings in the stone plugin's settings.
#[expect(clippy::too_many_arguments, reason = "one Bevy system")]
pub(crate) fn bake_pool_impostors(
    mut commands: Commands,
    settings: Res<SpawnSettings>,
    gate: ImpostorGate,
    sizes: Res<PoolSizes>,
    ahead: Query<&PooledStone, With<BakeAhead>>,
    mut definitions: ResMut<PoolDefinitions>,
    mut state: ResMut<PoolImpostors>,
    impostors: Option<ResMut<Impostors>>,
    quad: Option<Res<ImpostorQuad>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StoneMaterial>>,
) {
    let (Some(bake), Some(mut impostors), Some(quad)) = (settings.impostor, impostors, quad) else {
        return;
    };

    // Forget pools that are gone or were defined again.
    state.0.retain(|id, (version, _)| {
        definitions
            .pools
            .get(id)
            .is_some_and(|definition| definition.version == *version)
    });

    // Start building each new pool's variants.
    let ids: Vec<AssetId<StonePool>> = definitions.pools.keys().copied().collect();
    for id in &ids {
        let definition = &definitions.pools[id];
        if state.0.contains_key(id) {
            continue;
        }
        let variants = (0..definition.pool.variants)
            .map(|variant| {
                let pool = definition.pool.clone();
                let task =
                    AsyncComputeTaskPool::get().spawn(async move { game_mesh(&pool, variant) });
                VariantImpostor::Building(task)
            })
            .collect();
        state.0.insert(*id, (definition.version, variants));
    }

    // Move each variant along, and give finished ones their impostor level.
    let ahead: HashSet<(AssetId<StonePool>, u32)> = ahead
        .iter()
        .map(|stone| (stone.pool.id(), stone.variant))
        .collect();
    for (id, (_, variants)) in &mut state.0 {
        for (variant, step) in variants.iter_mut().enumerate() {
            if let VariantImpostor::Building(task) = step {
                let Some(built) = block_on(poll_once(task)) else {
                    continue;
                };
                *step = match built {
                    Some(mesh) => VariantImpostor::Baking(meshes.add(mesh)),
                    None => VariantImpostor::Done,
                };
            }
            let VariantImpostor::Baking(mesh) = step else {
                continue;
            };
            let Some(definition) = definitions.pools.get(id) else {
                continue;
            };
            let slot = &definition.slots[variant];
            let key = variant_key(&definition.pool, slot.specimen, bake, &settings);
            let source = slot.parts[0].material.clone().typed::<StoneMaterial>();
            let reachable = gate.can_show(sizes.drawn(*id, definition, variant as u32))
                || ahead.contains(&(*id, variant as u32));
            let ready = if reachable {
                impostors
                    .request(&mut commands, key, || {
                        bake_request(key, mesh.clone(), &source, &mut materials)
                    })
                    .is_some()
            } else {
                impostors.get(key).is_some()
            };
            let Some(material) = impostors.material(key).filter(|_| ready).cloned() else {
                continue;
            };

            // The baked impostor becomes the slot's last level, and the lower
            // mesh now ends where it takes over.
            let definition = definitions.pools.get_mut(id).expect("checked above");
            let mut slots = definition.slots.to_vec();
            slots[variant].parts.push(InstancedPart {
                level: Step::Impostor.index() as u32,
                mesh: PartMesh::Mesh(quad.0.clone()),
                material: material.untyped(),
            });
            slots[variant].lod = renderer_lod(true);
            definition.slots = slots.into();
            *step = VariantImpostor::Done;
        }
    }
}

/// The game-level mesh of `pool`'s variant `variant`, built on the CPU in
/// the pool's unit-cube units, as the pool draws it; `None` if its settings
/// are rejected.
fn game_mesh(pool: &StonePool, variant: u32) -> Option<Mesh> {
    let mut options = pool.variant_options(variant);
    options.scale = 1.0;
    let stone = Stone::new(options).ok()?;
    Some(stone_mesh(stone.mesh()))
}

/// The impostor key of a pool variant: its specimen, baked with `bake` from
/// `pool`'s game level, with the textures in use and this crate's version.
fn variant_key(
    pool: &StonePool,
    specimen: procgena_core::SpecimenId,
    bake: BakeSettings,
    settings: &SpawnSettings,
) -> ImpostorKey {
    let game_level = content_hash(&pool.options.mesh.game_level).to_le_bytes();
    let textures = format!(
        "{:?} {:?}",
        settings.texture_root, settings.texture_compression
    );
    ImpostorKey::specimen(
        specimen,
        bake,
        &[
            b"pool",
            &game_level,
            textures.as_bytes(),
            env!("CARGO_PKG_VERSION").as_bytes(),
        ],
    )
}

/// The bake request for a pool variant: `mesh` with bake copies of the
/// pool's material `source`, waiting for its textures.
fn bake_request(
    key: ImpostorKey,
    mesh: Handle<Mesh>,
    source: &Handle<StoneMaterial>,
    materials: &mut Assets<StoneMaterial>,
) -> ImpostorRequest {
    let mut parts = Vec::new();
    let mut wait_for: Vec<UntypedAssetId> = Vec::new();
    if let Some(source) = materials.get(source).cloned() {
        let extension = &source.extension;
        wait_for.extend(
            [&extension.color, &extension.normal, &extension.orh]
                .into_iter()
                .flatten()
                .map(|t| t.id().untyped()),
        );
        let albedo = materials.add(bake_copy(&source, BakeOutput::Albedo));
        let normal = materials.add(bake_copy(&source, BakeOutput::Normal));
        parts.push(BakePart::new(mesh, albedo, normal));
    }
    ImpostorRequest {
        key,
        parts,
        wait_for,
    }
}
