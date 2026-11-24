//! A preset file split into its parts.

use serde_json::{Map, Value};

use crate::PresetFileError;
use crate::merge::to_yaml;

/// The group a preset read from a file is listed under when the file
/// gives none.
pub const FILE_GROUP: &str = "File";

/// The keys a preset file has besides its kind.
const META_KEYS: [&str; 4] = ["base", "name", "description", "group"];

/// A preset file's parts, before its settings are laid over a base.
///
/// Build one with [`Document::parse`], or by hand to write a file with
/// [`Document::to_yaml`]. Every part may be missing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    /// The key of the built-in preset the settings are laid over.
    pub base: Option<String>,
    /// The preset's display name.
    pub name: Option<String>,
    /// What the preset looks like, in a sentence or two.
    pub description: Option<String>,
    /// The group the preset is listed under.
    pub group: Option<String>,
    /// The settings under the file's kind: a map, or `None` when the file
    /// changes nothing.
    pub settings: Option<Value>,
}

impl Document {
    /// Splits the text of a `.<kind>.yml` file into its parts.
    ///
    /// `kind` is the key the settings sit under, such as `"tree"`. An
    /// empty file is a document with every part missing.
    ///
    /// # Errors
    ///
    /// Returns [`PresetFileError::Yaml`] for text that isn't YAML,
    /// [`PresetFileError::NotAMap`] when the file or its settings aren't
    /// maps, [`PresetFileError::UnknownKey`] for a top-level key other than
    /// `base`, `name`, `description`, `group` and `kind`, and
    /// [`PresetFileError::Setting`] when one of the first four isn't text.
    pub fn parse(kind: &str, text: &str) -> Result<Self, PresetFileError> {
        let value: Value = serde_saphyr::from_str(text).map_err(|error| PresetFileError::Yaml {
            message: error.to_string(),
        })?;
        let mut map = match value {
            Value::Null => Map::new(),
            Value::Object(map) => map,
            _ => {
                return Err(PresetFileError::NotAMap {
                    path: "file".to_owned(),
                });
            }
        };

        // Every top-level key must be known.
        if let Some(key) = map
            .keys()
            .find(|key| key.as_str() != kind && !META_KEYS.contains(&key.as_str()))
        {
            let mut expected: Vec<String> = META_KEYS.iter().map(|key| (*key).to_owned()).collect();
            expected.push(kind.to_owned());
            return Err(PresetFileError::UnknownKey {
                key: key.clone(),
                expected,
            });
        }

        // Split off the text parts, then the settings.
        let mut text_part = |key: &str| match map.remove(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(text)) => Ok(Some(text)),
            Some(_) => Err(PresetFileError::Setting {
                path: key.to_owned(),
                message: "must be text".to_owned(),
            }),
        };
        let base = text_part("base")?;
        let name = text_part("name")?;
        let description = text_part("description")?;
        let group = text_part("group")?;
        let settings = match map.remove(kind) {
            None | Some(Value::Null) => None,
            Some(settings @ Value::Object(_)) => Some(settings),
            Some(_) => {
                return Err(PresetFileError::NotAMap {
                    path: kind.to_owned(),
                });
            }
        };
        Ok(Self {
            base,
            name,
            description,
            group,
            settings,
        })
    }

    /// Writes the document as the text of a `.<kind>.yml` file, which
    /// [`parse`](Self::parse) reads back to an equal document.
    ///
    /// Missing parts are left out.
    #[must_use]
    pub fn to_yaml(&self, kind: &str) -> String {
        let mut map = Map::new();
        let parts = [
            ("base", &self.base),
            ("name", &self.name),
            ("description", &self.description),
            ("group", &self.group),
        ];
        for (key, part) in parts {
            if let Some(text) = part {
                map.insert(key.to_owned(), Value::String(text.clone()));
            }
        }
        if let Some(settings) = &self.settings {
            map.insert(kind.to_owned(), settings.clone());
        }
        if map.is_empty() {
            return String::new();
        }
        to_yaml(&Value::Object(map))
    }
}
