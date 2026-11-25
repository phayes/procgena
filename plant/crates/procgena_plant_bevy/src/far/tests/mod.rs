use super::*;
use crate::PlantScatterPreset;

#[test]
fn a_terrain_of_your_own_gets_what_far_plant_material_uploads() {
    let far = FarPlants::new(&PlantScatterPreset::MEADOW.plant_scatter)
        .with_area(Rect::new(-10.0, -20.0, 30.0, 40.0));
    let wind = PlantWindFrame {
        strength: 0.7,
        gust_offset: Vec2::new(3.0, -1.0),
        ..PlantWindFrame::default()
    };
    let settings = PlantSettings {
        radius: 60.0,
        ..PlantSettings::HIGH
    };
    // What `sync_with_plants` leaves in a `FarPlantMaterial` for this frame.
    let synced = FarPlants {
        wind,
        radius: 60.0,
        ..far.clone()
    };
    assert_eq!(far.uniform(&wind, &settings), synced.packed());
    let uniform = far.uniform(&wind, &settings);
    assert_eq!(
        (uniform.fade.x, uniform.fade.y),
        (30.0, 57.0),
        "the fade follows the radius"
    );
    assert_eq!(uniform.area, Vec4::new(-10.0, -20.0, 30.0, 40.0));
    assert_eq!(uniform.fade.z, 0.3, "the sheen of `FarPlants::new`");
    assert_eq!(
        far.clone().with_sheen(0.0).uniform(&wind, &settings).fade.z,
        0.0
    );

    // A radius out of range is the radius the blades are drawn with.
    let nonsense = PlantSettings {
        radius: f32::NAN,
        ..PlantSettings::HIGH
    };
    assert_eq!(
        far.uniform(&wind, &nonsense).fade.y,
        PlantSettings::HIGH.radius * 0.95
    );
}

/// A plant past the last one a field grows does not colour far grass.
#[test]
fn far_plants_mix_only_the_plants_a_field_grows() {
    use crate::{MAX_PLANTS, ScatterPlant, Srgb};

    let grown: Vec<ScatterPlant> = PlantScatterPreset::MEADOW
        .plant_scatter
        .plants
        .iter()
        .copied()
        .cycle()
        .take(MAX_PLANTS)
        .collect();
    let mut red = grown[0];
    red.plant.segment.root_color = Srgb::new(1.0, 0.0, 0.0);
    red.plant.segment.tip_color = Srgb::new(1.0, 0.0, 0.0);
    let mut extra = grown.clone();
    extra.push(red);

    let field =
        |plants: Vec<ScatterPlant>| PlantScatterPreset::MEADOW.plant_scatter.with_plants(plants);
    assert_eq!(
        FarPlants::new(&field(extra)).color,
        FarPlants::new(&field(grown)).color
    );
}
