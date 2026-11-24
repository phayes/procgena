//! Laying a file's settings over a base, and finding what differs.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::PresetFileError;

/// The key whose value tells which kind of a setting a group holds.
const KIND_KEY: &str = "kind";

/// Lays `diff`'s settings over `base`, the way preset files do.
///
/// - Maps merge key by key, recursively.
/// - Anything else in `diff` (a number, text, a list or `null`) replaces the
///   base's value whole.
/// - A map whose `kind` differs from the base map's `kind` replaces the base
///   map whole.
///
/// Keys in `diff` that `base` doesn't have are added; reading the merged
/// value with [`from_value`] reports them if the options don't know them.
pub fn merge(base: &mut Value, diff: Value) {
    match (base, diff) {
        (Value::Object(base), Value::Object(diff)) if !changes_kind(base, &diff) => {
            for (key, value) in diff {
                match base.get_mut(&key) {
                    Some(slot) => merge(slot, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, diff) => *base = diff,
    }
}

/// Whether both maps name a `kind`, and the kinds differ.
fn changes_kind(
    base: &serde_json::Map<String, Value>,
    diff: &serde_json::Map<String, Value>,
) -> bool {
    match (base.get(KIND_KEY), diff.get(KIND_KEY)) {
        (Some(old), Some(new)) => old != new,
        _ => false,
    }
}

/// What `value` changes from `base`, as the smallest value that [`merge`]
/// lays over `base` to give `value` back. `None` when they are equal.
///
/// Maps are compared key by key. Anything else that differs, a list
/// included, is given whole. A map whose `kind` differs is given whole.
/// Keys that `base` has and `value` lacks are not reported.
#[must_use]
pub fn diff(base: &Value, value: &Value) -> Option<Value> {
    match (base, value) {
        (Value::Object(base), Value::Object(value)) if !changes_kind(base, value) => {
            let changed: serde_json::Map<String, Value> = value
                .iter()
                .filter_map(|(key, value)| {
                    let change = match base.get(key) {
                        Some(base) => diff(base, value)?,
                        None => value.clone(),
                    };
                    Some((key.clone(), change))
                })
                .collect();
            (!changed.is_empty()).then_some(Value::Object(changed))
        }
        (base, value) => (base != value).then(|| value.clone()),
    }
}

/// Writes `value` as a settings tree that [`merge`] and [`diff`] work on.
///
/// Numbers keep the shortest form their own type writes, so an `f32` of
/// 0.6 stays `0.6` rather than becoming 0.6000000238418579.
///
/// # Panics
///
/// Panics if `value` can't be written as YAML, such as a map with keys that
/// aren't text or numbers. Options types never do this.
#[must_use]
pub fn to_value<T: Serialize>(value: &T) -> Value {
    // Going through YAML text keeps each number in its own type's shortest
    // form, which a direct conversion to `Value` would widen to `f64`.
    let text = serde_saphyr::to_string(value).expect("options always write as YAML");
    serde_saphyr::from_str(&text).expect("written YAML always reads back")
}

/// Reads options from a settings tree, naming the path of any setting that
/// is unknown, missing or of the wrong type.
///
/// `prefix` is put in front of every path, such as `"tree"` for the
/// settings under a file's `tree` key. Use `""` for none.
///
/// # Errors
///
/// Returns [`PresetFileError::Setting`] when `value` doesn't match `T`.
pub fn from_value<T: DeserializeOwned>(value: Value, prefix: &str) -> Result<T, PresetFileError> {
    serde_path_to_error::deserialize(value).map_err(|error| {
        let inner = error.path().to_string();
        let path = match (prefix.is_empty(), inner.as_str()) {
            (true, _) => inner.clone(),
            (false, ".") => prefix.to_owned(),
            (false, _) => format!("{prefix}.{inner}"),
        };
        PresetFileError::Setting {
            path,
            message: error.into_inner().to_string(),
        }
    })
}

/// Writes a settings tree as YAML text.
///
/// # Panics
///
/// Panics if `value` holds a number YAML can't write. Values built with
/// [`to_value`], [`merge`] and [`diff`] never do.
#[must_use]
pub fn to_yaml(value: &Value) -> String {
    serde_saphyr::to_string(value).expect("a settings tree always writes as YAML")
}
