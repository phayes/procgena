//! Terrain features: the rivers, roads and ponds a terrain plans and cuts
//! into its ground, seen as one kind of thing, with one spatial index over
//! where each of them can change the ground.

use std::collections::{BTreeSet, HashMap};

use glam::DVec2;

use crate::carve::{FeatureKind, FeatureLine, Pond};
use crate::hash::mix64;

/// Which feature of a terrain: its kind and a key made from where the
/// feature starts.
///
/// A feature keeps its id however many other features the terrain plans,
/// and whatever tier the terrain is built to (see [`FeatureTier`]): the
/// same world built again gives the same ids, and a river keeps its id
/// when a neighbour's river starts flowing in. A feature that moves, such
/// as a river whose source moves when the settings change, gets a new id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FeatureId {
    /// What the feature is.
    pub kind: FeatureKind,
    /// The key: a hash of where the feature starts.
    pub key: u64,
}

/// How far a terrain's features are planned, from rough to fine.
///
/// A terrain is built tier by tier (see
/// [`Terrain::with_tier`](crate::Terrain::with_tier)). Each kind of feature
/// says which tiers it is drawn at ([`FeatureKind::tiers`]), by how costly it
/// is to plan, so a far view can be drawn before the costly features are
/// planned. Only a terrain at [`Fine`](Self::Fine) is the real ground:
/// placement, masks and queries must read that, and an earlier tier is for
/// drawing only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum FeatureTier {
    /// The relief, its borders and the cheap features: enough to draw far
    /// ground soon.
    Rough,
    /// Every feature: the real ground.
    #[default]
    Fine,
}

impl FeatureTier {
    /// Every tier, from roughest to finest.
    pub const ALL: [FeatureTier; 2] = [FeatureTier::Rough, FeatureTier::Fine];

    /// The position of this tier in [`ALL`](Self::ALL).
    pub(crate) fn position(self) -> usize {
        Self::ALL
            .iter()
            .position(|&t| t == self)
            .expect("every tier is listed")
    }
}

impl FeatureKind {
    /// Every kind of feature.
    pub const ALL: [FeatureKind; 3] = [FeatureKind::River, FeatureKind::Pond, FeatureKind::Road];

    /// The tiers this kind of feature is drawn at, roughest first. A kind
    /// drawn at several tiers is planned once, at the first of them, and
    /// kept unchanged at the later ones. Every kind is drawn at
    /// [`Fine`](FeatureTier::Fine).
    ///
    /// Rivers and ponds are cheap to plan, so they are drawn at every
    /// tier; roads take most of a region's planning, so they are fine only.
    pub fn tiers(self) -> &'static [FeatureTier] {
        match self {
            Self::River | Self::Pond => &FeatureTier::ALL,
            Self::Road => &[FeatureTier::Fine],
        }
    }

    /// The first tier this kind of feature is drawn at.
    pub fn first_tier(self) -> FeatureTier {
        self.tiers()[0]
    }

    /// The kinds of feature this kind reads while it is planned: roads
    /// avoid rivers and ponds, and ponds end rivers. A kind is never drawn
    /// at a tier before the ones it reads.
    pub fn reads(self) -> &'static [FeatureKind] {
        match self {
            Self::River => &[],
            Self::Pond => &[FeatureKind::River],
            Self::Road => &[FeatureKind::River, FeatureKind::Pond],
        }
    }
}

/// The keys of features of one `kind` that start at `starts` (each a
/// feature's first two points, or a pond's centre twice), in order: a hash
/// of the start, moved on to the next free key where two features start
/// at the same place.
pub(crate) fn feature_keys(kind: FeatureKind, starts: &[[DVec2; 2]]) -> Vec<u64> {
    let mut taken = std::collections::HashSet::new();
    starts
        .iter()
        .map(|[a, b]| {
            let mut key = [a.x, a.y, b.x, b.y]
                .iter()
                .fold(mix64(0, kind as u64), |h, v| mix64(h, v.to_bits()));
            while !taken.insert(key) {
                key = key.wrapping_add(1);
            }
            key
        })
        .collect()
}

/// The shape of a [`TerrainFeature`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FeatureShape<'a> {
    /// A river or road: a line of points with levels and widths.
    Line(&'a FeatureLine),
    /// A pond.
    Pond(&'a Pond),
}

/// One feature of a terrain: a river, road or pond, with the box outside
/// which it never changes the ground.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerrainFeature<'a> {
    /// Which feature it is.
    pub id: FeatureId,
    /// Its shape.
    pub shape: FeatureShape<'a>,
    /// The corners of the box, in metres (x and z), with the smaller
    /// coordinates first, outside which the feature changes neither the
    /// ground's height nor its water or road tags. It includes the
    /// feature's banks and valley.
    pub bounds: [DVec2; 2],
}

/// One piece of a feature in the [`FeatureIndex`]: a segment of a line
/// (from its point `segment` to the next), or a whole pond (`segment` 0).
/// `index` is the feature's place in its kind's list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Part {
    pub id: FeatureId,
    pub index: u32,
    pub segment: u32,
}

/// Size of the index's cells, in metres.
const INDEX_CELL: f64 = 64.0;

/// Which feature pieces can change the ground in each 64 m cell.
#[derive(Debug, Clone, Default)]
pub(crate) struct FeatureIndex {
    cells: HashMap<(i64, i64), Vec<Part>>,
    /// Each piece's box outside which it changes nothing.
    boxes: HashMap<Part, [DVec2; 2]>,
}

impl FeatureIndex {
    /// The index of `parts`, each given with the corners of the box
    /// outside which it changes nothing. Each cell lists its pieces in the
    /// order given.
    pub(crate) fn new(parts: impl IntoIterator<Item = (Part, [DVec2; 2])>) -> Self {
        let mut cells: HashMap<(i64, i64), Vec<Part>> = HashMap::new();
        let mut boxes = HashMap::new();
        for (part, [lo, hi]) in parts {
            boxes.insert(part, [lo, hi]);
            let (lo, hi) = ((lo / INDEX_CELL).floor(), (hi / INDEX_CELL).floor());
            for z in lo.y as i64..=hi.y as i64 {
                for x in lo.x as i64..=hi.x as i64 {
                    cells.entry((x, z)).or_default().push(part);
                }
            }
        }
        Self { cells, boxes }
    }

    /// The pieces that may change the ground at `p`, in the order given;
    /// empty where none can.
    pub(crate) fn at(&self, p: DVec2) -> &[Part] {
        let key = (
            (p.x / INDEX_CELL).floor() as i64,
            (p.y / INDEX_CELL).floor() as i64,
        );
        self.cells.get(&key).map_or(&[], Vec::as_slice)
    }

    /// The features with a piece whose box meets the box from `lo` to
    /// `hi`, in id order.
    pub(crate) fn near(&self, lo: DVec2, hi: DVec2) -> BTreeSet<FeatureId> {
        let (cell_lo, cell_hi) = ((lo / INDEX_CELL).floor(), (hi / INDEX_CELL).floor());
        let mut out = BTreeSet::new();
        for z in cell_lo.y as i64..=cell_hi.y as i64 {
            for x in cell_lo.x as i64..=cell_hi.x as i64 {
                let Some(parts) = self.cells.get(&(x, z)) else {
                    continue;
                };
                for part in parts {
                    let [a, b] = self.boxes[part];
                    if a.x <= hi.x && a.y <= hi.y && b.x >= lo.x && b.y >= lo.y {
                        out.insert(part.id);
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_is_fine_and_never_drawn_before_what_it_reads() {
        for kind in FeatureKind::ALL {
            let tiers = kind.tiers();
            assert!(!tiers.is_empty() && tiers.is_sorted(), "{kind:?}");
            assert_eq!(tiers.last(), Some(&FeatureTier::Fine), "{kind:?}");
            for &read in kind.reads() {
                assert!(
                    read.first_tier() <= kind.first_tier(),
                    "{kind:?} reads {read:?}, which comes later"
                );
            }
        }
    }
}
