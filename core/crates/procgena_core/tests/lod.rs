//! Fade bands: dither levels across a step's bands, cross-fades between
//! neighbouring steps, packing for the GPU, and the quality tiers.

use procgena_core::{FarForm, LodRange, Quality};

/// A middle step faded in over 8–12 m and out over 18–22 m, as in tree's
/// grove test.
fn middle() -> LodRange {
    LodRange {
        fade_in: 8.0..12.0,
        fade_out: 18.0..22.0,
    }
}

#[test]
fn quality_tiers_increase_and_high_is_the_reference() {
    let scales = Quality::ALL.map(Quality::distance_scale);
    assert!(scales.windows(2).all(|w| w[0] < w[1]), "{scales:?}");
    assert_eq!(Quality::default(), Quality::High);
    assert_eq!(Quality::High.distance_scale(), 1.0);
}

#[test]
fn dither_levels_match_the_grove_and_pool_renderers() {
    let range = middle();
    let cases = [
        (7.9, -16),
        (8.0, -16),
        (10.0, -8),
        (11.99, 0),
        (12.0, 0),
        (17.99, 0),
        (18.0, 0),
        (20.0, 8),
        (21.99, 16),
        (22.0, 16),
        (30.0, 16),
    ];
    for (distance, level) in cases {
        assert_eq!(range.dither_level(distance), level, "at {distance} m");
    }
    assert!(!range.is_drawn(8.0));
    assert!(range.is_drawn(8.2));
    assert!(range.is_drawn(21.0));
    assert!(!range.is_drawn(22.0));

    // The first step is fully drawn from the camera on.
    assert_eq!(LodRange::ALWAYS.dither_level(0.0), 0);
    assert_eq!(LodRange::ALWAYS.dither_level(1e9), 0);
    assert!(LodRange::ALWAYS.is_always());
    assert!(!range.is_always());
}

#[test]
fn dither_levels_cross_fade_neighbouring_levels() {
    // Over a shared band, a step fading out at +n covers exactly the pixels
    // its neighbour fading in at n - 16 leaves.
    let first = LodRange {
        fade_in: 0.0..0.0,
        fade_out: 8.0..12.0,
    };
    let second = middle();
    for step in 0..=40 {
        let distance = 8.0 + step as f32 * 0.1;
        let out = first.dither_level(distance);
        let into = second.dither_level(distance);
        assert_eq!(into, out - 16, "at {distance} m");
    }
}

#[test]
fn zero_hysteresis_swaps_in_one_step() {
    let first = LodRange {
        fade_in: 0.0..0.0,
        fade_out: 10.0..10.0,
    };
    let second = LodRange {
        fade_in: 10.0..10.0,
        fade_out: f32::INFINITY..f32::INFINITY,
    };
    assert_eq!(first.dither_level(9.99), 0);
    assert_eq!(first.dither_level(10.0), 16);
    assert_eq!(second.dither_level(9.99), -16);
    assert_eq!(second.dither_level(10.0), 0);
}

#[test]
fn packed_ranges_replace_infinity() {
    let packed = LodRange::ALWAYS.to_vec4();
    assert_eq!(packed.to_array(), [0.0, 0.0, f32::MAX, f32::MAX]);
}

#[test]
fn far_forms_have_their_file_names() {
    let form = FarForm::GroundMaterial;
    let json = serde_json::to_value(form).unwrap();
    assert_eq!(json, serde_json::json!("ground_material"));
    assert_eq!(serde_json::from_value::<FarForm>(json).unwrap(), form);
}
