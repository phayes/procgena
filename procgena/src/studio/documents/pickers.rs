//! Preset pickers inside editors: the presets they offer, built-in and the
//! project's, and remembering which one was picked so a save names it by
//! key.

use bevy::prelude::*;
use procgena_core::meta::SettingPath;
use procgena_core::preset::{Layer, Preset, Reference};

use crate::studio::project::Presets;

/// One preset a picker offers.
#[derive(Debug, Clone)]
pub struct PresetChoice<P: Preset> {
    /// The reference that names it, as a file writes it: the key, or the
    /// key with its namespace when the project and the built-in presets
    /// share it.
    pub reference: String,
    /// Its name as the picker shows it; project presets end in
    /// ` · project`.
    pub name: String,
    /// Its options.
    pub options: P::Options,
}

/// Every preset of type `P` a picker offers: the project's first, then the
/// built-in ones. Project presets that can't be read are left out.
pub fn preset_choices<P: Preset>(presets: &Presets) -> Vec<PresetChoice<P>>
where
    P::Options: Clone,
{
    let registry = presets.registry();
    let entries = registry.entries(P::KIND);
    entries
        .iter()
        .filter_map(|entry| {
            let shared = entries
                .iter()
                .filter(|other| other.key == entry.key)
                .count()
                > 1;
            let reference = match registry.namespace(entry.layer) {
                Some(namespace) if shared => Reference::qualified(namespace, &entry.key),
                _ => entry.key.clone(),
            };
            let options = registry.resolve::<P>(&reference).ok()?.options().clone();
            let name = match entry.layer {
                Layer::Project => format!("{} · project", entry.name),
                Layer::BuiltIn => entry.name.clone(),
            };
            Some(PresetChoice {
                reference,
                name,
                options,
            })
        })
        .collect()
}

/// The index in `choices` of the preset whose options equal `options`, or
/// `choices.len()` (the picker's Custom entry after them) when none does.
pub fn choice_of<P: Preset>(choices: &[PresetChoice<P>], options: &P::Options) -> usize
where
    P::Options: PartialEq,
{
    choices
        .iter()
        .position(|choice| &choice.options == options)
        .unwrap_or(choices.len())
}

/// Triggered by an editor when a picker in it chose the preset `reference`
/// for the setting at `path` of the editor's settings, such as
/// `scatters.boulders.classes.0.stone`. While the setting keeps that
/// preset's settings, saving the document writes `reference` there.
#[derive(Event, Debug, Clone)]
pub struct PresetPicked {
    /// The editor the picker is in.
    pub editor: Entity,
    /// Where the picked preset's settings go, in the editor's settings.
    pub path: SettingPath,
    /// The reference that names the picked preset.
    pub reference: String,
}
