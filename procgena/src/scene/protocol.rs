//! Messages between the host application and the scene.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use bevy::prelude::*;
use procgena_biome::{Ecotope, EcotopeError, EcotopePreset};
use procgena_biome_bevy::EcotopeSummary;
use procgena_clump::{ClumpOptions, ClumpPreset};
use procgena_deadwood::DeadwoodOptions;
use procgena_deadwood_bevy::DeadwoodBuildError;
use procgena_fruit::FruitOptions;
use procgena_mushroom::MushroomOptions;
use procgena_mushroom_bevy::MushroomBuildError;
use procgena_plant::prelude::{PlantScatter, PlantScatterPreset};
use procgena_stone::{OptionsError as StoneOptionsError, StoneOptions};
use procgena_terrain_bevy::TerrainProgress;
use procgena_tree::{OptionsError, TreeOptions, TreePreset, TreeScatter};
use procgena_tree_bevy::TreeScatterBuildError;

use super::environment::EnvironmentSettings;
use super::terrain::TerrainSettings;

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
    /// Clump subject and settings, as for [`ViewerCommand::SetClump`].
    pub clump: Arc<ClumpSettings>,
    /// Mushroom settings, as for [`ViewerCommand::SetMushroom`].
    pub mushroom: Arc<MushroomOptions>,
    /// Fruit or flower cluster settings, as for [`ViewerCommand::SetFruit`].
    pub fruit: Arc<FruitOptions>,
    /// The tree Scatter, as for [`ViewerCommand::SetGrove`].
    pub grove: Arc<TreeScatter>,
    /// Dead wood settings, as for [`ViewerCommand::SetDeadwood`].
    pub deadwood: Arc<DeadwoodSettings>,
    /// The Ecotope and its ground, as for [`ViewerCommand::SetEcotope`].
    pub ecotope: Arc<EcotopeSettings>,
    /// Terrain settings, as for [`ViewerCommand::SetTerrain`].
    pub terrain: Arc<TerrainSettings>,
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
            tree: Arc::new(TreePreset::oak_medium().tree.clone()),
            field: Arc::default(),
            stone: Arc::default(),
            clump: Arc::default(),
            mushroom: Arc::default(),
            fruit: Arc::default(),
            grove: Arc::default(),
            deadwood: Arc::default(),
            ecotope: Arc::default(),
            terrain: Arc::default(),
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
    /// A tree or rock with a clump, such as snow or moss, grown on it.
    Clump,
    /// A single mushroom generated from the current mushroom settings.
    Mushroom,
    /// A grove of trees on a square of flat ground,
    /// [`EnvironmentSettings::grove_size`] across.
    Grove,
    /// A single log, branch or stump generated from the current dead wood
    /// settings.
    Deadwood,
    /// An Ecotope's Scatters over a square of ground; see
    /// [`ViewerCommand::SetEcotope`].
    Ecotope,
    /// An endless world of terrain, streamed around the camera.
    Terrain,
    /// A single fruit or flower cluster, hanging from or standing on its
    /// attachment point, from the current cluster settings.
    Fruit,
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
    /// Sets the clump mode's subject, clump settings and colour. They are kept
    /// apart from the tree and rock modes' settings. In clump mode, a changed
    /// subject is rebuilt and then grows a new clump; changed clump settings
    /// grow a new clump on the subject already shown. Each clump build reports
    /// a [`ViewerEvent::ClumpBuilt`]. In other modes, saves the settings until
    /// clump mode is shown.
    SetClump(Arc<ClumpSettings>),
    /// Sets the mushroom settings. In mushroom mode, builds a replacement in
    /// the background and reports the result through
    /// [`ViewerEvent::MushroomBuilt`]; the previous mushroom stays visible if
    /// the new settings are rejected. In other modes, saves the settings
    /// until mushroom mode is shown. The scene uses its own detail levels,
    /// scaled to the mushroom's size, and ignores `options.detail`.
    SetMushroom(Arc<MushroomOptions>),
    /// Sets the fruit or flower cluster settings. In fruit mode, grows the
    /// cluster again at once; invalid settings keep the previous cluster.
    /// In other modes, saves the settings until fruit mode is shown.
    SetFruit(Arc<FruitOptions>),
    /// Sets the tree Scatter. In grove mode, the Scatter updates in place:
    /// trees that keep their spot, species and shape stay, and the rest are
    /// added, removed or rebuilt. A [`ViewerEvent::GroveBuilt`] follows. In
    /// other modes, saves the Scatter until grove mode is shown. The trees
    /// grow on a square of flat ground [`EnvironmentSettings::grove_size`]
    /// across.
    SetGrove(Arc<TreeScatter>),
    /// Sets what dead wood mode shows: a piece, with any cover grown on it,
    /// or a scatter of sticks. In dead wood mode, a changed piece or view is
    /// built in the background and reported through
    /// [`ViewerEvent::DeadwoodBuilt`]; a changed piece keeps the previous
    /// one visible until it is ready, or if its settings are rejected. A
    /// changed cover alone regrows only the cover. In other modes, saves the
    /// settings until dead wood mode is shown. The scene uses its own detail
    /// levels, scaled to the piece's size, and ignores `piece.detail`.
    SetDeadwood(Arc<DeadwoodSettings>),
    /// Sets the Ecotope and the ground it is shown on. In Ecotope mode, the
    /// Ecotope is placed again in the background and only the Scatters the
    /// change can move are drawn again; a [`ViewerEvent::EcotopeBuilt`]
    /// follows. If it can't be placed, the last drawing stays. In other
    /// modes, saves the settings until Ecotope mode is shown.
    SetEcotope(Arc<EcotopeSettings>),
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
    /// Shows only the mushroom's detail level `n` (0 is the most detailed;
    /// see [`BuildStats::levels`]) at every camera distance, or with `None`,
    /// the level that suits the camera distance. A
    /// [`ViewerEvent::MushroomLodShown`] follows in mushroom mode.
    SetMushroomLod(Option<usize>),
    /// Shows only the dead wood piece's detail level `n` (0 is the most
    /// detailed; see [`BuildStats::levels`]) at every camera distance, or
    /// with `None`, the level that suits the camera distance. A
    /// [`ViewerEvent::DeadwoodLodShown`] follows in dead wood mode.
    SetDeadwoodLod(Option<usize>),
    /// Sets what terrain mode shows. In terrain mode, changed terrain
    /// settings build a new world, whose chunks replace the old ones once
    /// ready; changed debug drawing alone applies at once. In other modes,
    /// saves the settings until terrain mode is shown.
    SetTerrain(Arc<TerrainSettings>),
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
    pub field: PlantScatter,
}

impl Default for FieldSettings {
    /// Starts with the meadow Plant Scatter preset.
    fn default() -> Self {
        Self {
            field: PlantScatterPreset::MEADOW.plant_scatter,
        }
    }
}

/// What dead wood mode shows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DeadwoodSettings {
    /// The log, branch or stump shown in [`DeadwoodView::Piece`].
    pub piece: DeadwoodOptions,
    /// A single piece, or a scatter of sticks.
    pub view: DeadwoodView,
    /// A clump, such as moss, grown on the piece, or `None` for bare wood.
    /// Scatters are always bare.
    pub cover: Option<&'static ClumpPreset>,
}

/// Whether dead wood mode shows one piece or a scatter of sticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DeadwoodView {
    /// The piece from [`DeadwoodSettings::piece`].
    #[default]
    Piece,
    /// [`ScatterOptions::kindling`](procgena_deadwood::ScatterOptions::kindling):
    /// sticks 1 to 7 cm thick on flat ground.
    Kindling,
    /// [`ScatterOptions::twig_litter`](procgena_deadwood::ScatterOptions::twig_litter):
    /// thin twigs on flat ground.
    TwigLitter,
}

/// What Ecotope mode shows: an Ecotope over a square of ground.
#[derive(Debug, Clone, PartialEq)]
pub struct EcotopeSettings {
    /// The Ecotope, placed with its own
    /// [`default_seed`](Ecotope::default_seed).
    pub ecotope: Ecotope,
    /// The ground it grows on.
    pub ground: EcotopeGround,
    /// Side of the square patch it covers, in metres, centred on the
    /// origin. The ground reaches a few metres beyond it.
    pub size: f32,
}

impl Default for EcotopeSettings {
    /// The pine forest floor over 40 metres of rolling hills.
    fn default() -> Self {
        Self {
            ecotope: EcotopePreset::pine_forest_floor().ecotope.clone(),
            ground: EcotopeGround::default(),
            size: 40.0,
        }
    }
}

/// The ground an Ecotope is shown on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EcotopeGround {
    /// Level ground at height 0.
    Flat,
    /// Long swells about 2 metres high with a shallower ripple across them.
    #[default]
    RollingHills,
}

/// What the clump mode shows: a subject, and the clump grown on it.
#[derive(Debug, Clone, PartialEq)]
pub struct ClumpSettings {
    /// The tree or rock the clump grows on.
    pub subject: ClumpSubject,
    /// The clump's settings, including how it looks. Lengths are in metres,
    /// like the subject.
    pub clump: ClumpOptions,
}

impl Default for ClumpSettings {
    /// A fresh snow cap on a rock with default settings.
    fn default() -> Self {
        Self {
            subject: ClumpSubject::Stone(Arc::default()),
            clump: ClumpOptions::default(),
        }
    }
}

/// The subject a clump grows on.
///
/// A clump grows on a tree's trunk and branches at full detail, not on its
/// leaves, fruit or flowers, and on a rock's most detailed mesh.
#[derive(Debug, Clone, PartialEq)]
pub enum ClumpSubject {
    /// A tree built from these options.
    Tree(Arc<TreeOptions>),
    /// A rock built from these settings.
    Stone(Arc<StoneOptions>),
}

/// Why a clump could not be shown.
#[derive(Debug, Clone, PartialEq)]
pub enum ClumpBuildError {
    /// The subject tree's options were rejected.
    Tree(OptionsError),
    /// The subject rock's settings were rejected.
    Stone(StoneOptionsError),
    /// The clump's settings were rejected, or its subject can't carry one.
    Clump(procgena_clump_bevy::ClumpBuildError),
}

impl fmt::Display for ClumpBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tree(error) => write!(f, "cannot build the tree: {error}"),
            Self::Stone(error) => write!(f, "cannot build the rock: {error}"),
            Self::Clump(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ClumpBuildError {}

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
    /// Reports the result of growing a clump in clump mode: its mesh size,
    /// as a single detail level, and build time on success, or why the clump
    /// or its subject could not be built. A rejected clump leaves the
    /// subject shown without one.
    ClumpBuilt(Result<BuildStats, ClumpBuildError>),
    /// Reports the result of generating a mushroom: mesh sizes per detail
    /// level and build time on success, or why its settings were rejected.
    /// A rejected replacement leaves the previous mushroom visible.
    MushroomBuilt(Result<BuildStats, MushroomBuildError>),
    /// Reports the result of growing a fruit or flower cluster: mesh sizes
    /// per detail level on success, or why its settings were rejected. A
    /// rejected cluster leaves the previous one visible.
    FruitBuilt(Result<BuildStats, procgena_fruit::OptionsError>),
    /// Reports the result of building the grove in grove mode: how many
    /// trees it has and their meshes added together, or why it could not be
    /// built.
    GroveBuilt(Result<GroveStats, TreeScatterBuildError>),
    /// How far the grove shown in grove mode has got, sent whenever that
    /// changes. Also sent when grove mode is shown.
    GroveProgress(GroveProgress),
    /// Reports the result of generating a dead wood piece: mesh sizes per
    /// detail level and build time on success, or why its settings were
    /// rejected. A rejected replacement leaves the previous piece visible.
    DeadwoodBuilt(Result<BuildStats, DeadwoodBuildError>),
    /// The tree's detail level now on screen changed, because of
    /// [`ViewerCommand::SetLod`] or because the camera moved. Also sent when
    /// tree mode is shown.
    LodShown(usize),
    /// The mushroom's detail level now on screen changed, because of
    /// [`ViewerCommand::SetMushroomLod`] or because the camera moved. `None`
    /// means the camera is too far away for any level to be drawn. Also sent
    /// when mushroom mode is shown.
    MushroomLodShown(Option<usize>),
    /// The dead wood piece's detail level now on screen changed, because of
    /// [`ViewerCommand::SetDeadwoodLod`] or because the camera moved. `None`
    /// means the camera is too far away for any level to be drawn. Also
    /// sent when dead wood mode is shown.
    DeadwoodLodShown(Option<usize>),
    /// Reports each placement of the Ecotope in Ecotope mode: how many
    /// copies each Scatter has and which were drawn again, or why it could
    /// not be placed. The copies' meshes may still be generating.
    EcotopeBuilt(Result<EcotopeSummary, EcotopeError>),
    /// How far the terrain in terrain mode has got, sent whenever that
    /// changes.
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
