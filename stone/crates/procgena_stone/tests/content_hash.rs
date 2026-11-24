//! Stable hashes of rock settings and the ids built from them.

use procgena_stone::{
    DetailLevel, Mesher, OptionsHash, PoolId, STONE_KIND, SpecimenId, StoneDetail, StoneOptions,
    StonePreset, TextureSize, content_hash,
};

/// Pinned hashes. A failure means every rock's pool id, and so every pooled
/// and scattered rock's shape, has changed: only accept new values for a
/// deliberate change to the hashed settings.
#[test]
fn hashes_match_pinned_values() {
    assert_eq!(STONE_KIND.salt(), 0x5635_7e1b);
    assert_eq!(
        StoneOptions::default().pool_id().options,
        OptionsHash(0x33b4_334c_d7bb_f01a)
    );
    assert_eq!(
        StonePreset::Boulder.options().pool_id().options,
        OptionsHash(0x3dfb_05a8_763c_5bec)
    );
    assert_eq!(content_hash(&StoneDetail::MEDIUM), 0xb1ab_a7f9_e6e0_ab85);
    assert_eq!(
        StoneOptions::default().variant(42, 3, 0.3).seed,
        0xd56f_fa82
    );
}

#[test]
fn the_seed_and_detail_levels_are_not_part_of_the_pool() {
    let base = StoneOptions::default();
    let mut other = base.clone();
    other.seed += 1;
    other.mesh.detail = StoneDetail::HIGH;
    assert_eq!(base.pool_id(), other.pool_id());
    assert_eq!(base.pool_id().kind, STONE_KIND);
}

#[test]
fn every_shape_and_look_setting_changes_the_pool() {
    let base = StoneOptions::default();
    let changes: Vec<(&str, fn(&mut StoneOptions))> = vec![
        ("scale", |o| o.scale *= 2.0),
        ("stretch", |o| o.stretch[1] *= 0.5),
        ("placement.settle", |o| {
            o.placement.settle = !o.placement.settle
        }),
        ("placement.max_embed_fraction", |o| {
            o.placement.max_embed_fraction *= 0.5
        }),
        ("placement.min_embed_fraction", |o| {
            o.placement.min_embed_fraction *= 0.5
        }),
        ("carve.steps", |o| o.carve.steps += 1),
        ("carve.smoothness", |o| o.carve.smoothness += 0.01),
        ("carve.base_radius", |o| o.carve.base_radius += 0.01),
        ("carve.radius_min", |o| o.carve.radius_min += 0.01),
        ("carve.radius_jitter", |o| o.carve.radius_jitter += 0.01),
        ("carve.depth", |o| o.carve.depth += 0.01),
        ("carve.blockiness", |o| o.carve.blockiness += 0.01),
        ("surface.displacement_scale", |o| {
            o.surface.displacement_scale += 0.01
        }),
        ("surface.displacement_spread", |o| {
            o.surface.displacement_spread += 0.01
        }),
        ("surface.octaves", |o| o.surface.octaves += 1),
        ("mesh.mesher", |o| o.mesh.mesher = Mesher::DualContouring),
        ("material.texture", |o| o.material.texture.push('x')),
        ("material.color", |o| o.material.color[0] *= 0.5),
        ("material.roughness", |o| o.material.roughness *= 0.5),
        ("material.metallic", |o| o.material.metallic += 0.5),
        ("material.texture_size", |o| o.material.texture_size *= 2.0),
    ];
    for (name, change) in changes {
        let mut changed = base.clone();
        change(&mut changed);
        assert_ne!(changed, base, "{name} did not change the settings");
        assert_ne!(changed.pool_id(), base.pool_id(), "{name} left the pool");
    }
}

#[test]
fn every_detail_setting_changes_the_detail_hash() {
    let base = StoneDetail::new(vec![
        DetailLevel::new(64, 0.0, TextureSize::Size1k),
        DetailLevel::new(32, 20.0, TextureSize::Size512),
    ]);
    let hash = content_hash(&base);
    let level = |l: DetailLevel| StoneDetail::new(vec![l, base.levels()[1]]);
    let first = base.levels()[0];
    for changed in [
        level(DetailLevel {
            resolution: 65,
            ..first
        }),
        level(DetailLevel {
            distance: 1.0,
            ..first
        }),
        level(DetailLevel {
            hysteresis: 0.1,
            ..first
        }),
        level(DetailLevel {
            texture_size: TextureSize::Size2k,
            ..first
        }),
        StoneDetail::new(vec![first]),
    ] {
        assert_ne!(content_hash(&changed), hash, "{changed:?}");
    }
}

#[test]
fn variants_are_specimens_of_one_pool() {
    let base = StoneOptions::default();
    let pool = PoolId::new(STONE_KIND, OptionsHash::of(&(&base, 0.0f32)));
    for slot in 0..4 {
        let variant = base.variant(9, slot, 0.0);
        assert_eq!(variant.seed, SpecimenId::new(9, pool, slot).seed());
        // Without stretch jitter only the seed differs.
        assert_eq!(
            StoneOptions {
                seed: base.seed,
                ..variant
            },
            base
        );
    }

    // The base seed plays no part; the World seed and the jitter do.
    let mut reseeded = base.clone();
    reseeded.seed += 100;
    assert_eq!(
        reseeded.variant(9, 2, 0.3).seed,
        base.variant(9, 2, 0.3).seed
    );
    assert_ne!(base.variant(10, 2, 0.3).seed, base.variant(9, 2, 0.3).seed);
    assert_ne!(base.variant(9, 2, 0.2).seed, base.variant(9, 2, 0.3).seed);
}
