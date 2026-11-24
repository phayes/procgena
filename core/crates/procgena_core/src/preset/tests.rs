use std::borrow::Cow;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    default_seed: u32,
    height: f32,
    enabled: bool,
    sizes: Vec<f32>,
    end: End,
    level: PerLevel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PerLevel {
    #[serde(rename = "0")]
    zero: f32,
    #[serde(rename = "1")]
    one: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum End {
    Saw { step: f32 },
    Axe { angle: f32, jaggedness: f32 },
}

impl Default for Options {
    fn default() -> Self {
        Self {
            default_seed: 1,
            height: 0.6,
            enabled: false,
            sizes: vec![1.0, 2.0],
            end: End::Saw { step: 0.1 },
            level: PerLevel {
                zero: 0.5,
                one: 0.25,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Thing {
    key: Cow<'static, str>,
    name: Cow<'static, str>,
    description: Cow<'static, str>,
    group: Cow<'static, str>,
    thing: Options,
}

static ALL: LazyLock<Vec<Thing>> = LazyLock::new(|| {
    vec![
        Thing {
            key: "tall".into(),
            name: "Tall".into(),
            description: "A tall thing.".into(),
            group: "Big".into(),
            thing: Options {
                height: 3.0,
                ..Options::default()
            },
        },
        Thing {
            key: "small".into(),
            name: "Small".into(),
            description: "A small thing.".into(),
            group: "Little".into(),
            thing: Options::default(),
        },
        Thing {
            key: "huge".into(),
            name: "Huge".into(),
            description: "A huge thing.".into(),
            group: "Big".into(),
            thing: Options {
                height: 9.0,
                ..Options::default()
            },
        },
    ]
});

impl Preset for Thing {
    type Options = Options;
    const KIND: &'static str = "thing";
    fn key(&self) -> &str {
        &self.key
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn group(&self) -> &str {
        &self.group
    }
    fn options(&self) -> &Options {
        &self.thing
    }
    fn into_options(self) -> Options {
        self.thing
    }
    fn from_parts(meta: PresetMeta, thing: Options, _base: Option<&'static Self>) -> Self {
        Self {
            key: meta.key.into(),
            name: meta.name.into(),
            description: meta.description.into(),
            group: meta.group.into(),
            thing,
        }
    }
    fn all() -> impl Iterator<Item = &'static Self> {
        ALL.iter()
    }
    fn check(options: &Options) -> Result<(), String> {
        if options.height < 0.0 {
            return Err("height must not be negative".to_owned());
        }
        Ok(())
    }
}

#[test]
fn a_file_changes_only_what_it_names() {
    let text = "base: tall\nthing:\n  enabled: yes\n  level:\n    1: 0.75\n";
    let thing: Thing = read("mine", text).unwrap();
    let expected = Options {
        enabled: true,
        level: PerLevel {
            zero: 0.5,
            one: 0.75,
        },
        ..ALL[0].thing.clone()
    };
    assert_eq!(thing.thing, expected);
    assert_eq!(thing.key, "mine");
    assert_eq!(thing.name, "mine");
    assert_eq!(thing.description, "");
    assert_eq!(thing.group, FILE_GROUP);
}

#[test]
fn without_a_base_settings_lay_over_the_defaults() {
    let thing: Thing = read("mine", "name: Mine\nthing:\n  height: 2.5\n").unwrap();
    assert_eq!(
        thing.thing,
        Options {
            height: 2.5,
            ..Options::default()
        }
    );
    assert_eq!(thing.name, "Mine");
    let empty: Thing = read("empty", "").unwrap();
    assert_eq!(empty.thing, Options::default());
}

#[test]
fn lists_replace_the_base_list_whole() {
    let thing: Thing = read("mine", "thing:\n  sizes: [5.0]\n").unwrap();
    assert_eq!(thing.thing.sizes, vec![5.0]);
}

#[test]
fn a_different_kind_replaces_the_group_whole() {
    // The saw's `step` must not linger in the axe end.
    let text = "thing:\n  end:\n    kind: axe\n    angle: 30.0\n    jaggedness: 0.2\n";
    let thing: Thing = read("mine", text).unwrap();
    assert_eq!(
        thing.thing.end,
        End::Axe {
            angle: 30.0,
            jaggedness: 0.2
        }
    );

    // The same kind merges key by key.
    let mut base = json!({"end": {"kind": "saw", "step": 0.1, "teeth": 3}});
    merge(&mut base, json!({"end": {"kind": "saw", "step": 0.2}}));
    assert_eq!(
        base,
        json!({"end": {"kind": "saw", "step": 0.2, "teeth": 3}})
    );
}

#[test]
fn errors_name_the_setting() {
    let error =
        read::<Thing>("mine", "thing:\n  level:\n    \"0\": 1.0\n    two: 1.0\n").unwrap_err();
    assert!(
        matches!(&error, PresetFileError::Setting { path, .. } if path == "thing.level.two"),
        "{error:?}"
    );
    assert!(error.to_string().contains("unknown field"), "{error}");

    let error = read::<Thing>("mine", "thing:\n  height: tall\n").unwrap_err();
    assert!(
        matches!(&error, PresetFileError::Setting { path, .. } if path == "thing.height"),
        "{error:?}"
    );

    let error = read::<Thing>("mine", "thing:\n  sizes: [1.0, x]\n").unwrap_err();
    assert!(
        matches!(&error, PresetFileError::Setting { path, .. } if path == "thing.sizes[1]"),
        "{error:?}"
    );
}

#[test]
fn errors_report_bad_files() {
    let error = read::<Thing>("mine", "base: nope\n").unwrap_err();
    assert!(
        matches!(&error, PresetFileError::UnknownBase { key, known } if key == "nope" && known.len() == 3),
        "{error:?}"
    );

    let error = read::<Thing>("mine", "tree:\n  height: 1.0\n").unwrap_err();
    assert!(
        matches!(&error, PresetFileError::UnknownKey { key, .. } if key == "tree"),
        "{error:?}"
    );

    let error = read::<Thing>("mine", "thing: [1, 2]\n").unwrap_err();
    assert!(
        matches!(&error, PresetFileError::NotAMap { path } if path == "thing"),
        "{error:?}"
    );

    let error = read::<Thing>("mine", "thing:\n  height: 1.0\n  height: 2.0\n").unwrap_err();
    assert!(matches!(&error, PresetFileError::Yaml { .. }), "{error:?}");

    let error = read::<Thing>("mine", "thing:\n  height: -1.0\n").unwrap_err();
    assert_eq!(
        error,
        PresetFileError::Invalid {
            message: "height must not be negative".to_owned()
        }
    );
}

#[test]
fn a_written_file_holds_only_what_differs_and_reads_back() {
    let mut thing = Thing::from_parts(
        PresetMeta {
            key: "mine".to_owned(),
            name: "Mine".to_owned(),
            description: "My own.".to_owned(),
            group: FILE_GROUP.to_owned(),
        },
        ALL[0].thing.clone(),
        None,
    );
    thing.thing.height = 0.6;
    thing.thing.level.one = 0.7;
    thing.thing.end = End::Axe {
        angle: 20.0,
        jaggedness: 0.1,
    };

    let text = write(&thing, Some(&ALL[0]));
    assert_eq!(
        text,
        "base: tall\nname: Mine\ndescription: My own.\nthing:\n  height: 0.6\n  end:\n    kind: axe\n    angle: 20.0\n    jaggedness: 0.1\n  level:\n    \"1\": 0.7\n"
    );
    assert_eq!(read::<Thing>("mine", &text).unwrap(), thing);

    // Without a base, the file is relative to the defaults.
    let text = write(&thing, None);
    assert_eq!(read::<Thing>("mine", &text).unwrap(), thing);
}

#[test]
fn groups_keep_catalogue_order() {
    let groups = groups::<Thing>();
    let keys: Vec<(&str, Vec<&str>)> = groups
        .iter()
        .map(|(group, presets)| (*group, presets.iter().map(|preset| preset.key()).collect()))
        .collect();
    assert_eq!(
        keys,
        vec![("Big", vec!["tall", "huge"]), ("Little", vec!["small"])]
    );
}
