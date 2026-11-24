//! What tools that edit stone presets need to know about each setting.

use procgena_core::meta::{Choice, ChoiceLists};
use procgena_core::preset::to_value;

use crate::{StoneDetail, TEXTURE_SETS};

/// The text of `stone.meta.yml`: the stone kind's name, parts, and each
/// setting's label, description, range and conditions. Read it with
/// [`KindMeta::parse`](procgena_core::meta::KindMeta::parse), and give
/// [`meta_choices`] to anything that shows its choices.
pub const STONE_META: &str = include_str!("../presets/stone.meta.yml");

/// The choice lists `stone.meta.yml` names: `texture_sets` (the shipped
/// texture sets) and `details` (the [`StoneDetail`] presets).
#[must_use]
pub fn meta_choices() -> ChoiceLists {
    let textures = TEXTURE_SETS
        .iter()
        .map(|set| Choice {
            value: (*set).into(),
            label: (*set).to_owned(),
        })
        .collect();
    let details = StoneDetail::PRESETS
        .iter()
        .map(|(name, detail)| Choice {
            value: to_value(detail),
            label: (*name).to_owned(),
        })
        .collect();
    ChoiceLists::new()
        .with("texture_sets", textures)
        .with("details", details)
}
