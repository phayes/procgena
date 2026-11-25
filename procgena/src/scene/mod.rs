//! The 3D scene: environment, an orbiting or flying camera, and one subject, such as a
//! tree, a plant field, a rock, a clump on a tree or rock, a mushroom, a
//! fruit cluster, a grove, a piece of dead wood, an Ecotope or terrain.
//!
//! Add [`ScenePlugin`] to the app, show the scene in a UI node marked
//! [`SceneViewport`], and drive it with [`ViewerCommand`] messages: show a
//! subject with [`ViewerCommand::Show`]. The scene reports back with
//! [`ViewerEvent`] messages. The studio's layout shows how the pieces fit
//! together.
//!
//! Each kind of subject, such as [`TreeSubject`], has a module of its own.
//!
//! [`headless`] helps render without a window, for tests and headless runs.

mod assets;
mod clump;
mod deadwood;
mod ecotope;
mod environment;
mod fly;
mod fruit;
mod grove;
#[cfg(any(test, debug_assertions))]
pub mod headless;
#[cfg(test)]
mod headless_scene;
mod mushroom;
mod orbit;
mod overlay;
mod plants;
mod protocol;
#[cfg(test)]
mod render_tests;
mod stone;
mod subject;
mod terrain;
mod texture_reader;
mod tree;
mod viewport;

use bevy::camera::visibility::VisibilitySystems;
use bevy::prelude::*;
use procgena_clump_bevy::ClumpPlugin;
use procgena_deadwood_bevy::DeadwoodPlugin;
use procgena_impostor::{BakedImpostor, ImpostorPlugin, ImpostorRequest, ImpostorSettings};
use procgena_mushroom_bevy::MushroomPlugin;
use procgena_plant_bevy::prelude::PlantPlugins;
use procgena_stone_bevy::StonePlugin;
use procgena_tree::TextureCompression;
use procgena_tree_bevy::{TreePlugin, default_tree_impostor};

pub use assets::{
    CLUMP_TEXTURE_ROOT, DEADWOOD_TEXTURE_ROOT, MUSHROOM_TEXTURE_ROOT, STONE_TEXTURE_ROOT,
    TERRAIN_TEXTURE_ROOT, TREE_TEXTURE_ROOT, add_asset_sources,
};
pub use clump::{ClumpHost, ClumpSettings, ClumpSubject};
pub use deadwood::{DeadwoodSettings, DeadwoodSubject, DeadwoodView};
pub use ecotope::{EcotopeGround, EcotopeSettings, EcotopeSubject};
pub use environment::EnvironmentSettings;
pub use fly::{CameraMode, FlySpeed, ViewKeys};
pub use fruit::FruitSubject;
pub use grove::GroveSubject;
pub use mushroom::MushroomSubject;
pub use orbit::OrbitCamera;
pub use overlay::{Legend, Overlay, OverlayStatus};
pub use plants::{FieldSettings, PlantsSubject};
pub use protocol::{BuildStats, GroveProgress, ViewerCommand, ViewerEvent};
pub use stone::StoneSubject;
pub use subject::{AddSubject, Detail, Show, Shown};
pub use terrain::{TerrainSettings, TerrainShow, TerrainSubject};
pub use texture_reader::TextureDownloads;
pub use tree::TreeSubject;
pub use viewport::{SceneViewport, ViewportCamera};

// Used by the headless studio, to tell when the scene is idle.
#[cfg(any(test, debug_assertions))]
pub use viewport::ViewportActivity;

// Used only by tests, to drive the scene and check what it shows.
#[cfg(test)]
pub use {
    clump::ClumpBuildError,
    protocol::MeshSize,
    subject::Subject,
    tree::TREE_STEPS,
    viewport::{PointerButton, ViewportInput, ViewportTarget},
};

/// The viewer scene: sky, sun, ground, an orbit camera, and the subject
/// shown, which starts out empty.
///
/// Add it after Bevy's default plugins, with [`add_asset_sources`] called
/// before them. It adds the generator plugins itself, spawns the
/// [`ViewportCamera`], and renders into
/// [`ViewportTarget`](viewport::ViewportTarget), which a
/// [`SceneViewport`] UI node displays.
pub struct ScenePlugin {
    /// Whether clumps load their textures; without them they are flat colours.
    clump_textures: bool,
    /// Whether the ground loads its textures; without them it is drawn in
    /// its materials' flat colours.
    ground_textures: bool,
    /// Whether baked impostors are saved to and read from the user's cache
    /// folder; without it they are kept in memory only.
    impostor_cache: bool,
    /// Whether far trees are drawn as impostors.
    impostors: bool,
}

impl Default for ScenePlugin {
    fn default() -> Self {
        Self {
            clump_textures: true,
            ground_textures: true,
            impostor_cache: true,
            impostors: true,
        }
    }
}

impl ScenePlugin {
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

    /// Draws the ground in its materials' flat colours, without textures,
    /// so tests can recognise ground materials by colour.
    #[cfg(test)]
    pub fn without_ground_textures(mut self) -> Self {
        self.ground_textures = false;
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
                impostor: self.impostors.then(default_tree_impostor),
                ..default()
            },
            PlantPlugins,
            StonePlugin {
                texture_root: Some(STONE_TEXTURE_ROOT.to_owned()),
                texture_compression: procgena_stone::TextureCompression::Bc7,
                // The studio shows one close rock.
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
            procgena_fruit_bevy::FruitPlugin,
            terrain::plugin(),
        ))
        .insert_resource(procgena_terrain_bevy::GroundTextureSettings {
            root: self
                .ground_textures
                .then(|| TERRAIN_TEXTURE_ROOT.to_owned()),
            ..default()
        })
        .init_resource::<EnvironmentSettings>()
        .init_resource::<subject::SceneState>()
        .init_resource::<subject::ChosenDetail>()
        .init_resource::<subject::SubjectLook>()
        .init_resource::<CameraMode>()
        .init_resource::<FlySpeed>()
        .add_message::<ViewerCommand>()
        .add_message::<ViewerEvent>();

        // Each kind of subject.
        tree::plugin(app);
        plants::plugin(app);
        stone::plugin(app);
        clump::plugin(app);
        mushroom::plugin(app);
        fruit::plugin(app);
        grove::plugin(app);
        deadwood::plugin(app);
        ecotope::plugin(app);
        terrain::add_subject(app);
        overlay::plugin(app);

        app.add_systems(
            Startup,
            (environment::spawn_environment, subject::setup_scene),
        )
        // Host commands run before the generator systems see this frame's entities.
        .add_systems(
            PreUpdate,
            (subject::apply_commands, plants::apply_plant_lod).chain(),
        )
        .add_systems(
            Update,
            (
                (
                    orbit::drive_orbit,
                    fly::drive_fly,
                    environment::apply_environment,
                ),
                orbit::follow_orbit,
                render_while_baking,
                (clump::show_clump, clump::show_deadwood_cover),
            )
                .chain(),
        )
        // Parts spawned during Update are hidden or shown, with their distance
        // ranges removed or restored, before Bevy decides what is visible this frame.
        .add_systems(
            PostUpdate,
            (
                (tree::apply_lod, mushroom::apply_lod, deadwood::apply_lod),
                ApplyDeferred,
            )
                .chain()
                .before(VisibilitySystems::VisibilityPropagate),
        );
    }
}

/// Requests more frames while impostors are baking, since they bake only
/// in frames that are drawn.
fn render_while_baking(
    baking: Query<(), (With<ImpostorRequest>, Without<BakedImpostor>)>,
    mut activity: ResMut<viewport::ViewportActivity>,
) {
    if !baking.is_empty() {
        activity.request_frames(viewport::ViewportActivity::SETTLE_FRAMES);
    }
}

/// Whether the texture `texture`, if any, is still loading.
fn is_loading(server: &AssetServer, texture: &Option<Handle<Image>>) -> bool {
    texture.as_ref().is_some_and(|handle| {
        server
            .get_load_state(handle)
            .is_some_and(|state| state.is_loading())
    })
}
