//! The Trees tab: preset, seed, and every tree setting.

use std::sync::LazyLock;

use procgena_core::meta::{GroupMeta, SettingPath};
use procgena_core::preset::to_value;
use procgena_tree::procgena_fruit::{FlowerPreset, FruitOptions, FruitPreset};
use procgena_tree::{AttachedFruit, TreeOptions};

use crate::params::Param;
use crate::studio::inspector::{MetaKind, Settings};

pub mod editor;
#[cfg(test)]
mod inspector_tests;

/// The key of the preset shown when the app starts.
pub const START_PRESET: &str = "oak_medium";
/// How far Jiggle varies each shape setting: 0.1 is up to ±10%.
pub const JIGGLE_AMOUNT: f32 = 0.1;

/// The tree settings' metadata, from `tree.meta.yml`.
pub static META: LazyLock<MetaKind> =
    LazyLock::new(|| MetaKind::new(procgena_tree::TREE_META, procgena_tree::meta_choices()));

/// The custom controls of the tree's metadata: the fruit and flower preset
/// pickers (`cluster_picker`). Other names get no controls.
pub fn custom(name: &str, group: &'static GroupMeta) -> Vec<Param<Settings<TreeOptions>>> {
    if name != "cluster_picker" {
        return Vec::new();
    }
    group
        .settings
        .iter()
        .filter_map(|pattern| pattern.path())
        .map(cluster_picker)
        .collect()
}

/// A preset a tree's fruit or flowers can grow: its display name and
/// settings.
type ClusterChoice = (&'static str, &'static FruitOptions);

/// The presets the tree's slot at `path` can grow, fruit presets for
/// `fruit` and flower presets for anything else, in catalogue order.
fn cluster_presets(path: &SettingPath) -> &'static [ClusterChoice] {
    static FRUIT: LazyLock<Vec<ClusterChoice>> = LazyLock::new(|| {
        FruitPreset::all()
            .map(|p| (p.name.as_ref(), &p.fruit))
            .collect()
    });
    static FLOWERS: LazyLock<Vec<ClusterChoice>> = LazyLock::new(|| {
        FlowerPreset::all()
            .map(|p| (p.name.as_ref(), &p.flower))
            .collect()
    });
    if path.to_string() == "fruit" {
        &FRUIT
    } else {
        &FLOWERS
    }
}

/// The tree's fruit or flower slot at `path`.
fn slot<'a>(options: &'a TreeOptions, path: &SettingPath) -> &'a Option<AttachedFruit> {
    if path.to_string() == "fruit" {
        &options.fruit
    } else {
        &options.flowers
    }
}

/// The preset picker for the tree's fruit or flowers at `path`: "None",
/// then each preset. Settings that match no preset, such as ones loaded
/// from a file, show as the first preset. Picking a preset keeps the
/// slot's placement.
fn cluster_picker(path: SettingPath) -> Param<Settings<TreeOptions>> {
    let presets = cluster_presets(&path);
    let names: Vec<&'static str> = std::iter::once("None")
        .chain(presets.iter().map(|(name, _)| *name))
        .collect();
    let meta = &META.meta;
    let entry = meta.setting(&path).map(|(entry, _)| entry);
    let part = meta.part_of(&path);
    let read = path.clone();
    let mut picker = Param::choice_of(
        path.to_string(),
        entry.map_or("Preset", |e| e.label.as_str()),
        std::borrow::Cow::Owned(names),
        move |s: &Settings<TreeOptions>| {
            let Some(attached) = slot(s.options(), &read) else {
                return 0;
            };
            presets
                .iter()
                .position(|(_, fruit)| **fruit == attached.fruit)
                .map_or(1, |i| i + 1)
        },
        move |s: &mut Settings<TreeOptions>, index| {
            let current = slot(s.options(), &path).clone();
            let new = index
                .checked_sub(1)
                .and_then(|i| presets.get(i))
                .map(|(_, preset)| match current {
                    Some(attached) => AttachedFruit {
                        fruit: (*preset).clone(),
                        ..attached
                    },
                    None => AttachedFruit::new((*preset).clone(), Default::default()),
                });
            s.set(&path, to_value(&new));
        },
    );
    picker.description = entry.map(|e| e.description.as_str());
    picker.part = part;
    picker
}
