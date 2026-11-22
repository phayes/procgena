use super::*;
use crate::api::{GroundTint, PlantVariation};

fn srgb(color: Color) -> (f32, f32, f32) {
    let c = color.to_srgba();
    (c.red, c.green, c.blue)
}

/// Finds the shipped preset under `assets/plants` or `assets/fields`.
fn shipped_path(folder: &str, key: &str, extension: &str) -> String {
    format!(
        "{}/assets/{folder}/{}.{extension}",
        env!("CARGO_MANIFEST_DIR"),
        key.to_ascii_lowercase()
    )
}

/// The comment at the top of a shipped file.
fn shipped_doc(kind: &str, key: &str, extension: &str) -> String {
    format!(
        "// {kind}::{key}, as a file. Copy it, edit it, and load it with\n\
         // `asset_server.load(\"path/to/copy.{extension}\")`. Every setting is\n\
         // optional: a missing one comes from `base`, a preset's key.\n"
    )
}

/// Reads a shipped preset file for comparison with its constant.
/// With `BLESS_PRESETS` set, writes the current constant first, creating the
/// file if needed. Without it, a missing file fails the test.
fn shipped(path: &str, text: impl FnOnce() -> String) -> String {
    if std::env::var_os("BLESS_PRESETS").is_some() {
        let folder = Path::new(path).parent().expect("a file in a folder");
        std::fs::create_dir_all(folder).unwrap();
        std::fs::write(path, text()).unwrap();
    }
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

#[test]
fn every_shipped_plant_file_matches_its_constant() {
    // `BLESS_PRESETS=1 cargo test` rewrites the files from the constants.
    for preset in PlantPreset::all() {
        let path = shipped_path("plants", &preset.key, "plant.ron");
        let text = shipped(&path, || {
            format!(
                "{}{}\n",
                shipped_doc("PlantPreset", &preset.key, "plant.ron"),
                preset.to_ron()
            )
        });
        let read = PlantPreset::from_ron(&preset.key.to_ascii_lowercase(), &text)
            .unwrap_or_else(|e| panic!("{}: {e}", preset.key));
        assert_eq!(read.plant, preset.plant, "{}", preset.key);
        assert_eq!(read.name, preset.name, "{}", preset.key);
        assert_eq!(read.description, preset.description, "{}", preset.key);
    }
}

#[test]
fn every_shipped_field_file_matches_its_constant() {
    // `BLESS_PRESETS=1 cargo test` rewrites the files from the constants.
    for preset in FieldPreset::all() {
        let path = shipped_path("fields", &preset.key, "field.ron");
        let text = shipped(&path, || {
            format!(
                "{}{}\n",
                shipped_doc("FieldPreset", &preset.key, "field.ron"),
                preset.to_ron()
            )
        });
        let read = FieldPreset::from_ron(&preset.key.to_ascii_lowercase(), &text)
            .unwrap_or_else(|e| panic!("{}: {e}", preset.key));
        assert_eq!(read.field, preset.field, "{}", preset.key);
        assert_eq!(read.name, preset.name, "{}", preset.key);
        assert_eq!(read.description, preset.description, "{}", preset.key);
    }
}

#[test]
fn every_preset_is_shipped_as_an_embedded_file_that_is_the_preset() {
    use bevy::MinimalPlugins;
    use bevy::asset::{AssetPlugin, AssetServer, Assets, Handle};

    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<PlantPreset>()
        .init_asset::<FieldPreset>()
        .add_plugins(PlantFilePlugin);
    let (plants, fields): (Vec<Handle<PlantPreset>>, Vec<Handle<FieldPreset>>) = {
        let server = app.world().resource::<AssetServer>();
        let plants = PlantPreset::all()
            .map(|preset| {
                server.load(format!(
                    "embedded://procgena_plant/plants/{}.plant.ron",
                    preset.key.to_ascii_lowercase()
                ))
            })
            .collect();
        let fields = FieldPreset::all()
            .map(|preset| {
                server.load(format!(
                    "embedded://procgena_plant/fields/{}.field.ron",
                    preset.key.to_ascii_lowercase()
                ))
            })
            .collect();
        (plants, fields)
    };
    let loaded = |app: &App| {
        let plant_assets = app.world().resource::<Assets<PlantPreset>>();
        let field_assets = app.world().resource::<Assets<FieldPreset>>();
        plants
            .iter()
            .all(|handle| plant_assets.get(handle).is_some())
            && fields
                .iter()
                .all(|handle| field_assets.get(handle).is_some())
    };
    for _ in 0..500 {
        app.update();
        if loaded(&app) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let plant_assets = app.world().resource::<Assets<PlantPreset>>();
    for (preset, handle) in PlantPreset::all().zip(&plants) {
        let loaded = plant_assets
            .get(handle)
            .unwrap_or_else(|| panic!("{} did not load", preset.key));
        assert_eq!(loaded.plant, preset.plant, "{}", preset.key);
        assert_eq!(loaded.name, preset.name, "{}", preset.key);
        // A loaded file's key is its file stem, and its group is "File".
        assert_eq!(loaded.key, preset.key.to_ascii_lowercase());
        assert_eq!(loaded.group, FILE_GROUP);
    }
    let field_assets = app.world().resource::<Assets<FieldPreset>>();
    for (preset, handle) in FieldPreset::all().zip(&fields) {
        let loaded = field_assets
            .get(handle)
            .unwrap_or_else(|| panic!("{} did not load", preset.key));
        assert_eq!(loaded.field, preset.field, "{}", preset.key);
        assert_eq!(loaded.description, preset.description, "{}", preset.key);
        assert_eq!(loaded.key, preset.key.to_ascii_lowercase());
        assert_eq!(loaded.group, FILE_GROUP);
    }
}

#[test]
fn a_plant_is_given_by_key_or_written_inline() {
    let field = PlantField::from_ron(
        r#"(
            base: "SUMMER_TALL",
            plants: [
                (plant: "SEED_HEADS", density_scale: 0.1),
                (plant: (base: "WHITE_FLOWERS", head_color: (0.75, 0.55, 0.95)),
                 density_scale: 0.03, clustering: 0.8),
                (plant: (lean: 0.15)),
            ],
        )"#,
    )
    .unwrap();
    assert_eq!(field.plants.len(), 3);
    assert_eq!(field.plants[0].plant, PlantParams::SEED_HEADS);
    assert_eq!(field.plants[0].clustering, 0.0);
    assert_eq!(field.plants[1].clustering, 0.8);
    assert_eq!(srgb(field.plants[1].plant.head_color), (0.75, 0.55, 0.95));
    assert_eq!(
        field.plants[1].plant.height,
        PlantParams::WHITE_FLOWERS.height
    );
    // An inline plant without a base starts from meadow grass.
    assert_eq!(field.plants[2].plant, PlantParams::MEADOW_GRASS);
    // A density scale not given is 1.
    let scales: Vec<f32> = field
        .plants
        .iter()
        .map(|entry| entry.density_scale)
        .collect();
    assert_eq!(scales, vec![0.1, 0.03, 1.0]);
    assert_eq!(
        field.density_scale,
        FieldPreset::SUMMER_TALL.field.density_scale
    );

    // The same in JSON.
    let json = PlantField::from_json(
        r#"{"plants": [{"plant": "FERN", "density_scale": 0.5},
                       {"plant": {"base": "MOSS", "lean": 0.1, "density": 80.0}}]}"#,
    )
    .unwrap();
    assert_eq!(json.plants[0].plant, PlantParams::FERN);
    assert_eq!(json.plants[1].plant.lean, 0.1);
    assert_eq!(json.plants[1].plant.height, PlantParams::MOSS.height);
    assert_eq!(json.plants[0].density_scale, 0.5);
    assert_eq!(json.plants[1].plant.density, 80.0);

    // A plant equal to a preset is written as its key; any other is inline.
    let text = FieldPreset::SUMMER_WILD.field.to_ron();
    assert!(text.contains("plant: \"SEED_HEADS\""), "{text}");
    assert!(text.contains("plant: ("), "{text}");
    let json = FieldPreset::SUMMER_WILD.field.to_json();
    assert!(json.contains("\"plant\": \"SEED_HEADS\""), "{json}");
}

#[test]
fn colours_can_be_hex_as_artists_copy_them() {
    let field = PlantField::from_ron(
        r##"(patch_color: "#5c8f24",
             plants: [(plant: (head_color: "#ff8000", segment: (root_color: (0.1, 0.2, 0.3))))])"##,
    )
    .unwrap();
    let (r, g, b) = srgb(field.variation.patch_color);
    let near = |a: f32, b: u8| (a - f32::from(b) / 255.0).abs() < 1e-5;
    assert!(
        near(r, 0x5c) && near(g, 0x8f) && near(b, 0x24),
        "{r} {g} {b}"
    );
    assert!(near(srgb(field.plants[0].plant.head_color).0, 0xff));
    assert_eq!(
        srgb(field.plants[0].plant.segment.root_color),
        (0.1, 0.2, 0.3),
        "tuples still work"
    );

    let Err(PlantFileError::Parse { message, .. }) =
        PlantParams::from_ron(r##"(segment: (tip_color: "#5c8f2"))"##)
    else {
        panic!("a bad hex colour must be an error");
    };
    assert!(message.contains("hex colour"), "{message}");
}

/// Distinct segment values, offset by `k`, expose settings lost during serialisation.
fn odd_segment(k: f32) -> PlantSegment {
    PlantSegment {
        profile: 0.41 + k,
        stiffness: 0.13 + k,
        droop: 0.18 + k,
        twist: 0.17 + k,
        fold: 0.19 + k,
        root_color: Color::srgb(0.1 + k, 0.2, 0.3),
        tip_color: Color::srgb(0.4 + k, 0.5, 0.6),
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
        head_color: Color::srgb(0.7, 0.8, 0.9),
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
            head_color: Color::srgb(0.49, 0.5, 0.51),
        },
    }
}

#[test]
fn the_formats_hold_every_setting_of_a_plant_and_a_field() {
    // Changing every setting before a round trip catches fields missing from the file
    // format, including nested plant, segment and branch settings.
    let plant = odd_plant();
    let text = plant.to_ron();
    assert_eq!(PlantParams::from_ron(&text).unwrap(), plant, "{text}");
    assert_eq!(PlantParams::from_json(&plant.to_json()).unwrap(), plant);
    assert!(text.contains("Alternate"), "arrangements by name: {text}");

    let field = PlantField {
        plants: Cow::Owned(vec![
            FieldPlant::new(PlantParams::CLOVER)
                .scaled(0.4)
                .clustered(0.41),
            FieldPlant::new(plant).scaled(0.6),
        ]),
        wind_scale: 0.42,
        density_scale: 1.2,
        toon_bands: 3,
        ground_tint: GroundTint {
            color: Color::srgb(0.9, 0.8, 0.7),
            amount: 0.31,
        },
        variation: PlantVariation {
            patch_color: Color::srgb(0.3, 0.2, 0.1),
            patch_amount: 0.32,
            patch_size: 7.0,
            dry_tips: 0.33,
            dead_share: 0.34,
            bare_share: 0.37,
            bare_patch_size: 5.0,
        },
        ..FieldPreset::SUMMER_WILD.field
    };
    let text = field.to_ron();
    assert_eq!(PlantField::from_ron(&text).unwrap(), field, "{text}");
    assert_eq!(PlantField::from_json(&field.to_json()).unwrap(), field);
}

#[test]
fn an_accent_names_only_what_differs_and_is_left_out_when_off() {
    let plant = PlantParams::from_ron(
        r#"(base: "MEADOW_GRASS", accent: (share: 0.1, height: (0.9, 1.2), head_length: 0.2))"#,
    )
    .unwrap();
    assert_eq!(plant.accent.share, 0.1);
    assert_eq!(plant.accent.height, Vec2::new(0.9, 1.2));
    assert_eq!(plant.accent.head_length, 0.2);
    assert_eq!(plant.accent.width, PlantAccent::NONE.width);
    assert_eq!(plant.accent.segment, PlantAccent::NONE.segment);
    // A plant without an accent writes none; one with an accent writes it.
    assert!(!PlantParams::MEADOW_GRASS.to_ron().contains("accent"));
    assert!(plant.to_ron().contains("accent"));
    let pampas = &FieldPreset::PAMPAS.field.plants[0].plant;
    assert!(pampas.accent.is_on());
    assert_eq!(PlantParams::from_ron(&pampas.to_ron()).unwrap(), *pampas);
}

#[test]
fn a_file_holds_no_size_ground_or_map() {
    // Field files store appearance only; the game supplies size and placement.
    let field = PlantField::from_preset(&FieldPreset::MEADOW, Vec2::splat(64.0));
    let read = PlantField::from_ron(&field.to_ron()).unwrap();
    assert_eq!(read.size, Vec2::ZERO);
    assert_eq!(read, FieldPreset::MEADOW.field);
}

#[test]
fn a_variant_names_only_what_differs() {
    let field = PlantField::from_ron(r#"(base: "AUTUMN_TALL", density_scale: 0.5)"#).unwrap();
    assert_eq!(field.density_scale, 0.5);
    assert_eq!(field.plants, FieldPreset::AUTUMN_TALL.field.plants);
    assert_eq!(field.variation, FieldPreset::AUTUMN_TALL.field.variation);

    let plant =
        PlantParams::from_ron(r#"(base: "FERN", segment: (tip_color: (0.70, 0.72, 0.74)))"#)
            .unwrap();
    assert_eq!(plant.height, PlantParams::FERN.height);
    assert_eq!(plant.segment.stiffness, PlantParams::FERN.segment.stiffness);
    assert_eq!(plant.levels, PlantParams::FERN.levels);
    assert_eq!(srgb(plant.segment.tip_color), (0.70, 0.72, 0.74));
}

#[test]
fn a_level_names_only_what_differs_from_its_base_level() {
    // Partial first-level overrides must preserve unspecified values from the base.
    let plant = PlantParams::from_ron(
        r"(levels: [(count: 12, arrangement: Alternate, segment: (profile: 0.8))])",
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

#[test]
fn mistakes_are_reported() {
    assert!(matches!(
        PlantField::from_ron("(base: \"NOT_A_PRESET\")"),
        Err(PlantFileError::UnknownField(_))
    ));
    assert!(matches!(
        PlantParams::from_ron("(base: \"NOT_A_PRESET\")"),
        Err(PlantFileError::UnknownPlant(_))
    ));
    assert!(matches!(
        PlantField::from_ron("(plants: [(plant: \"NOT_A_PLANT\")])"),
        Err(PlantFileError::UnknownPlant(_))
    ));
    assert!(matches!(
        PlantField::from_ron("(plants: [(plant: (base: \"MEADOW\"))])"),
        Err(PlantFileError::UnknownPlant(_))
    ));
    assert!(matches!(
        PlantParams::from_ron("(hieght: (0.1, 0.2))"),
        Err(PlantFileError::Parse { .. })
    ));
    for invalid in [
        "(height: (-0.1, 0.2))",
        "(height: (0.0, 0.0))",
        "(stems_per_plant: 0)",
        "(stems_per_plant: 9)",
        "(tufts: 1.5)",
        "(tuft_size: 0.0)",
        "(segment: (profile: 1.5))",
        "(levels: [(count: 33)])",
        "(levels: [(count: 4, attachment_start: 1.0)])",
        "(levels: [(count: 4, angle: 1.2)])",
        "(levels: [(count: 4, spread: -0.1)])",
        "(levels: [(count: 4, length: -0.1)])",
        "(levels: [(count: 4, segment: (profile: -0.5))])",
        "(levels: [(count: 4), (count: 3), (count: 2)])",
    ] {
        assert!(
            matches!(
                PlantParams::from_ron(invalid),
                Err(PlantFileError::Invalid(_))
            ),
            "{invalid}"
        );
    }
    assert!(PlantParams::from_ron("(levels: [(count: 4), (count: 3)])").is_ok());
    for invalid in [
        "(accent: (share: 1.5))",
        "(accent: (share: 0.1, height: (0.0, 0.0)))",
        "(accent: (share: 0.1, width: 0.0))",
        "(accent: (share: 0.1, head_length: 0.95))",
        "(accent: (share: 0.1, segment: (profile: 2.0)))",
    ] {
        assert!(
            matches!(
                PlantParams::from_ron(invalid),
                Err(PlantFileError::Invalid(_))
            ),
            "{invalid}"
        );
    }

    let seventeen = format!(
        "(plants: [{}])",
        "(plant: \"CLOVER\"), ".repeat(MAX_PLANTS + 1)
    );
    for invalid in [
        "(plants: [])",
        "(plants: [(plant: \"CLOVER\", density_scale: 0.0)])",
        "(plants: [(plant: \"CLOVER\", density_scale: -1.0)])",
        "(plants: [(plant: (base: \"CLOVER\", density: 0.0))])",
        "(plants: [(plant: (base: \"CLOVER\", density: -1.0))])",
        "(plants: [(plant: \"CLOVER\", clustering: 1.5)])",
        "(plants: [(plant: (height: (-2.0, 1.0)))])",
        "(density_scale: -1.0)",
        "(wind_scale: -1.0)",
        "(patch_size: 0.0)",
        &seventeen,
    ] {
        assert!(
            matches!(
                PlantField::from_ron(invalid),
                Err(PlantFileError::Invalid(_))
            ),
            "{invalid}"
        );
    }
}

#[test]
fn an_unknown_key_lists_the_valid_ones() {
    let plant = PlantField::from_ron("(plants: [(plant: \"FERNS\")])")
        .expect_err("an unknown plant key")
        .to_string();
    assert!(
        plant.contains("\"FERNS\"") && plant.contains("FERN,"),
        "{plant}"
    );
    assert!(plant.contains("MEADOW_GRASS"), "{plant}");
    let field = PlantField::from_ron("(base: \"MEADOWS\")")
        .expect_err("an unknown field key")
        .to_string();
    assert!(
        field.contains("\"MEADOWS\"") && field.contains("SUMMER_WILD"),
        "{field}"
    );
}

#[test]
fn settings_the_plant_does_not_draw_are_kept_not_rejected() {
    // Saving and loading must preserve valid settings even when they currently
    // have no visible effect or the height limits are written in reverse order.
    for (text, check) in [
        // Keep the height limits in the order supplied.
        (
            "(height: (0.5, 0.2))",
            (|p: &PlantParams| p.height == Vec2::new(0.5, 0.2)) as fn(&PlantParams) -> bool,
        ),
        // Keep second-layer branch settings while the first layer is disabled.
        ("(levels: [(count: 0), (count: 3)])", |p| {
            p.levels[1].count == 3
        }),
        // Keep the head length while branches hide the head.
        ("(head_length: 0.2, levels: [(count: 4)])", |p| {
            p.head_length == 0.2
        }),
        // Keep the tuft size while the plant has no tufts.
        ("(tufts: 0.0, tuft_size: 2.5)", |p| p.tuft_size == 2.5),
    ] {
        let plant = PlantParams::from_ron(text).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert!(check(&plant), "{text}");
        assert_eq!(
            PlantParams::from_ron(&plant.to_ron()).unwrap(),
            plant,
            "{text}"
        );
    }
}

#[test]
fn a_parse_error_says_where_and_why() {
    let error = PlantField::from_ron("(\n    density_scale: 2.0,\n    dencity_scale: 3.0,\n)")
        .expect_err("a misspelt setting must be an error");
    let PlantFileError::Parse { line, message, .. } = &error else {
        panic!("a misspelt setting is a parse error, not {error:?}");
    };
    assert_eq!(*line, 3, "{error}");
    assert!(message.contains("dencity"), "{message}");
    assert!(
        error.to_string().starts_with("not a grass file: 3:"),
        "{error}"
    );
}

#[test]
fn a_plant_in_a_field_is_a_plant_and_a_file_cannot_say_otherwise() {
    // Reject field settings inside plant files rather than silently
    // ignoring them. Rust keeps these settings on separate types too.
    let Err(error) =
        PlantField::from_ron("(plants: [(plant: (base: \"CLOVER\", density_scale: 0.5))])")
    else {
        panic!("field settings in a plant must be an error");
    };
    let PlantFileError::Parse { message, .. } = &error else {
        panic!("wrong error: {error}");
    };
    assert!(message.contains("density_scale"), "{message}");

    // Nested plant lists are the same mistake, and so are tufts on the field,
    // and a plant's own density written next to it instead of inside it.
    for wrong in [
        "(plants: [(plant: (plants: []))])",
        "(tufts: 0.3)",
        "(plants: [(plant: \"CLOVER\", density: 3.0)])",
    ] {
        assert!(
            matches!(
                PlantField::from_ron(wrong),
                Err(PlantFileError::Parse { .. })
            ),
            "{wrong}"
        );
    }
}

#[test]
fn every_preset_survives_a_round_trip_through_a_file() {
    for preset in FieldPreset::all() {
        let text = preset.field.to_ron();
        let read = PlantField::from_ron(&text).unwrap_or_else(|e| panic!("{}: {e}", preset.key));
        assert_eq!(read, preset.field, "{}", preset.key);
        // A plant holds plant settings, without field-wide settings. (A
        // plant's own density belongs to it, and each entry has a
        // `density_scale` of its own, so neither is a field-wide setting.)
        let plants = text.split("plants:").nth(1).unwrap_or("");
        for field_setting in ["wind_scale", "ground_tint", "toon_bands"] {
            assert!(
                !plants.contains(field_setting),
                "{}: {field_setting}",
                preset.key
            );
        }
    }
    for preset in PlantPreset::all() {
        let read = PlantParams::from_ron(&preset.plant.to_ron())
            .unwrap_or_else(|e| panic!("{}: {e}", preset.key));
        assert_eq!(read, preset.plant, "{}", preset.key);
    }
}

#[test]
fn every_preset_survives_a_round_trip_through_json() {
    for preset in FieldPreset::all() {
        let read = PlantField::from_json(&preset.field.to_json())
            .unwrap_or_else(|e| panic!("{}: {e}", preset.key));
        assert_eq!(read, preset.field, "{}", preset.key);
    }
    for preset in PlantPreset::all() {
        let read = PlantParams::from_json(&preset.plant.to_json())
            .unwrap_or_else(|e| panic!("{}: {e}", preset.key));
        assert_eq!(read, preset.plant, "{}", preset.key);
    }
}

#[test]
fn a_preset_file_keeps_its_name_and_description() {
    let plant = PlantPreset::from_ron(
        "blue_fern",
        r#"(name: "Blue fern", description: "A fern, but blue.", base: "FERN")"#,
    )
    .unwrap();
    assert_eq!(plant.key, "blue_fern");
    assert_eq!(plant.name, "Blue fern");
    assert_eq!(plant.description, "A fern, but blue.");
    assert_eq!(plant.group, FILE_GROUP);
    assert_eq!(plant.plant, PlantParams::FERN);
    let again = PlantPreset::from_ron("blue_fern", &plant.to_ron()).unwrap();
    assert_eq!(again, plant);

    // Without a name, the key names it.
    let field = FieldPreset::from_ron("my_field", "()").unwrap();
    assert_eq!(field.name, "my_field");
    assert_eq!(field.description, "");
    assert_eq!(field.field, FieldPreset::MEADOW.field);
    let again = FieldPreset::from_ron("my_field", &FieldPreset::FOREST_FLOOR.to_ron()).unwrap();
    assert_eq!(again.name, FieldPreset::FOREST_FLOOR.name);
    assert_eq!(again.field, FieldPreset::FOREST_FLOOR.field);

    // The plain loaders ignore the name.
    assert_eq!(
        PlantParams::from_ron(&PlantPreset::FERN.to_ron()).unwrap(),
        PlantParams::FERN
    );
}

#[test]
fn json_follows_the_same_rules_as_a_file() {
    // Omitted settings inherit the chosen preset; colours also accept hexadecimal text.
    let field =
        PlantField::from_json(r##"{"base": "SUMMER_TALL", "patch_color": "#ff8000"}"##).unwrap();
    let mut expected = FieldPreset::SUMMER_TALL.field;
    expected.variation.patch_color = Color::Srgba(Srgba::hex("#ff8000").unwrap());
    assert_eq!(field, expected);

    // Report unknown presets and invalid values, and locate malformed settings in the text.
    assert!(matches!(
        PlantField::from_json(r#"{"base": "NOT_A_PRESET"}"#),
        Err(PlantFileError::UnknownField(_))
    ));
    assert!(matches!(
        PlantParams::from_json(r#"{"stems_per_plant": 0}"#),
        Err(PlantFileError::Invalid(_))
    ));
    let error = PlantField::from_json("{\n  \"density_scale\": 2.0,\n  \"dencity_scale\": 3.0\n}")
        .expect_err("a misspelt setting must be an error");
    let PlantFileError::Parse { line, message, .. } = &error else {
        panic!("a misspelt setting is a parse error, not {error:?}");
    };
    assert_eq!(*line, 3, "{error}");
    assert!(message.contains("dencity"), "{message}");
}
