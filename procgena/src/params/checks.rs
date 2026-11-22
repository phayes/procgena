//! Shared tests for editor controls and the settings they change.
//!
//! Checks ranges, choices and visibility for plant settings, field settings
//! and each plant's weight and clustering controls.

use std::collections::HashSet;

use bevy::reflect::{GetPath, PartialReflect};

use super::spec::{Control, Param};

/// Creates edited copies to exercise a control's supported values.
///
/// Uses both numeric limits, every choice, the opposite toggle state, or
/// two sample colours, depending on the control type.
pub fn variants<T: Clone>(param: &Param<T>, value: &T) -> Vec<T> {
    let with = |edit: &dyn Fn(&mut T)| {
        let mut changed = value.clone();
        edit(&mut changed);
        changed
    };
    match &param.control {
        Control::Number { min, max, set, .. } => {
            vec![with(&|v| set(v, *min)), with(&|v| set(v, *max))]
        }
        Control::Toggle { get, set } => vec![with(&|v| set(v, !get(value)))],
        Control::Color { set, .. } => {
            vec![with(&|v| set(v, 0x123456)), with(&|v| set(v, 0xfedcba))]
        }
        Control::Choice { options, set, .. } => {
            (0..options.len()).map(|i| with(&|v| set(v, i))).collect()
        }
    }
}

/// Panics if two settings share a key.
pub fn keys_are_unique<T>(params: &[Param<T>]) {
    let mut seen = HashSet::new();
    for param in params {
        assert!(
            seen.insert(param.key.clone()),
            "duplicate key {}",
            param.key
        );
    }
}

/// Checks that each control changes only the setting named by its key.
///
/// Panics if a control changes another setting or has no effect in any case.
pub fn settings_write_their_keys<T>(params: &[Param<T>], cases: &[(String, T)])
where
    T: Clone + PartialEq + std::fmt::Debug + GetPath + PartialReflect,
{
    for param in params {
        let key = &param.key;
        let mut changes_something = false;
        for (name, value) in cases {
            for changed in variants(param, value) {
                // Copying only the field at `key` must give the changed value.
                let field = changed
                    .reflect_path(key.as_str())
                    .unwrap_or_else(|e| panic!("{key} is not a field path: {e}"));
                let mut copied = value.clone();
                let target = copied.reflect_path_mut(key.as_str()).unwrap();
                target.apply(field);
                assert_eq!(&copied, &changed, "{name}: {key} writes another field");
                changes_something |= changed != *value;
            }
        }
        assert!(changes_something, "{key} never changes the value");
    }
}

/// Checks that control edits produce values accepted by `check`.
///
/// Panics for rejected numeric limits, choices, toggle changes or sample colours.
pub fn range_ends_are_valid<T: Clone>(
    params: &[Param<T>],
    cases: &[(String, T)],
    check: impl Fn(T) -> Result<(), String>,
) {
    for (name, value) in cases {
        for param in params {
            for changed in variants(param, value) {
                if let Err(error) = check(changed) {
                    panic!("{name}: {}: {error}", param.key);
                }
            }
        }
    }
}

/// Checks that dependent controls follow the controls that reveal them.
///
/// Panics if a setting's visibility depends on a setting listed below it.
pub fn settings_follow_what_shows_them<T: Clone>(params: &[Param<T>], cases: &[(String, T)]) {
    for (name, value) in cases {
        for (index, master) in params.iter().enumerate() {
            for changed in variants(master, value) {
                // Any setting whose visibility follows `master` must be below it.
                let dependents = params.iter().enumerate().filter(|(_, dependent)| {
                    dependent.is_shown(value) != dependent.is_shown(&changed)
                });
                for (dependent_index, dependent) in dependents {
                    assert!(
                        dependent_index > index,
                        "{name}: {} is shown or hidden by {}, which comes after it",
                        dependent.key,
                        master.key
                    );
                }
            }
        }
    }
}

/// Panics if changing a shown setting hides it.
pub fn no_setting_hides_itself<T: Clone>(params: &[Param<T>], cases: &[(String, T)]) {
    for (name, value) in cases {
        for param in params.iter().filter(|p| p.is_shown(value)) {
            for changed in variants(param, value) {
                assert!(
                    param.is_shown(&changed),
                    "{name}: changing {} hides it",
                    param.key
                );
            }
        }
    }
}

/// Checks that each test value can be represented by its controls.
///
/// Reports all out-of-range numbers and unsupported choices in one panic.
pub fn cases_fit_the_controls<T: Clone + PartialEq>(params: &[Param<T>], cases: &[(String, T)]) {
    let mut problems = Vec::new();
    for (name, value) in cases {
        for param in params {
            let key = &param.key;
            match &param.control {
                Control::Number { min, max, get, .. } => {
                    let number = get(value);
                    if !(*min..=*max).contains(&number) {
                        problems.push(format!("{name}: {key} is {number}, outside {min}..={max}"));
                    }
                }
                // A value missing from the list would show as the first option.
                Control::Choice { get, set, .. } => {
                    let mut shown = value.clone();
                    set(&mut shown, get(value));
                    if shown != *value {
                        problems.push(format!("{name}: {key} is not one of the choices"));
                    }
                }
                Control::Toggle { .. } | Control::Color { .. } => {}
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
