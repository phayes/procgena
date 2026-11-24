//! Check where a tree places its fruit and flower clusters, how it turns
//! and scales them, and which slot specimen each shows. The clusters' own
//! shapes are tested in `procgena_fruit`.

use glam::Vec3;
use procgena_tree::procgena_fruit::{ClusterKind, Facing, FruitOptions, FruitPreset};
use procgena_tree::{
    AttachedFruit, ClusterPart, ClusterPlacement, Skeleton, SkeletonCluster, TreeOptions,
    TreePreset, TreeType,
};

/// Fruit clusters with `fruit` settings, two per branch at level 2 or deeper.
fn attached(fruit: FruitOptions) -> AttachedFruit {
    AttachedFruit::new(
        fruit,
        ClusterPlacement {
            count: 2,
            min_level: 2,
            ..ClusterPlacement::default()
        },
    )
}

/// A small broadleaf tree with two default fruit clusters on each branch at
/// level 2 or deeper.
fn fruiting() -> TreeOptions {
    let mut options = TreePreset::ash_small().tree.clone();
    options.fruit = Some(attached(FruitOptions::default()));
    options
}

/// The tree's fruit settings, which must be present.
fn fruit(options: &mut TreeOptions) -> &mut AttachedFruit {
    options.fruit.as_mut().expect("the tree grows fruit")
}

/// The tree's flower settings, added with defaults if absent.
fn flowers(options: &mut TreeOptions) -> &mut AttachedFruit {
    options.flowers.get_or_insert_with(|| AttachedFruit {
        placement: ClusterPlacement {
            count: 0,
            ..ClusterPlacement::default()
        },
        ..AttachedFruit::new(
            FruitPreset::dogwood_bloom().fruit.clone(),
            Default::default(),
        )
    })
}

/// Generates a skeleton and requires at least one fruit or flower cluster.
#[track_caller]
fn grow(options: &TreeOptions) -> Skeleton {
    let skeleton = Skeleton::generate(options).unwrap();
    assert!(
        !skeleton.fruit.is_empty() || !skeleton.flowers.is_empty(),
        "no clusters grew"
    );
    assert_well_formed(&skeleton, ClusterPart::Fruit);
    assert_well_formed(&skeleton, ClusterPart::Flowers);
    skeleton
}

/// The cluster's attachment position as a fraction of its parent branch's
/// section count: 0 at the base and 1 at the tip.
fn along(skeleton: &Skeleton, cluster: &SkeletonCluster) -> f32 {
    let parent = cluster.parent;
    let sections = skeleton.branches[parent.branch].sections.len() - 1;
    (parent.section as f32 + parent.t) / sections as f32
}

/// Checks finite positions, unit rotations, positive scales and slots that
/// name a grown specimen.
#[track_caller]
fn assert_well_formed(skeleton: &Skeleton, part: ClusterPart) {
    let (clusters, slots) = skeleton.clusters(part);
    for cluster in clusters {
        assert!(cluster.origin.is_finite() && cluster.orientation.is_normalized());
        assert!(cluster.scale.is_finite() && cluster.scale > 0.0);
        assert!((cluster.slot as usize) < slots.len());
    }
}

#[test]
fn ez_tree_presets_grow_no_fruit_or_flowers() {
    for preset in TreePreset::ez_tree() {
        let skeleton = Skeleton::generate(&preset.tree).unwrap();
        assert!(skeleton.fruit.is_empty(), "{}", preset.name);
        assert!(skeleton.flowers.is_empty(), "{}", preset.name);
        assert!(skeleton.fruit_slots.is_empty() && skeleton.flower_slots.is_empty());
    }
}

#[test]
fn fruit_and_flowers_leave_branches_and_leaves_unchanged() {
    let plain = Skeleton::generate(&TreePreset::oak_medium().tree).unwrap();

    let mut options = TreePreset::oak_medium().tree.clone();
    options.fruit = Some(attached(FruitOptions::default()));
    fruit(&mut options).placement.count = 3;
    flowers(&mut options).placement.tips = 1.0;
    let decorated = Skeleton::generate(&options).unwrap();

    assert!(!decorated.fruit.is_empty() && !decorated.flowers.is_empty());
    assert_eq!(decorated.branches, plain.branches);
    assert_eq!(decorated.leaves, plain.leaves);
}

#[test]
fn flowers_do_not_move_fruit() {
    let options = fruiting();
    let mut with_flowers = options.clone();
    let added = flowers(&mut with_flowers);
    added.placement.count = 4;
    added.fruit.cluster.kind = ClusterKind::Raceme;
    added.fruit.cluster.items = 8;

    let a = Skeleton::generate(&options).unwrap();
    let b = Skeleton::generate(&with_flowers).unwrap();
    assert!(a.flowers.is_empty() && !b.flowers.is_empty());
    assert_eq!(a.fruit, b.fruit);
    assert_eq!(a.fruit_slots, b.fruit_slots);
}

#[test]
fn clusters_are_repeatable_and_follow_the_seed() {
    let options = fruiting();
    let a = Skeleton::generate(&options).unwrap();
    assert_eq!(a, Skeleton::generate(&options).unwrap());

    let mut reseeded = options.clone();
    reseeded.default_seed += 1;
    assert_ne!(a.fruit, Skeleton::generate(&reseeded).unwrap().fruit);
}

#[test]
fn side_clusters_respect_min_level_start_and_count() {
    let mut options = fruiting();
    fruit(&mut options).placement.start = 0.5;
    let skeleton = grow(&options);

    // Each eligible branch should carry the two configured side clusters.
    let eligible = skeleton.branches.iter().filter(|b| b.level >= 2).count();
    assert_eq!(skeleton.fruit.len(), eligible * 2);
    for cluster in &skeleton.fruit {
        assert!(skeleton.branches[cluster.parent.branch].level >= 2);
        assert!(along(&skeleton, cluster) >= 0.5 - 1e-5);
    }
}

#[test]
fn side_clusters_start_on_the_branch_surface() {
    let skeleton = grow(&fruiting());
    for cluster in &skeleton.fruit {
        // The cluster should begin on the surface, one radius from the branch centre.
        let branch = &skeleton.branches[cluster.parent.branch];
        let p = cluster.parent;
        let a = &branch.sections[p.section];
        let b = branch.sections.get(p.section + 1).unwrap_or(a);
        let centre = a.origin.lerp(b.origin, p.t);
        let radius = a.radius + (b.radius - a.radius) * p.t;
        let gap = cluster.origin.distance(centre);
        assert!(
            (gap - radius).abs() < 1e-4 * radius.max(0.1),
            "{gap} vs {radius}"
        );

        // A hanging cluster's +Z points away from the branch.
        let out = (cluster.origin - centre).normalize();
        assert!((cluster.orientation * Vec3::Z).dot(out) > 0.0);
    }
}

#[test]
fn tips_grow_only_on_branch_ends_without_a_continuation() {
    // Only the deepest branches of a deciduous tree have tips without continuations.
    let mut options = TreePreset::ash_small().tree.clone();
    let added = flowers(&mut options);
    added.placement.tips = 1.0;
    added.placement.min_level = 0;
    let skeleton = grow(&options);
    let levels = options.branch.levels;
    let outer = skeleton
        .branches
        .iter()
        .filter(|b| b.level == levels)
        .count();
    assert_eq!(skeleton.flowers.len(), outer);
    for cluster in &skeleton.flowers {
        let branch = &skeleton.branches[cluster.parent.branch];
        assert_eq!(branch.level, levels);
        assert_eq!(cluster.parent.section, branch.sections.len() - 1);
        assert_eq!(cluster.origin, branch.sections.last().unwrap().origin);
    }

    // Evergreen branches have no tip continuations, so every eligible level can flower.
    let mut options = TreePreset::pine_small().tree.clone();
    assert_eq!(options.tree_type, TreeType::Evergreen);
    let added = flowers(&mut options);
    added.placement.tips = 1.0;
    added.placement.min_level = 1;
    let skeleton = grow(&options);
    let eligible = skeleton.branches.iter().filter(|b| b.level >= 1).count();
    assert_eq!(skeleton.flowers.len(), eligible);
}

#[test]
fn tip_chance_thins_out_tip_clusters() {
    let mut options = TreePreset::oak_medium().tree.clone();
    flowers(&mut options).placement.tips = 0.3;
    let skeleton = grow(&options);
    let levels = options.branch.levels;
    let outer = skeleton
        .branches
        .iter()
        .filter(|b| b.level == levels)
        .count();
    let share = skeleton.flowers.len() as f32 / outer as f32;
    assert!((0.2..0.4).contains(&share), "{share} of {outer} tips");
}

#[test]
fn hanging_clusters_face_down_and_upright_ones_up() {
    for (facing, expected) in [(Facing::Hang, Vec3::NEG_Y), (Facing::Up, Vec3::Y)] {
        let mut options = fruiting();
        let added = fruit(&mut options);
        added.fruit.facing = facing;
        added.placement.facing_jitter = 0.0;
        let skeleton = grow(&options);
        for cluster in &skeleton.fruit {
            assert!((cluster.orientation * Vec3::Y).abs_diff_eq(expected, 1e-5));
        }
    }
}

#[test]
fn a_hanging_apple_hangs_below_its_branch() {
    let mut options = fruiting();
    let added = fruit(&mut options);
    added.fruit = FruitPreset::apple().fruit.clone();
    added.placement.facing_jitter = 0.0;
    let skeleton = grow(&options);
    for cluster in &skeleton.fruit {
        let specimen = &skeleton.fruit_slots[cluster.slot as usize];
        let item = specimen.skeleton().items[0];
        let tip = cluster.to_tree(item.origin + item.orientation * Vec3::Y * item.size);
        assert!(tip.y < cluster.origin.y, "{tip} above {}", cluster.origin);
    }
}

#[test]
fn facing_jitter_limits_the_tilt() {
    let mut options = fruiting();
    let added = fruit(&mut options);
    added.fruit.facing = Facing::Up;
    added.placement.facing_jitter = 20.0;
    let skeleton = grow(&options);
    let mut largest: f32 = 0.0;
    for cluster in &skeleton.fruit {
        let facing = cluster.orientation * Vec3::Y;
        largest = largest.max(facing.angle_between(Vec3::Y).to_degrees());
    }
    assert!(largest <= 20.0 + 1e-3, "{largest}");
    assert!(largest > 5.0, "{largest}");
}

#[test]
fn cluster_scales_stay_within_the_variance() {
    let mut options = fruiting();
    let added = fruit(&mut options);
    added.placement.scale = 2.0;
    added.placement.scale_variance = 0.25;
    let skeleton = grow(&options);
    let scales: Vec<f32> = skeleton.fruit.iter().map(|c| c.scale).collect();
    assert!(
        scales.iter().all(|s| (1.5 - 1e-5..=2.5 + 1e-5).contains(s)),
        "{scales:?}"
    );
    assert!(scales.windows(2).any(|w| w[0] != w[1]), "scales never vary");
}

/// Density adds side clusters in proportion to branch length, so longer
/// branches carry more clusters than shorter ones, and zero density adds none.
#[test]
fn density_scales_side_clusters_with_branch_length() {
    let mut options = fruiting();
    let added = fruit(&mut options);
    added.placement.count = 0;
    added.placement.start = 0.0;
    added.placement.density = 20.0;
    let min_level = added.placement.min_level;
    let skeleton = grow(&options);

    // Compare the cluster count on each branch with its measured length.
    let mut counts = vec![0u32; skeleton.branches.len()];
    for cluster in &skeleton.fruit {
        counts[cluster.parent.branch] += 1;
    }
    for (branch, count) in skeleton.branches.iter().zip(counts) {
        if branch.level < min_level {
            assert_eq!(count, 0);
            continue;
        }
        let length: f32 = branch
            .sections
            .windows(2)
            .map(|pair| pair[0].origin.distance(pair[1].origin))
            .sum();
        let expected = 20.0 * length;
        assert!(
            (count as f32 - expected).abs() <= 1.0,
            "{count} clusters on a branch {length} long"
        );
    }

    // Zero density with zero count and tips disables clusters entirely.
    fruit(&mut options).placement.density = 0.0;
    assert!(Skeleton::generate(&options).unwrap().fruit.is_empty());
}

/// Rejects a negative or non-finite cluster density.
#[test]
fn density_must_be_finite_and_non_negative() {
    for bad in [-1.0, f32::NAN, f32::INFINITY] {
        let mut options = fruiting();
        flowers(&mut options).placement.density = bad;
        assert!(options.validate().is_err(), "{bad} accepted");
    }
}

#[test]
fn slots_name_grown_specimens_of_one_pool() {
    let mut options = fruiting();
    fruit(&mut options).default_slots = 4;
    let skeleton = grow(&options);
    assert_eq!(skeleton.fruit_slots.len(), 4);
    let pool = options.fruit.as_ref().unwrap().fruit.pool_id();
    for (slot, specimen) in (0u32..).zip(&skeleton.fruit_slots) {
        assert_eq!(specimen.id().pool, pool);
        assert_eq!(specimen.id().slot, slot);
    }

    // Many clusters spread over every slot.
    let mut used = [false; 4];
    for cluster in &skeleton.fruit {
        used[cluster.slot as usize] = true;
    }
    assert_eq!(used, [true; 4], "{} clusters", skeleton.fruit.len());
}

#[test]
fn shrinking_the_pool_keeps_the_clusters_whose_slot_remains() {
    let mut options = fruiting();
    fruit(&mut options).default_slots = 6;
    let wide = grow(&options);
    fruit(&mut options).default_slots = 3;
    let narrow = grow(&options);
    for (a, b) in wide.fruit.iter().zip(&narrow.fruit) {
        assert_eq!(a.origin, b.origin);
        if a.slot < 3 {
            assert_eq!(a.slot, b.slot);
        }
    }
}

#[test]
fn the_cluster_table_is_sorted_by_slot_and_keeps_each_index() {
    let skeleton = grow(&fruiting());
    let table = skeleton.cluster_table(ClusterPart::Fruit);
    assert_eq!(table.len(), skeleton.fruit.len());
    assert!(
        table
            .windows(2)
            .all(|w| (w[0].slot, w[0].index) < (w[1].slot, w[1].index))
    );
    for row in &table {
        let cluster = &skeleton.fruit[row.index as usize];
        assert_eq!(row.slot, cluster.slot);
        assert_eq!(row.origin, cluster.origin);
        assert!(row.radius > 0.0);
        assert!(row.wind[1] >= 0.0, "branch weight {:?}", row.wind);
    }
}

#[test]
fn every_copy_of_a_tree_specimen_carries_the_same_fruit() {
    let options = fruiting();
    let a = Skeleton::generate(&options).unwrap();
    let b = Skeleton::generate(&options).unwrap();
    let slots = |s: &Skeleton| s.fruit.iter().map(|c| c.slot).collect::<Vec<_>>();
    assert_eq!(slots(&a), slots(&b));
    assert_eq!(a.fruit_slots, b.fruit_slots);
}

/// A part path `/ fruit i / item k` resolves through the cluster's specimen,
/// and names the same cluster whatever detail level draws it.
#[test]
fn a_part_path_resolves_to_a_fruit_item_on_every_level() {
    use procgena_tree::procgena_fruit::FruitPart;
    use procgena_tree::{FRUIT_PART, default_lod};

    let options = TreePreset::spring_pussy_willow().tree.clone();
    let skeleton = Skeleton::generate(&options).unwrap();
    let tree = options.specimen_id(options.default_seed as u32, 0);
    let step = skeleton.cluster_step(ClusterPart::Fruit, 17, tree);
    assert_eq!(step.kind, procgena_core::PartStep::kind_of(FRUIT_PART));

    let link = skeleton.part(step).unwrap();
    assert_eq!(link.part, ClusterPart::Fruit);
    assert_eq!(link.cluster, &skeleton.fruit[17]);
    let item = link.specimen.item_step(2);
    let Some(FruitPart::Item(found)) = link.specimen.part(item) else {
        panic!("item 2 doesn't resolve");
    };
    assert_eq!(*found, link.specimen.skeleton().items[2]);

    // A step pinned to another fruit specimen is set aside.
    let other = skeleton
        .fruit_slots
        .iter()
        .find(|f| f.id() != link.specimen.id())
        .unwrap();
    assert_eq!(other.part(item), None);
    assert!(
        skeleton
            .part(skeleton.cluster_step(ClusterPart::Fruit, 1 << 30, tree))
            .is_none()
    );

    // Every level that keeps cluster 17 draws it from that same cluster.
    for level in default_lod().levels {
        let mesh = skeleton.mesh(&options, &level.detail);
        for span in &mesh.fruit_spans {
            assert!(
                skeleton
                    .part(skeleton.cluster_step(ClusterPart::Fruit, span.cluster as u32, tree))
                    .is_some()
            );
        }
    }
}
