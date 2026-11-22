//! Messages between the host application and the scene.

use std::sync::Arc;
use std::time::Duration;

use bevy::prelude::*;
use procgena_plant::prelude::{FieldPreset, PlantField};
use procgena_stone::{OptionsError as StoneOptionsError, StoneOptions};
use procgena_tree::{OptionsError, TreeOptions, TreePreset};

use super::environment::EnvironmentSettings;

/// What a new scene shows first, and the settings it starts with.
///
/// Starting a scene with the right settings, rather than sending commands
/// after it starts, avoids building a tree or rock that is replaced at once.
#[derive(Debug, Clone)]
pub struct SceneStart {
    /// The mode shown first.
    pub mode: SceneMode,
    /// Tree options, as for [`ViewerCommand::SetTree`].
    pub tree: Arc<TreeOptions>,
    /// Initial plants and field appearance; see [`ViewerCommand::SetField`].
    pub field: Arc<FieldSettings>,
    /// Rock settings, as for [`ViewerCommand::SetStone`].
    pub stone: Arc<StoneOptions>,
    /// Lighting, ground, wind and auto-rotation, as for [`ViewerCommand::SetEnvironment`].
    pub environment: EnvironmentSettings,
    /// The tree detail level to show, as for [`ViewerCommand::SetLod`].
    pub lod: Option<usize>,
    /// The grass and plant detail level to show, as for [`ViewerCommand::SetPlantLod`].
    pub plant_lod: Option<u32>,
}

impl Default for SceneStart {
    /// Tree mode showing the Oak Medium preset, with default settings elsewhere.
    fn default() -> Self {
        Self {
            mode: SceneMode::default(),
            tree: Arc::new(TreePreset::OakMedium.options()),
            field: Arc::default(),
            stone: Arc::default(),
            environment: EnvironmentSettings::default(),
            lod: None,
            plant_lod: None,
        }
    }
}

/// What the scene shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SceneMode {
    /// A single generated tree.
    #[default]
    Tree,
    /// A field of grass and plants.
    Plants,
    /// A single rock generated from the current stone settings.
    Stone,
}

/// A change requested by the host application.
///
/// Write these as Bevy messages, for example with a `MessageWriter<ViewerCommand>`
/// or `world.write_message(ViewerCommand::SetMode(SceneMode::Plants))`.
/// Commands are applied in order at the start of the next frame.
#[derive(Message, Debug, Clone)]
pub enum ViewerCommand {
    /// Switches what the scene shows. Each mode remembers its own camera view.
    SetMode(SceneMode),
    /// Replaces the tree's options. The tree regenerates in the background and
    /// a [`ViewerEvent::TreeBuilt`] follows. The options are kept while another
    /// mode is shown.
    SetTree(Arc<TreeOptions>),
    /// Replaces the field's plants and appearance on the next frame.
    /// The settings are retained while another scene mode is active.
    /// Field size remains controlled by [`EnvironmentSettings::field_size`].
    SetField(Arc<FieldSettings>),
    /// Sets the rock settings. In stone mode, builds a replacement in the
    /// background and reports the result through [`ViewerEvent::StoneBuilt`].
    /// The previous rock stays visible if the new settings are rejected.
    /// In other modes, saves the settings until stone mode is shown.
    /// `options.mesh.detail` sets the rock's detail levels: each level's mesh
    /// resolution, switching distance and texture size.
    SetStone(Arc<StoneOptions>),
    /// Replaces the lighting, ground, wind and auto-rotation settings.
    SetEnvironment(EnvironmentSettings),
    /// Shows only the tree's detail level `n` (0 is the most detailed; see
    /// [`BuildStats::levels`]) at every camera distance, or with `None`, the
    /// level that suits the camera distance. A [`ViewerEvent::LodShown`] follows.
    SetLod(Option<usize>),
    /// Draws every grass blade and plant at detail level `n` (0 is the most
    /// detailed, 2 the coarsest) at every camera distance, or with `None`, at the
    /// level that suits the distance. Levels above 2 show level 2.
    SetPlantLod(Option<u32>),
    /// Returns the camera to the current mode's starting view.
    ResetCamera,
}

/// Plants and appearance for the scene's plant preview.
///
/// Used in [`SceneMode::Plants`]. Set the preview size separately with
/// [`EnvironmentSettings::field_size`].
#[derive(Debug, Clone, PartialEq, Reflect)]
pub struct FieldSettings {
    /// Plants, relative weights and shared appearance settings.
    /// The scene supplies size, flat ground and no density map, ignoring
    /// those parts of this value. Keep values within procgena_plant's documented
    /// ranges; assigning the component does not run file-loader validation.
    pub field: PlantField,
}

impl Default for FieldSettings {
    /// Starts with the meadow field preset.
    fn default() -> Self {
        Self {
            field: FieldPreset::MEADOW.field,
        }
    }
}

/// A notification from the scene to the host application, delivered as a Bevy
/// message. Read it with a `MessageReader<ViewerEvent>`.
#[derive(Message, Debug, Clone, PartialEq)]
pub enum ViewerEvent {
    /// The tree finished regenerating after it was shown or its options changed,
    /// or its options were rejected.
    TreeBuilt(Result<BuildStats, OptionsError>),
    /// Reports the result of generating a rock: mesh sizes and build time on
    /// success, or the reason its settings were rejected. A rejected
    /// replacement leaves the previous rock visible.
    StoneBuilt(Result<BuildStats, StoneOptionsError>),
    /// The tree's detail level now on screen changed, because of
    /// [`ViewerCommand::SetLod`] or because the camera moved. Also sent when
    /// tree mode is shown.
    LodShown(usize),
}

/// Mesh counts and elapsed build time for a generated tree or rock.
/// Detail levels are simpler versions used when the camera is farther away.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildStats {
    /// Vertex and triangle counts for each detail level, most detailed first.
    /// Trellis wires count toward every tree level. A rock that disappears
    /// completely at a detail level has zero counts for that level.
    pub levels: Vec<MeshSize>,
    /// Elapsed time from the latest build request until generation finishes,
    /// including time waiting for earlier work. This does not wait for textures to finish loading.
    pub build_time: Duration,
}

/// Size of a mesh, or of several meshes together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MeshSize {
    /// Number of mesh vertices.
    pub vertices: usize,
    /// Number of mesh triangles.
    pub triangles: usize,
}
