//! What tools that edit mushroom presets need to know about each setting.

use procgena_core::meta::{Choice, ChoiceLists};
use serde_json::Value;

use crate::{CAP_TEXTURE_SETS, STEM_TEXTURE_SETS};

/// The text of `mushroom.meta.yml`: the mushroom kind's name, parts, and
/// each setting's label, description, range and conditions. Read it with
/// [`KindMeta::parse`](procgena_core::meta::KindMeta::parse), and give
/// [`meta_choices`] to anything that shows its choices.
pub const MUSHROOM_META: &str = include_str!("../presets/mushroom.meta.yml");

/// The choice lists `mushroom.meta.yml` names: `cap_textures` and
/// `stem_textures`, each "None" followed by the shipped texture sets.
#[must_use]
pub fn meta_choices() -> ChoiceLists {
    ChoiceLists::new()
        .with("cap_textures", optional_sets(&CAP_TEXTURE_SETS))
        .with("stem_textures", optional_sets(&STEM_TEXTURE_SETS))
}

/// "None" (no texture), then each set in `sets`.
fn optional_sets(sets: &[&str]) -> Vec<Choice> {
    let none = Choice {
        value: Value::Null,
        label: "None".to_owned(),
    };
    std::iter::once(none)
        .chain(sets.iter().map(|set| Choice {
            value: (*set).into(),
            label: (*set).to_owned(),
        }))
        .collect()
}
