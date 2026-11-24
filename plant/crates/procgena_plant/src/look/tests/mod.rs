use super::*;
use crate::presets::PlantPreset;
use crate::scatter::PlantScatterOptions;

/// Returns the last plant in a preset's mix, used here as its background plant.
/// List position alone does not imply the largest weight.
fn main_plant(field: &PlantScatterOptions) -> &PlantParams {
    &field.plants.last().expect("a preset grows a plant").plant
}

#[test]
fn spacing_and_density_describe_the_same_field() {
    let field = PlantScatterPreset::SUMMER_TALL
        .plant_scatter
        .with_spacing(0.2);
    assert!((field.density() - 25.0).abs() < 1e-3);
    assert!((field.spacing() - 0.2).abs() < 1e-6);
    let field = field.with_density(100.0);
    assert!((field.spacing() - 0.1).abs() < 1e-6);
}

#[test]
fn every_season_preset_is_tall_and_valid() {
    for preset in &PlantScatterPreset::SEASONS {
        let plant = main_plant(&preset.plant_scatter);
        assert!(plant.height.x > 0.0 && plant.height.x < plant.height.y);
        assert!(plant.height.y >= 1.0, "tall grass reaches at least 1 m");
        assert!(preset.plant_scatter.density() > 0.0 && plant.width > 0.0);
        assert!((0.0..=1.0).contains(&plant.translucency));
    }
}

#[test]
fn summer_is_green_and_autumn_is_brown() {
    let rgb = |preset: &PlantScatterPreset| main_plant(&preset.plant_scatter).segment.tip_color;
    let summer @ Srgb([red, green, blue]) = rgb(&PlantScatterPreset::SUMMER_TALL);
    assert!(green > red && green > blue, "{summer:?}");
    let autumn @ Srgb([red, green, blue]) = rgb(&PlantScatterPreset::AUTUMN_TALL);
    assert!(red > green && green > blue, "{autumn:?}");
    // The two seasons are one field, so they mix and blend: the same blade
    // sizes and density.
    let (summer, autumn) = (
        &PlantScatterPreset::SUMMER_TALL.plant_scatter,
        &PlantScatterPreset::AUTUMN_TALL.plant_scatter,
    );
    assert_eq!(main_plant(summer).height, main_plant(autumn).height);
    assert_eq!(main_plant(summer).width, main_plant(autumn).width);
    assert_eq!(summer.density(), autumn.density());
}

/// Checks that bounded segment settings stay within the unit interval, except a
/// main stem's droop, which goes to 2 to land its tip on the ground.
fn assert_unit_segment(segment: &PlantSegment, name: &str, main_stem: bool) {
    for unit in [segment.profile, segment.stiffness, segment.fold] {
        assert!((0.0..=1.0).contains(&unit), "{name}");
    }
    let most_droop = if main_stem { 2.0 } else { 1.0 };
    assert!((0.0..=most_droop).contains(&segment.droop), "{name}");
}

/// Checks that every setting of `plant` is in its valid range.
fn assert_valid_plant(plant: &PlantParams, name: &str) {
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
    let fields: Vec<_> = PlantScatterPreset::all().collect();
    for preset in &fields {
        for entry in preset.plant_scatter.plants.iter() {
            assert_valid_plant(&entry.plant, &preset.key);
        }
        assert!(preset.plant_scatter.density() > 0.0, "{}", preset.key);
    }
    // No two presets are the same field.
    for (index, preset) in fields.iter().enumerate() {
        for other in &fields[index + 1..] {
            assert_ne!(
                preset.plant_scatter, other.plant_scatter,
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
    type Push = (&'static str, &'static str, fn(&PlantScatterOptions) -> bool);
    let pushes: [Push; 11] = [
        ("corkscrew_alien", "ribbons twist", |f| {
            main_plant(f).segment.twist >= 1.5
        }),
        ("crystal_spires", "crystal barely moves", |f| {
            main_plant(f).segment.stiffness >= 0.95
        }),
        ("crystal_spires", "crystal is faceted", |f| {
            main_plant(f).segment.fold >= 0.95
        }),
        ("kelp_bed", "kelp has no stiffness", |f| {
            main_plant(f).segment.stiffness <= 0.05
        }),
        ("pampas", "tufts become fountains", |f| {
            main_plant(f).tufts >= 1.0 && main_plant(f).segment.droop >= 0.85
        }),
        ("spinifex", "hummocks splay out", |f| {
            main_plant(f).lean >= 0.8 && main_plant(f).tufts >= 1.0
        }),
        ("dune_marram", "mostly sand", |f| {
            f.variation.bare_share >= 0.4
        }),
        ("elephant_grass", "taller than a person", |f| {
            main_plant(f).height.x >= 2.0
        }),
        ("reed_marsh", "over your head", |f| {
            main_plant(f).height.y >= 1.8
        }),
        ("cel_field", "two bands", |f| f.toon_bands == 2),
        ("cozy_pastel", "toon", |f| f.toon_bands > 0),
    ];
    for (name, why, holds) in pushes {
        let preset = PlantScatterPreset::named(name).expect("a preset");
        assert!(holds(&preset.plant_scatter), "{name}: {why}");
    }
    let value = |Srgb([red, green, blue]): Srgb| red.max(green).max(blue);
    assert!(
        value(
            main_plant(&PlantScatterPreset::BURNT_REGROWTH.plant_scatter)
                .segment
                .tip_color
        ) < 0.15,
        "charcoal"
    );
    let Srgb([red, _, blue]) = main_plant(&PlantScatterPreset::FROSTED_MEADOW.plant_scatter)
        .segment
        .tip_color;
    assert!(red > 0.75 && blue > 0.75, "near white");
    let Srgb([red, green, blue]) = PlantParams::CORNFLOWER.head_color;
    assert!(blue > red && blue > green);
    let grows = |field: &PlantScatterOptions, plant: PlantParams| {
        field.plants.iter().any(|entry| entry.plant == plant)
    };
    assert!(grows(
        &PlantScatterPreset::COTTON_BOG.plant_scatter,
        PlantParams::COTTON_GRASS
    ));
    assert!(grows(
        &PlantScatterPreset::REED_MARSH.plant_scatter,
        PlantParams::CATTAIL
    ));
}

#[test]
fn nested_struct_updates_work_in_constants() {
    // Nested struct updates must work in constants as well as runtime expressions.
    const DRY: PlantParams = PlantParams {
        segment: PlantSegment {
            tip_color: Srgb::new(0.7, 0.6, 0.3),
            ..PlantParams::MEADOW_GRASS.segment
        },
        ..PlantParams::MEADOW_GRASS
    };
    const DRY_FIELD: PlantScatterOptions = PlantScatterOptions {
        density_scale: 0.2,
        ..PlantScatterPreset::MEADOW.plant_scatter
    };
    assert_eq!(DRY.height, PlantParams::MEADOW_GRASS.height);
    assert_ne!(
        DRY.segment.tip_color,
        PlantParams::MEADOW_GRASS.segment.tip_color
    );
    assert_eq!(
        DRY_FIELD.variation,
        PlantScatterPreset::MEADOW.plant_scatter.variation
    );
    assert_eq!(
        DRY_FIELD.ground_tint,
        PlantScatterPreset::MEADOW.plant_scatter.ground_tint
    );
    assert_ne!(
        DRY_FIELD,
        PlantScatterPreset::MEADOW.plant_scatter,
        "fields compare"
    );

    // Defaults should reproduce meadow plants with branching disabled.
    assert_eq!(
        PlantScatterOptions::default(),
        PlantScatterPreset::MEADOW.plant_scatter
    );
    assert_eq!(PlantParams::default(), PlantParams::MEADOW_GRASS);
    assert_eq!(PlantSegment::default(), PlantParams::MEADOW_GRASS.segment);
    assert_eq!(PlantBranches::default(), PlantBranches::NONE);
    assert!(!PlantBranches::NONE.is_on());
    assert_eq!(PlantArrangement::default(), PlantArrangement::Opposite);
    assert_eq!(
        PlantVariation::default(),
        PlantScatterPreset::MEADOW.plant_scatter.variation
    );
    assert_eq!(
        GroundTint::default(),
        PlantScatterPreset::MEADOW.plant_scatter.ground_tint
    );
    assert_eq!(
        ScatterPlant::default(),
        ScatterPlant::new(PlantParams::MEADOW_GRASS)
    );
}

#[test]
fn the_forest_presets_are_short_wide_and_mixed() {
    // The group holds each forest field once, in order.
    let listed = [
        PlantScatterPreset::FOREST_MOSS,
        PlantScatterPreset::FOREST_FERNS,
        PlantScatterPreset::FOREST_FLOOR,
        PlantScatterPreset::FOREST_BRACKEN,
    ];
    for (preset, group) in listed.iter().zip(&PlantScatterPreset::FOREST) {
        let name = &preset.key;
        assert_eq!(preset, group, "{name}");
        assert!(preset.plant_scatter.density() > 0.0, "{name}");
        let plant = main_plant(&preset.plant_scatter);
        assert!(plant.height.y >= plant.height.x, "{name}");
        assert!(plant.width > 0.02, "{name}: forest plants are wide");
    }

    // Moss is a carpet: very short, wide, dense, and only moss.
    let moss = &PlantScatterPreset::FOREST_MOSS.plant_scatter;
    assert!(main_plant(moss).height.y <= 0.1, "moss stays low");
    assert!(
        main_plant(moss).width > main_plant(moss).height.y * 0.3,
        "moss is broad"
    );
    assert!(moss.density() > PlantScatterPreset::LAWN.plant_scatter.density() * 0.5);
    assert_eq!(moss.plants.len(), 1);

    // Each fern frond has one level of leaflets and no second-level branches.
    for fern in [PlantParams::FERN, PlantParams::BRACKEN] {
        assert!(fern.stems_per_plant >= 4);
        assert!(fern.levels[0].count >= 20);
        assert!(!fern.levels[1].is_on(), "two levels, not three");
    }
    // Fern mixes should contain about one to two ferns per square metre; the full
    // forest-floor mix includes all its ground-cover plants.
    let ferns_per_m2 = |field: &PlantScatterOptions, fern: PlantParams| {
        field
            .plants
            .iter()
            .filter(|entry| entry.plant == fern)
            .map(|entry| entry.density() * field.density_scale)
            .sum::<f32>()
    };
    for (preset, fern) in [
        (PlantScatterPreset::FOREST_FERNS, PlantParams::FERN),
        (PlantScatterPreset::FOREST_FLOOR, PlantParams::FERN),
        (PlantScatterPreset::FOREST_BRACKEN, PlantParams::BRACKEN),
    ] {
        let density = ferns_per_m2(&preset.plant_scatter, fern);
        assert!((1.0..=2.0).contains(&density), "{density} ferns per m²");
    }
    assert!(
        PlantScatterPreset::FOREST_FERNS
            .plant_scatter
            .plants
            .iter()
            .any(|entry| entry.clustering >= 0.7)
    );
    assert!(
        PlantScatterPreset::FOREST_FLOOR
            .plant_scatter
            .plants
            .iter()
            .any(|entry| entry.plant == PlantParams::LEAF_LITTER)
    );
    let Srgb([red, green, blue]) = PlantParams::BRACKEN.segment.tip_color;
    assert!(red > green && green > blue);
}

#[test]
fn a_field_plant_picks_whole_plants() {
    // Densities count whole plants rather than individual stems.
    let wild = &PlantScatterPreset::SUMMER_WILD.plant_scatter.plants;
    assert_eq!(wild[0].plant, PlantParams::SEED_HEADS);
    assert_eq!(wild[0].density_scale, 0.1);
    let entry = ScatterPlant::new(PlantParams::CLOVER)
        .scaled(0.2)
        .clustered(0.5);
    assert_eq!((entry.density_scale, entry.clustering), (0.2, 0.5));
    assert_eq!(entry.plant, PlantParams::CLOVER);
    assert_eq!(ScatterPlant::new(PlantParams::CLOVER).density_scale, 1.0);
}

#[test]
fn a_field_grows_each_plant_at_its_density_times_both_scales() {
    let sparse = PlantParams {
        density: 2.0,
        ..PlantParams::FERN
    };
    let dense = PlantParams {
        density: 80.0,
        ..PlantParams::MOSS
    };
    let field = PlantScatterOptions::default().with_plants(vec![
        ScatterPlant::new(sparse).scaled(0.5),
        ScatterPlant::new(dense),
    ]);
    assert_eq!(field.plants[0].density(), 1.0);
    assert_eq!(field.density(), 81.0);
    let half = PlantScatterOptions {
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
    assert_eq!(ScatterPlant::new(sparse).scaled(-1.0).density(), 0.0);
    let bare =
        PlantScatterOptions::default().with_plants(vec![ScatterPlant::new(sparse).scaled(0.0)]);
    assert_eq!(bare.clone().with_density(10.0), bare);
}
