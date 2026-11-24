//! Draws groves through the shared instanced renderer of
//! `procgena_core_bevy`: each grove entity gets an [`InstancedScatter`] with
//! one slot per species variant (its detail levels, then its impostor once
//! baked) and one copy per placed tree that isn't promoted.

use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;

use bevy::camera::primitives::Aabb;
use bevy::platform::hash::FixedHasher;
use bevy::prelude::*;
use procgena_core::{CopyAddress, Lod};
use procgena_core_bevy::instanced::{
    InstancedCopy, InstancedPart, InstancedScatter, InstancedSlot, PartMesh, SpecimenCache,
};

use super::promote::{PromotedAs, PromotedTree};
use super::{TreeScatterAssets, TreeScatterInstances, TreeScatterVariant};
use crate::spawn::{SpawnSettings, TreeCache, TreeGenerating};

/// How much a variant's bounding sphere is enlarged to cover wind sway.
const SWAY_ALLOWANCE: f32 = 1.15;

/// Marks a grove entity whose [`InstancedScatter`] this module keeps.
#[derive(Component, Debug, Default)]
pub(crate) struct GroveDraw {
    /// The trees left out because they are promoted, as positions in the
    /// grove's trees, when the copies were last built.
    hidden: HashSet<u32>,
}

/// Keeps every grove's [`InstancedScatter`] in step with its trees, its
/// variants and its promoted trees, and removes it from entities that are
/// no longer groves.
#[expect(clippy::type_complexity, reason = "Bevy system parameters")]
pub(crate) fn update_grove_draws(
    mut commands: Commands,
    groves: Query<(
        Entity,
        Ref<TreeScatterInstances>,
        Ref<TreeScatterAssets>,
        Option<&InstancedScatter>,
        Option<&GroveDraw>,
    )>,
    gone: Query<Entity, (With<GroveDraw>, Without<TreeScatterInstances>)>,
    promoted: Query<&PromotedTree, (With<PromotedAs>, Without<TreeGenerating>)>,
    settings: Res<SpawnSettings>,
    cache: Res<TreeCache>,
    mut specimens: ResMut<SpecimenCache>,
) {
    for entity in &gone {
        commands
            .entity(entity)
            .remove::<(GroveDraw, InstancedScatter)>();
    }

    // Promoted trees that draw themselves, which their groves leave out.
    let mut hidden: HashMap<Entity, HashSet<u32>> = HashMap::new();
    for promotion in &promoted {
        if let Ok((_, instances, ..)) = groves.get(promotion.grove)
            && let Some(index) = instances.index_of(&promotion.address)
        {
            hidden.entry(promotion.grove).or_default().insert(index);
        }
    }

    // Rebuild what changed: the slots when the variants did, the copies when
    // the trees or the promoted trees did.
    for (entity, instances, assets, current, drawn) in &groves {
        let hide = hidden.remove(&entity).unwrap_or_default();
        let current = current.filter(|_| drawn.is_some());
        let copies = match current {
            Some(scatter) if !instances.is_changed() && drawn.is_some_and(|d| d.hidden == hide) => {
                scatter.copies.clone()
            }
            _ => grove_copies(&instances, &hide, settings.lod.reference_size),
        };
        let slots = match current {
            Some(scatter) if !assets.is_changed() => scatter.slots.clone(),
            _ => grove_slots(&assets, &settings, &cache, &mut specimens),
        };
        let unchanged = current.is_some_and(|scatter| {
            std::sync::Arc::ptr_eq(&scatter.copies, &copies)
                && std::sync::Arc::ptr_eq(&scatter.slots, &slots)
        });
        if !unchanged {
            commands.entity(entity).insert((
                InstancedScatter { slots, copies },
                GroveDraw { hidden: hide },
            ));
        }
    }
}

/// One slot per variant of a grove: its parts at every level, and its
/// impostor as one more level once baked. A variant that isn't ready has no
/// parts and draws nothing. Each ready variant's parts are cached by its
/// specimen and level.
fn grove_slots(
    assets: &TreeScatterAssets,
    settings: &SpawnSettings,
    cache: &TreeCache,
    specimens: &mut SpecimenCache,
) -> std::sync::Arc<[InstancedSlot]> {
    let mesh_levels = settings.lod.levels.len() as u32;
    assets
        .variants()
        .iter()
        .map(|variant| {
            let specimen = cache.specimen_id(variant.key);
            let (lod, parts) = variant_parts(variant, settings, mesh_levels);
            if variant.is_ready() {
                for level in 0..lod.levels.len() as u32 {
                    let at_level = parts.iter().filter(|p| p.level == level).cloned();
                    specimens.insert(specimen, 0, level, at_level.collect());
                }
            }
            InstancedSlot {
                specimen,
                piece: 0,
                lod,
                bounds: variant_bounds(variant),
                parts,
            }
        })
        .collect()
}

/// A variant's detail levels and parts: its mesh levels, then its impostor
/// once baked (until then, its farthest level is drawn at every distance
/// beyond).
fn variant_parts(
    variant: &TreeScatterVariant,
    settings: &SpawnSettings,
    mesh_levels: u32,
) -> (Lod, Vec<InstancedPart>) {
    let mut parts: Vec<InstancedPart> = variant
        .parts()
        .map(|part| InstancedPart {
            level: part.lod as u32,
            mesh: PartMesh::Mesh(part.mesh.clone()),
            material: part.material.clone().untyped(),
        })
        .collect();
    let lod = match variant.impostor() {
        Some(impostor) => {
            parts.push(InstancedPart {
                level: mesh_levels,
                mesh: PartMesh::Mesh(impostor.quad.clone()),
                material: impostor.material.clone().untyped(),
            });
            settings.draw_lod().map_detail(|_| ())
        }
        None => settings.lod.clone().map_detail(|_| ()),
    };
    (lod, parts)
}

/// A sphere around every part of `variant` at every level, enlarged for
/// wind sway, in the tree's own coordinates; a point at the origin before
/// the variant is ready.
fn variant_bounds(variant: &TreeScatterVariant) -> (Vec3, f32) {
    let (min, max) = variant.parts().map(|part| part.bounds).fold(
        (Vec3::INFINITY, Vec3::NEG_INFINITY),
        |(min, max), aabb: Aabb| (min.min(aabb.min().into()), max.max(aabb.max().into())),
    );
    if min.x > max.x {
        return (Vec3::ZERO, 0.0);
    }
    let center = (min + max) / 2.0;
    (center, (max - center).length() * SWAY_ALLOWANCE)
}

/// One copy per tree of a grove, leaving out the trees at positions in
/// `hidden`. Trees aren't sized yet, so each counts as the levels'
/// reference size, `size`.
fn grove_copies(
    instances: &TreeScatterInstances,
    hidden: &HashSet<u32>,
    size: f32,
) -> std::sync::Arc<[InstancedCopy]> {
    instances
        .trees()
        .iter()
        .enumerate()
        .filter(|(index, _)| !hidden.contains(&(*index as u32)))
        .map(|(_, tree)| InstancedCopy {
            slot: tree.slot,
            transform: tree.transform.compute_affine(),
            size,
            wind_phase: wind_phase(&tree.address),
        })
        .collect()
}

/// A repeatable number from 0 to 1 for the tree on the spot at `address`.
fn wind_phase(address: &CopyAddress) -> f32 {
    (FixedHasher.hash_one(address) >> 40) as f32 / (1u64 << 24) as f32
}
