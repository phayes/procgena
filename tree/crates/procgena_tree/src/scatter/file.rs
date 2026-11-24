//! How a [`ScatterTree`] is written in a `.tree_scatter.yml` file.

use procgena_core::preset::{from_value, merge, to_value};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use super::{DEFAULT_VARIANTS, ScatterTree};
use crate::options::TreeOptions;
use crate::presets::TreePreset;

/// One species as a file writes it. Settings left out come from the tree's
/// preset, or from [`ScatterTree::custom`]'s defaults.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScatterTreeFile {
    tree: TreeRef,
    #[serde(default)]
    density: Option<f32>,
    #[serde(default)]
    spacing: Option<f32>,
    #[serde(default)]
    density_scale: Option<f32>,
    #[serde(default)]
    clustering: Option<f32>,
    #[serde(default)]
    variants: Option<u32>,
    /// Missing: the tree preset's; `null`: the Scatter's; a number: its own.
    #[serde(default, deserialize_with = "present")]
    max_slope: Option<Option<f32>>,
}

/// A tree in a file: a [`TreePreset`] key, or a map of tree settings with
/// an optional `base` preset key they are laid over.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum TreeRef {
    Key(String),
    Settings(Map<String, Value>),
}

/// Reads a setting that may be missing (`None`), `null` (`Some(None)`) or
/// given (`Some(Some(value))`).
fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Option<f32>>, D::Error> {
    Option::<f32>::deserialize(deserializer).map(Some)
}

/// The built-in tree preset with `key`, or an error listing the valid keys.
fn tree_preset(key: &str) -> Result<&'static TreePreset, String> {
    TreePreset::named(key).ok_or_else(|| {
        let keys: Vec<_> = TreePreset::all()
            .map(|preset| preset.key.as_ref())
            .collect();
        format!(
            "unknown tree preset {key:?}; valid keys are {}",
            keys.join(", ")
        )
    })
}

impl TryFrom<ScatterTreeFile> for ScatterTree {
    type Error = String;

    fn try_from(file: ScatterTreeFile) -> Result<Self, String> {
        // Find the tree's preset, if any, and its settings.
        let (preset, tree) = match file.tree {
            TreeRef::Key(key) => {
                let preset = tree_preset(&key)?;
                (Some(preset), preset.tree.clone())
            }
            TreeRef::Settings(mut settings) => {
                let preset = match settings.remove("base") {
                    Some(Value::String(key)) => Some(tree_preset(&key)?),
                    Some(_) => return Err("tree.base must be a tree preset key".to_owned()),
                    None => None,
                };
                let mut value = match preset {
                    Some(preset) => to_value(&preset.tree),
                    None => to_value(&TreeOptions::default()),
                };
                merge(&mut value, Value::Object(settings));
                let tree: TreeOptions = from_value(value, "tree").map_err(|e| e.to_string())?;
                (preset, tree)
            }
        };

        // Density, spacing and slope come from the preset's stand unless the
        // entry sets them.
        let stand = preset.and_then(|preset| preset.stand);
        let missing = |field: &str| {
            format!("the tree names no preset with a stand, so the entry must set {field}")
        };
        let density = file
            .density
            .or(stand.map(|stand| stand.density))
            .ok_or_else(|| missing("density"))?;
        let spacing = file
            .spacing
            .or(stand.map(|stand| stand.spacing))
            .ok_or_else(|| missing("spacing"))?;
        Ok(Self {
            tree,
            density,
            spacing,
            density_scale: file.density_scale.unwrap_or(1.0),
            clustering: file.clustering.unwrap_or(0.0),
            variants: file.variants.unwrap_or(DEFAULT_VARIANTS),
            max_slope: file
                .max_slope
                .unwrap_or_else(|| stand.and_then(|stand| stand.max_slope)),
        })
    }
}

impl From<ScatterTree> for ScatterTreeFile {
    /// Writes every setting, and a tree whose settings equal a
    /// [`TreePreset`]'s as that preset's key.
    fn from(tree: ScatterTree) -> Self {
        let preset = TreePreset::all().find(|preset| preset.tree == tree.tree);
        Self {
            tree: match preset {
                Some(preset) => TreeRef::Key(preset.key.to_string()),
                None => match to_value(&tree.tree) {
                    Value::Object(settings) => TreeRef::Settings(settings),
                    _ => unreachable!("tree options always write as a map"),
                },
            },
            density: Some(tree.density),
            spacing: Some(tree.spacing),
            density_scale: Some(tree.density_scale),
            clustering: Some(tree.clustering),
            variants: Some(tree.variants),
            max_slope: Some(tree.max_slope),
        }
    }
}
