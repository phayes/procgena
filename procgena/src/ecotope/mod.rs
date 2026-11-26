//! Compose an Ecotope from Scatters of trees, stones, dead wood and
//! mushrooms, each under a key with a priority, and see it on a patch of
//! ground.
//!
//! An Ecotope document starts from an Ecotope preset. Its list shows one row
//! per Scatter, highest priority first; the selected Scatter's settings show
//! below the list. Loading a `.ecotope.yml` file shows its Ecotope in the
//! selected document.

pub mod editor;
mod ground_layers;
mod ground_params;
pub mod params;

use procgena_biome::{Ecotope, EcotopeError, EcotopePreset, EcotopeScatter, ScatterKind};
use procgena_biome_bevy::EcotopeSummary;
use procgena_core::ChildKey;
use procgena_core::preset::{FILE_GROUP, Preset, PresetMeta, read as read_preset_file};
use procgena_deadwood::ScatterOptions as DeadwoodScatterOptions;
use procgena_mushroom::{MushroomPreset, ScatterOptions as MushroomScatterOptions};
use procgena_plant_bevy::PlantScatterPreset;
use procgena_stone::{StoneClass, StonePreset, StoneScatterOptions};
use procgena_tree::TreeScatterPreset;

use crate::scene::{EcotopeGround, EcotopeSettings};

/// The key of the Ecotope preset opened first.
pub const START_PRESET: &str = "pine_forest_floor";

/// The kinds of Scatter, as the Kind picker lists them.
pub const KINDS: [&str; 5] = ["Tree", "Stone", "Dead Wood", "Mushroom", "Plant"];

/// How the Ecotope is shown: the ground under it and the patch it covers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EcotopeView {
    /// The ground the Ecotope grows on.
    pub ground: EcotopeGround,
    /// Side of the square patch, in metres.
    pub size: f32,
}

impl Default for EcotopeView {
    /// 40 metres of rolling hills.
    fn default() -> Self {
        Self {
            ground: EcotopeGround::RollingHills,
            size: 40.0,
        }
    }
}

/// Where placing the Ecotope shown stands, for the overlay over the 3D
/// view.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum EcotopeStatus {
    /// Being placed.
    #[default]
    Placing,
    /// Placed, with what each Scatter gave.
    Placed(EcotopeSummary),
    /// Could not be placed, for this reason. The last drawing stays.
    Failed(String),
}

impl EcotopeStatus {
    /// The status for a placement the scene reported.
    pub fn from_result(result: &Result<EcotopeSummary, EcotopeError>) -> Self {
        match result {
            Ok(summary) => Self::Placed(summary.clone()),
            Err(error) => Self::Failed(error.to_string()),
        }
    }

    /// The overlay text, and whether it reports a failure: for example
    /// `412 copies in 5 Scatters · placed in 85 ms · 2 redrawn`.
    pub fn line(&self) -> (String, bool) {
        match self {
            Self::Placing => ("Placing the Ecotope…".into(), false),
            Self::Placed(summary) => {
                let redrawn = summary.layers.iter().filter(|l| l.redrawn).count();
                let line = format!(
                    "{} copies in {} Scatters · placed in {} ms · {redrawn} redrawn",
                    summary.copies(),
                    summary.layers.len(),
                    summary.place_time.as_millis(),
                );
                (line, false)
            }
            Self::Failed(error) => (format!("Cannot place this Ecotope: {error}"), true),
        }
    }
}

/// The Ecotope preset [`START_PRESET`] names.
#[cfg(test)]
pub fn start_preset() -> &'static EcotopePreset {
    EcotopePreset::named(START_PRESET).expect("the start preset is built in")
}

/// What the scene shows for `ecotope` seen as `view`.
pub fn settings(ecotope: &Ecotope, view: EcotopeView) -> EcotopeSettings {
    EcotopeSettings {
        ecotope: ecotope.clone(),
        ground: view.ground,
        size: view.size,
    }
}

/// The index in [`KINDS`] of `kind`.
pub fn kind_index(kind: &ScatterKind) -> usize {
    match kind {
        ScatterKind::Tree(_) => 0,
        ScatterKind::Stone(_) => 1,
        ScatterKind::Deadwood(_) => 2,
        ScatterKind::Mushroom { .. } => 3,
        ScatterKind::Plant(_) => 4,
    }
}

/// A new Scatter of kind `kind`, an index in [`KINDS`]: a pine stand,
/// river stones, fallen logs, porcini or a forest floor of moss, ferns and
/// leaf litter. Indices past the end give mushrooms.
pub fn new_scatter(kind: usize) -> ScatterKind {
    match kind {
        0 => ScatterKind::Tree(TreeScatterPreset::pine_stand().tree_scatter.clone()),
        1 => ScatterKind::Stone(StoneScatterOptions {
            default_seed: 0,
            classes: vec![new_stone_class(ChildKey::from_static("stones"))],
        }),
        2 => ScatterKind::Deadwood(DeadwoodScatterOptions::fallen_logs()),
        4 => ScatterKind::Plant(PlantScatterPreset::FOREST_FLOOR.plant_scatter.clone()),
        _ => ScatterKind::Mushroom {
            mushroom: MushroomPreset::porcini().mushroom.clone(),
            scatter: MushroomScatterOptions {
                density: 0.1,
                min_spacing: 1.0,
                ..MushroomScatterOptions::default()
            },
        },
    }
}

/// A new stone class named `key`: river stones, about one every 50 square
/// metres.
pub fn new_stone_class(key: ChildKey) -> StoneClass {
    StoneClass::new(key, StonePreset::river_stone().stone.clone(), 0.02, 2.0)
}

/// The usual priority of a new Scatter of kind `kind`, an index in
/// [`KINDS`]: 900 for trees, 850 for stones, 800 for dead wood, 200 for
/// mushrooms and 150 for plants.
fn usual_priority(kind: usize) -> u32 {
    match kind {
        0 => 900,
        1 => 850,
        2 => 800,
        4 => 150,
        _ => 200,
    }
}

/// The priority nearest to `wanted` that no Scatter of `ecotope` has,
/// looking below it first.
pub fn free_priority(ecotope: &Ecotope, wanted: u32) -> u32 {
    let taken = |p: u32| ecotope.scatters.values().any(|s| s.priority == p);
    (0..=wanted)
        .rev()
        .chain(wanted + 1..)
        .find(|p| !taken(*p))
        .expect("an Ecotope has fewer Scatters than priorities")
}

/// The first of `stem`, `stem_2`, `stem_3` and so on that no Scatter of
/// `ecotope` is keyed by and whose seed differs from every other key's.
///
/// # Panics
///
/// Panics if `stem` is not a valid key.
pub fn free_key(ecotope: &Ecotope, stem: &str) -> ChildKey {
    (1..)
        .map(|n| match n {
            1 => stem.to_owned(),
            n => format!("{stem}_{n}"),
        })
        .map(|key| ChildKey::new(key).expect("the stem is a valid key"))
        .find(|key| {
            ecotope
                .scatters
                .keys()
                .all(|taken| taken != key && taken.salt() != key.salt())
        })
        .expect("a free key")
}

/// Adds a new Scatter of kind `kind`, an index in [`KINDS`], under a free
/// key named after the kind and at a free priority near the kind's usual
/// one. Returns its key.
pub fn add_scatter(ecotope: &mut Ecotope, kind: usize) -> ChildKey {
    let stems = ["trees", "stones", "deadwood", "mushrooms", "plants"];
    let key = free_key(ecotope, stems[kind.min(stems.len() - 1)]);
    let priority = free_priority(ecotope, usual_priority(kind));
    ecotope.scatters.insert(
        key.clone(),
        EcotopeScatter::new(priority, new_scatter(kind)),
    );
    key
}

/// Renames the Scatter keyed `from` to `to`, keeping its settings. Renaming
/// a Scatter moves every copy of it, since its seed comes from its key.
///
/// # Errors
///
/// Returns a readable message if `to` is not a valid key, or another
/// Scatter already has it or a key with the same seed.
pub fn rename_scatter(
    ecotope: &mut Ecotope,
    from: &ChildKey,
    to: &str,
) -> Result<ChildKey, String> {
    let to = ChildKey::new(to.trim()).map_err(|error| format!("Cannot use that key: {error}"))?;
    if &to == from {
        return Ok(to);
    }
    if let Some(taken) = ecotope
        .scatters
        .keys()
        .find(|key| *key != from && (**key == to || key.salt() == to.salt()))
    {
        return Err(format!(
            "Cannot use the key {to}: the Scatter {taken} would share its seed"
        ));
    }
    let scatter = ecotope
        .scatters
        .remove(from)
        .ok_or_else(|| format!("There is no Scatter {from}"))?;
    ecotope.scatters.insert(to.clone(), scatter);
    Ok(to)
}

/// Reads the `.ecotope.yml` preset file `text`, whose key (its file name
/// without the ending) is `key`. Returns a readable error if it is not a
/// valid Ecotope.
pub fn read_ecotope(key: &str, text: &str) -> Result<EcotopePreset, String> {
    read_preset_file::<EcotopePreset>(key, text)
        .map_err(|error| format!("Not Ecotope settings: {error}"))
}

/// The text of a `.ecotope.yml` file named `key` holding `ecotope`.
///
/// The file names `base`, if any, and holds only what differs from it.
pub fn file_text(ecotope: &Ecotope, key: &str, base: Option<&'static EcotopePreset>) -> String {
    let meta = PresetMeta {
        key: key.to_owned(),
        name: key.to_owned(),
        description: String::new(),
        group: FILE_GROUP.to_owned(),
    };
    EcotopePreset::from_parts(meta, ecotope.clone(), base).to_file(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: &'static str) -> ChildKey {
        ChildKey::from_static(key)
    }

    #[test]
    fn new_scatters_of_every_kind_are_valid_and_take_free_keys_and_priorities() {
        let mut ecotope = start_preset().ecotope.clone();
        for (kind, name) in KINDS.iter().enumerate() {
            let added = add_scatter(&mut ecotope, kind);
            assert_eq!(
                kind_index(&ecotope.scatters[&added].scatter),
                kind,
                "{name}"
            );
        }
        // Two more trees take the next keys, at free priorities.
        let second = add_scatter(&mut ecotope, 0);
        let third = add_scatter(&mut ecotope, 0);
        assert_eq!((second.as_str(), third.as_str()), ("trees_2", "trees_3"));
        ecotope.validate().expect("every added Scatter is valid");
        assert_eq!(free_priority(&ecotope, 900), 896);
    }

    #[test]
    fn renaming_keeps_the_settings_and_refuses_taken_or_bad_keys() {
        let mut ecotope = start_preset().ecotope.clone();
        let logs = ecotope.scatters[&key("logs")].clone();
        assert_eq!(
            rename_scatter(&mut ecotope, &key("logs"), " fallen_logs "),
            Ok(key("fallen_logs"))
        );
        assert_eq!(ecotope.scatters[&key("fallen_logs")], logs);
        assert!(!ecotope.scatters.contains_key(&key("logs")));
        assert!(rename_scatter(&mut ecotope, &key("pines"), "porcini").is_err());
        assert!(rename_scatter(&mut ecotope, &key("pines"), "Pines!").is_err());
        assert!(ecotope.scatters.contains_key(&key("pines")));
    }

    #[test]
    fn saved_files_read_back_to_the_same_ecotope() {
        let base = start_preset();
        let read = |text: &str| read_ecotope("mine", text).expect(text).ecotope;

        // An edit, an added Scatter and a renamed one, over the base.
        let mut ecotope = base.ecotope.clone();
        ecotope.default_seed = 99;
        ecotope.scatters.get_mut(&key("pines")).unwrap().clearance = 1.5;
        add_scatter(&mut ecotope, 3);
        let text = file_text(&ecotope, "mine", Some(base));
        assert!(text.contains("base: pine_forest_floor"), "{text}");
        assert_eq!(read(&text), ecotope);

        // A renamed Scatter removes its old key from the base.
        rename_scatter(&mut ecotope, &key("logs"), "old_logs").unwrap();
        let text = file_text(&ecotope, "mine", Some(base));
        assert!(text.contains("base: pine_forest_floor"), "{text}");
        assert!(text.contains("logs: null"), "{text}");
        assert_eq!(read(&text), ecotope);
    }

    #[test]
    fn a_setting_set_back_to_its_kinds_default_survives_saving_over_a_base() {
        let base = start_preset();
        let mut ecotope = base.ecotope.clone();
        let ScatterKind::Deadwood(logs) =
            &mut ecotope.scatters.get_mut(&key("logs")).unwrap().scatter
        else {
            panic!("logs are dead wood");
        };
        logs.density = DeadwoodScatterOptions::fallen_logs().density;
        let text = file_text(&ecotope, "mine", Some(base));
        assert_eq!(
            read_ecotope("mine", &text).unwrap().ecotope,
            ecotope,
            "{text}"
        );
    }

    #[test]
    fn files_that_are_not_ecotopes_are_refused() {
        assert!(read_ecotope("x", "not an ecotope").is_err());
        assert!(read_ecotope("x", "ecotope:\n  scatters:\n    a: { kind: tree }\n").is_err());
    }
}
