//! Options validation and `.mushroom.yml` preset files.

use procgena_core::Quality;
use procgena_core::chain::{DistanceChain, ViewScale};
use procgena_mushroom::{
    CapFamily, Mushroom, MushroomDetail, MushroomOptions, MushroomPreset, OptionsError,
    PresetFileError, ScatterOptions,
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
fn the_game_level_is_checked_with_the_options() {
    let mut o = MushroomOptions::default();
    o.game_level.cap_sides = 2;
    assert!(matches!(
        o.validate(),
        Err(OptionsError::IntOutOfRange {
            field: "game_level.cap_sides",
            ..
        })
    ));
    o.game_level = MushroomDetail::FULL;
    o.game_level.stem_segments = 0;
    assert!(matches!(
        o.validate(),
        Err(OptionsError::IntOutOfRange {
            field: "game_level.stem_segments",
            ..
        })
    ));
}

#[test]
fn the_game_level_defaults_to_high_and_can_be_set_in_a_file() {
    assert_eq!(MushroomOptions::default().game_level, MushroomDetail::HIGH);
    assert_eq!(read("base: porcini\n").unwrap().game_level, MushroomDetail::HIGH);
    let full = read(
        "base: porcini\n\
         mushroom:\n  \
           game_level:\n    \
             shape: solid\n    \
             cap_sides: 48\n    \
             cap_segments: 16\n    \
             stem_sides: 24\n    \
             stem_segments: 12\n    \
             annulus: true\n    \
             paint_warts: true\n",
    )
    .unwrap();
    assert_eq!(full.game_level, MushroomDetail::FULL);
}

#[test]
fn bigger_mushrooms_switch_further_away() {
    let mushroom = |size: f32| {
        let options = MushroomOptions {
            size,
            ..MushroomOptions::default()
        };
        Mushroom::new(options).unwrap()
    };
    let (small, big) = (mushroom(0.04), mushroom(0.16));
    let ratio = big.bounding_diameter() / small.bounding_diameter();
    assert!((ratio - 4.0).abs() < 1e-3, "{ratio}");

    // The lower level of a mushroom's specimen starts where the default
    // chain switches to it.
    let factor = DistanceChain::factor(big.bounding_diameter(), ViewScale::DEFAULT);
    let [_, lower, _] = DistanceChain::DEFAULT.ranges(factor, Quality::High);
    let specimen = big.specimen();
    assert_eq!(specimen.levels.len(), 2);
    assert_eq!(specimen.levels[0].start, 0.0);
    let middle = (lower.fade_in.start + lower.fade_in.end) / 2.0;
    assert!((specimen.levels[1].start - middle).abs() < 1e-4);
}
