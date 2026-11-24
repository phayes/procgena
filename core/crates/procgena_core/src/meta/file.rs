//! A kind's metadata file, read into [`KindMeta`].

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use super::condition::{Condition, NamedConditions};
use super::path::{Match, Pattern, SettingPath, Template};

/// Why a metadata file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaError {
    /// What is wrong and where, such as
    /// `settings.bark.tint.range: invalid length 1`.
    pub message: String,
}

impl fmt::Display for MetaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for MetaError {}

fn error(message: impl Into<String>) -> MetaError {
    MetaError {
        message: message.into(),
    }
}

/// Everything a metadata file says about one kind of preset: its name,
/// its parts, how the Inspector lays out its settings, and each setting's
/// label, description, range and conditions.
#[derive(Debug, Clone)]
pub struct KindMeta {
    /// The preset file kind, such as `tree` for `.tree.yml` files.
    pub kind: String,
    /// The kind's display name, such as `Tree`.
    pub name: String,
    /// What a preset of this kind is, for people using the studio.
    pub description: String,
    /// The parts settings can target, in display order, by key.
    pub parts: Vec<(String, PartMeta)>,
    /// The Inspector's sections, in display order.
    pub sections: Vec<SectionMeta>,
    conditions: Vec<(String, Condition)>,
    settings: Vec<(Pattern, SettingMeta)>,
}

/// A part of an asset that settings target, such as a tree's leaves.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartMeta {
    /// The part's display name, such as `Leaves`.
    pub name: String,
    /// What the part is, for people using the studio.
    #[serde(default)]
    pub description: String,
    /// The specimen tags whose triangles belong to this part, for picking
    /// in the viewport.
    #[serde(default)]
    pub tags: Vec<String>,
    /// The kind of preset attached here, such as `fruit`, when the part is
    /// an attached asset.
    #[serde(default)]
    pub attached: Option<String>,
}

/// One collapsible section of the Inspector.
#[derive(Debug, Clone)]
pub struct SectionMeta {
    /// The section's heading.
    pub title: String,
    /// The part every setting in the section targets unless the setting
    /// names its own.
    pub part: Option<String>,
    /// The section's groups of settings, in display order.
    pub groups: Vec<GroupMeta>,
}

/// Settings shown together in a section, under an optional sub-heading.
#[derive(Debug, Clone)]
pub struct GroupMeta {
    /// The group's sub-heading, if it has one.
    pub title: Option<String>,
    /// Shows the whole group only while this holds, in addition to each
    /// setting's own condition.
    pub shown_if: Option<Condition>,
    /// The settings' paths or patterns, in display order. A pattern lists
    /// every matching setting of the value, in the value's order.
    pub settings: Vec<Pattern>,
}

/// What a metadata file says about one setting, or every setting a
/// pattern matches.
#[derive(Debug, Clone)]
pub struct SettingMeta {
    /// The label shown next to the control.
    pub label: String,
    /// What the setting does, for people using the studio; shown as the
    /// control's tooltip.
    pub description: String,
    /// The unit of a number, such as `m` or `°`.
    pub unit: Option<String>,
    /// The slider's ends for a number. Values outside it are allowed but
    /// can't be reached with the slider.
    pub range: Option<[f64; 2]>,
    /// The slider's step for a number.
    pub step: Option<f64>,
    /// How a number's values are spread along the slider.
    pub scale: Scale,
    /// How the setting is edited, when its value's shape doesn't say.
    pub control: ControlKind,
    /// The part the setting targets, if not its section's.
    pub part: Option<String>,
    /// Shows the setting only while this holds.
    pub shown_if: Option<Condition>,
    /// The value a number takes when an edit makes it appear while it is
    /// zero or at the bottom of its range.
    pub revive: Option<Revive>,
    /// Folded away in the Inspector by default.
    pub advanced: bool,
    /// In the file format but not in the Inspector, such as a value the
    /// Inspector's header edits.
    pub hidden: bool,
}

/// How a number's values are spread along its slider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scale {
    /// Evenly.
    #[default]
    Linear,
    /// Giving small values more room.
    Exponential,
}

/// How a setting is edited, when it is not plain from its value.
#[derive(Debug, Clone, PartialEq)]
pub enum ControlKind {
    /// Chosen from the value's shape: a slider for a number, a switch for
    /// `true` or `false`, a text field for text.
    FromValue,
    /// A colour picker. The value is `0xRRGGBB` as a number, `"#rrggbb"`
    /// text, or three linear RGB numbers.
    Color,
    /// One of a list of values.
    Choice(Choices),
    /// The key of a preset of the named kind.
    Ref(String),
}

/// Where a choice's options come from.
#[derive(Debug, Clone, PartialEq)]
pub enum Choices {
    /// Written in the metadata file.
    Listed(Vec<Choice>),
    /// A list the kind's crate supplies by name; see [`ChoiceLists`].
    Named(String),
}

/// One option of a choice.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    /// The value the setting takes, as the preset file writes it.
    pub value: Value,
    /// The option's display name.
    pub label: String,
}

/// Choice lists a crate supplies by name, for options the metadata file
/// can't list itself, such as the texture sets a crate ships.
#[derive(Debug, Clone, Default)]
pub struct ChoiceLists(BTreeMap<String, Vec<Choice>>);

impl ChoiceLists {
    /// No lists.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces the list `name`.
    #[must_use]
    pub fn with(mut self, name: impl Into<String>, choices: Vec<Choice>) -> Self {
        self.0.insert(name.into(), choices);
        self
    }

    /// The list `name`, if there is one.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&[Choice]> {
        self.0.get(name).map(Vec::as_slice)
    }

    /// Adds every list of `other`, replacing lists of the same name.
    pub fn extend(&mut self, other: Self) {
        self.0.extend(other.0);
    }
}

/// The value a number takes when it comes back into effect.
#[derive(Debug, Clone, PartialEq)]
pub enum Revive {
    /// A fixed value.
    Value(f64),
    /// Another setting's value, `from`, multiplied by `times` and raised to
    /// at least `min`. When `from` is zero or missing, `if_zero` is used
    /// instead, if given.
    From {
        /// The setting to read; may use `{index}` and `{parent}`.
        from: String,
        /// Multiplies the value read.
        times: f64,
        /// The smallest value given.
        min: Option<f64>,
        /// The value given when `from` is zero or missing.
        if_zero: Option<f64>,
    },
}

// The file's shapes as written, before conditions and patterns are read.

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    kind: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    parts: Map<String, Value>,
    #[serde(default)]
    conditions: Map<String, Value>,
    #[serde(default)]
    sections: Vec<RawSection>,
    #[serde(default)]
    settings: Map<String, Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSection {
    title: String,
    #[serde(default)]
    part: Option<String>,
    #[serde(default)]
    shown_if: Option<String>,
    #[serde(default)]
    settings: Vec<String>,
    #[serde(default)]
    groups: Vec<RawGroup>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGroup {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    shown_if: Option<String>,
    settings: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSetting {
    label: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    unit: Option<String>,
    #[serde(default)]
    range: Option<[f64; 2]>,
    #[serde(default)]
    step: Option<f64>,
    #[serde(default)]
    scale: Scale,
    #[serde(default)]
    color: bool,
    #[serde(default)]
    choices: Option<RawChoices>,
    #[serde(default, rename = "ref")]
    reference: Option<String>,
    #[serde(default)]
    part: Option<String>,
    #[serde(default)]
    shown_if: Option<String>,
    #[serde(default)]
    revive: Option<RawRevive>,
    #[serde(default)]
    advanced: bool,
    #[serde(default)]
    hidden: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawChoices {
    Named(String),
    Listed(Vec<Choice>),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawRevive {
    Value(f64),
    From(RawReviveFrom),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawReviveFrom {
    from: String,
    #[serde(default = "one")]
    times: f64,
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    if_zero: Option<f64>,
}

fn one() -> f64 {
    1.0
}

/// Reads `value` as a `T`, naming the path of what doesn't fit under `at`.
fn read<T: DeserializeOwned>(value: Value, at: &str) -> Result<T, MetaError> {
    serde_path_to_error::deserialize(value).map_err(|e| {
        let inner = e.path().to_string();
        let path = if inner == "." {
            at.to_owned()
        } else {
            format!("{at}.{inner}")
        };
        error(format!("{path}: {}", e.into_inner()))
    })
}

fn condition(text: &str, at: &str) -> Result<Condition, MetaError> {
    Condition::parse(text).map_err(|message| error(format!("{at}: {message}")))
}

impl KindMeta {
    /// Reads a `<kind>.meta.yml` file.
    ///
    /// # Errors
    ///
    /// Returns a [`MetaError`] naming the place in the file when the text
    /// is not YAML, has a key the format doesn't know, a value of the wrong
    /// shape, or a condition that can't be read.
    pub fn parse(text: &str) -> Result<Self, MetaError> {
        let value: Value =
            serde_saphyr::from_str(text).map_err(|e| error(format!("not valid YAML: {e}")))?;
        let raw: RawFile = read(value, "file")?;

        // Parts and named conditions.
        let parts = raw
            .parts
            .into_iter()
            .map(|(key, value)| Ok((key.clone(), read(value, &format!("parts.{key}"))?)))
            .collect::<Result<_, MetaError>>()?;
        let conditions = raw
            .conditions
            .into_iter()
            .map(|(name, value)| {
                let at = format!("conditions.{name}");
                let text: String = read(value, &at)?;
                Ok((name, condition(&text, &at)?))
            })
            .collect::<Result<_, MetaError>>()?;

        // Sections, with a section's own settings as its first, untitled group.
        let sections = raw
            .sections
            .into_iter()
            .enumerate()
            .map(|(i, raw)| {
                let at = format!("sections.{i}");
                let section_if = raw
                    .shown_if
                    .as_deref()
                    .map(|text| condition(text, &format!("{at}.shown_if")))
                    .transpose()?;
                let mut groups = Vec::new();
                if !raw.settings.is_empty() {
                    groups.push(GroupMeta {
                        title: None,
                        shown_if: None,
                        settings: raw.settings.iter().map(|s| Pattern::parse(s)).collect(),
                    });
                }
                for (g, group) in raw.groups.into_iter().enumerate() {
                    let shown_if = group
                        .shown_if
                        .as_deref()
                        .map(|text| condition(text, &format!("{at}.groups.{g}.shown_if")))
                        .transpose()?;
                    groups.push(GroupMeta {
                        title: group.title,
                        shown_if,
                        settings: group.settings.iter().map(|s| Pattern::parse(s)).collect(),
                    });
                }
                if let Some(section_if) = section_if {
                    for group in &mut groups {
                        group.shown_if = Some(match group.shown_if.take() {
                            Some(own) => section_if.clone().and(own),
                            None => section_if.clone(),
                        });
                    }
                }
                Ok(SectionMeta {
                    title: raw.title,
                    part: raw.part,
                    groups,
                })
            })
            .collect::<Result<_, MetaError>>()?;

        // Settings.
        let settings = raw
            .settings
            .into_iter()
            .map(|(key, value)| {
                let at = format!("settings.{key}");
                let raw: RawSetting = read(value, &at)?;
                Ok((Pattern::parse(&key), setting(raw, &at)?))
            })
            .collect::<Result<_, MetaError>>()?;

        Ok(Self {
            kind: raw.kind,
            name: raw.name,
            description: raw.description,
            parts,
            sections,
            conditions,
            settings,
        })
    }

    /// The part with key `key`, if there is one.
    #[must_use]
    pub fn part(&self, key: &str) -> Option<&PartMeta> {
        self.parts.iter().find(|(k, _)| k == key).map(|(_, p)| p)
    }

    /// Every setting entry: the path or pattern it covers, and what it says.
    pub fn entries(&self) -> impl Iterator<Item = (&Pattern, &SettingMeta)> {
        self.settings.iter().map(|(pattern, meta)| (pattern, meta))
    }

    /// Every named condition.
    pub fn conditions(&self) -> impl Iterator<Item = (&str, &Condition)> {
        self.conditions.iter().map(|(name, c)| (name.as_str(), c))
    }

    /// The first entry that covers `path`, with how it matched.
    #[must_use]
    pub fn setting(&self, path: &SettingPath) -> Option<(&SettingMeta, Match)> {
        self.settings
            .iter()
            .find_map(|(pattern, meta)| pattern.matches(path).map(|m| (meta, m)))
    }

    /// The group that lists `path`, with its section.
    #[must_use]
    pub fn group_of(&self, path: &SettingPath) -> Option<(&SectionMeta, &GroupMeta)> {
        self.sections.iter().find_map(|section| {
            section
                .groups
                .iter()
                .find(|group| group.settings.iter().any(|p| p.matches(path).is_some()))
                .map(|group| (section, group))
        })
    }

    /// Whether the setting at `path` is shown while editing `value`: it has
    /// an entry that isn't hidden, and its own and its group's conditions
    /// hold. Settings no section lists are not shown.
    #[must_use]
    pub fn is_shown(&self, path: &SettingPath, value: &Value) -> bool {
        let Some((meta, m)) = self.setting(path) else {
            return false;
        };
        let Some((_, group)) = self.group_of(path) else {
            return false;
        };
        !meta.hidden
            && [&meta.shown_if, &group.shown_if]
                .into_iter()
                .flatten()
                .all(|c| c.holds(value, m.index, self))
    }

    /// The part the setting at `path` targets: its own, or its section's.
    #[must_use]
    pub fn part_of(&self, path: &SettingPath) -> Option<&str> {
        let own = self
            .setting(path)
            .and_then(|(meta, _)| meta.part.as_deref());
        own.or_else(|| self.group_of(path).and_then(|(s, _)| s.part.as_deref()))
    }

    /// The Inspector's layout for `value`: each section's groups with the
    /// concrete settings they list, in display order. Patterns are expanded
    /// over `value`; listed settings with no entry or not in `value` are
    /// left out. Hidden settings are kept, marked not shown.
    #[must_use]
    pub fn layout(&self, value: &Value) -> Vec<SectionLayout<'_>> {
        self.sections
            .iter()
            .map(|section| SectionLayout {
                section,
                groups: section
                    .groups
                    .iter()
                    .map(|group| GroupLayout {
                        group,
                        settings: group
                            .settings
                            .iter()
                            .flat_map(|pattern| pattern.expand(value))
                            .filter_map(|path| {
                                let (meta, m) = self.setting(&path)?;
                                Some(SettingLayout {
                                    shown: self.is_shown(&path, value),
                                    part: meta.part.as_deref().or(section.part.as_deref()),
                                    path,
                                    meta,
                                    index: m.index,
                                })
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect()
    }

    /// Gives usable values to number settings that an edit has just made
    /// appear.
    ///
    /// Call it after a user edit that changed `before` into `after`. Every
    /// listed number setting that was not shown in `before`, is shown in
    /// `after`, sits at zero or the bottom of its range there, and has a
    /// [`Revive`] value, takes that value. Settings that appear because of a
    /// revived value are revived in turn. Returns whether anything changed.
    pub fn revive(&self, before: &Value, after: &mut Value) -> bool {
        let candidates: Vec<(SettingPath, &SettingMeta, Option<usize>)> = self
            .layout(after)
            .into_iter()
            .flat_map(|s| s.groups)
            .flat_map(|g| g.settings)
            .filter(|s| s.meta.revive.is_some() && !self.is_shown(&s.path, before))
            .map(|s| (s.path, s.meta, s.index))
            .collect();
        let mut changed = false;

        // Each pass may reveal more settings; revived settings are no longer
        // dead, so there are at most as many passes as settings.
        for _ in 0..=candidates.len() {
            let mut revived = false;
            for (path, meta, index) in &candidates {
                let Some(current) = path.get(after).and_then(Value::as_f64) else {
                    continue;
                };
                let dead = current == 0.0 || meta.range.is_some_and(|[min, _]| current == min);
                if !dead || !self.is_shown(path, after) {
                    continue;
                }
                let Some(new) = meta.revive_value(after, *index) else {
                    continue;
                };
                let slot = path.get(after).cloned().unwrap_or(Value::Null);
                path.set(after, write_number(&slot, new));
                revived |= new != 0.0 && meta.range.is_none_or(|[min, _]| new != min);
            }
            if !revived {
                break;
            }
            changed = true;
        }
        changed
    }
}

impl NamedConditions for KindMeta {
    fn named(&self, name: &str) -> Option<&Condition> {
        self.conditions
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, c)| c)
    }
}

/// A section of [`KindMeta::layout`].
#[derive(Debug)]
pub struct SectionLayout<'a> {
    /// The section as the file describes it.
    pub section: &'a SectionMeta,
    /// Its groups, with their concrete settings.
    pub groups: Vec<GroupLayout<'a>>,
}

/// A group of [`KindMeta::layout`].
#[derive(Debug)]
pub struct GroupLayout<'a> {
    /// The group as the file describes it.
    pub group: &'a GroupMeta,
    /// Its concrete settings, in display order.
    pub settings: Vec<SettingLayout<'a>>,
}

/// One concrete setting of [`KindMeta::layout`].
#[derive(Debug)]
pub struct SettingLayout<'a> {
    /// Where the setting is in the value.
    pub path: SettingPath,
    /// Its entry in the metadata file.
    pub meta: &'a SettingMeta,
    /// The index its pattern's `*` matched, if any.
    pub index: Option<usize>,
    /// The part it targets, if any.
    pub part: Option<&'a str>,
    /// Whether it is shown for the value the layout was made for.
    pub shown: bool,
}

fn setting(raw: RawSetting, at: &str) -> Result<SettingMeta, MetaError> {
    // At most one control override.
    let overrides = usize::from(raw.color)
        + usize::from(raw.choices.is_some())
        + usize::from(raw.reference.is_some());
    if overrides > 1 {
        return Err(error(format!(
            "{at}: give at most one of color, choices and ref"
        )));
    }
    let control = match (raw.color, raw.choices, raw.reference) {
        (true, _, _) => ControlKind::Color,
        (_, Some(RawChoices::Named(name)), _) => ControlKind::Choice(Choices::Named(name)),
        (_, Some(RawChoices::Listed(list)), _) => ControlKind::Choice(Choices::Listed(list)),
        (_, _, Some(kind)) => ControlKind::Ref(kind),
        _ => ControlKind::FromValue,
    };
    if let Some([min, max]) = raw.range
        && min > max
    {
        return Err(error(format!("{at}.range: {min} is above {max}")));
    }
    let revive = match raw.revive {
        None => None,
        Some(RawRevive::Value(v)) => Some(Revive::Value(v)),
        Some(RawRevive::From(from)) => {
            Template::parse(&from.from).map_err(|m| error(format!("{at}.revive: {m}")))?;
            Some(Revive::From {
                from: from.from,
                times: from.times,
                min: from.min,
                if_zero: from.if_zero,
            })
        }
    };
    Ok(SettingMeta {
        label: raw.label,
        description: raw.description,
        unit: raw.unit,
        range: raw.range,
        step: raw.step,
        scale: raw.scale,
        control,
        part: raw.part,
        shown_if: raw
            .shown_if
            .as_deref()
            .map(|text| condition(text, &format!("{at}.shown_if")))
            .transpose()?,
        revive,
        advanced: raw.advanced,
        hidden: raw.hidden,
    })
}

impl SettingMeta {
    /// The options of a choice, looking named lists up in `lists`. `None`
    /// for settings that aren't choices, or name a list `lists` lacks.
    #[must_use]
    pub fn choices<'a>(&'a self, lists: &'a ChoiceLists) -> Option<&'a [Choice]> {
        match &self.control {
            ControlKind::Choice(Choices::Listed(list)) => Some(list),
            ControlKind::Choice(Choices::Named(name)) => lists.get(name),
            _ => None,
        }
    }

    /// The value this setting's [`Revive`] gives in `value`, with `{index}`
    /// filled from `index`.
    #[must_use]
    pub fn revive_value(&self, value: &Value, index: Option<usize>) -> Option<f64> {
        match self.revive.as_ref()? {
            Revive::Value(v) => Some(*v),
            Revive::From {
                from,
                times,
                min,
                if_zero,
            } => {
                let path = Template::parse(from).ok()?.path(index)?;
                let source = path.get(value).and_then(Value::as_f64).unwrap_or(0.0);
                match (source, if_zero) {
                    (0.0, Some(fallback)) => Some(*fallback),
                    _ => Some(min.map_or(source * times, |m| (source * times).max(m))),
                }
            }
        }
    }
}

/// The value to store when a number control sets `new` on a setting that
/// holds `current`: a whole number, rounded, when `current` is one (so
/// whole-number options still read), otherwise `new`.
#[must_use]
pub fn write_number(current: &Value, new: f64) -> Value {
    match current {
        Value::Number(n) if n.is_u64() && new >= 0.0 => Value::from(new.round() as u64),
        Value::Number(n) if n.is_u64() || n.is_i64() => Value::from(new.round() as i64),
        _ => Value::from(new),
    }
}

/// The index of `current` among `choices`: the equal option, or for a map
/// with a `kind`, the option of the same kind. `None` when none fits.
#[must_use]
pub fn choice_index(choices: &[Choice], current: &Value) -> Option<usize> {
    choices
        .iter()
        .position(|c| c.value == *current)
        .or_else(|| {
            let kind = current.get("kind")?;
            choices
                .iter()
                .position(|c| c.value.get("kind") == Some(kind))
        })
}

/// The value to store when `choice` is picked for a setting that holds
/// `current`: `current` itself when it is already that option, or a map of
/// the same `kind` (keeping that kind's settings), otherwise the option's
/// value.
#[must_use]
pub fn pick_choice(choice: &Choice, current: &Value) -> Value {
    let same_kind =
        current.get("kind").is_some() && current.get("kind") == choice.value.get("kind");
    if same_kind || choice.value == *current {
        current.clone()
    } else {
        choice.value.clone()
    }
}
