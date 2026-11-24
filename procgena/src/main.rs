//! procgena: a desktop studio for procedural trees, plants and rocks.
//!
//! Debug builds also accept `--headless <script>`, which runs the studio
//! without a window and follows the script; see [`headless`].

mod clump;
mod deadwood;
mod download;
mod ecotope;
mod environment;
mod export;
mod field;
mod fruit;
mod grove;
#[cfg(any(test, debug_assertions))]
mod headless;
mod mushrooms;
mod params;
mod plant;
mod scene;
mod stones;
mod studio;
mod terrain;
mod tree;
mod ui;

use bevy::feathers::FeathersPlugins;
use bevy::prelude::*;
use bevy::winit::WinitSettings;
use scene::{EnvironmentSettings, ScenePlugin};

use params::{PanelWidgetsPlugin, ParamPanelPlugin};

fn main() {
    #[cfg(debug_assertions)]
    if let Some(code) = headless::run_from_args() {
        std::process::exit(code);
    }

    let mut app = App::new();
    scene::add_asset_sources(&mut app);
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "procgena".into(),
            resolution: (1440, 900).into(),
            ..default()
        }),
        ..default()
    }))
    // Sleep while nothing changes; the scene asks for frames when it needs them.
    .insert_resource(WinitSettings::desktop_app());
    add_studio(&mut app);
    app.run();
}

/// Adds the studio to `app`, which must already have Bevy's default plugins
/// (with or without a window) and the scene's asset sources.
fn add_studio(app: &mut App) {
    app.add_plugins((
        FeathersPlugins,
        ui::UiPlugin,
        ScenePlugin::new(studio::scene_start()),
        PanelWidgetsPlugin,
        ParamPanelPlugin::<EnvironmentSettings>::default(),
        studio::StudioPlugin,
    ));
}
