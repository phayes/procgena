//! The world elevation field E: the height of the land everywhere before
//! any terrain's own relief, in metres above sea level.

use std::fmt;
use std::sync::Arc;

use glam::DVec2;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::hash::{ELEVATION_CONTINENTS, ELEVATION_UPLIFT, seed_for};
use crate::noise::{MAX_OCTAVES, Octaves, fbm_gradient, fbm_slope_bound};
use crate::options::NoiseLayer;

/// Reads and writes a `DVec2` as `[x, z]` in settings files.
mod xz {
    use super::{DVec2, Deserialize, Deserializer, Serialize, Serializer};

    pub(super) fn serialize<S: Serializer>(v: &DVec2, serializer: S) -> Result<S::Ok, S::Error> {
        [v.x, v.y].serialize(serializer)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<DVec2, D::Error> {
        <[f64; 2]>::deserialize(deserializer).map(DVec2::from)
    }
}

/// The steepest a [`HeightBump`] gets, as a multiple of its height over its
/// radius: the largest slope of `(1 − t²)³` for `t` from 0 to 1, which is
/// 96 / (25·√5) ≈ 1.717 300 2, rounded up.
const BUMP_SLOPE: f64 = 1.717_301;

/// Settings for the world elevation field. See [`Elevation`].
///
/// In settings files, the tilt and each bump's centre are written as
/// `[x, z]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElevationOptions {
    /// Land and sea: a very long noise layer, typically 50 to 200 km
    /// between rises. A height of 0 turns it off.
    pub continents: NoiseLayer,
    /// Highlands and lowlands within the land, typically 10 to 50 km
    /// between rises. A height of 0 turns it off.
    pub uplift: NoiseLayer,
    /// A constant slope added everywhere: metres of rise per metre along x
    /// and along z. Heights grow without limit with distance along it, so
    /// it suits test worlds and bounded areas more than endless ones.
    #[serde(with = "xz")]
    pub tilt: DVec2,
    /// The steepest E may get anywhere, as rise over run. The layers,
    /// the tilt and the bumps together must stay within it.
    pub max_slope: f32,
    /// Hand-placed rises (or, with a negative height, hollows) of the land.
    pub bumps: Vec<HeightBump>,
}

/// A smooth rise of the land, added to E: `height` at `centre`, falling to
/// nothing at `radius` metres and beyond.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeightBump {
    /// The middle of the bump, in metres (x and z).
    #[serde(with = "xz")]
    pub centre: DVec2,
    /// How far the bump reaches, in metres. Must be positive.
    pub radius: f64,
    /// How much the bump raises the land at its middle, in metres. Negative
    /// for a hollow.
    pub height: f32,
}

impl Default for ElevationOptions {
    /// Gentle continents and uplands at Valheim-like scale (200 m over
    /// 75 km and 60 m over 20 km), no tilt and no bumps.
    fn default() -> Self {
        Self {
            continents: NoiseLayer {
                height: 200.0,
                wavelength: 75_000.0,
                octaves: 2,
                roughness: 0.4,
            },
            uplift: NoiseLayer {
                height: 60.0,
                wavelength: 20_000.0,
                octaves: 2,
                roughness: 0.4,
            },
            tilt: DVec2::ZERO,
            max_slope: 0.05,
            bumps: Vec::new(),
        }
    }
}

/// Why elevation settings were refused.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ElevationError {
    /// A noise layer's settings are out of range: `"continents"` or
    /// `"uplift"`.
    Layer(&'static str),
    /// The tilt must be finite.
    Tilt,
    /// The steepest slope allowed must be finite and positive.
    MaxSlope(f32),
    /// The bump at this index needs a finite centre and height, and a
    /// finite, positive radius.
    Bump(usize),
    /// The layers, tilt and bumps could together be steeper than allowed.
    TooSteep {
        /// The steepest the settings could make E.
        steepest: f64,
        /// The steepest allowed.
        allowed: f32,
    },
}

impl fmt::Display for ElevationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Layer(name) => write!(f, "the {name} layer's settings are out of range"),
            Self::Tilt => write!(f, "the tilt must be finite"),
            Self::MaxSlope(s) => {
                write!(f, "the steepest slope must be finite and positive, not {s}")
            }
            Self::Bump(i) => write!(
                f,
                "bump {i} needs a finite centre and height and a positive radius"
            ),
            Self::TooSteep { steepest, allowed } => write!(
                f,
                "these settings could make the land as steep as {steepest:.4}, over the {allowed} allowed"
            ),
        }
    }
}

impl std::error::Error for ElevationError {}

impl ElevationOptions {
    /// Checks the settings, including that E can never be steeper than
    /// [`max_slope`](Self::max_slope).
    pub fn validate(&self) -> Result<(), ElevationError> {
        for (name, layer) in [("continents", &self.continents), ("uplift", &self.uplift)] {
            let ok = layer.height.is_finite()
                && layer.height >= 0.0
                && layer.wavelength.is_finite()
                && layer.wavelength > 0.0
                && (1..=MAX_OCTAVES).contains(&layer.octaves)
                && (0.0..1.0).contains(&layer.roughness);
            if !ok {
                return Err(ElevationError::Layer(name));
            }
        }
        if !self.tilt.is_finite() {
            return Err(ElevationError::Tilt);
        }
        if !self.max_slope.is_finite() || self.max_slope <= 0.0 {
            return Err(ElevationError::MaxSlope(self.max_slope));
        }
        for (i, bump) in self.bumps.iter().enumerate() {
            let ok = bump.centre.is_finite()
                && bump.height.is_finite()
                && bump.radius.is_finite()
                && bump.radius > 0.0;
            if !ok {
                return Err(ElevationError::Bump(i));
            }
        }
        let steepest = self.steepest();
        if steepest > f64::from(self.max_slope) {
            return Err(ElevationError::TooSteep {
                steepest,
                allowed: self.max_slope,
            });
        }
        Ok(())
    }

    /// The steepest these settings could make E, as rise over run.
    pub fn steepest(&self) -> f64 {
        let layer = |l: &NoiseLayer| {
            0.5 * f64::from(l.height) * fbm_slope_bound(octaves(l)) / f64::from(l.wavelength)
        };

        // Where bumps overlap their slopes add. Every bump holding a point
        // overlaps every other one there, so the worst point is no steeper
        // than some bump plus all the bumps overlapping it.
        let slope = |b: &HeightBump| BUMP_SLOPE * f64::from(b.height.abs()) / b.radius;
        let bumps = self
            .bumps
            .iter()
            .map(|a| {
                self.bumps
                    .iter()
                    .filter(|b| a.centre.distance(b.centre) < a.radius + b.radius)
                    .map(slope)
                    .sum::<f64>()
            })
            .fold(0.0, f64::max);
        layer(&self.continents) + layer(&self.uplift) + self.tilt.length() + bumps
    }
}

/// The octaves of noise layer `layer`.
fn octaves(layer: &NoiseLayer) -> Octaves {
    Octaves {
        count: layer.octaves,
        gain: layer.roughness,
    }
}

/// The world elevation field E: how high the land lies everywhere, in
/// metres above sea level, before each terrain adds its own relief. Its
/// zero is the sea level.
///
/// E is the same for every region of a world, so neighbouring regions start
/// from the same height at their shared borders. Pass it to each terrain
/// with [`TerrainGenerator::with_elevation`](crate::TerrainGenerator::with_elevation).
/// Cloning is cheap.
#[derive(Debug, Clone, Default)]
pub struct Elevation {
    /// `None` for flat land at sea level.
    field: Option<Arc<Field>>,
}

#[derive(Debug)]
struct Field {
    options: ElevationOptions,
    continents_seed: u32,
    uplift_seed: u32,
}

impl Elevation {
    /// Flat land at sea level everywhere: E is 0.
    pub fn flat() -> Self {
        Self::default()
    }

    /// The field for `options` and the world's `seed`, the same seed every
    /// region's [`TerrainOptions::default_seed`](crate::TerrainOptions::default_seed)
    /// holds.
    ///
    /// # Errors
    ///
    /// If `options` aren't valid (see [`ElevationOptions::validate`]).
    pub fn new(options: ElevationOptions, seed: u32) -> Result<Self, ElevationError> {
        options.validate()?;
        Ok(Self {
            field: Some(Arc::new(Field {
                options,
                continents_seed: seed_for(seed, ELEVATION_CONTINENTS),
                uplift_seed: seed_for(seed, ELEVATION_UPLIFT),
            })),
        })
    }

    /// Whether this is [`flat`](Self::flat) land, where E is 0 everywhere.
    pub(crate) fn is_flat(&self) -> bool {
        self.field.is_none()
    }

    /// The settings, or `None` for [`flat`](Self::flat) land.
    pub fn options(&self) -> Option<&ElevationOptions> {
        self.field.as_ref().map(|f| &f.options)
    }

    /// E at world position `p` (x and z, metres), in metres above sea
    /// level.
    pub fn height(&self, p: DVec2) -> f32 {
        self.sample(p).0 as f32
    }

    /// The rate of change of E along x and z at `p`, as rise over run.
    pub fn gradient(&self, p: DVec2) -> DVec2 {
        self.sample(p).1
    }

    /// E and its gradient at `p`.
    fn sample(&self, p: DVec2) -> (f64, DVec2) {
        let Some(field) = &self.field else {
            return (0.0, DVec2::ZERO);
        };
        let o = &field.options;

        // The noise layers and the tilt.
        let mut height = o.tilt.dot(p);
        let mut gradient = o.tilt;
        for (layer, seed) in [
            (&o.continents, field.continents_seed),
            (&o.uplift, field.uplift_seed),
        ] {
            if layer.height == 0.0 {
                continue;
            }
            let wavelength = f64::from(layer.wavelength);
            let (value, slope) = fbm_gradient(p / wavelength, seed, octaves(layer));
            let scale = 0.5 * f64::from(layer.height);
            height += scale * f64::from(value);
            gradient += slope * (scale / wavelength);
        }

        // The bumps: `height · (1 − t²)³` with `t` the distance over the
        // radius.
        for bump in &o.bumps {
            let offset = p - bump.centre;
            let t2 = offset.length_squared() / (bump.radius * bump.radius);
            if t2 >= 1.0 {
                continue;
            }
            let h = f64::from(bump.height);
            let fall = 1.0 - t2;
            height += h * fall * fall * fall;
            gradient += offset * (-6.0 * h * fall * fall / (bump.radius * bump.radius));
        }
        (height, gradient)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Points scattered over `extent` metres around the origin.
    fn points(extent: f64) -> impl Iterator<Item = DVec2> {
        (0..40_000).map(move |i| {
            let i = f64::from(i);
            DVec2::new(
                (i * 0.618_034 % 1.0 - 0.5) * extent,
                (i * 0.414_214 % 1.0 - 0.5) * extent,
            )
        })
    }

    /// Steep settings that only just validate: every layer, a tilt, and
    /// overlapping bumps.
    fn steep() -> ElevationOptions {
        let mut options = ElevationOptions {
            continents: NoiseLayer {
                height: 400.0,
                wavelength: 40_000.0,
                octaves: 3,
                roughness: 0.5,
            },
            uplift: NoiseLayer {
                height: 60.0,
                wavelength: 8_000.0,
                octaves: 3,
                roughness: 0.5,
            },
            tilt: DVec2::new(0.004, -0.003),
            max_slope: 1.0,
            bumps: vec![
                HeightBump {
                    centre: DVec2::new(0.0, 0.0),
                    radius: 6000.0,
                    height: 80.0,
                },
                HeightBump {
                    centre: DVec2::new(3000.0, 1000.0),
                    radius: 5000.0,
                    height: -60.0,
                },
            ],
        };
        options.max_slope = options.steepest() as f32 * 1.001;
        options
    }

    #[test]
    fn the_slope_never_passes_the_bound() {
        let options = steep();
        let bound = options.steepest();
        let e = Elevation::new(options, 4).unwrap();
        let mut steepest = 0.0f64;
        for p in points(60_000.0) {
            steepest = steepest.max(e.gradient(p).length());
        }
        assert!(steepest <= bound, "{steepest} over {bound}");
        assert!(
            steepest > 0.2 * bound,
            "the test should come near the bound"
        );
    }

    #[test]
    fn overlapping_bumps_add_their_slopes_in_the_bound() {
        // Two bumps on one spot, with no noise or tilt: E is as steep as
        // the bound says, nearly exactly.
        let bump = HeightBump {
            centre: DVec2::new(100.0, 200.0),
            radius: 2000.0,
            height: 30.0,
        };
        let off = NoiseLayer {
            height: 0.0,
            ..ElevationOptions::default().uplift
        };
        let options = ElevationOptions {
            continents: off.clone(),
            uplift: off,
            tilt: DVec2::ZERO,
            max_slope: 1.0,
            bumps: vec![bump, bump],
        };
        let bound = options.steepest();
        assert!((bound - 2.0 * BUMP_SLOPE * 30.0 / 2000.0).abs() < 1e-12);
        let e = Elevation::new(options, 1).unwrap();
        let steepest = (0..=4000)
            .map(|i| {
                e.gradient(bump.centre + DVec2::new(f64::from(i) * 0.5, 0.0))
                    .length()
            })
            .fold(0.0, f64::max);
        assert!(
            steepest <= bound && steepest > 0.999 * bound,
            "{steepest} against {bound}"
        );
    }

    #[test]
    fn the_gradient_matches_the_height() {
        let e = Elevation::new(steep(), 4).unwrap();
        let h = 0.5;
        for p in points(20_000.0).step_by(97) {
            let d = |dir: DVec2| {
                (f64::from(e.height(p + dir * h)) - f64::from(e.height(p - dir * h))) / (2.0 * h)
            };
            let numeric = DVec2::new(d(DVec2::X), d(DVec2::Y));
            assert!(
                e.gradient(p).distance(numeric) < 2e-3,
                "{p}: {} vs {numeric}",
                e.gradient(p)
            );
        }
    }

    #[test]
    fn bumps_raise_the_land_within_their_radius_only() {
        let bump = HeightBump {
            centre: DVec2::new(5000.0, -2000.0),
            radius: 3000.0,
            height: 150.0,
        };
        let mut options = ElevationOptions {
            max_slope: 0.2,
            ..Default::default()
        };
        let plain = Elevation::new(options.clone(), 8).unwrap();
        options.bumps.push(bump);
        let bumped = Elevation::new(options, 8).unwrap();
        assert_eq!(
            bumped.height(bump.centre) - plain.height(bump.centre),
            150.0
        );
        for p in points(30_000.0) {
            let d = p.distance(bump.centre);
            let raised = bumped.height(p) - plain.height(p);
            if d >= bump.radius {
                assert_eq!(raised, 0.0, "{p}");
            } else {
                assert!(raised > 0.0 || d > 0.999 * bump.radius, "{p}: {raised}");
            }
        }
    }

    #[test]
    fn flat_land_is_at_sea_level_and_settings_are_checked() {
        let flat = Elevation::flat();
        assert_eq!(flat.height(DVec2::new(1e7, -3e6)), 0.0);
        assert_eq!(flat.gradient(DVec2::ZERO), DVec2::ZERO);
        assert!(flat.options().is_none());

        assert_eq!(ElevationOptions::default().validate(), Ok(()));
        let too_steep = ElevationOptions {
            tilt: DVec2::new(0.05, 0.0),
            ..Default::default()
        };
        assert!(matches!(
            too_steep.validate(),
            Err(ElevationError::TooSteep { .. })
        ));
        let mut bad_bump = ElevationOptions::default();
        bad_bump.bumps.push(HeightBump {
            centre: DVec2::ZERO,
            radius: 0.0,
            height: 1.0,
        });
        assert_eq!(bad_bump.validate(), Err(ElevationError::Bump(0)));
        let mut bad_layer = ElevationOptions::default();
        bad_layer.uplift.octaves = 0;
        assert_eq!(bad_layer.validate(), Err(ElevationError::Layer("uplift")));
    }

    #[test]
    fn the_same_seed_gives_the_same_field() {
        let a = Elevation::new(ElevationOptions::default(), 3).unwrap();
        let b = Elevation::new(ElevationOptions::default(), 3).unwrap();
        let c = Elevation::new(ElevationOptions::default(), 4).unwrap();
        let p = DVec2::new(12_345.6, -7_890.1);
        assert_eq!(a.height(p), b.height(p));
        assert_ne!(a.height(p), c.height(p));
    }
}
