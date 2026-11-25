use super::*;
use crate::look::{GroundTint, PlantVariation};
use crate::scatter::PlantScatterOptions;

fn srgb(Srgb([red, green, blue]): Srgb) -> (f32, f32, f32) {
    (red, green, blue)
}

/// The shipped files under `assets/<folder>` whose names end in
/// `.<extension>`, as (key, text) pairs.
fn shipped(folder: &str, extension: &str) -> Vec<(String, String)> {
    let dir = format!("{}/assets/{folder}", env!("CARGO_MANIFEST_DIR"));
    let suffix = format!(".{extension}");
    let mut files: Vec<(String, String)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("{dir}: {error}"))
        .map(|entry| entry.unwrap().path())
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?.to_owned();
            let key = name.strip_suffix(&suffix)?.to_owned();
            let text = std::fs::read_to_string(&path).unwrap();
            Some((key, text))
        })
        .collect();
    files.sort();
    files
}

#[test]
fn every_plant_preset_is_shipped_as_a_file_that_reads_back_as_it() {
    let files = shipped("plants", "plant.yml");
    for preset in PlantPreset::all() {
        let (_, text) = files
            .iter()
            .find(|(key, _)| *key == preset.key)
            .unwrap_or_else(|| panic!("assets/plants has no {}.plant.yml", preset.key));
        let read = PlantPreset::from_yaml(&preset.key, text)
            .unwrap_or_else(|e| panic!("{}: {e}", preset.key));
        assert_eq!(read, *preset, "{}", preset.key);
    }
    for (key, _) in &files {
        assert!(
            PlantPreset::named(key).is_some(),
            "assets/plants/{key}.plant.yml is no built-in preset"
        );
    }
}

#[test]
fn every_scatter_preset_is_shipped_as_a_file_that_reads_back_as_it() {
    let files = shipped("scatters", "plant_scatter.yml");
    for preset in PlantScatterPreset::all() {
        let (_, text) = files
            .iter()
            .find(|(key, _)| *key == preset.key)
            .unwrap_or_else(|| panic!("assets/scatters has no {}.plant_scatter.yml", preset.key));
        let read = PlantScatterPreset::from_yaml(&preset.key, text)
            .unwrap_or_else(|e| panic!("{}: {e}", preset.key));
        assert_eq!(read, *preset, "{}", preset.key);
    }
    for (key, _) in &files {
        assert!(
            PlantScatterPreset::named(key).is_some(),
            "assets/scatters/{key}.plant_scatter.yml is no built-in preset"
        );
    }
}

#[test]
fn a_plant_is_given_by_key_or_written_inline() {
    let scatter = PlantScatterOptions::from_yaml(
        "base: summer_tall
plant_scatter:
  plants:
    - plant: seed_heads
      density_scale: 0.1
    - plant: { base: white_flowers, head_color: [0.75, 0.55, 0.95] }
      density_scale: 0.03
      clustering: 0.8
    - plant: { lean: 0.15 }
",
    )
    .unwrap();
    assert_eq!(scatter.plants.len(), 3);
    assert_eq!(scatter.plants[0].plant, PlantParams::SEED_HEADS);
    assert_eq!(scatter.plants[0].clustering, 0.0);
    assert_eq!(scatter.plants[1].clustering, 0.8);
    assert_eq!(srgb(scatter.plants[1].plant.head_color), (0.75, 0.55, 0.95));
    assert_eq!(
        scatter.plants[1].plant.height,
        PlantParams::WHITE_FLOWERS.height
    );
    // An inline plant without a base starts from meadow grass.
    assert_eq!(scatter.plants[2].plant, PlantParams::MEADOW_GRASS);
    // A density scale not given is 1.
    let scales: Vec<f32> = scatter
        .plants
        .iter()
        .map(|entry| entry.density_scale)
        .collect();
    assert_eq!(scales, vec![0.1, 0.03, 1.0]);
    assert_eq!(
        scatter.density_scale,
        PlantScatterPreset::SUMMER_TALL.plant_scatter.density_scale
    );

    // A plant equal to a preset is written as its key; any other is inline.
    let text = PlantScatterPreset::SUMMER_WILD.plant_scatter.to_yaml();
    assert!(text.contains("plant: seed_heads"), "{text}");
    assert!(text.contains("plant:\n"), "{text}");
}

#[test]
fn colours_can_be_hex_as_artists_copy_them() {
    let scatter = PlantScatterOptions::from_yaml(
        r##"plant_scatter:
  variation:
    patch_color: "#5c8f24"
  plants:
    - plant: { head_color: "#ff8000", segment: { root_color: [0.1, 0.2, 0.3] } }
"##,
    )
    .unwrap();
    let (r, g, b) = srgb(scatter.variation.patch_color);
    let near = |a: f32, b: u8| (a - f32::from(b) / 255.0).abs() < 1e-5;
    assert!(
        near(r, 0x5c) && near(g, 0x8f) && near(b, 0x24),
        "{r} {g} {b}"
    );
    assert!(near(srgb(scatter.plants[0].plant.head_color).0, 0xff));
    assert_eq!(
        srgb(scatter.plants[0].plant.segment.root_color),
        (0.1, 0.2, 0.3),
        "lists still work"
    );

    let error = PlantParams::from_yaml("plant:\n  segment:\n    tip_color: \"#5c8f2\"\n")
        .expect_err("a bad hex colour must be an error");
    assert!(matches!(error, PlantFileError::File(_)), "{error:?}");
    let message = error.to_string();
    assert!(message.contains("hex colour"), "{message}");
    assert!(message.contains("plant.segment.tip_color"), "{message}");
}

/// Distinct segment values, offset by `k`, expose settings lost during serialisation.
fn odd_segment(k: f32) -> PlantSegment {
    PlantSegment {
        profile: 0.41 + k,
        stiffness: 0.13 + k,
        droop: 0.18 + k,
        twist: 0.17 + k,
        fold: 0.19 + k,
        root_color: Srgb::new(0.1 + k, 0.2, 0.3),
        tip_color: Srgb::new(0.4 + k, 0.5, 0.6),
    }
}

/// A plant whose every setting differs from every preset's.
fn odd_plant() -> PlantParams {
    let level = |k: f32, arrangement| PlantBranches {
        count: 7 + k as u32,
        arrangement,
        attachment_start: 0.21 + k,
        length: 0.22 + k,
        length_envelope: 0.23 + k,
        width: 0.24 + k,
        angle: 0.25 + k,
        spread: 0.26 + k,
        segment: odd_segment(0.02 + k),
    };
    PlantParams {
        default_seed: 77,
        density: 0.1,
        height: Vec2::new(0.11, 0.22),
        width: 0.011,
        lean: 0.12,
        stems_per_plant: 3,
        segment: odd_segment(0.0),
        levels: [
            level(0.0, PlantArrangement::Alternate),
            level(1.0 / 64.0, PlantArrangement::Opposite),
        ],
        root_occlusion: 0.14,
        translucency: 0.15,
        specular: 0.16,
        // Disable the head on branched plants while preserving its width and colour
        // settings.
        head_length: 0.0,
        head_width: 0.023,
        head_color: Srgb::new(0.7, 0.8, 0.9),
        tufts: 0.38,
        tuft_size: 0.39,
        accent: PlantAccent {
            share: 0.41,
            height: Vec2::new(0.42, 0.43),
            width: 0.044,
            lean: 0.45,
            translucency: 0.46,
            segment: odd_segment(0.03),
            head_length: 0.47,
            head_width: 0.048,
            head_color: Srgb::new(0.49, 0.5, 0.51),
        },
    }
}

/// A Scatter whose every setting differs from every preset's.
fn odd_scatter() -> PlantScatterOptions {
    PlantScatterOptions {
        plants: Cow::Owned(vec![
            ScatterPlant::new(PlantParams::CLOVER)
                .scaled(0.4)
                .clustered(0.41),
            ScatterPlant::new(odd_plant()).scaled(0.6),
        ]),
        default_seed: 91,
        wind_scale: 0.42,
        density_scale: 1.2,
        toon_bands: 3,
        max_slope: 33.0,
        ground_tint: GroundTint {
            color: Srgb::new(0.9, 0.8, 0.7),
            amount: 0.31,
        },
        variation: PlantVariation {
            patch_color: Srgb::new(0.3, 0.2, 0.1),
            patch_amount: 0.32,
            patch_size: 7.0,
            dry_tips: 0.33,
            dead_share: 0.34,
            bare_share: 0.37,
            bare_patch_size: 5.0,
        },
    }
}

#[test]
fn the_format_holds_every_setting_of_a_plant_and_a_scatter() {
    // Changing every setting before a round trip catches fields missing from the file
    // format, including nested plant, segment and branch settings.
    let plant = odd_plant();
    let text = plant.to_yaml();
    assert_eq!(PlantParams::from_yaml(&text).unwrap(), plant, "{text}");
    assert!(text.contains("alternate"), "arrangements by name: {text}");
    assert!(text.contains("default_seed: 77"), "{text}");

    let scatter = odd_scatter();
    let text = scatter.to_yaml();
    assert_eq!(
        PlantScatterOptions::from_yaml(&text).unwrap(),
        scatter,
        "{text}"
    );
    assert!(text.contains("default_seed: 91"), "{text}");
}

#[test]
fn serde_reads_and_writes_the_settings_a_file_holds() {
    // Through serde, the options read back equal, with the same layout as
    // the settings under a file's kind.
    let plant = odd_plant();
    let value = to_value(&plant);
    assert_eq!(from_value::<PlantParams>(value.clone(), "").unwrap(), plant);
    assert_eq!(value["default_seed"], 77);
    let scatter = odd_scatter();
    assert_eq!(
        from_value::<PlantScatterOptions>(to_value(&scatter), "").unwrap(),
        scatter
    );

    // Missing settings come from the default.
    let read: PlantScatterOptions =
        from_value(serde_json::json!({ "default_seed": 5 }), "").unwrap();
    assert_eq!(
        read,
        PlantScatterOptions {
            default_seed: 5,
            ..PlantScatterOptions::default()
        }
    );

    // Unknown settings and invalid values are errors, as in a file.
    assert!(from_value::<PlantParams>(serde_json::json!({ "hieght": [0.1, 0.2] }), "").is_err());
    assert!(from_value::<PlantParams>(serde_json::json!({ "stems_per_plant": 0 }), "").is_err());
    assert!(from_value::<PlantSegment>(serde_json::json!({ "profile": 2.0 }), "").is_err());
}

#[test]
fn an_accent_names_only_what_differs() {
    let plant = PlantParams::from_yaml(
        "base: meadow_grass
plant:
  accent:
    share: 0.1
    height: [0.9, 1.2]
    head_length: 0.2
",
    )
    .unwrap();
    assert_eq!(plant.accent.share, 0.1);
    assert_eq!(plant.accent.height, Vec2::new(0.9, 1.2));
    assert_eq!(plant.accent.head_length, 0.2);
    assert_eq!(plant.accent.width, PlantAccent::NONE.width);
    assert_eq!(plant.accent.segment, PlantAccent::NONE.segment);

    // Written over a base, a plant names its accent only when it differs.
    let grass = PlantPreset::MEADOW_GRASS;
    let accented = PlantPreset {
        plant,
        ..grass.clone()
    };
    let lawn = PlantPreset::LAWN_GRASS;
    assert!(!lawn.to_yaml(Some(&grass)).contains("accent"));
    let text = accented.to_yaml(Some(&grass));
    assert!(text.contains("accent"), "{text}");
    assert!(!text.contains("width"), "only what differs: {text}");
    assert_eq!(
        PlantPreset::from_yaml("meadow_grass", &text).unwrap(),
        accented
    );
    let pampas = &PlantScatterPreset::PAMPAS.plant_scatter.plants[0].plant;
    assert!(pampas.accent.is_on());
    assert_eq!(PlantParams::from_yaml(&pampas.to_yaml()).unwrap(), *pampas);
}

#[test]
fn a_variant_names_only_what_differs() {
    let scatter =
        PlantScatterOptions::from_yaml("base: autumn_tall\nplant_scatter:\n  density_scale: 0.5\n")
            .unwrap();
    assert_eq!(scatter.density_scale, 0.5);
    assert_eq!(
        scatter.plants,
        PlantScatterPreset::AUTUMN_TALL.plant_scatter.plants
    );
    assert_eq!(
        scatter.variation,
        PlantScatterPreset::AUTUMN_TALL.plant_scatter.variation
    );

    // Nested settings merge one by one.
    let scatter = PlantScatterOptions::from_yaml(
        "base: autumn_tall\nplant_scatter:\n  variation:\n    dry_tips: 0.9\n",
    )
    .unwrap();
    let mut expected = PlantScatterPreset::AUTUMN_TALL.plant_scatter;
    expected.variation.dry_tips = 0.9;
    assert_eq!(scatter, expected);

    let plant = PlantParams::from_yaml(
        "base: fern\nplant:\n  segment:\n    tip_color: [0.70, 0.72, 0.74]\n",
    )
    .unwrap();
    assert_eq!(plant.height, PlantParams::FERN.height);
    assert_eq!(plant.segment.stiffness, PlantParams::FERN.segment.stiffness);
    assert_eq!(plant.levels, PlantParams::FERN.levels);
    assert_eq!(srgb(plant.segment.tip_color), (0.70, 0.72, 0.74));
}

#[test]
fn a_plants_list_replaces_the_base_list_whole() {
    let scatter = PlantScatterOptions::from_yaml(
        "base: summer_wild\nplant_scatter:\n  plants:\n    - plant: clover\n",
    )
    .unwrap();
    assert_eq!(scatter.plants.len(), 1);
    assert_eq!(scatter.plants[0], ScatterPlant::new(PlantParams::CLOVER));
}

#[test]
fn a_level_names_only_what_differs_from_its_base_level() {
    // Partial first-level overrides must preserve unspecified values from the base.
    let plant = PlantParams::from_yaml(
        "plant:
  levels:
    - count: 12
      arrangement: alternate
      segment: { profile: 0.8 }
",
    )
    .unwrap();
    let [first, second] = plant.levels;
    assert_eq!(first.count, 12);
    assert_eq!(first.arrangement, PlantArrangement::Alternate);
    assert_eq!(first.segment.profile, 0.8);
    assert_eq!(first.length, PlantBranches::NONE.length);
    assert_eq!(
        first.segment.stiffness,
        PlantBranches::NONE.segment.stiffness
    );
    assert_eq!(
        second,
        PlantBranches::NONE,
        "a level not given is the base's"
    );
}

/// Reads `settings` as the settings of a plant file.
fn plant(settings: &str) -> Result<PlantParams, PlantFileError> {
    PlantParams::from_yaml(&format!("plant: {settings}\n"))
}

/// Reads `settings` as the settings of a Scatter file.
fn scatter(settings: &str) -> Result<PlantScatterOptions, PlantFileError> {
    PlantScatterOptions::from_yaml(&format!("plant_scatter: {settings}\n"))
}

#[test]
fn mistakes_are_reported() {
    assert!(matches!(
        PlantScatterOptions::from_yaml("base: not_a_preset"),
        Err(PlantFileError::UnknownScatter(_))
    ));
    assert!(matches!(
        PlantParams::from_yaml("base: not_a_preset"),
        Err(PlantFileError::UnknownPlant(_))
    ));
    assert!(matches!(
        scatter("{ plants: [{ plant: not_a_plant }] }"),
        Err(PlantFileError::UnknownPlant(_))
    ));
    assert!(matches!(
        scatter("{ plants: [{ plant: { base: meadow } }] }"),
        Err(PlantFileError::UnknownPlant(_))
    ));
    // Keys are lower case, with no other spellings.
    assert!(matches!(
        PlantParams::from_yaml("base: FERN"),
        Err(PlantFileError::UnknownPlant(_))
    ));
    assert!(matches!(
        plant("{ hieght: [0.1, 0.2] }"),
        Err(PlantFileError::File(_))
    ));
    // The base goes at the top of the file, not among the settings.
    assert!(matches!(
        plant("{ base: fern }"),
        Err(PlantFileError::File(_))
    ));
    for invalid in [
        "{ height: [-0.1, 0.2] }",
        "{ height: [0.0, 0.0] }",
        "{ stems_per_plant: 0 }",
        "{ stems_per_plant: 9 }",
        "{ tufts: 1.5 }",
        "{ tuft_size: 0.0 }",
        "{ segment: { profile: 1.5 } }",
        "{ levels: [{ count: 33 }] }",
        "{ levels: [{ count: 4, attachment_start: 1.0 }] }",
        "{ levels: [{ count: 4, angle: 1.2 }] }",
        "{ levels: [{ count: 4, spread: -0.1 }] }",
        "{ levels: [{ count: 4, length: -0.1 }] }",
        "{ levels: [{ count: 4, segment: { profile: -0.5 } }] }",
        "{ levels: [{ count: 4 }, { count: 3 }, { count: 2 }] }",
    ] {
        assert!(
            matches!(plant(invalid), Err(PlantFileError::Invalid(_))),
            "{invalid}"
        );
    }
    assert!(plant("{ levels: [{ count: 4 }, { count: 3 }] }").is_ok());
    for invalid in [
        "{ accent: { share: 1.5 } }",
        "{ accent: { share: 0.1, height: [0.0, 0.0] } }",
        "{ accent: { share: 0.1, width: 0.0 } }",
        "{ accent: { share: 0.1, head_length: 0.95 } }",
        "{ accent: { share: 0.1, segment: { profile: 2.0 } } }",
    ] {
        assert!(
            matches!(plant(invalid), Err(PlantFileError::Invalid(_))),
            "{invalid}"
        );
    }

    let seventeen = format!(
        "{{ plants: [{}] }}",
        "{ plant: clover }, ".repeat(MAX_PLANTS + 1)
    );
    for invalid in [
        "{ plants: [] }",
        "{ plants: [{ plant: clover, density_scale: 0.0 }] }",
        "{ plants: [{ plant: clover, density_scale: -1.0 }] }",
        "{ plants: [{ plant: { base: clover, density: 0.0 } }] }",
        "{ plants: [{ plant: { base: clover, density: -1.0 } }] }",
        "{ plants: [{ plant: clover, clustering: 1.5 }] }",
        "{ plants: [{ plant: { height: [-2.0, 1.0] } }] }",
        "{ density_scale: -1.0 }",
        "{ wind_scale: -1.0 }",
        "{ variation: { patch_size: 0.0 } }",
        &seventeen,
    ] {
        assert!(
            matches!(scatter(invalid), Err(PlantFileError::Invalid(_))),
            "{invalid}"
        );
    }
}

#[test]
fn an_unknown_key_lists_the_valid_ones() {
    let plant = scatter("{ plants: [{ plant: ferns }] }")
        .expect_err("an unknown plant key")
        .to_string();
    assert!(
        plant.contains("\"ferns\"") && plant.contains("fern,"),
        "{plant}"
    );
    assert!(plant.contains("meadow_grass"), "{plant}");
    let scatter = PlantScatterOptions::from_yaml("base: meadows")
        .expect_err("an unknown Scatter key")
        .to_string();
    assert!(
        scatter.contains("\"meadows\"") && scatter.contains("summer_wild"),
        "{scatter}"
    );
}

#[test]
fn settings_the_plant_does_not_draw_are_kept_not_rejected() {
    // Saving and loading must preserve valid settings even when they currently
    // have no visible effect or the height limits are written in reverse order.
    for (text, check) in [
        // Keep the height limits in the order supplied.
        (
            "{ height: [0.5, 0.2] }",
            (|p: &PlantParams| p.height == Vec2::new(0.5, 0.2)) as fn(&PlantParams) -> bool,
        ),
        // Keep second-layer branch settings while the first layer is disabled.
        ("{ levels: [{ count: 0 }, { count: 3 }] }", |p| {
            p.levels[1].count == 3
        }),
        // Keep the head length while branches hide the head.
        ("{ head_length: 0.2, levels: [{ count: 4 }] }", |p| {
            p.head_length == 0.2
        }),
        // Keep the tuft size while the plant has no tufts.
        ("{ tufts: 0.0, tuft_size: 2.5 }", |p| p.tuft_size == 2.5),
    ] {
        let read = plant(text).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert!(check(&read), "{text}");
        assert_eq!(
            PlantParams::from_yaml(&read.to_yaml()).unwrap(),
            read,
            "{text}"
        );
    }
}

#[test]
fn an_error_says_where_and_why() {
    // A misspelt setting is named by its path in the file.
    let error = PlantScatterOptions::from_yaml(
        "plant_scatter:\n  density_scale: 2.0\n  variation:\n    dry_tip: 3.0\n",
    )
    .expect_err("a misspelt setting must be an error");
    let message = error.to_string();
    assert!(
        message.starts_with("not a grass file: plant_scatter.variation"),
        "{message}"
    );
    assert!(message.contains("dry_tip"), "{message}");

    // Text that isn't YAML is reported with its line and column.
    let error = PlantScatterOptions::from_yaml(
        "plant_scatter:\n  density_scale: 2.0\n   wind_scale: 1.0\n",
    )
    .expect_err("broken YAML must be an error");
    assert!(
        matches!(error, PlantFileError::File(PresetFileError::Yaml { .. })),
        "{error:?}"
    );
    assert!(error.to_string().contains("line 3 column 14"), "{error}");

    // So is a top-level key the file can't have.
    let error = PlantScatterOptions::from_yaml("plant: {}\n").expect_err("the wrong kind");
    assert!(error.to_string().contains("plant_scatter"), "{error}");
}

#[test]
fn a_plant_in_a_scatter_is_a_plant_and_a_file_cannot_say_otherwise() {
    // Reject Scatter settings inside plant settings rather than silently
    // ignoring them. Rust keeps these settings on separate types too.
    let error = scatter("{ plants: [{ plant: { base: clover, density_scale: 0.5 } }] }")
        .expect_err("Scatter settings in a plant must be an error");
    assert!(matches!(error, PlantFileError::File(_)), "{error:?}");
    assert!(error.to_string().contains("density_scale"), "{error}");

    // Nested plant lists are the same mistake, and so are tufts on the Scatter,
    // and a plant's own density written next to it instead of inside it.
    for wrong in [
        "{ plants: [{ plant: { plants: [] } }] }",
        "{ tufts: 0.3 }",
        "{ plants: [{ plant: clover, density: 3.0 }] }",
    ] {
        assert!(
            matches!(scatter(wrong), Err(PlantFileError::File(_))),
            "{wrong}"
        );
    }
}

#[test]
fn every_preset_reads_back_from_a_file_with_or_without_a_base() {
    for preset in PlantScatterPreset::all() {
        for base in [None, Some(&PlantScatterPreset::MEADOW)] {
            let text = preset.to_yaml(base);
            let read = PlantScatterPreset::from_yaml(&preset.key, &text)
                .unwrap_or_else(|e| panic!("{}: {e}\n{text}", preset.key));
            assert_eq!(read, *preset, "{}\n{text}", preset.key);
        }
        // A plant holds plant settings, without Scatter-wide settings. (A
        // plant's own density belongs to it, and each entry has a
        // `density_scale` of its own, so neither is a Scatter-wide setting.)
        let text = preset.to_yaml(None);
        let plants = text.split("plants:").nth(1).unwrap_or("");
        for scatter_setting in ["wind_scale", "ground_tint", "toon_bands"] {
            assert!(
                !plants.contains(scatter_setting),
                "{}: {scatter_setting}",
                preset.key
            );
        }
    }
    for preset in PlantPreset::all() {
        for base in [None, Some(&PlantPreset::FERN)] {
            let text = preset.to_yaml(base);
            let read = PlantPreset::from_yaml(&preset.key, &text)
                .unwrap_or_else(|e| panic!("{}: {e}\n{text}", preset.key));
            assert_eq!(read, *preset, "{}\n{text}", preset.key);
        }
    }
}

#[test]
fn a_preset_file_keeps_its_name_description_and_group() {
    let plant = PlantPreset::from_yaml(
        "blue_fern",
        "base: fern\nname: Blue fern\ndescription: A fern, but blue.\n",
    )
    .unwrap();
    assert_eq!(plant.key, "blue_fern");
    assert_eq!(plant.name, "Blue fern");
    assert_eq!(plant.description, "A fern, but blue.");
    assert_eq!(plant.group, FILE_GROUP);
    assert_eq!(plant.plant, PlantParams::FERN);
    let again = PlantPreset::from_yaml("blue_fern", &plant.to_yaml(None)).unwrap();
    assert_eq!(again, plant);
    let grouped = PlantPreset::from_yaml("blue_fern", "group: My ferns\n").unwrap();
    assert_eq!(grouped.group, "My ferns");

    // Without a name, the key names it; an empty file is the default Scatter.
    let scatter = PlantScatterPreset::from_yaml("my_scatter", "").unwrap();
    assert_eq!(scatter.name, "my_scatter");
    assert_eq!(scatter.description, "");
    assert_eq!(
        scatter.plant_scatter,
        PlantScatterPreset::MEADOW.plant_scatter
    );
    let again = PlantScatterPreset::from_yaml(
        "my_scatter",
        &PlantScatterPreset::FOREST_FLOOR.to_yaml(None),
    )
    .unwrap();
    assert_eq!(again.name, PlantScatterPreset::FOREST_FLOOR.name);
    assert_eq!(again.group, PlantScatterPreset::FOREST_FLOOR.group);
    assert_eq!(
        again.plant_scatter,
        PlantScatterPreset::FOREST_FLOOR.plant_scatter
    );

    // The plain readers ignore the name.
    assert_eq!(
        PlantParams::from_yaml(&PlantPreset::FERN.to_yaml(None)).unwrap(),
        PlantParams::FERN
    );
}

#[test]
fn the_default_seed_is_a_setting_like_any_other() {
    let plant = PlantParams::from_yaml("base: fern\nplant:\n  default_seed: 12\n").unwrap();
    assert_eq!(plant.default_seed, 12);
    assert_eq!(
        PlantParams {
            default_seed: 0,
            ..plant
        },
        PlantParams::FERN
    );
    let scatter =
        PlantScatterOptions::from_yaml("base: lawn\nplant_scatter:\n  default_seed: 3\n").unwrap();
    assert_eq!(scatter.default_seed, 3);
}

/// A registry of plants and plant Scatters.
fn registry() -> Registry {
    let mut registry = Registry::new();
    registry
        .register::<PlantPreset>()
        .register::<PlantScatterPreset>();
    registry
}

#[test]
fn shipped_files_read_the_same_through_a_registry() {
    let registry = registry();
    for (key, text) in shipped("plants", "plant.yml") {
        let read: PlantPreset = registry.read(&key, &text).unwrap();
        assert_eq!(read, PlantPreset::from_yaml(&key, &text).unwrap(), "{key}");
    }
    for (key, text) in shipped("scatters", "plant_scatter.yml") {
        let read: PlantScatterPreset = registry.read(&key, &text).unwrap();
        assert_eq!(
            read,
            PlantScatterPreset::from_yaml(&key, &text).unwrap(),
            "{key}"
        );
    }
}

#[test]
fn a_project_scatter_uses_a_project_plant() {
    let mut registry = registry();
    registry
        .add(
            "plant",
            "tall_fern",
            "base: fern\nplant:\n  default_seed: 77\n",
            None,
        )
        .unwrap();
    registry
        .add(
            "plant_scatter",
            "fern_bank",
            "base: meadow\nplant_scatter:\n  plants:\n    - plant: tall_fern\n      density_scale: 0.5\n",
            None,
        )
        .unwrap();
    let fern = registry.resolve::<PlantPreset>("tall_fern").unwrap();
    assert_eq!(fern.plant.default_seed, 77);
    let bank = registry.resolve::<PlantScatterPreset>("fern_bank").unwrap();
    assert_eq!(bank.plant_scatter.plants[0].plant, fern.plant);
    assert_eq!(bank.plant_scatter.plants[0].density_scale, 0.5);
    let uses = registry.used_by("plant", procgena_core::preset::Layer::Project, "tall_fern");
    assert_eq!(uses[0].path.as_deref(), Some("plants.0.plant"));
}
