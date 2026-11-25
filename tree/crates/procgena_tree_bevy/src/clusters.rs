//! Fruit and flower clusters drawn on trees through the shared instanced
//! renderer of `procgena_core_bevy`.
//!
//! A tree shows a few cluster specimens (its fruit or flower slots), each
//! placed many times. The specimens become slots of an
//! [`InstancedScatter`], and the placements become the
//! [`InstancedAttached`] rows of the tree's own slot, so each cluster is
//! drawn at the step its own size on screen gives. A grove adds them to its
//! Scatter; a tree drawn on its own gets one child Scatter for them.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use procgena_core::chain::{self, DistanceChain, Step};
use procgena_core::{SpecimenId, content_hash};
use procgena_core_bevy::instanced::{
    AttachedRow, InstancedAttached, InstancedCopy, InstancedPart, InstancedScatter, InstancedSlot,
    PartMesh,
};
use procgena_tree::procgena_fruit::Fruit;
use procgena_tree::{ClusterPart, ClusterRow, LodDetail, Skeleton};

use crate::fruit_impostor::FruitImpostor;
use crate::material::TreeMaterial;
use crate::mesh::attached_cluster_mesh;
use crate::spawn::SingleTree;

/// How much a cluster's bounding sphere is enlarged to cover its flutter.
const FLUTTER_ALLOWANCE: f32 = 1.2;

/// One cluster specimen a tree shows: its id, its size and reach, and its
/// game-level and lower meshes (`None` for a mesh with no triangles).
pub(crate) struct ClusterSpecimen<M> {
    pub(crate) id: SpecimenId,
    /// [`Fruit::size`]: the diameter its steps switch by.
    pub(crate) size: f32,
    /// [`Fruit::radius`], in the cluster's own frame.
    pub(crate) radius: f32,
    /// The hash of the cluster's
    /// [`game_level`](procgena_tree::procgena_fruit::FruitOptions::game_level),
    /// which fixes its meshes but not its specimen id.
    pub(crate) detail: u64,
    /// The game-level mesh, then the lower mesh.
    pub(crate) levels: [Option<M>; 2],
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
    /// Which kind of cluster these are.
    pub(crate) part: ClusterPart,
    pub(crate) material: Handle<TreeMaterial>,
    pub(crate) specimens: Vec<ClusterSpecimen<Handle<Mesh>>>,
    pub(crate) rows: Vec<ClusterRow>,
}

/// The tree's fruit and flowers, with their meshes built, for a background
/// task to return. Parts the tree doesn't grow are left out.
pub(crate) fn build_clusters(skeleton: &Skeleton) -> Vec<BuiltClusters> {
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
                .map(|fruit: &Fruit| {
                    let meshes = fruit.meshes();
                    ClusterSpecimen {
                        id: fruit.id(),
                        size: fruit.size(),
                        radius: fruit.radius(),
                        detail: content_hash(&fruit.options().game_level),
                        levels: [&meshes.game, &meshes.lower].map(|mesh| {
                            (!mesh.indices.is_empty()).then(|| attached_cluster_mesh(mesh))
                        }),
                    }
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
            part: clusters.part,
            material: material(clusters.part),
            rows: clusters.rows,
            specimens: clusters
                .specimens
                .into_iter()
                .map(|specimen| ClusterSpecimen {
                    id: specimen.id,
                    size: specimen.size,
                    radius: specimen.radius,
                    detail: specimen.detail,
                    levels: specimen
                        .levels
                        .map(|mesh| mesh.map(|mesh| meshes.add(mesh))),
                })
                .collect(),
        })
        .collect()
}

/// The baked cluster impostors a set of trees can draw their clusters with:
/// the quad mesh, and each specimen's impostor.
#[derive(Clone, Copy)]
pub(crate) struct ClusterImpostors<'a> {
    pub(crate) quad: &'a Handle<Mesh>,
    pub(crate) baked: &'a HashMap<SpecimenId, FruitImpostor>,
}

/// The instanced slot of one cluster specimen, drawn with `material` as its
/// game-level and lower meshes, then as its impostor where `impostors` has
/// one for it, through the shared [`DistanceChain`].
///
/// It has no cull of its own: the tree decides how far its clusters are
/// drawn. They stop where the tree's impostor takes over (no row covers
/// that step), or at the tree's cull.
fn cluster_slot(
    specimen: &ClusterSpecimen<Handle<Mesh>>,
    material: &Handle<TreeMaterial>,
    impostors: Option<ClusterImpostors>,
) -> InstancedSlot {
    let mut parts: Vec<InstancedPart> = [Step::Game, Step::Lower]
        .into_iter()
        .zip(&specimen.levels)
        .filter_map(|(step, mesh)| {
            Some(InstancedPart {
                step,
                mesh: PartMesh::Mesh(mesh.clone()?),
                material: material.clone().untyped(),
            })
        })
        .collect();
    let impostor = impostors.and_then(|impostors| {
        let baked = impostors.baked.get(&specimen.id)?;
        parts.push(InstancedPart {
            step: Step::Impostor,
            mesh: PartMesh::Mesh(impostors.quad.clone()),
            material: baked.material.clone().untyped(),
        });
        Some(())
    });
    InstancedSlot {
        specimen: specimen.id,
        piece: 0,
        chain: DistanceChain::DEFAULT,
        cull: false,
        impostor: impostor.is_some(),
        parts,
        bounds: (Vec3::ZERO, specimen.radius * FLUTTER_ALLOWANCE),
    }
}

/// The attached rows of one kind of cluster on a tree whose two meshes are
/// built at `tree_lod`. `slot_of` gives the Scatter slot of each of the
/// tree's cluster slots.
///
/// Each of the tree's meshes decides which clusters it shows
/// ([`LodDetail::cluster_stride`]) and how much it enlarges them
/// ([`LodDetail::cluster_scale`]): a cluster gets one row per scale it is
/// drawn at, covering the tree's mesh steps that draw it so. No row covers
/// the tree's impostor step, so clusters stop where it takes over.
fn attached_rows(
    clusters: &TreeClusters,
    tree_lod: &chain::Lod<LodDetail>,
    slot_of: &[u32],
) -> Vec<AttachedRow> {
    let mut rows = Vec::new();
    for row in &clusters.rows {
        // The tree's mesh steps that draw this cluster, grouped by their scale.
        let mut scales: Vec<(f32, u32)> = Vec::new();
        for (step, detail) in tree_lod.meshes() {
            if row.index % detail.cluster_stride.max(1) != 0 {
                continue;
            }
            let bit = 1 << step.index();
            match scales.iter_mut().find(|(s, _)| *s == detail.cluster_scale) {
                Some((_, mask)) => *mask |= bit,
                None => scales.push((detail.cluster_scale, bit)),
            }
        }

        // One row per scale.
        let specimen = &clusters.specimens[row.slot as usize];
        for (scale, host_steps) in scales {
            let scale = row.scale * scale;
            rows.push(AttachedRow {
                slot: slot_of[row.slot as usize],
                position: row.origin,
                rotation: row.rotation,
                scale,
                size: specimen.size * scale,
                wind: Vec3::from_array(row.wind),
                host_steps,
            });
        }
    }
    rows
}

/// Scatter slots and rows for the clusters of several trees, each given
/// with the details its two meshes are built at. Cluster slots start at
/// Scatter slot `first`, and trees that show the same specimen share its
/// slot. A specimen that `impostors` has an impostor for draws it when it is
/// small on screen. Returns the cluster slots, and each tree's rows, in the
/// order `trees` gives them.
pub(crate) fn grove_clusters<'a>(
    trees: impl IntoIterator<Item = (&'a [TreeClusters], &'a chain::Lod<LodDetail>)>,
    first: u32,
    impostors: Option<ClusterImpostors>,
) -> (Vec<InstancedSlot>, Vec<Arc<[AttachedRow]>>) {
    let mut slots = Vec::new();
    let mut index: HashMap<SpecimenId, u32> = HashMap::new();
    let mut tables = Vec::new();
    for (tree, tree_lod) in trees {
        let mut rows = Vec::new();
        for clusters in tree {
            let slot_of: Vec<u32> = clusters
                .specimens
                .iter()
                .map(|specimen| {
                    *index.entry(specimen.id).or_insert_with(|| {
                        slots.push(cluster_slot(specimen, &clusters.material, impostors));
                        first + slots.len() as u32 - 1
                    })
                })
                .collect();
            rows.extend(attached_rows(clusters, tree_lod, &slot_of));
        }
        tables.push(Arc::from(rows));
    }
    (slots, tables)
}

/// What a tree drawn on its own needs to draw its clusters: a Scatter of
/// one copy of the tree (slot 0: the tree specimen `tree`, with no parts,
/// carrying only the chain's steps, with the impostor step once
/// `impostor` is shown) with its cluster specimens as the other slots, and
/// the rows. `tree_lod` gives the details of its two meshes, `bounds` a
/// sphere around it, and `copy` its size and wind phase, so the clusters
/// switch and sway with the tree. Clusters are drawn as their impostors
/// when small on screen where `impostors` has one. `None` when the tree
/// grows no clusters.
pub(crate) fn single_tree_clusters(
    clusters: &[TreeClusters],
    tree_lod: &chain::Lod<LodDetail>,
    tree: SpecimenId,
    impostor: bool,
    bounds: (Vec3, f32),
    copy: SingleTree,
    impostors: Option<ClusterImpostors>,
) -> Option<(InstancedScatter, InstancedAttached)> {
    if clusters.is_empty() {
        return None;
    }
    let host = InstancedSlot {
        specimen: tree,
        piece: 0,
        chain: DistanceChain::DEFAULT,
        cull: true,
        impostor,
        parts: Vec::new(),
        bounds,
    };
    let (cluster_slots, tables) = grove_clusters([(clusters, tree_lod)], 1, impostors);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A specimen with both meshes, 10 cm across.
    fn specimen(meshes: &mut Assets<Mesh>) -> ClusterSpecimen<Handle<Mesh>> {
        let mut mesh = || Some(meshes.add(Cuboid::from_length(0.1)));
        ClusterSpecimen {
            id: SpecimenId::new(
                1,
                procgena_core::PoolId::new(
                    procgena_core::GeneratorKind::from_key("procgena_tree_bevy.test.cluster"),
                    procgena_core::OptionsHash(1),
                ),
                0,
            ),
            size: 0.1,
            radius: 0.05,
            detail: 0,
            levels: [mesh(), mesh()],
        }
    }

    /// A cluster slot draws the shared chain's steps, with its impostor
    /// last once baked, and no cull of its own: its tree decides how far it
    /// is drawn.
    #[test]
    fn a_cluster_slot_has_the_chain_without_a_cull() {
        let mut meshes = Assets::<Mesh>::default();
        let specimen = specimen(&mut meshes);
        let material = Handle::<TreeMaterial>::default();
        let quad = meshes.add(Rectangle::default());
        let mut baked = HashMap::new();

        let without = cluster_slot(&specimen, &material, None);
        let impostors = ClusterImpostors {
            quad: &quad,
            baked: &baked,
        };
        let unbaked = cluster_slot(&specimen, &material, Some(impostors));
        let material_handle = Handle::default();
        baked.insert(
            specimen.id,
            FruitImpostor {
                material: material_handle,
            },
        );
        let impostors = ClusterImpostors {
            quad: &quad,
            baked: &baked,
        };
        let with = cluster_slot(&specimen, &material, Some(impostors));

        for slot in [&without, &unbaked, &with] {
            assert_eq!(slot.chain, DistanceChain::DEFAULT);
            assert!(!slot.cull, "the tree decides");
        }
        let steps = |slot: &InstancedSlot| slot.parts.iter().map(|p| p.step).collect::<Vec<_>>();
        assert!(!without.impostor && !unbaked.impostor);
        assert_eq!(steps(&without), [Step::Game, Step::Lower]);
        assert_eq!(steps(&unbaked), [Step::Game, Step::Lower]);
        assert!(with.impostor);
        assert_eq!(steps(&with), Step::ALL);
    }
}
