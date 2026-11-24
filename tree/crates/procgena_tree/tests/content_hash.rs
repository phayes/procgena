//! Checks that tree content hashes and keys are repeatable and reflect
//! changes to settings. Covers presets, saved settings, the seed, and the
//! relationship between a tree and its options.

use std::collections::HashSet;

use procgena_core::content_hash;
use procgena_tree::{
    ClusterKind, Facing, GrowthForce, TREE_KIND, Tree, TreeOptions, TreePreset, TreeType,
    TrellisForce,
};

#[test]
fn equal_options_share_a_hash() {
    for preset in TreePreset::all() {
        let a = preset.tree.clone();
        let b = preset.tree.clone();
        assert_eq!(a.options_hash(), b.options_hash(), "{}", preset.name);
        assert_eq!(a.pool_id(), b.pool_id(), "{}", preset.name);
        assert_eq!(a.cache_key(), b.cache_key(), "{}", preset.name);
    }
}

#[test]
fn presets_have_distinct_pools() {
    let pools: HashSet<_> = TreePreset::all().map(|p| p.tree.pool_id()).collect();
    assert_eq!(pools.len(), TreePreset::all().count());
}

#[test]
fn pools_are_tree_pools() {
    assert_eq!(TreeOptions::default().pool_id().kind, TREE_KIND);
}

#[test]
fn cache_key_matches_its_options() {
    let tree = Tree::from_preset(TreePreset::ash_small()).unwrap();
    assert_eq!(tree.cache_key(), tree.options().cache_key());
}

#[test]
fn yaml_round_trip_keeps_the_hash() {
    for preset in TreePreset::all() {
        let options = &preset.tree;
        let restored = TreePreset::from_yaml(&preset.key, &preset.to_yaml(None))
            .unwrap()
            .tree;
        assert_eq!(options.cache_key(), restored.cache_key(), "{}", preset.name);
    }
}

#[test]
fn every_kind_of_setting_changes_the_hash() {
    let base = TreeOptions::default();
    type Edit = (&'static str, fn(&mut TreeOptions));
    let edits: Vec<Edit> = vec![
        ("tree_type", |o| o.tree_type = TreeType::Evergreen),
        ("bark.kind", |o| o.bark.kind.push('x')),
        ("bark.tint", |o| o.bark.tint ^= 1),
        ("bark.texture_scale", |o| o.bark.texture_scale.y += 1.0),
        ("branch.levels", |o| o.branch.levels -= 1),
        ("branch.length[3]", |o| o.branch.length[3] += 0.5),
        ("branch.sections[2]", |o| o.branch.sections[2] += 1),
        ("branch.force.direction", |o| {
            o.branch.force.direction.x = 0.5
        }),
        ("leaves.count", |o| o.leaves.count += 1),
        ("leaves.rounded_normals", |o| {
            o.leaves.rounded_normals ^= true
        }),
        ("leaves.flatten", |o| o.leaves.flatten = 0.5),
        ("trellis.position", |o| o.trellis.position.z += 1.0),
        ("trellis.force.falloff", |o| o.trellis.force.falloff += 1.0),
        ("fruit.placement.facing", |o| {
            o.fruit.placement.facing = Facing::Up
        }),
        ("fruit.cluster.kind", |o| {
            o.fruit.cluster.kind = ClusterKind::Umbel
        }),
        ("fruit.shape.calyx", |o| o.fruit.shape.calyx ^= true),
        ("fruit.maturity", |o| o.fruit.maturity = 0.5),
        ("flowers.shape.fade_color", |o| {
            o.flowers.shape.fade_color ^= 1
        }),
        ("flowers.maturity", |o| o.flowers.maturity = 0.75),
    ];

    let mut hashes = HashSet::from([base.options_hash()]);
    for (name, edit) in edits {
        let mut options = base.clone();
        edit(&mut options);
        assert!(
            hashes.insert(options.options_hash()),
            "changing {name} did not give a new hash"
        );
    }
}

/// The seed picks a tree from its pool: it changes the cache key and the
/// specimen, never the options hash or the pool.
#[test]
fn the_seed_changes_the_key_but_not_the_pool() {
    let a = TreeOptions::default();
    let mut b = a.clone();
    b.default_seed += 1;
    assert_eq!(a.options_hash(), b.options_hash());
    assert_eq!(a.pool_id(), b.pool_id());
    assert_ne!(a.cache_key(), b.cache_key());
    assert_ne!(
        a.specimen_id(a.default_seed as u32, 0).seed(),
        b.specimen_id(b.default_seed as u32, 0).seed()
    );
}

/// A negative seed is a seed of its own, not the same as its magnitude.
#[test]
fn negative_seeds_have_their_own_key() {
    let a = TreeOptions {
        default_seed: 5,
        ..TreeOptions::default()
    };
    let mut b = a.clone();
    b.default_seed = -5;
    assert_ne!(a.cache_key(), b.cache_key());
    assert_ne!(a.cache_key().value(), b.cache_key().value());
}

#[test]
fn swapping_values_between_fields_changes_the_hash() {
    let mut a = TreeOptions::default();
    a.leaves.size = 1.0;
    a.leaves.size_variance = 2.0;
    let mut b = a.clone();
    b.leaves.size = 2.0;
    b.leaves.size_variance = 1.0;
    assert_ne!(a.options_hash(), b.options_hash());
}

#[test]
fn string_boundaries_are_part_of_the_hash() {
    let mut a = TreeOptions::default();
    a.bark.kind = "ab".into();
    a.leaves.kind = "c".into();
    let mut b = a.clone();
    b.bark.kind = "a".into();
    b.leaves.kind = "bc".into();
    assert_ne!(a.options_hash(), b.options_hash());
}

/// `0.0` and `-0.0` compare equal, so equal settings hash the same.
#[test]
fn negative_zero_hashes_like_zero() {
    let mut a = TreeOptions::default();
    a.branch.twist[0] = 0.0;
    let mut b = a.clone();
    b.branch.twist[0] = -0.0;
    assert_eq!(a, b);
    assert_eq!(a.options_hash(), b.options_hash());
}

/// Hashes of small settings, worked out independently from the encoding
/// rules of `procgena_core::ContentHasher` (one word per value, floats by
/// their bits, `-0.0` as `0.0`), with a Python copy of `mix64`.
#[test]
fn hashes_match_reference_values() {
    let force = GrowthForce {
        direction: glam::Vec3::new(1.0, 2.0, 3.0),
        strength: 0.5,
    };
    assert_eq!(content_hash(&force), 10_202_923_523_112_364_632);

    let trellis = TrellisForce {
        strength: 1.5,
        max_distance: -0.0,
        falloff: 2.0,
    };
    assert_eq!(content_hash(&trellis), 7_827_661_828_296_368_942);
}

/// The default settings keep their known hash and seed. A failure means
/// changed defaults or a changed encoding: review the cause before
/// updating the expected values, since every cached tree and baked
/// impostor is keyed by them.
#[test]
fn default_hashes_are_pinned() {
    let options = TreeOptions::default();
    assert_eq!(options.options_hash().0, 5_340_872_859_848_595_951);
    assert_eq!(options.specimen_id(0, 0).seed(), 1_712_387_412);
}
