//! Check a cluster's stalks and items in its own frame: arrangements, sizes,
//! counts, and how the specimens of one pool differ.

use glam::Vec3;
use procgena_fruit::{
    ClusterDetail, ClusterKind, Facing, Fruit, FruitOptions, FruitPreset, FruitSkeleton,
};

/// Default fruit settings arranged as `kind`.
fn arranged(kind: ClusterKind) -> FruitOptions {
    let mut options = FruitOptions::default();
    options.cluster.kind = kind;
    options
}

/// Grows slot `slot` of `options`' pool, outside a World.
fn grow_slot(options: &FruitOptions, slot: u32) -> Fruit {
    Fruit::new(options.clone(), options.default_seed, slot).unwrap()
}

/// Grows the first slot and checks it is well formed.
#[track_caller]
fn grow(options: &FruitOptions) -> FruitSkeleton {
    let skeleton = grow_slot(options, 0).skeleton().clone();
    assert_well_formed(&skeleton);
    skeleton
}

/// Checks finite positions, unit rotations, non-negative sizes and path
/// distances, stalks of at least two points, and references to existing stalks.
#[track_caller]
fn assert_well_formed(skeleton: &FruitSkeleton) {
    assert!(!skeleton.items.is_empty());
    for (k, stalk) in skeleton.stalks.iter().enumerate() {
        assert!(stalk.points.len() >= 2);
        assert!(stalk.points.iter().all(|p| p.is_finite()));
        assert!(stalk.base_distance >= 0.0);
        assert!(stalk.parent.is_none_or(|p| p < k));
    }
    for item in &skeleton.items {
        assert!(item.origin.is_finite() && item.orientation.is_normalized());
        assert!(item.size.is_finite() && item.size >= 0.0);
        assert!(item.distance.is_finite() && item.distance >= 0.0);
        assert!(item.stalk.is_none_or(|s| s < skeleton.stalks.len()));
    }
}

#[test]
fn every_arrangement_grows_a_well_formed_cluster() {
    for kind in [
        ClusterKind::Single,
        ClusterKind::Umbel,
        ClusterKind::Raceme,
        ClusterKind::Panicle,
        ClusterKind::Catkin,
    ] {
        for facing in [Facing::Hang, Facing::Up, Facing::Outward] {
            let mut options = arranged(kind);
            options.facing = facing;
            options.cluster.items = 7;
            options.cluster.droop = 0.5;
            grow(&options);
        }
    }
}

#[test]
fn a_hanging_single_fruit_hangs_from_its_stalk() {
    let skeleton = grow(&FruitOptions::default());
    assert_eq!(skeleton.items.len(), 1);
    assert_eq!(skeleton.stalks.len(), 1);
    let item = skeleton.items[0];
    let stalk = &skeleton.stalks[0];

    // The item points along the facing (+Y) at the stalk tip, and its path
    // distance from the host is the stalk length, not the straight line.
    assert!((item.orientation * Vec3::Y).abs_diff_eq(Vec3::Y, 1e-5));
    assert_eq!(item.origin, *stalk.points.last().unwrap());
    assert_eq!(stalk.points[0], Vec3::ZERO);
    assert_eq!(item.distance, FruitOptions::default().cluster.stalk_length);

    // The stalk leaves the host along +Z, then turns to the facing.
    let n = stalk.points.len();
    assert!((stalk.points[1] - stalk.points[0]).normalize().z > 0.5);
    let last = stalk.points[n - 1] - stalk.points[n - 2];
    assert!(last.normalize().y > 0.9);
}

#[test]
fn droop_bends_stems_toward_the_frames_ground() {
    // Down is +Y for hanging clusters, −Y for upright ones and −Z for outward ones.
    for (facing, down) in [
        (Facing::Hang, Vec3::Y),
        (Facing::Up, Vec3::NEG_Y),
        (Facing::Outward, Vec3::NEG_Z),
    ] {
        let tip = |droop: f32| {
            let mut options = arranged(ClusterKind::Raceme);
            options.facing = facing;
            options.cluster.droop = droop;
            *grow(&options).stalks[0].points.last().unwrap()
        };
        assert!(tip(1.0).dot(down) > tip(0.0).dot(down) + 0.01, "{facing:?}");
    }
}

#[test]
fn item_sizes_stay_within_the_variance() {
    let mut options = arranged(ClusterKind::Umbel);
    options.cluster.items = 20;
    options.size = 2.0;
    options.size_variance = 0.25;
    let skeleton = grow(&options);
    for item in &skeleton.items {
        assert!(
            (1.5 - 1e-5..=2.5 + 1e-5).contains(&item.size),
            "{}",
            item.size
        );
    }
}

#[test]
fn item_counts_stay_within_the_variance() {
    for kind in [
        ClusterKind::Umbel,
        ClusterKind::Raceme,
        ClusterKind::Panicle,
        ClusterKind::Catkin,
    ] {
        let mut options = arranged(kind);
        options.cluster.items = 10;
        options.cluster.items_variance = 0.3;
        let counts: Vec<usize> = (0..16)
            .map(|slot| grow_slot(&options, slot).skeleton().items.len())
            .collect();
        assert!(
            counts.iter().all(|n| (7..=13).contains(n)),
            "{kind:?}: {counts:?}"
        );
        let (min, max) = (counts.iter().min().unwrap(), counts.iter().max().unwrap());
        assert!(min < max, "{kind:?}: counts never vary");
    }
}

#[test]
fn umbel_stalks_fan_out_by_the_spread() {
    let mut options = arranged(ClusterKind::Umbel);
    options.cluster.items = 3;
    options.cluster.spread = 30.0;
    let skeleton = grow(&options);
    assert_eq!(skeleton.stalks.len(), 3);
    for (item, stalk) in skeleton.items.iter().zip(&skeleton.stalks) {
        assert_eq!(stalk.points[0], Vec3::ZERO);
        let direction = item.orientation * Vec3::Y;
        assert!((direction.angle_between(Vec3::Y).to_degrees() - 30.0).abs() < 1e-2);
    }
}

#[test]
fn raceme_items_line_the_axis_and_taper_toward_the_tip() {
    let mut options = arranged(ClusterKind::Raceme);
    options.cluster.items = 6;
    options.cluster.taper = 0.8;
    options.size_variance = 0.0;
    let skeleton = grow(&options);

    // One main stem and six item stalks, each attached to that stem.
    assert_eq!(skeleton.stalks.len(), 7);
    let axis = &skeleton.stalks[0];
    assert_eq!(axis.parent, None);
    for (k, (item, stalk)) in skeleton.items.iter().zip(&skeleton.stalks[1..]).enumerate() {
        assert_eq!(stalk.parent, Some(0));
        assert_eq!(item.stalk, Some(k + 1));
        assert!(stalk.base_distance <= options.cluster.axis_length + 1e-4);
        assert!(axis.points.windows(2).any(|w| {
            let (a, b) = (w[0], w[1]);
            let t = (stalk.points[0] - a).dot(b - a) / (b - a).length_squared();
            (0.0..=1.0).contains(&t) && a.lerp(b, t).distance(stalk.points[0]) < 1e-4
        }));
    }
    let sizes: Vec<f32> = skeleton.items.iter().map(|i| i.size).collect();
    assert!(sizes.windows(2).all(|w| w[1] < w[0]), "{sizes:?}");
}

#[test]
fn panicle_shares_items_among_side_stalks() {
    let mut options = arranged(ClusterKind::Panicle);
    options.cluster.items = 11;
    options.cluster.side_stalks = 4;
    let skeleton = grow(&options);
    assert_eq!(skeleton.items.len(), 11);
    // One main stem, four side stems, and eleven individual item stalks.
    assert_eq!(skeleton.stalks.len(), 1 + 4 + 11);
    for item in &skeleton.items {
        let stalk = &skeleton.stalks[item.stalk.unwrap()];
        assert!(item.distance >= stalk.base_distance);
    }
    // Each item hangs from a side stem, and each side stem from the main stem.
    assert_eq!(skeleton.stalks[0].parent, None);
    let sides: Vec<usize> = (1..skeleton.stalks.len())
        .filter(|&k| skeleton.stalks[k].parent == Some(0))
        .collect();
    assert_eq!(sides.len(), 4);
    for item in &skeleton.items {
        let parent = skeleton.stalks[item.stalk.unwrap()].parent.unwrap();
        assert!(sides.contains(&parent));
    }
}

#[test]
fn catkin_items_sit_on_the_axis_after_the_bare_stalk() {
    let mut options = arranged(ClusterKind::Catkin);
    options.cluster.items = 30;
    options.cluster.stalk_length = 0.5;
    options.cluster.axis_length = 2.0;
    let skeleton = grow(&options);
    assert_eq!(skeleton.stalks.len(), 1);
    for item in &skeleton.items {
        assert_eq!(item.stalk, Some(0));
        assert!((0.5..=2.5).contains(&item.distance), "{}", item.distance);
    }
}

#[test]
fn zero_stalk_length_puts_a_single_item_at_the_attachment() {
    let mut options = FruitOptions::default();
    options.cluster.stalk_length = 0.0;
    let skeleton = grow(&options);
    assert!(skeleton.stalks.is_empty());
    assert_eq!(skeleton.items[0].origin, Vec3::ZERO);
    assert_eq!(skeleton.items[0].stalk, None);
}

#[test]
fn clusters_are_repeatable_and_follow_the_slot() {
    let options = arranged(ClusterKind::Raceme);
    assert_eq!(grow_slot(&options, 3), grow_slot(&options, 3));
    assert_ne!(
        grow_slot(&options, 3).skeleton(),
        grow_slot(&options, 4).skeleton()
    );
}

/// Variation draws from a stream of its own, so turning it on changes how
/// items look, never where they are.
#[test]
fn variation_never_moves_items() {
    let plain = FruitPreset::apple().fruit.clone();
    let mut still = plain.clone();
    still.variation = Default::default();
    let (a, b) = (grow(&plain), grow(&still));
    assert_eq!(a.stalks, b.stalks);
    for (x, y) in a.items.iter().zip(&b.items) {
        assert_eq!((x.origin, x.orientation, x.size), (y.origin, y.orientation, y.size));
    }
}

/// The shape of slot `slot`'s single item in the item's own frame, with its
/// size, place and turn undone.
fn item_shape(options: &FruitOptions, slot: u32) -> Vec<Vec3> {
    let fruit = grow_slot(options, slot);
    let item = fruit.skeleton().items[0];
    let to_item = item.orientation.inverse();
    fruit
        .mesh(&ClusterDetail::default())
        .positions
        .iter()
        .map(|p| to_item * (Vec3::from_array(*p) - item.origin) / item.size)
        .collect()
}

/// Whether two item shapes match, allowing for float noise from undoing
/// the turn.
fn same_shape(a: &[Vec3], b: &[Vec3]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(p, q)| p.abs_diff_eq(*q, 1e-4))
}

/// Every preset's slots differ. For single-item clusters only the
/// variation sets them apart: without it, every slot's item is the same
/// shape, merely scaled and turned.
#[test]
fn every_presets_slots_differ() {
    for preset in FruitPreset::all() {
        let skeletons: Vec<_> = (0..6).map(|slot| grow_slot(&preset.fruit, slot)).collect();
        for (i, a) in skeletons.iter().enumerate() {
            for b in &skeletons[i + 1..] {
                assert_ne!(a.skeleton(), b.skeleton(), "{}", preset.key);
            }
        }
        if preset.fruit.cluster.kind != ClusterKind::Single {
            continue;
        }

        let shapes: Vec<_> = (0..6).map(|slot| item_shape(&preset.fruit, slot)).collect();
        for (i, a) in shapes.iter().enumerate() {
            for b in &shapes[i + 1..] {
                assert!(!same_shape(a, b), "{}: two slots have the same shape", preset.key);
            }
        }
        let mut still = preset.fruit.clone();
        still.variation = Default::default();
        assert!(
            same_shape(&item_shape(&still, 0), &item_shape(&still, 1)),
            "{}",
            preset.key
        );
    }
}
