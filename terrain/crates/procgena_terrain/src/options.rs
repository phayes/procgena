//! Settings for one terrain generator.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::noise::MAX_OCTAVES;
use crate::ports::MAX_PORT_WIDTH;

/// The longest feature wavelength a noise layer accepts, in metres.
pub const MAX_WAVELENGTH: f32 = 1.0e6;

/// The largest height a layer or elevation accepts, in metres.
pub const MAX_HEIGHT: f32 = 1.0e4;

/// The widest edge margin accepted, in metres.
pub const MAX_MARGIN: f32 = 1.0e4;

/// The strongest erosion damping accepted.
pub const MAX_EROSION: f32 = 10.0;

/// The most river sources per square kilometre accepted.
pub const MAX_RIVER_DENSITY: f32 = 20.0;

/// Settings for one terrain generator. All lengths and heights are in metres.
///
/// Every field has a default, so JSON may name only the settings it changes
/// (see [`from_json`](Self::from_json)). Call [`validate`](Self::validate)
/// before use; the generator does this for you.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainOptions {
    /// Seed for every random choice: the world's seed. The same seed and
    /// settings always give the same terrain. Give neighbouring terrains
    /// the same seed so they share their noise and join more naturally; a
    /// different seed gives unrelated ground that the border has to bend
    /// further to join.
    pub seed: u32,
    /// The shape of the land before any features are cut into it.
    pub base: BaseOptions,
    /// How this terrain meets its neighbours along region borders.
    pub edges: EdgeOptions,
    /// Rivers and roads cut into the land.
    pub features: FeatureOptions,
}

/// Rivers and roads.
///
/// Features are made only for regions with a polygon boundary.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureOptions {
    /// Rivers that start in this terrain and flow downhill.
    pub rivers: RiverOptions,
    /// Roads that link this terrain's border crossings.
    pub roads: RoadOptions,
}

/// Rivers: each starts high in the terrain and flows downhill until it ends
/// in a pond, joins another river, or crosses into a neighbour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RiverOptions {
    /// River sources per square kilometre, from 0 (no rivers of its own) to
    /// [`MAX_RIVER_DENSITY`]. Rivers flowing in from neighbours are made
    /// either way.
    pub density: f32,
    /// The least distance between two sources, in metres. Positive.
    pub spacing: f32,
    /// River width, in metres. Positive, at most [`MAX_PORT_WIDTH`](crate::MAX_PORT_WIDTH).
    pub width: f32,
    /// Depth of the river bed below the water, in metres. Positive, at most
    /// [`MAX_HEIGHT`].
    pub depth: f32,
    /// Rivers shorter than this, in metres, that end in a pond are left out.
    /// Zero or more.
    pub min_length: f32,
}

/// Roads: each border crossing links to a hub near the middle of the
/// region, or to a road already linked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoadOptions {
    /// Whether this terrain asks for road crossings on its borders. Roads
    /// asked for by a neighbour are made either way.
    pub enabled: bool,
    /// Road width, in metres. Positive, at most [`MAX_PORT_WIDTH`](crate::MAX_PORT_WIDTH).
    pub width: f32,
    /// The grade roads keep to where they can, as rise over run. Roads may
    /// climb more steeply, at a cost, up to `max_grade`. Above 0, at most 1.
    pub grade: f32,
    /// The steepest grade a road may climb, as rise over run. At least
    /// `grade`, at most 1.
    ///
    /// Near a border with a steeper-roaded neighbour, both grades rise
    /// towards the neighbour's.
    pub max_grade: f32,
}

impl RoadOptions {
    /// The road grades.
    pub fn grades(&self) -> RoadGrades {
        RoadGrades {
            grade: self.grade,
            max_grade: self.max_grade,
        }
    }
}

/// The grades a road keeps to, as rise over run (see [`RoadOptions`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadGrades {
    /// The grade kept to where possible. Above 0, at most 1.
    pub grade: f32,
    /// The steepest grade allowed. At least `grade`, at most 1.
    pub max_grade: f32,
}

impl RoadGrades {
    /// The steeper of each grade of `self` and `other`.
    #[must_use]
    pub fn steepest(self, other: Self) -> Self {
        Self {
            grade: self.grade.max(other.grade),
            max_grade: self.max_grade.max(other.max_grade),
        }
    }

    /// Returns true if both grades are in range.
    pub(crate) fn is_valid(&self) -> bool {
        self.grade > 0.0 && self.grade <= self.max_grade && self.max_grade <= 1.0
    }
}

impl Default for RiverOptions {
    fn default() -> Self {
        Self {
            density: 0.4,
            spacing: 500.0,
            width: 6.0,
            depth: 1.5,
            min_length: 400.0,
        }
    }
}

impl Default for RoadOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            width: 6.0,
            grade: 0.1,
            max_grade: 0.2,
        }
    }
}

/// The shape of the land: a mean elevation plus layers of relief.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaseOptions {
    /// Mean ground height, in metres. May be negative.
    pub elevation: f32,
    /// Broad, gentle rises and dips.
    pub swells: NoiseLayer,
    /// Hills: smaller and steeper than swells.
    pub hills: NoiseLayer,
    /// Mountain ranges, only where the range mask allows.
    pub mountains: MountainLayer,
    /// How worn the hills look, from 0 (raw noise) to [`MAX_EROSION`].
    /// Higher values keep slopes smooth and put detail in valleys and on
    /// crests, and lower the hills somewhat.
    pub erosion: f32,
}

/// One layer of rolling relief.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoiseLayer {
    /// Typical height of the layer, in metres: about 95 % of the ground lies
    /// within this distance above or below the layer's mean. 0 turns the
    /// layer off. At most [`MAX_HEIGHT`].
    pub height: f32,
    /// Typical distance between neighbouring rises, in metres. Must be
    /// positive and at most [`MAX_WAVELENGTH`].
    pub wavelength: f32,
    /// Number of noise layers, 1 through [`MAX_OCTAVES`]. More octaves add
    /// finer detail and cost more to sample.
    pub octaves: u32,
    /// How strong each finer octave is compared with the one before, from 0
    /// (smooth) to 1 (rough). Must be below 1.
    pub roughness: f32,
}

/// Mountain ranges: sharp ridged peaks inside a mask of range areas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MountainLayer {
    /// Typical peak height above the surrounding land, in metres. 0 turns
    /// mountains off. At most [`MAX_HEIGHT`].
    pub height: f32,
    /// Typical distance between neighbouring ridges, in metres. Must be
    /// positive and at most [`MAX_WAVELENGTH`]. Ranges are about four times
    /// this size.
    pub wavelength: f32,
    /// Share of the land covered by mountain ranges, from 0 to 1.
    pub coverage: f32,
    /// Number of ridge octaves, 1 through [`MAX_OCTAVES`].
    pub octaves: u32,
}

/// How a terrain meets its neighbours.
///
/// Along a shared border both neighbours agree on one height profile. Each
/// bends its own terrain to meet that profile within its margin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeOptions {
    /// How strongly this terrain keeps its own shape at a border, compared
    /// with its neighbour. The agreed border height is a mix of both sides'
    /// own heights, weighted by stiffness. Must be positive.
    pub stiffness: f32,
    /// How far inside its region, in metres, this terrain may bend to meet a
    /// border. Must be positive and at most [`MAX_MARGIN`]. Wider margins give
    /// gentler transitions.
    pub margin: f32,
    /// Height in metres of the small irregularities added along a border so
    /// it doesn't look ruled. The smaller of the two neighbours' values is
    /// used. Must be zero or positive.
    pub roughness: f32,
}

impl Default for BaseOptions {
    fn default() -> Self {
        Self {
            elevation: 40.0,
            swells: NoiseLayer {
                height: 12.0,
                wavelength: 1500.0,
                octaves: 3,
                roughness: 0.45,
            },
            hills: NoiseLayer {
                height: 8.0,
                wavelength: 300.0,
                octaves: 6,
                roughness: 0.5,
            },
            mountains: MountainLayer {
                height: 0.0,
                wavelength: 1500.0,
                coverage: 0.4,
                octaves: 6,
            },
            erosion: 0.5,
        }
    }
}

impl Default for EdgeOptions {
    fn default() -> Self {
        Self {
            stiffness: 1.0,
            margin: 200.0,
            roughness: 2.0,
        }
    }
}

/// A setting that [`TerrainOptions::validate`] rejected.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum OptionsError {
    /// The setting is NaN or infinite.
    NotFinite {
        /// Path of the setting, such as `base.hills.height`.
        field: &'static str,
    },
    /// The setting is outside its allowed range.
    OutOfRange {
        /// Path of the setting, such as `base.hills.height`.
        field: &'static str,
        /// The rejected value.
        value: f64,
        /// What the setting must be, such as "from 0 to 1".
        expected: &'static str,
    },
}

impl fmt::Display for OptionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFinite { field } => write!(f, "{field} must be a finite number"),
            Self::OutOfRange {
                field,
                value,
                expected,
            } => {
                write!(f, "{field} is {value}, but must be {expected}")
            }
        }
    }
}

impl std::error::Error for OptionsError {}

/// Checks that `value` is finite and inside `range`.
fn check(
    field: &'static str,
    value: f32,
    ok: impl Fn(f32) -> bool,
    expected: &'static str,
) -> Result<(), OptionsError> {
    if !value.is_finite() {
        return Err(OptionsError::NotFinite { field });
    }
    if !ok(value) {
        return Err(OptionsError::OutOfRange {
            field,
            value: f64::from(value),
            expected,
        });
    }
    Ok(())
}

/// Checks an octave count.
fn check_octaves(field: &'static str, octaves: u32) -> Result<(), OptionsError> {
    if (1..=MAX_OCTAVES).contains(&octaves) {
        Ok(())
    } else {
        Err(OptionsError::OutOfRange {
            field,
            value: f64::from(octaves),
            expected: "from 1 to MAX_OCTAVES",
        })
    }
}

impl NoiseLayer {
    /// Checks the layer, reporting errors under `fields`: the paths of its
    /// height, wavelength, octaves and roughness.
    fn validate(&self, fields: [&'static str; 4]) -> Result<(), OptionsError> {
        let [height, wavelength, octaves, roughness] = fields;
        check(
            height,
            self.height,
            |v| (0.0..=MAX_HEIGHT).contains(&v),
            "from 0 to MAX_HEIGHT",
        )?;
        check(
            wavelength,
            self.wavelength,
            |v| v > 0.0 && v <= MAX_WAVELENGTH,
            "positive and at most MAX_WAVELENGTH",
        )?;
        check_octaves(octaves, self.octaves)?;
        check(
            roughness,
            self.roughness,
            |v| (0.0..1.0).contains(&v),
            "from 0 up to (not including) 1",
        )
    }
}

impl TerrainOptions {
    /// Checks every setting and returns the first one out of range. Values
    /// are never clamped.
    pub fn validate(&self) -> Result<(), OptionsError> {
        let b = &self.base;
        let height = |v: f32| (0.0..=MAX_HEIGHT).contains(&v);
        let wavelength = |v: f32| v > 0.0 && v <= MAX_WAVELENGTH;
        check(
            "base.elevation",
            b.elevation,
            |v| v.abs() <= MAX_HEIGHT,
            "within ±MAX_HEIGHT",
        )?;
        check(
            "base.erosion",
            b.erosion,
            |v| (0.0..=MAX_EROSION).contains(&v),
            "from 0 to MAX_EROSION",
        )?;

        // Rolling layers.
        b.swells.validate([
            "base.swells.height",
            "base.swells.wavelength",
            "base.swells.octaves",
            "base.swells.roughness",
        ])?;
        b.hills.validate([
            "base.hills.height",
            "base.hills.wavelength",
            "base.hills.octaves",
            "base.hills.roughness",
        ])?;

        // Mountains.
        let m = &b.mountains;
        check(
            "base.mountains.height",
            m.height,
            height,
            "from 0 to MAX_HEIGHT",
        )?;
        check(
            "base.mountains.wavelength",
            m.wavelength,
            wavelength,
            "positive and at most MAX_WAVELENGTH",
        )?;
        check(
            "base.mountains.coverage",
            m.coverage,
            |v| (0.0..=1.0).contains(&v),
            "from 0 to 1",
        )?;
        check_octaves("base.mountains.octaves", m.octaves)?;

        // Edges.
        let e = &self.edges;
        check("edges.stiffness", e.stiffness, |v| v > 0.0, "positive")?;
        check(
            "edges.margin",
            e.margin,
            |v| v > 0.0 && v <= MAX_MARGIN,
            "positive and at most MAX_MARGIN",
        )?;
        check(
            "edges.roughness",
            e.roughness,
            |v| (0.0..=MAX_HEIGHT).contains(&v),
            "from 0 to MAX_HEIGHT",
        )?;

        // Features.
        let width = |v: f32| v > 0.0 && v <= MAX_PORT_WIDTH;
        let r = &self.features.rivers;
        check(
            "features.rivers.density",
            r.density,
            |v| (0.0..=MAX_RIVER_DENSITY).contains(&v),
            "from 0 to MAX_RIVER_DENSITY",
        )?;
        check(
            "features.rivers.spacing",
            r.spacing,
            |v| v > 0.0 && v <= MAX_WAVELENGTH,
            "positive and at most MAX_WAVELENGTH",
        )?;
        check(
            "features.rivers.width",
            r.width,
            width,
            "positive and at most MAX_PORT_WIDTH",
        )?;
        check(
            "features.rivers.depth",
            r.depth,
            |v| v > 0.0 && v <= MAX_HEIGHT,
            "positive and at most MAX_HEIGHT",
        )?;
        check(
            "features.rivers.min_length",
            r.min_length,
            |v| (0.0..=MAX_WAVELENGTH).contains(&v),
            "from 0 to MAX_WAVELENGTH",
        )?;
        let road = &self.features.roads;
        check(
            "features.roads.width",
            road.width,
            width,
            "positive and at most MAX_PORT_WIDTH",
        )?;
        check(
            "features.roads.grade",
            road.grade,
            |v| v > 0.0 && v <= 1.0,
            "above 0 and at most 1",
        )?;
        check(
            "features.roads.max_grade",
            road.max_grade,
            |v| v >= road.grade && v <= 1.0,
            "at least features.roads.grade and at most 1",
        )?;
        Ok(())
    }

    /// Reads settings from JSON. Settings left out keep their defaults, at
    /// any depth: `{"base": {"hills": {"height": 20}}}` changes only the hill
    /// height. Unknown setting names are an error.
    ///
    /// This does not validate the values; see [`validate`](Self::validate).
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let source: serde_json::Value = serde_json::from_str(json)?;
        let mut merged = serde_json::to_value(Self::default())?;
        merge_json(&mut merged, source);
        serde_json::from_value(merged)
    }

    /// Returns all settings as indented JSON, including settings at their
    /// defaults.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("terrain options always serialize to JSON")
    }
}

/// Copies supplied JSON values over the defaults in `target`. Objects merge
/// recursively; anything else replaces the whole value.
fn merge_json(target: &mut serde_json::Value, source: serde_json::Value) {
    use serde_json::Value;
    match (target, source) {
        (Value::Object(target), Value::Object(source)) => {
            for (key, value) in source {
                match target.get_mut(&key) {
                    Some(slot) => merge_json(slot, value),
                    None => {
                        target.insert(key, value);
                    }
                }
            }
        }
        (target, source) => *target = source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        assert_eq!(TerrainOptions::default().validate(), Ok(()));
    }

    #[test]
    fn json_round_trips() {
        let mut options = TerrainOptions {
            seed: 9,
            ..TerrainOptions::default()
        };
        options.base.mountains.height = 600.0;
        assert_eq!(
            TerrainOptions::from_json(&options.to_json()).unwrap(),
            options
        );
    }

    #[test]
    fn partial_json_keeps_other_defaults() {
        let options = TerrainOptions::from_json(r#"{"base": {"hills": {"height": 20}}}"#).unwrap();
        let mut expected = TerrainOptions::default();
        expected.base.hills.height = 20.0;
        assert_eq!(options, expected);
    }

    #[test]
    fn unknown_names_are_rejected() {
        assert!(TerrainOptions::from_json(r#"{"base": {"hils": {}}}"#).is_err());
    }

    #[test]
    fn bad_values_are_rejected() {
        type Case = (&'static str, fn(&mut TerrainOptions));
        let cases: [Case; 13] = [
            ("base.elevation", |o| o.base.elevation = f32::NAN),
            ("base.erosion", |o| o.base.erosion = -1.0),
            ("base.swells.height", |o| o.base.swells.height = -1.0),
            ("base.hills.wavelength", |o| o.base.hills.wavelength = 0.0),
            ("base.hills.roughness", |o| o.base.hills.roughness = 1.0),
            ("base.mountains.coverage", |o| {
                o.base.mountains.coverage = 1.5
            }),
            ("base.mountains.octaves", |o| o.base.mountains.octaves = 0),
            ("edges.stiffness", |o| o.edges.stiffness = 0.0),
            ("edges.margin", |o| o.edges.margin = f32::INFINITY),
            ("features.rivers.width", |o| o.features.rivers.width = 100.0),
            ("features.roads.grade", |o| o.features.roads.grade = 0.0),
            ("features.roads.max_grade", |o| {
                o.features.roads.max_grade = 0.0
            }),
            // Below the preferred grade.
            ("features.roads.max_grade", |o| {
                o.features.roads.max_grade = 0.05
            }),
        ];
        for (field, change) in cases {
            let mut options = TerrainOptions::default();
            change(&mut options);
            let error = options.validate().unwrap_err();
            let reported = match error {
                OptionsError::NotFinite { field } | OptionsError::OutOfRange { field, .. } => field,
            };
            assert_eq!(reported, field);
        }
    }
}
