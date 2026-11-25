//! Messages between the host application and the scene.

use std::time::Duration;

use bevy::prelude::*;
use procgena_biome::EcotopeError;
use procgena_biome_bevy::EcotopeSummary;
use procgena_deadwood_bevy::DeadwoodBuildError;
use procgena_mushroom_bevy::MushroomBuildError;
use procgena_stone::OptionsError as StoneOptionsError;
use procgena_terrain_bevy::TerrainProgress;
use procgena_tree::OptionsError;
use procgena_tree_bevy::TreeScatterBuildError;

use super::clump::ClumpBuildError;
use super::environment::EnvironmentSettings;
use super::fly::CameraMode;
use super::orbit::OrbitCamera;
use super::subject::{Detail, Show};

/// A change requested by the host application.
///
/// Write these as Bevy messages, for example with a `MessageWriter<ViewerCommand>`.
/// Commands are applied in order at the start of the next frame.
#[derive(Message, Debug, Clone)]
pub enum ViewerCommand {
    /// Shows a subject, such as a tree, built from its settings; see
    /// [`Show::new`]. Showing another document replaces the subject and fits
    /// the camera to it; showing the same document again updates it.
    Show(Show),
    /// Removes the shown subject, leaving the view empty.
    Clear,
    /// Replaces the lighting, ground, wind, auto-rotation and preview area
    /// settings.
    SetEnvironment(EnvironmentSettings),
    /// Sets which detail levels subjects with levels draw. Some subjects
    /// draw a detail of their own whatever is chosen. The scene starts with
    /// [`Detail::default`].
    SetDetail(Detail),
    /// Returns the camera to the shown subject's starting view.
    ResetCamera,
    /// Points the camera with this view, for example one saved earlier.
    SetCamera(OrbitCamera),
    /// Switches between orbiting and flying; see [`CameraMode`]. The
    /// camera keeps its place. The scene starts in [`CameraMode::Orbit`],
    /// and showing, resetting or setting the camera keeps the mode.
    SetCameraMode(CameraMode),
}

/// A notification from the scene to the host application, delivered as a Bevy
/// message. Read it with a `MessageReader<ViewerEvent>`.
#[derive(Message, Debug, Clone, PartialEq)]
pub enum ViewerEvent {
    /// The shown subject started building: a newly shown subject, or new
    /// settings that rebuild it. A report of its build follows.
    Building,
    /// The tree finished regenerating after it was shown or its options changed,
    /// or its options were rejected.
    TreeBuilt(Result<BuildStats, OptionsError>),
    /// Reports the result of generating a rock: mesh sizes and build time on
    /// success, or the reason its settings were rejected. A rejected
    /// replacement leaves the previous rock visible.
    StoneBuilt(Result<BuildStats, StoneOptionsError>),
    /// Reports the result of growing a clump: its mesh size, as a single
    /// detail level, and build time on success, or why the clump or its
    /// host could not be built. A rejected clump leaves the host shown
    /// without one.
    ClumpBuilt(Result<BuildStats, ClumpBuildError>),
    /// Reports the result of generating a mushroom: mesh sizes per detail
    /// level and build time on success, or why its settings were rejected.
    /// A rejected replacement leaves the previous mushroom visible.
    MushroomBuilt(Result<BuildStats, MushroomBuildError>),
    /// Reports the result of growing a fruit or flower cluster: mesh sizes
    /// per detail level on success, or why its settings were rejected. A
    /// rejected cluster leaves the previous one visible.
    FruitBuilt(Result<BuildStats, procgena_fruit::OptionsError>),
    /// Reports the result of building the shown grove: how many trees it
    /// has and their meshes added together, or why it could not be built.
    GroveBuilt(Result<GroveStats, TreeScatterBuildError>),
    /// How far the shown grove has got, sent whenever that changes. Also
    /// sent when a grove is shown.
    GroveProgress(GroveProgress),
    /// Reports the result of generating a dead wood piece: mesh sizes per
    /// detail level and build time on success, or why its settings were
    /// rejected. A rejected replacement leaves the previous piece visible.
    DeadwoodBuilt(Result<BuildStats, DeadwoodBuildError>),
    /// The shown tree's detail level on screen changed, because of
    /// [`ViewerCommand::SetDetail`] or because the camera moved. Also sent
    /// when a tree is shown.
    LodShown(usize),
    /// The shown mushroom's detail level on screen changed, because of
    /// [`ViewerCommand::SetDetail`] or because the camera moved. `None`
    /// means the camera is too far away for any level to be drawn. Also
    /// sent when a mushroom is shown.
    MushroomLodShown(Option<usize>),
    /// The shown dead wood piece's detail level on screen changed, because
    /// of [`ViewerCommand::SetDetail`] or because the camera moved. `None`
    /// means the camera is too far away for any level to be drawn. Also
    /// sent when a piece is shown.
    DeadwoodLodShown(Option<usize>),
    /// Reports each placement of the shown Ecotope: how many copies each
    /// Scatter has and which were drawn again, or why it could not be
    /// placed. The copies' meshes may still be generating.
    EcotopeBuilt(Result<EcotopeSummary, EcotopeError>),
    /// How far the shown terrain has got, sent whenever that changes.
    TerrainProgress(TerrainProgress),
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

/// The size of a built grove.
#[derive(Debug, Clone, PartialEq)]
pub struct GroveStats {
    /// How many trees the grove has.
    pub trees: usize,
    /// Vertex and triangle counts of every tree together, for each tree
    /// detail level, and the build time. Trees that share a mesh are each
    /// counted.
    pub build: BuildStats,
}

/// How far the grove shown has got. Its trees are generated first, one
/// shape per species variant that the grove places; then the impostors its
/// far trees are drawn with are baked, one per shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroveProgress {
    /// The grove is being built: `done` of its `total` tree shapes are
    /// generated. `total` is 0 until the trees are placed.
    Generating {
        /// Tree shapes generated.
        done: usize,
        /// Tree shapes the grove places.
        total: usize,
    },
    /// The grove is built and drawn, and the impostors of `done` of its
    /// `total` generated tree shapes are baked. Until a shape's impostor is
    /// baked, its trees are drawn at their farthest mesh level at every
    /// distance.
    Baking {
        /// Impostors baked.
        done: usize,
        /// Generated tree shapes.
        total: usize,
    },
    /// Nothing is left to do.
    #[default]
    Idle,
}

/// Size of a mesh, or of several meshes together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MeshSize {
    /// Number of mesh vertices.
    pub vertices: usize,
    /// Number of mesh triangles.
    pub triangles: usize,
}
