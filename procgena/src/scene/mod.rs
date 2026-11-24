//! The 3D scene: environment, orbit camera, and a tree, a plant field, a rock,
//! a tree or rock with a clump grown on it, a mushroom, a grove, a piece
//! of dead wood, or an Ecotope.
//!
//! Add [`ScenePlugin`] to the app, show the scene in a UI node marked
//! [`SceneViewport`], and drive it with [`ViewerCommand`] messages. The scene
//! reports back with [`ViewerEvent`] messages. The studio's layout shows how
//! the pieces fit together.
//!
//! [`headless`] helps render without a window, for tests and headless runs.

mod assets;
mod clump;
mod ecotope;
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
mod terrain;
mod texture_reader;
mod viewport;

use bevy::camera::visibility::VisibilitySystems;
use bevy::prelude::*;
use procgena_clump_bevy::ClumpPlugin;
use procgena_core::Lod;
use procgena_deadwood_bevy::DeadwoodPlugin;
use procgena_impostor::{ImpostorLevel, ImpostorPlugin, ImpostorSettings};
use procgena_mushroom_bevy::MushroomPlugin;
use procgena_plant::prelude::PlantPlugins;
use procgena_stone_bevy::StonePlugin;
use procgena_tree::{LOD_REFERENCE_SIZE, TextureCompression};
use procgena_tree_bevy::{TreePlugin, default_tree_impostor};

pub use assets::{
    CLUMP_TEXTURE_ROOT, DEADWOOD_TEXTURE_ROOT, MUSHROOM_TEXTURE_ROOT, STONE_TEXTURE_ROOT,
    TREE_TEXTURE_ROOT, add_asset_sources,
};
pub use environment::EnvironmentSettings;
pub use protocol::{
    BuildStats, ClumpSettings, ClumpSubject, DeadwoodSettings, DeadwoodView, EcotopeGround,
    EcotopeSettings, FieldSettings, GroveProgress, SceneMode, SceneStart, ViewerCommand,
    ViewerEvent,
};
pub use terrain::{TerrainSettings, TerrainShow};
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
    /// Whether baked impostors are saved to and read from the user's cache
    /// folder; without it they are kept in memory only.
    impostor_cache: bool,
    /// Whether far trees are drawn as impostors.
    impostors: bool,
}

impl ScenePlugin {
    /// Creates a scene that first shows what `start` describes.
    pub fn new(start: SceneStart) -> Self {
        Self {
            start,
            clump_textures: true,
            impostor_cache: true,
            impostors: true,
        }
    }

    /// Draws far trees at their farthest mesh level at every distance
    /// beyond it, never as impostors, so tests can compare the two.
    #[cfg(test)]
    pub fn without_impostors(mut self) -> Self {
        self.impostors = false;
        self
    }

    /// Bakes every impostor afresh and keeps it in memory only, so tests
    /// neither depend on nor change the user's cache folder.
    #[cfg(test)]
    pub fn without_impostor_cache(mut self) -> Self {
        self.impostor_cache = false;
        self
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
        // Impostors baked for far trees are kept between runs in the user's
        // cache folder, beside the downloaded textures.
        let cache_dir = dirs::cache_dir()
            .filter(|_| self.impostor_cache)
            .map(|dir| dir.join("procgena").join("impostors"));
        app.add_plugins(ImpostorPlugin {
            settings: ImpostorSettings {
                cache_dir,
                ..default()
            },
        });
        app.add_plugins((
            TreePlugin {
                texture_root: Some(TREE_TEXTURE_ROOT.to_owned()),
                texture_compression: TextureCompression::Bc7,
                // The scene reports which of these levels is on screen.
                lod: Lod::new(LOD_REFERENCE_SIZE, subject::lod_levels()),
                impostor: self.impostors.then(|| ImpostorLevel {
                    distance: subject::IMPOSTOR_DISTANCE,
                    ..default_tree_impostor()
                }),
            },
            PlantPlugins,
            StonePlugin {
                texture_root: Some(STONE_TEXTURE_ROOT.to_owned()),
                texture_compression: procgena_stone::TextureCompression::Bc7,
                // The studio's rock tab draws one close rock.
                impostor: None,
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
        .add_plugins((
            procgena_biome_bevy::EcotopePlugin,
            ecotope::EcotopeScenePlugin,
        ))
        .add_plugins(procgena_fruit_bevy::FruitPlugin)
        .add_plugins(terrain::plugin())
        .add_systems(
            Update,
            (
                terrain::apply_show,
                terrain::hide_ground.after(environment::apply_environment),
                terrain::report_progress,
            ),
        )
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
        // After Update, once the tree plugin has asked for this frame's bakes.
        .add_systems(PostUpdate, subject::report_grove_progress)
        .add_observer(subject::report_tree)
        .add_observer(subject::report_stone)
        .add_observer(subject::report_mushroom)
        .add_observer(subject::report_fruit)
        .add_observer(subject::report_deadwood)
        .add_observer(subject::report_grove)
        .add_observer(clump::report_clump);
    }
}
