//! What tools that edit tree and tree Scatter presets need to know about
//! each setting.

use procgena_core::meta::{Choice, ChoiceLists};

use crate::{BARK_KINDS, LEAF_KINDS};

/// The text of `tree.meta.yml`: the tree kind's name, parts, and each
/// setting's label, description, range and conditions. Read it with
/// [`KindMeta::parse`](procgena_core::meta::KindMeta::parse), and give
/// [`meta_choices`] to anything that shows its choices.
pub const TREE_META: &str = include_str!("../presets/tree.meta.yml");

/// The text of `tree_scatter.meta.yml`: the tree Scatter kind's name and
/// each setting's label, description and range. Read it with
/// [`KindMeta::parse`](procgena_core::meta::KindMeta::parse).
pub const TREE_SCATTER_META: &str = include_str!("../presets/tree_scatter.meta.yml");

/// The choice lists `tree.meta.yml` names: `bark_kinds` and `leaf_kinds`,
/// the shipped bark and leaf texture kinds.
#[must_use]
pub fn meta_choices() -> ChoiceLists {
    ChoiceLists::new()
        .with("bark_kinds", names(&BARK_KINDS))
        .with("leaf_kinds", names(&LEAF_KINDS))
}

/// One choice per name, labelled with the name itself.
fn names(names: &[&str]) -> Vec<Choice> {
    names
        .iter()
        .map(|name| Choice {
            value: (*name).into(),
            label: (*name).to_owned(),
        })
        .collect()
}
