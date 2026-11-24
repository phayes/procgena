//! Options hashes and specimen ids.

use procgena_core::{OptionsHash, SpecimenId};
use procgena_mushroom::{
    CapFamily, ClusterLayout, GENERATOR_KIND, Mushroom, MushroomDetail, MushroomOptions,
    MushroomPreset, Pattern,
};

/// The options hash of `options`, as a bare number.
fn hash(options: &MushroomOptions) -> u64 {
    OptionsHash::of(options).0
}

/// Pinned hashes: a change here means every saved pool and every specimen
/// grown from these options changes. Update them only on purpose.
#[test]
fn hashes_are_pinned() {
    // FNV-1a of "mushroom.generator.kind", checked by hand.
    assert_eq!(GENERATOR_KIND.salt(), 0x93d1_50bc);
    assert_eq!(hash(&MushroomOptions::default()), 0x51e3_65fa_2708_f856);
    // In the order of `MushroomPreset::ALL`.
    let pinned: [u64; 11] = [
        0x1d5c_8771_b967_fedb, // Fly agaric
        0xce23_c528_4586_9a83, // Porcini
        0x23a6_32c7_92e1_f89c, // Chanterelle
        0x72d0_f6d4_0cad_b3a2, // Parasol
        0xf296_b0e4_6873_f8b2, // Liberty cap
        0x8da3_4b63_3b1b_8c81, // Shaggy ink cap
        0x0d1e_f283_db1b_0b4d, // Button
        0x6707_f412_965b_ab89, // Enoki
        0x03da_5782_a5ac_3951, // Puffball
        0x5d5d_dfd0_6eb9_efc6, // Storybook
        0x31de_f8e5_14c8_df28, // Fairy ring
    ];
    for (preset, value) in MushroomPreset::ALL.into_iter().zip(pinned) {
        assert_eq!(hash(&preset.options()), value, "{}", preset.name());
    }
    assert_eq!(
        MushroomOptions::default().specimen_id(0).seed(),
        0xd362_d2a1
    );
}

#[test]
fn every_preset_has_its_own_pool() {
    for (i, a) in MushroomPreset::ALL.iter().enumerate() {
        for b in &MushroomPreset::ALL[i + 1..] {
            assert_ne!(a.options().pool(), b.options().pool(), "{a:?} {b:?}");
        }
    }
}

#[test]
fn seed_and_detail_are_left_out() {
    let base = MushroomOptions::default();
    let mut other = base.clone();
    other.seed = 99;
    other.detail = MushroomDetail::FLAT;
    assert_eq!(base.pool(), other.pool());
    assert_ne!(base.specimen_id(0), other.specimen_id(0));
}

#[test]
fn every_field_changes_the_hash() {
    type Edit = fn(&mut MushroomOptions);
    let edits: &[(&str, Edit)] = &[
        ("size", |o| o.size *= 2.0),
        ("cap.family", |o| o.cap.family = CapFamily::Conical),
        ("cap.height", |o| o.cap.height += 0.1),
        ("cap.lobes", |o| o.cap.lobes += 1),
        ("cap.lobe_depth", |o| o.cap.lobe_depth += 0.1),
        ("cap.asymmetry", |o| o.cap.asymmetry += 0.1),
        ("cap.tilt", |o| o.cap.tilt += 1.0),
        ("cap.roughness", |o| o.cap.roughness += 0.01),
        ("cap.roughness_scale", |o| o.cap.roughness_scale += 1.0),
        ("underside.rim_thickness", |o| {
            o.underside.rim_thickness += 0.01
        }),
        ("underside.sag", |o| o.underside.sag += 0.01),
        ("underside.decurrent", |o| o.underside.decurrent += 0.1),
        ("stem.height", |o| o.stem.height += 0.1),
        ("stem.radius", |o| o.stem.radius += 0.01),
        ("stem.base", |o| o.stem.base += 0.1),
        ("stem.collar", |o| o.stem.collar += 0.1),
        ("stem.lean", |o| o.stem.lean += 0.1),
        ("stem.curve", |o| o.stem.curve += 0.1),
        ("stem.upturn", |o| o.stem.upturn += 0.1),
        ("stem.lean_direction", |o| o.stem.lean_direction += 10.0),
        ("stem.roughness", |o| o.stem.roughness += 0.01),
        ("bulb.closed", |o| o.bulb.closed = !o.bulb.closed),
        ("bulb.size", |o| o.bulb.size += 0.1),
        ("bulb.height", |o| o.bulb.height += 0.1),
        ("bulb.depth", |o| o.bulb.depth += 0.1),
        ("annulus.enabled", |o| {
            o.annulus.enabled = !o.annulus.enabled
        }),
        ("annulus.position", |o| o.annulus.position -= 0.1),
        ("annulus.length", |o| o.annulus.length += 0.01),
        ("annulus.flare", |o| o.annulus.flare += 0.1),
        ("annulus.raggedness", |o| o.annulus.raggedness += 0.1),
        ("warts.count", |o| o.warts.count += 1),
        ("warts.size", |o| o.warts.size += 0.01),
        ("warts.size_jitter", |o| o.warts.size_jitter += 0.1),
        ("warts.height", |o| o.warts.height += 0.1),
        ("warts.coverage", |o| o.warts.coverage -= 0.1),
        ("material.cap_centre", |o| {
            o.material.cap_centre = "#000001".into()
        }),
        ("material.cap_rim", |o| {
            o.material.cap_rim = "#000001".into()
        }),
        ("material.underside", |o| {
            o.material.underside = "#000001".into()
        }),
        ("material.stem_top", |o| {
            o.material.stem_top = "#000001".into()
        }),
        ("material.stem_base", |o| {
            o.material.stem_base = "#000001".into()
        }),
        ("material.annulus", |o| {
            o.material.annulus = "#000001".into()
        }),
        ("material.wart", |o| o.material.wart = "#000001".into()),
        ("material.gills", |o| o.material.gills = !o.material.gills),
        ("material.gill_lines", |o| o.material.gill_lines += 1),
        ("material.gill_strength", |o| {
            o.material.gill_strength += 0.1
        }),
        ("material.pattern", |o| o.material.pattern = Pattern::Spots),
        ("material.pattern_colour", |o| {
            o.material.pattern_colour = "#000001".into()
        }),
        ("material.pattern_scale", |o| {
            o.material.pattern_scale += 1.0
        }),
        ("material.pattern_strength", |o| {
            o.material.pattern_strength += 0.1
        }),
        ("material.roughness", |o| o.material.roughness -= 0.1),
        ("material.cap_texture", |o| {
            o.material.cap_texture = Some("x".into())
        }),
        ("material.cap_texture_size", |o| {
            o.material.cap_texture_size += 0.01
        }),
        ("material.stem_texture", |o| {
            o.material.stem_texture = Some("x".into())
        }),
        ("material.stem_texture_size", |o| {
            o.material.stem_texture_size += 0.01
        }),
        ("cluster.stems", |o| o.cluster.stems += 1),
        ("cluster.layout", |o| {
            o.cluster.layout = ClusterLayout::FairyRing
        }),
        ("cluster.size_variation", |o| {
            o.cluster.size_variation -= 0.1
        }),
        ("cluster.lean", |o| o.cluster.lean += 1.0),
        ("cluster.sweep", |o| o.cluster.sweep += 0.1),
        ("cluster.spacing", |o| o.cluster.spacing += 0.1),
        ("cluster.variants", |o| o.cluster.variants += 1),
        ("cluster.fairy_ring.radius", |o| {
            o.cluster.fairy_ring.radius += 1.0
        }),
        ("cluster.fairy_ring.width", |o| {
            o.cluster.fairy_ring.width += 0.01
        }),
        ("cluster.fairy_ring.arc", |o| {
            o.cluster.fairy_ring.arc -= 0.1
        }),
    ];
    let base = hash(&MushroomOptions::default());
    let mut seen = vec![base];
    for (name, edit) in edits {
        let mut options = MushroomOptions::default();
        edit(&mut options);
        let changed = hash(&options);
        assert!(!seen.contains(&changed), "{name} did not change the hash");
        seen.push(changed);
    }
}

#[test]
fn specimens_of_one_pool_share_nothing_but_the_pool() {
    let options = MushroomPreset::Enoki.options();
    assert!(options.cluster.stems > 1);
    let mushroom = Mushroom::new(options.clone()).unwrap();
    assert_eq!(mushroom.id(), options.specimen_id(0));
    assert_eq!(
        mushroom.id(),
        SpecimenId::new(options.seed, options.pool(), 0)
    );
    for (i, body) in mushroom.bodies().iter().enumerate() {
        assert_eq!(body.specimen(), mushroom.id());
        assert_eq!(body.index(), i as u32);
    }
    // Equal options give an identical mushroom.
    let again = Mushroom::new(options).unwrap();
    assert_eq!(again.mesh().surfaces, mushroom.mesh().surfaces);
}
