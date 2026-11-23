//! Controls for choosing a dead wood preset (a log, branch or stump) and
//! customising its shape, ends, decay and colours, and for choosing what
//! the Dead Wood tab shows: that piece, with any cover such as moss grown
//! on it, or a scatter of sticks.

pub mod editor;
pub mod params;

use procgena_clump::ClumpPreset;
use procgena_deadwood::DeadwoodPreset;

use crate::params::{Group, Section, choice};
use crate::scene::{DeadwoodSettings, DeadwoodView};

/// The dead wood preset the editor shows first.
pub const START_PRESET: DeadwoodPreset = DeadwoodPreset::WindthrowLog;

/// What the Dead Wood tab shows, apart from the piece's own settings.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DeadwoodShow {
    /// A single piece, or a scatter of sticks.
    pub view: DeadwoodView,
    /// A clump grown on a single piece, or `None` for bare wood.
    pub cover: Option<ClumpPreset>,
}

impl DeadwoodShow {
    /// The scene settings for showing `piece` this way.
    pub fn settings(self, piece: procgena_deadwood::DeadwoodOptions) -> DeadwoodSettings {
        DeadwoodSettings {
            piece,
            view: self.view,
            cover: self.cover,
        }
    }
}

/// What the scene shows on the Dead Wood tab at first: [`START_PRESET`],
/// bare.
pub fn start_settings() -> DeadwoodSettings {
    DeadwoodShow::default().settings(START_PRESET.options())
}

/// The View and Cover controls.
pub fn show_sections() -> Vec<Section<DeadwoodShow>> {
    vec![Section {
        title: "Show",
        groups: vec![Group::plain(vec![
            choice!(DeadwoodShow, "view", "View", [
                "Single Piece" => DeadwoodView::Piece,
                "Kindling" => DeadwoodView::Kindling,
                "Twig Litter" => DeadwoodView::TwigLitter,
            ], |s| s.view),
            choice!(DeadwoodShow, "cover", "Cover", [
                "None" => None,
                "Moss Patches" => Some(ClumpPreset::MossPatches),
                "Moss Cushions" => Some(ClumpPreset::MossCushions),
                "Snow" => Some(ClumpPreset::SnowCap),
                "Burl" => Some(ClumpPreset::Burl),
            ], |s| s.cover)
            .shown_if(|s| s.view == DeadwoodView::Piece),
        ])],
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::checks;

    #[test]
    fn show_controls_cover_every_choice() {
        let params: Vec<_> = show_sections()
            .into_iter()
            .flat_map(|s| s.groups)
            .flat_map(|g| g.params)
            .collect();
        let cases = vec![("start".to_owned(), DeadwoodShow::default())];
        checks::keys_are_unique(&params);
        checks::cases_fit_the_controls(&params, &cases);
        checks::settings_follow_what_shows_them(&params, &cases);
        checks::no_setting_hides_itself(&params, &cases);
    }
}
