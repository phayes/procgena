//! The 3D scene: environment, orbit camera, and a tree, a plant field, a rock,
//! or a tree or rock with a clump grown on it.
//!
//! Add [`ScenePlugin`] to the app, show the scene in a UI node marked
//! [`SceneViewport`], and drive it with [`ViewerCommand`] messages. The scene
//! reports back with [`ViewerEvent`] messages. The studio's layout shows how
//! the pieces fit together.
//!
//! [`headless`] helps render without a window, for tests and headless runs.

mod assets;
mod clump;
mod environment;
#[cfg(any(test, debug_assertions))]
pub mod headless;
#[cfg(test)]
mod headless_scene;
mod orbit;
mod protocol;
#[cfg(test)]
mod render_tests;
mod subject;
mod viewport;

use bevy::camera::visibility::VisibilitySystems;
use bevy::prelude::*;
use procgena_plant::prelude::PlantPlugins;
use procgena_stone_bevy::StonePlugin;
use procgena_tree::TextureCompression;
use procgena_tree_bevy::TreePlugin;

pub use assets::{STONE_TEXTURE_ROOT, TREE_TEXTURE_ROOT, add_asset_sources};
pub use environment::EnvironmentSettings;
pub use protocol::{
    BuildStats, ClumpSettings, ClumpSubject, FieldSettings, SceneMode, SceneStart, ViewerCommand,
    ViewerEvent,
};
pub use viewport::SceneViewport;

// Used by the headless studio, to tell when the scene is idle.
#[cfg(any(test, debug_assertions))]
pub use viewport::ViewportActivity;

// Used only by tests, to drive the scene and check what it shows.
#[cfg(test)]
pub use {
    orbit::OrbitCamera,
    protocol::MeshSize,
    subject::lod_levels,
    viewport::{PointerButton, ViewportCamera, ViewportInput, ViewportTarget},
};

/// The viewer scene: sky, sun, ground, an orbit camera, and a tree, a grass
/// field or a rock.
///
/// Add it after Bevy's default plugins, with [`add_asset_sources`] called
/// before them. It adds the tree, grass and rock plugins itself, spawns the
/// [`ViewportCamera`](viewport::ViewportCamera), and renders into
/// [`ViewportTarget`](viewport::ViewportTarget), which a
/// [`SceneViewport`] UI node displays.
pub struct ScenePlugin {
    start: SceneStart,
}

impl ScenePlugin {
    /// Creates a scene that first shows what `start` describes.
    pub fn new(start: SceneStart) -> Self {
        Self { start }
    }
}

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            TreePlugin {
                texture_root: Some(TREE_TEXTURE_ROOT.to_owned()),
                texture_compression: TextureCompression::Bc7,
                // The scene reports which of these levels is on screen.
                lod_levels: subject::lod_levels(),
            },
            PlantPlugins,
            StonePlugin {
                texture_root: Some(STONE_TEXTURE_ROOT.to_owned()),
                texture_compression: procgena_stone::TextureCompression::Bc7,
            },
            viewport::ViewportPlugin,
        ))
        .insert_resource(self.start.environment)
        .insert_resource(subject::LodChoice(self.start.lod))
        .insert_resource(subject::PlantLodChoice(self.start.plant_lod))
        .insert_resource(subject::Start(self.start.clone()))
        .init_resource::<clump::ClumpJob>()
        .add_message::<ViewerCommand>()
        .add_message::<ViewerEvent>()
        .add_systems(
            Startup,
            (environment::spawn_environment, subject::setup_scene),
        )
        // Host commands run before the tree and grass systems see this frame's entities.
        .add_systems(
            PreUpdate,
            (subject::apply_commands, subject::apply_plant_lod).chain(),
        )
        .add_systems(
            Update,
            (
                (orbit::drive_orbit, environment::apply_environment),
                orbit::follow_orbit,
                subject::render_while_loading,
                subject::report_lod,
                clump::grow_clump,
            )
                .chain(),
        )
        // Parts spawned during Update are hidden or shown, with their distance
        // ranges removed or restored, before Bevy decides what is visible this frame.
        .add_systems(
            PostUpdate,
            (subject::apply_lod, ApplyDeferred)
                .chain()
                .before(VisibilitySystems::VisibilityPropagate),
        )
        .add_observer(subject::report_tree)
        .add_observer(subject::report_stone);
    }
}
