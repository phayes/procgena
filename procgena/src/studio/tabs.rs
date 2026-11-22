//! The studio's tabs and what each one shows.

use crate::scene::SceneMode;

/// The tabs along the top of the settings panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StudioTab {
    /// A single tree.
    #[default]
    Trees,
    /// One plant, previewed on its own.
    Plant,
    /// A field mixing several plants.
    PlantField,
    /// A single rock.
    Stones,
}

impl StudioTab {
    /// Every tab, left to right.
    pub const ALL: [StudioTab; 4] = [
        StudioTab::Trees,
        StudioTab::Plant,
        StudioTab::PlantField,
        StudioTab::Stones,
    ];

    /// The tab's label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Trees => "Trees",
            Self::Plant => "Plant",
            Self::PlantField => "Plant Field",
            Self::Stones => "Stones",
        }
    }

    /// What the scene shows on this tab. Both plant tabs show a plant field;
    /// the Plant tab fills it with only its selected plant.
    pub fn mode(self) -> SceneMode {
        match self {
            Self::Trees => SceneMode::Tree,
            Self::Plant | Self::PlantField => SceneMode::Plants,
            Self::Stones => SceneMode::Stone,
        }
    }

    /// The tab at `index` in [`ALL`](Self::ALL), if any.
    pub fn at(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }
}
