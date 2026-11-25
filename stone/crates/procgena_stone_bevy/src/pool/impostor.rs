//! Impostors for pooled rocks: each variant of a [`StonePool`] is baked into
//! an impostor and drawn far away as one quad, the last level of the
//! variant's levels of detail, as single rocks are.
//!
//! The pool's own meshes never reach the CPU, so the bake draws the
//! variant's baked level built on the CPU, the reference path, which the
//! GPU meshes match.

use std::collections::HashMap;

use bevy::asset::UntypedAssetId;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use procgena_core_bevy::instanced::{InstancedPart, PartMesh};
use procgena_impostor::{
    BakeOutput, BakePart, ImpostorKey, ImpostorLevel, ImpostorQuad, ImpostorRequest, Impostors,
};
use procgena_stone::{Stone, StoneDetail};

use super::{PoolDefinitions, StonePool};
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
    /// Its baked level is being built on the CPU in the background.
    Building(Task<Option<Mesh>>),
    /// Its baked level is built, and its impostor asked for.
    Baking(Handle<Mesh>),
    /// Its impostor is drawn as the slot's last level, or it has none.
    Done,
}

/// Starts, follows and finishes the impostor of every variant of every
/// defined pool, and gives each finished one's slot an impostor level.
/// Does nothing without an impostor level in the stone plugin's settings,
/// or for pools whose levels leave no room for one.
#[expect(clippy::too_many_arguments, reason = "one Bevy system")]
pub(crate) fn bake_pool_impostors(
    mut commands: Commands,
    settings: Res<SpawnSettings>,
    mut definitions: ResMut<PoolDefinitions>,
    mut state: ResMut<PoolImpostors>,
    impostors: Option<ResMut<Impostors>>,
    quad: Option<Res<ImpostorQuad>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StoneMaterial>>,
) {
    let (Some(impostor), Some(mut impostors), Some(quad)) = (settings.impostor, impostors, quad)
    else {
        return;
    };

    // Forget pools that are gone or were defined again.
    state.0.retain(|id, (version, _)| {
        definitions
            .pools
            .get(id)
            .is_some_and(|definition| definition.version == *version)
    });

    // Start building each new pool's variants, if they draw an impostor.
    let ids: Vec<AssetId<StonePool>> = definitions.pools.keys().copied().collect();
    for id in &ids {
        let definition = &definitions.pools[id];
        if state.0.contains_key(id) {
            continue;
        }
        let variants = if impostor_lod(&definition.pool, impostor).is_some() {
            (0..definition.pool.variants)
                .map(|variant| {
                    let pool = definition.pool.clone();
                    let task = AsyncComputeTaskPool::get()
                        .spawn(async move { baked_level_mesh(&pool, variant, impostor) });
                    VariantImpostor::Building(task)
                })
                .collect()
        } else {
            Vec::new()
        };
        state.0.insert(*id, (definition.version, variants));
    }

    // Move each variant along, and give finished ones their impostor level.
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
            let key = variant_key(slot.specimen, impostor, &settings);
            let source = slot.parts[0].material.clone().typed::<StoneMaterial>();
            let ready = impostors
                .request(&mut commands, key, || {
                    bake_request(key, mesh.clone(), &source, &mut materials)
                })
                .is_some();
            let Some(material) = impostors.material(key).filter(|_| ready).cloned() else {
                continue;
            };

            // The baked impostor becomes the slot's last level.
            let lod = impostor_lod(&definition.pool, impostor)
                .expect("only pools with room for an impostor bake one");
            let definition = definitions.pools.get_mut(id).expect("checked above");
            let mut slots = definition.slots.to_vec();
            let levels = slots[variant].lod.levels.len() as u32;
            slots[variant].parts.push(InstancedPart {
                level: levels,
                mesh: PartMesh::Mesh(quad.0.clone()),
                material: material.untyped(),
            });
            slots[variant].lod = lod;
            definition.slots = slots.into();
            *step = VariantImpostor::Done;
        }
    }
}

/// The levels of `pool`'s variants with `impostor` as the last level, or
/// `None` when they leave no room for it: there is no level to bake, or
/// the farthest level reaches past the impostor's distance.
fn impostor_lod(pool: &StonePool, impostor: ImpostorLevel) -> Option<procgena_core::Lod> {
    let mut lod = pool.options.mesh.detail.lod().map_detail(Some);
    if impostor.baked_level >= lod.levels.len() {
        return None;
    }
    lod.levels.push(impostor.level(None));
    lod.validate().ok()?;
    Some(lod.map_detail(|_| ()))
}

/// The mesh of `pool`'s variant `variant` at `impostor`'s baked level,
/// built on the CPU in the pool's unit-cube units, as the pool draws it;
/// `None` if its settings are rejected.
fn baked_level_mesh(pool: &StonePool, variant: u32, impostor: ImpostorLevel) -> Option<Mesh> {
    let mut options = pool.variant_options(variant);
    let level = options.mesh.detail.levels()[impostor.baked_level];
    options.mesh.detail = StoneDetail::single(level.resolution, level.texture_size);
    options.scale = 1.0;
    let stone = Stone::new(options).ok()?;
    let (_, mesh) = stone.levels().next()?;
    Some(stone_mesh(mesh))
}

/// The impostor key of a pool variant: its specimen, baked with
/// `impostor`'s settings from its baked level, with the textures in use and
/// this crate's version.
fn variant_key(
    specimen: procgena_core::SpecimenId,
    impostor: ImpostorLevel,
    settings: &SpawnSettings,
) -> ImpostorKey {
    let level = (impostor.baked_level as u64).to_le_bytes();
    let textures = format!(
        "{:?} {:?}",
        settings.texture_root, settings.texture_compression
    );
    ImpostorKey::specimen(
        specimen,
        impostor.bake,
        &[
            b"pool",
            &level,
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
