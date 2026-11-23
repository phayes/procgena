//! The 3D scene: environment, orbit camera, and a tree, a plant field, a rock,
//! a tree or rock with a clump grown on it, a mushroom, a grove, or a piece
//! of dead wood.
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
mod texture_reader;
mod viewport;

use bevy::camera::visibility::VisibilitySystems;
use bevy::prelude::*;
use procgena_clump_bevy::ClumpPlugin;
use procgena_deadwood_bevy::DeadwoodPlugin;
use procgena_mushroom_bevy::MushroomPlugin;
use procgena_plant::prelude::PlantPlugins;
use procgena_stone_bevy::StonePlugin;
use procgena_tree::TextureCompression;
use procgena_tree_bevy::TreePlugin;

pub use assets::{
    CLUMP_TEXTURE_ROOT, DEADWOOD_TEXTURE_ROOT, MUSHROOM_TEXTURE_ROOT, STONE_TEXTURE_ROOT,
    TREE_TEXTURE_ROOT, add_asset_sources,
};
pub use environment::EnvironmentSettings;
pub use protocol::{
    BuildStats, ClumpSettings, ClumpSubject, DeadwoodSettings, DeadwoodView, FieldSettings,
    GroveSettings, SceneMode, SceneStart, ViewerCommand, ViewerEvent,
};
pub use texture_reader::TextureDownloads;
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
    /// Whether clumps load their textures; without them they are flat colours.
    clump_textures: bool,
}

impl ScenePlugin {
    /// Creates a scene that first shows what `start` describes.
    pub fn new(start: SceneStart) -> Self {
        Self {
            start,
            clump_textures: true,
        }
    }

    /// Draws clumps in their plain material colours, without textures, so
    /// tests can recognise them by colour.
    #[cfg(test)]
    pub fn without_clump_textures(mut self) -> Self {
        self.clump_textures = false;
        self
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
            ClumpPlugin {
                texture_root: self.clump_textures.then(|| CLUMP_TEXTURE_ROOT.to_owned()),
                ..default()
            },
            MushroomPlugin {
                texture_root: Some(MUSHROOM_TEXTURE_ROOT.to_owned()),
                ..default()
            },
            DeadwoodPlugin {
                texture_root: Some(DEADWOOD_TEXTURE_ROOT.to_owned()),
                ..default()
            },
            viewport::ViewportPlugin,
        ))
        .insert_resource(self.start.environment)
        .insert_resource(subject::LodChoice(self.start.lod))
        .insert_resource(subject::PlantLodChoice(self.start.plant_lod))
        .init_resource::<subject::MushroomLodChoice>()
        .init_resource::<subject::DeadwoodLodChoice>()
        .insert_resource(subject::Start(self.start.clone()))
        .init_resource::<clump::ShownClump>()
        .init_resource::<clump::ShownCover>()
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
                (subject::render_while_loading, clump::render_while_loading),
                (
                    subject::report_lod,
                    subject::report_mushroom_lod,
                    subject::report_deadwood_lod,
                ),
                (clump::show_clump, clump::show_deadwood_cover),
            )
                .chain(),
        )
        // Parts spawned during Update are hidden or shown, with their distance
        // ranges removed or restored, before Bevy decides what is visible this frame.
        .add_systems(
            PostUpdate,
            (
                (
                    subject::apply_lod,
                    subject::apply_mushroom_lod,
                    subject::apply_deadwood_lod,
                ),
                ApplyDeferred,
            )
                .chain()
                .before(VisibilitySystems::VisibilityPropagate),
        )
        .add_observer(subject::report_tree)
        .add_observer(subject::report_stone)
        .add_observer(subject::report_mushroom)
        .add_observer(subject::report_deadwood)
        .add_observer(subject::report_grove)
        .add_observer(clump::report_clump);
    }
}
