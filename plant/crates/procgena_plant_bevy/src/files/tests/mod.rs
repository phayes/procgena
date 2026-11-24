use bevy::MinimalPlugins;
use bevy::asset::{AssetPlugin, AssetServer, Assets, Handle};
use bevy::math::Vec2;

use super::*;
use crate::{PlantScatter, PlantScatterOptions};

#[test]
fn every_preset_is_shipped_as_an_embedded_file_that_is_the_preset() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<PlantPresetAsset>()
        .init_asset::<PlantScatterPresetAsset>()
        .add_plugins(PlantFilePlugin);
    let (plants, scatters): (
        Vec<Handle<PlantPresetAsset>>,
        Vec<Handle<PlantScatterPresetAsset>>,
    ) = {
        let server = app.world().resource::<AssetServer>();
        let plants = PlantPreset::all()
            .map(|preset| {
                server.load(format!(
                    "embedded://procgena_plant/plants/{}.plant.yml",
                    preset.key
                ))
            })
            .collect();
        let scatters = PlantScatterPreset::all()
            .map(|preset| {
                server.load(format!(
                    "embedded://procgena_plant/scatters/{}.plant_scatter.yml",
                    preset.key
                ))
            })
            .collect();
        (plants, scatters)
    };
    let loaded = |app: &App| {
        let plant_assets = app.world().resource::<Assets<PlantPresetAsset>>();
        let scatter_assets = app.world().resource::<Assets<PlantScatterPresetAsset>>();
        plants
            .iter()
            .all(|handle| plant_assets.get(handle).is_some())
            && scatters
                .iter()
                .all(|handle| scatter_assets.get(handle).is_some())
    };
    for _ in 0..500 {
        app.update();
        if loaded(&app) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    // A loaded file's key is its file name, and it keeps its group.
    let plant_assets = app.world().resource::<Assets<PlantPresetAsset>>();
    for (preset, handle) in PlantPreset::all().zip(&plants) {
        let loaded = plant_assets
            .get(handle)
            .unwrap_or_else(|| panic!("{} did not load", preset.key));
        assert_eq!(&loaded.0, preset, "{}", preset.key);
    }
    let scatter_assets = app.world().resource::<Assets<PlantScatterPresetAsset>>();
    for (preset, handle) in PlantScatterPreset::all().zip(&scatters) {
        let loaded = scatter_assets
            .get(handle)
            .unwrap_or_else(|| panic!("{} did not load", preset.key));
        assert_eq!(&loaded.0, preset, "{}", preset.key);
    }
}

#[test]
fn a_file_holds_no_size_ground_or_map() {
    // Scatter files store appearance only; the game supplies size and placement.
    let scatter = PlantScatter::from_preset(&PlantScatterPreset::MEADOW, Vec2::splat(64.0));
    let text = scatter.options.to_yaml();
    let read = PlantScatterOptions::from_yaml(&text).unwrap();
    assert_eq!(read, PlantScatterPreset::MEADOW.plant_scatter);
    for left_out in ["size:", "ground:", "density_map:"] {
        assert!(
            !text.lines().any(|line| line.trim_start() == left_out
                || line.trim_start().starts_with(&format!("{left_out} "))),
            "{left_out}: {text}"
        );
    }
}
