use std::any::TypeId;

use bevy::app::App;
use bevy::asset::{AssetPlugin, Assets, ReflectAsset};
use bevy::ecs::reflect::{AppTypeRegistry, ReflectComponent, ReflectResource};
use bevy::math::{Vec2, Vec3};
use bevy::reflect::serde::{ReflectDeserializer, ReflectSerializer};
use bevy::reflect::std_traits::ReflectDefault;
use bevy::reflect::{FromReflect, GetPath, PartialReflect, TypeRegistry};
use serde::de::DeserializeSeed;

use super::*;

/// An app with exactly what the plugin registers for reflection.
fn app() -> App {
    let mut app = App::new();
    app.add_plugins(AssetPlugin::default());
    register_types(&mut app);
    app
}

/// Writes `value` as RON through reflection and reads it back, the way a
/// `.scn.ron` file or a save game does.
fn through_ron(value: &dyn PartialReflect, registry: &TypeRegistry) -> Box<dyn PartialReflect> {
    let text =
        ron::ser::to_string(&ReflectSerializer::new(value, registry)).expect("the type serializes");
    let mut deserializer = ron::de::Deserializer::from_str(&text).unwrap();
    ReflectDeserializer::new(registry)
        .deserialize(&mut deserializer)
        .unwrap_or_else(|error| panic!("{error}\n{text}"))
}

#[test]
fn the_readme_quick_start_is_the_minimal_example() {
    // The first thing a new user copies must be a program that runs, so
    // the README holds the example's text and a test keeps them equal.
    let example = include_str!("../../examples/minimal.rs");
    let code = example
        .split(
            "

",
        )
        .skip(1)
        .collect::<Vec<_>>()
        .join(
            "

",
        );
    let readme = include_str!("../../README.md");
    let section = readme
        .split("## Quick start")
        .nth(1)
        .expect("a quick start");
    let block = section
        .split(
            "```rust
",
        )
        .nth(1)
        .and_then(|rest| rest.split("```").next())
        .expect("a code block");
    assert_eq!(
        block.trim(),
        code.trim(),
        "update the README from examples/minimal.rs"
    );
}

#[test]
fn fields_and_presets_can_be_compared() {
    // `set_if_neq` and plain `==` in games and tests.
    let field = PlantField {
        size: Vec2::splat(8.0),
        ..Default::default()
    };
    assert_eq!(field, field.clone());
    assert_ne!(
        field,
        PlantField {
            size: Vec2::splat(9.0),
            ..field.clone()
        }
    );
    assert_ne!(
        PlantGround::Flat,
        PlantGround::Heightmap {
            image: bevy::asset::Handle::default(),
            height_scale: 1.0
        }
    );
    assert_eq!(FieldPreset::MEADOW, FieldPreset::MEADOW.clone());
    assert_ne!(FieldPreset::MEADOW, FieldPreset::WHEAT);
    assert_eq!(PlantPreset::FERN, PlantPreset::FERN.clone());
    assert_eq!(
        FieldPreset::SUMMER_WILD.field.plants[0],
        FieldPreset::SUMMER_WILD.field.plants[0]
    );
}

#[test]
fn a_field_can_be_built_from_a_preset_or_plants_in_one_call() {
    let field = PlantField::from_preset(&FieldPreset::WHEAT, Vec2::splat(8.0));
    assert_eq!(field.size, Vec2::splat(8.0));
    assert_eq!(field.ground, PlantGround::Flat);
    assert_eq!(field.density_map, None);
    assert_eq!(field.plants, FieldPreset::WHEAT.field.plants);
    assert_eq!(field.density_scale, FieldPreset::WHEAT.field.density_scale);

    let plants = vec![FieldPlant::new(PlantParams::FERN)];
    let field = PlantField::new(Vec2::splat(4.0), plants.clone());
    assert_eq!(field.size, Vec2::splat(4.0));
    assert_eq!(field.plants.to_vec(), plants);
    assert_eq!(field.variation, PlantField::default().variation);
    assert_eq!(PlantField::default().size, Vec2::ZERO);
}

#[test]
fn every_public_type_has_the_type_data_tools_look_for() {
    let app = app();
    let registry = app.world().resource::<AppTypeRegistry>().read();
    let has = |id: TypeId, what: &str, found: bool| {
        let name = registry
            .get(id)
            .map_or("<unregistered>", |r| r.type_info().type_path());
        assert!(found, "{name} lacks {what}");
    };
    macro_rules! check {
        ($t:ty: $($data:ident),*) => {{
            let id = TypeId::of::<$t>();
            has(id, "registration", registry.contains(id));
            $( has(id, stringify!($data), registry.get_type_data::<$data>(id).is_some()); )*
        }};
    }
    // Components: scenes, BSN, Blender exporters and inspectors.
    check!(PlantField: ReflectComponent, ReflectDefault);
    check!(PlantInteractor: ReflectComponent, ReflectDefault);
    check!(PlantInteractorSpread: ReflectComponent, ReflectDefault);
    check!(PlantInteractorFootprint: ReflectComponent, ReflectDefault);
    check!(PlantInteractorFromCollider: ReflectComponent, ReflectDefault);
    check!(PlantColliderBounds: ReflectComponent, ReflectDefault);
    check!(NoPlants: ReflectComponent, ReflectDefault);
    check!(PlantCamera: ReflectComponent, ReflectDefault);
    // Resources: inspector resource panels.
    check!(PlantWind: ReflectResource, ReflectComponent, ReflectDefault);
    check!(PlantWindFrame: ReflectResource, ReflectDefault);
    check!(PlantSettings: ReflectResource, ReflectComponent, ReflectDefault);
    // The preset assets: inspector asset panels.
    check!(FieldPreset: ReflectAsset);
    check!(PlantPreset: ReflectAsset);
    // What they contain.
    check!(PlantParams: ReflectDefault);
    check!(PlantSegment: ReflectDefault);
    check!(PlantBranches: ReflectDefault);
    check!(PlantArrangement: ReflectDefault);
    check!(PlantVariation: ReflectDefault);
    check!(GroundTint: ReflectDefault);
    check!(PlantGround: ReflectDefault);
    check!(PlantShadows: ReflectDefault);
    check!(PlantQuality: ReflectDefault);
    check!(FieldPlant: ReflectDefault);
}

#[test]
fn a_field_survives_a_scene_file_and_can_be_inserted_from_it() {
    let mut app = app();
    let field = PlantField {
        size: Vec2::new(48.0, 24.0),
        ground: PlantGround::Flat,
        ..Default::default()
    };
    let registry = app.world().resource::<AppTypeRegistry>().clone();
    let read = through_ron(field.as_partial_reflect(), &registry.read());

    // What a scene loader does: look up the component's type data and
    // insert the deserialized value.
    let mut entity = app.world_mut().spawn_empty();
    let registry = registry.read();
    let component = registry
        .get_type_data::<ReflectComponent>(TypeId::of::<PlantField>())
        .expect("ReflectComponent");
    component.insert(&mut entity, read.as_ref(), &registry);
    let inserted = entity.get::<PlantField>().expect("inserted");
    assert_eq!(inserted.size, Vec2::new(48.0, 24.0));
    assert!(matches!(inserted.ground, PlantGround::Flat));
}

#[test]
fn the_small_components_and_the_settings_round_trip() {
    let app = app();
    let registry = app.world().resource::<AppTypeRegistry>().read();

    let camera = PlantCamera {
        radius: Some(30.0),
        shadows: Some(PlantShadows::Off),
        ..Default::default()
    };
    let read = through_ron(camera.as_partial_reflect(), &registry);
    assert_eq!(PlantCamera::from_reflect(read.as_ref()), Some(camera));

    let interactor = PlantInteractor {
        radius: 1.5,
        strength: 0.5,
        max_height: Some(1.0),
    };
    let read = through_ron(interactor.as_partial_reflect(), &registry);
    assert_eq!(
        PlantInteractor::from_reflect(read.as_ref()),
        Some(interactor)
    );

    let spread = PlantInteractorSpread {
        offsets: vec![Vec3::X, Vec3::new(0.0, 1.0, 2.0)],
    };
    let read = through_ron(spread.as_partial_reflect(), &registry);
    assert_eq!(
        PlantInteractorSpread::from_reflect(read.as_ref()),
        Some(spread)
    );

    let settings = PlantSettings {
        radius: 140.0,
        ..PlantSettings::ULTRA
    };
    let read = through_ron(settings.as_partial_reflect(), &registry);
    assert_eq!(PlantSettings::from_reflect(read.as_ref()), Some(settings));

    let read = through_ron(PlantWind::STORM.as_partial_reflect(), &registry);
    let wind = PlantWind::from_reflect(read.as_ref()).expect("a wind");
    assert_eq!(wind.strength, PlantWind::STORM.strength);
    assert_eq!(wind.gust_speed, PlantWind::STORM.gust_speed);

    let frame = PlantWindFrame::from(PlantWind::GUSTY);
    let read = through_ron(frame.as_partial_reflect(), &registry);
    assert_eq!(PlantWindFrame::from_reflect(read.as_ref()), Some(frame));

    let read = through_ron(NoPlants.as_partial_reflect(), &registry);
    assert!(NoPlants::from_reflect(read.as_ref()).is_some());
}

#[test]
fn every_preset_survives_a_save_file_and_can_be_edited_like_an_inspector_does() {
    let mut app = app();
    // Every field preset, mixed ones with all their plants included.
    {
        let registry = app.world().resource::<AppTypeRegistry>().read();
        for preset in FieldPreset::all() {
            let read = through_ron(preset.as_partial_reflect(), &registry);
            let read = FieldPreset::from_reflect(read.as_ref()).expect(&preset.key);
            assert_eq!(&read, preset);
        }
        for preset in PlantPreset::all() {
            let read = through_ron(preset.as_partial_reflect(), &registry);
            let read = PlantPreset::from_reflect(read.as_ref()).expect(&preset.key);
            assert_eq!(&read, preset);
        }
    }

    // An inspector's asset panel: add through `ReflectAsset`, edit a
    // setting by its path in the struct, and the real asset has changed.
    let registry = app.world().resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let reflect_asset = registry
        .get_type_data::<ReflectAsset>(TypeId::of::<FieldPreset>())
        .expect("ReflectAsset");
    let handle = reflect_asset.add(app.world_mut(), FieldPreset::MEADOW.as_partial_reflect());
    let asset = reflect_asset
        .get_mut(app.world_mut(), handle.id())
        .expect("the asset is there");
    *asset
        .reflect_path_mut("field.plants[0].plant.height.y")
        .unwrap()
        .try_downcast_mut::<f32>()
        .unwrap() = 2.5;
    *asset
        .reflect_path_mut("field.plants[0].plant.tufts")
        .unwrap()
        .try_downcast_mut::<f32>()
        .unwrap() = 0.7;
    let presets = app.world().resource::<Assets<FieldPreset>>();
    let edited = &presets
        .get(handle.id().typed::<FieldPreset>())
        .unwrap()
        .field;
    assert_eq!(edited.plants[0].plant.height.y, 2.5);
    assert_eq!(edited.plants[0].plant.tufts, 0.7);
    assert_eq!(
        edited.density_scale,
        FieldPreset::MEADOW.field.density_scale
    );
}

#[test]
fn a_resource_is_edited_through_reflection_like_an_inspector_does() {
    let mut wind = PlantWind::BREEZE;
    let storm = PlantWind::STORM;
    // An inspector applies a reflected value onto the live one.
    wind.apply(storm.as_partial_reflect());
    assert_eq!(wind.strength, PlantWind::STORM.strength);
    *wind
        .reflect_path_mut("gust_scale")
        .unwrap()
        .try_downcast_mut::<f32>()
        .unwrap() = 99.0;
    assert_eq!(wind.gust_scale, 99.0);

    let mut settings = PlantSettings::LOW;
    settings.apply(PlantSettings::ULTRA.as_partial_reflect());
    assert_eq!(settings, PlantSettings::ULTRA);
    // `Default` through reflection is the type's default.
    let app = app();
    let registry = app.world().resource::<AppTypeRegistry>().read();
    let make_default = registry
        .get_type_data::<ReflectDefault>(TypeId::of::<PlantSettings>())
        .unwrap();
    let value = make_default.default();
    assert_eq!(
        PlantSettings::from_reflect(value.as_partial_reflect()),
        Some(PlantSettings::HIGH)
    );
}
