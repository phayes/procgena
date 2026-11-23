//! Options validation and JSON.

use procgena_mushroom::{
    CapFamily, Mushroom, MushroomError, MushroomLod, MushroomOptions, MushroomPreset, OptionsError,
};

#[test]
fn defaults_and_presets_validate() {
    MushroomOptions::default().validate().unwrap();
    for preset in MushroomPreset::ALL {
        preset.options().validate().unwrap();
    }
    MushroomLod::default().validate().unwrap();
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
    let small = MushroomLod::for_size(0.04);
    let big = MushroomLod::for_size(0.16);
    assert!((big.cull_distance - small.cull_distance * 4.0).abs() < 1e-3);
    assert_eq!(small.levels[0].distance, 0.0);

    let mut bad = MushroomLod {
        cull_distance: 1.0,
        ..MushroomLod::default()
    };
    assert!(bad.validate().is_err());
    bad = MushroomLod::default();
    bad.levels.swap(1, 2);
    assert!(bad.validate().is_err());
}
