//! Cluster cards: fruit and flower clusters drawn far away as one
//! camera-facing card each, showing a baked picture of a cluster.
//!
//! [`cluster_card_mesh`] builds the cluster a card shows, on its own in
//! cluster-local coordinates, for a renderer to bake. With
//! [`LodDetail::cluster_cards`] set, [`Skeleton::mesh`](crate::Skeleton::mesh)
//! then puts one card per kept cluster into [`TreeMesh::fruit_cards`] and
//! [`TreeMesh::flower_cards`].

use glam::{Quat, Vec3};
use procgena_core::hash::{derive_seed, pcg2};

use crate::cluster::{SkeletonCluster, cluster_extent, grow_lone_cluster};
use crate::cluster_mesh::{ItemSelection, mesh_flowers, mesh_fruit};
use crate::mesh::BillboardVertex;
use crate::wind::{BranchWind, wind_at};
use crate::{LodDetail, MeshBuffers, TreeOptions};

/// Which clusters of a tree: its fruit or its flowers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClusterPart {
    /// The fruit clusters ([`TreeOptions::fruit`]).
    Fruit,
    /// The flower clusters ([`TreeOptions::flowers`]).
    Flowers,
}

impl ClusterPart {
    /// Both parts, fruit first.
    pub const ALL: [Self; 2] = [Self::Fruit, Self::Flowers];

    /// The part's name, `"fruit"` or `"flowers"`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Fruit => "fruit",
            Self::Flowers => "flowers",
        }
    }
}

/// The cluster that cards of `part` show for cluster variant `variant`, as
/// a mesh in cluster-local coordinates: the cluster starts at the origin
/// and its axis is +Y. Wind data is all zero.
///
/// Variant 0 is a representative cluster: the settings' mean item count,
/// with no random variation in item count, size or facing. Variants 1 and
/// up are grown with random variation, each from its own seed. The mesh
/// depends only on `options` without their seed, so every tree of a pool
/// shows the same pictures. Empty when the tree grows no such clusters.
pub fn cluster_card_mesh(options: &TreeOptions, part: ClusterPart, variant: u32) -> MeshBuffers {
    let cluster = lone_cluster(options, part, variant);
    let winds = [BranchWind {
        bend: vec![0.0],
        weight: vec![0.0],
        phase: 0.0,
    }];
    let detail = LodDetail::default();
    let mut out = MeshBuffers::default();
    let mut cull = MeshBuffers::default();
    let clusters = std::slice::from_ref(&cluster);
    match part {
        ClusterPart::Fruit => {
            mesh_fruit(
                [&mut out, &mut cull],
                clusters,
                &options.fruit,
                &winds,
                &detail,
            );
        }
        ClusterPart::Flowers => {
            mesh_flowers(&mut out, clusters, &options.flowers, &winds, &detail);
        }
    }
    out.wind.fill([0.0; 4]);
    out
}

/// The sphere a card's picture covers: the centre and radius of a sphere
/// around every vertex of `buffers`, the cluster's
/// [`cluster_card_mesh`]. A renderer must bake the picture over this
/// sphere, centred on it, for cards to line up with their clusters.
/// Returns a tiny sphere at the origin for an empty mesh.
pub fn cluster_card_bounds(buffers: &MeshBuffers) -> (Vec3, f32) {
    let points = || buffers.positions.iter().map(|p| Vec3::from_array(*p));
    if buffers.positions.is_empty() {
        return (Vec3::ZERO, 1e-3);
    }
    let (min, max) = points().fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(min, max), p| {
        (min.min(p), max.max(p))
    });
    let center = (min + max) / 2.0;
    let radius = points().map(|p| p.distance(center)).fold(1e-3f32, f32::max);
    (center, radius)
}

/// The lone cluster cards of `part` show for `variant`.
fn lone_cluster(options: &TreeOptions, part: ClusterPart, variant: u32) -> SkeletonCluster {
    let (placement, shape) = match part {
        ClusterPart::Fruit => (&options.fruit.placement, &options.fruit.cluster),
        ClusterPart::Flowers => (&options.flowers.placement, &options.flowers.cluster),
    };
    let options_hash = options.options_hash().0;
    let pool = derive_seed(options_hash as u32, (options_hash >> 32) as u32);
    let seed = derive_seed(pool, pcg2(variant, part as u32));
    grow_lone_cluster(placement, shape, seed, variant == 0)
}

/// One card variant: its cluster's extent, and the sphere its picture covers.
struct CardVariant {
    extent: f32,
    center: Vec3,
    radius: f32,
}

/// Appends one card per cluster that `detail` keeps to `cards`, one buffer
/// per variant (`detail.cluster_cards` of them). Each cluster picks its
/// variant by a hash of its index. Pass the skeleton's `clusters` of
/// `part` and the branch winds they attach to.
pub(crate) fn mesh_cards(
    cards: &mut Vec<MeshBuffers>,
    clusters: &[SkeletonCluster],
    options: &TreeOptions,
    part: ClusterPart,
    winds: &[BranchWind],
    detail: &LodDetail,
) {
    let size = match part {
        ClusterPart::Fruit => options.fruit.placement.size,
        ClusterPart::Flowers => options.flowers.placement.size,
    };
    if clusters.is_empty() || size < detail.min_item_size {
        return;
    }
    let count = detail.cluster_cards.max(1);
    let variants: Vec<CardVariant> = (0..count)
        .map(|variant| {
            let (center, radius) = cluster_card_bounds(&cluster_card_mesh(options, part, variant));
            CardVariant {
                extent: cluster_extent(&lone_cluster(options, part, variant)).max(1e-6),
                center,
                radius,
            }
        })
        .collect();
    cards.resize_with(count as usize, MeshBuffers::default);

    // Every kept cluster, whatever its items.
    let selection = ItemSelection::new(detail, false);
    for kept in selection.kept(clusters) {
        let variant = (pcg2(kept.index as u32, part as u32) % count) as usize;
        let card = &variants[variant];
        let cluster = kept.cluster;
        let scale = cluster_extent(cluster) / card.extent;
        let axis = (cluster.orientation * Vec3::Y).normalize();
        let turn = Quat::from_rotation_arc(Vec3::Y, axis);
        let pivot = cluster.origin + turn * (card.center * scale);
        let radius = card.radius * scale;
        let [bend, weight, phase] = wind_at(winds, &cluster.parent);
        push_card(
            &mut cards[variant],
            pivot,
            axis,
            turn * Vec3::X,
            radius,
            [bend, weight, phase, 0.0],
        );
    }
}

/// Appends a square card of half-width `radius` at `pivot`, turned about
/// `axis`, unturned across `side`, with wind data `wind` at every corner.
fn push_card(
    out: &mut MeshBuffers,
    pivot: Vec3,
    axis: Vec3,
    side: Vec3,
    radius: f32,
    wind: [f32; 4],
) {
    let base = out.positions.len() as u32;
    for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let position = pivot + side * (x * radius) + axis * (y * radius);
        out.positions.push(position.to_array());
        out.uvs.push([(x + 1.0) / 2.0, (1.0 - y) / 2.0]);
        out.wind.push(wind);
        out.colors.push([1.0; 4]);
        out.billboard.push(BillboardVertex {
            pivot: pivot.to_array(),
            axis: axis.to_array(),
            offset: [x * radius, y * radius],
        });
    }
    out.indices
        .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
}
