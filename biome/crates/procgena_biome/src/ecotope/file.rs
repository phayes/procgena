//! How an Ecotope's Scatters are written in a `.ecotope.yml` file.
//!
//! Scatters are a map from key to settings, so a file built on a `base`
//! changes, adds or replaces one Scatter without restating the others. Each
//! Scatter names its `kind`, and needs to give only the settings that
//! differ from that kind's defaults; written files give every setting of a
//! Scatter they write, so differences over a `base` stay exact. Assets
//! inside it (a stone class's `stone`, dead
//! wood's `piece`, a mushroom Scatter's `mushroom`) name a built-in preset
//! by key, or give settings laid over one (`{ base: <key>, ... }`) or over
//! the asset's defaults.

use std::collections::BTreeMap;

use procgena_core::ChildKey;
use procgena_core::preset::{Preset, diff, from_value, merge, to_value};
use procgena_deadwood::{DeadwoodPreset, ScatterOptions as DeadwoodScatterOptions};
use procgena_mushroom::{
    MushroomOptions, MushroomPreset, ScatterOptions as MushroomScatterOptions,
};
use procgena_stone::{StoneClass, StoneOptions, StonePreset, StoneScatterOptions};
use procgena_tree::TreeScatter;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use super::EcotopeScatter;
use super::scatters::ScatterKind;

/// Reads and writes an Ecotope's map of Scatters with [`ChildKey`]s as keys.
pub(crate) mod keyed {
    use super::*;

    pub(crate) fn serialize<S: Serializer>(
        scatters: &BTreeMap<ChildKey, EcotopeScatter>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.collect_map(
            scatters
                .iter()
                .map(|(key, scatter)| (key.as_str(), scatter)),
        )
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<ChildKey, EcotopeScatter>, D::Error> {
        let raw = BTreeMap::<String, EcotopeScatter>::deserialize(deserializer)?;
        raw.into_iter()
            .map(|(key, scatter)| match ChildKey::new(key.as_str()) {
                Ok(key) => Ok((key, scatter)),
                Err(error) => Err(D::Error::custom(format!("Scatter key {key:?}: {error}"))),
            })
            .collect()
    }
}

impl Serialize for EcotopeScatter {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        to_file(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for EcotopeScatter {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        from_file(Value::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// The settings a dead wood Scatter's left-out settings come from.
fn deadwood_default() -> DeadwoodScatterOptions {
    DeadwoodScatterOptions::fallen_logs()
}

/// The settings a stone Scatter's left-out settings come from: no classes.
fn stone_default() -> StoneScatterOptions {
    StoneScatterOptions {
        default_seed: 0,
        classes: Vec::new(),
    }
}

/// The settings a stone class's left-out settings come from, its `key`
/// aside: every class names its own key.
fn class_default() -> StoneClass {
    StoneClass::new(
        ChildKey::from_static("class"),
        StoneOptions::default(),
        0.01,
        2.0,
    )
}

/// `value` without its `key`, so a class laid over it, or written against
/// it, always gives its own key.
fn without_key(mut value: Value) -> Value {
    if let Value::Object(map) = &mut value {
        map.remove("key");
    }
    value
}

/// Reads one Scatter from its file settings.
fn from_file(value: Value) -> Result<EcotopeScatter, String> {
    let Value::Object(mut map) = value else {
        return Err("a Scatter must be a map of settings".to_owned());
    };

    // The settings every kind shares.
    let priority = match map.remove("priority") {
        Some(Value::Number(n)) => n
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or("priority must be a whole number from 0 to 4294967295")?,
        Some(_) => return Err("priority must be a whole number".to_owned()),
        None => return Err("a Scatter must give its priority".to_owned()),
    };
    let clearance = match map.remove("clearance") {
        Some(Value::Number(n)) => n.as_f64().ok_or("clearance must be a number")? as f32,
        Some(_) => return Err("clearance must be a number".to_owned()),
        None => 0.0,
    };
    let kind = match map.remove("kind") {
        Some(Value::String(kind)) => kind,
        _ => {
            return Err(
                "a Scatter must name its kind: tree, stone, deadwood or mushroom".to_owned(),
            );
        }
    };

    // The kind's own settings, laid over its defaults.
    let scatter = match kind.as_str() {
        "tree" => ScatterKind::Tree(fill(&TreeScatter::default(), map)?),
        "stone" => {
            if let Some(Value::Array(classes)) = map.get_mut("classes") {
                let base = without_key(to_value(&class_default()));
                for (i, class) in classes.iter_mut().enumerate() {
                    let at = |e: String| format!("classes.{i}: {e}");
                    resolve::<StonePreset>(class, "stone").map_err(at)?;
                    let mut full = base.clone();
                    merge(&mut full, class.take());
                    *class = full;
                }
            }
            ScatterKind::Stone(fill(&stone_default(), map)?)
        }
        "deadwood" => {
            let mut settings = Value::Object(map);
            resolve::<DeadwoodPreset>(&mut settings, "piece")?;
            let Value::Object(map) = settings else {
                unreachable!("settings stay a map")
            };
            ScatterKind::Deadwood(fill(&deadwood_default(), map)?)
        }
        "mushroom" => {
            let mut species = Map::new();
            if let Some(mushroom) = map.remove("mushroom") {
                species.insert("mushroom".to_owned(), mushroom);
            }
            let mut species = Value::Object(species);
            resolve::<MushroomPreset>(&mut species, "mushroom")?;
            let mushroom = match species {
                Value::Object(mut species) => match species.remove("mushroom") {
                    Some(Value::Object(settings)) => fill(&MushroomOptions::default(), settings)
                        .map_err(|e| format!("mushroom.{e}"))?,
                    Some(_) => return Err("mushroom must be a preset key or a map".to_owned()),
                    None => MushroomOptions::default(),
                },
                _ => unreachable!("species stays a map"),
            };
            ScatterKind::Mushroom {
                mushroom,
                scatter: fill(&MushroomScatterOptions::default(), map)?,
            }
        }
        other => {
            return Err(format!(
                "unknown kind {other:?}; a Scatter's kind is tree, stone, deadwood or mushroom"
            ));
        }
    };
    Ok(EcotopeScatter {
        priority,
        clearance,
        scatter,
    })
}

/// Writes one Scatter as its file settings, every setting of its kind
/// included, so a file written over a `base` also records a setting set
/// back to its kind's default.
fn to_file(scatter: &EcotopeScatter) -> Value {
    let mut map = Map::new();
    map.insert("priority".to_owned(), Value::from(scatter.priority));
    map.insert("clearance".to_owned(), to_value(&scatter.clearance));
    map.insert(
        "kind".to_owned(),
        Value::String(scatter.scatter.name().to_owned()),
    );

    // The kind's own settings.
    let settings = match &scatter.scatter {
        ScatterKind::Tree(options) => to_value(options),
        ScatterKind::Stone(options) => stone_file(options),
        ScatterKind::Deadwood(options) => deadwood_file(options),
        ScatterKind::Mushroom { mushroom, scatter } => {
            map.insert("mushroom".to_owned(), reference::<MushroomPreset>(mushroom));
            to_value(scatter)
        }
    };
    if let Value::Object(settings) = settings {
        map.extend(settings);
    }
    Value::Object(map)
}

/// A stone Scatter's settings, each class written as what differs from
/// [`class_default`] and its stone as a preset reference.
fn stone_file(options: &StoneScatterOptions) -> Value {
    let mut value = to_value(options);
    let base = without_key(class_file(&class_default()));
    let classes = options
        .classes
        .iter()
        .map(|class| diff(&base, &class_file(class)).unwrap_or_else(|| Value::Object(Map::new())))
        .collect();
    value["classes"] = Value::Array(classes);
    value
}

/// One stone class's settings, with its stone as a preset reference.
fn class_file(class: &StoneClass) -> Value {
    let mut value = to_value(class);
    value["stone"] = reference::<StonePreset>(&class.stone);
    value
}

/// A dead wood Scatter's settings, with its piece as a preset reference.
fn deadwood_file(options: &DeadwoodScatterOptions) -> Value {
    let mut value = to_value(options);
    value["piece"] = reference::<DeadwoodPreset>(&options.piece);
    value
}

/// Writes an asset's settings as `{ base: <key> }` when they equal a
/// built-in preset's, and in full otherwise.
fn reference<P: Preset>(options: &P::Options) -> Value
where
    P::Options: PartialEq,
{
    match P::all().find(|preset| preset.options() == options) {
        Some(preset) => {
            let mut map = Map::new();
            map.insert("base".to_owned(), Value::String(preset.key().to_owned()));
            Value::Object(map)
        }
        None => to_value(options),
    }
}

/// Replaces an asset reference at `settings[field]` with the full settings
/// it names: a preset key gives that preset's settings, and a map with a
/// `base` key lays the rest of the map over that preset's. A map without
/// `base` is left as it is, to be laid over the defaults later.
fn resolve<P: Preset>(settings: &mut Value, field: &str) -> Result<(), String> {
    let Some(slot) = settings.get_mut(field) else {
        return Ok(());
    };
    let preset = |key: &str| {
        P::named(key).ok_or_else(|| {
            let keys: Vec<&str> = P::all().map(Preset::key).collect();
            format!(
                "{field}: unknown {} preset {key:?}; valid keys are {}",
                P::KIND,
                keys.join(", ")
            )
        })
    };
    match slot {
        Value::String(key) => *slot = to_value(preset(key)?.options()),
        Value::Object(map) => match map.remove("base") {
            Some(Value::String(key)) => {
                let mut full = to_value(preset(&key)?.options());
                merge(&mut full, slot.take());
                *slot = full;
            }
            Some(_) => return Err(format!("{field}.base must be a {} preset key", P::KIND)),
            None => {}
        },
        _ => return Err(format!("{field} must be a {} preset key or a map", P::KIND)),
    }
    Ok(())
}

/// Reads `T` from `settings` laid over `defaults`.
fn fill<T: Serialize + serde::de::DeserializeOwned>(
    defaults: &T,
    settings: Map<String, Value>,
) -> Result<T, String> {
    let mut value = to_value(defaults);
    merge(&mut value, Value::Object(settings));
    from_value(value, "").map_err(|error| error.to_string())
}
