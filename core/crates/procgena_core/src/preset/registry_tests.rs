#![allow(
    dead_code,
    reason = "the catalogues here declare functions only some tests use"
)]

use std::borrow::Cow;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::*;

/// A rock's settings. A pile's rock given as settings without a base
/// leaves out what it doesn't change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RockOptions {
    size: f32,
    rough: bool,
}

impl Default for RockOptions {
    fn default() -> Self {
        Self {
            size: 1.0,
            rough: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct RockPreset {
    key: Cow<'static, str>,
    name: Cow<'static, str>,
    description: Cow<'static, str>,
    group: Cow<'static, str>,
    rock: RockOptions,
    /// Inherited from a base, never written in a file.
    weight: f32,
}

crate::catalogue! {
    RockPreset {
        rock: RockOptions,
        inherit: [weight],
        check: |options| {
            if options.size < 0.0 {
                return Err("size must not be negative".to_owned());
            }
            Ok(())
        },
    }
    "Large" {
        boulder { name: "Boulder", description: "A big rock.", weight: 900.0 }
            => RockOptions { size: 2.5, rough: true };
    }
    "Small" {
        pebble { name: "Pebble", description: "A small rock.", weight: 0.1 }
            => RockOptions { size: 0.1, rough: false };
    }
}

/// A pile of rocks: each rock is a rock preset's key, or settings laid
/// over one.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PileOptions {
    rocks: Vec<RockOptions>,
}

#[derive(Debug, Clone, PartialEq)]
struct PilePreset {
    key: Cow<'static, str>,
    name: Cow<'static, str>,
    description: Cow<'static, str>,
    group: Cow<'static, str>,
    pile: PileOptions,
}

/// The rock slots of a pile's settings.
fn pile_slots(settings: &Value) -> Vec<ReferenceSlot> {
    let Some(Value::Array(rocks)) = settings.get("rocks") else {
        return Vec::new();
    };
    rocks
        .iter()
        .enumerate()
        .filter_map(|(i, rock)| {
            Some(ReferenceSlot {
                path: format!("rocks.{i}"),
                kind: "rock",
                reference: slot_reference(rock)?.to_owned(),
            })
        })
        .collect()
}

/// Replaces each rock reference in a pile's settings with its settings.
fn pile_expand(settings: &mut Value, registry: &Registry) -> Result<(), PresetFileError> {
    if let Some(Value::Array(rocks)) = settings.get_mut("rocks") {
        for (i, rock) in rocks.iter_mut().enumerate() {
            expand_slot::<RockPreset>(rock, registry)
                .map_err(|error| error.at(&format!("rocks.{i}")))?;
        }
    }
    Ok(())
}

crate::catalogue! {
    PilePreset {
        pile: PileOptions,
        references: References { slots: pile_slots, expand: pile_expand },
        check: |_| Ok(()),
    }
    "Piles" {
        cairn { name: "Cairn", description: "Stacked rocks." }
            => PileOptions { rocks: vec![RockOptions::default()] };
    }
}

fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.register::<RockPreset>().register::<PilePreset>();
    registry
}

#[test]
fn a_project_preset_can_be_the_base_of_another() {
    let mut registry = registry();
    registry
        .add("rock", "tall", "base: boulder\nrock:\n  size: 5.0\n", None)
        .unwrap();
    registry
        .add(
            "rock",
            "taller",
            "base: tall\nname: Taller\nrock:\n  rough: false\n",
            None,
        )
        .unwrap();

    let taller = registry.resolve::<RockPreset>("taller").unwrap();
    assert_eq!(taller.layer(), Layer::Project);
    assert_eq!(
        taller.rock,
        RockOptions {
            size: 5.0,
            rough: false
        }
    );
    assert_eq!(taller.name, "Taller");
    assert_eq!(taller.group, FILE_GROUP);
    // Fields a file can't set come from the built-in at the chain's root.
    assert_eq!(taller.weight, 900.0);

    // A built-in preset resolves to itself.
    let boulder = registry.resolve::<RockPreset>("boulder").unwrap();
    assert!(std::ptr::eq(&*boulder, RockPreset::boulder()));
}

#[test]
fn a_preset_without_a_base_inherits_defaults() {
    let mut registry = registry();
    registry
        .add("rock", "plain", "rock:\n  size: 3.0\n", None)
        .unwrap();
    let plain = registry.resolve::<RockPreset>("plain").unwrap();
    assert_eq!(
        plain.rock,
        RockOptions {
            size: 3.0,
            rough: false
        }
    );
    assert_eq!(plain.weight, 0.0);
}

#[test]
fn a_loop_of_bases_is_an_error_naming_the_chain() {
    let mut registry = registry();
    registry.add("rock", "a", "base: b\n", None).unwrap();
    registry.add("rock", "b", "base: a\n", None).unwrap();
    let error = registry.resolve::<RockPreset>("a").unwrap_err();
    assert_eq!(
        error,
        PresetFileError::BaseCycle {
            kind: "rock".to_owned(),
            keys: vec!["a".to_owned(), "b".to_owned(), "a".to_owned()],
        }
    );
}

#[test]
fn an_unknown_base_lists_every_layer_s_keys() {
    let mut registry = registry();
    registry.add("rock", "mine", "base: nope\n", None).unwrap();
    let error = registry.resolve::<RockPreset>("mine").unwrap_err();
    assert_eq!(
        error,
        PresetFileError::UnknownBase {
            key: "nope".to_owned(),
            known: vec!["mine".to_owned(), "boulder".to_owned(), "pebble".to_owned()],
        }
    );
}

#[test]
fn a_key_in_two_layers_is_named_with_its_namespace() {
    let mut registry = registry();
    registry.set_namespace("quarry").unwrap();
    registry
        .add(
            "rock",
            "boulder",
            "base: builtin:boulder\nrock:\n  size: 7.0\n",
            None,
        )
        .unwrap();

    // The bare key is ambiguous, and the shared key is listed.
    let error = registry.resolve::<RockPreset>("boulder").unwrap_err();
    assert_eq!(
        error,
        PresetFileError::Ambiguous {
            kind: "rock".to_owned(),
            key: "boulder".to_owned(),
            choices: vec!["quarry:boulder".to_owned(), "builtin:boulder".to_owned()],
        }
    );
    assert_eq!(
        registry.problems(),
        vec![Problem::SharedKey {
            kind: "rock",
            key: "boulder".to_owned(),
            choices: vec!["quarry:boulder".to_owned(), "builtin:boulder".to_owned()],
        }]
    );

    // Each layer is reached by its namespace; the key never includes it.
    let mine = registry.resolve::<RockPreset>("quarry:boulder").unwrap();
    assert_eq!((mine.rock.size, mine.key.as_ref()), (7.0, "boulder"));
    assert_eq!(mine.weight, 900.0);
    let builtin = registry.resolve::<RockPreset>("builtin:boulder").unwrap();
    assert_eq!(builtin.rock.size, 2.5);

    // A namespace no layer has, and a key a namespace lacks, are errors.
    let error = registry.resolve::<RockPreset>("other:boulder").unwrap_err();
    assert!(error.to_string().contains("quarry, builtin"), "{error}");
    assert!(matches!(
        registry.resolve::<RockPreset>("quarry:pebble"),
        Err(PresetFileError::UnknownPreset { .. })
    ));

    // Unknown keys list shared keys with their namespaces.
    let error = registry.resolve::<RockPreset>("gravel").unwrap_err();
    let PresetFileError::UnknownPreset { known, .. } = error else {
        panic!("expected an unknown preset, got {error:?}");
    };
    assert_eq!(known, ["quarry:boulder", "builtin:boulder", "pebble"]);
}

#[test]
fn builtin_is_not_a_project_namespace() {
    let mut registry = registry();
    for bad in [BUILTIN, "My Project", ""] {
        let error = registry.set_namespace(bad).unwrap_err();
        assert!(
            matches!(error, PresetFileError::BadNamespace { .. }),
            "{bad}"
        );
    }
    assert!(
        check_namespace(BUILTIN)
            .unwrap_err()
            .to_string()
            .contains("reserved")
    );
    assert_eq!(registry.namespace(Layer::Project), None);
    assert_eq!(registry.namespace(Layer::BuiltIn), Some(BUILTIN));
}

#[test]
fn references_parse_with_at_most_one_namespace() {
    let reference = Reference::parse("builtin:boulder").unwrap();
    assert_eq!(reference.namespace, Some(BUILTIN));
    assert_eq!(reference.key, "boulder");
    assert_eq!(reference.to_string(), "builtin:boulder");
    assert_eq!(Reference::parse("boulder").unwrap().namespace, None);
    for bad in ["a:b:c", ":boulder", "builtin:", ""] {
        assert!(
            matches!(
                Reference::parse(bad),
                Err(PresetFileError::BadReference { .. })
            ),
            "{bad}"
        );
    }
}

#[test]
fn references_resolve_through_the_registry() {
    let mut registry = registry();
    registry
        .add("rock", "tall", "base: boulder\nrock:\n  size: 5.0\n", None)
        .unwrap();
    registry
        .add(
            "pile",
            "heap",
            "pile:\n  rocks:\n    - tall\n    - { base: pebble, rough: true }\n    - { size: 4.0 }\n",
            None,
        )
        .unwrap();

    let heap = registry.resolve::<PilePreset>("heap").unwrap();
    assert_eq!(
        heap.pile.rocks,
        [
            RockOptions {
                size: 5.0,
                rough: true
            },
            RockOptions {
                size: 0.1,
                rough: true
            },
            RockOptions {
                size: 4.0,
                rough: false
            },
        ]
    );

    // The file form keeps the references as written.
    let settings = registry.settings("pile", "heap").unwrap();
    assert_eq!(settings["rocks"][0], "tall");
}

#[test]
fn a_bad_reference_names_its_setting() {
    let mut registry = registry();
    registry
        .add("pile", "heap", "pile:\n  rocks: [nope]\n", None)
        .unwrap();
    let error = registry.resolve::<PilePreset>("heap").unwrap_err();
    let PresetFileError::Setting { path, message } = error else {
        panic!("expected a setting error, got {error:?}");
    };
    assert_eq!(path, "pile.rocks.0");
    assert!(
        message.contains("unknown rock preset \"nope\""),
        "{message}"
    );
}

#[test]
fn used_by_lists_bases_and_references() {
    let mut registry = registry();
    registry
        .add("rock", "tall", "base: boulder\n", None)
        .unwrap();
    registry
        .add("rock", "taller", "base: tall\n", None)
        .unwrap();
    registry.set_namespace("quarry").unwrap();
    registry
        .add(
            "pile",
            "heap",
            "pile:\n  rocks: [pebble, quarry:tall]\n",
            None,
        )
        .unwrap();
    registry
        .add("pile", "bigger_heap", "base: heap\n", None)
        .unwrap();

    let uses = registry.used_by("rock", Layer::Project, "tall");
    let found: Vec<(&str, &str, Option<&str>)> = uses
        .iter()
        .map(|used| (used.kind, used.key.as_str(), used.path.as_deref()))
        .collect();
    assert_eq!(
        found,
        [
            ("rock", "taller", None),
            ("pile", "heap", Some("rocks.1")),
            ("pile", "bigger_heap", Some("rocks.1")),
        ]
    );
    let pebble: Vec<String> = registry
        .used_by("rock", Layer::BuiltIn, "pebble")
        .into_iter()
        .map(|used| used.key)
        .collect();
    assert_eq!(pebble, ["heap", "bigger_heap"]);
    assert!(
        registry
            .used_by("rock", Layer::Project, "pebble")
            .is_empty()
    );
}

#[test]
fn entries_list_the_project_before_the_built_ins() {
    let mut registry = registry();
    registry
        .add(
            "rock",
            "tall",
            "base: boulder\nname: Tall\ngroup: Mine\n",
            None,
        )
        .unwrap();
    let entries = registry.entries("rock");
    let keys: Vec<(Layer, &str, &str)> = entries
        .iter()
        .map(|entry| (entry.layer, entry.key.as_str(), entry.group.as_str()))
        .collect();
    assert_eq!(
        keys,
        [
            (Layer::Project, "tall", "Mine"),
            (Layer::BuiltIn, "boulder", "Large"),
            (Layer::BuiltIn, "pebble", "Small"),
        ]
    );
    assert_eq!(entries[0].base.as_deref(), Some("boulder"));
}

#[test]
fn adding_rejects_bad_keys_unknown_kinds_and_bad_text() {
    let mut registry = registry();
    assert!(matches!(
        registry.add("rock", "Tall-Rock", "", None),
        Err(Problem::BadKey { .. })
    ));
    assert!(matches!(
        registry.add("tree", "tall", "", None),
        Err(Problem::UnknownKind { .. })
    ));
    assert!(matches!(
        registry.add("rock", "tall", "colour: red\n", None),
        Err(Problem::Unreadable { .. })
    ));
    assert!(registry.project().is_empty());
}

/// A new, empty folder for one test.
fn temp_folder(name: &str) -> PathBuf {
    let folder =
        std::env::temp_dir().join(format!("procgena_registry_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

#[test]
fn a_presets_folder_loads_and_reports_what_it_skips() {
    let presets = temp_folder("load");
    let write = |path: &str, text: &str| {
        let path = presets.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    write("rock/tall.rock.yml", "base: boulder\nrock:\n  size: 5.0\n");
    write("rock/notes.txt", "hello");
    write("rock/Big.rock.yml", "");
    write("rock/.DS_Store", "");
    write("pile/heap.pile.yml", "pile:\n  rocks: [tall]\n");
    write("weird/thing.weird.yml", "");
    write("README.md", "");

    let mut registry = registry();
    registry.load_folder(&presets).unwrap();

    // The presets that fit are read.
    let keys: Vec<(&str, &str)> = registry
        .project()
        .iter()
        .map(|preset| (preset.kind, preset.key.as_str()))
        .collect();
    assert_eq!(keys, [("pile", "heap"), ("rock", "tall")]);
    assert_eq!(
        registry.project_preset("rock", "tall").unwrap().path,
        Some(preset_path(&presets, "rock", "tall"))
    );
    let heap = registry.resolve::<PilePreset>("heap").unwrap();
    assert_eq!(heap.pile.rocks[0].size, 5.0);

    // The rest is reported, and dot files are ignored.
    let problems = registry.problems();
    assert_eq!(problems.len(), 4, "{problems:#?}");
    assert!(matches!(&problems[0], Problem::Misplaced { path, .. } if path.ends_with("README.md")));
    assert!(matches!(&problems[1], Problem::BadKey { key, .. } if key == "Big"));
    assert!(matches!(&problems[2], Problem::Misplaced { path, .. } if path.ends_with("notes.txt")));
    assert!(matches!(&problems[3], Problem::UnknownKind { path } if path.ends_with("weird")));

    // A missing folder is an empty project.
    registry.load_folder(&presets.join("missing")).unwrap();
    assert!(registry.project().is_empty());
    assert!(registry.problems().is_empty());
    std::fs::remove_dir_all(&presets).unwrap();
}

#[test]
fn a_file_is_written_as_differences_from_its_project_base() {
    let mut registry = registry();
    registry
        .add("rock", "tall", "base: boulder\nrock:\n  size: 5.0\n", None)
        .unwrap();

    // An edited copy of `tall`'s full settings, saved over `tall`.
    let mut settings = registry.settings("rock", "tall").unwrap();
    settings["rough"] = Value::Bool(false);
    let document = Document {
        base: Some("tall".to_owned()),
        name: Some("Smooth".to_owned()),
        settings: Some(settings),
        ..Document::default()
    };
    let text = registry.file_text("rock", &document).unwrap();
    assert_eq!(text, "base: tall\nname: Smooth\nrock:\n  rough: false\n");
    assert_eq!(registry.file_text("rock", &document).unwrap(), text);

    // It reads back to the same settings.
    registry.add("rock", "smooth", &text, None).unwrap();
    let smooth = registry.resolve::<RockPreset>("smooth").unwrap();
    assert_eq!(
        smooth.rock,
        RockOptions {
            size: 5.0,
            rough: false
        }
    );

    // Nothing differing writes no settings.
    let same = Document {
        base: Some("tall".to_owned()),
        settings: Some(registry.settings("rock", "tall").unwrap()),
        ..Document::default()
    };
    assert_eq!(registry.file_text("rock", &same).unwrap(), "base: tall\n");
}

#[test]
fn a_document_not_in_the_registry_reads_over_project_bases() {
    let mut registry = registry();
    registry
        .add("rock", "tall", "base: boulder\nrock:\n  size: 5.0\n", None)
        .unwrap();
    let rock: RockPreset = registry
        .read("draft", "base: tall\nrock:\n  rough: false\n")
        .unwrap();
    assert_eq!(
        rock.rock,
        RockOptions {
            size: 5.0,
            rough: false
        }
    );
    assert_eq!(rock.weight, 900.0);
}
