//! The terrain's own height, before it is bent to meet its neighbours.

use glam::{DVec2, Vec2};

use crate::calibration::{MASK_OCTAVES, RIDGED_OCTAVES, erosion_damping, mask_threshold};
use crate::converge::{Fade, LayerSpreads};
use crate::elevation::Elevation;
use crate::hash::{RELIEF_HILLS, RELIEF_MOUNTAIN_MASK, RELIEF_RIDGES, RELIEF_SWELLS, seed_for};
use crate::noise::{Octaves, fbm, fbm_gradient, fbm_slope, ridged, ridged_slope};
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
/// the same options, fade and elevation.
///
/// Heights are the world elevation E plus the terrain's relief. The fade
/// works on the relief alone: it scales the relief's layers and blends
/// with the neighbours' relief, and E is added after.
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
    elevation: Elevation,
}

/// The spread of each layer of `base`, in metres (see [`LayerSpreads`]).
/// The hills count how much erosion lowers them (see
/// [`erosion_damping`]); the mountain layer counts its range mask as fully
/// on or off.
pub(crate) fn layer_spreads(base: &BaseOptions) -> LayerSpreads {
    let m = &base.mountains;
    let c = m.coverage;
    let mean_square = c * (CREST_MEAN * CREST_MEAN + CREST_SPREAD * CREST_SPREAD);
    let squared_mean = (c * CREST_MEAN).powi(2);
    [
        0.5 * base.swells.height,
        0.5 * base.hills.height * erosion_damping(base.erosion),
        m.height * (mean_square - squared_mean).max(0.0).sqrt(),
    ]
}

impl Relief {
    /// Prepares the relief for `options`, which must be valid. Every layer
    /// takes its seed from `options.default_seed` and its role, so terrains
    /// with the same seed share their noise.
    pub(crate) fn new(options: &TerrainOptions) -> Self {
        let seed = options.default_seed;
        let base = options.base.clone();
        Self {
            mask_threshold: mask_threshold(1.0 - base.mountains.coverage),
            base,
            swell_seed: seed_for(seed, RELIEF_SWELLS),
            hill_seed: seed_for(seed, RELIEF_HILLS),
            mask_seed: seed_for(seed, RELIEF_MOUNTAIN_MASK),
            ridge_seed: seed_for(seed, RELIEF_RIDGES),
            roads: options.features.roads.grades(),
            fade: None,
            elevation: Elevation::flat(),
        }
    }

    /// The world elevation this relief stands on.
    pub(crate) fn elevation(&self) -> &Elevation {
        &self.elevation
    }

    /// This relief standing on `elevation` instead of its current one.
    pub(crate) fn with_elevation(mut self, elevation: Elevation) -> Self {
        self.elevation = elevation;
        self
    }

    /// The road grades at `p`: the terrain's own, faded towards its edges'
    /// targets near them.
    pub(crate) fn road_grades(&self, p: DVec2) -> RoadGrades {
        self.fade.as_ref().map_or(self.roads, |f| f.roads(p))
    }

    /// How far from its edges the fade reaches, in metres, or `None` with
    /// no fade.
    pub(crate) fn fade_width(&self) -> Option<f64> {
        self.fade.as_ref().map(Fade::reach)
    }

    /// Height at `p` without the fade: E plus every layer as the settings
    /// give it.
    pub(crate) fn unfaded_height(&self, p: DVec2) -> f32 {
        self.own_relief(p) + self.elevation.height(p)
    }

    /// The relief alone at `p`, without E or the fade: what the fade
    /// blends between neighbours.
    pub(crate) fn own_relief(&self, p: DVec2) -> f32 {
        self.scaled_height(p, 1.0, 1.0, 1.0)
    }

    /// The typical height of the relief without the fade, in metres: its
    /// offset above E plus the mountain layer's mean. E isn't counted: it
    /// is the same for every terrain, so it drops out wherever terrains
    /// compare their mean heights.
    pub(crate) fn mean_height(&self) -> f64 {
        let m = &self.base.mountains;
        f64::from(self.base.elevation) + f64::from(m.height * m.coverage * CREST_MEAN)
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

    /// Height in metres at world position `p` (x and z): E plus the faded
    /// relief.
    pub(crate) fn height(&self, p: DVec2) -> f32 {
        self.height_above_elevation(p) + self.elevation.height(p)
    }

    /// The height at `p` above the world elevation E: the relief alone,
    /// faded.
    pub(crate) fn height_above_elevation(&self, p: DVec2) -> f32 {
        let Some(fade) = &self.fade else {
            return self.own_relief(p);
        };

        // Scaled layers alone: no blend share.
        let scales = fade.scales(p);
        let share = fade.share();
        let scaled = || {
            let [swells, hills, mountains] = scales.unwrap_or([1.0; 3]);
            self.scaled_height(p, swells, hills, mountains)
        };
        if share == 0.0 {
            return scaled();
        }

        // The border blend, alone or mixed with the scaled layers. Outside
        // both fades the height is exactly the terrain's own.
        let own = self.own_relief(p);
        let Some(blended) = fade.blended(p, f64::from(own)) else {
            let scaled = scaled();
            return if scaled == own {
                own
            } else {
                ((1.0 - share) * f64::from(scaled) + share * f64::from(own)) as f32
            };
        };
        if share == 1.0 {
            return blended as f32;
        }
        // Where both give the same height, keep it exactly.
        let (scaled, blended) = (scaled(), blended as f32);
        if scaled == blended {
            return scaled;
        }
        ((1.0 - share) * f64::from(scaled) + share * f64::from(blended)) as f32
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

    /// Each layer's height at `p` at full scale, in metres, with its
    /// gradient: swells, hills, then mountains (see [`LayerSpreads`]).
    fn layers(&self, p: DVec2) -> [(f64, DVec2); 3] {
        let b = &self.base;
        let rolling = |layer: &NoiseLayer, seed: u32, erosion: f32| {
            if layer.height == 0.0 {
                return (0.0, DVec2::ZERO);
            }
            let octaves = Octaves {
                count: layer.octaves,
                gain: layer.roughness,
            };
            let wavelength = f64::from(layer.wavelength);
            let q = p / wavelength;
            let scale = 0.5 * f64::from(layer.height);
            (
                scale * f64::from(fbm(q, seed, octaves, erosion)),
                fbm_slope(q, seed, octaves, erosion) * (scale / wavelength),
            )
        };
        let swells = rolling(&b.swells, self.swell_seed, 0.0);
        let hills = rolling(&b.hills, self.hill_seed, b.erosion);

        // Mountains: ridges inside a range mask, `height · mask · crest`.
        let m = &b.mountains;
        let mut mountains = (0.0, DVec2::ZERO);
        if m.height > 0.0 && m.coverage > 0.0 {
            let wavelength = f64::from(m.wavelength);
            let range_wavelength = wavelength * RANGE_SCALE;
            let (range, range_slope) =
                fbm_gradient(p / range_wavelength, self.mask_seed, MASK_OCTAVES);
            let u = f64::from((range - self.mask_threshold) / MASK_RIM);
            if u > 0.0 {
                let u_slope = range_slope / (f64::from(MASK_RIM) * range_wavelength);
                let fall = (-2.0 * u * u).exp();
                let (mask, mask_slope) = (1.0 - fall, u_slope * (4.0 * u * fall));
                let octaves = Octaves {
                    count: m.octaves,
                    gain: RIDGE_GAIN,
                };
                let q = p / wavelength;
                let crest = 0.5 + 0.25 * f64::from(ridged(q, self.ridge_seed, octaves));
                if crest > 0.0 {
                    let crest_slope =
                        ridged_slope(q, self.ridge_seed, octaves) * (0.25 / wavelength);
                    let height = f64::from(m.height);
                    mountains = (
                        height * mask * crest,
                        (mask_slope * crest + crest_slope * mask) * height,
                    );
                }
            }
        }
        [swells, hills, mountains]
    }

    /// Rate of height change along x and z at `p`, as rise over run, worked
    /// out from the noise's own slopes rather than from nearby heights.
    /// Within the fade, the parts that come from the edges' pull are taken
    /// by differences (see [`Fade::scales_gradient`]).
    pub(crate) fn slope(&self, p: DVec2) -> DVec2 {
        self.relief_slope(p) + self.elevation.gradient(p)
    }

    /// [`slope`](Self::slope) without E's.
    fn relief_slope(&self, p: DVec2) -> DVec2 {
        let layers = self.layers(p);
        let own_slope = layers.iter().map(|(_, g)| *g).sum::<DVec2>();
        let Some(fade) = &self.fade else {
            return own_slope;
        };

        // The scaled layers: the scales change near the edges too.
        let scaled_slope = || match fade.scales_gradient(p) {
            None => own_slope,
            Some((scales, scale_slopes)) => (0..3)
                .map(|k| layers[k].1 * scales[k] + scale_slopes[k] * layers[k].0)
                .sum(),
        };
        let share = fade.share();
        if share == 0.0 {
            return scaled_slope();
        }

        // The border blend, alone or mixed with the scaled layers, making
        // the same choices as `height_above_elevation`.
        let own = self.own_relief(p);
        let scaled = || {
            let [swells, hills, mountains] = fade.scales(p).unwrap_or([1.0; 3]);
            self.scaled_height(p, swells, hills, mountains)
        };
        let Some((blended, blended_slope)) = fade.blended_gradient(p, f64::from(own), own_slope)
        else {
            return if scaled() == own {
                own_slope
            } else {
                scaled_slope() * (1.0 - share) + own_slope * share
            };
        };
        if share == 1.0 {
            return blended_slope;
        }
        if scaled() == blended as f32 {
            return scaled_slope();
        }
        scaled_slope() * (1.0 - share) + blended_slope * share
    }

    /// The gradient of [`own_relief`](Self::own_relief) at `p`.
    pub(crate) fn own_relief_slope(&self, p: DVec2) -> DVec2 {
        self.layers(p).iter().map(|(_, g)| *g).sum()
    }

    /// Rate of height change along x and z at `p`: E's own gradient plus
    /// the relief's, by central differences. The relief's heights are
    /// small beside E's, so they keep more precision in the differences.
    pub(crate) fn gradient(&self, p: DVec2) -> Vec2 {
        let h = |q: DVec2| self.height_above_elevation(q);
        let dx = h(p + DVec2::X * GRADIENT_STEP) - h(p - DVec2::X * GRADIENT_STEP);
        let dz = h(p + DVec2::Y * GRADIENT_STEP) - h(p - DVec2::Y * GRADIENT_STEP);
        let relief = Vec2::new(dx, dz) / (2.0 * GRADIENT_STEP as f32);
        if self.elevation.is_flat() {
            return relief;
        }
        relief + self.elevation.gradient(p).as_vec2()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::TerrainPreset;

    /// Checks `relief`'s slope against central differences of its heights
    /// 2 m either side of a grid of points, in x and in z: each difference
    /// must match the slope averaged over the same span (Simpson's rule).
    /// Mountain heights carry up to about 5e-3 m of f32 rounding, too much
    /// for shorter spans, and it sets the tolerance. Spans across a sharp
    /// crest, where the slope jumps between neighbouring samples, are
    /// skipped: mountain ridges have them every few metres, so up to a
    /// quarter of the spans.
    fn assert_slope_matches(relief: &Relief, label: &str) {
        const HALF: f64 = 2.0;
        const STEPS: i32 = 128;
        let (mut skipped, mut total) = (0, 0);
        for i in 0..1600 {
            let p = DVec2::new(
                f64::from(i % 40) * 97.3 - 1900.0,
                f64::from(i / 40) * 83.1 - 1700.0,
            );
            for axis in [DVec2::X, DVec2::Y] {
                total += 1;
                let slopes: Vec<f64> = (0..=STEPS)
                    .map(|k| {
                        let at = p + axis * HALF * (2.0 * f64::from(k) / f64::from(STEPS) - 1.0);
                        relief.slope(at).dot(axis)
                    })
                    .collect();
                if slopes.windows(2).any(|w| (w[1] - w[0]).abs() > 0.05) {
                    skipped += 1;
                    continue;
                }
                let mean = slopes
                    .iter()
                    .enumerate()
                    .map(|(k, &s)| match k as i32 {
                        0 | STEPS => s,
                        k if k % 2 == 1 => 4.0 * s,
                        _ => 2.0 * s,
                    })
                    .sum::<f64>()
                    / (3.0 * f64::from(STEPS));
                let difference = (f64::from(relief.height(p + axis * HALF))
                    - f64::from(relief.height(p - axis * HALF)))
                    / (2.0 * HALF);
                assert!(
                    (difference - mean).abs() < 4e-3 + 2e-3 * mean.abs(),
                    "{label} at {p} along {axis}: mean slope {mean} against {difference}"
                );
            }
        }
        assert!(
            skipped * 4 <= total,
            "{label}: {skipped} of {total} skipped"
        );
    }

    #[test]
    fn slopes_match_the_heights() {
        for preset in [
            TerrainPreset::plains(),
            TerrainPreset::rolling_hills(),
            TerrainPreset::mountains(),
        ] {
            assert_slope_matches(&Relief::new(&preset.terrain), &preset.name);
        }
    }

    #[test]
    fn relief_repeats_and_follows_seed() {
        let mut options = TerrainPreset::rolling_hills().terrain.clone();
        let a = Relief::new(&options);
        options.default_seed = 1;
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
        let mut options = TerrainPreset::mountains().terrain.clone();
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
