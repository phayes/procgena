//! The dead wood editor: controls for customising a dead wood preset (a
//! log, branch or stump), its shape, ends, decay and colours, and for
//! choosing what its document shows: that piece, with any cover such as
//! moss grown on it, or a scatter of sticks.

pub mod editor;
/// Hand-written dead wood settings, still used by the Ecotope's dead wood
/// Scatter panels until the Ecotope's Inspector is built from metadata.
pub mod params;

use std::sync::LazyLock;

use procgena_clump::ClumpPreset;

use crate::params::{Group, Section, choice};
use crate::scene::{DeadwoodSettings, DeadwoodView};
use crate::studio::inspector::MetaKind;

/// The dead wood settings' metadata, from `deadwood.meta.yml`.
pub static META: LazyLock<MetaKind> = LazyLock::new(|| {
    MetaKind::new(
        procgena_deadwood::DEADWOOD_META,
        procgena_deadwood::meta_choices(),
    )
});

/// The key of the dead wood preset opened when the studio starts on dead
/// wood.
pub const START_PRESET: &str = "windthrow_log";

/// What a dead wood document shows, apart from the piece's own settings.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DeadwoodShow {
    /// A single piece, or a scatter of sticks.
    pub view: DeadwoodView,
    /// A clump grown on a single piece, or `None` for bare wood.
    pub cover: Option<&'static ClumpPreset>,
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
                "Moss Patches" => Some(ClumpPreset::moss_patches()),
                "Moss Cushions" => Some(ClumpPreset::moss_cushions()),
                "Fresh Snow" => Some(ClumpPreset::fresh_snow_cap()),
                "Dirty Snow" => Some(ClumpPreset::dirty_snow_cap()),
                "Burl" => Some(ClumpPreset::burl()),
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
