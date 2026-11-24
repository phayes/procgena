//! The kinds of preset the studio can open, such as trees and Ecotopes.
//!
//! Each kind is described once by a [`Kind`] in a module of its own and
//! registered with one line in [`register_all`]. The Library, the
//! documents, the view frame and the status overlay all work from these
//! descriptions, so adding a kind touches nothing else in the studio.

mod clump;
mod deadwood;
mod ecotope;
mod fruit;
mod mushroom;
mod plant;
mod plant_scatter;
mod stone;
mod terrain;
mod tree;
mod tree_scatter;

use bevy::prelude::*;

use super::status::BuildState;
use crate::scene::Show;

/// One preset of a kind, as the Library lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetEntry {
    /// The preset's key, such as `oak_medium`.
    pub key: String,
    /// Its display name, such as `Oak Medium`.
    pub name: String,
    /// The group it is listed under within its kind.
    pub group: String,
}

/// How the studio opens, shows and describes presets of one kind.
#[derive(Clone)]
pub struct Kind {
    /// The kind's id: its preset files' kind, such as `"tree"` for
    /// `.tree.yml` files.
    pub id: &'static str,
    /// The kind's name in the Library's filters, such as `"Trees"`.
    pub label: &'static str,
    /// The built-in presets, in Library order.
    pub presets: fn() -> Vec<PresetEntry>,
    /// The key of the preset opened when the studio starts, if this kind
    /// is the one opened.
    pub start: &'static str,
    /// Spawns the Inspector's editor for the preset `key` as the last child
    /// of `page`, and returns the editor's entity: the one whose component
    /// the kind's [`show`](Self::show) reads.
    pub spawn_editor: fn(&mut Commands, Entity, &str) -> Entity,
    /// What the scene shows for the editor `editor`, for the document whose
    /// id is given, or `None` before the editor is ready.
    pub show: fn(&mut World, Entity, u64) -> Option<Show>,
    /// The names of the detail levels the Detail control can force, most
    /// detailed first, or none for kinds without levels.
    pub detail_levels: &'static [&'static str],
    /// The Environment settings that don't apply to this kind, by key.
    pub hidden_environment: &'static [&'static str],
    /// The status overlay's text, and whether it reports a failure.
    pub status_line: fn(&BuildState) -> (String, bool),
}

/// Every registered kind, in Library order.
#[derive(Resource, Default)]
pub struct Kinds(Vec<Kind>);

impl Kinds {
    /// The kind with id `id`.
    pub fn get(&self, id: &str) -> Option<&Kind> {
        self.0.iter().find(|kind| kind.id == id)
    }

    /// Every kind, in Library order.
    pub fn iter(&self) -> impl Iterator<Item = &Kind> {
        self.0.iter()
    }
}

/// Registers kinds of preset with the studio.
pub trait RegisterKind {
    /// Adds `kind` after the kinds already registered.
    ///
    /// # Panics
    ///
    /// Panics if a kind with the same id is already registered.
    fn register_kind(&mut self, kind: Kind) -> &mut Self;
}

impl RegisterKind for App {
    fn register_kind(&mut self, kind: Kind) -> &mut Self {
        let mut kinds = self.world_mut().get_resource_or_init::<Kinds>();
        assert!(
            kinds.get(kind.id).is_none(),
            "kind {} is registered twice",
            kind.id
        );
        kinds.0.push(kind);
        self
    }
}

/// Registers every kind the studio knows, with its editor's plugin, in
/// Library order.
pub fn register_all(app: &mut App) {
    tree::register(app);
    tree_scatter::register(app);
    plant::register(app);
    plant_scatter::register(app);
    stone::register(app);
    clump::register(app);
    mushroom::register(app);
    fruit::register_fruit(app);
    fruit::register_flower(app);
    deadwood::register(app);
    ecotope::register(app);
    terrain::register(app);
}

/// The Library entries of the presets `presets` lists, with each one's key,
/// name and group read by the functions given.
pub fn entries<P: 'static>(
    presets: impl Iterator<Item = &'static P>,
    key: impl Fn(&P) -> &str,
    name: impl Fn(&P) -> &str,
    group: impl Fn(&P) -> &str,
) -> Vec<PresetEntry> {
    presets
        .map(|preset| PresetEntry {
            key: key(preset).to_owned(),
            name: name(preset).to_owned(),
            group: group(preset).to_owned(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds() -> Kinds {
        let mut app = App::new();
        register_all(&mut app);
        app.world_mut().remove_resource::<Kinds>().unwrap()
    }

    #[test]
    fn every_kind_lists_its_start_preset_and_has_a_unique_id() {
        let kinds = kinds();
        let mut ids: Vec<&str> = kinds.iter().map(|kind| kind.id).collect();
        assert_eq!(ids.len(), 12);
        for kind in kinds.iter() {
            let presets = (kind.presets)();
            assert!(
                presets.iter().any(|preset| preset.key == kind.start),
                "{} has no preset {}",
                kind.id,
                kind.start
            );
        }
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 12);
    }

    #[test]
    fn every_detail_level_can_be_chosen() {
        let kinds = kinds();
        let levels = |id| kinds.get(id).unwrap().detail_levels.len();
        assert_eq!(levels("tree"), crate::scene::lod_levels().len());
        // The scene uses the mushroom and dead wood plugins' default levels.
        assert_eq!(
            levels("mushroom"),
            procgena_mushroom::default_lod().levels.len()
        );
        assert_eq!(
            levels("deadwood"),
            procgena_deadwood::default_lod().levels.len()
        );
        // Grass and plants have three levels.
        assert_eq!(levels("plant"), 3);
        assert_eq!(levels("plant_scatter"), 3);
    }

    #[test]
    fn each_kind_hides_the_environment_settings_it_does_not_use() {
        use crate::environment::{FIELD_SIZE, GROVE_SIZE, WIND};
        for kind in kinds().iter() {
            let hidden = |key| kind.hidden_environment.contains(&key);
            let plants = matches!(kind.id, "plant" | "plant_scatter");
            assert_eq!(hidden(FIELD_SIZE), !plants, "{}", kind.id);
            assert_eq!(hidden(GROVE_SIZE), kind.id != "tree_scatter", "{}", kind.id);
            let still = matches!(kind.id, "clump" | "fruit" | "flower");
            assert_eq!(hidden(WIND), still, "{}", kind.id);
        }
    }

    #[test]
    fn preset_names_are_unique_within_each_kind() {
        for kind in kinds().iter() {
            let mut names: Vec<String> = (kind.presets)().into_iter().map(|p| p.name).collect();
            let count = names.len();
            names.sort_unstable();
            names.dedup();
            assert_eq!(names.len(), count, "{} repeats a name", kind.id);
        }
    }
}
