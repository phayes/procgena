//! Levels of detail for fruit and flower clusters.

use procgena_core::{Lod, LodLevel};

use crate::{ClusterDetail, ItemDetail};

/// A cluster's levels of detail: which [`ClusterDetail`] to draw at which
/// distance.
pub type FruitLod = Lod<ClusterDetail>;

/// The cluster size, in metres, that [`default_lod`]'s distances are
/// written for: a cluster 10 cm across. Clusters of other sizes scale the
/// distances by their [`Fruit::size`](crate::Fruit::size) over this.
pub const FRUIT_LOD_REFERENCE_SIZE: f32 = 0.1;

/// Three suggested mesh levels for a 10 cm cluster, each fading in over
/// 10% either side of its distance:
///
/// - from 0 m, full detail;
/// - from 3 m, [`ItemDetail::Simple`]: fewer sides, three-sided stalks, no
///   calyx;
/// - from 8 m, [`ItemDetail::Minimal`]: every second item at 1.25 times its
///   size, flat strips for stalks, and fruit or flowers smaller than 7.5 cm
///   drawn flat. It casts no shadows.
///
/// There is no cull distance: a cluster grown on a host is drawn until the
/// host's own far form takes over.
pub fn default_lod() -> FruitLod {
    let simple = ClusterDetail {
        item_detail: ItemDetail::Simple,
        ..ClusterDetail::default()
    };
    let minimal = ClusterDetail {
        item_detail: ItemDetail::Minimal,
        item_stride: 2,
        item_scale: 1.25,
        flat_item_size: 0.075,
        ..ClusterDetail::default()
    };
    Lod::new(
        FRUIT_LOD_REFERENCE_SIZE,
        vec![
            LodLevel::new(0.0, 0.0, ClusterDetail::default()),
            LodLevel::new(3.0, 0.1, simple),
            LodLevel::new(8.0, 0.1, minimal).without_shadows(),
        ],
    )
}
