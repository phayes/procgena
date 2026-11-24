//! Save and load plant and Scatter appearances as text.
//!
//! `.plant.yml` and `.plant_scatter.yml` files read as [`PlantPreset`]s and
//! [`PlantScatterPreset`]s. In Bevy, `procgena_plant_bevy` loads them as
//! assets and reloads them when they change.
//!
//! A file may name a built-in preset as its `base`, give the preset a
//! `name`, `description` and `group`, and holds its settings under its kind:
//! `plant:` or `plant_scatter:`. Every setting is optional. A missing one
//! comes from `base`, so a variant names only what differs. A plant file
//! starts from a [`PlantPreset`] (`meadow_grass` when not given); nest its
//! shape and colours under `segment`, its branches under `levels`, and its
//! accent stems under `accent`:
//!
//! ```yaml
//! # assets/plants/blue_fern.plant.yml
//! base: fern
//! name: Blue fern
//! plant:
//!   segment:
//!     tip_color: [0.20, 0.34, 0.60]
//!   accent:
//!     share: 0.1
//!     height: [0.9, 1.2]
//!     head_length: 0.2
//! ```
//!
//! A Scatter file starts from a [`PlantScatterPreset`] (`meadow` when not
//! given). Its `plants` list replaces the base's plants. Each entry names a
//! plant preset by key, or writes a plant inline, with the same settings as
//! a plant file plus an optional `base`. Each plant grows at its own
//! `density` times its `density_scale` (1 when not given), and the Scatter's
//! `density_scale` scales them all. These entries give 4 seed heads,
//! 1 flower and 35 grass plants per square metre. The last plant with a
//! positive density fills the remaining gaps:
//!
//! ```yaml
//! # assets/scatters/wild_edge.plant_scatter.yml
//! base: summer_tall
//! plant_scatter:
//!   plants:
//!     - plant: seed_heads
//!       density_scale: 0.1
//!     - plant: { base: white_flowers, head_color: "#c08cf2", density: 1.0 }
//!       clustering: 0.8
//!     - plant: { base: summer_tall_grass, tufts: 0.3 }
//!       density_scale: 0.875
//! ```
//!
//! Colours are sRGB `[red, green, blue]` lists, or hexadecimal text in
//! quotes (an unquoted `#` starts a YAML comment). The crate's
//! `assets/plants` and `assets/scatters` folders hold every preset as a file
//! to copy from. Scatter files hold no size, ground or density map: whatever
//! places the Scatter gives it those.

use std::borrow::Cow;
use std::fmt;

use glam::Vec2;
use procgena_core::preset::{Document, FILE_GROUP, PresetFileError, diff, from_value, to_value};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::look::{
    GroundTint, MAX_BRANCHES, MAX_PLANTS, MAX_STEMS_PER_PLANT, PlantAccent, PlantArrangement,
    PlantBranches, PlantParams, PlantSegment, PlantVariation, ScatterPlant, Srgb,
};
use crate::presets::{PlantPreset, PlantScatterPreset};
use crate::scatter::PlantScatterOptions;

/// The kind of a plant file: its settings sit under this key, and its name
/// ends in `.plant.yml`.
const PLANT_KIND: &str = "plant";

/// The kind of a Scatter file: its settings sit under this key, and its name
/// ends in `.plant_scatter.yml`.
const SCATTER_KIND: &str = "plant_scatter";

impl PlantParams {
    /// Reads a plant from the text of a `.plant.yml` file.
    ///
    /// The file's `name`, `description` and `group` are ignored; use
    /// [`PlantPreset::from_yaml`] to keep them.
    ///
    /// # Errors
    ///
    /// Returns [`PlantFileError`] for text that isn't a plant file, unknown
    /// settings or base presets, or out-of-range values.
    pub fn from_yaml(text: &str) -> Result<Self, PlantFileError> {
        Ok(PlantPreset::from_yaml("", text)?.plant)
    }

    /// Checks every setting, as a `.plant.yml` file's are checked.
    ///
    /// # Errors
    ///
    /// Returns [`PlantFileError::Invalid`] naming the first value out of
    /// range.
    pub fn validate(&self) -> Result<(), PlantFileError> {
        validate_plant(self)
    }

    /// Writes every plant setting as the text of a `.plant.yml` file.
    ///
    /// The result names no base preset and does not rely on one.
    #[must_use]
    pub fn to_yaml(&self) -> String {
        Document {
            settings: Some(to_value(self)),
            ..Document::default()
        }
        .to_yaml(PLANT_KIND)
    }
}

impl PlantScatterOptions {
    /// Reads a Scatter's plants and look from the text of a
    /// `.plant_scatter.yml` file.
    ///
    /// The file's `name`, `description` and `group` are ignored; use
    /// [`PlantScatterPreset::from_yaml`] to keep them.
    ///
    /// # Errors
    ///
    /// Returns [`PlantFileError`] for text that isn't a Scatter file, unknown
    /// settings or presets, or out-of-range values.
    pub fn from_yaml(text: &str) -> Result<Self, PlantFileError> {
        Ok(PlantScatterPreset::from_yaml("", text)?.plant_scatter)
    }

    /// Writes the Scatter's plants and look as the text of a
    /// `.plant_scatter.yml` file.
    ///
    /// A plant equal to a [`PlantPreset`] is written as its key. The result names no base
    /// preset and does not rely on one.
    #[must_use]
    pub fn to_yaml(&self) -> String {
        Document {
            settings: Some(to_value(self)),
            ..Document::default()
        }
        .to_yaml(SCATTER_KIND)
    }
}

impl PlantPreset {
    /// Reads a named plant preset from the text of a `.plant.yml` file.
    ///
    /// The caller supplies `key`, usually the file's name without
    /// `.plant.yml`; it is not read from the text. Its name is the file's
    /// `name`, or `key`; its description is the file's `description`, or
    /// empty; its group is the file's `group`, or `"File"`.
    ///
    /// # Errors
    ///
    /// Returns [`PlantFileError`] for text that isn't a plant file, unknown
    /// settings or base presets, or out-of-range values.
    pub fn from_yaml(key: &str, text: &str) -> Result<Self, PlantFileError> {
        let mut document = Document::parse(PLANT_KIND, text)?;
        let base = match &document.base {
            Some(base) => plant_preset(base)?.plant,
            None => PlantParams::default(),
        };
        let file: PlantParamsFile = match document.settings.take() {
            Some(settings) => from_value(settings, PLANT_KIND)?,
            None => PlantParamsFile::default(),
        };
        let [key, name, description, group] = name_parts(key, document);
        Ok(Self {
            key,
            name,
            description,
            group,
            plant: file.resolve(base)?,
        })
    }

    /// Writes the preset's name, description, group and settings as the
    /// text of a `.plant.yml` file that [`from_yaml`](Self::from_yaml) reads
    /// back to an equal preset under the same key.
    ///
    /// With a `base`, the file names it and holds only the settings that
    /// differ from it; without one, only those that differ from
    /// [`PlantParams::default`]. The key is not written: it is the file's
    /// name. The group is left out when it is `"File"`.
    #[must_use]
    pub fn to_yaml(&self, base: Option<&Self>) -> String {
        let (base_key, base_plant) = match base {
            Some(base) => (Some(&*base.key), base.plant),
            None => (None, PlantParams::default()),
        };
        document(
            &self.name,
            &self.description,
            &self.group,
            base_key,
            &to_value(&base_plant),
            &to_value(&self.plant),
        )
        .to_yaml(PLANT_KIND)
    }
}

impl PlantScatterPreset {
    /// Reads a named Scatter preset from the text of a `.plant_scatter.yml`
    /// file.
    ///
    /// The caller supplies `key`, usually the file's name without
    /// `.plant_scatter.yml`; it is not read from the text. Its name is the
    /// file's `name`, or `key`; its description is the file's
    /// `description`, or empty; its group is the file's `group`, or
    /// `"File"`.
    ///
    /// # Errors
    ///
    /// Returns [`PlantFileError`] for text that isn't a Scatter file, unknown
    /// settings or presets, or out-of-range values.
    pub fn from_yaml(key: &str, text: &str) -> Result<Self, PlantFileError> {
        let mut document = Document::parse(SCATTER_KIND, text)?;
        let base = match &document.base {
            Some(base) => scatter_preset(base)?.plant_scatter.clone(),
            None => PlantScatterOptions::default(),
        };
        let file: PlantScatterFile = match document.settings.take() {
            Some(settings) => from_value(settings, SCATTER_KIND)?,
            None => PlantScatterFile::default(),
        };
        let [key, name, description, group] = name_parts(key, document);
        Ok(Self {
            key,
            name,
            description,
            group,
            plant_scatter: file.resolve(base)?,
        })
    }

    /// Writes the preset's name, description, group and settings as the
    /// text of a `.plant_scatter.yml` file that
    /// [`from_yaml`](Self::from_yaml) reads back to an equal preset under
    /// the same key.
    ///
    /// With a `base`, the file names it and holds only the settings that
    /// differ from it; without one, only those that differ from
    /// [`PlantScatterOptions::default`]. The key is not written: it is the
    /// file's name. The group is left out when it is `"File"`.
    #[must_use]
    pub fn to_yaml(&self, base: Option<&Self>) -> String {
        let (base_key, base_scatter) = match base {
            Some(base) => (Some(&*base.key), base.plant_scatter.clone()),
            None => (None, PlantScatterOptions::default()),
        };
        document(
            &self.name,
            &self.description,
            &self.group,
            base_key,
            &to_value(&base_scatter),
            &to_value(&self.plant_scatter),
        )
        .to_yaml(SCATTER_KIND)
    }
}

/// A preset's key, name, description and group, in that order, for a file
/// read under `key`: the name is the file's or the key, the description the
/// file's or empty, and the group the file's or `"File"`.
fn name_parts(key: &str, document: Document) -> [Cow<'static, str>; 4] {
    [
        Cow::Owned(key.to_owned()),
        Cow::Owned(document.name.unwrap_or_else(|| key.to_owned())),
        Cow::Owned(document.description.unwrap_or_default()),
        Cow::Owned(document.group.unwrap_or_else(|| FILE_GROUP.to_owned())),
    ]
}

/// A preset file's parts: its name parts, the key of its base preset if it
/// has one, and the settings in `settings` that differ from `base_settings`.
fn document(
    name: &str,
    description: &str,
    group: &str,
    base_key: Option<&str>,
    base_settings: &Value,
    settings: &Value,
) -> Document {
    Document {
        base: base_key.map(str::to_owned),
        name: Some(name.to_owned()),
        description: (!description.is_empty()).then(|| description.to_owned()),
        group: (group != FILE_GROUP).then(|| group.to_owned()),
        settings: diff(base_settings, settings),
    }
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

impl FileValue for Srgb {
    type File = FileColor;
    fn to_file(&self) -> FileColor {
        FileColor(*self)
    }
    fn from_file(file: FileColor) -> Srgb {
        file.0
    }
}

impl FileValue for PlantArrangement {
    type File = PlantArrangement;
    fn to_file(&self) -> PlantArrangement {
        *self
    }
    fn from_file(file: PlantArrangement) -> PlantArrangement {
        file
    }
}

/// A colour in a file: sRGB as `[0.38, 0.55, 0.16]`, or hex as `"#5c8f24"`
/// (the form colour pickers copy). Written as the list, which keeps every
/// bit of a preset's colour.
#[derive(Debug, Clone, Copy)]
struct FileColor(Srgb);

impl Serialize for FileColor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Srgb([red, green, blue]) = self.0;
        (red, green, blue).serialize(serializer)
    }
}

/// The colour `text` writes as hexadecimal sRGB: `#rgb`, `#rgba`, `#rrggbb`
/// or `#rrggbbaa`, with or without the `#`. Alpha is read and dropped.
fn hex_color(text: &str) -> Option<Srgb> {
    let digits = text.strip_prefix('#').unwrap_or(text);
    if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let nibble = |i: usize| u8::from_str_radix(&digits[i..=i], 16).ok();
    let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    let [r, g, b] = match digits.len() {
        // One digit per channel stands for that digit twice: `a` is `aa`.
        3 | 4 => [nibble(0)? * 17, nibble(1)? * 17, nibble(2)? * 17],
        6 | 8 => [byte(0)?, byte(2)?, byte(4)?],
        _ => return None,
    };
    Some(Srgb([r, g, b].map(|channel| f32::from(channel) / 255.0)))
}

impl<'de> Deserialize<'de> for FileColor {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ColorVisitor;

        impl<'de> serde::de::Visitor<'de> for ColorVisitor {
            type Value = FileColor;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(
                    "a colour: [red, green, blue] in sRGB, or a hex string like \"#5c8f24\"",
                )
            }

            fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<FileColor, E> {
                hex_color(text).map(FileColor).ok_or_else(|| {
                    E::custom(format!("{text:?} is not a hex colour; write \"#rrggbb\""))
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
                Ok(FileColor(Srgb::new(r, g, b)))
            }
        }

        deserializer.deserialize_any(ColorVisitor)
    }
}

/// Defines optional file settings and their read/write helpers together. Listing each
/// setting once keeps serialisation and application in sync.
///
/// Settings in `extra` are written after the plain settings and resolved
/// separately. Plain settings map directly to fields of `$ty`.
macro_rules! settings_file {
    (
        $(#[$meta:meta])*
        struct $file:ident for $ty:ty {
            $( $name:ident : $vty:ty ),* $(,)?
        }
        extra {
            $( $(#[$emeta:meta])* $ename:ident : $ety:ty ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Default, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub(crate) struct $file {
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
    /// A main stem's or branch level's `segment` settings in a file, and how
    /// a [`PlantSegment`] is serialised.
    struct PlantSegmentFile for PlantSegment {
        profile: f32,
        stiffness: f32,
        droop: f32,
        twist: f32,
        fold: f32,
        root_color: Srgb,
        tip_color: Srgb,
    }
    extra {}
}

settings_file! {
    /// One entry in a plant's `levels` list in a file, and how a
    /// [`PlantBranches`] is serialised.
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
    /// A plant's settings in a file: under `plant:` in a `.plant.yml` file,
    /// or written inline in a Scatter file, and how a [`PlantParams`] is
    /// serialised.
    struct PlantParamsFile for PlantParams {
        default_seed: u32,
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
        head_color: Srgb,
        tufts: f32,
        tuft_size: f32,
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
    /// A plant's `accent` settings in a file, and how a [`PlantAccent`] is
    /// serialised.
    struct PlantAccentFile for PlantAccent {
        share: f32,
        height: Vec2,
        width: f32,
        lean: f32,
        translucency: f32,
        head_length: f32,
        head_width: f32,
        head_color: Srgb,
    }
    extra {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        segment: Option<PlantSegmentFile>,
    }
}

settings_file! {
    /// A Scatter's `variation` settings in a file, and how a
    /// [`PlantVariation`] is serialised.
    struct PlantVariationFile for PlantVariation {
        patch_color: Srgb,
        patch_amount: f32,
        patch_size: f32,
        dry_tips: f32,
        dead_share: f32,
        bare_share: f32,
        bare_patch_size: f32,
    }
    extra {}
}

settings_file! {
    /// A Scatter's `ground_tint` settings in a file, and how a
    /// [`GroundTint`] is serialised.
    struct GroundTintFile for GroundTint {
        color: Srgb,
        amount: f32,
    }
    extra {}
}

settings_file! {
    /// A Scatter's settings in a file: under `plant_scatter:` in a
    /// `.plant_scatter.yml` file, and how a [`PlantScatterOptions`] is
    /// serialised.
    struct PlantScatterFile for PlantScatterOptions {
        default_seed: u32,
        wind_scale: f32,
        density_scale: f32,
        toon_bands: u32,
    }
    extra {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        variation: Option<PlantVariationFile>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ground_tint: Option<GroundTintFile>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plants: Option<Vec<ScatterPlantFile>>,
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

impl PlantParamsFile {
    /// Captures every plant setting, including both levels and the accent,
    /// so the file does not rely on a base preset.
    fn from_plant(plant: &PlantParams) -> Self {
        Self {
            segment: Some(PlantSegmentFile::plain_from(&plant.segment)),
            levels: Some(
                plant
                    .levels
                    .iter()
                    .map(PlantBranchesFile::from_branches)
                    .collect(),
            ),
            accent: Some(PlantAccentFile::from_accent(&plant.accent)),
            ..Self::plain_from(plant)
        }
    }

    /// Applies the file's settings over `plant`, then validates the result.
    ///
    /// The `levels` list applies entry by entry: its first entry over the
    /// first level, its second over the second.
    fn resolve(&self, mut plant: PlantParams) -> Result<PlantParams, PlantFileError> {
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

impl PlantScatterFile {
    /// Captures every Scatter setting.
    fn from_scatter(scatter: &PlantScatterOptions) -> Self {
        Self {
            variation: Some(PlantVariationFile::plain_from(&scatter.variation)),
            ground_tint: Some(GroundTintFile::plain_from(&scatter.ground_tint)),
            plants: Some(
                scatter
                    .plants
                    .iter()
                    .map(ScatterPlantFile::from_entry)
                    .collect(),
            ),
            ..Self::plain_from(scatter)
        }
    }

    /// Applies the file's settings over `scatter`, then validates the result.
    /// The `plants` list replaces the base's plants whole.
    fn resolve(
        &self,
        mut scatter: PlantScatterOptions,
    ) -> Result<PlantScatterOptions, PlantFileError> {
        // Lay the settings and plants over the base.
        self.apply_plain(&mut scatter);
        if let Some(variation) = &self.variation {
            variation.apply_plain(&mut scatter.variation);
        }
        if let Some(tint) = &self.ground_tint {
            tint.apply_plain(&mut scatter.ground_tint);
        }
        if let Some(plants) = &self.plants {
            if plants.len() > MAX_PLANTS {
                return Err(PlantFileError::Invalid("a Scatter grows at most 16 plants"));
            }
            scatter.plants = Cow::Owned(
                plants
                    .iter()
                    .map(ScatterPlantFile::resolve)
                    .collect::<Result<_, _>>()?,
            );
        }

        validate_scatter(&scatter)?;
        Ok(scatter)
    }
}

/// Checks a Scatter's plants, each entry's scale and clustering, and the
/// Scatter-wide settings.
pub(crate) fn validate_scatter(scatter: &PlantScatterOptions) -> Result<(), PlantFileError> {
    // Check the plants and how each one is mixed in.
    if scatter.plants.len() > MAX_PLANTS {
        return Err(PlantFileError::Invalid("a Scatter grows at most 16 plants"));
    }
    for entry in scatter.plants.iter() {
        validate_entry(entry.density_scale, entry.clustering)?;
        validate_plant(&entry.plant)?;
    }

    // Check the Scatter-wide settings.
    if !scatter.plants.iter().any(|entry| entry.density() > 0.0) {
        return Err(PlantFileError::Invalid(
            "a Scatter needs a plant with a density above 0",
        ));
    }
    if scatter.density_scale < 0.0 || !scatter.density_scale.is_finite() {
        return Err(PlantFileError::Invalid("density_scale must be at least 0"));
    }
    if scatter.wind_scale < 0.0 || scatter.wind_scale.is_nan() {
        return Err(PlantFileError::Invalid("wind_scale must be at least 0"));
    }
    validate_variation(&scatter.variation)
}

/// Checks one Scatter entry's density scale and clustering.
fn validate_entry(density_scale: f32, clustering: f32) -> Result<(), PlantFileError> {
    if density_scale < 0.0 || !density_scale.is_finite() {
        return Err(PlantFileError::Invalid(
            "a plant's density_scale must be at least 0",
        ));
    }
    if !(0.0..=1.0).contains(&clustering) {
        return Err(PlantFileError::Invalid(
            "a plant's clustering must be from 0 to 1",
        ));
    }
    Ok(())
}

/// Looks up a plant preset by key, reporting an unknown key as an error.
fn plant_preset(key: &str) -> Result<&'static PlantPreset, PlantFileError> {
    PlantPreset::named(key).ok_or_else(|| PlantFileError::UnknownPlant(key.to_owned()))
}

/// Looks up a Scatter preset by key, reporting an unknown key as an error.
fn scatter_preset(key: &str) -> Result<&'static PlantScatterPreset, PlantFileError> {
    PlantScatterPreset::named(key).ok_or_else(|| PlantFileError::UnknownScatter(key.to_owned()))
}

/// A built-in plant key or inline plant settings in a Scatter file.
/// For example, `fern` uses the preset unchanged;
/// `{ base: fern, lean: 0.2 }` changes just its lean. Keys refer to built-in
/// presets, not paths to other plant files.
#[derive(Debug)]
enum PlantRef {
    Preset(String),
    Inline {
        base: Option<String>,
        file: Box<PlantParamsFile>,
    },
}

impl PlantRef {
    /// The key of the preset equal to `plant`, or the plant written out.
    fn from_plant(plant: &PlantParams) -> Self {
        match PlantPreset::all().find(|preset| preset.plant == *plant) {
            Some(preset) => Self::Preset(preset.key.to_string()),
            None => Self::Inline {
                base: None,
                file: Box::new(PlantParamsFile::from_plant(plant)),
            },
        }
    }

    /// The plant: the named preset's, or the inline settings over their base
    /// preset or over meadow grass.
    fn resolve(&self) -> Result<PlantParams, PlantFileError> {
        match self {
            Self::Preset(key) => Ok(plant_preset(key)?.plant),
            Self::Inline { base, file } => {
                let base = match base {
                    Some(key) => plant_preset(key)?.plant,
                    None => PlantParams::default(),
                };
                file.resolve(base)
            }
        }
    }
}

impl Serialize for PlantRef {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        /// An inline plant with its base written first.
        #[derive(Serialize)]
        struct WithBase<'a> {
            base: &'a str,
            #[serde(flatten)]
            file: &'a PlantParamsFile,
        }

        match self {
            Self::Preset(key) => serializer.serialize_str(key),
            Self::Inline { base: None, file } => file.serialize(serializer),
            Self::Inline {
                base: Some(base),
                file,
            } => WithBase { base, file }.serialize(serializer),
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
                    "a plant: a plant preset's key such as \"fern\", or plant settings \
                     such as { base: fern, lean: 0.2 }",
                )
            }

            fn visit_str<E: serde::de::Error>(self, key: &str) -> Result<PlantRef, E> {
                Ok(PlantRef::Preset(key.to_owned()))
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<PlantRef, A::Error> {
                // Split off the base, then read the rest as plant settings.
                let mut base = None;
                let mut settings = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if key == "base" {
                        base = Some(map.next_value::<String>()?);
                    } else {
                        settings.insert(key, map.next_value::<Value>()?);
                    }
                }
                let file = PlantParamsFile::deserialize(Value::Object(settings))
                    .map_err(serde::de::Error::custom)?;
                Ok(PlantRef::Inline {
                    base,
                    file: Box::new(file),
                })
            }
        }

        deserializer.deserialize_any(PlantVisitor)
    }
}

/// One plant in a Scatter file, with its density scale and clustering, and
/// how a [`ScatterPlant`] is serialised. The nested `plant` accepts plant
/// settings only; Scatter-wide settings such as `density_scale` belong
/// outside the plant list.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScatterPlantFile {
    plant: PlantRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    density_scale: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    clustering: Option<f32>,
}

impl ScatterPlantFile {
    /// The entry as written: the plant by key or inline, and its scale and
    /// clustering when they differ from 1 and 0.
    fn from_entry(entry: &ScatterPlant) -> Self {
        Self {
            plant: PlantRef::from_plant(&entry.plant),
            density_scale: (entry.density_scale != 1.0).then_some(entry.density_scale),
            clustering: (entry.clustering != 0.0).then_some(entry.clustering),
        }
    }

    /// The entry, with a density scale of 1 and no clustering when not
    /// given, validated.
    fn resolve(&self) -> Result<ScatterPlant, PlantFileError> {
        let density_scale = self.density_scale.unwrap_or(1.0);
        let clustering = self.clustering.unwrap_or(0.0);
        validate_entry(density_scale, clustering)?;
        Ok(ScatterPlant::new(self.plant.resolve()?)
            .scaled(density_scale)
            .clustered(clustering))
    }
}

// Serde for the public options types goes through the file structs above:
// missing settings come from the type's default, and values are checked as
// a file's are.

impl From<PlantParams> for PlantParamsFile {
    fn from(plant: PlantParams) -> Self {
        Self::from_plant(&plant)
    }
}

impl TryFrom<PlantParamsFile> for PlantParams {
    type Error = PlantFileError;
    fn try_from(file: PlantParamsFile) -> Result<Self, PlantFileError> {
        file.resolve(Self::default())
    }
}

impl From<PlantSegment> for PlantSegmentFile {
    fn from(segment: PlantSegment) -> Self {
        Self::plain_from(&segment)
    }
}

impl TryFrom<PlantSegmentFile> for PlantSegment {
    type Error = PlantFileError;
    fn try_from(file: PlantSegmentFile) -> Result<Self, PlantFileError> {
        let mut segment = Self::default();
        file.apply(&mut segment);
        validate_segment(&segment)?;
        Ok(segment)
    }
}

impl From<PlantBranches> for PlantBranchesFile {
    fn from(branches: PlantBranches) -> Self {
        Self::from_branches(&branches)
    }
}

impl TryFrom<PlantBranchesFile> for PlantBranches {
    type Error = PlantFileError;
    fn try_from(file: PlantBranchesFile) -> Result<Self, PlantFileError> {
        let mut branches = Self::default();
        file.apply(&mut branches);
        validate_branches(&branches)?;
        Ok(branches)
    }
}

impl From<PlantAccent> for PlantAccentFile {
    fn from(accent: PlantAccent) -> Self {
        Self::from_accent(&accent)
    }
}

impl TryFrom<PlantAccentFile> for PlantAccent {
    type Error = PlantFileError;
    fn try_from(file: PlantAccentFile) -> Result<Self, PlantFileError> {
        let mut accent = Self::default();
        file.apply(&mut accent);
        validate_accent(&accent)?;
        Ok(accent)
    }
}

impl From<PlantVariation> for PlantVariationFile {
    fn from(variation: PlantVariation) -> Self {
        Self::plain_from(&variation)
    }
}

impl TryFrom<PlantVariationFile> for PlantVariation {
    type Error = PlantFileError;
    fn try_from(file: PlantVariationFile) -> Result<Self, PlantFileError> {
        let mut variation = Self::default();
        file.apply_plain(&mut variation);
        validate_variation(&variation)?;
        Ok(variation)
    }
}

impl From<GroundTint> for GroundTintFile {
    fn from(tint: GroundTint) -> Self {
        Self::plain_from(&tint)
    }
}

impl TryFrom<GroundTintFile> for GroundTint {
    type Error = PlantFileError;
    fn try_from(file: GroundTintFile) -> Result<Self, PlantFileError> {
        let mut tint = Self::default();
        file.apply_plain(&mut tint);
        Ok(tint)
    }
}

impl From<ScatterPlant> for ScatterPlantFile {
    fn from(entry: ScatterPlant) -> Self {
        Self::from_entry(&entry)
    }
}

impl TryFrom<ScatterPlantFile> for ScatterPlant {
    type Error = PlantFileError;
    fn try_from(file: ScatterPlantFile) -> Result<Self, PlantFileError> {
        file.resolve()
    }
}

impl From<PlantScatterOptions> for PlantScatterFile {
    fn from(scatter: PlantScatterOptions) -> Self {
        Self::from_scatter(&scatter)
    }
}

impl TryFrom<PlantScatterFile> for PlantScatterOptions {
    type Error = PlantFileError;
    fn try_from(file: PlantScatterFile) -> Result<Self, PlantFileError> {
        file.resolve(Self::default())
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

/// Checks that the variation's patch sizes are above 0.
fn validate_variation(variation: &PlantVariation) -> Result<(), PlantFileError> {
    if !(variation.patch_size > 0.0 && variation.bare_patch_size > 0.0) {
        return Err(PlantFileError::Invalid(
            "patch_size and bare_patch_size must be above 0",
        ));
    }
    Ok(())
}

/// An error reading plant or Scatter settings from a YAML file or through
/// serde.
///
/// Distinguishes unreadable files, text that isn't a preset file, unknown
/// presets and invalid setting values.
#[derive(Debug)]
#[non_exhaustive]
pub enum PlantFileError {
    /// The file could not be read.
    Io(std::io::Error),
    /// The text is not a plant or Scatter file: it isn't YAML (the message
    /// gives the line and column), it has a top-level key other than
    /// `base`, `name`, `description`, `group` and its kind, or a setting is
    /// unknown or of the wrong type (the message names its path, such as
    /// `plant.segment.tip_colour`).
    File(PresetFileError),
    /// A Scatter's `base` names no [`PlantScatterPreset`].
    UnknownScatter(String),
    /// A plant's `base`, or a plant given by key, names no [`PlantPreset`].
    UnknownPlant(String),
    /// A value is out of range.
    Invalid(&'static str),
}

impl From<PresetFileError> for PlantFileError {
    fn from(error: PresetFileError) -> Self {
        Self::File(error)
    }
}

impl fmt::Display for PlantFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let keys =
            |keys: &mut dyn Iterator<Item = &'static str>| keys.collect::<Vec<_>>().join(", ");
        match self {
            Self::Io(error) => write!(f, "could not read the grass file: {error}"),
            Self::File(error) => write!(f, "not a grass file: {error}"),
            Self::UnknownScatter(key) => write!(
                f,
                "no Scatter preset called {key:?}; the Scatter presets are {}",
                keys(&mut PlantScatterPreset::all().map(|preset| &*preset.key))
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

#[cfg(test)]
mod tests;
