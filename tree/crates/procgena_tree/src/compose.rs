//! The tree's fruit and flowers: the cluster table, and the clusters moved
//! into place as one mesh.

use glam::{Quat, Vec3};
use procgena_core::Quality;
use procgena_fruit::{Fruit, FruitMesh, default_lod};

use crate::mesh::{ClusterSpan, LodDetail, MeshBuffers};
use crate::wind::{BranchWind, branch_winds, wind_at};
use crate::{ClusterPart, Skeleton, SkeletonCluster};

/// One row of a tree's cluster table: a placed cluster with everything a
/// renderer needs to draw its specimen there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClusterRow {
    /// The cluster's index in [`Skeleton::fruit`] or [`Skeleton::flowers`]:
    /// part `fruit i` or `flower i`.
    pub index: u32,
    /// The slot of the specimen the cluster shows.
    pub slot: u32,
    /// Where the cluster attaches, in tree coordinates.
    pub origin: Vec3,
    /// Rotation from the cluster frame to tree coordinates.
    pub rotation: Quat,
    /// Scale from the cluster frame to tree coordinates.
    pub scale: f32,
    /// The branch sway at the attachment, `[trunk_bend, branch_weight,
    /// branch_phase]`, as the first three values of
    /// [`MeshBuffers::wind`]. Add each specimen vertex's flutter as the fourth.
    pub wind: [f32; 3],
    /// The radius of the ball around [`origin`](Self::origin) that holds
    /// the whole cluster, in tree coordinates.
    pub radius: f32,
}

impl Skeleton {
    /// The cluster table of one part: a row per placed cluster, sorted by
    /// slot and then by index, so the rows that show one specimen are
    /// together.
    pub fn cluster_table(&self, part: ClusterPart) -> Vec<ClusterRow> {
        let (clusters, slots) = self.clusters(part);
        let winds = branch_winds(self);
        let mut rows: Vec<ClusterRow> = (0u32..)
            .zip(clusters)
            .map(|(index, cluster)| ClusterRow {
                index,
                slot: cluster.slot,
                origin: cluster.origin,
                rotation: cluster.orientation,
                scale: cluster.scale,
                wind: wind_at(&winds, &cluster.parent),
                radius: slots[cluster.slot as usize].radius() * cluster.scale,
            })
            .collect();
        rows.sort_by_key(|row| (row.slot, row.index));
        rows
    }
}

/// Appends the clusters `detail` keeps to `out`, each at its own fruit level
/// for [`LodDetail::fruit_distance`], and returns their spans in skeleton
/// order. Clusters past the fruit levels' cull distance are left out.
pub(crate) fn compose_clusters(
    out: &mut MeshBuffers,
    clusters: &[SkeletonCluster],
    slots: &[Fruit],
    winds: &[BranchWind],
    detail: &LodDetail,
) -> Vec<ClusterSpan> {
    let lod = default_lod();
    let stride = detail.cluster_stride.max(1) as usize;
    let mut meshes: Vec<Vec<Option<FruitMesh>>> = vec![vec![None; lod.levels.len()]; slots.len()];
    let mut spans = Vec::with_capacity(clusters.len().div_ceil(stride));
    for (index, cluster) in clusters.iter().enumerate().step_by(stride) {
        // The cluster's own level, from its size where it is drawn.
        let fruit = &slots[cluster.slot as usize];
        let scale = cluster.scale * detail.cluster_scale;
        let factor = lod.factor(fruit.size() * scale, Quality::High);
        let Some(level) = lod.level_at(detail.fruit_distance, factor) else {
            continue;
        };
        let mesh = meshes[cluster.slot as usize][level]
            .get_or_insert_with(|| fruit.mesh(&lod.levels[level].detail));

        // Move the specimen's mesh into place, with its branch's sway.
        let (first_vertex, first_index) = (out.next_index(), out.indices.len());
        let [bend, weight, phase] = wind_at(winds, &cluster.parent);
        for k in 0..mesh.vertex_count() {
            let position = Vec3::from_array(mesh.positions[k]);
            let normal = cluster.orientation * Vec3::from_array(mesh.normals[k]);
            let position = cluster.origin + cluster.orientation * (position * scale);
            let wind = [bend, weight, phase, mesh.flutter[k]];
            out.push_colored(position, normal, mesh.uvs[k], wind, mesh.colors[k]);
        }
        out.indices
            .extend(mesh.indices.iter().map(|i| first_vertex + i));
        spans.push(ClusterSpan {
            cluster: index,
            vertices: first_vertex..out.next_index(),
            indices: index_range(first_index, out.indices.len()),
        });
    }
    spans
}

/// `start..end` as index-buffer offsets. Panics if either exceeds `u32::MAX`.
fn index_range(start: usize, end: usize) -> std::ops::Range<u32> {
    let convert = |i: usize| u32::try_from(i).expect("index buffer longer than u32::MAX");
    convert(start)..convert(end)
}
