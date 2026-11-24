//! Builds a kind's Inspector settings from its metadata file.

use std::borrow::Cow;

use procgena_core::meta::{
    Choice, ChoiceLists, ControlKind, GroupMeta, KindMeta, Scale, SettingLayout, SettingMeta,
    SettingPath, choice_index, pick_choice,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::Settings;
use crate::params::{Group, Param, Section, linear_from_hex, linear_to_hex};

/// A kind's metadata with the choice lists its crate supplies.
pub struct MetaKind {
    /// The kind's metadata file, read.
    pub meta: KindMeta,
    /// The choice lists the metadata names.
    pub lists: ChoiceLists,
}

impl MetaKind {
    /// Reads the metadata file `text`, whose choices come from `lists`.
    ///
    /// # Panics
    ///
    /// Panics if `text` doesn't read. Each crate's drift test reads its
    /// file, so this only happens if that test fails.
    pub fn new(text: &str, lists: ChoiceLists) -> Self {
        let meta = KindMeta::parse(text)
            .unwrap_or_else(|error| panic!("a built-in metadata file doesn't read: {error}"));
        Self { meta, lists }
    }
}

/// Builds the custom controls of a group whose metadata names one (see
/// [`GroupMeta::custom`]): given the control's name and the group, returns
/// its settings.
pub type CustomGroup<T> = dyn Fn(&str, &'static GroupMeta) -> Vec<Param<Settings<T>>>;

/// The Inspector's settings of `kind`, laid out as its metadata says, for
/// settings shaped like `value`.
///
/// Settings matched by a `*` pattern are listed once for each match in
/// `value`, so `value` must have the shape the panel will edit. Hidden
/// settings are left out. Groups that name a custom control get the
/// settings `custom` builds for them.
pub fn sections<T>(
    kind: &'static MetaKind,
    value: &Value,
    custom: &CustomGroup<T>,
) -> Vec<Section<Settings<T>>>
where
    T: Serialize + DeserializeOwned + Clone + Send + Sync + 'static,
{
    kind.meta
        .layout(value)
        .into_iter()
        .map(|section| Section {
            title: section.section.title.as_str(),
            groups: section
                .groups
                .into_iter()
                .map(|group| {
                    let params = match &group.group.custom {
                        Some(name) => custom(name, group.group),
                        None => group
                            .settings
                            .into_iter()
                            .filter(|setting| !setting.meta.hidden)
                            .flat_map(|setting| params(kind, setting, value))
                            .collect(),
                    };
                    Group {
                        title: group.group.title.as_deref(),
                        params,
                    }
                })
                .collect(),
        })
        .collect()
}

/// The settings of one item of a list, from a custom group whose settings
/// are patterns such as `trees.*.spacing`: `prefix` names the item, such as
/// `trees.2`, and the panel edits only the item's value, `item`.
///
/// # Panics
///
/// Panics if one of the group's settings has a condition: conditions read
/// the whole settings tree, which an item's panel doesn't have.
pub fn item_params<T>(
    kind: &'static MetaKind,
    group: &'static GroupMeta,
    prefix: &SettingPath,
    item: &Value,
) -> Vec<Param<Settings<T>>>
where
    T: Serialize + DeserializeOwned + Clone + Send + Sync + 'static,
{
    group
        .settings
        .iter()
        .filter_map(|pattern| {
            // The item's own path: the pattern's segments after the prefix.
            let segments = SettingPath::parse(&pattern.to_string());
            let rest = segments.segments().get(prefix.segments().len()..)?;
            let full = rest
                .iter()
                .fold(prefix.clone(), |path, s| path.child(s.clone()));
            let (meta, m) = kind.meta.setting(&full)?;
            assert!(
                meta.shown_if.is_none(),
                "{full}: settings of a list item can't have conditions"
            );
            let path = rest
                .iter()
                .fold(SettingPath::default(), |path, s| path.child(s.clone()));
            (!meta.hidden && !rest.is_empty()).then(|| controls(kind, &path, meta, m.index, item))
        })
        .flatten()
        .collect()
}

/// The controls of one setting, shown while its metadata's conditions hold.
fn params<T>(
    kind: &'static MetaKind,
    setting: SettingLayout<'static>,
    value: &Value,
) -> Vec<Param<Settings<T>>>
where
    T: Serialize + DeserializeOwned + Clone + Send + Sync + 'static,
{
    let SettingLayout {
        path, meta, index, ..
    } = setting;
    let mut params = controls(kind, &path, meta, index, value);
    for param in &mut params {
        let path = path.clone();
        let meta_kind = &kind.meta;
        let own = param.shown.take();
        param.shown = Some(std::sync::Arc::new(move |s: &Settings<T>| {
            meta_kind.is_shown(&path, s.value()) && own.as_ref().is_none_or(|own| own(s))
        }));
    }
    params
}

/// The controls of the setting at `path`, with its description and part:
/// usually one, two for an optional number (its "Set by Hand" switch, then
/// the number, shown while set), none for settings the Inspector can't edit
/// (references and text, which kinds edit with their own controls).
fn controls<T>(
    kind: &'static MetaKind,
    path: &SettingPath,
    meta: &'static SettingMeta,
    index: Option<usize>,
    value: &Value,
) -> Vec<Param<Settings<T>>>
where
    T: Serialize + DeserializeOwned + Clone + Send + Sync + 'static,
{
    let current = path.get(value).cloned().unwrap_or(Value::Null);
    let label = match &meta.unit {
        Some(unit) => format!("{} ({unit})", meta.label),
        None => meta.label.clone(),
    };
    let key = path.to_string();

    // The control the setting's metadata or value calls for.
    let mut params = match &meta.control {
        ControlKind::Choice(_) => {
            let choices = meta.choices(&kind.lists).unwrap_or_default();
            vec![choice(key, label, path.clone(), choices)]
        }
        ControlKind::Color => vec![color(key, label, path.clone())],
        ControlKind::Ref(_) => Vec::new(),
        ControlKind::FromValue => match current {
            Value::Bool(_) => vec![toggle(key, label, path.clone())],
            Value::Number(_) | Value::Null if meta.optional.is_some() || current.is_number() => {
                number_params(key, label, path, meta, index, value)
            }
            _ => Vec::new(),
        },
    };
    let part = meta.part.as_deref().or_else(|| kind.meta.part_of(path));
    for param in &mut params {
        param.description = (!meta.description.is_empty()).then_some(meta.description.as_str());
        param.part = part;
    }
    params
}

/// A number setting, after a "Set by Hand" switch when it is optional.
fn number_params<T>(
    key: String,
    label: String,
    path: &SettingPath,
    meta: &'static SettingMeta,
    index: Option<usize>,
    value: &Value,
) -> Vec<Param<Settings<T>>>
where
    T: Serialize + DeserializeOwned + Clone + Send + Sync + 'static,
{
    let [min, max] = meta.range.unwrap_or([0.0, 1.0]);
    let step = meta.step.unwrap_or(0.01);
    #[allow(clippy::cast_possible_truncation)]
    let range = (min as f32, max as f32, step as f32);
    let start = meta.optional;

    // The number reads as its start value (or the range's bottom) while unset.
    let read = path.clone();
    let write = path.clone();
    #[allow(clippy::cast_possible_truncation)]
    let mut number = Param::number(
        key.clone(),
        label.clone(),
        range,
        move |s: &Settings<T>| {
            let value = read.get(s.value()).and_then(Value::as_f64);
            value.unwrap_or(start.unwrap_or(min)) as f32
        },
        move |s: &mut Settings<T>, v| {
            let current = write.get(s.value()).cloned().unwrap_or(Value::Null);
            if !current.is_null() {
                s.set(&write, meta.number_value(&current, f64::from(v)));
            }
        },
    );
    if meta.scale == Scale::Exponential {
        number = number.exponential();
    }
    if meta.revive.is_some() && meta.revive_value(value, index).is_some() {
        #[allow(clippy::cast_possible_truncation)]
        let revive =
            move |s: &Settings<T>| meta.revive_value(s.value(), index).unwrap_or(0.0) as f32;
        number = number.revive_with(revive);
    }
    let Some(start) = start else {
        return vec![number];
    };

    // An optional number shows only while set, after its switch.
    let set_path = path.clone();
    let switch_path = path.clone();
    let switch = Param::toggle(
        format!("{key}.set"),
        format!("{label}: Set by Hand"),
        move |s: &Settings<T>| !set_path.get(s.value()).is_none_or(Value::is_null),
        move |s: &mut Settings<T>, on| {
            let new = if on {
                meta.number_value(&Value::Null, start)
            } else {
                Value::Null
            };
            s.set(&switch_path, new);
        },
    );
    let shown_path = path.clone();
    let number = number
        .shown_if(move |s: &Settings<T>| !shown_path.get(s.value()).is_none_or(Value::is_null));
    vec![switch, number]
}

/// An on/off setting.
fn toggle<T>(key: String, label: String, path: SettingPath) -> Param<Settings<T>>
where
    T: Serialize + DeserializeOwned + Clone + Send + Sync + 'static,
{
    let read = path.clone();
    Param::toggle(
        key,
        label,
        move |s: &Settings<T>| {
            read.get(s.value())
                .and_then(Value::as_bool)
                .unwrap_or(false)
        },
        move |s: &mut Settings<T>, on| s.set(&path, Value::Bool(on)),
    )
}

/// A choice between `choices`. A value none of them fits shows as the
/// first.
fn choice<T>(
    key: String,
    label: String,
    path: SettingPath,
    choices: &'static [Choice],
) -> Param<Settings<T>>
where
    T: Serialize + DeserializeOwned + Clone + Send + Sync + 'static,
{
    let labels: Vec<&'static str> = choices.iter().map(|c| c.label.as_str()).collect();
    let read = path.clone();
    Param::choice_of(
        key,
        label,
        Cow::Owned(labels),
        move |s: &Settings<T>| {
            read.get(s.value())
                .and_then(|current| choice_index(choices, current))
                .unwrap_or(0)
        },
        move |s: &mut Settings<T>, index| {
            let current = path.get(s.value()).cloned().unwrap_or(Value::Null);
            if let Some(choice) = choices.get(index) {
                s.set(&path, pick_choice(choice, &current));
            }
        },
    )
}

/// A colour, written as the setting writes it: `0xRRGGBB`, `"#rrggbb"`
/// text, or three linear RGB numbers.
fn color<T>(key: String, label: String, path: SettingPath) -> Param<Settings<T>>
where
    T: Serialize + DeserializeOwned + Clone + Send + Sync + 'static,
{
    let read = path.clone();
    Param::color(
        key,
        label,
        move |s: &Settings<T>| read.get(s.value()).map_or(0, hex_of),
        move |s: &mut Settings<T>, hex| {
            let current = path.get(s.value()).cloned().unwrap_or(Value::Null);
            s.set(&path, colour_like(&current, hex));
        },
    )
}

/// A colour setting's value as the picker's `0xRRGGBB`; black when it
/// can't be read.
fn hex_of(value: &Value) -> u32 {
    match value {
        Value::Number(n) => n.as_u64().map_or(0, |n| (n & 0x00ff_ffff) as u32),
        Value::String(text) => {
            u32::from_str_radix(text.trim_start_matches('#'), 16).unwrap_or(0) & 0x00ff_ffff
        }
        Value::Array(rgb) => {
            #[allow(clippy::cast_possible_truncation)]
            let channel = |i: usize| rgb.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32;
            linear_to_hex([channel(0), channel(1), channel(2)])
        }
        _ => 0,
    }
}

/// The picker's `hex` written the way `current` is.
fn colour_like(current: &Value, hex: u32) -> Value {
    match current {
        Value::String(_) => Value::from(format!("#{:06x}", hex & 0x00ff_ffff)),
        Value::Array(_) => {
            let rgb = linear_from_hex(hex).map(|c| procgena_core::preset::to_value(&c));
            Value::Array(rgb.to_vec())
        }
        _ => Value::from(hex),
    }
}
