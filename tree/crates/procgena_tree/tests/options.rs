//! Check defaults, presets, `.tree.yml` files and option validation.

use std::path::Path;

use glam::Vec3;
use procgena_tree::procgena_fruit::{ClusterKind, Facing, FruitOptions, FruitPreset, ItemShape};
use procgena_tree::{
    AttachedFruit, BarkUvMode, OptionsError, PresetFileError, Tree, TreeOptions, TreePreset,
    TreeScatterPreset,
};

/// Default tree settings that grow default fruit and flowers.
fn with_clusters() -> TreeOptions {
    TreeOptions {
        fruit: Some(AttachedFruit::default()),
        flowers: Some(AttachedFruit::default()),
        ..TreeOptions::default()
    }
}

/// The tree's fruit settings, which must be present.
fn fruit(options: &mut TreeOptions) -> &mut AttachedFruit {
    options.fruit.as_mut().unwrap()
}

/// The tree's flower settings, which must be present.
fn flowers(options: &mut TreeOptions) -> &mut AttachedFruit {
    options.flowers.as_mut().unwrap()
}
use serde_json::Value;

/// Reads a file in the crate's folder.
fn crate_file(path: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
        .unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// The names of the files in a folder of the crate that end with `suffix`.
fn files_ending(folder: &str, suffix: &str) -> Vec<String> {
    let mut names: Vec<String> =
        std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join(folder))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.ends_with(suffix))
            .collect();
    names.sort();
    names
}

/// Reads tree preset file text with the default key used by these tests.
fn tree_yaml(text: &str) -> Result<TreeOptions, PresetFileError> {
    TreePreset::from_yaml("test", text).map(|preset| preset.tree)
}

/// Each preset is its file in `presets/`, and writes and reads back equal.
#[test]
fn preset_files_match_the_catalogue() {
    let mut expected: Vec<String> = TreePreset::all()
        .map(|preset| format!("{}.tree.yml", preset.key))
        .collect();
    expected.sort();
    assert_eq!(files_ending("presets", ".tree.yml"), expected);

    let other = TreePreset::oak_medium();
    for preset in TreePreset::all() {
        let key = &preset.key;
        let file = TreePreset::from_yaml(key, &crate_file(&format!("presets/{key}.tree.yml")))
            .unwrap_or_else(|error| panic!("{key}: {error}"));
        assert_eq!(file.key, preset.key);
        assert_eq!(file.tree, preset.tree, "{key}");
        assert_eq!(file.name, preset.name, "{key}");
        assert_eq!(file.description, preset.description, "{key}");
        assert_eq!(file.group, preset.group, "{key}");

        for base in [None, Some(other)] {
            let text = preset.to_yaml(base);
            let read = TreePreset::from_yaml(key, &text)
                .unwrap_or_else(|error| panic!("{key}: {error}\n{text}"));
            assert_eq!(read.tree, preset.tree, "{key}\n{text}");
            assert_eq!(read.name, preset.name, "{key}");
            assert_eq!(read.description, preset.description, "{key}");
            assert_eq!(read.group, preset.group, "{key}");
        }
    }
}

/// Each Scatter preset is its file in `scatters/`, and writes and reads back
/// equal.
#[test]
fn scatter_preset_files_match_the_catalogue() {
    let mut expected: Vec<String> = TreeScatterPreset::all()
        .map(|preset| format!("{}.tree_scatter.yml", preset.key))
        .collect();
    expected.sort();
    assert_eq!(files_ending("scatters", ".tree_scatter.yml"), expected);

    let other = TreeScatterPreset::pine_stand();
    for preset in TreeScatterPreset::all() {
        let key = &preset.key;
        let file = TreeScatterPreset::from_yaml(
            key,
            &crate_file(&format!("scatters/{key}.tree_scatter.yml")),
        )
        .unwrap_or_else(|error| panic!("{key}: {error}"));
        assert_eq!(file, *preset, "{key}");

        for base in [None, Some(other)] {
            let text = preset.to_yaml(base);
            let read = TreeScatterPreset::from_yaml(key, &text)
                .unwrap_or_else(|error| panic!("{key}: {error}\n{text}"));
            assert_eq!(read, *preset, "{key}\n{text}");
        }
    }
}

#[test]
fn presets_are_valid_and_found_by_key() {
    for preset in TreePreset::all() {
        assert_eq!(preset.tree.validate(), Ok(()), "{}", preset.name);
        assert_eq!(TreePreset::named(&preset.key), Some(preset));
    }
    assert_eq!(TreePreset::named("maple"), None);
}

#[test]
fn partial_yaml_keeps_nested_defaults() {
    let options = tree_yaml(
        "tree:\n  \
           branch: { angle: { \"1\": 10 }, levels: 2 }\n  \
           trellis: { position: { z: 5 } }\n  \
           leaves: { billboard: single }\n",
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
fn bark_uv_mode_defaults_to_continuous_and_reads_from_yaml() {
    assert_eq!(TreeOptions::default().bark.uv_mode, BarkUvMode::Continuous);
    for preset in TreePreset::all() {
        assert_eq!(preset.tree.bark.uv_mode, BarkUvMode::Continuous);
    }
    let options = tree_yaml("tree:\n  bark: { uv_mode: mirrored }\n").unwrap();
    assert_eq!(options.bark.uv_mode, BarkUvMode::Mirrored);
    let value = serde_json::to_value(&options).unwrap();
    assert_eq!(value["bark"]["uv_mode"], "mirrored");
}

/// Trees grow no fruit or flowers by default, and `placement.density` reads
/// from a file, with the other cluster settings at their defaults.
#[test]
fn cluster_density_reads_from_yaml() {
    let defaults = TreeOptions::default();
    assert_eq!(defaults.fruit, None);
    assert_eq!(defaults.flowers, None);

    let options = tree_yaml("tree:\n  fruit: { placement: { density: 1.5 } }\n").unwrap();
    let fruit = options.fruit.unwrap();
    assert_eq!(fruit.placement.density, 1.5);
    assert_eq!(fruit.fruit, FruitOptions::default());

    // The pussy willow preset is leafless and places catkins by density.
    let willow = &TreePreset::spring_pussy_willow().tree;
    assert_eq!(willow.leaves.count, 0);
    assert!(willow.fruit.as_ref().unwrap().placement.density > 0.0);
}

/// A tree file names its fruit by a fruit preset key, or gives settings over
/// a `base` fruit preset; an unknown key is an error.
#[test]
fn fruit_reads_as_a_preset_key_or_settings_over_a_base() {
    let options = tree_yaml("tree:\n  fruit: { fruit: apple }\n").unwrap();
    assert_eq!(options.fruit.unwrap().fruit, FruitPreset::apple().fruit);

    let options =
        tree_yaml("tree:\n  flowers: { fruit: { base: dogwood_bloom, maturity: 0.2 } }\n")
            .unwrap();
    let flowers = options.flowers.unwrap().fruit;
    assert_eq!(flowers.maturity, 0.2);
    assert_eq!(flowers.item, FruitPreset::dogwood_bloom().fruit.item);

    assert!(tree_yaml("tree:\n  fruit: { fruit: quince }\n").is_err());

    // A tree whose fruit is a built-in preset writes the preset's key.
    let value = serde_json::to_value(&TreePreset::apple().tree).unwrap();
    assert_eq!(value["fruit"]["fruit"], "apple");
}

#[test]
fn unknown_settings_are_rejected() {
    let options = tree_yaml("tree:\n  default_seed: 4\n").unwrap();
    let expected = TreeOptions {
        default_seed: 4,
        ..TreeOptions::default()
    };
    assert_eq!(options, expected);

    for text in [
        "tree:\n  colour: 1\n",
        "tree:\n  branch: { extra: true }\n",
        "tree:\n  branch: { angle: { \"7\": 1 } }\n",
    ] {
        assert!(
            matches!(tree_yaml(text), Err(PresetFileError::Setting { .. })),
            "{text}"
        );
    }
    assert!(matches!(
        tree_yaml("colour: 1\n"),
        Err(PresetFileError::UnknownKey { .. })
    ));
}

#[test]
fn serde_deserialize_requires_complete_options() {
    let options = &TreePreset::oak_medium().tree;
    let complete: TreeOptions =
        serde_json::from_str(&serde_json::to_string(options).unwrap()).unwrap();
    assert_eq!(&complete, options);

    assert!(serde_json::from_str::<TreeOptions>(r#"{ "default_seed": 4 }"#).is_err());
}

#[test]
fn wrong_types_are_errors() {
    for text in [
        "tree:\n  default_seed: \"12\"\n",
        "tree:\n  type: palm\n",
        "tree:\n  branch: { levels: -1 }\n",
    ] {
        assert!(
            matches!(tree_yaml(text), Err(PresetFileError::Setting { .. })),
            "{text}"
        );
    }
    assert!(matches!(
        tree_yaml("tree: {"),
        Err(PresetFileError::Yaml { .. })
    ));
    assert!(matches!(
        tree_yaml("not yaml settings"),
        Err(PresetFileError::NotAMap { .. })
    ));
}

/// The seed is a whole number in the range of an `i32`.
#[test]
fn default_seed_is_a_whole_32_bit_number() {
    let seed = |text: &str| tree_yaml(text).map(|options| options.default_seed);
    assert_eq!(seed("tree:\n  default_seed: 26867\n"), Ok(26867));
    assert_eq!(seed("tree:\n  default_seed: -5\n"), Ok(-5));
    for text in [
        "tree:\n  default_seed: 3000000000\n",
        "tree:\n  default_seed: -2.7\n",
    ] {
        assert!(
            matches!(seed(text), Err(PresetFileError::Setting { .. })),
            "{text}"
        );
    }
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

    // A file with too many levels reports the same problem.
    assert!(matches!(
        tree_yaml("tree:\n  branch: { levels: 4 }\n"),
        Err(PresetFileError::Invalid { .. })
    ));
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
fn partial_fruit_and_flower_yaml_keeps_nested_defaults() {
    let options = tree_yaml(
        "tree:\n  \
           fruit: { fruit: { cluster: { arrangement: panicle, items: 40 }, maturity: 0.25 } }\n  \
           flowers: { placement: { count: 3 }, fruit: { facing: up, item: { kind: flower, petal_count: 6 } } }\n",
    )
    .unwrap();

    let mut expected = with_clusters();
    fruit(&mut expected).fruit.cluster.kind = ClusterKind::Panicle;
    fruit(&mut expected).fruit.cluster.items = 40;
    fruit(&mut expected).fruit.maturity = 0.25;
    flowers(&mut expected).placement.count = 3;
    flowers(&mut expected).fruit.facing = Facing::Up;
    flowers(&mut expected).fruit.item = ItemShape::Flower(procgena_tree::procgena_fruit::FlowerShape {
        petal_count: 6,
        ..Default::default()
    });
    assert_eq!(options, expected);
}

#[test]
fn fruit_and_flower_settings_round_trip() {
    let mut preset = TreePreset::oak_medium().clone();
    preset.tree.fruit = Some(AttachedFruit::default());
    preset.tree.flowers = Some(AttachedFruit::new(
        FruitPreset::dogwood_bloom().fruit.clone(),
        Default::default(),
    ));
    let options = &mut preset.tree;
    fruit(options).placement.count = 4;
    fruit(options).fruit.facing = Facing::Outward;
    fruit(options).fruit.cluster.kind = ClusterKind::Umbel;
    fruit(options).default_slots = 3;
    flowers(options).placement.tips = 0.5;
    flowers(options).fruit.cluster.kind = ClusterKind::Catkin;
    flowers(options).fruit.maturity = 0.9;

    for base in [None, Some(TreePreset::oak_medium()), Some(TreePreset::apple())] {
        let text = preset.to_yaml(base);
        assert_eq!(tree_yaml(&text).unwrap(), preset.tree, "{text}");
    }

    // Serialized settings should use the documented file names and enum values.
    let value: Value = serde_json::to_value(&preset.tree).unwrap();
    assert_eq!(value["fruit"]["fruit"]["facing"], "outward");
    assert_eq!(value["fruit"]["fruit"]["cluster"]["arrangement"], "umbel");
    assert_eq!(value["fruit"]["default_slots"], 3);
    assert_eq!(value["flowers"]["fruit"]["cluster"]["arrangement"], "catkin");
    assert_eq!(value["flowers"]["fruit"]["item"]["kind"], "flower");
    assert!(value["flowers"]["placement"]["scale_variance"].is_number());
}

#[test]
fn fruit_and_flower_defaults_are_valid() {
    assert_eq!(TreeOptions::default().validate(), Ok(()));
    assert_eq!(with_clusters().validate(), Ok(()));
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
    let options = tree_yaml("tree:\n  leaves: { flatten: 0.75 }\n").unwrap();
    assert_eq!(options.leaves.flatten, 0.75);
}

#[test]
fn out_of_range_fruit_and_flower_settings_are_rejected() {
    let invalid = |field: &str, requirement: &'static str| OptionsError::InvalidSetting {
        field: field.to_owned(),
        requirement,
    };
    let check = |change: fn(&mut TreeOptions), error: OptionsError| {
        let mut options = with_clusters();
        flowers(&mut options).fruit = FruitPreset::dogwood_bloom().fruit.clone();
        change(&mut options);
        assert_eq!(options.validate(), Err(error));
    };
    // Placement and slot settings.
    check(
        |o| fruit(o).placement.tips = 1.5,
        invalid("fruit.placement.tips", "between 0 and 1"),
    );
    check(
        |o| flowers(o).placement.scale = 0.0,
        invalid("flowers.placement.scale", "a finite value above 0"),
    );
    check(
        |o| flowers(o).placement.scale_variance = 1.5,
        invalid("flowers.placement.scale_variance", "between 0 and 1"),
    );
    check(
        |o| fruit(o).default_slots = 0,
        invalid("fruit.default_slots", "between 1 and 256"),
    );

    // Cluster settings, named under their part's `fruit` key.
    check(
        |o| fruit(o).fruit.maturity = 1.5,
        invalid("fruit.fruit.maturity", "between 0 and 1"),
    );
    check(
        |o| flowers(o).fruit.maturity = -0.1,
        invalid("flowers.fruit.maturity", "between 0 and 1"),
    );
    check(
        |o| fruit(o).fruit.maturity = f32::NAN,
        invalid("fruit.fruit.maturity", "between 0 and 1"),
    );
    check(
        |o| fruit(o).fruit.size = -1.0,
        invalid("fruit.fruit.size", "a finite value of 0 or more"),
    );
    check(
        |o| flowers(o).fruit.cluster.stalk_length = f32::INFINITY,
        invalid(
            "flowers.fruit.cluster.stalk_length",
            "a finite value of 0 or more",
        ),
    );
    check(
        |o| flowers(o).fruit.size_variance = 1.5,
        invalid("flowers.fruit.size_variance", "between 0 and 1"),
    );
    check(
        |o| fruit(o).fruit.cluster.side_stalks = 0,
        invalid("fruit.fruit.cluster.side_stalks", "at least 1"),
    );
    check(
        |o| {
            if let ItemShape::Fruit(shape) = &mut fruit(o).fruit.item {
                shape.segments = 2;
            }
        },
        invalid("fruit.fruit.item.segments", "at least 3"),
    );
    check(
        |o| {
            if let ItemShape::Fruit(shape) = &mut fruit(o).fruit.item {
                shape.lobe_depth = 1.0;
            }
        },
        invalid("fruit.fruit.item.lobe_depth", "at least 0 and below 1"),
    );
    check(
        |o| {
            if let ItemShape::Flower(shape) = &mut flowers(o).fruit.item {
                shape.petal_count = 0;
            }
        },
        invalid("flowers.fruit.item.petal_count", "at least 1"),
    );
    check(
        |o| {
            if let ItemShape::Flower(shape) = &mut flowers(o).fruit.item {
                shape.cup = 2.0;
            }
        },
        invalid("flowers.fruit.item.cup", "between 0 and 1"),
    );

    // An invalid branch depth should be reported before an invalid fruit setting.
    check(
        |o| {
            o.branch.levels = 4;
            fruit(o).fruit.maturity = 2.0;
        },
        OptionsError::TooManyLevels { levels: 4 },
    );
}

#[test]
fn invalid_setting_message_names_the_field() {
    let mut options = with_clusters();
    fruit(&mut options).fruit.cluster.droop = 3.0;
    let message = options.validate().unwrap_err().to_string();
    assert_eq!(message, "fruit.fruit.cluster.droop must be between 0 and 1");
}
