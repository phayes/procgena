//! How an Ecotope's Scatters are written in a `.ecotope.yml` file.
//!
//! Scatters are a map from key to settings, so a file built on a `base`
//! changes, adds or replaces one Scatter without restating the others. Each
//! Scatter names its `kind`, and gives only the settings that differ from
//! that kind's defaults; a Scatter set to `null` is removed. Assets inside
//! it (a stone class's `stone`, dead wood's `piece`, a mushroom Scatter's
//! `mushroom`) name a built-in preset by key, or give settings laid over
//! one (`{ base: <key>, ... }`) or over the asset's defaults. A plant
//! Scatter's settings are those of a `.plant_scatter.yml` file's
//! `plant_scatter:`, laid over the `meadow` preset, or over the plant
//! Scatter preset its `base` names; its plants name plant presets by key.
//!
//! Ground layers are a map from key to settings in the same way: a file
//! changes, adds or (with `null`) removes one layer without restating the
//! others.

use std::collections::BTreeMap;

use procgena_core::ChildKey;
use procgena_core::preset::{
    Preset, PresetFileError, ReferenceSlot, References, Registry, diff, expand_slot, from_value,
    merge, slot_reference, to_value,
};
use procgena_deadwood::{DeadwoodPreset, ScatterOptions as DeadwoodScatterOptions};
use procgena_mushroom::{
    MushroomOptions, MushroomPreset, ScatterOptions as MushroomScatterOptions,
};
use procgena_plant::{PlantScatterOptions, PlantScatterPreset};
use procgena_stone::{StoneClass, StoneOptions, StonePreset, StoneScatterOptions};
use procgena_terrain::ground::GroundLayer;
use procgena_tree::{TreeScatter, TreeScatterPreset};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use super::scatters::ScatterKind;
use super::{Ecotope, EcotopeScatter};

/// Reads and writes an Ecotope's maps of Scatters and ground layers with
/// [`ChildKey`]s as keys. An entry set to `null` is left out.
pub(crate) mod keyed {
    use super::*;

    pub(crate) fn serialize<S: Serializer, T: Serialize>(
        entries: &BTreeMap<ChildKey, T>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.collect_map(entries.iter().map(|(key, entry)| (key.as_str(), entry)))
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<ChildKey, T>, D::Error> {
        let raw = BTreeMap::<String, Option<T>>::deserialize(deserializer)?;
        raw.into_iter()
            .filter_map(|(key, entry)| Some((key, entry?)))
            .map(|(key, entry)| match ChildKey::new(key.as_str()) {
                Ok(key) => Ok((key, entry)),
                Err(error) => Err(D::Error::custom(format!("key {key:?}: {error}"))),
            })
            .collect()
    }
}

impl Serialize for EcotopeScatter {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        to_file(self, false).serialize(serializer)
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
                "a Scatter must name its kind: tree, stone, deadwood, mushroom or plant".to_owned(),
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
        "plant" => {
            let base = match map.remove("base") {
                Some(Value::String(key)) => PlantScatterPreset::named(&key)
                    .ok_or_else(|| {
                        let keys: Vec<&str> = PlantScatterPreset::all().map(|p| &*p.key).collect();
                        format!(
                            "base: unknown plant Scatter preset {key:?}; valid keys are {}",
                            keys.join(", ")
                        )
                    })?
                    .plant_scatter
                    .clone(),
                Some(_) => return Err("base must be a plant Scatter preset key".to_owned()),
                None => PlantScatterOptions::default(),
            };
            ScatterKind::Plant(fill(&base, map)?)
        }
        other => {
            return Err(format!(
                "unknown kind {other:?}; a Scatter's kind is tree, stone, deadwood, mushroom \
                 or plant"
            ));
        }
    };
    Ok(EcotopeScatter {
        priority,
        clearance,
        scatter,
    })
}

/// The settings of an Ecotope file holding `ecotope`, laid over `base`'s
/// Ecotope or, without one, over the defaults. `None` when nothing differs.
///
/// Over a base, each Scatter gives what differs from the base's Scatter
/// with the same key, compared on every setting, and a Scatter the base
/// has and `ecotope` lacks is written as `null`. Without a base, each
/// Scatter gives what differs from its kind's defaults.
pub(crate) fn settings_over(base: Option<&Ecotope>, ecotope: &Ecotope) -> Option<Value> {
    let Some(base) = base else {
        let mut value = to_value(ecotope);
        for (key, scatter) in &ecotope.scatters {
            value["scatters"][key.as_str()] = to_file(scatter, true);
        }
        for (key, layer) in &ecotope.ground {
            value["ground"][key.as_str()] = short_layer(layer);
        }
        return diff(&to_value(&Ecotope::default()), &value);
    };

    // Exact differences, with new ground layers written short.
    let mut settings = diff(&to_value(base), &to_value(ecotope));
    for (key, layer) in &ecotope.ground {
        if !base.ground.contains_key(key) {
            entry(&mut settings, "ground")[key.as_str()] = short_layer(layer);
        }
    }

    // The removed Scatters and ground layers.
    for key in base.scatters.keys() {
        if !ecotope.scatters.contains_key(key) {
            entry(&mut settings, "scatters")[key.as_str()] = Value::Null;
        }
    }
    for key in base.ground.keys() {
        if !ecotope.ground.contains_key(key) {
            entry(&mut settings, "ground")[key.as_str()] = Value::Null;
        }
    }
    settings
}

/// The map at `settings[field]`, made (with `settings`) if missing.
fn entry<'a>(settings: &'a mut Option<Value>, field: &str) -> &'a mut Value {
    let settings = settings.get_or_insert_with(|| Value::Object(Map::new()));
    if settings.get(field).is_none() {
        settings[field] = Value::Object(Map::new());
    }
    &mut settings[field]
}

/// A ground layer's settings, leaving out those equal to a new layer's of
/// the same material and order.
fn short_layer(layer: &GroundLayer) -> Value {
    let defaults = GroundLayer::new(layer.material.clone(), layer.order);
    let mut value =
        diff(&to_value(&defaults), &to_value(layer)).unwrap_or_else(|| Value::Object(Map::new()));
    value["material"] = Value::String(layer.material.as_str().to_owned());
    value["order"] = Value::from(layer.order);
    value
}

/// Writes one Scatter as its file settings. With `short`, the settings of
/// its kind that equal the kind's defaults are left out; otherwise every
/// setting is written.
fn to_file(scatter: &EcotopeScatter, short: bool) -> Value {
    let mut map = Map::new();
    map.insert("priority".to_owned(), Value::from(scatter.priority));
    map.insert("clearance".to_owned(), to_value(&scatter.clearance));
    map.insert(
        "kind".to_owned(),
        Value::String(scatter.scatter.name().to_owned()),
    );

    // The kind's own settings, and the defaults they are compared with.
    let (settings, defaults) = match &scatter.scatter {
        ScatterKind::Tree(options) => (to_value(options), to_value(&TreeScatter::default())),
        ScatterKind::Stone(options) => (stone_file(options), stone_file(&stone_default())),
        ScatterKind::Deadwood(options) => {
            (deadwood_file(options), deadwood_file(&deadwood_default()))
        }
        ScatterKind::Mushroom { mushroom, scatter } => {
            map.insert("mushroom".to_owned(), reference::<MushroomPreset>(mushroom));
            (
                to_value(scatter),
                to_value(&MushroomScatterOptions::default()),
            )
        }
        ScatterKind::Plant(options) => {
            (to_value(options), to_value(&PlantScatterOptions::default()))
        }
    };
    let written = if short {
        diff(&defaults, &settings)
    } else {
        Some(settings)
    };
    if let Some(Value::Object(written)) = written {
        map.extend(written);
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

/// How an Ecotope's settings refer to asset presets: a tree Scatter's
/// trees, a stone Scatter's classes' stones, dead wood's piece and a
/// mushroom Scatter's mushroom.
pub(crate) const REFERENCES: References = References {
    slots: ecotope_slots,
    expand: expand_ecotope,
};

/// Each Scatter in an Ecotope's settings with its key and kind. Scatters
/// set to `null` and Scatters without a kind are left out.
fn scatters(settings: &Value) -> impl Iterator<Item = (&String, &str, &Value)> {
    settings
        .get("scatters")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(key, scatter)| Some((key, scatter.get("kind")?.as_str()?, scatter)))
}

/// The asset preset references in an Ecotope's settings, at paths such as
/// `scatters.pines.trees.0.tree` and `scatters.boulders.classes.1.stone`.
fn ecotope_slots(settings: &Value) -> Vec<ReferenceSlot> {
    let mut slots = Vec::new();
    for (key, kind, scatter) in scatters(settings) {
        // Each slot's path within the Scatter, and the kind it names.
        let found: Vec<(String, &'static str, Option<&Value>)> = match kind {
            "tree" => {
                for tree in (TreeScatterPreset::references().slots)(scatter) {
                    slots.push(ReferenceSlot {
                        path: format!("scatters.{key}.{}", tree.path),
                        ..tree
                    });
                }
                Vec::new()
            }
            "stone" => {
                let classes = scatter.get("classes").and_then(Value::as_array);
                classes
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .map(|(i, class)| {
                        (
                            format!("classes.{i}.stone"),
                            StonePreset::KIND,
                            class.get("stone"),
                        )
                    })
                    .collect()
            }
            "deadwood" => vec![(
                "piece".to_owned(),
                DeadwoodPreset::KIND,
                scatter.get("piece"),
            )],
            "mushroom" => vec![(
                "mushroom".to_owned(),
                MushroomPreset::KIND,
                scatter.get("mushroom"),
            )],
            _ => Vec::new(),
        };

        // Those that hold a reference.
        for (path, kind, value) in found {
            if let Some(reference) = value.and_then(slot_reference) {
                slots.push(ReferenceSlot {
                    path: format!("scatters.{key}.{path}"),
                    kind,
                    reference: reference.to_owned(),
                });
            }
        }
    }
    slots
}

/// Replaces every asset preset reference in an Ecotope's settings with
/// the settings it names, looked up in `registry`.
fn expand_ecotope(settings: &mut Value, registry: &Registry) -> Result<(), PresetFileError> {
    let Some(Value::Object(scatters)) = settings.get_mut("scatters") else {
        return Ok(());
    };
    for (key, scatter) in scatters.iter_mut() {
        let at = |path: &str| format!("scatters.{key}.{path}");
        let kind = scatter
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match kind {
            "tree" => (TreeScatterPreset::references().expand)(scatter, registry)
                .map_err(|error| error.at(&format!("scatters.{key}")))?,
            "stone" => {
                if let Some(Value::Array(classes)) = scatter.get_mut("classes") {
                    for (i, class) in classes.iter_mut().enumerate() {
                        if let Some(stone) = class.get_mut("stone") {
                            expand_slot::<StonePreset>(stone, registry)
                                .map_err(|error| error.at(&at(&format!("classes.{i}.stone"))))?;
                        }
                    }
                }
            }
            "deadwood" => {
                if let Some(piece) = scatter.get_mut("piece") {
                    expand_slot::<DeadwoodPreset>(piece, registry)
                        .map_err(|error| error.at(&at("piece")))?;
                }
            }
            "mushroom" => {
                if let Some(mushroom) = scatter.get_mut("mushroom") {
                    expand_slot::<MushroomPreset>(mushroom, registry)
                        .map_err(|error| error.at(&at("mushroom")))?;
                }
            }
            _ => {}
        }
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
