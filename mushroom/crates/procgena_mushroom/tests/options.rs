//! Options validation and `.mushroom.yml` preset files.

use procgena_core::{FarForm, LevelForm, LodRange, Quality};
use procgena_mushroom::{
    CapFamily, DEFAULT_LOD_SIZE, Mushroom, MushroomDetail, MushroomOptions, MushroomPreset,
    OptionsError, PresetFileError, ScatterOptions, default_lod, validate_lod,
};

/// Reads the options of a `.mushroom.yml` file.
fn read(text: &str) -> Result<MushroomOptions, PresetFileError> {
    MushroomPreset::from_yaml("test", text).map(|preset| preset.mushroom)
}

#[test]
fn defaults_and_presets_validate() {
    MushroomOptions::default().validate().unwrap();
    for preset in MushroomPreset::all() {
        preset.mushroom.validate().unwrap();
    }
    validate_lod(&default_lod()).unwrap();
}

#[test]
fn a_file_round_trips() {
    for preset in MushroomPreset::all() {
        let back = read(&preset.to_yaml(None)).unwrap();
        assert_eq!(back, preset.mushroom, "{}", preset.name);
    }
}

#[test]
fn a_partial_file_keeps_nested_defaults() {
    let options =
        read("mushroom:\n  default_seed: 7\n  cap:\n    family: conical\n    lobes: 4\n").unwrap();
    let defaults = MushroomOptions::default();
    assert_eq!(options.default_seed, 7);
    assert_eq!(options.cap.family, CapFamily::Conical);
    assert_eq!(options.cap.lobes, 4);
    assert_eq!(options.cap.height, defaults.cap.height);
    assert_eq!(options.stem, defaults.stem);
}

#[test]
fn a_file_with_a_base_keeps_the_base_settings() {
    let options =
        read("base: porcini\nmushroom:\n  material:\n    cap_rim: \"#102030\"\n").unwrap();
    let mut expected = MushroomPreset::porcini().mushroom.clone();
    expected.material.cap_rim = "#102030".into();
    assert_eq!(options, expected);
}

#[test]
fn a_file_rejects_unknown_names() {
    for text in [
        "mushroom:\n  seed: 7\n",
        "mushroom:\n  cap:\n    note: 1\n",
        "mushroom:\n  cluster:\n    fairy_ring:\n      note: 1\n",
        "tags: [a]\n",
        "base: toadstool\n",
    ] {
        assert!(read(text).is_err(), "accepted {text}");
    }
}

#[test]
fn a_file_rejects_bad_values() {
    for text in [
        "mushroom:\n  default_seed: twelve\n",
        "mushroom:\n  default_seed: -1\n",
        "mushroom:\n  cap:\n    family: square\n",
        "mushroom:\n  cap: 3\n",
        "[1, 2]\n",
        "mushroom: {default_seed: 1\n",
    ] {
        assert!(read(text).is_err(), "accepted {text}");
    }
}

#[test]
fn a_file_reports_unreadable_text_and_out_of_range_settings() {
    assert!(matches!(
        read("mushroom: {"),
        Err(PresetFileError::Yaml { .. })
    ));
    assert!(matches!(
        read("mushroom:\n  size: 0\n"),
        Err(PresetFileError::Invalid { .. })
    ));
    let options = read("mushroom:\n  size: 0.05\n").unwrap();
    assert!(Mushroom::new(options).is_ok());
}

#[test]
fn scatter_options_round_trip_and_reject_unknown_names() {
    let mut options = ScatterOptions {
        default_seed: 9,
        ..ScatterOptions::default()
    };
    options.up = glam::Vec3::new(0.0, 1.0, 0.5);
    let text = serde_json::to_string(&options).unwrap();
    let back: ScatterOptions = serde_json::from_str(&text).unwrap();
    assert_eq!(back, options);

    let unknown = text.replacen('{', "{\"seed\":1,", 1);
    assert!(serde_json::from_str::<ScatterOptions>(&unknown).is_err());
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
