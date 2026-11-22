use super::*;
use crate::presets::FieldPreset;

#[test]
fn a_terrain_of_your_own_gets_what_far_grass_material_uploads() {
    let far =
        FarGrass::new(&FieldPreset::MEADOW.field).with_area(Rect::new(-10.0, -20.0, 30.0, 40.0));
    let wind = GrassWindFrame {
        strength: 0.7,
        gust_offset: Vec2::new(3.0, -1.0),
        ..GrassWindFrame::default()
    };
    let settings = GrassSettings {
        radius: 60.0,
        ..GrassSettings::HIGH
    };
    // What `sync_with_grass` leaves in a `FarGrassMaterial` for this frame.
    let synced = FarGrass {
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
    assert_eq!(uniform.fade.z, 0.3, "the sheen of `FarGrass::new`");
    assert_eq!(
        far.clone().with_sheen(0.0).uniform(&wind, &settings).fade.z,
        0.0
    );

    // A radius out of range is the radius the blades are drawn with.
    let nonsense = GrassSettings {
        radius: f32::NAN,
        ..GrassSettings::HIGH
    };
    assert_eq!(
        far.uniform(&wind, &nonsense).fade.y,
        GrassSettings::HIGH.radius * 0.95
    );
}

/// A plant past the last one a field grows does not colour far grass.
#[test]
fn far_grass_mixes_only_the_plants_a_field_grows() {
    use crate::look::{FieldPlant, MAX_PLANTS};

    let grown: Vec<FieldPlant> = FieldPreset::MEADOW
        .field
        .plants
        .iter()
        .copied()
        .cycle()
        .take(MAX_PLANTS)
        .collect();
    let mut red = grown[0];
    red.plant.segment.root_color = Color::srgb(1.0, 0.0, 0.0);
    red.plant.segment.tip_color = Color::srgb(1.0, 0.0, 0.0);
    let mut extra = grown.clone();
    extra.push(red);

    let field = |plants: Vec<FieldPlant>| GrassField {
        plants: plants.into(),
        ..FieldPreset::MEADOW.field
    };
    assert_eq!(
        FarGrass::new(&field(extra)).color,
        FarGrass::new(&field(grown)).color
    );
}
