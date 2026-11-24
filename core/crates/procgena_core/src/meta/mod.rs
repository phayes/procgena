//! Metadata about each kind of preset: names, descriptions, parts, and how
//! each setting is labelled, ranged and shown.
//!
//! Each generator crate ships one `<kind>.meta.yml` beside its presets.
//! Tools such as the studio build their settings panels, tooltips and part
//! filters from it, so a new setting needs no hand-written UI. Games can
//! use it for their own tools.
//!
//! # The file
//!
//! ```yaml
//! kind: stone
//! name: Stone
//! description: "One rock, carved from a ball."
//! parts:
//!   rock: { name: Rock, description: "The whole rock.", tags: [rock] }
//! conditions:                  # named conditions, used as @name
//!   carved: "carve.steps > 0"
//! sections:                    # the Inspector's layout, in order
//!   - title: Carving
//!     part: rock
//!     settings: [carve.steps, carve.depth]
//!     groups:                  # optional titled groups after the settings
//!       - { title: Edges, shown_if: "@carved", settings: [carve.smoothness] }
//! settings:                    # one entry per setting, by path or pattern
//!   carve.steps:
//!     label: Cuts
//!     description: How many flat cuts shape the rock.
//!     range: [0, 64]
//!   carve.depth:
//!     label: Cut Depth
//!     range: [0.1, 1.2]
//!     step: 0.01
//!     shown_if: "@carved"
//!     revive: 0.5
//!   carve.smoothness:
//!     label: Edge Softness
//!     range: [0.005, 0.5]
//!     scale: exponential
//! ```
//!
//! An entry may also give `unit`, `part`, `advanced`, `hidden`, and one
//! control override: `color: true`, `choices` (a list of `{value, label}`,
//! or the name of a list the crate supplies through [`ChoiceLists`]), or
//! `ref: <kind>` for a preset key of another kind. `revive` is a number or
//! `{from: <path>, times, min, if_zero}`. A `*` in an entry's path matches
//! any one segment, such as `branch.length.*` for every branch level, and
//! fills `{index}` in its conditions. An entry also covers every setting
//! below its path, for values edited whole such as a colour's three
//! numbers. Conditions use the language of [`Condition`].
//!
//! # Keeping it in step
//!
//! Each crate calls [`assert_in_step`] from one test, with its defaults and
//! built-in presets as samples, so a renamed or added setting fails in the
//! crate that changed it.

mod check;
mod condition;
mod file;
mod path;
#[cfg(test)]
mod tests;

use serde_json::Value;

pub use check::{assert_in_step, problems, variants};
pub use condition::{Condition, NamedConditions};
pub use file::{
    Choice, ChoiceLists, Choices, ControlKind, GroupLayout, GroupMeta, KindMeta, MetaError,
    PartMeta, Revive, Scale, SectionLayout, SectionMeta, SettingLayout, SettingMeta, choice_index,
    pick_choice, write_number,
};
pub use path::{Match, Pattern, SettingPath, all_paths, leaves};

/// One change to a settings tree: the setting at `path` went from `old` to
/// `new`. Undo puts `old` back.
#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    /// Where the setting is.
    pub path: SettingPath,
    /// Its value before the change.
    pub old: Value,
    /// Its value after the change.
    pub new: Value,
}

impl Edit {
    /// Puts `new` at `path` in `value` and returns the edit, or `None`,
    /// changing nothing, when `path` can't be set in `value` (see
    /// [`SettingPath::set`]) or already holds `new`.
    pub fn apply(value: &mut Value, path: SettingPath, new: Value) -> Option<Self> {
        if path.get(value) == Some(&new) {
            return None;
        }
        let old = path.set(value, new.clone())?;
        Some(Self { path, old, new })
    }
}
