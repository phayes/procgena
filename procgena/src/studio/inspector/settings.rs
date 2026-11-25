//! A preset's settings as the Inspector edits them: the settings tree the
//! preset file writes, with the typed options read back from it.

use bevy::log::error;
use procgena_core::meta::{Edit, SettingPath};
use procgena_core::preset::{from_value, to_value};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// The settings of one preset: the settings tree as its file writes it,
/// and the typed options `T` read from that tree.
///
/// Every change goes through [`Settings::set`], which records an
/// [`Edit`] and reads the typed options again. Two `Settings` are equal
/// when their trees are.
#[derive(Debug, Clone)]
pub struct Settings<T> {
    value: Value,
    options: T,
    edits: Vec<Edit>,
}

impl<T: Serialize + DeserializeOwned> Settings<T> {
    /// The settings of `options`, as their file writes them.
    pub fn new(options: T) -> Self {
        Self {
            value: to_value(&options),
            options,
            edits: Vec::new(),
        }
    }

    /// Puts `new` at `path` and reads the typed options again.
    ///
    /// Changes nothing, and records nothing, when `path` holds `new`
    /// already, can't be set (see [`SettingPath::set`]), or the changed tree
    /// no longer reads as a `T`. The last is a mistake in the kind's
    /// metadata: it is logged.
    pub fn set(&mut self, path: &SettingPath, new: Value) {
        let mut value = self.value.clone();
        let Some(edit) = Edit::apply(&mut value, path.clone(), new) else {
            return;
        };
        match from_value::<T>(value.clone(), "") {
            Ok(options) => {
                self.value = value;
                self.options = options;
                self.edits.push(edit);
            }
            Err(error) => error!("the Inspector can't set {path}: {error}"),
        }
    }

    /// The edits made since the last call, oldest first.
    pub fn take_edits(&mut self) -> Vec<Edit> {
        std::mem::take(&mut self.edits)
    }
}

impl<T> Settings<T> {
    /// The typed options.
    pub fn options(&self) -> &T {
        &self.options
    }

    /// The settings tree, as the preset file writes it.
    pub fn value(&self) -> &Value {
        &self.value
    }
}

impl<T> PartialEq for Settings<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}
