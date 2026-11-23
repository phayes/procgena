//! Reading and writing groves as `.grove.json` files.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{GrovePreset, GroveTree, TreeGrove};
use crate::options::{TreeOptions, merge_json};
use crate::presets::TreePreset;

/// Why a `.grove.json` file could not be read.
#[derive(Debug)]
#[non_exhaustive]
pub enum GroveFileError {
    /// The text is not valid JSON, has a setting of the wrong type, or has a
    /// setting the format does not know.
    Json(serde_json::Error),
    /// `base` names no [`GrovePreset`].
    UnknownGrove {
        /// The key the file gave.
        key: String,
    },
    /// A tree, or a tree's `base`, names no [`TreePreset`].
    UnknownTree {
        /// The key the file gave.
        key: String,
    },
    /// A tree entry's `tree` is neither a preset key nor an object of tree
    /// settings.
    BadTree {
        /// The entry's position in the file's `trees` list.
        index: usize,
    },
    /// A tree written out without a `base` must give a setting that would
    /// otherwise come from its preset.
    MissingSetting {
        /// The entry's position in the file's `trees` list.
        index: usize,
        /// The missing setting: `"density"` or `"spacing"`.
        field: &'static str,
    },
}

impl fmt::Display for GroveFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(f, "{error}"),
            Self::UnknownGrove { key } => {
                let keys: Vec<_> = GrovePreset::ALL.iter().map(|p| p.key()).collect();
                write!(
                    f,
                    "unknown grove preset {key:?}; valid keys are {}",
                    keys.join(", ")
                )
            }
            Self::UnknownTree { key } => {
                let keys: Vec<_> = TreePreset::ALL.iter().map(|p| p.key()).collect();
                write!(
                    f,
                    "unknown tree preset {key:?}; valid keys are {}",
                    keys.join(", ")
                )
            }
            Self::BadTree { index } => write!(
                f,
                "trees[{index}].tree must be a tree preset key or an object of tree settings"
            ),
            Self::MissingSetting { index, field } => write!(
                f,
                "trees[{index}] has no base preset, so it must set {field:?}"
            ),
        }
    }
}

impl std::error::Error for GroveFileError {}

impl From<serde_json::Error> for GroveFileError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// A grove file as written: every setting optional.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GroveFile {
    base: Option<String>,
    seed: Option<u32>,
    density_scale: Option<f32>,
    patchiness: Option<f32>,
    variation: Option<VariationFile>,
    trees: Option<Vec<TreeFile>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VariationFile {
    scale_jitter: Option<f32>,
    lean: Option<f32>,
    max_slope: Option<f32>,
    sink: Option<f32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TreeFile {
    tree: Value,
    density: Option<f32>,
    spacing: Option<f32>,
    density_scale: Option<f32>,
    clustering: Option<f32>,
    variants: Option<u32>,
}

impl TreeGrove {
    /// Reads a grove from a `.grove.json` file's text.
    ///
    /// Every setting is optional. `base` names a [`GrovePreset`] key, such as
    /// `"oak_woodland"`, that missing settings come from; without one they
    /// come from [`TreeGrove::new`] with no trees. A `trees` list, when
    /// given, replaces the base's trees. Each entry's `tree` is either a
    /// [`TreePreset`] key, such as `"oak_medium"`, or an object of tree
    /// settings (ez-tree field names) with an optional `base` tree preset key
    /// they are laid over. An entry's `density` and `spacing` default to its
    /// tree preset's [`grove_density`](TreePreset::grove_density) and
    /// [`grove_spacing`](TreePreset::grove_spacing).
    ///
    /// The result is not validated; [`GrovePlacement::new`](super::GrovePlacement::new)
    /// and [`validate`](Self::validate) check it.
    ///
    /// # Errors
    ///
    /// Returns a [`GroveFileError`] for malformed JSON, settings the format
    /// does not know, unknown preset keys, and a tree without a base preset
    /// that leaves out its density or spacing.
    pub fn from_json(json: &str) -> Result<Self, GroveFileError> {
        let file: GroveFile = serde_json::from_str(json)?;

        // Start from the base preset, then lay the file's settings over it.
        let mut grove = match &file.base {
            Some(key) => GrovePreset::from_key(key)
                .ok_or_else(|| GroveFileError::UnknownGrove { key: key.clone() })?
                .grove(),
            None => TreeGrove::new(Vec::new()),
        };
        if let Some(seed) = file.seed {
            grove.seed = seed;
        }
        if let Some(scale) = file.density_scale {
            grove.density_scale = scale;
        }
        if let Some(patchiness) = file.patchiness {
            grove.patchiness = patchiness;
        }
        if let Some(v) = file.variation {
            let variation = &mut grove.variation;
            variation.scale_jitter = v.scale_jitter.unwrap_or(variation.scale_jitter);
            variation.lean = v.lean.unwrap_or(variation.lean);
            variation.max_slope = v.max_slope.unwrap_or(variation.max_slope);
            variation.sink = v.sink.unwrap_or(variation.sink);
        }
        if let Some(trees) = file.trees {
            grove.trees = trees
                .into_iter()
                .enumerate()
                .map(|(index, entry)| read_tree(index, entry))
                .collect::<Result<_, _>>()?;
        }
        Ok(grove)
    }

    /// Writes the grove as a `.grove.json` file that
    /// [`from_json`](Self::from_json) reads back to an equal grove.
    ///
    /// Every setting is written out, except that a tree whose settings equal
    /// a [`TreePreset`]'s is written as that preset's key. Non-finite numbers
    /// become `null`, which cannot be read back.
    pub fn to_json(&self) -> String {
        let variation = self.variation;
        let file = GroveOut {
            seed: self.seed,
            density_scale: self.density_scale,
            patchiness: self.patchiness,
            variation: VariationOut {
                scale_jitter: variation.scale_jitter,
                lean: variation.lean,
                max_slope: variation.max_slope,
                sink: variation.sink,
            },
            trees: self.trees.iter().map(write_tree).collect(),
        };
        serde_json::to_string_pretty(&file).expect("groves always serialize to JSON")
    }
}

/// Builds entry `index` of a file's `trees` list.
fn read_tree(index: usize, entry: TreeFile) -> Result<GroveTree, GroveFileError> {
    // Find the tree's preset, if any, and its settings.
    let (preset, tree) = match entry.tree {
        Value::String(key) => {
            let preset = tree_preset(&key)?;
            (Some(preset), preset.options())
        }
        Value::Object(mut settings) => {
            let preset = match settings.remove("base") {
                Some(Value::String(key)) => Some(tree_preset(&key)?),
                Some(_) => return Err(GroveFileError::BadTree { index }),
                None => None,
            };
            let base = preset.map_or_else(TreeOptions::default, TreePreset::options);
            let mut merged = serde_json::to_value(base)?;
            merge_json(&mut merged, Value::Object(settings));
            (preset, serde_json::from_value(merged)?)
        }
        _ => return Err(GroveFileError::BadTree { index }),
    };

    // Density and spacing come from the preset unless the entry sets them.
    let density = entry
        .density
        .or(preset.map(TreePreset::grove_density))
        .ok_or(GroveFileError::MissingSetting {
            index,
            field: "density",
        })?;
    let spacing = entry
        .spacing
        .or(preset.map(TreePreset::grove_spacing))
        .ok_or(GroveFileError::MissingSetting {
            index,
            field: "spacing",
        })?;
    let mut tree = GroveTree::custom(tree, density, spacing);
    tree.density_scale = entry.density_scale.unwrap_or(tree.density_scale);
    tree.clustering = entry.clustering.unwrap_or(tree.clustering);
    tree.variants = entry.variants.unwrap_or(tree.variants);
    Ok(tree)
}

fn tree_preset(key: &str) -> Result<TreePreset, GroveFileError> {
    TreePreset::from_key(key).ok_or_else(|| GroveFileError::UnknownTree {
        key: key.to_string(),
    })
}

/// Writes one species as an entry of a file's `trees` list.
fn write_tree(tree: &GroveTree) -> TreeOut<'_> {
    let preset = TreePreset::ALL
        .into_iter()
        .find(|preset| preset.options() == tree.tree);
    TreeOut {
        tree: match preset {
            Some(preset) => TreeRefOut::Key(preset.key()),
            None => TreeRefOut::Settings(&tree.tree),
        },
        density: tree.density,
        spacing: tree.spacing,
        density_scale: tree.density_scale,
        clustering: tree.clustering,
        variants: tree.variants,
    }
}

// The file as written. Numbers stay `f32`, so they are written in their
// shortest form, such as `0.6` rather than `0.6000000238418579`.

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GroveOut<'a> {
    seed: u32,
    density_scale: f32,
    patchiness: f32,
    variation: VariationOut,
    trees: Vec<TreeOut<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VariationOut {
    scale_jitter: f32,
    lean: f32,
    max_slope: f32,
    sink: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TreeOut<'a> {
    tree: TreeRefOut<'a>,
    density: f32,
    spacing: f32,
    density_scale: f32,
    clustering: f32,
    variants: u32,
}

#[derive(Serialize)]
#[serde(untagged)]
enum TreeRefOut<'a> {
    Key(&'static str),
    Settings(&'a TreeOptions),
}
