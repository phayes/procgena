//! Fruit and flower clusters drawn on trees through the shared instanced
//! renderer of `procgena_core_bevy`.
//!
//! A tree shows a few cluster specimens (its fruit or flower slots), each
//! placed many times. The specimens become slots of an
//! [`InstancedScatter`], and the placements become the
//! [`InstancedAttached`] rows of the tree's own slot, so each cluster is
//! drawn at the fruit level its own distance gives. A grove adds them to its
//! Scatter; a tree drawn on its own gets one child Scatter for them.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use procgena_core::{Lod, LodLevel, SpecimenId};
use procgena_core_bevy::instanced::{
    AttachedRow, InstancedAttached, InstancedCopy, InstancedPart, InstancedScatter, InstancedSlot,
    PartMesh,
};
use procgena_tree::procgena_fruit::{Fruit, default_lod as fruit_lod};
use procgena_tree::{ClusterPart, ClusterRow, Skeleton};

use crate::material::TreeMaterial;
use crate::mesh::attached_cluster_mesh;
use crate::spawn::{SingleTree, TreeLevel};

/// How much a cluster's bounding sphere is enlarged to cover its flutter.
const FLUTTER_ALLOWANCE: f32 = 1.2;

/// One cluster specimen a tree shows: its id, its size and reach, and its
/// mesh at every fruit level (`None` for an empty level).
pub(crate) struct ClusterSpecimen<M> {
    pub(crate) id: SpecimenId,
    /// [`Fruit::size`], which sizes its fruit levels.
    pub(crate) size: f32,
    /// [`Fruit::radius`], in the cluster's own frame.
    pub(crate) radius: f32,
    pub(crate) levels: Vec<Option<M>>,
}

/// One kind of cluster on a tree (its fruit or its flowers) as a background
/// task builds it: the specimens of its slots with their meshes, and one
/// row per placed cluster.
pub(crate) struct BuiltClusters {
    pub(crate) part: ClusterPart,
    /// The specimens, by the tree's slot index.
    pub(crate) specimens: Vec<ClusterSpecimen<Mesh>>,
    /// The tree's cluster table (see [`Skeleton::cluster_table`]).
    pub(crate) rows: Vec<ClusterRow>,
}

/// One kind of cluster on a tree, ready to draw: as [`BuiltClusters`], with
/// mesh assets and the material.
pub(crate) struct TreeClusters {
    pub(crate) material: Handle<TreeMaterial>,
    pub(crate) specimens: Vec<ClusterSpecimen<Handle<Mesh>>>,
    pub(crate) rows: Vec<ClusterRow>,
}

/// The tree's fruit and flowers, with their meshes built, for a background
/// task to return. Parts the tree doesn't grow are left out.
pub(crate) fn build_clusters(skeleton: &Skeleton) -> Vec<BuiltClusters> {
    let lod = fruit_lod();
    [ClusterPart::Fruit, ClusterPart::Flowers]
        .into_iter()
        .filter_map(|part| {
            let rows = skeleton.cluster_table(part);
            if rows.is_empty() {
                return None;
            }
            let (_, slots) = skeleton.clusters(part);
            let specimens = slots
                .iter()
                .map(|fruit: &Fruit| ClusterSpecimen {
                    id: fruit.id(),
                    size: fruit.size(),
                    radius: fruit.radius(),
                    levels: lod
                        .levels
                        .iter()
                        .map(|level| {
                            let mesh = fruit.mesh(&level.detail);
                            (!mesh.indices.is_empty()).then(|| attached_cluster_mesh(&mesh))
                        })
                        .collect(),
                })
                .collect();
            Some(BuiltClusters {
                part,
                specimens,
                rows,
            })
        })
        .collect()
}

/// [`build_clusters`]' output with its meshes added to `meshes`, drawn with
/// the material `material` gives each part.
pub(crate) fn add_meshes(
    clusters: Vec<BuiltClusters>,
    meshes: &mut Assets<Mesh>,
    material: impl Fn(ClusterPart) -> Handle<TreeMaterial>,
) -> Vec<TreeClusters> {
    clusters
        .into_iter()
        .map(|clusters| TreeClusters {
            material: material(clusters.part),
            rows: clusters.rows,
            specimens: clusters
                .specimens
                .into_iter()
                .map(|specimen| ClusterSpecimen {
                    id: specimen.id,
                    size: specimen.size,
                    radius: specimen.radius,
                    levels: specimen
                        .levels
                        .into_iter()
                        .map(|mesh| mesh.map(|mesh| meshes.add(mesh)))
                        .collect(),
                })
                .collect(),
        })
        .collect()
}

/// The instanced slot of one cluster specimen, drawn with `material` at
/// the fruit levels.
fn cluster_slot(
    specimen: &ClusterSpecimen<Handle<Mesh>>,
    material: &Handle<TreeMaterial>,
) -> InstancedSlot {
    let parts = (0u32..)
        .zip(&specimen.levels)
        .filter_map(|(level, mesh)| {
            Some(InstancedPart {
                level,
                mesh: PartMesh::Mesh(mesh.clone()?),
                material: material.clone().untyped(),
            })
        })
        .collect();
    InstancedSlot {
        specimen: specimen.id,
        piece: 0,
        lod: fruit_lod().map_detail(|_| ()),
        parts,
        bounds: (Vec3::ZERO, specimen.radius * FLUTTER_ALLOWANCE),
    }
}

/// The attached rows of one kind of cluster on a tree whose mesh levels are
/// `tree_levels`. `slot_of` gives the Scatter slot of each of the tree's
/// cluster slots.
///
/// The tree decides which clusters each of its levels shows
/// ([`LodDetail::cluster_stride`](procgena_tree::LodDetail::cluster_stride))
/// and how much it enlarges them
/// ([`LodDetail::cluster_scale`](procgena_tree::LodDetail::cluster_scale)):
/// a cluster gets one row per scale it is drawn at, covering the tree
/// levels that draw it so. No row covers a level past the mesh levels, so
/// clusters stop where the tree's impostor takes over.
fn attached_rows(
    clusters: &TreeClusters,
    tree_levels: &[LodLevel<TreeLevel>],
    slot_of: &[u32],
) -> Vec<AttachedRow> {
    let mut rows = Vec::new();
    for row in &clusters.rows {
        // The tree levels that draw this cluster, grouped by their scale.
        let mut scales: Vec<(f32, u32)> = Vec::new();
        for (level, tree_level) in tree_levels.iter().enumerate().take(32) {
            let detail = &tree_level.detail.detail;
            if row.index % detail.cluster_stride.max(1) != 0 {
                continue;
            }
            let bit = 1 << level;
            match scales.iter_mut().find(|(s, _)| *s == detail.cluster_scale) {
                Some((_, mask)) => *mask |= bit,
                None => scales.push((detail.cluster_scale, bit)),
            }
        }

        // One row per scale.
        let specimen = &clusters.specimens[row.slot as usize];
        for (scale, host_levels) in scales {
            let scale = row.scale * scale;
            rows.push(AttachedRow {
                slot: slot_of[row.slot as usize],
                position: row.origin,
                rotation: row.rotation,
                scale,
                size: specimen.size * scale,
                wind: Vec3::from_array(row.wind),
                host_levels,
            });
        }
    }
    rows
}

/// Scatter slots and rows for the clusters of several trees, whose mesh
/// levels are `tree_levels`. Cluster slots start at Scatter slot `first`,
/// and trees that show the same specimen share its slot. Returns the
/// cluster slots, and each tree's rows, in the order `trees` gives them.
pub(crate) fn grove_clusters<'a>(
    trees: impl IntoIterator<Item = &'a [TreeClusters]>,
    tree_levels: &[LodLevel<TreeLevel>],
    first: u32,
) -> (Vec<InstancedSlot>, Vec<Arc<[AttachedRow]>>) {
    let mut slots = Vec::new();
    let mut index: HashMap<SpecimenId, u32> = HashMap::new();
    let mut tables = Vec::new();
    for tree in trees {
        let mut rows = Vec::new();
        for clusters in tree {
            let slot_of: Vec<u32> = clusters
                .specimens
                .iter()
                .map(|specimen| {
                    *index.entry(specimen.id).or_insert_with(|| {
                        slots.push(cluster_slot(specimen, &clusters.material));
                        first + slots.len() as u32 - 1
                    })
                })
                .collect();
            rows.extend(attached_rows(clusters, tree_levels, &slot_of));
        }
        tables.push(Arc::from(rows));
    }
    (slots, tables)
}

/// What a tree drawn on its own needs to draw its clusters: a Scatter of
/// one copy of the tree (slot 0: the tree specimen `tree`, with no parts,
/// carrying only the tree's levels `tree_lod`) with its cluster specimens
/// as the other slots, and the rows. `tree_levels` are its mesh levels,
/// `bounds` a sphere around it, and `copy` its size and wind phase, so the
/// clusters switch and sway with the tree. `None` when the tree grows no
/// clusters.
pub(crate) fn single_tree_clusters(
    clusters: &[TreeClusters],
    tree: SpecimenId,
    tree_levels: &[LodLevel<TreeLevel>],
    tree_lod: Lod,
    bounds: (Vec3, f32),
    copy: SingleTree,
) -> Option<(InstancedScatter, InstancedAttached)> {
    if clusters.is_empty() {
        return None;
    }
    let host = InstancedSlot {
        specimen: tree,
        piece: 0,
        lod: tree_lod,
        parts: Vec::new(),
        bounds,
    };
    let (cluster_slots, tables) = grove_clusters([clusters], tree_levels, 1);
    let slots: Vec<InstancedSlot> = std::iter::once(host).chain(cluster_slots).collect();
    Some((
        InstancedScatter {
            slots: slots.into(),
            copies: Arc::from([InstancedCopy {
                slot: 0,
                transform: default(),
                size: copy.size,
                wind_phase: copy.wind_phase,
            }]),
        },
        InstancedAttached {
            tables: tables.into(),
        },
    ))
}
