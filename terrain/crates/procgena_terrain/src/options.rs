//! Settings for one terrain generator.

use std::fmt;

use glam::DVec2;
use serde::{Deserialize, Serialize};

use crate::noise::{MAX_OCTAVES, Octaves, fbm};
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
/// Start with [`Self::default()`] or a [`crate::TerrainPreset`], or read a
/// `.terrain.yml` file with [`crate::TerrainPreset::from_yaml`]. Call
/// [`validate`](Self::validate) before use; the generator does this for you.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainOptions {
    /// The World seed used when this Terrain is built on its own. Every
    /// random choice comes from it, so the same seed and settings always
    /// give the same terrain. Give neighbouring terrains the same seed so
    /// they share their noise and join more naturally; a different seed
    /// gives unrelated ground that the border has to bend further to join.
    /// A Terrain built as part of a Biome takes the World's seed instead.
    pub default_seed: u32,
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
    /// How narrow a road may get where it cuts deep into the ground, in
    /// metres. Positive, at most `width`.
    pub min_width: f32,
    /// The steepest grade roads climb comfortably, as rise over run. Roads
    /// always prefer flatter ground; above this grade they avoid steep
    /// stretches much more strongly, and may still climb them up to
    /// `max_grade`. Above 0, at most 1.
    pub grade: f32,
    /// The steepest grade a road may climb, as rise over run. At least
    /// `grade`, at most 1.
    ///
    /// Near a border with a steeper-roaded neighbour, both grades rise
    /// towards the neighbour's.
    pub max_grade: f32,
    /// The tightest bend a road should take, as the radius of its centre
    /// line, in metres. Positive, at most [`MAX_ROAD_RADIUS`]. Where the
    /// ground leaves no room for a bend this wide, the road bends tighter
    /// and the bend is listed as a [`SharpBend`](crate::PoiKind::SharpBend)
    /// point of interest.
    pub min_radius: f32,
    /// The width of the shoulder on each side of the road, in metres: level
    /// with the road's edge and falling at its crossfall, before the
    /// cutting or embankment starts. Zero or more, at most
    /// [`MAX_ROAD_SHOULDER`].
    pub shoulder: f32,
    /// How steeply the road surface falls from its crown to each edge, as
    /// fall over run (0.025 is 2.5%). Zero or more, at most
    /// [`MAX_ROAD_CROSSFALL`].
    pub crossfall: f32,
    /// How steeply cuttings rise from the shoulder to the natural ground,
    /// as rise over run (1 is 45°). Above 0, at most
    /// [`MAX_ROAD_SIDE_SLOPE`]. A cutting that doesn't meet the ground
    /// within reach of the road ends in a retaining wall.
    pub cut_slope: f32,
    /// How steeply embankments fall from the shoulder to the natural
    /// ground, as fall over run. Above 0, at most [`MAX_ROAD_SIDE_SLOPE`].
    /// An embankment that doesn't meet the ground within reach of the road
    /// ends in a retaining wall.
    pub fill_slope: f32,
}

/// The largest [`RoadOptions::min_radius`], in metres.
pub const MAX_ROAD_RADIUS: f32 = 500.0;

/// The largest [`RoadOptions::shoulder`], in metres.
pub const MAX_ROAD_SHOULDER: f32 = 5.0;

/// The largest [`RoadOptions::crossfall`], as fall over run.
pub const MAX_ROAD_CROSSFALL: f32 = 0.1;

/// The steepest [`RoadOptions::cut_slope`] and
/// [`RoadOptions::fill_slope`], as rise over run (about 87°).
pub const MAX_ROAD_SIDE_SLOPE: f32 = 20.0;

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
    /// The steepest grade climbed comfortably. Above 0, at most 1.
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
            spacing: 250.0,
            width: 6.0,
            depth: 1.5,
            min_length: 200.0,
        }
    }
}

impl Default for RoadOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            width: 6.0,
            min_width: 4.0,
            grade: 0.1,
            max_grade: 0.2,
            min_radius: 12.0,
            shoulder: 1.0,
            crossfall: 0.025,
            cut_slope: 1.0,
            fill_slope: 1.0 / 1.5,
        }
    }
}

/// The shape of the land: a mean elevation plus layers of relief.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaseOptions {
    /// Mean ground height above the world elevation E, in metres (see
    /// [`Elevation`](crate::Elevation)); above sea level where the terrain
    /// stands on flat land. May be negative.
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
            elevation: 20.0,
            swells: NoiseLayer {
                height: 6.0,
                wavelength: 750.0,
                octaves: 3,
                roughness: 0.45,
            },
            hills: NoiseLayer {
                height: 4.0,
                wavelength: 150.0,
                octaves: 6,
                roughness: 0.5,
            },
            mountains: MountainLayer {
                height: 0.0,
                wavelength: 750.0,
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
            margin: 100.0,
            roughness: 1.0,
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
    /// The layer's value at world position `p` (x and z, metres) for noise
    /// seed `seed`, with no erosion: smooth noise centred on 0, about 95 %
    /// of it within `height` above or below. The same `p` and `seed` always
    /// give the same value.
    ///
    /// Use it to sample a layer as a field of its own, such as a World's
    /// selector field. The layer must have a positive wavelength and 1 to
    /// [`MAX_OCTAVES`] octaves (see [`TerrainOptions::validate`]); a height
    /// of 0 gives 0 everywhere.
    pub fn sample(&self, p: DVec2, seed: u32) -> f32 {
        if self.height == 0.0 {
            return 0.0;
        }
        let octaves = Octaves {
            count: self.octaves,
            gain: self.roughness,
        };
        0.5 * self.height * fbm(p / f64::from(self.wavelength), seed, octaves, 0.0)
    }

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
            "features.roads.min_width",
            road.min_width,
            |v| v > 0.0 && v <= road.width,
            "positive and at most features.roads.width",
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
        check(
            "features.roads.min_radius",
            road.min_radius,
            |v| v > 0.0 && v <= MAX_ROAD_RADIUS,
            "positive and at most MAX_ROAD_RADIUS",
        )?;
        check(
            "features.roads.shoulder",
            road.shoulder,
            |v| (0.0..=MAX_ROAD_SHOULDER).contains(&v),
            "zero or more and at most MAX_ROAD_SHOULDER",
        )?;
        check(
            "features.roads.crossfall",
            road.crossfall,
            |v| (0.0..=MAX_ROAD_CROSSFALL).contains(&v),
            "zero or more and at most MAX_ROAD_CROSSFALL",
        )?;
        check(
            "features.roads.cut_slope",
            road.cut_slope,
            |v| v > 0.0 && v <= MAX_ROAD_SIDE_SLOPE,
            "above 0 and at most MAX_ROAD_SIDE_SLOPE",
        )?;
        check(
            "features.roads.fill_slope",
            road.fill_slope,
            |v| v > 0.0 && v <= MAX_ROAD_SIDE_SLOPE,
            "above 0 and at most MAX_ROAD_SIDE_SLOPE",
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TerrainPreset;

    #[test]
    fn defaults_are_valid() {
        assert_eq!(TerrainOptions::default().validate(), Ok(()));
    }

    /// Reads the options of a `.terrain.yml` file with no base.
    fn read(text: &str) -> Result<TerrainOptions, procgena_core::preset::PresetFileError> {
        TerrainPreset::from_yaml("test", text).map(|preset| preset.terrain)
    }

    #[test]
    fn a_file_round_trips() {
        let mut options = TerrainOptions {
            default_seed: 9,
            ..TerrainOptions::default()
        };
        options.base.mountains.height = 600.0;
        let preset = TerrainPreset {
            terrain: options.clone(),
            ..TerrainPreset::plains().clone()
        };
        assert_eq!(read(&preset.to_yaml(None)).unwrap(), options);
    }

    #[test]
    fn a_partial_file_keeps_other_defaults() {
        let options = read("terrain:\n  base:\n    hills:\n      height: 20\n").unwrap();
        let mut expected = TerrainOptions::default();
        expected.base.hills.height = 20.0;
        assert_eq!(options, expected);
    }

    #[test]
    fn a_file_rejects_unknown_names() {
        for text in [
            "terrain:\n  base:\n    hils: {}\n",
            "terrain:\n  seed: 3\n",
            "tags: [a]\n",
        ] {
            assert!(read(text).is_err(), "accepted {text}");
        }
    }

    #[test]
    fn a_file_rejects_bad_values() {
        for text in [
            "terrain:\n  default_seed: -1\n",
            "terrain:\n  base:\n    hills: 3\n",
            "terrain:\n  base:\n    erosion: -1.0\n",
            "terrain: {default_seed: 1\n",
        ] {
            assert!(read(text).is_err(), "accepted {text}");
        }
    }

    #[test]
    fn bad_values_are_rejected() {
        type Case = (&'static str, fn(&mut TerrainOptions));
        let cases: [Case; 20] = [
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
            ("features.roads.min_width", |o| {
                o.features.roads.min_width = 0.0
            }),
            // Wider than the road.
            ("features.roads.min_width", |o| {
                o.features.roads.min_width = 7.0
            }),
            ("features.roads.grade", |o| o.features.roads.grade = 0.0),
            ("features.roads.max_grade", |o| {
                o.features.roads.max_grade = 0.0
            }),
            // Below the preferred grade.
            ("features.roads.max_grade", |o| {
                o.features.roads.max_grade = 0.05
            }),
            ("features.roads.min_radius", |o| {
                o.features.roads.min_radius = 0.0
            }),
            ("features.roads.shoulder", |o| {
                o.features.roads.shoulder = -1.0
            }),
            ("features.roads.crossfall", |o| {
                o.features.roads.crossfall = 0.5
            }),
            ("features.roads.cut_slope", |o| {
                o.features.roads.cut_slope = 0.0
            }),
            ("features.roads.fill_slope", |o| {
                o.features.roads.fill_slope = 30.0
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
