//! Check defaults, presets, JSON handling and option validation.

use glam::Vec3;
use procgena_tree::{BarkUvMode, ClusterKind, Facing, OptionsError, Tree, TreeOptions, TreePreset};
use serde_json::Value;

/// Checks that each preset returns the full contents of its embedded JSON file.
#[test]
fn preset_json_is_the_embedded_file() {
    for preset in TreePreset::ALL {
        let stem = preset.name().to_lowercase().replace(' ', "_");
        let file = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("presets/{stem}.json")),
        )
        .unwrap();
        assert_eq!(preset.json(), file, "{stem}");
    }
}

#[test]
fn presets_are_valid_and_round_trip() {
    for preset in TreePreset::ALL {
        let options = preset.options();
        assert_eq!(options.validate(), Ok(()), "{}", preset.name());
        assert_eq!(
            TreeOptions::from_json(&options.to_json()).unwrap(),
            options,
            "{}",
            preset.name()
        );
        assert_eq!(TreePreset::from_name(preset.name()), Some(preset));
    }
    assert_eq!(TreePreset::from_name("Maple"), None);
}

#[test]
fn partial_json_keeps_nested_defaults() {
    let options = TreeOptions::from_json(
        r#"{
            "branch": { "angle": { "1": 10 }, "levels": 2 },
            "trellis": { "position": { "z": 5 } },
            "leaves": { "billboard": "single" }
        }"#,
    )
    .unwrap();

    let mut expected = TreeOptions::default();
    expected.branch.angle[1] = 10.0;
    expected.branch.levels = 2;
    expected.trellis.position = Vec3::new(0.0, 0.0, 5.0);
    expected.leaves.billboard = procgena_tree::Billboard::Single;
    assert_eq!(options, expected);
}

#[test]
fn bark_uv_mode_defaults_to_continuous_and_reads_from_json() {
    assert_eq!(TreeOptions::default().bark.uv_mode, BarkUvMode::Continuous);
    for preset in TreePreset::ALL {
        assert_eq!(preset.options().bark.uv_mode, BarkUvMode::Continuous);
    }
    let options = TreeOptions::from_json(r#"{ "bark": { "uvMode": "mirrored" } }"#).unwrap();
    assert_eq!(options.bark.uv_mode, BarkUvMode::Mirrored);
    let json: Value = serde_json::from_str(&options.to_json()).unwrap();
    assert_eq!(json["bark"]["uvMode"], "mirrored");
}

/// Clusters default to no density, and `placement.density` reads from JSON.
#[test]
fn cluster_density_reads_from_json() {
    let defaults = TreeOptions::default();
    assert_eq!(defaults.fruit.placement.density, 0.0);
    assert_eq!(defaults.flowers.placement.density, 0.0);

    let options =
        TreeOptions::from_json(r#"{ "fruit": { "placement": { "density": 1.5 } } }"#).unwrap();
    assert_eq!(options.fruit.placement.density, 1.5);

    // The pussy willow preset is leafless and places catkins by density.
    let willow = TreePreset::SpringPussyWillow.options();
    assert_eq!(willow.leaves.count, 0);
    assert!(willow.fruit.placement.density > 0.0);
}

#[test]
fn unknown_fields_are_ignored() {
    let options = TreeOptions::from_json(
        r#"{ "seed": 4, "colour": 1, "branch": { "angle": { "7": 1 }, "extra": true } }"#,
    )
    .unwrap();
    let expected = TreeOptions {
        seed: 4,
        ..TreeOptions::default()
    };
    assert_eq!(options, expected);
}

#[test]
fn serde_deserialize_requires_complete_options() {
    let options = TreePreset::OakMedium.options();
    let complete: TreeOptions = serde_json::from_str(&options.to_json()).unwrap();
    assert_eq!(complete, options);

    assert!(serde_json::from_str::<TreeOptions>(r#"{ "seed": 4 }"#).is_err());
}

#[test]
fn wrong_types_are_errors() {
    assert!(TreeOptions::from_json(r#"{ "seed": "12" }"#).is_err());
    assert!(TreeOptions::from_json(r#"{ "type": "palm" }"#).is_err());
    assert!(TreeOptions::from_json(r#"{ "branch": { "levels": -1 } }"#).is_err());
    assert!(TreeOptions::from_json("not json").is_err());
}

#[test]
fn seed_is_converted_like_javascript() {
    let seed = |json: &str| TreeOptions::from_json(json).unwrap().seed;
    assert_eq!(seed(r#"{ "seed": 26867 }"#), 26867);
    assert_eq!(seed(r#"{ "seed": -5 }"#), -5);
    assert_eq!(seed(r#"{ "seed": 3000000000 }"#), -1_294_967_296);
    assert_eq!(seed(r#"{ "seed": 4294967301 }"#), 5);
    assert_eq!(seed(r#"{ "seed": -2.7 }"#), -2);
}

#[test]
fn too_many_levels_is_rejected() {
    let mut options = TreeOptions::default();
    options.branch.levels = 4;
    assert_eq!(
        options.validate(),
        Err(OptionsError::TooManyLevels { levels: 4 })
    );
    options.branch.levels = 3;
    assert_eq!(options.validate(), Ok(()));
}

#[test]
fn zero_sections_is_rejected_only_for_levels_in_use() {
    let mut options = TreeOptions::default();
    options.branch.levels = 1;
    options.branch.sections[1] = 0;
    assert_eq!(
        options.validate(),
        Err(OptionsError::ZeroSections { level: 1 })
    );

    // With levels 0 and 1 configured, a zero section count at level 2 is ignored.
    options.branch.sections[1] = 5;
    options.branch.sections[2] = 0;
    assert_eq!(options.validate(), Ok(()));
}

#[test]
fn zero_radius_is_rejected_only_for_levels_in_use() {
    let mut options = TreeOptions::default();
    options.branch.levels = 1;
    options.branch.radius[1] = 0.0;
    assert_eq!(
        options.validate(),
        Err(OptionsError::InvalidSetting {
            field: "branch.radius.1".to_owned(),
            requirement: "a finite value above 0",
        })
    );
    // Tree creation must also reject a zero radius at a configured level.
    assert!(Tree::new(options.clone()).is_err());

    // Level 2 is outside the configured depth, so its zero radius is allowed.
    options.branch.radius[1] = 0.5;
    options.branch.radius[2] = 0.0;
    assert_eq!(options.validate(), Ok(()));
}

#[test]
fn partial_fruit_and_flower_json_keeps_nested_defaults() {
    let options = TreeOptions::from_json(
        r#"{
            "fruit": { "cluster": { "kind": "panicle", "items": 40 }, "maturity": 0.25 },
            "flowers": { "placement": { "facing": "up", "count": 3 }, "shape": { "petalCount": 6 } }
        }"#,
    )
    .unwrap();

    let mut expected = TreeOptions::default();
    expected.fruit.cluster.kind = ClusterKind::Panicle;
    expected.fruit.cluster.items = 40;
    expected.fruit.maturity = 0.25;
    expected.flowers.placement.facing = Facing::Up;
    expected.flowers.placement.count = 3;
    expected.flowers.shape.petal_count = 6;
    assert_eq!(options, expected);
}

#[test]
fn fruit_and_flower_settings_round_trip() {
    let mut options = TreePreset::OakMedium.options();
    options.fruit.placement.count = 4;
    options.fruit.placement.facing = Facing::Outward;
    options.fruit.cluster.kind = ClusterKind::Umbel;
    options.fruit.shape.lobes = 5;
    options.fruit.shape.calyx = false;
    options.flowers.placement.tips = 0.5;
    options.flowers.cluster.kind = ClusterKind::Catkin;
    options.flowers.shape.fade_color = 0x123456;
    options.flowers.maturity = 0.9;

    let json = options.to_json();
    assert_eq!(TreeOptions::from_json(&json).unwrap(), options);

    // Serialized settings should use the documented JSON names and enum values.
    let value: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["fruit"]["placement"]["facing"], "outward");
    assert_eq!(value["fruit"]["cluster"]["kind"], "umbel");
    assert_eq!(value["fruit"]["shape"]["calyx"], false);
    assert_eq!(value["flowers"]["cluster"]["kind"], "catkin");
    assert_eq!(value["flowers"]["shape"]["fadeColor"], 0x123456);
    assert!(value["flowers"]["placement"]["sizeVariance"].is_number());
}

#[test]
fn fruit_and_flower_defaults_are_valid() {
    assert_eq!(TreeOptions::default().validate(), Ok(()));
}

#[test]
fn leaf_flatten_must_be_between_0_and_1() {
    let mut options = TreeOptions::default();
    for (flatten, ok) in [
        (0.0, true),
        (1.0, true),
        (-0.1, false),
        (1.1, false),
        (f32::NAN, false),
    ] {
        options.leaves.flatten = flatten;
        let expected = if ok {
            Ok(())
        } else {
            Err(OptionsError::InvalidSetting {
                field: "leaves.flatten".to_owned(),
                requirement: "between 0 and 1",
            })
        };
        assert_eq!(options.validate(), expected, "{flatten}");
    }
    let options = TreeOptions::from_json(r#"{ "leaves": { "flatten": 0.75 } }"#).unwrap();
    assert_eq!(options.leaves.flatten, 0.75);
}

#[test]
fn out_of_range_fruit_and_flower_settings_are_rejected() {
    let invalid = |field: &str, requirement: &'static str| OptionsError::InvalidSetting {
        field: field.to_owned(),
        requirement,
    };
    let check = |change: fn(&mut TreeOptions), error: OptionsError| {
        let mut options = TreeOptions::default();
        change(&mut options);
        assert_eq!(options.validate(), Err(error));
    };

    check(
        |o| o.fruit.maturity = 1.5,
        invalid("fruit.maturity", "between 0 and 1"),
    );
    check(
        |o| o.flowers.maturity = -0.1,
        invalid("flowers.maturity", "between 0 and 1"),
    );
    check(
        |o| o.fruit.maturity = f32::NAN,
        invalid("fruit.maturity", "between 0 and 1"),
    );
    check(
        |o| o.fruit.placement.size = -1.0,
        invalid("fruit.placement.size", "a finite value of 0 or more"),
    );
    check(
        |o| o.flowers.cluster.stalk_length = f32::INFINITY,
        invalid("flowers.cluster.stalkLength", "a finite value of 0 or more"),
    );
    check(
        |o| o.flowers.placement.size_variance = 1.5,
        invalid("flowers.placement.sizeVariance", "between 0 and 1"),
    );
    check(
        |o| o.fruit.cluster.side_stalks = 0,
        invalid("fruit.cluster.sideStalks", "at least 1"),
    );
    check(
        |o| o.fruit.shape.segments = 2,
        invalid("fruit.shape.segments", "at least 3"),
    );
    check(
        |o| o.fruit.shape.rings = 1,
        invalid("fruit.shape.rings", "at least 2"),
    );
    check(
        |o| o.fruit.shape.elongation = 0.0,
        invalid("fruit.shape.elongation", "a finite value above 0"),
    );
    check(
        |o| o.fruit.shape.lobe_depth = 1.0,
        invalid("fruit.shape.lobeDepth", "at least 0 and below 1"),
    );
    check(
        |o| o.flowers.shape.petal_count = 0,
        invalid("flowers.shape.petalCount", "at least 1"),
    );
    check(
        |o| o.flowers.shape.petal_segments = 0,
        invalid("flowers.shape.petalSegments", "at least 1"),
    );
    check(
        |o| o.flowers.shape.cup = 2.0,
        invalid("flowers.shape.cup", "between 0 and 1"),
    );

    // An invalid branch depth should be reported before an invalid fruit setting.
    check(
        |o| {
            o.branch.levels = 4;
            o.fruit.maturity = 2.0;
        },
        OptionsError::TooManyLevels { levels: 4 },
    );
}

#[test]
fn invalid_setting_message_names_the_field() {
    let mut options = TreeOptions::default();
    options.fruit.cluster.droop = 3.0;
    let message = options.validate().unwrap_err().to_string();
    assert_eq!(message, "fruit.cluster.droop must be between 0 and 1");
}
