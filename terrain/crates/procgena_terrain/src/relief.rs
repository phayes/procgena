//! The terrain's own height, before it meets its neighbours' along its
//! borders.

use std::sync::Arc;

use glam::DVec2;

use crate::border::{
    BorderField, BorderProfile, NoTargets, Place, ProfileShape, Target, Targets, pin_shares,
};
use crate::calibration::{MASK_OCTAVES, RIDGED_OCTAVES, erosion_damping, mask_threshold};
use crate::converge::LayerSpreads;
use crate::elevation::Elevation;
use crate::hash::{RELIEF_HILLS, RELIEF_MOUNTAIN_MASK, RELIEF_RIDGES, RELIEF_SWELLS, seed_for};
use crate::noise::{Octaves, fbm, fbm_gradient, fbm_slope, ridged, ridged_slope};
use crate::options::{BaseOptions, NoiseLayer, RoadGrades, TerrainOptions};
use crate::region::EdgeId;

/// Width of the soft rim of the mountain mask, in fBm standard deviations.
const MASK_RIM: f32 = 0.6;

/// How much larger mountain ranges are than their ridge wavelength.
const RANGE_SCALE: f64 = 4.0;

/// Gain between ridge octaves: the gain the ridged statistics were measured
/// with.
const RIDGE_GAIN: f32 = RIDGED_OCTAVES.gain;

/// Mean of a mountain crest's height, as a share of the layer height.
const CREST_MEAN: f32 = 0.5;

/// Spread of a mountain crest's height, as a share of the layer height.
const CREST_SPREAD: f32 = 0.25;

/// A terrain's height field before carving: pure, defined everywhere, and
/// the same for the same options, border field and elevation.
///
/// Heights are the world elevation E plus the terrain's relief. Near its
/// borders the border field scales the relief's layers towards the
/// amplitude agreed with the neighbours (the terrain's own ground), and
/// the ground gives way to the agreed border profiles (the final ground).
#[derive(Debug, Clone)]
pub(crate) struct Relief {
    base: BaseOptions,
    swell_seed: u32,
    hill_seed: u32,
    mask_seed: u32,
    ridge_seed: u32,
    mask_threshold: f32,
    roads: RoadGrades,
    field: Option<Arc<BorderField>>,
    /// Each active edge's agreed profile, in the field's edge order.
    borders: Arc<[BorderProfile]>,
    elevation: Elevation,
}

/// The relief's three layers at one point, at full scale, in metres: its
/// offset above E, then swells, hills and mountains.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Layers {
    pub offset: f32,
    pub values: [f32; 3],
}

impl Layers {
    /// The relief with each layer scaled by `scales`, in metres above E.
    pub(crate) fn combine(&self, scales: &[f64; 3]) -> f64 {
        let [a, b, c] = self.values.map(f64::from);
        f64::from(self.offset) + scales[0] * a + scales[1] * b + scales[2] * c
    }

    /// The relief at full scale, in metres above E.
    pub(crate) fn own(&self) -> f64 {
        self.combine(&[1.0; 3])
    }
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
            field: None,
            borders: Arc::new([]),
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

    /// This relief with its layers scaled near the edges of `field` (or
    /// never scaled, for `None`), meeting `borders`, one profile for each
    /// of the field's edges in order. Without `borders` the ground is the
    /// terrain's own, scaled but not meeting any profile.
    pub(crate) fn with_field(
        mut self,
        field: Option<Arc<BorderField>>,
        borders: Arc<[BorderProfile]>,
    ) -> Self {
        self.field = field;
        self.borders = borders;
        self
    }

    /// The border field, if the terrain has any active edge.
    pub(crate) fn field(&self) -> Option<&Arc<BorderField>> {
        self.field.as_ref()
    }

    /// The agreed profile of active edge `id`, if it has one.
    pub(crate) fn border(&self, id: EdgeId) -> Option<&BorderProfile> {
        let field = self.field.as_ref()?;
        let k = field.edges().iter().position(|e| e.id == id)?;
        self.borders.get(k)
    }

    /// The road grades at `p`: the terrain's own, faded towards its edges'
    /// targets near them.
    pub(crate) fn road_grades(&self, p: DVec2) -> RoadGrades {
        let Some(field) = &self.field else {
            return self.roads;
        };
        let own = [self.roads.grade, self.roads.max_grade].map(f64::from);
        let [grade, max_grade] = field.blend(p, own, |edge, along| {
            let r = edge.target.roads_at(along, edge.length);
            [r.grade, r.max_grade].map(f64::from)
        });
        RoadGrades {
            grade: grade as f32,
            max_grade: (max_grade as f32).max(grade as f32),
        }
    }

    /// Height at `p` without any scaling: E plus every layer as the
    /// settings give it.
    pub(crate) fn unfaded_height(&self, p: DVec2) -> f32 {
        (self.layer_values(p).own() + f64::from(self.elevation.height(p))) as f32
    }

    /// The typical height of the relief with its layers scaled by
    /// `scales`, in metres: its offset above E plus the mountain layer's
    /// mean. E isn't counted: it is the same for every terrain, so it drops
    /// out wherever terrains compare their mean heights.
    pub(crate) fn mean_height(&self, scales: [f32; 3]) -> f64 {
        let m = &self.base.mountains;
        f64::from(self.base.elevation)
            + f64::from(scales[2]) * f64::from(m.height * m.coverage * CREST_MEAN)
    }

    /// The spread of each layer, in metres.
    pub(crate) fn spreads(&self) -> LayerSpreads {
        layer_spreads(&self.base)
    }

    /// The final ground's height in metres at world position `p` (x and
    /// z), before carving: E plus the relief, meeting the agreed profiles
    /// near the border.
    pub(crate) fn height(&self, p: DVec2) -> f32 {
        self.base(p, false).0
    }

    /// [`height`](Self::height) less E at `p`.
    pub(crate) fn height_above_elevation(&self, p: DVec2) -> f32 {
        (f64::from(self.height(p)) - f64::from(self.elevation.height(p))) as f32
    }

    /// The terrain's own ground at `p`, in metres: E plus its relief, with
    /// its layers scaled near the border but not meeting any profile. What
    /// its offers describe.
    pub(crate) fn own_height(&self, p: DVec2) -> f32 {
        let layers = self.layer_values(p);
        (layers.combine(&self.scales(p, false).0) + f64::from(self.elevation.height(p))) as f32
    }

    /// The layer scales at `p`, with their gradients if `slope` (else
    /// zero).
    pub(crate) fn scales(&self, p: DVec2, slope: bool) -> ([f64; 3], [DVec2; 3]) {
        let Some(field) = &self.field else {
            return ([1.0; 3], [DVec2::ZERO; 3]);
        };
        match field.at(p, slope, &NoTargets) {
            Place::Beyond => ([1.0; 3], [DVec2::ZERO; 3]),
            Place::OnEdge {
                edge,
                along,
                direction,
            } => {
                let (scales, change) = field.edge_scales(edge, along);
                (scales, change.map(|c| direction * c))
            }
            Place::OnCorner { edge, end } => {
                let length = field.edges()[edge].length;
                let along = if end == 0 { 0.0 } else { length };
                (field.edge_scales(edge, along).0, [DVec2::ZERO; 3])
            }
            Place::Near(mix) => (mix.scales, mix.scale_gradients),
        }
    }

    /// Each layer's height at `p` at full scale, in metres, with the
    /// relief's offset above E.
    pub(crate) fn layer_values(&self, p: DVec2) -> Layers {
        let b = &self.base;
        let rolling = |layer: &NoiseLayer, seed: u32, erosion: f32| {
            if layer.height == 0.0 {
                return 0.0;
            }
            let octaves = Octaves {
                count: layer.octaves,
                gain: layer.roughness,
            };
            // Unit-spread noise, so ±height covers about two standard deviations.
            0.5 * layer.height * fbm(p / f64::from(layer.wavelength), seed, octaves, erosion)
        };
        let swells = rolling(&b.swells, self.swell_seed, 0.0);
        let hills = rolling(&b.hills, self.hill_seed, b.erosion);

        // Mountains: ridges inside a range mask.
        let m = &b.mountains;
        let mut mountains = 0.0;
        if m.height > 0.0 && m.coverage > 0.0 {
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
                mountains = m.height * mask * crest;
            }
        }
        Layers {
            offset: b.elevation,
            values: [swells, hills, mountains],
        }
    }

    /// Each layer's height at `p` at full scale, in metres, with its
    /// gradient: swells, hills, then mountains (see [`LayerSpreads`]).
    pub(crate) fn layers(&self, p: DVec2) -> [(f64, DVec2); 3] {
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

    /// The rate of change of [`own_height`](Self::own_height) along x and
    /// z at `p`.
    pub(crate) fn own_slope(&self, p: DVec2) -> DVec2 {
        let layers = self.layers(p);
        let (scales, scale_slopes) = self.scales(p, true);
        (0..3)
            .map(|k| layers[k].1 * scales[k] + scale_slopes[k] * layers[k].0)
            .sum::<DVec2>()
            + self.elevation.gradient(p)
    }

    /// The final ground's height before carving at `p`, meeting the agreed
    /// border profiles, and with `slope` its gradient (else zero), analytic
    /// throughout.
    pub(crate) fn base(&self, p: DVec2, slope: bool) -> (f32, DVec2) {
        let elevation = &self.elevation;
        let layers = self.layer_values(p);
        let e = f64::from(elevation.height(p));
        // The layers' gradients and E's, only when a gradient is asked for.
        let slopes = slope.then(|| {
            let layers = self.layers(p);
            let e_slope = if elevation.is_flat() {
                DVec2::ZERO
            } else {
                elevation.gradient(p)
            };
            (layers.map(|(_, g)| g), e_slope)
        });
        let own = |scales: &[f64; 3], scale_slopes: &[DVec2; 3]| {
            let height = layers.combine(scales) + e;
            let gradient = slopes.map_or(DVec2::ZERO, |(g, e_slope)| {
                let values = layers.values.map(f64::from);
                (0..3)
                    .map(|k| g[k] * scales[k] + scale_slopes[k] * values[k])
                    .sum::<DVec2>()
                    + e_slope
            });
            (height, gradient)
        };
        let unscaled = || {
            let (h, g) = own(&[1.0; 3], &[DVec2::ZERO; 3]);
            (h as f32, g)
        };
        let Some(field) = &self.field else {
            return unscaled();
        };
        if self.borders.is_empty() {
            let (scales, scale_slopes) = self.scales(p, slope);
            let (h, g) = own(&scales, &scale_slopes);
            return (h as f32, g);
        }

        let targets = EdgeTargets {
            borders: &self.borders,
            field,
            layers: &layers,
            e,
            slopes: slopes.as_ref(),
        };
        match field.at(p, slope, &targets) {
            Place::Beyond => unscaled(),
            // Exactly the agreed profile: both regions on the edge give the
            // same height here.
            Place::OnEdge {
                edge,
                along,
                direction,
            } => {
                let target = targets.target(edge, along, &field.edge_scales(edge, along));
                (
                    target.height as f32,
                    target.gradient + direction * target.along_slope,
                )
            }
            Place::OnCorner { edge, end } => {
                let height = self.borders[edge].corners[end];
                let length = field.edges()[edge].length;
                let along = if end == 0 { 0.0 } else { length };
                let target = targets.target(edge, along, &field.edge_scales(edge, along));
                (height, if slope { target.gradient } else { DVec2::ZERO })
            }
            // The terrain's own scaled ground giving way to the mixed
            // profiles towards the border.
            Place::Near(mix) => {
                let (own_height, own_slope) = own(&mix.scales, &mix.scale_gradients);
                let height = mix.border + mix.own * (own_height - mix.border);
                let gradient = if slope {
                    mix.border_gradient
                        + mix.own_gradient * (own_height - mix.border)
                        + (own_slope - mix.border_gradient) * mix.own
                } else {
                    DVec2::ZERO
                };
                (height as f32, gradient)
            }
        }
    }
}

/// The agreed profiles of a terrain's active edges, read at one point.
struct EdgeTargets<'a> {
    borders: &'a [BorderProfile],
    field: &'a BorderField,
    /// The terrain's relief layers at the point.
    layers: &'a Layers,
    /// E at the point.
    e: f64,
    /// The layers' gradients and E's, when a gradient is asked for.
    slopes: Option<&'a ([DVec2; 3], DVec2)>,
}

impl Targets for EdgeTargets<'_> {
    fn target(&self, edge: usize, along: f64, scales: &([f64; 3], [f64; 3])) -> Target {
        let border = &self.borders[edge];
        match &border.shape {
            ProfileShape::Shared(knots) => {
                let (height, along_slope) = knots.at(along);
                Target {
                    height: height + self.e,
                    along_slope,
                    gradient: self.slopes.map_or(DVec2::ZERO, |(_, e_slope)| *e_slope),
                }
            }
            // The shared relief itself at the point, scaled as the edge
            // asks, plus the corner offsets. Both regions compute it from
            // the same layers, so they agree on the edge exactly.
            ProfileShape::Relative { offsets, ramp } => {
                let length = self.field.edges()[edge].length;
                let (shares, share_slopes) = pin_shares(along, length, *ramp);
                let values = self.layers.values.map(f64::from);
                let height = self.layers.combine(&scales.0)
                    + self.e
                    + offsets[0] * shares[0]
                    + offsets[1] * shares[1];
                let along_slope = (0..3).map(|k| scales.1[k] * values[k]).sum::<f64>()
                    + offsets[0] * share_slopes[0]
                    + offsets[1] * share_slopes[1];
                let gradient = self.slopes.map_or(DVec2::ZERO, |(g, e_slope)| {
                    (0..3).map(|k| g[k] * scales.0[k]).sum::<DVec2>() + *e_slope
                });
                Target {
                    height,
                    along_slope,
                    gradient,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::TerrainPreset;

    /// Checks `relief`'s slope against central differences of its heights
    /// `half` metres either side of a grid of points, in x and in z: each
    /// difference must match the slope averaged over the same span
    /// (Simpson's rule). Spans across a sharp crest, where the slope jumps
    /// between neighbouring samples, are skipped: mountain ridges have them
    /// every few metres, so up to a quarter of the spans.
    ///
    /// `half` follows the preset's scale: 2 m for medium-scale presets,
    /// whose mountain heights carry up to about 5e-3 m of f32 rounding (too
    /// much for shorter spans, and it sets the tolerance), and 1 m for the
    /// shipped presets at half that scale, whose ridges are twice as close
    /// and whose rounding is half as large.
    fn assert_slope_matches(relief: &Relief, half: f64, label: &str) {
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
                        let at = p + axis * half * (2.0 * f64::from(k) / f64::from(STEPS) - 1.0);
                        relief.own_slope(at).dot(axis)
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
                let difference = (f64::from(relief.height(p + axis * half))
                    - f64::from(relief.height(p - axis * half)))
                    / (2.0 * half);
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
        for preset in TerrainPreset::all() {
            assert_slope_matches(&Relief::new(&preset.terrain), 1.0, &preset.name);
        }
        for preset in crate::fixtures::all() {
            assert_slope_matches(&Relief::new(&preset.terrain), 2.0, &preset.name);
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
