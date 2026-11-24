//! Options validation and JSON.

use procgena_core::{FarForm, LevelForm, LodRange, Quality};
use procgena_mushroom::{
    CapFamily, DEFAULT_LOD_SIZE, Mushroom, MushroomDetail, MushroomError, MushroomOptions,
    MushroomPreset, OptionsError, default_lod, validate_lod,
};

#[test]
fn defaults_and_presets_validate() {
    MushroomOptions::default().validate().unwrap();
    for preset in MushroomPreset::ALL {
        preset.options().validate().unwrap();
    }
    validate_lod(&default_lod()).unwrap();
}

#[test]
fn json_round_trips() {
    for preset in MushroomPreset::ALL {
        let options = preset.options();
        let back = MushroomOptions::from_json(&options.to_json()).unwrap();
        assert_eq!(back, options, "{}", preset.name());
    }
}

#[test]
fn partial_json_keeps_defaults() {
    let options =
        MushroomOptions::from_json(r#"{"seed": 7, "cap": {"family": "conical", "lobes": 4}}"#)
            .unwrap();
    let defaults = MushroomOptions::default();
    assert_eq!(options.seed, 7);
    assert_eq!(options.cap.family, CapFamily::Conical);
    assert_eq!(options.cap.lobes, 4);
    assert_eq!(options.cap.height, defaults.cap.height);
    assert_eq!(options.stem, defaults.stem);
}

#[test]
fn bad_values_are_rejected() {
    let mut o = MushroomOptions::default();
    o.stem.upturn = 1.5;
    assert!(matches!(
        o.validate(),
        Err(OptionsError::FloatNotInRange {
            field: "stem.upturn",
            ..
        })
    ));

    let o = MushroomOptions {
        size: -1.0,
        ..MushroomOptions::default()
    };
    assert!(matches!(
        o.validate(),
        Err(OptionsError::FloatOutOfRange { field: "size", .. })
    ));

    let mut o = MushroomOptions::default();
    o.cap.tilt = 90.0;
    assert!(matches!(
        o.validate(),
        Err(OptionsError::FloatNotInRange {
            field: "cap.tilt",
            ..
        })
    ));

    let mut o = MushroomOptions::default();
    o.cap.lobes = 99;
    assert!(matches!(
        o.validate(),
        Err(OptionsError::IntOutOfRange {
            field: "cap.lobes",
            ..
        })
    ));

    let mut o = MushroomOptions::default();
    o.material.cap_rim = "red".into();
    assert!(matches!(o.validate(), Err(OptionsError::Colour { .. })));

    let mut o = MushroomOptions::default();
    o.stem.height = f32::NAN;
    assert!(o.validate().is_err());

    let mut o = MushroomOptions::default();
    o.cluster.stems = 0;
    assert!(matches!(
        o.validate(),
        Err(OptionsError::IntOutOfRange {
            field: "cluster.stems",
            ..
        })
    ));

    let mut o = MushroomOptions::default();
    o.cluster.fairy_ring.radius = 0.5;
    assert!(matches!(
        o.validate(),
        Err(OptionsError::FloatNotInRange {
            field: "cluster.fairy_ring.radius",
            ..
        })
    ));

    let mut o = MushroomOptions::default();
    o.bulb.depth = 0.0;
    assert!(matches!(
        o.validate(),
        Err(OptionsError::FloatNotInRange {
            field: "bulb.depth",
            ..
        })
    ));
}

#[test]
fn from_json_reports_both_kinds_of_error() {
    assert!(matches!(
        Mushroom::from_json("{"),
        Err(MushroomError::Json(_))
    ));
    assert!(matches!(
        Mushroom::from_json(r#"{"size": 0}"#),
        Err(MushroomError::Options(_))
    ));
    assert!(Mushroom::from_json(r#"{"size": 0.05}"#).is_ok());
}

#[test]
fn lod_scales_with_size() {
    let lod = default_lod();
    let small = lod.ranges(lod.factor(0.04, Quality::High));
    let big = lod.ranges(lod.factor(0.16, Quality::High));
    let cull = |ranges: &[LodRange]| ranges.last().unwrap().fade_out.end;
    assert!((cull(&big) - cull(&small) * 4.0).abs() < 1e-3);
    assert_eq!(small[0].fade_in, 0.0..0.0);

    let mut bad = default_lod().with_cull(1.0);
    assert!(matches!(validate_lod(&bad), Err(OptionsError::Lod(_))));
    bad = default_lod();
    bad.levels.swap(1, 2);
    assert!(matches!(validate_lod(&bad), Err(OptionsError::Lod(_))));
    bad = default_lod();
    bad.levels[0].detail.cap_sides = 2;
    assert!(matches!(
        validate_lod(&bad),
        Err(OptionsError::IntOutOfRange { .. })
    ));
}

#[test]
fn the_farthest_default_level_is_a_card_without_shadows() {
    let lod = default_lod();
    let last = lod.levels.last().unwrap();
    assert_eq!(last.detail, MushroomDetail::BILLBOARD);
    assert_eq!(last.form, LevelForm::Far(FarForm::Card));
    assert!(!last.casts_shadows);
    assert!(
        lod.levels[..lod.levels.len() - 1]
            .iter()
            .all(|l| { l.form == LevelForm::Mesh && l.casts_shadows })
    );
    assert_eq!(lod.cull_distance, Some(40.0));
    assert_eq!(lod.reference_size, DEFAULT_LOD_SIZE);
}
