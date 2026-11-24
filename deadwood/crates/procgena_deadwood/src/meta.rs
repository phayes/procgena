//! What tools that edit dead wood presets need to know about each setting.

use procgena_core::meta::{Choice, ChoiceLists};
use procgena_core::preset::to_value;
use serde_json::Value;

use crate::{BARK_TEXTURE_SETS, END_TEXTURE_SETS, EndKind, LEAF_TEXTURE_SETS};

/// The text of `deadwood.meta.yml`: the dead wood kind's name, parts, and
/// each setting's label, description, range and conditions. Read it with
/// [`KindMeta::parse`](procgena_core::meta::KindMeta::parse), and give
/// [`meta_choices`] to anything that shows its choices.
pub const DEADWOOD_META: &str = include_str!("../presets/deadwood.meta.yml");

/// The end shapes offered for each end, in menu order, each with the
/// settings it starts from when picked.
const END_SHAPES: [(&str, EndKind); 5] = [
    ("Saw Cut", EndKind::Saw { tilt: 0.0 }),
    (
        "Chainsaw Cut",
        EndKind::Chainsaw {
            tilt: 0.0,
            step: 0.1,
        },
    ),
    ("Axe Cut", EndKind::Axe { angle: 60.0 }),
    (
        "Snapped",
        EndKind::Snapped {
            jaggedness: 0.35,
            splinters: 6,
            one_sided: 0.4,
        },
    ),
    ("Natural Tip", EndKind::Taper {}),
];

/// The choice lists `deadwood.meta.yml` names:
///
/// - `end_shapes`: each [`EndKind`] with the settings it starts from;
/// - `leaf_textures`: the shipped leaf card texture sets;
/// - `bark_textures` and `end_textures`: "None", then the shipped bark or
///   end-grain texture sets.
#[must_use]
pub fn meta_choices() -> ChoiceLists {
    let ends = END_SHAPES
        .iter()
        .map(|(label, end)| Choice {
            value: to_value(end),
            label: (*label).to_owned(),
        })
        .collect();
    ChoiceLists::new()
        .with("end_shapes", ends)
        .with("leaf_textures", sets(&LEAF_TEXTURE_SETS))
        .with("bark_textures", optional_sets(&BARK_TEXTURE_SETS))
        .with("end_textures", optional_sets(&END_TEXTURE_SETS))
}

/// Each set in `sets`.
fn sets(sets: &[&str]) -> Vec<Choice> {
    sets.iter()
        .map(|set| Choice {
            value: (*set).into(),
            label: (*set).to_owned(),
        })
        .collect()
}

/// "None" (no texture), then each set in `sets`.
fn optional_sets(names: &[&str]) -> Vec<Choice> {
    let none = Choice {
        value: Value::Null,
        label: "None".to_owned(),
    };
    std::iter::once(none).chain(sets(names)).collect()
}
