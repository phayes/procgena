//! The studio's tabs and what each one shows.

use crate::scene::SceneMode;

/// The tabs along the top of the settings panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StudioTab {
    /// A single tree.
    #[default]
    Trees,
    /// A tree Scatter mixing several kinds of tree.
    TreeScatter,
    /// One plant, previewed on its own.
    Plant,
    /// A Plant Scatter mixing several plants.
    PlantScatter,
    /// A single rock.
    Stones,
    /// A tree or rock with a clump, such as snow or moss, grown on it.
    Clumps,
    /// A single mushroom.
    Mushrooms,
    /// A single fallen log, branch or stump.
    Deadwood,
    /// An Ecotope: trees, stones, dead wood and mushrooms over a patch of
    /// ground.
    Ecotope,
    /// An endless world of terrain.
    Terrain,
}

impl StudioTab {
    /// Every tab, left to right.
    pub const ALL: [StudioTab; 10] = [
        StudioTab::Trees,
        StudioTab::TreeScatter,
        StudioTab::Plant,
        StudioTab::PlantScatter,
        StudioTab::Stones,
        StudioTab::Clumps,
        StudioTab::Mushrooms,
        StudioTab::Deadwood,
        StudioTab::Ecotope,
        StudioTab::Terrain,
    ];

    /// The tab's label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Trees => "Trees",
            Self::TreeScatter => "Tree Scatter",
            Self::Plant => "Plant",
            Self::PlantScatter => "Plant Scatter",
            Self::Stones => "Stones",
            Self::Clumps => "Clumps",
            Self::Mushrooms => "Mushrooms",
            Self::Deadwood => "Dead Wood",
            Self::Ecotope => "Ecotope",
            Self::Terrain => "Terrain",
        }
    }

    /// What the scene shows on this tab. Both plant tabs show a Plant
    /// Scatter; the Plant tab fills it with only its selected plant.
    pub fn mode(self) -> SceneMode {
        match self {
            Self::Trees => SceneMode::Tree,
            Self::TreeScatter => SceneMode::Grove,
            Self::Plant | Self::PlantScatter => SceneMode::Plants,
            Self::Stones => SceneMode::Stone,
            Self::Clumps => SceneMode::Clump,
            Self::Mushrooms => SceneMode::Mushroom,
            Self::Deadwood => SceneMode::Deadwood,
            Self::Ecotope => SceneMode::Ecotope,
            Self::Terrain => SceneMode::Terrain,
        }
    }

    /// The tab at `index` in [`ALL`](Self::ALL), if any.
    pub fn at(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }
}
