//! Draws groves through the shared instanced renderer of
//! `procgena_core_bevy`: each grove entity gets an [`InstancedScatter`] with
//! one slot per species variant (its game-level and lower meshes, then its
//! impostor once baked) and one copy per placed tree that isn't promoted,
//! each sized by its bounding-sphere diameter, then one slot
//! per fruit or flower cluster specimen the variants show, drawn by the
//! [`InstancedAttached`] rows of the variants' slots.

use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;
use std::sync::Arc;

use bevy::camera::primitives::Aabb;
use bevy::platform::hash::FixedHasher;
use bevy::prelude::*;
use procgena_core::CopyAddress;
use procgena_core::chain::{DistanceChain, Lod, Step};
use procgena_core_bevy::instanced::{
    InstancedAttached, InstancedCopy, InstancedPart, InstancedScatter, InstancedSlot, PartMesh,
    SpecimenCache, chain_lod,
};
use procgena_tree::LodDetail;

use super::promote::{PromotedAs, PromotedTree};
use super::{TreeScatterAssets, TreeScatterInstances, TreeScatterVariant};
use crate::clusters::{ClusterImpostors, grove_clusters};
use crate::spawn::{SpawnSettings, TreeCache, TreeGenerating, copy_size};

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
        Option<&InstancedAttached>,
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
            .remove::<(GroveDraw, InstancedScatter, InstancedAttached)>();
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

    // Rebuild what changed: the slots and rows when the variants did, the
    // copies when the trees, their variants' sizes or the promoted trees
    // did.
    for (entity, instances, assets, current, current_rows, drawn) in &groves {
        let hide = hidden.remove(&entity).unwrap_or_default();
        let current = current.zip(current_rows).filter(|_| drawn.is_some());
        let copies = match current {
            Some((scatter, _))
                if !instances.is_changed()
                    && !assets.is_changed()
                    && drawn.is_some_and(|d| d.hidden == hide) =>
            {
                scatter.copies.clone()
            }
            _ => grove_copies(&instances, &assets, &hide),
        };
        let (slots, rows) = match current {
            Some((scatter, rows)) if !assets.is_changed() => (scatter.slots.clone(), rows.clone()),
            _ => grove_slots(&assets, &settings, &cache, &mut specimens),
        };
        let unchanged = current.is_some_and(|(scatter, current_rows)| {
            Arc::ptr_eq(&scatter.copies, &copies)
                && Arc::ptr_eq(&scatter.slots, &slots)
                && Arc::ptr_eq(&current_rows.tables, &rows.tables)
        });
        if !unchanged {
            commands.entity(entity).insert((
                InstancedScatter { slots, copies },
                rows,
                GroveDraw { hidden: hide },
            ));
        }
    }
}

/// One slot per variant of a grove: its game-level and lower meshes, and
/// its impostor as the last step once baked. A variant that isn't ready has
/// no parts and draws nothing. Each ready variant's parts are cached by its
/// specimen and step. Then one slot per cluster specimen the variants show,
/// and the variants' cluster rows.
fn grove_slots(
    assets: &TreeScatterAssets,
    settings: &SpawnSettings,
    cache: &TreeCache,
    specimens: &mut SpecimenCache,
) -> (Arc<[InstancedSlot]>, InstancedAttached) {
    let mut slots: Vec<InstancedSlot> = assets
        .variants()
        .iter()
        .map(|variant| {
            let specimen = cache.specimen_id(variant.key);
            let (lod, parts) = variant_parts(variant);
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
        .collect();

    // The clusters, after the variants, drawn as their impostors when small
    // on screen once their variant's impostors are baked.
    let baked_variants: Vec<_> = assets
        .variants()
        .iter()
        .filter_map(|variant| variant.impostor())
        .collect();
    let baked: HashMap<_, _> = baked_variants
        .iter()
        .flat_map(|assets| assets.clusters.iter())
        .map(|(id, impostor)| (*id, impostor.clone()))
        .collect();
    let impostors = baked_variants.first().and_then(|first| {
        settings.fruit_impostor?;
        Some(ClusterImpostors {
            quad: &first.quad,
            baked: &baked,
        })
    });
    // A variant that isn't ready has no clusters, so the details it is
    // given don't matter; it keeps its place among the tables.
    let unready = Lod::from_game(LodDetail::default());
    let trees = assets
        .variants()
        .iter()
        .map(|variant| variant.clusters().unwrap_or((&[], &unready)));
    let (cluster_slots, tables) = grove_clusters(trees, slots.len() as u32, impostors);
    slots.extend(cluster_slots);
    (
        slots.into(),
        InstancedAttached {
            tables: tables.into(),
        },
    )
}

/// A variant's level table and parts: the shared chain's game-level and
/// lower meshes, then its impostor once baked (until then, its lower mesh
/// is drawn until the cull).
fn variant_parts(variant: &TreeScatterVariant) -> (procgena_core::Lod, Vec<InstancedPart>) {
    let mut parts: Vec<InstancedPart> = variant
        .parts()
        .map(|part| InstancedPart {
            level: part.step.index() as u32,
            mesh: PartMesh::Mesh(part.mesh.clone()),
            material: part.material.clone().untyped(),
        })
        .collect();
    if let Some(impostor) = variant.impostor() {
        parts.push(InstancedPart {
            level: Step::Impostor.index() as u32,
            mesh: PartMesh::Mesh(impostor.quad.clone()),
            material: impostor.material.clone().untyped(),
        });
    }
    let lod = chain_lod(&DistanceChain::DEFAULT, variant.impostor().is_some());
    (lod, parts)
}

/// A sphere around every part of `variant` at both mesh steps, enlarged for
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
/// `hidden`. Each copy's size is its variant's diameter times its largest
/// scale; a variant not generated yet, which draws nothing, counts as 1 m
/// across.
fn grove_copies(
    instances: &TreeScatterInstances,
    assets: &TreeScatterAssets,
    hidden: &HashSet<u32>,
) -> std::sync::Arc<[InstancedCopy]> {
    instances
        .trees()
        .iter()
        .enumerate()
        .filter(|(index, _)| !hidden.contains(&(*index as u32)))
        .map(|(_, tree)| {
            let diameter = assets.of(tree).diameter().unwrap_or(1.0);
            InstancedCopy {
                slot: tree.slot,
                transform: tree.transform.compute_affine(),
                size: copy_size(diameter, &tree.transform),
                wind_phase: wind_phase(&tree.address),
            }
        })
        .collect()
}

/// A repeatable number from 0 to 1 for the tree on the spot at `address`:
/// its wind phase, the same for its grove copy and its promoted tree.
pub(crate) fn wind_phase(address: &CopyAddress) -> f32 {
    (FixedHasher.hash_one(address) >> 40) as f32 / (1u64 << 24) as f32
}
