//! Settings for one World.

use std::collections::BTreeSet;
use std::fmt;

use procgena_terrain::{
    Convergence, ElevationError, ElevationOptions, HeightBump, MAX_OCTAVES, NoiseLayer,
    RegionGraphError, RegionGraphOptions, relief_meeting_is_valid,
};
use serde::{Deserialize, Serialize};

/// Settings for a [`World`](crate::World): its seed, its scale, how it is
/// split into regions, the fields that choose each region's Biome, the
/// rules that turn field values into Biomes, and pinned regions.
///
/// Landscape lengths and heights here (the world elevation E, the selector
/// fields and the rules' elevation bounds) are written at reference scale
/// 1, and [`scale`](Self::scale) stretches them when the World is built.
/// Region sizes and border warp are in real metres and never scaled.
///
/// Start from [`Self::default()`] (the medium scale class) or a
/// [`WorldPreset`](crate::WorldPreset), or read a `.world.yml` file with
/// [`WorldPreset::from_yaml`](crate::WorldPreset::from_yaml).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldOptions {
    /// The World seed. Everything in the World comes from it: the region
    /// graph, E, the selector fields, every region's Biome and Terrain
    /// seeds. The same seed and settings always give the same World.
    pub seed: u32,
    /// How much the World stretches its landscape: see [`WorldScale`].
    pub scale: WorldScale,
    /// How the plane is split into regions, in real metres (not scaled).
    pub regions: RegionGraphOptions,
    /// The world elevation E every region stands on, at reference scale.
    /// Its noise layers, tilt and bumps (centres, radii and heights) are
    /// stretched by the scale. Its `max_slope` is not: it is checked
    /// against the stretched field.
    pub elevation: ElevationOptions,
    /// How neighbouring terrains become alike near their shared borders.
    pub convergence: Convergence,
    /// The fields besides E that choose each region's Biome.
    pub fields: SelectorFields,
    /// Which Biome fills a region, from the field values at its site.
    pub biomes: BiomeRules,
    /// Regions whose Biome is set by hand, whatever the rules say. At most
    /// one pin per region.
    pub pins: Vec<BiomePin>,
}

impl Default for WorldOptions {
    /// The medium scale class (scale 1): 4 km regions on today's Terrain
    /// presets, with the built-in bare-land Biomes.
    fn default() -> Self {
        Self {
            seed: 0,
            scale: WorldScale::default(),
            regions: RegionGraphOptions::default(),
            elevation: ElevationOptions::default(),
            convergence: Convergence::default(),
            fields: SelectorFields::default(),
            biomes: BiomeRules::default(),
            pins: Vec::new(),
        }
    }
}

/// How much a World stretches its landscape against reference scale 1.
///
/// `horizontal` multiplies every landscape wavelength and distance (E's
/// layers, its bumps' centres and radii, the selector fields' wavelengths);
/// `vertical` multiplies every landscape height (E's layers and bumps, the
/// rules' elevation bounds). Slopes change by `vertical / horizontal`.
/// Objects, road and river widths, chunks and level of detail never
/// scale.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldScale {
    /// The stretch of landscape lengths. Finite and positive.
    pub horizontal: f64,
    /// The stretch of landscape heights. Finite and positive.
    pub vertical: f64,
}

impl Default for WorldScale {
    /// Scale 1: today's lengths and heights.
    fn default() -> Self {
        Self {
            horizontal: 1.0,
            vertical: 1.0,
        }
    }
}

/// The World's selector fields: smooth noise over the World that, with E,
/// decides which Biome fills each region.
///
/// They choose Biomes; they never change the height of the land.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectorFields {
    /// How rugged the land wants to be: high values favour hills and
    /// mountains over plains at the same height.
    pub ruggedness: SelectorField,
    /// How wet the land is: low values favour dry Biomes, high values wet
    /// ones.
    pub moisture: SelectorField,
}

impl Default for SelectorFields {
    fn default() -> Self {
        Self {
            ruggedness: SelectorField {
                wavelength: 12_000.0,
                octaves: 3,
                roughness: 0.5,
            },
            moisture: SelectorField {
                wavelength: 20_000.0,
                octaves: 3,
                roughness: 0.5,
            },
        }
    }
}

/// One selector field: smooth noise with values centred on 0, about 95 %
/// of them from −1 to 1.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectorField {
    /// Typical distance between the field's highs, in metres at reference
    /// scale. It sets how large an area of one Biome tends to be. Finite
    /// and positive.
    pub wavelength: f32,
    /// Number of noise layers, 1 through [`MAX_OCTAVES`]. More octaves
    /// make the edges of Biome areas more ragged.
    pub octaves: u32,
    /// How strong each finer octave is compared with the one before, from
    /// 0 up to (not including) 1.
    pub roughness: f32,
}

impl SelectorField {
    /// The field as a noise layer at `horizontal` scale, with values about
    /// 95 % within −1 to 1.
    pub(crate) fn layer(&self, horizontal: f64) -> NoiseLayer {
        NoiseLayer {
            height: 1.0,
            wavelength: (f64::from(self.wavelength) * horizontal) as f32,
            octaves: self.octaves,
            roughness: self.roughness,
        }
    }

    fn is_valid(&self) -> bool {
        self.wavelength.is_finite()
            && self.wavelength > 0.0
            && (1..=MAX_OCTAVES).contains(&self.octaves)
            && (0.0..1.0).contains(&self.roughness)
    }
}

/// The rules that choose each region's Biome from the field values at the
/// region's site.
///
/// The first rule whose bounds all hold wins; if none does, the region
/// gets [`fallback`](Self::fallback). Neighbouring regions may get the
/// same Biome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BiomeRules {
    /// The rules, tried in order.
    pub rules: Vec<BiomeRule>,
    /// The Biome for a region no rule matches, by key.
    pub fallback: String,
}

impl Default for BiomeRules {
    /// Rules for the built-in bare-land Biomes: mountains where the land is
    /// high and rugged, dry high steppe on plains, rolling hills on other
    /// high land and on rugged, not very wet lowland, and plains elsewhere.
    fn default() -> Self {
        let rule = |biome: &str, elevation, ruggedness, moisture| BiomeRule {
            biome: biome.to_owned(),
            elevation,
            ruggedness,
            moisture,
        };
        Self {
            rules: vec![
                rule(
                    "mountains",
                    FieldRange::at_least(150.0),
                    FieldRange::at_least(0.2),
                    FieldRange::ANY,
                ),
                rule(
                    "plains",
                    FieldRange::at_least(150.0),
                    FieldRange::ANY,
                    FieldRange::below(-0.2),
                ),
                rule(
                    "rolling_hills",
                    FieldRange::at_least(150.0),
                    FieldRange::ANY,
                    FieldRange::ANY,
                ),
                rule(
                    "rolling_hills",
                    FieldRange::ANY,
                    FieldRange::at_least(0.4),
                    FieldRange::below(0.4),
                ),
            ],
            fallback: "plains".to_owned(),
        }
    }
}

/// One rule: a Biome, and the bounds the field values at a region's site
/// must fall within for the region to get it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BiomeRule {
    /// The Biome, by key, such as `"mountains"`.
    pub biome: String,
    /// Bounds on the world elevation E, in metres above sea level at
    /// reference scale: at another scale they are multiplied by
    /// [`WorldScale::vertical`], as E is.
    #[serde(default)]
    pub elevation: FieldRange,
    /// Bounds on the ruggedness field.
    #[serde(default)]
    pub ruggedness: FieldRange,
    /// Bounds on the moisture field.
    #[serde(default)]
    pub moisture: FieldRange,
}

/// Bounds on one field value: at least `min` and below `max`. A missing
/// bound doesn't limit the value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldRange {
    /// The smallest value allowed, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f32>,
    /// The values allowed are below this, if given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f32>,
}

impl FieldRange {
    /// Any value.
    pub const ANY: Self = Self {
        min: None,
        max: None,
    };

    /// `min` or more.
    pub const fn at_least(min: f32) -> Self {
        Self {
            min: Some(min),
            max: None,
        }
    }

    /// Below `max`.
    pub const fn below(max: f32) -> Self {
        Self {
            min: None,
            max: Some(max),
        }
    }

    /// Whether `value` is within the bounds, with both bounds multiplied by
    /// `scale`.
    pub(crate) fn holds(&self, value: f32, scale: f64) -> bool {
        let value = f64::from(value);
        self.min.is_none_or(|min| value >= f64::from(min) * scale)
            && self.max.is_none_or(|max| value < f64::from(max) * scale)
    }

    fn is_valid(&self) -> bool {
        let finite = |bound: Option<f32>| bound.is_none_or(f32::is_finite);
        finite(self.min)
            && finite(self.max)
            && match (self.min, self.max) {
                (Some(min), Some(max)) => min <= max,
                _ => true,
            }
    }
}

/// A region whose Biome is set by hand.
///
/// A pin chooses the Biome only. To make a pinned mountain range actually
/// stand high, also add a [`HeightBump`] to the World's elevation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BiomePin {
    /// The region's id: the number in its
    /// [`RegionId`](procgena_terrain::RegionId), as
    /// [`World::region_at`](crate::World::region_at) gives it.
    pub region: u64,
    /// The Biome, by key.
    pub biome: String,
}

/// Why World settings were refused.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum WorldOptionsError {
    /// Both parts of the scale must be finite and positive.
    Scale(WorldScale),
    /// The region graph settings aren't valid.
    Regions(RegionGraphError),
    /// The world elevation isn't valid once stretched by the scale.
    Elevation(ElevationError),
    /// A selector field's settings are out of range: `"ruggedness"` or
    /// `"moisture"`.
    Field(&'static str),
    /// The relief meeting exponent is above 1 or NaN.
    ReliefMeeting(f32),
    /// The relief blend is outside 0 to 1, or NaN.
    ReliefBlend(f32),
    /// The rule at this index has a bound that isn't finite, or a `min`
    /// above its `max`.
    Rule(usize),
    /// More than one pin names this region.
    DuplicatePin(u64),
}

impl fmt::Display for WorldOptionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scale(s) => write!(
                f,
                "the scale must be finite and positive, not {} × {}",
                s.horizontal, s.vertical
            ),
            Self::Regions(e) => write!(f, "regions: {e}"),
            Self::Elevation(e) => write!(f, "elevation: {e}"),
            Self::Field(name) => write!(f, "the {name} field's settings are out of range"),
            Self::ReliefMeeting(p) => write!(f, "relief meeting exponent {p} is above 1 or NaN"),
            Self::ReliefBlend(b) => write!(f, "relief blend {b} is outside 0 to 1"),
            Self::Rule(i) => write!(
                f,
                "Biome rule {i} has a bound that isn't finite, or a min above its max"
            ),
            Self::DuplicatePin(id) => write!(f, "region {id} is pinned more than once"),
        }
    }
}

impl std::error::Error for WorldOptionsError {}

impl WorldOptions {
    /// Checks every setting and returns the first one out of range. Biome
    /// keys are not checked here: a [`World`](crate::World) checks them
    /// against the Biomes it is given.
    pub fn validate(&self) -> Result<(), WorldOptionsError> {
        let WorldScale {
            horizontal,
            vertical,
        } = self.scale;
        let positive = |v: f64| v.is_finite() && v > 0.0;
        if !positive(horizontal) || !positive(vertical) {
            return Err(WorldOptionsError::Scale(self.scale));
        }
        self.regions
            .validate()
            .map_err(WorldOptionsError::Regions)?;
        self.scaled_elevation()
            .validate()
            .map_err(WorldOptionsError::Elevation)?;
        for (name, field) in [
            ("ruggedness", &self.fields.ruggedness),
            ("moisture", &self.fields.moisture),
        ] {
            if !field.is_valid() {
                return Err(WorldOptionsError::Field(name));
            }
        }
        let Convergence { meeting, blend } = self.convergence;
        if !relief_meeting_is_valid(meeting) {
            return Err(WorldOptionsError::ReliefMeeting(meeting));
        }
        if !(0.0..=1.0).contains(&blend) {
            return Err(WorldOptionsError::ReliefBlend(blend));
        }
        for (i, rule) in self.biomes.rules.iter().enumerate() {
            let ranges = [rule.elevation, rule.ruggedness, rule.moisture];
            if !ranges.iter().all(FieldRange::is_valid) {
                return Err(WorldOptionsError::Rule(i));
            }
        }
        let mut pinned = BTreeSet::new();
        for pin in &self.pins {
            if !pinned.insert(pin.region) {
                return Err(WorldOptionsError::DuplicatePin(pin.region));
            }
        }
        Ok(())
    }

    /// The world elevation E as the World builds it: [`elevation`](Self::elevation)
    /// stretched by the scale. Layer and bump heights are multiplied by
    /// `vertical`, wavelengths and bump centres and radii by `horizontal`,
    /// and the tilt by `vertical / horizontal`. `max_slope` is kept.
    pub fn scaled_elevation(&self) -> ElevationOptions {
        let WorldScale {
            horizontal,
            vertical,
        } = self.scale;
        let layer = |l: &NoiseLayer| NoiseLayer {
            height: (f64::from(l.height) * vertical) as f32,
            wavelength: (f64::from(l.wavelength) * horizontal) as f32,
            ..l.clone()
        };
        let e = &self.elevation;
        ElevationOptions {
            continents: layer(&e.continents),
            uplift: layer(&e.uplift),
            tilt: e.tilt * (vertical / horizontal),
            max_slope: e.max_slope,
            bumps: e
                .bumps
                .iter()
                .map(|b| HeightBump {
                    centre: b.centre * horizontal,
                    radius: b.radius * horizontal,
                    height: (f64::from(b.height) * vertical) as f32,
                })
                .collect(),
        }
    }
}
