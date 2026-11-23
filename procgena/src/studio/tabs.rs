//! The studio's tabs and what each one shows.

use crate::scene::SceneMode;

/// The tabs along the top of the settings panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StudioTab {
    /// A single tree.
    #[default]
    Trees,
    /// A grove mixing several kinds of tree.
    TreeGrove,
    /// One plant, previewed on its own.
    Plant,
    /// A field mixing several plants.
    PlantField,
    /// A single rock.
    Stones,
    /// A tree or rock with a clump, such as snow or moss, grown on it.
    Clumps,
    /// A single mushroom.
    Mushrooms,
    /// A single fallen log, branch or stump.
    Deadwood,
}

impl StudioTab {
    /// Every tab, left to right.
    pub const ALL: [StudioTab; 8] = [
        StudioTab::Trees,
        StudioTab::TreeGrove,
        StudioTab::Plant,
        StudioTab::PlantField,
        StudioTab::Stones,
        StudioTab::Clumps,
        StudioTab::Mushrooms,
        StudioTab::Deadwood,
    ];

    /// The tab's label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Trees => "Trees",
            Self::TreeGrove => "Tree Grove",
            Self::Plant => "Plant",
            Self::PlantField => "Plant Field",
            Self::Stones => "Stones",
            Self::Clumps => "Clumps",
            Self::Mushrooms => "Mushrooms",
            Self::Deadwood => "Dead Wood",
        }
    }

    /// What the scene shows on this tab. Both plant tabs show a plant field;
    /// the Plant tab fills it with only its selected plant.
    pub fn mode(self) -> SceneMode {
        match self {
            Self::Trees => SceneMode::Tree,
            Self::TreeGrove => SceneMode::Grove,
            Self::Plant | Self::PlantField => SceneMode::Plants,
            Self::Stones => SceneMode::Stone,
            Self::Clumps => SceneMode::Clump,
            Self::Mushrooms => SceneMode::Mushroom,
            Self::Deadwood => SceneMode::Deadwood,
        }
    }

    /// The tab at `index` in [`ALL`](Self::ALL), if any.
    pub fn at(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }
}
