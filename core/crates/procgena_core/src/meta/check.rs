//! Keeping a metadata file in step with its options types.

use std::collections::BTreeSet;

use serde_json::Value;

use super::condition::{Condition, NamedConditions};
use super::file::{
    ChoiceLists, ControlKind, KindMeta, Revive, SettingMeta, choice_index, pick_choice,
};
use super::path::{SettingPath, Template, all_paths, leaves};

/// Finds every way `meta` is out of step with the settings it describes,
/// as one message each. An empty list means it is in step.
///
/// `samples` are named settings trees of the kind, as preset files write
/// them: the options' defaults and every built-in preset, plus any value a
/// crate adds so that settings no preset uses (such as an enum variant)
/// appear. Each choice's options that are maps are also tried in place, so
/// a variant listed in a choice needs no sample of its own. `lists` are the
/// crate's named choice lists, and `kinds` every preset kind a `ref` may
/// name.
///
/// Reports:
/// - a setting in a sample with no entry (an entry covers its path and
///   everything below it);
/// - an entry that matches no setting in any sample;
/// - a shown entry that no section lists or that has no description, and a
///   listed path that matches no entry;
/// - a condition or revive that reads a setting no sample has, or names an
///   unknown condition;
/// - an unknown `ref` kind, choice list or part, and a part no setting
///   targets;
/// - a sample's number outside its entry's range, or a value that is not
///   one of its entry's choices.
#[must_use]
pub fn problems(
    meta: &KindMeta,
    samples: &[(String, Value)],
    lists: &ChoiceLists,
    kinds: &[&str],
) -> Vec<String> {
    let mut found = BTreeSet::new();
    let trees = with_choices(meta, samples, lists);
    let paths: BTreeSet<SettingPath> = trees.iter().flat_map(|(_, v)| all_paths(v)).collect();

    // Every setting has an entry, through itself or a parent.
    for path in trees.iter().flat_map(|(_, v)| leaves(v)) {
        let covered = (0..=path.segments().len()).any(|n| {
            let prefix = SettingPath::parse(&path.segments()[..n].join("."));
            !prefix.segments().is_empty() && meta.setting(&prefix).is_some()
        });
        if !covered && !path.segments().is_empty() {
            found.insert(format!("{path} has no entry"));
        }
    }

    // Every entry matches a setting and is listed unless hidden; every
    // named list, ref kind and part exists.
    let part_known = |part: &str| meta.part(part).is_some();
    for (pattern, entry) in meta.entries() {
        let matched: Vec<&SettingPath> = paths
            .iter()
            .filter(|p| {
                pattern.matches(p).is_some()
                    && meta.setting(p).is_some_and(|(e, _)| std::ptr::eq(e, entry))
            })
            .collect();
        if matched.is_empty() {
            found.insert(format!("entry {pattern} matches no setting"));
        }
        if !entry.hidden && matched.iter().any(|p| meta.group_of(p).is_none()) {
            found.insert(format!("entry {pattern} is in no section"));
        }
        if !entry.hidden && entry.description.trim().is_empty() {
            found.insert(format!("entry {pattern} has no description"));
        }
        match &entry.control {
            ControlKind::Choice(_) if entry.choices(lists).is_none() => {
                found.insert(format!("entry {pattern} names an unknown choice list"));
            }
            ControlKind::Ref(kind) if !kinds.contains(&kind.as_str()) => {
                found.insert(format!("entry {pattern} refers to unknown kind {kind}"));
            }
            _ => {}
        }
        if let Some(part) = &entry.part
            && !part_known(part)
        {
            found.insert(format!("entry {pattern} names unknown part {part}"));
        }
    }

    // Sections list settings that exist, and name known parts.
    for section in &meta.sections {
        if let Some(part) = &section.part
            && !part_known(part)
        {
            found.insert(format!(
                "section {} names unknown part {part}",
                section.title
            ));
        }
        for pattern in section.groups.iter().flat_map(|g| &g.settings) {
            if !paths
                .iter()
                .any(|p| pattern.matches(p).is_some() && meta.setting(p).is_some())
            {
                found.insert(format!(
                    "section {} lists {pattern}, which matches no setting",
                    section.title
                ));
            }
        }
    }

    // Conditions and revives read settings that exist.
    for path in &paths {
        let Some((entry, m)) = meta.setting(path) else {
            continue;
        };
        let group_if = meta.group_of(path).and_then(|(_, g)| g.shown_if.as_ref());
        for condition in [entry.shown_if.as_ref(), group_if].into_iter().flatten() {
            check_condition(meta, condition, m.index, &paths, path, &mut found, 0);
        }
        if let Some(Revive::From { from, .. }) = &entry.revive {
            let source = Template::parse(from).ok().and_then(|t| t.path(m.index));
            if source.is_some_and(|s| !paths.contains(&s)) {
                found.insert(format!("{path}: revive reads {from}, which no sample has"));
            }
        }
    }

    // Every part has a setting.
    for (key, _) in &meta.parts {
        if !paths.iter().any(|p| meta.part_of(p) == Some(key.as_str())) {
            found.insert(format!("part {key} has no setting"));
        }
    }

    // Built-in values fit their ranges.
    for (name, value) in samples {
        for path in leaves(value) {
            let Some((entry, _)) = meta.setting(&path) else {
                continue;
            };
            if let (Some([min, max]), Some(v)) =
                (entry.range, path.get(value).and_then(Value::as_f64))
                && !(min..=max).contains(&v)
            {
                found.insert(format!("{name}: {path} = {v} is outside {min}..={max}"));
            }
        }

        // ...and their choices, which would otherwise show as another option.
        for path in all_paths(value) {
            let Some((entry, _)) = meta.setting(&path) else {
                continue;
            };
            let (Some(choices), Some(current)) = (entry.choices(lists), path.get(value)) else {
                continue;
            };
            if choice_index(choices, current).is_none() {
                found.insert(format!(
                    "{name}: {path} = {current} is not one of its choices"
                ));
            }
        }
    }
    found.into_iter().collect()
}

/// Panics, listing every problem [`problems`] finds, unless `meta` is in
/// step with `samples`. Call it from one test in each generator crate.
///
/// # Panics
///
/// Panics when [`problems`] reports anything.
pub fn assert_in_step(
    meta: &KindMeta,
    samples: &[(String, Value)],
    lists: &ChoiceLists,
    kinds: &[&str],
) {
    let found = problems(meta, samples, lists, kinds);
    assert!(
        found.is_empty(),
        "{}.meta.yml is out of step with its options:\n  {}",
        meta.kind,
        found.join("\n  ")
    );
}

fn check_condition(
    meta: &KindMeta,
    condition: &Condition,
    index: Option<usize>,
    paths: &BTreeSet<SettingPath>,
    at: &SettingPath,
    found: &mut BTreeSet<String>,
    depth: usize,
) {
    for read in condition.paths(index) {
        if !paths.contains(&read) {
            found.insert(format!(
                "{at}: a condition reads {read}, which no sample has"
            ));
        }
    }
    for name in condition.names(index) {
        match meta.named(&name) {
            None => {
                found.insert(format!("{at}: a condition names unknown condition @{name}"));
            }
            Some(named) if depth < 32 => {
                check_condition(meta, named, index, paths, at, found, depth + 1);
            }
            Some(_) => {
                found.insert(format!("{at}: condition @{name} refers to itself"));
            }
        }
    }
}

/// `samples`, plus each sample with each map-valued choice option put in
/// place, once per setting and option, so that every variant's settings
/// appear.
fn with_choices(
    meta: &KindMeta,
    samples: &[(String, Value)],
    lists: &ChoiceLists,
) -> Vec<(String, Value)> {
    let mut trees = samples.to_vec();
    let mut tried = BTreeSet::new();
    for (name, sample) in samples {
        for path in all_paths(sample) {
            let Some((entry, _)) = meta.setting(&path) else {
                continue;
            };
            for (i, choice) in entry.choices(lists).into_iter().flatten().enumerate() {
                if choice.value.is_object() && tried.insert((path.clone(), i)) {
                    let mut tree = sample.clone();
                    path.set(&mut tree, choice.value.clone());
                    trees.push((format!("{name} with {path} = {}", choice.label), tree));
                }
            }
        }
    }
    trees
}

/// Copies of `value` with the setting at `path` changed to each kind of
/// value its control offers: both range ends of a number, the other state
/// of a switch, every option of a choice, or two sample colours. Settings
/// with no entry, references and text give none.
///
/// Tests use these to try every control's reach, such as checking that
/// each range end still builds an asset.
#[must_use]
pub fn variants(
    meta: &KindMeta,
    lists: &ChoiceLists,
    path: &SettingPath,
    value: &Value,
) -> Vec<Value> {
    let (Some((entry, _)), Some(current)) = (meta.setting(path), path.get(value)) else {
        return Vec::new();
    };
    let with = |new: Value| {
        let mut changed = value.clone();
        path.set(&mut changed, new);
        changed
    };
    setting_values(entry, lists, current)
        .into_iter()
        .map(with)
        .collect()
}

fn setting_values(entry: &SettingMeta, lists: &ChoiceLists, current: &Value) -> Vec<Value> {
    match &entry.control {
        ControlKind::Choice(_) => entry
            .choices(lists)
            .into_iter()
            .flatten()
            .map(|choice| pick_choice(choice, current))
            .collect(),
        ControlKind::Color => match current {
            Value::Number(_) => vec![Value::from(0x12_3456), Value::from(0xfe_dcba)],
            Value::String(_) => vec![Value::from("#123456"), Value::from("#fedcba")],
            _ => vec![
                serde_json::json!([0.1, 0.2, 0.3]),
                serde_json::json!([0.9, 0.8, 0.7]),
            ],
        },
        ControlKind::Ref(_) => Vec::new(),
        ControlKind::FromValue => {
            let number = current.is_number() || (current.is_null() && entry.optional.is_some());
            let mut values = match current {
                Value::Bool(on) => vec![Value::Bool(!on)],
                _ if number => entry
                    .range
                    .map(|[min, max]| {
                        vec![
                            entry.number_value(current, min),
                            entry.number_value(current, max),
                        ]
                    })
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            // An optional number can also be unset.
            if entry.optional.is_some() && !current.is_null() {
                values.push(Value::Null);
            }
            values
        }
    }
}
