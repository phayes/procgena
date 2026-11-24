//! Stable hashes of clump settings.

use procgena_clump::{CLUMP_KIND, ClumpOptions, ClumpPreset, Direction, Placement};
use procgena_core::OptionsHash;
use procgena_specimen::tags;

/// Pinned hashes. A failure means every clump's pool id has changed: only
/// accept new values for a deliberate change to the hashed settings.
#[test]
fn hashes_match_pinned_values() {
    assert_eq!(CLUMP_KIND.salt(), 0x6cc1_f111);
    assert_eq!(
        ClumpOptions::default().pool_id().options,
        OptionsHash(0x950b_9b8a_fbfd_afcd)
    );
    assert_eq!(
        ClumpPreset::moss_cushions().clump.pool_id().options,
        OptionsHash(0x1bea_8826_9ac8_39f9)
    );
}

#[test]
fn the_seed_is_not_part_of_the_pool() {
    let base = ClumpOptions::default();
    let reseeded = ClumpOptions {
        default_seed: base.default_seed + 1,
        ..base.clone()
    };
    assert_eq!(base.pool_id(), reseeded.pool_id());
    assert_eq!(base.pool_id().kind, CLUMP_KIND);
}

/// A setting's name, and an edit that changes it.
type Change = (&'static str, fn(&mut ClumpOptions));

#[test]
fn every_other_setting_changes_the_pool() {
    let base = ClumpOptions::default();
    let changes: Vec<Change> = vec![
        ("directions", |o| {
            o.directions.push(Direction::new([1.0, 0.0, 0.0]))
        }),
        ("directions.vector", |o| o.directions[0].vector[0] = 0.5),
        ("directions.strength", |o| o.directions[0].strength = 0.5),
        ("scale", |o| o.scale = 2.0),
        ("mask.any_facing", |o| {
            o.mask.any_facing = !o.mask.any_facing
        }),
        ("mask.facing_min", |o| o.mask.facing_min += 0.01),
        ("mask.facing_full", |o| o.mask.facing_full += 0.01),
        ("mask.exposure_rays", |o| o.mask.exposure_rays += 1),
        ("mask.exposure_power", |o| o.mask.exposure_power += 0.1),
        ("placement.mode", |o| o.placement.mode = Placement::Scatter),
        ("placement.count", |o| o.placement.count += 1),
        ("placement.radius", |o| o.placement.radius += 0.01),
        ("placement.size_variation", |o| {
            o.placement.size_variation += 0.01
        }),
        ("placement.min_spacing", |o| o.placement.min_spacing += 0.01),
        ("shape.thickness", |o| o.shape.thickness += 0.01),
        ("shape.sink", |o| o.shape.sink += 0.01),
        ("shape.max_slope", |o| o.shape.max_slope += 0.01),
        ("shape.min_support", |o| o.shape.min_support += 0.01),
        ("shape.lumpiness", |o| o.shape.lumpiness += 0.01),
        ("shape.lump_size", |o| o.shape.lump_size += 0.01),
        ("shape.neck", |o| o.shape.neck += 0.01),
        ("mesh.edge_length", |o| o.mesh.edge_length += 0.01),
        ("mesh.closed", |o| o.mesh.closed = !o.mesh.closed),
        ("mesh.strict", |o| o.mesh.strict = !o.mesh.strict),
        ("grows_on.any_of", |o| {
            o.grows_on.any_of.pop().map(drop).unwrap()
        }),
        ("grows_on.none_of", |o| o.grows_on.none_of.clear()),
        ("tags", |o| o.tags.insert(tags::MOSS)),
        ("material.texture", |o| {
            o.material.texture = Some("moss_004".into())
        }),
        ("material.color", |o| o.material.color[2] = 0.5),
        ("material.roughness", |o| o.material.roughness = 0.5),
        ("material.texture_size", |o| o.material.texture_size = 2.0),
    ];
    for (name, change) in changes {
        let mut changed = base.clone();
        change(&mut changed);
        assert_ne!(changed, base, "{name} did not change the settings");
        assert_ne!(changed.pool_id(), base.pool_id(), "{name} left the pool");
    }
}

#[test]
fn a_tag_moved_from_wanted_to_excluded_changes_the_pool() {
    let base = ClumpOptions::default();
    let mut moved = base.clone();
    let tag = moved.grows_on.any_of.pop().unwrap();
    moved.grows_on.none_of.insert(0, tag);
    assert_ne!(moved.pool_id(), base.pool_id());
}
