//! Save and load plant and field appearances without rebuilding the game.
//!
//! `.plant.ron` and `.field.ron` files load as preset assets. Bevy's
//! `file_watcher` feature reloads those assets when files change; copy the
//! updated settings to the field component to show the changes.
//!
//! Every setting is optional. A missing one comes from `base`, the key of a
//! built-in preset, so a variant names only what differs. A plant file starts
//! from a [`PlantPreset`] (`MEADOW_GRASS` when not given); nest its shape and
//! colours under `segment`, its branches under `levels`, and its accent
//! stems under `accent`:
//!
//! ```text
//! // assets/plants/blue_fern.plant.ron
//! (
//!     name: "Blue fern",
//!     base: "FERN",
//!     segment: (tip_color: (0.20, 0.34, 0.60)),
//!     accent: (share: 0.1, height: (0.9, 1.2), head_length: 0.2),
//! )
//! ```
//!
//! A field file starts from a [`FieldPreset`] (`MEADOW` when not given). Its
//! `plants` list replaces the base's plants. Each entry names a plant preset
//! by key, or writes a plant inline, with the same settings as a plant file.
//! Each plant grows at its own `density` times its `density_scale` (1 when
//! not given), and the field's `density_scale` scales them all. These entries
//! give 4 seed heads, 1 flower and 35 grass plants per square metre. The last
//! plant with a positive density fills the remaining gaps:
//!
//! ```text
//! // assets/fields/wild_edge.field.ron
//! (
//!     base: "SUMMER_TALL",
//!     plants: [
//!         (plant: "SEED_HEADS", density_scale: 0.1),
//!         (plant: (base: "WHITE_FLOWERS", head_color: "#c08cf2", density: 1.0), clustering: 0.8),
//!         (plant: (base: "SUMMER_TALL_GRASS", tufts: 0.3), density_scale: 0.875),
//!     ],
//! )
//! ```
//!
//! Colours are sRGB, as in `Color::srgb`, or hexadecimal text. The crate's
//! `assets/plants` and `assets/fields` folders hold every preset
//! as a file to copy from.
//!
//! [`PlantParams::to_json`], [`PlantField::to_json`] and their `from_json`
//! counterparts save and load the same settings as JSON. Field files omit
//! size, ground and density maps; set those on the field in the game.

use std::borrow::Cow;
use std::fmt;
use std::path::{Path, PathBuf};

use bevy::app::{App, Plugin};
use bevy::asset::io::Reader;
use bevy::asset::io::embedded::EmbeddedAssetRegistry;
use bevy::asset::{AssetApp, AssetLoader, LoadContext};
use bevy::color::{Color, Srgba};
use bevy::math::Vec2;
use bevy::reflect::TypePath;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::api::{
    FieldPlant, PlantAccent, PlantArrangement, PlantBranches, PlantField, PlantParams, PlantSegment,
};
use crate::look::{MAX_BRANCHES, MAX_PLANTS, MAX_STEMS_PER_PLANT};
use crate::presets::{FieldPreset, PlantPreset};

/// The group of a preset loaded from a file.
const FILE_GROUP: &str = "File";

impl PlantParams {
    /// Reads a plant from the text of a `.plant.ron` file.
    ///
    /// A `name` or `description` in the file is ignored; use
    /// [`PlantPreset::from_ron`] to keep them.
    ///
    /// # Errors
    ///
    /// Returns [`PlantFileError`] for malformed RON, unknown settings or
    /// base presets, or out-of-range values.
    pub fn from_ron(text: &str) -> Result<Self, PlantFileError> {
        parse_ron::<PlantParamsFile>(text)?.resolve()
    }

    /// Writes all plant settings as a `.plant.ron` file.
    ///
    /// The result is self-contained and does not rely on a base preset.
    ///
    /// # Panics
    ///
    /// Panics if RON serialisation fails. The plant's numeric and colour
    /// settings are serialisable, so this is not expected in normal use.
    #[must_use]
    pub fn to_ron(&self) -> String {
        write_ron(&PlantParamsFile::from_plant(self))
    }

    /// Reads plant settings from JSON.
    ///
    /// Uses the same settings and defaults as a `.plant.ron` file. Write
    /// colours as sRGB `[red, green, blue]` values
    /// from 0 to 1, or as hexadecimal text such as `"#ff8000"`.
    ///
    /// # Errors
    ///
    /// Returns [`PlantFileError`] for malformed JSON, unknown settings or
    /// presets, or out-of-range values.
    pub fn from_json(text: &str) -> Result<Self, PlantFileError> {
        parse_json::<PlantParamsFile>(text)?.resolve()
    }

    /// Writes all plant settings as formatted JSON.
    ///
    /// Load the result with [`from_json`](Self::from_json).
    ///
    /// # Panics
    ///
    /// Panics if JSON serialisation fails. The plant's numeric and colour
    /// settings are serialisable, so this is not expected in normal use.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&PlantParamsFile::from_plant(self))
            .expect("a plant always serializes")
    }
}

impl PlantField {
    /// Reads a field's plants and look from the text of a `.field.ron` file.
    ///
    /// A file holds what a [`FieldPreset`] holds, so the field it gives has
    /// size 0, flat ground and no density map: set those afterwards. A `name`
    /// or `description` in the file is ignored; use [`FieldPreset::from_ron`]
    /// to keep them.
    ///
    /// # Errors
    ///
    /// Returns [`PlantFileError`] for malformed RON, unknown settings or
    /// presets, or out-of-range values.
    pub fn from_ron(text: &str) -> Result<Self, PlantFileError> {
        parse_ron::<PlantFieldFile>(text)?.resolve()
    }

    /// Writes the field's plants and appearance as a `.field.ron` file.
    ///
    /// The size, ground and density map are not written. A plant equal to a
    /// [`PlantPreset`] is written as its key.
    ///
    /// # Panics
    ///
    /// Panics if RON serialisation fails. The field's numeric and colour
    /// settings are serialisable, so this is not expected in normal use.
    #[must_use]
    pub fn to_ron(&self) -> String {
        write_ron(&PlantFieldFile::from_field(self))
    }

    /// Reads the field's plants and appearance from JSON.
    ///
    /// Uses the same settings and defaults as a `.field.ron` file. The result
    /// has size 0, flat ground and no density map; set placement afterwards.
    ///
    /// # Errors
    ///
    /// Returns [`PlantFileError`] for malformed JSON, unknown settings or
    /// presets, or out-of-range values.
    pub fn from_json(text: &str) -> Result<Self, PlantFileError> {
        parse_json::<PlantFieldFile>(text)?.resolve()
    }

    /// Writes the field's plants and appearance as formatted JSON.
    ///
    /// Load the result with [`from_json`](Self::from_json).
    ///
    /// The size, ground and density map are not written.
    ///
    /// # Panics
    ///
    /// Panics if JSON serialisation fails. The field's numeric and colour
    /// settings are serialisable, so this is not expected in normal use.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&PlantFieldFile::from_field(self))
            .expect("a field always serializes")
    }
}

impl PlantPreset {
    /// Reads a named plant preset from a `.plant.ron` file.
    ///
    /// The caller supplies `key`; it is not read from the text.
    /// Its name is the file's `name`, or `key`; its description is the
    /// file's `description`, or empty; its group is `"File"`.
    ///
    /// # Errors
    ///
    /// Returns [`PlantFileError`] for malformed RON, unknown settings or
    /// base presets, or out-of-range values.
    pub fn from_ron(key: &str, text: &str) -> Result<Self, PlantFileError> {
        let file = parse_ron::<PlantParamsFile>(text)?;
        Ok(Self {
            key: Cow::Owned(key.to_owned()),
            name: Cow::Owned(file.name.clone().unwrap_or_else(|| key.to_owned())),
            description: Cow::Owned(file.description.clone().unwrap_or_default()),
            group: Cow::Borrowed(FILE_GROUP),
            plant: file.resolve()?,
        })
    }

    /// Writes the preset's name, description and settings as a `.plant.ron` file.
    ///
    /// The key and catalogue group are not written.
    ///
    /// # Panics
    ///
    /// Panics if RON serialisation fails. The plant's numeric and colour
    /// settings are serialisable, so this is not expected in normal use.
    #[must_use]
    pub fn to_ron(&self) -> String {
        write_ron(&PlantParamsFile {
            name: Some(self.name.to_string()),
            description: Some(self.description.to_string()),
            ..PlantParamsFile::from_plant(&self.plant)
        })
    }
}

impl FieldPreset {
    /// Reads a named field preset from a `.field.ron` file.
    ///
    /// The caller supplies `key`; it is not read from the text.
    /// Its name is the file's `name`, or `key`; its description is the
    /// file's `description`, or empty; its group is `"File"`.
    ///
    /// # Errors
    ///
    /// Returns [`PlantFileError`] for malformed RON, unknown settings or
    /// presets, or out-of-range values.
    pub fn from_ron(key: &str, text: &str) -> Result<Self, PlantFileError> {
        let file = parse_ron::<PlantFieldFile>(text)?;
        let name = file.name.clone().unwrap_or_else(|| key.to_owned());
        let description = file.description.clone().unwrap_or_default();
        Ok(Self {
            key: Cow::Owned(key.to_owned()),
            name: Cow::Owned(name),
            description: Cow::Owned(description),
            group: Cow::Borrowed(FILE_GROUP),
            field: file.resolve()?,
        })
    }

    /// Writes the preset's name, description and settings as a `.field.ron` file.
    ///
    /// The key and catalogue group are not written.
    /// Field size, ground and density map are also omitted.
    ///
    /// # Panics
    ///
    /// Panics if RON serialisation fails. The field's numeric and colour
    /// settings are serialisable, so this is not expected in normal use.
    #[must_use]
    pub fn to_ron(&self) -> String {
        write_ron(&PlantFieldFile {
            name: Some(self.name.to_string()),
            description: Some(self.description.to_string()),
            ..PlantFieldFile::from_field(&self.field)
        })
    }
}

/// Fields are written plainly, `width: 0.04`, not `width: Some(0.04)`.
fn options() -> ron::Options {
    ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
}

/// Parses RON settings and preserves the line and column of parse errors.
fn parse_ron<T: DeserializeOwned>(text: &str) -> Result<T, PlantFileError> {
    options()
        .from_str(text)
        .map_err(|error| PlantFileError::Parse {
            line: error.span.start.line,
            column: error.span.start.col,
            message: error.code.to_string(),
        })
}

/// Parses JSON settings and preserves the line and column of parse errors.
fn parse_json<T: DeserializeOwned>(text: &str) -> Result<T, PlantFileError> {
    serde_json::from_str(text).map_err(|error| PlantFileError::Parse {
        line: error.line(),
        column: error.column(),
        message: error.to_string(),
    })
}

/// Writes a file struct as pretty RON with Unix line endings.
fn write_ron<T: Serialize>(file: &T) -> String {
    let config = ron::ser::PrettyConfig::new()
        .struct_names(false)
        .new_line("\n".to_owned());
    options()
        .to_string_pretty(file, config)
        .expect("a plant or field always serializes")
}

/// How one setting is written in a file: `File` is what the text holds.
trait FileValue: Sized {
    type File: Copy + Serialize + for<'de> Deserialize<'de>;
    fn to_file(&self) -> Self::File;
    fn from_file(file: Self::File) -> Self;
}

impl FileValue for f32 {
    type File = f32;
    fn to_file(&self) -> f32 {
        *self
    }
    fn from_file(file: f32) -> f32 {
        file
    }
}

impl FileValue for u32 {
    type File = u32;
    fn to_file(&self) -> u32 {
        *self
    }
    fn from_file(file: u32) -> u32 {
        file
    }
}

impl FileValue for Vec2 {
    type File = (f32, f32);
    fn to_file(&self) -> (f32, f32) {
        (self.x, self.y)
    }
    fn from_file((x, y): (f32, f32)) -> Vec2 {
        Vec2::new(x, y)
    }
}

impl FileValue for Color {
    type File = FileColor;
    fn to_file(&self) -> FileColor {
        FileColor(*self)
    }
    fn from_file(file: FileColor) -> Color {
        file.0
    }
}

impl FileValue for PlantArrangement {
    type File = FileArrangement;
    fn to_file(&self) -> FileArrangement {
        match self {
            PlantArrangement::Opposite => FileArrangement::Opposite,
            PlantArrangement::Alternate => FileArrangement::Alternate,
        }
    }
    fn from_file(file: FileArrangement) -> PlantArrangement {
        match file {
            FileArrangement::Opposite => PlantArrangement::Opposite,
            FileArrangement::Alternate => PlantArrangement::Alternate,
        }
    }
}

/// Branch arrangement names accepted in files: `Opposite` and `Alternate`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum FileArrangement {
    Opposite,
    Alternate,
}

/// A colour in a file: sRGB as `(0.38, 0.55, 0.16)`, or hex as `"#5c8f24"`
/// (the form colour pickers copy). Written as the tuple, which keeps every
/// bit of a preset's colour.
#[derive(Debug, Clone, Copy)]
struct FileColor(Color);

impl Serialize for FileColor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let c = self.0.to_srgba();
        (c.red, c.green, c.blue).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for FileColor {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ColorVisitor;

        impl<'de> serde::de::Visitor<'de> for ColorVisitor {
            type Value = FileColor;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(
                    "a colour: (red, green, blue) in sRGB, or a hex string like \"#5c8f24\"",
                )
            }

            fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<FileColor, E> {
                Srgba::hex(text)
                    .map(|color| FileColor(Color::Srgba(color)))
                    .map_err(|error| {
                        E::custom(format!(
                            "{text:?} is not a hex colour ({error}); write \"#rrggbb\""
                        ))
                    })
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<FileColor, A::Error> {
                let mut channel = |name: &'static str| -> Result<f32, A::Error> {
                    seq.next_element()?
                        .ok_or_else(|| serde::de::Error::custom(format!("a colour needs {name}")))
                };
                let (r, g, b) = (channel("red")?, channel("green")?, channel("blue")?);
                Ok(FileColor(Color::srgb(r, g, b)))
            }
        }

        deserializer.deserialize_any(ColorVisitor)
    }
}

/// Defines optional file settings and their read/write helpers together. Listing each
/// setting once keeps serialisation and application in sync.
///
/// Settings in `before` are written ahead of the plain settings, and those in
/// `extra` after them; both are resolved separately. Plain settings map
/// directly to fields of `$ty`.
macro_rules! settings_file {
    (
        $(#[$meta:meta])*
        struct $file:ident for $ty:ty {
            $( $name:ident : $vty:ty ),* $(,)?
        }
        $(before {
            $( $(#[$bmeta:meta])* $bname:ident : $bty:ty ),* $(,)?
        })?
        extra {
            $( $(#[$emeta:meta])* $ename:ident : $ety:ty ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Default, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        struct $file {
            $($( $(#[$bmeta])* $bname: $bty, )*)?
            $(
                #[serde(default, skip_serializing_if = "Option::is_none")]
                $name: Option<<$vty as FileValue>::File>,
            )*
            $( $(#[$emeta])* $ename: $ety, )*
        }

        impl $file {
            /// Copies all plain settings from `value`, leaving the others unset.
            fn plain_from(value: &$ty) -> Self {
                let mut file = Self::default();
                $( file.$name = Some(<$vty as FileValue>::to_file(&value.$name)); )*
                file
            }

            /// Applies only explicitly supplied plain settings, preserving other
            /// values.
            fn apply_plain(&self, value: &mut $ty) {
                $(
                    if let Some(file) = self.$name {
                        value.$name = <$vty as FileValue>::from_file(file);
                    }
                )*
            }
        }
    };
}

settings_file! {
    /// Optional `segment: (...)` overrides for a main stem or branch level.
    struct PlantSegmentFile for PlantSegment {
        profile: f32,
        stiffness: f32,
        droop: f32,
        twist: f32,
        fold: f32,
        root_color: Color,
        tip_color: Color,
    }
    extra {}
}

settings_file! {
    /// Optional overrides for one entry in a plant's `levels` list.
    struct PlantBranchesFile for PlantBranches {
        count: u32,
        arrangement: PlantArrangement,
        attachment_start: f32,
        length: f32,
        length_envelope: f32,
        width: f32,
        angle: f32,
        spread: f32,
    }
    extra {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        segment: Option<PlantSegmentFile>,
    }
}

settings_file! {
    /// A plant: a `.plant.ron` file, or a plant written inline in a field
    /// file. `base` selects a plant preset before these overrides apply.
    struct PlantParamsFile for PlantParams {
        density: f32,
        height: Vec2,
        width: f32,
        lean: f32,
        stems_per_plant: u32,
        root_occlusion: f32,
        translucency: f32,
        specular: f32,
        head_length: f32,
        head_width: f32,
        head_color: Color,
        tufts: f32,
        tuft_size: f32,
    }
    before {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base: Option<String>,
    }
    extra {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        segment: Option<PlantSegmentFile>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        levels: Option<Vec<PlantBranchesFile>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        accent: Option<PlantAccentFile>,
    }
}

settings_file! {
    /// Optional `accent: (...)` overrides for a plant's accent stems.
    struct PlantAccentFile for PlantAccent {
        share: f32,
        height: Vec2,
        width: f32,
        lean: f32,
        translucency: f32,
        head_length: f32,
        head_width: f32,
        head_color: Color,
    }
    extra {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        segment: Option<PlantSegmentFile>,
    }
}

impl PlantAccentFile {
    /// Captures every accent setting, including its segment, for serialisation.
    fn from_accent(accent: &PlantAccent) -> Self {
        Self {
            segment: Some(PlantSegmentFile::plain_from(&accent.segment)),
            ..Self::plain_from(accent)
        }
    }

    /// Applies accent and segment overrides while preserving omitted values.
    fn apply(&self, accent: &mut PlantAccent) {
        self.apply_plain(accent);
        if let Some(segment) = &self.segment {
            segment.apply(&mut accent.segment);
        }
    }
}

impl PlantSegmentFile {
    /// Applies supplied segment settings without replacing omitted values.
    fn apply(&self, segment: &mut PlantSegment) {
        self.apply_plain(segment);
    }
}

impl PlantBranchesFile {
    /// Captures every branch setting, including its segment, for serialisation.
    fn from_branches(branches: &PlantBranches) -> Self {
        Self {
            segment: Some(PlantSegmentFile::plain_from(&branches.segment)),
            ..Self::plain_from(branches)
        }
    }

    /// Applies branch and segment overrides while preserving omitted values.
    fn apply(&self, branches: &mut PlantBranches) {
        self.apply_plain(branches);
        if let Some(segment) = &self.segment {
            segment.apply(&mut branches.segment);
        }
    }
}

impl PlantParamsFile {
    /// Captures all plant settings without relying on a base preset. Omits
    /// `levels` only when both exactly equal `PlantBranches::NONE`, and
    /// `accent` only when it exactly equals `PlantAccent::NONE`, matching the
    /// defaults when the file is read again.
    fn from_plant(plant: &PlantParams) -> Self {
        let unbranched = plant
            .levels
            .iter()
            .all(|level| *level == PlantBranches::NONE);
        Self {
            segment: Some(PlantSegmentFile::plain_from(&plant.segment)),
            levels: (!unbranched).then(|| {
                plant
                    .levels
                    .iter()
                    .map(PlantBranchesFile::from_branches)
                    .collect()
            }),
            accent: (plant.accent != PlantAccent::NONE)
                .then(|| PlantAccentFile::from_accent(&plant.accent)),
            ..Self::plain_from(plant)
        }
    }

    /// Applies overrides to the named base plant, or `MEADOW_GRASS`, then
    /// validates the result.
    fn resolve(&self) -> Result<PlantParams, PlantFileError> {
        let mut plant = match &self.base {
            Some(key) => plant_preset(key)?.plant,
            None => PlantParams::MEADOW_GRASS,
        };
        self.apply_plain(&mut plant);
        if let Some(segment) = &self.segment {
            segment.apply(&mut plant.segment);
        }
        if let Some(levels) = &self.levels {
            if levels.len() > plant.levels.len() {
                return Err(PlantFileError::Invalid(
                    "a plant has at most 2 levels of branches",
                ));
            }
            for (file, level) in levels.iter().zip(&mut plant.levels) {
                file.apply(level);
            }
        }
        if let Some(accent) = &self.accent {
            accent.apply(&mut plant.accent);
        }
        validate_plant(&plant)?;
        Ok(plant)
    }
}

/// Looks up a plant preset by key, reporting an unknown key as an error.
fn plant_preset(key: &str) -> Result<&'static PlantPreset, PlantFileError> {
    PlantPreset::named(key).ok_or_else(|| PlantFileError::UnknownPlant(key.to_owned()))
}

/// Looks up a field preset by key, reporting an unknown key as an error.
fn field_preset(key: &str) -> Result<&'static FieldPreset, PlantFileError> {
    FieldPreset::named(key).ok_or_else(|| PlantFileError::UnknownField(key.to_owned()))
}

/// A built-in plant key or inline plant settings in a field file.
/// For example, `"FERN"` uses the preset unchanged;
/// `(base: "FERN", lean: 0.2)` changes just its lean. Keys refer to built-in
/// presets, not paths to other plant files.
#[derive(Debug)]
enum PlantRef {
    Preset(String),
    Inline(Box<PlantParamsFile>),
}

impl PlantRef {
    /// The key of the preset equal to `plant`, or the plant written out.
    fn from_plant(plant: &PlantParams) -> Self {
        match PlantPreset::all().find(|preset| preset.plant == *plant) {
            Some(preset) => Self::Preset(preset.key.to_string()),
            None => Self::Inline(Box::new(PlantParamsFile::from_plant(plant))),
        }
    }

    fn resolve(&self) -> Result<PlantParams, PlantFileError> {
        match self {
            Self::Preset(key) => Ok(plant_preset(key)?.plant),
            Self::Inline(file) => file.resolve(),
        }
    }
}

impl Serialize for PlantRef {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Preset(key) => serializer.serialize_str(key),
            Self::Inline(file) => file.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for PlantRef {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PlantVisitor;

        impl<'de> serde::de::Visitor<'de> for PlantVisitor {
            type Value = PlantRef;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(
                    "a plant: a plant preset's key such as \"FERN\", or plant settings \
                     such as (base: \"FERN\", lean: 0.2)",
                )
            }

            fn visit_str<E: serde::de::Error>(self, key: &str) -> Result<PlantRef, E> {
                Ok(PlantRef::Preset(key.to_owned()))
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(self, map: A) -> Result<PlantRef, A::Error> {
                PlantParamsFile::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                    .map(|file| PlantRef::Inline(Box::new(file)))
            }
        }

        deserializer.deserialize_any(PlantVisitor)
    }
}

/// One plant in a field file, with its density scale and clustering.
/// The nested `plant` accepts plant settings only; field-wide settings such
/// as `density_scale` belong outside the plant list.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldPlantFile {
    plant: PlantRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    density_scale: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    clustering: Option<f32>,
}

impl FieldPlantFile {
    fn from_entry(entry: &FieldPlant) -> Self {
        Self {
            plant: PlantRef::from_plant(&entry.plant),
            density_scale: (entry.density_scale != 1.0).then_some(entry.density_scale),
            clustering: (entry.clustering != 0.0).then_some(entry.clustering),
        }
    }

    fn resolve(&self) -> Result<FieldPlant, PlantFileError> {
        let density_scale = self.density_scale.unwrap_or(1.0);
        if density_scale < 0.0 || !density_scale.is_finite() {
            return Err(PlantFileError::Invalid(
                "a plant's density_scale must be at least 0",
            ));
        }
        let clustering = self.clustering.unwrap_or(0.0);
        if !(0.0..=1.0).contains(&clustering) {
            return Err(PlantFileError::Invalid(
                "a plant's clustering must be from 0 to 1",
            ));
        }
        Ok(FieldPlant::new(self.plant.resolve()?)
            .scaled(density_scale)
            .clustered(clustering))
    }
}

/// Maps a field file's settings to their paths in `PlantField`.
macro_rules! field_file {
    (
        $( $fname:ident : $fty:ty => $($fpath:ident).+ ),* $(,)?
    ) => {
        /// The `.field.ron` format: a field's plants and look, every setting
        /// optional.
        #[derive(Debug, Default, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        struct PlantFieldFile {
            #[serde(default, skip_serializing_if = "Option::is_none")]
            name: Option<String>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            description: Option<String>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            base: Option<String>,
            $(
                #[serde(default, skip_serializing_if = "Option::is_none")]
                $fname: Option<<$fty as FileValue>::File>,
            )*
            #[serde(default, skip_serializing_if = "Option::is_none")]
            plants: Option<Vec<FieldPlantFile>>,
        }

        impl PlantFieldFile {
            /// A file that sets every setting of `field` but its size,
            /// ground and map.
            fn from_field(field: &PlantField) -> Self {
                Self {
                    name: None,
                    description: None,
                    base: None,
                    $( $fname: Some(<$fty as FileValue>::to_file(&field.$($fpath).+)), )*
                    plants: Some(field.plants.iter().map(FieldPlantFile::from_entry).collect()),
                }
            }

            /// The settings the file gives, over `field`.
            fn apply_plain(&self, field: &mut PlantField) {
                $(
                    if let Some(value) = self.$fname {
                        field.$($fpath).+ = <$fty as FileValue>::from_file(value);
                    }
                )*
            }
        }
    };
}

field_file! {
    wind_scale: f32 => wind_scale,
    density_scale: f32 => density_scale,
    ground_color: Color => ground_tint.color,
    ground_tint: f32 => ground_tint.amount,
    toon_bands: u32 => toon_bands,
    patch_color: Color => variation.patch_color,
    patch_amount: f32 => variation.patch_amount,
    patch_size: f32 => variation.patch_size,
    dry_tips: f32 => variation.dry_tips,
    dead_share: f32 => variation.dead_share,
    bare_share: f32 => variation.bare_share,
    bare_patch_size: f32 => variation.bare_patch_size,
}

impl PlantFieldFile {
    /// The base preset's field with the file's settings over it, validated.
    fn resolve(&self) -> Result<PlantField, PlantFileError> {
        // Start from the base preset, then apply the plain settings and plants.
        let base = self
            .base
            .as_deref()
            .map_or(Ok(&FieldPreset::MEADOW), field_preset)?;
        let mut field = base.field.clone();
        self.apply_plain(&mut field);
        if let Some(plants) = &self.plants {
            if plants.len() > MAX_PLANTS {
                return Err(PlantFileError::Invalid("a field grows at most 16 plants"));
            }
            field.plants = Cow::Owned(
                plants
                    .iter()
                    .map(FieldPlantFile::resolve)
                    .collect::<Result<_, _>>()?,
            );
        }

        // Check the field-wide settings.
        if !field.plants.iter().any(|entry| entry.density() > 0.0) {
            return Err(PlantFileError::Invalid(
                "a field needs a plant with a density above 0",
            ));
        }
        if field.density_scale < 0.0 || !field.density_scale.is_finite() {
            return Err(PlantFileError::Invalid("density_scale must be at least 0"));
        }
        if field.wind_scale < 0.0 || field.wind_scale.is_nan() {
            return Err(PlantFileError::Invalid("wind_scale must be at least 0"));
        }
        let variation = &field.variation;
        if !(variation.patch_size > 0.0 && variation.bare_patch_size > 0.0) {
            return Err(PlantFileError::Invalid(
                "patch_size and bare_patch_size must be above 0",
            ));
        }
        Ok(field)
    }
}

/// Checks supported plant dimensions, stem counts and shape settings.
/// Accepts height limits in either order and keeps valid settings for hidden
/// branches or heads so they can be used again when enabled.
fn validate_plant(plant: &PlantParams) -> Result<(), PlantFileError> {
    if plant.density < 0.0 || !plant.density.is_finite() {
        return Err(PlantFileError::Invalid("density must be at least 0"));
    }
    let (shortest, tallest) = plant.height_range();
    if !(shortest >= 0.0 && tallest > 0.0) {
        return Err(PlantFileError::Invalid(
            "height must be two lengths of at least 0, one of them above 0",
        ));
    }
    if plant.width.is_nan() || plant.width <= 0.0 {
        return Err(PlantFileError::Invalid("width must be above 0"));
    }
    if !(0.0..0.9).contains(&plant.head_length) {
        return Err(PlantFileError::Invalid(
            "head_length must be at least 0 and below 0.9",
        ));
    }
    if !(1..=MAX_STEMS_PER_PLANT).contains(&plant.stems_per_plant) {
        return Err(PlantFileError::Invalid(
            "stems_per_plant must be from 1 to 8",
        ));
    }
    if !(0.0..=1.0).contains(&plant.tufts) {
        return Err(PlantFileError::Invalid("tufts must be from 0 to 1"));
    }
    if plant.tuft_size <= 0.0 || plant.tuft_size.is_nan() {
        return Err(PlantFileError::Invalid("tuft_size must be above 0"));
    }
    validate_segment(&plant.segment)?;
    for level in &plant.levels {
        validate_branches(level)?;
    }
    validate_accent(&plant.accent)
}

/// Checks an accent's share, and, as for a plant, its size, head and shape.
fn validate_accent(accent: &PlantAccent) -> Result<(), PlantFileError> {
    if !(0.0..=1.0).contains(&accent.share) {
        return Err(PlantFileError::Invalid(
            "an accent's share must be from 0 to 1",
        ));
    }
    let (shortest, tallest) = (accent.height.min_element(), accent.height.max_element());
    if !(shortest >= 0.0 && tallest > 0.0) {
        return Err(PlantFileError::Invalid(
            "an accent's height must be two lengths of at least 0, one of them above 0",
        ));
    }
    if accent.width.is_nan() || accent.width <= 0.0 {
        return Err(PlantFileError::Invalid("an accent's width must be above 0"));
    }
    if !(0.0..0.9).contains(&accent.head_length) {
        return Err(PlantFileError::Invalid(
            "an accent's head_length must be at least 0 and below 0.9",
        ));
    }
    validate_segment(&accent.segment)
}

/// Checks the segment profile range for main stems and both branch levels.
fn validate_segment(segment: &PlantSegment) -> Result<(), PlantFileError> {
    if !(0.0..=1.0).contains(&segment.profile) {
        return Err(PlantFileError::Invalid("profile must be from 0 to 1"));
    }
    Ok(())
}

/// Checks branch counts, attachment positions, dimensions and segment settings.
fn validate_branches(branches: &PlantBranches) -> Result<(), PlantFileError> {
    if branches.count > MAX_BRANCHES {
        return Err(PlantFileError::Invalid(
            "a level of branches has a count of at most 32",
        ));
    }
    if !(0.0..1.0).contains(&branches.attachment_start) {
        return Err(PlantFileError::Invalid(
            "attachment_start must be at least 0 and below 1",
        ));
    }
    if ![branches.length_envelope, branches.angle, branches.spread]
        .iter()
        .all(|value| (0.0..=1.0).contains(value))
    {
        return Err(PlantFileError::Invalid(
            "length_envelope, angle and spread must be from 0 to 1",
        ));
    }
    if !(branches.length >= 0.0 && branches.width >= 0.0) {
        return Err(PlantFileError::Invalid(
            "a level's length and width must be at least 0",
        ));
    }
    validate_segment(&branches.segment)
}

/// An error reading plant or field settings from RON or JSON.
///
/// Distinguishes unreadable files, malformed text, unknown presets and
/// invalid setting values.
#[derive(Debug)]
#[non_exhaustive]
pub enum PlantFileError {
    /// The file could not be read.
    Io(std::io::Error),
    /// The text could not be parsed as plant or field settings.
    /// Includes the location and explanation, such as an unknown setting name.
    #[non_exhaustive]
    Parse {
        /// The line, counted from 1.
        line: usize,
        /// The column, counted from 1.
        column: usize,
        /// What is wrong, such as an unknown setting and the valid names.
        message: String,
    },
    /// A field's `base` names no [`FieldPreset`].
    UnknownField(String),
    /// A plant's `base`, or a plant given by key, names no [`PlantPreset`].
    UnknownPlant(String),
    /// A value is out of range.
    Invalid(&'static str),
}

impl fmt::Display for PlantFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let keys =
            |keys: &mut dyn Iterator<Item = &'static str>| keys.collect::<Vec<_>>().join(", ");
        match self {
            Self::Io(error) => write!(f, "could not read the grass file: {error}"),
            Self::Parse {
                line,
                column,
                message,
            } => write!(f, "not a grass file: {line}:{column}: {message}"),
            Self::UnknownField(key) => write!(
                f,
                "no field preset called {key:?}; the field presets are {}",
                keys(&mut FieldPreset::all().map(|preset| &*preset.key))
            ),
            Self::UnknownPlant(key) => write!(
                f,
                "no plant preset called {key:?}; the plant presets are {}",
                keys(&mut PlantPreset::all().map(|preset| &*preset.key))
            ),
            Self::Invalid(why) => write!(f, "invalid grass file: {why}"),
        }
    }
}

impl std::error::Error for PlantFileError {}

/// The key of a loaded file: its name without the `.plant.ron` or
/// `.field.ron` extension.
fn file_key(load_context: &LoadContext<'_>, extension: &str) -> String {
    let name = load_context
        .path()
        .path()
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.strip_suffix(extension).unwrap_or(&name).to_owned()
}

/// Reads a whole asset as UTF-8 text.
async fn read_text(reader: &mut dyn Reader) -> Result<String, PlantFileError> {
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .await
        .map_err(PlantFileError::Io)?;
    String::from_utf8(bytes).map_err(|error| {
        PlantFileError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    })
}

/// Loads `.plant.ron` files as [`PlantPreset`] assets.
///
/// Registered by [`PlantFilePlugin`]. The asset key is the filename without
/// the `.plant.ron` extension.
#[derive(Debug, Default, TypePath)]
pub struct PlantParamsLoader;

impl AssetLoader for PlantParamsLoader {
    type Asset = PlantPreset;
    type Settings = ();
    type Error = PlantFileError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        load_context: &mut LoadContext<'_>,
    ) -> Result<PlantPreset, PlantFileError> {
        let text = read_text(reader).await?;
        PlantPreset::from_ron(&file_key(load_context, ".plant.ron"), &text)
    }

    fn extensions(&self) -> &[&str] {
        &["plant.ron"]
    }
}

/// Loads `.field.ron` files as [`FieldPreset`] assets.
///
/// Registered by [`PlantFilePlugin`]. The asset key is the filename without
/// the `.field.ron` extension.
#[derive(Debug, Default, TypePath)]
pub struct PlantFieldLoader;

impl AssetLoader for PlantFieldLoader {
    type Asset = FieldPreset;
    type Settings = ();
    type Error = PlantFileError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        load_context: &mut LoadContext<'_>,
    ) -> Result<FieldPreset, PlantFileError> {
        let text = read_text(reader).await?;
        FieldPreset::from_ron(&file_key(load_context, ".field.ron"), &text)
    }

    fn extensions(&self) -> &[&str] {
        &["field.ron"]
    }
}

/// Loads plant and field files and provides the built-in preset assets.
///
/// Part of [`PlantPlugins`](crate::PlantPlugins); requires
/// [`PlantPlugin`](crate::PlantPlugin).
///
/// Every preset is an asset of the `embedded` source, whatever the game's own
/// asset folder holds, so a game can load one by path, to use as it is or to
/// copy. A loaded field is a [`FieldPreset`]; copy its field onto a
/// [`PlantField`] once it has loaded. Keep watching for asset changes and
/// copy the settings again if the field should follow live file edits:
///
/// ```no_run
/// # use bevy::prelude::*;
/// # use procgena_plant::prelude::*;
/// #[derive(Resource)]
/// struct Loading(Handle<FieldPreset>);
///
/// fn load(mut commands: Commands, assets: Res<AssetServer>) {
///     let preset = assets.load("embedded://procgena_plant/fields/summer_tall.field.ron");
///     commands.insert_resource(Loading(preset));
/// }
///
/// fn spawn(mut commands: Commands, loading: Res<Loading>, presets: Res<Assets<FieldPreset>>) {
///     if let Some(preset) = presets.get(&loading.0) {
///         commands.spawn(PlantField::from_preset(preset, Vec2::splat(64.0)));
///         commands.remove_resource::<Loading>();
///     }
/// }
/// ```
///
/// The file names are the presets' lower-case keys:
/// `embedded://procgena_plant/fields/meadow.field.ron` and
/// `embedded://procgena_plant/plants/fern.plant.ron`, for example.
#[derive(Debug, Default, Clone, Copy)]
pub struct PlantFilePlugin;

impl Plugin for PlantFilePlugin {
    fn build(&self, app: &mut App) {
        app.init_asset_loader::<PlantParamsLoader>()
            .init_asset_loader::<PlantFieldLoader>();
        if let Some(registry) = app.world().get_resource::<EmbeddedAssetRegistry>() {
            // Generated from the presets, so an embedded preset cannot drift
            // from its constant or from the shipped file.
            for preset in PlantPreset::all() {
                let path = format!(
                    "procgena_plant/plants/{}.plant.ron",
                    preset.key.to_ascii_lowercase()
                );
                registry.insert_asset(
                    PathBuf::new(),
                    Path::new(&path),
                    preset.to_ron().into_bytes(),
                );
            }
            for preset in FieldPreset::all() {
                let path = format!(
                    "procgena_plant/fields/{}.field.ron",
                    preset.key.to_ascii_lowercase()
                );
                registry.insert_asset(
                    PathBuf::new(),
                    Path::new(&path),
                    preset.to_ron().into_bytes(),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
