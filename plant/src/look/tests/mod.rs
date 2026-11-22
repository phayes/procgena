use super::*;
use crate::api::GrassField;
use crate::presets::PlantPreset;

/// Returns the last plant in a preset's mix, used here as its background plant.
/// List position alone does not imply the largest weight.
fn main_plant(field: &GrassField) -> &GrassPlant {
    &field.plants.last().expect("a preset grows a plant").plant
}

#[test]
fn spacing_and_density_describe_the_same_field() {
    let field = FieldPreset::SUMMER_TALL.field.with_spacing(0.2);
    assert!((field.density() - 25.0).abs() < 1e-3);
    assert!((field.spacing() - 0.2).abs() < 1e-6);
    let field = field.with_density(100.0);
    assert!((field.spacing() - 0.1).abs() < 1e-6);
}

#[test]
fn every_season_preset_is_tall_and_valid() {
    for preset in &FieldPreset::SEASONS {
        let plant = main_plant(&preset.field);
        assert!(plant.height.x > 0.0 && plant.height.x < plant.height.y);
        assert!(plant.height.y >= 1.0, "tall grass reaches at least 1 m");
        assert!(preset.field.density() > 0.0 && plant.width > 0.0);
        assert!((0.0..=1.0).contains(&plant.translucency));
    }
}

#[test]
fn summer_is_green_and_autumn_is_brown() {
    let rgb = |preset: &FieldPreset| main_plant(&preset.field).segment.tip_color.to_srgba();
    let summer = rgb(&FieldPreset::SUMMER_TALL);
    assert!(
        summer.green > summer.red && summer.green > summer.blue,
        "{summer:?}"
    );
    let autumn = rgb(&FieldPreset::AUTUMN_TALL);
    assert!(
        autumn.red > autumn.green && autumn.green > autumn.blue,
        "{autumn:?}"
    );
    // The two seasons are one field, so they mix and blend: the same blade
    // sizes and density.
    let (summer, autumn) = (
        &FieldPreset::SUMMER_TALL.field,
        &FieldPreset::AUTUMN_TALL.field,
    );
    assert_eq!(main_plant(summer).height, main_plant(autumn).height);
    assert_eq!(main_plant(summer).width, main_plant(autumn).width);
    assert_eq!(summer.density(), autumn.density());
}

/// Checks that bounded segment settings stay within the unit interval, except a
/// main stem's droop, which goes to 2 to land its tip on the ground.
fn assert_unit_segment(segment: &GrassSegment, name: &str, main_stem: bool) {
    for unit in [segment.profile, segment.stiffness, segment.fold] {
        assert!((0.0..=1.0).contains(&unit), "{name}");
    }
    let most_droop = if main_stem { 2.0 } else { 1.0 };
    assert!((0.0..=most_droop).contains(&segment.droop), "{name}");
}

/// Checks that every setting of `plant` is in its valid range.
fn assert_valid_plant(plant: &GrassPlant, name: &str) {
    assert!(
        plant.height.x >= 0.0 && plant.height.x <= plant.height.y,
        "{name}"
    );
    assert!(plant.width > 0.0, "{name}");
    for unit in [
        plant.lean,
        plant.translucency,
        plant.specular,
        plant.root_occlusion,
        plant.tufts,
    ] {
        assert!((0.0..=1.0).contains(&unit), "{name}");
    }
    assert!(plant.tuft_size > 0.0, "{name}");
    assert!((0.0..0.9).contains(&plant.head_length), "{name}");
    assert!(
        (1..=MAX_STEMS_PER_PLANT).contains(&plant.stems_per_plant),
        "{name}"
    );
    assert_unit_segment(&plant.segment, name, true);
    let [first, second] = &plant.levels;
    assert!(first.is_on() || !second.is_on(), "{name}: levels in order");
    for level in &plant.levels {
        assert!(level.count <= MAX_BRANCHES, "{name}");
        assert_unit_segment(&level.segment, name, false);
    }
}

#[test]
fn every_preset_is_valid_and_looks_like_no_other() {
    let fields: Vec<_> = FieldPreset::all().collect();
    for preset in &fields {
        for entry in preset.field.plants.iter() {
            assert_valid_plant(&entry.plant, &preset.key);
        }
        assert!(preset.field.density() > 0.0, "{}", preset.key);
    }
    // No two presets are the same field.
    for (index, preset) in fields.iter().enumerate() {
        for other in &fields[index + 1..] {
            assert_ne!(
                preset.field, other.field,
                "{} and {}",
                preset.key, other.key
            );
        }
    }
    let plants: Vec<_> = PlantPreset::all().collect();
    for preset in &plants {
        assert_valid_plant(&preset.plant, &preset.key);
    }
    for (index, preset) in plants.iter().enumerate() {
        for other in &plants[index + 1..] {
            assert_ne!(
                preset.plant, other.plant,
                "{} and {}",
                preset.key, other.key
            );
        }
    }
}

#[test]
fn the_new_looks_push_what_the_meadows_leave_alone() {
    // Each of these is in the set because it uses a setting harder than any
    // general preset does; keep it that way when tuning them.
    // A preset, why it is in the set, and the setting that shows it.
    type Push = (&'static str, &'static str, fn(&GrassField) -> bool);
    let pushes: [Push; 11] = [
        ("CORKSCREW_ALIEN", "ribbons twist", |f| {
            main_plant(f).segment.twist >= 1.5
        }),
        ("CRYSTAL_SPIRES", "crystal barely moves", |f| {
            main_plant(f).segment.stiffness >= 0.95
        }),
        ("CRYSTAL_SPIRES", "crystal is faceted", |f| {
            main_plant(f).segment.fold >= 0.95
        }),
        ("KELP_BED", "kelp has no stiffness", |f| {
            main_plant(f).segment.stiffness <= 0.05
        }),
        ("PAMPAS", "tufts become fountains", |f| {
            main_plant(f).tufts >= 1.0 && main_plant(f).segment.droop >= 0.85
        }),
        ("SPINIFEX", "hummocks splay out", |f| {
            main_plant(f).lean >= 0.8 && main_plant(f).tufts >= 1.0
        }),
        ("DUNE_MARRAM", "mostly sand", |f| {
            f.variation.bare_share >= 0.4
        }),
        ("ELEPHANT_GRASS", "taller than a person", |f| {
            main_plant(f).height.x >= 2.0
        }),
        ("REED_MARSH", "over your head", |f| {
            main_plant(f).height.y >= 1.8
        }),
        ("CEL_FIELD", "two bands", |f| f.toon_bands == 2),
        ("COZY_PASTEL", "toon", |f| f.toon_bands > 0),
    ];
    for (name, why, holds) in pushes {
        let preset = FieldPreset::named(name).expect("a preset");
        assert!(holds(&preset.field), "{name}: {why}");
    }
    let value = |color: Color| {
        let c = color.to_srgba();
        c.red.max(c.green).max(c.blue)
    };
    assert!(
        value(
            main_plant(&FieldPreset::BURNT_REGROWTH.field)
                .segment
                .tip_color
        ) < 0.15,
        "charcoal"
    );
    let frost = main_plant(&FieldPreset::FROSTED_MEADOW.field)
        .segment
        .tip_color
        .to_srgba();
    assert!(frost.red > 0.75 && frost.blue > 0.75, "near white");
    let cornflower = GrassPlant::CORNFLOWER.head_color.to_srgba();
    assert!(cornflower.blue > cornflower.red && cornflower.blue > cornflower.green);
    let grows = |field: &GrassField, plant: GrassPlant| {
        field.plants.iter().any(|entry| entry.plant == plant)
    };
    assert!(grows(
        &FieldPreset::COTTON_BOG.field,
        GrassPlant::COTTON_GRASS
    ));
    assert!(grows(&FieldPreset::REED_MARSH.field, GrassPlant::CATTAIL));
}

#[test]
fn nested_struct_updates_work_in_constants() {
    // Nested struct updates must work in constants as well as runtime expressions.
    const DRY: GrassPlant = GrassPlant {
        segment: GrassSegment {
            tip_color: Color::srgb(0.7, 0.6, 0.3),
            ..GrassPlant::MEADOW_GRASS.segment
        },
        ..GrassPlant::MEADOW_GRASS
    };
    const DRY_FIELD: GrassField = GrassField {
        density_scale: 0.2,
        ..FieldPreset::MEADOW.field
    };
    assert_eq!(DRY.height, GrassPlant::MEADOW_GRASS.height);
    assert_ne!(
        DRY.segment.tip_color,
        GrassPlant::MEADOW_GRASS.segment.tip_color
    );
    assert_eq!(DRY_FIELD.variation, FieldPreset::MEADOW.field.variation);
    assert_eq!(DRY_FIELD.ground_tint, FieldPreset::MEADOW.field.ground_tint);
    assert_ne!(DRY_FIELD, FieldPreset::MEADOW.field, "fields compare");

    // Defaults should reproduce meadow plants with branching disabled.
    assert_eq!(GrassField::default(), FieldPreset::MEADOW.field);
    assert_eq!(GrassPlant::default(), GrassPlant::MEADOW_GRASS);
    assert_eq!(GrassSegment::default(), GrassPlant::MEADOW_GRASS.segment);
    assert_eq!(GrassBranches::default(), GrassBranches::NONE);
    assert!(!GrassBranches::NONE.is_on());
    assert_eq!(GrassArrangement::default(), GrassArrangement::Opposite);
    assert_eq!(
        GrassVariation::default(),
        FieldPreset::MEADOW.field.variation
    );
    assert_eq!(GroundTint::default(), FieldPreset::MEADOW.field.ground_tint);
    assert_eq!(
        FieldPlant::default(),
        FieldPlant::new(GrassPlant::MEADOW_GRASS)
    );
}

#[test]
fn the_forest_presets_are_short_wide_and_mixed() {
    // The group holds each forest field once, in order.
    let listed = [
        FieldPreset::FOREST_MOSS,
        FieldPreset::FOREST_FERNS,
        FieldPreset::FOREST_FLOOR,
        FieldPreset::FOREST_BRACKEN,
    ];
    for (preset, group) in listed.iter().zip(&FieldPreset::FOREST) {
        let name = &preset.key;
        assert_eq!(preset, group, "{name}");
        assert!(preset.field.density() > 0.0, "{name}");
        let plant = main_plant(&preset.field);
        assert!(plant.height.y >= plant.height.x, "{name}");
        assert!(plant.width > 0.02, "{name}: forest plants are wide");
    }

    // Moss is a carpet: very short, wide, dense, and only moss.
    let moss = &FieldPreset::FOREST_MOSS.field;
    assert!(main_plant(moss).height.y <= 0.1, "moss stays low");
    assert!(
        main_plant(moss).width > main_plant(moss).height.y * 0.3,
        "moss is broad"
    );
    assert!(moss.density() > FieldPreset::LAWN.field.density() * 0.5);
    assert_eq!(moss.plants.len(), 1);

    // Each fern frond has one level of leaflets and no second-level branches.
    for fern in [GrassPlant::FERN, GrassPlant::BRACKEN] {
        assert!(fern.stems_per_plant >= 4);
        assert!(fern.levels[0].count >= 20);
        assert!(!fern.levels[1].is_on(), "two levels, not three");
    }
    // Fern mixes should contain about one to two ferns per square metre; the full
    // forest-floor mix includes all its ground-cover plants.
    let ferns_per_m2 = |field: &GrassField, fern: GrassPlant| {
        field
            .plants
            .iter()
            .filter(|entry| entry.plant == fern)
            .map(|entry| entry.density() * field.density_scale)
            .sum::<f32>()
    };
    for (preset, fern) in [
        (FieldPreset::FOREST_FERNS, GrassPlant::FERN),
        (FieldPreset::FOREST_FLOOR, GrassPlant::FERN),
        (FieldPreset::FOREST_BRACKEN, GrassPlant::BRACKEN),
    ] {
        let density = ferns_per_m2(&preset.field, fern);
        assert!((1.0..=2.0).contains(&density), "{density} ferns per m²");
    }
    assert!(
        FieldPreset::FOREST_FERNS
            .field
            .plants
            .iter()
            .any(|entry| entry.clustering >= 0.7)
    );
    assert!(
        FieldPreset::FOREST_FLOOR
            .field
            .plants
            .iter()
            .any(|entry| entry.plant == GrassPlant::LEAF_LITTER)
    );
    let bracken = GrassPlant::BRACKEN.segment.tip_color.to_srgba();
    assert!(bracken.red > bracken.green && bracken.green > bracken.blue);
}

#[test]
fn a_field_plant_picks_whole_plants() {
    // Densities count whole plants rather than individual stems.
    let wild = &FieldPreset::SUMMER_WILD.field.plants;
    assert_eq!(wild[0].plant, GrassPlant::SEED_HEADS);
    assert_eq!(wild[0].density_scale, 0.1);
    let entry = FieldPlant::new(GrassPlant::CLOVER)
        .scaled(0.2)
        .clustered(0.5);
    assert_eq!((entry.density_scale, entry.clustering), (0.2, 0.5));
    assert_eq!(entry.plant, GrassPlant::CLOVER);
    assert_eq!(FieldPlant::new(GrassPlant::CLOVER).density_scale, 1.0);
}

#[test]
fn a_field_grows_each_plant_at_its_density_times_both_scales() {
    let sparse = GrassPlant {
        density: 2.0,
        ..GrassPlant::FERN
    };
    let dense = GrassPlant {
        density: 80.0,
        ..GrassPlant::MOSS
    };
    let field = GrassField::new(
        Vec2::ZERO,
        vec![FieldPlant::new(sparse).scaled(0.5), FieldPlant::new(dense)],
    );
    assert_eq!(field.plants[0].density(), 1.0);
    assert_eq!(field.density(), 81.0);
    let half = GrassField {
        density_scale: 0.5,
        ..field.clone()
    };
    assert_eq!(half.density(), 40.5);

    // with_density changes the field's scale and keeps the mix.
    let thin = field.clone().with_density(27.0);
    assert!((thin.density() - 27.0).abs() < 1e-4);
    assert_eq!(thin.plants, field.plants);

    // Negative scales count as nothing, and a field with nothing growing
    // keeps its scale.
    assert_eq!(FieldPlant::new(sparse).scaled(-1.0).density(), 0.0);
    let bare = GrassField::new(Vec2::ZERO, vec![FieldPlant::new(sparse).scaled(0.0)]);
    assert_eq!(bare.clone().with_density(10.0), bare);
}
