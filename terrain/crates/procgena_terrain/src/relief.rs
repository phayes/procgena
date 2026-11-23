//! The terrain's own height, before it is bent to meet its neighbours.

use glam::{DVec2, Vec2};

use crate::calibration::{MASK_OCTAVES, RIDGED_OCTAVES, mask_threshold};
use crate::converge::{Fade, LayerSpreads};
use crate::hash::derive_seed;
use crate::noise::{Octaves, fbm, ridged};
use crate::options::{BaseOptions, NoiseLayer, RoadGrades, TerrainOptions};

/// Width of the soft rim of the mountain mask, in fBm standard deviations.
const MASK_RIM: f32 = 0.6;

/// How much larger mountain ranges are than their ridge wavelength.
const RANGE_SCALE: f64 = 4.0;

/// Step for the central-difference gradient, in metres.
const GRADIENT_STEP: f64 = 0.5;

/// Gain between ridge octaves: the gain the ridged statistics were measured
/// with.
const RIDGE_GAIN: f32 = RIDGED_OCTAVES.gain;

/// Mean of a mountain crest's height, as a share of the layer height.
const CREST_MEAN: f32 = 0.5;

/// Spread of a mountain crest's height, as a share of the layer height.
const CREST_SPREAD: f32 = 0.25;

/// A terrain's own height field: pure, defined everywhere, and the same for
/// the same options and fade.
#[derive(Debug, Clone)]
pub(crate) struct Relief {
    base: BaseOptions,
    swell_seed: u32,
    hill_seed: u32,
    mask_seed: u32,
    ridge_seed: u32,
    mask_threshold: f32,
    roads: RoadGrades,
    fade: Option<Fade>,
}

/// The spread of each layer of `base`, in metres (see [`LayerSpreads`]).
/// Rolling layers count their noise only, not the lowering by erosion; the
/// mountain layer counts its range mask as fully on or off.
pub(crate) fn layer_spreads(base: &BaseOptions) -> LayerSpreads {
    let m = &base.mountains;
    let c = m.coverage;
    let mean_square = c * (CREST_MEAN * CREST_MEAN + CREST_SPREAD * CREST_SPREAD);
    let squared_mean = (c * CREST_MEAN).powi(2);
    [
        0.5 * base.swells.height,
        0.5 * base.hills.height,
        m.height * (mean_square - squared_mean).max(0.0).sqrt(),
    ]
}

impl Relief {
    /// Prepares the relief for `options`, which must be valid. Every layer
    /// takes its seed from `options.seed` and its role, so terrains with the
    /// same seed share their noise.
    pub(crate) fn new(options: &TerrainOptions) -> Self {
        let seed = options.seed;
        let base = options.base.clone();
        Self {
            mask_threshold: mask_threshold(1.0 - base.mountains.coverage),
            base,
            swell_seed: derive_seed(seed, 1),
            hill_seed: derive_seed(seed, 2),
            mask_seed: derive_seed(seed, 3),
            ridge_seed: derive_seed(seed, 4),
            roads: options.features.roads.grades(),
            fade: None,
        }
    }

    /// The road grades at `p`: the terrain's own, faded towards its edges'
    /// targets near them.
    pub(crate) fn road_grades(&self, p: DVec2) -> RoadGrades {
        self.fade.as_ref().map_or(self.roads, |f| f.roads(p))
    }

    /// How far from its edges the fade reaches, in metres, or `None` with
    /// no fade.
    pub(crate) fn fade_width(&self) -> Option<f64> {
        self.fade.as_ref().map(Fade::width)
    }

    /// Height at `p` without the fade: every layer as the settings give it.
    pub(crate) fn unfaded_height(&self, p: DVec2) -> f32 {
        self.scaled_height(p, 1.0, 1.0, 1.0)
    }

    /// The spread of each layer, in metres.
    pub(crate) fn spreads(&self) -> LayerSpreads {
        layer_spreads(&self.base)
    }

    /// This relief, faded by `fade` (or not faded at all for `None`).
    pub(crate) fn with_fade(mut self, fade: Option<Fade>) -> Self {
        self.fade = fade;
        self
    }

    /// Height in metres at world position `p` (x and z).
    pub(crate) fn height(&self, p: DVec2) -> f32 {
        let [swells, hills, mountains] = match &self.fade {
            Some(fade) => fade.scales(p).unwrap_or([1.0; 3]),
            None => [1.0; 3],
        };
        self.scaled_height(p, swells, hills, mountains)
    }

    /// Height at `p` with the swell, hill and mountain layers scaled by the
    /// given factors.
    fn scaled_height(&self, p: DVec2, swells: f32, hills: f32, mountains: f32) -> f32 {
        let b = &self.base;
        let rolling = |layer: &NoiseLayer, seed: u32, erosion: f32, scale: f32| {
            if layer.height == 0.0 || scale == 0.0 {
                return 0.0;
            }
            let octaves = Octaves {
                count: layer.octaves,
                gain: layer.roughness,
            };
            // Unit-spread noise, so ±height covers about two standard deviations.
            0.5 * scale
                * layer.height
                * fbm(p / f64::from(layer.wavelength), seed, octaves, erosion)
        };

        // Rolling layers.
        let mut h = b.elevation;
        h += rolling(&b.swells, self.swell_seed, 0.0, swells);
        h += rolling(&b.hills, self.hill_seed, b.erosion, hills);

        // Mountains: ridges inside a range mask.
        let m = &b.mountains;
        if m.height > 0.0 && m.coverage > 0.0 && mountains > 0.0 {
            let wavelength = f64::from(m.wavelength);
            let range = fbm(
                p / (wavelength * RANGE_SCALE),
                self.mask_seed,
                MASK_OCTAVES,
                0.0,
            );
            let u = ((range - self.mask_threshold) / MASK_RIM).max(0.0);
            // Rises smoothly from 0 and approaches 1 without a flat top.
            let mask = 1.0 - (-2.0 * u * u).exp();
            if mask > 0.0 {
                let octaves = Octaves {
                    count: m.octaves,
                    gain: RIDGE_GAIN,
                };
                let crest =
                    (0.5 + 0.25 * ridged(p / wavelength, self.ridge_seed, octaves)).max(0.0);
                h += mountains * m.height * mask * crest;
            }
        }
        h
    }

    /// Rate of height change along x and z at `p`, by central differences.
    pub(crate) fn gradient(&self, p: DVec2) -> Vec2 {
        let dx =
            self.height(p + DVec2::X * GRADIENT_STEP) - self.height(p - DVec2::X * GRADIENT_STEP);
        let dz =
            self.height(p + DVec2::Y * GRADIENT_STEP) - self.height(p - DVec2::Y * GRADIENT_STEP);
        Vec2::new(dx, dz) / (2.0 * GRADIENT_STEP as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::TerrainPreset;

    #[test]
    fn relief_repeats_and_follows_seed() {
        let mut options = TerrainPreset::RollingHills.options();
        let a = Relief::new(&options);
        options.seed = 1;
        let b = Relief::new(&options);
        let p = DVec2::new(123.4, -567.8);
        assert_eq!(a.height(p), a.height(p));
        assert_ne!(a.height(p), b.height(p));
    }

    #[test]
    fn hill_height_means_metres() {
        // With only hills, about 95 % of samples lie within ±height of the mean.
        let mut options = TerrainOptions::default();
        options.base.swells.height = 0.0;
        options.base.erosion = 0.0;
        options.base.hills.height = 20.0;
        let relief = Relief::new(&options);
        let samples: Vec<f32> = (0..20_000)
            .map(|i| {
                let i = f64::from(i);
                relief.height(DVec2::new((i * 61.803) % 30_000.0, (i * 1.37) % 30_000.0))
                    - options.base.elevation
            })
            .collect();
        let inside =
            samples.iter().filter(|h| h.abs() <= 20.0).count() as f32 / samples.len() as f32;
        assert!(
            (0.9..0.99).contains(&inside),
            "share within ±height: {inside}"
        );
    }

    #[test]
    fn mountain_coverage_means_area() {
        let mut options = TerrainPreset::Mountains.options();
        options.base.mountains.coverage = 0.3;
        let relief = Relief::new(&options);
        let wavelength = f64::from(options.base.mountains.wavelength) * RANGE_SCALE;
        let inside = (0..20_000)
            .filter(|&i| {
                let i = f64::from(i);
                let p = DVec2::new((i * 0.618_034) % 200.0, (i * 0.01) % 200.0) * wavelength;
                fbm(p / wavelength, relief.mask_seed, MASK_OCTAVES, 0.0) > relief.mask_threshold
            })
            .count() as f32
            / 20_000.0;
        assert!((inside - 0.3).abs() < 0.05, "covered share: {inside}");
    }
}
