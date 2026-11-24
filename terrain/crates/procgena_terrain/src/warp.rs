//! Border warp: straight region borders made irregular, the same on both
//! sides.
//!
//! The unit of warping is one straight polygon side. Its points move along
//! the side's normal by noise that fades to nothing at both ends, so the
//! side's corners stay put. How far a side may move is limited by the
//! pieces around it, so warped borders never cross each other or close up
//! a thin region.

use std::collections::BTreeMap;
use std::fmt;

use glam::DVec2;

use crate::calibration::NOISE_SLOPE_MAX;
use crate::graph::MIN_EDGE;
use crate::hash::{GRAPH_BORDER_WARP, derive_seed, mix64, seed_for};
use crate::noise::gradient_noise;
use crate::ports::FEATURE_CLEARANCE;
use crate::region::corner_id;
use crate::stitch::RegionSpec;

/// The shortest [`BorderWarp::wavelength`] allowed, in metres: shorter
/// wiggles would put bends closer together than a river or road crossing
/// needs.
pub const MIN_WARP_WAVELENGTH: f64 = 4.0 * FEATURE_CLEARANCE;

/// The steepest a warped border may run against the straight line between
/// its corners: 1 is 45°.
const MAX_WARP_SLOPE: f64 = 1.0;

/// How many straight pieces a warped side has per wavelength.
const POINTS_PER_WAVELENGTH: f64 = 16.0;

/// The largest value [`gradient_noise`] takes, for unit gradients in 2D:
/// dividing by it scales the noise into −1 to 1.
const NOISE_PEAK: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// The steepest the fade gets, per unit of its input: smoothstep's slope
/// at its middle.
const FADE_SLOPE_MAX: f64 = 1.5;

/// The most smoothstep(x) can be, as a share of x, for x from 0 to 1.
const FADE_RATIO_MAX: f64 = 1.125;

/// How much of the room between two pieces meeting at a corner each may
/// use: below 1, so warped sides stay clear of the line halfway between.
const WEDGE_SHARE: f64 = 0.5;

/// How region borders are made irregular. The default passes
/// [`validate`](Self::validate); an amplitude of 0 leaves borders straight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BorderWarp {
    /// The most a border moves from the straight line between its corners,
    /// in metres. Short or sharply angled sides, and thin regions, move
    /// less, so borders never cross.
    pub amplitude: f64,
    /// The length of the border's wiggles, in metres. At least
    /// [`MIN_WARP_WAVELENGTH`].
    pub wavelength: f64,
}

impl Default for BorderWarp {
    fn default() -> Self {
        Self {
            amplitude: 100.0,
            wavelength: 800.0,
        }
    }
}

/// Why border warp settings were refused.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum BorderWarpError {
    /// The amplitude must be finite and not negative.
    Amplitude(f64),
    /// The wavelength must be finite and at least [`MIN_WARP_WAVELENGTH`].
    Wavelength(f64),
    /// The amplitude is too large for the wavelength: the border could run
    /// steeper than 45° against the line between its corners. The largest
    /// amplitude allowed at this wavelength is given.
    TooSteep {
        /// The amplitude asked for, in metres.
        amplitude: f64,
        /// The largest amplitude allowed, in metres.
        most: f64,
    },
}

impl fmt::Display for BorderWarpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Amplitude(a) => write!(
                f,
                "the border warp amplitude must be finite and not negative, not {a}"
            ),
            Self::Wavelength(w) => write!(
                f,
                "the border warp wavelength must be finite and at least {MIN_WARP_WAVELENGTH} m, \
                 not {w}"
            ),
            Self::TooSteep { amplitude, most } => write!(
                f,
                "a border warp amplitude of {amplitude} m is too steep for its wavelength; at most \
                 {most} m"
            ),
        }
    }
}

impl std::error::Error for BorderWarpError {}

impl BorderWarp {
    /// Checks the settings.
    pub fn validate(&self) -> Result<(), BorderWarpError> {
        if !self.amplitude.is_finite() || self.amplitude < 0.0 {
            return Err(BorderWarpError::Amplitude(self.amplitude));
        }
        if !self.wavelength.is_finite() || self.wavelength < MIN_WARP_WAVELENGTH {
            return Err(BorderWarpError::Wavelength(self.wavelength));
        }
        let most = MAX_WARP_SLOPE / self.slope_per_metre();
        if self.amplitude > most {
            return Err(BorderWarpError::TooSteep {
                amplitude: self.amplitude,
                most,
            });
        }
        Ok(())
    }

    /// The length over which a side's warp fades in from each end.
    fn fade_length(&self) -> f64 {
        0.5 * self.wavelength
    }

    /// The steepest a side warped by 1 m can run against its chord.
    ///
    /// The offset is `A·fade(s)·n(s/λ)` with |n| ≤ 1 and fade ≤ 1, so its
    /// slope is at most `A·(fade′ + n′/λ)`.
    fn slope_per_metre(&self) -> f64 {
        FADE_SLOPE_MAX / self.fade_length()
            + f64::from(NOISE_SLOPE_MAX) / (NOISE_PEAK * self.wavelength)
    }
}

/// How far side `a`–`b` may be warped, in metres, given every other
/// straight piece of border near it, from all regions around it. 0 when
/// the warp is off or there's no room.
///
/// The result depends only on the side and the set of pieces, not on their
/// order or direction, so both regions on a side get the same bits.
/// `others` may include the side itself (either way round); it is skipped.
pub(crate) fn side_amplitude(
    a: DVec2,
    b: DVec2,
    others: impl IntoIterator<Item = (DVec2, DVec2)>,
    warp: &BorderWarp,
) -> f64 {
    let mut amplitude = warp.amplitude;
    if amplitude == 0.0 {
        return 0.0;
    }
    // Every piece in its canonical direction, so the rounding is the same
    // whichever way round each was given.
    let (a, b) = canonical(a, b);
    for (c, d) in others {
        let (c, d) = canonical(c, d);
        if c == a && d == b {
            continue;
        }
        // A piece meeting this side at a corner: each keeps to its own side
        // of the line halfway between them.
        let shared = [(a, b), (b, a)].into_iter().find_map(|(corner, far)| {
            let other = if c == corner {
                Some(d)
            } else if d == corner {
                Some(c)
            } else {
                None
            };
            other.map(|o| (corner, far, o))
        });
        let limit = match shared {
            Some((corner, far, other)) => {
                let angle = (far - corner).angle_to(other - corner).abs();
                let half = 0.5 * angle.min(std::f64::consts::FRAC_PI_2);
                WEDGE_SHARE * half.tan() * warp.fade_length() / FADE_RATIO_MAX
            }
            // A piece apart from it: each keeps half of the room, less the
            // shortest edge, so a thin region stays open.
            None => 0.5 * (segment_distance(a, b, c, d) - MIN_EDGE),
        };
        amplitude = amplitude.min(limit.max(0.0));
    }
    amplitude
}

/// The regions `specs` with every border they share warped as `warp`
/// says, for the world seeded with `seed`. Sides on the world's edge (used
/// by one region only) stay straight, and so does every corner of every
/// polygon.
///
/// Both regions on a border get exactly the same new points, so the result
/// can go straight to [`StitchedWorld::build`](crate::StitchedWorld::build).
/// The same specs, warp and seed always give the same result.
///
/// # Errors
///
/// If `warp` isn't valid (see [`BorderWarp::validate`]).
pub fn warp_borders(
    specs: &[RegionSpec],
    warp: &BorderWarp,
    seed: u32,
) -> Result<Vec<RegionSpec>, BorderWarpError> {
    warp.validate()?;

    // Every side of every polygon, once, with how many regions use it.
    let mut uses: BTreeMap<SideKey, usize> = BTreeMap::new();
    for spec in specs {
        for (a, b) in polygon_sides(&spec.polygon) {
            *uses.entry(side_key(a, b)).or_default() += 1;
        }
    }
    let pieces: Vec<(DVec2, DVec2)> = uses.keys().map(|k| k.points()).collect();

    // Each region's outline with its shared sides replaced by warped ones.
    let mut out = Vec::with_capacity(specs.len());
    for spec in specs {
        let mut polygon = Vec::with_capacity(spec.polygon.len());
        for (a, b) in polygon_sides(&spec.polygon) {
            if uses[&side_key(a, b)] < 2 {
                polygon.push(a);
                continue;
            }
            let amplitude = side_amplitude(a, b, pieces.iter().copied(), warp);
            let points = warp_side(a, b, amplitude, warp, seed);
            polygon.extend_from_slice(&points[..points.len() - 1]);
        }
        out.push(RegionSpec {
            polygon,
            ..spec.clone()
        });
    }
    Ok(out)
}

/// A polygon side by the bits of its ends, smaller end first: the same
/// whichever region walks it, in whichever direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SideKey([u64; 4]);

impl SideKey {
    /// The side's two ends, smaller first.
    fn points(self) -> (DVec2, DVec2) {
        let [ax, ay, bx, by] = self.0.map(f64::from_bits);
        (DVec2::new(ax, ay), DVec2::new(bx, by))
    }
}

/// The key of side `a`–`b`.
fn side_key(a: DVec2, b: DVec2) -> SideKey {
    let (a, b) = canonical(a, b);
    let bits = |v: f64| if v == 0.0 { 0 } else { v.to_bits() };
    SideKey([bits(a.x), bits(a.y), bits(b.x), bits(b.y)])
}

/// The sides of closed polygon `points`, in order, each from one point to
/// the next.
fn polygon_sides(points: &[DVec2]) -> impl Iterator<Item = (DVec2, DVec2)> + '_ {
    let n = points.len();
    (0..n).map(move |i| (points[i], points[(i + 1) % n]))
}

/// Side `a`–`b` warped by up to `amplitude` metres (from
/// [`side_amplitude`]), from `a` to `b`, for the world seeded with `seed`.
///
/// Both regions on a side get the same points, in opposite orders if they
/// walk it in opposite directions. An amplitude of 0 gives `[a, b]`.
pub(crate) fn warp_side(
    a: DVec2,
    b: DVec2,
    amplitude: f64,
    warp: &BorderWarp,
    seed: u32,
) -> Vec<DVec2> {
    if amplitude == 0.0 {
        return vec![a, b];
    }

    // Work in the side's canonical direction, so both regions agree.
    let (start, end) = canonical(a, b);
    let flipped = start != a;
    let length = start.distance(end);
    let normal = (end - start).perp() / length;
    let side_seed = derive_seed(
        seed_for(seed, GRAPH_BORDER_WARP),
        fold(mix64(corner_id(start).0, corner_id(end).0)),
    );

    // Points every few tens of metres, moved along the normal.
    let pieces = (length / warp.wavelength * POINTS_PER_WAVELENGTH)
        .ceil()
        .max(1.0) as usize;
    let mut points = Vec::with_capacity(pieces + 1);
    points.push(start);
    for i in 1..pieces {
        let t = i as f64 / pieces as f64;
        let s = t * length;
        let fade = smoothstep((s.min(length - s) / warp.fade_length()).min(1.0));
        let noise = side_noise(s / warp.wavelength, side_seed);
        points.push(start + (end - start) * t + normal * (amplitude * fade * noise));
    }
    points.push(end);
    if flipped {
        points.reverse();
    }
    points
}

/// The ends of piece `a`–`b`, smaller (by x, then z) first.
fn canonical(a: DVec2, b: DVec2) -> (DVec2, DVec2) {
    if (b.x, b.y) < (a.x, a.y) {
        (b, a)
    } else {
        (a, b)
    }
}

/// The side's noise at `x` wavelengths along it, from −1 to 1.
fn side_noise(x: f64, seed: u32) -> f64 {
    let n = f64::from(gradient_noise(noise_point(x), seed).value) / NOISE_PEAK;
    n.clamp(-1.0, 1.0)
}

/// Where the side's noise is read, `x` noise cells along a slanting line.
///
/// A line along a lattice row or column could run through whole cells
/// where the noise is exactly 0 (for some pairs of corner gradients), so
/// the line slants across the cells.
fn noise_point(x: f64) -> DVec2 {
    DVec2::new(0.31, 0.47) + DVec2::new(0.8, 0.6) * x
}

/// `3x² − 2x³`: 0 at 0, 1 at 1, flat at both.
fn smoothstep(x: f64) -> f64 {
    x * x * (3.0 - 2.0 * x)
}

/// A 64-bit hash folded into a seed.
fn fold(h: u64) -> u32 {
    (h as u32) ^ ((h >> 32) as u32)
}

/// The shortest distance between segments `a`–`b` and `c`–`d`.
pub(crate) fn segment_distance(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> f64 {
    let crosses = |p: DVec2, q: DVec2, r: DVec2, s: DVec2| {
        let side = |o: DVec2, x: DVec2, y: DVec2| (x - o).perp_dot(y - o);
        side(p, q, r) * side(p, q, s) < 0.0 && side(r, s, p) * side(r, s, q) < 0.0
    };
    if crosses(a, b, c, d) {
        return 0.0;
    }
    point_segment(a, c, d)
        .min(point_segment(b, c, d))
        .min(point_segment(c, a, b))
        .min(point_segment(d, a, b))
}

/// The distance from `p` to segment `a`–`b`.
fn point_segment(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The strongest warp allowed at `wavelength`.
    fn strongest(wavelength: f64) -> BorderWarp {
        let probe = BorderWarp {
            amplitude: 0.0,
            wavelength,
        };
        BorderWarp {
            amplitude: MAX_WARP_SLOPE / probe.slope_per_metre(),
            wavelength,
        }
    }

    #[test]
    fn settings_are_checked() {
        assert_eq!(BorderWarp::default().validate(), Ok(()));
        let ok = BorderWarp::default();
        assert_eq!(
            BorderWarp {
                amplitude: -1.0,
                ..ok
            }
            .validate(),
            Err(BorderWarpError::Amplitude(-1.0))
        );
        assert_eq!(
            BorderWarp {
                wavelength: 200.0,
                ..ok
            }
            .validate(),
            Err(BorderWarpError::Wavelength(200.0))
        );
        assert!(matches!(
            BorderWarp {
                amplitude: 1000.0,
                ..ok
            }
            .validate(),
            Err(BorderWarpError::TooSteep { .. })
        ));
        assert_eq!(strongest(800.0).validate(), Ok(()));
        for bad in [
            BorderWarp {
                amplitude: f64::NAN,
                ..ok
            },
            BorderWarp {
                wavelength: f64::NAN,
                ..ok
            },
            BorderWarp {
                wavelength: f64::INFINITY,
                ..ok
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_noise_stays_within_its_peak() {
        // Scaled by the peak, the noise needs no clamping.
        let mut largest = 0.0f64;
        for seed in 0..20 {
            for i in 0..20_000 {
                let x = f64::from(i) * 0.013 - 100.0;
                let n = f64::from(gradient_noise(noise_point(x), seed).value) / NOISE_PEAK;
                largest = largest.max(n.abs());
            }
        }
        assert!(largest <= 1.0, "{largest}");
        assert!(largest > 0.5, "the noise should use its range: {largest}");
    }

    #[test]
    fn no_warp_keeps_the_side_straight() {
        let (a, b) = (DVec2::new(3.0, -2.0), DVec2::new(-900.0, 50.0));
        assert_eq!(warp_side(a, b, 0.0, &BorderWarp::default(), 1), vec![a, b]);
        let off = BorderWarp {
            amplitude: 0.0,
            ..BorderWarp::default()
        };
        assert_eq!(side_amplitude(a, b, [], &off), 0.0);
    }

    #[test]
    fn both_directions_give_the_same_points() {
        let (a, b) = (DVec2::new(3.0, -2.0), DVec2::new(-900.0, 1250.0));
        let warp = strongest(800.0);
        let forward = warp_side(a, b, warp.amplitude, &warp, 9);
        let mut backward = warp_side(b, a, warp.amplitude, &warp, 9);
        backward.reverse();
        assert_eq!(forward, backward);
        assert_eq!((forward[0], *forward.last().unwrap()), (a, b));
        // Another world seed warps it differently.
        assert_ne!(forward, warp_side(a, b, warp.amplitude, &warp, 10));
    }

    #[test]
    fn a_warped_side_stays_within_its_amplitude_and_slope() {
        for wavelength in [MIN_WARP_WAVELENGTH, 800.0, 3000.0] {
            let warp = strongest(wavelength);
            for seed in 0..30 {
                let (a, b) = (DVec2::ZERO, DVec2::new(5000.0, 1200.0));
                let points = warp_side(a, b, warp.amplitude, &warp, seed);
                let (dir, normal) = ((b - a).normalize(), (b - a).perp().normalize());
                let mut furthest = 0.0f64;
                for w in points.windows(2) {
                    let along = (w[1] - w[0]).dot(dir);
                    let across = (w[1] - w[0]).dot(normal);
                    assert!(along > 0.0, "the side never doubles back");
                    assert!(
                        across.abs() <= MAX_WARP_SLOPE * along * (1.0 + 1e-9),
                        "slope {} at wavelength {wavelength}",
                        across / along
                    );
                    furthest = furthest.max((w[1] - a).dot(normal).abs());
                }
                assert!(furthest <= warp.amplitude * (1.0 + 1e-12));
            }
        }
    }

    #[test]
    fn sides_meeting_at_a_sharp_corner_do_not_cross() {
        let warp = strongest(800.0);
        for degrees in [5.0f64, 20.0, 60.0, 120.0] {
            let corner = DVec2::new(10.0, 20.0);
            let a = corner + DVec2::new(3000.0, 0.0);
            let b = corner + DVec2::from_angle(degrees.to_radians()) * 3000.0;
            let pieces = [(corner, a), (corner, b)];
            let amp_a = side_amplitude(corner, a, pieces, &warp);
            let amp_b = side_amplitude(b, corner, pieces, &warp);
            assert_eq!(amp_a, amp_b, "the same room on both sides");
            assert!(amp_a < warp.amplitude || degrees > 90.0);
            for seed in 0..20 {
                let pa = warp_side(corner, a, amp_a, &warp, seed);
                let pb = warp_side(corner, b, amp_b, &warp, seed + 100);
                for (i, wa) in pa.windows(2).enumerate() {
                    for (j, wb) in pb.windows(2).enumerate() {
                        if i == 0 && j == 0 {
                            continue;
                        }
                        assert!(
                            segment_distance(wa[0], wa[1], wb[0], wb[1]) > 0.0,
                            "{degrees}°: pieces {i} and {j} cross"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn opposite_sides_of_a_thin_strip_stay_apart() {
        let warp = strongest(800.0);
        for width in [300.0, 500.0, 700.0, 2000.0] {
            let (a, b) = (DVec2::ZERO, DVec2::new(4000.0, 0.0));
            let (c, d) = (DVec2::new(0.0, width), DVec2::new(4000.0, width));
            let pieces = [(a, b), (c, d)];
            let amp_low = side_amplitude(a, b, pieces, &warp);
            let amp_high = side_amplitude(c, d, pieces, &warp);
            assert_eq!(amp_low, amp_high);
            let expected = (0.5 * (width - MIN_EDGE)).clamp(0.0, warp.amplitude);
            assert!((amp_low - expected).abs() < 1e-9, "{width}: {amp_low}");
            for seed in 0..20 {
                let low = warp_side(a, b, amp_low, &warp, seed);
                let high = warp_side(c, d, amp_high, &warp, seed + 1);
                let gap = low
                    .windows(2)
                    .flat_map(|l| {
                        high.windows(2)
                            .map(move |h| segment_distance(l[0], l[1], h[0], h[1]))
                    })
                    .fold(f64::INFINITY, f64::min);
                assert!(gap >= MIN_EDGE.min(width) - 1e-9, "{width}: {gap}");
            }
        }
    }

    /// Two regions split by a bent border, as in the seam tests.
    fn two_specs() -> Vec<RegionSpec> {
        let points = |p: &[(f64, f64)]| p.iter().map(|&(x, z)| DVec2::new(x, z)).collect();
        let options = crate::TerrainPreset::Plains.options();
        vec![
            RegionSpec {
                id: crate::RegionId(1),
                polygon: points(&[
                    (-2000.0, -1500.0),
                    (0.0, -1500.0),
                    (150.0, -700.0),
                    (-200.0, 0.0),
                    (100.0, 800.0),
                    (0.0, 1500.0),
                    (-2000.0, 1500.0),
                ]),
                options: options.clone(),
            },
            RegionSpec {
                id: crate::RegionId(2),
                polygon: points(&[
                    (0.0, -1500.0),
                    (2000.0, -1500.0),
                    (2000.0, 1500.0),
                    (0.0, 1500.0),
                    (100.0, 800.0),
                    (-200.0, 0.0),
                    (150.0, -700.0),
                ]),
                options,
            },
        ]
    }

    #[test]
    fn warped_polygons_share_their_border_points() {
        let specs = two_specs();
        let warped = warp_borders(&specs, &strongest(800.0), 4).unwrap();
        let (west, east) = (&warped[0].polygon, &warped[1].polygon);
        // Every original corner is kept, and the shared border gains points
        // that both regions hold, bit for bit.
        for spec in &specs {
            for p in &spec.polygon {
                assert!(warped.iter().any(|w| w.polygon.contains(p)));
            }
        }
        let bits = |p: &DVec2| (p.x.to_bits(), p.y.to_bits());
        let shared: Vec<&DVec2> = west
            .iter()
            .filter(|p| east.iter().any(|q| bits(q) == bits(p)))
            .collect();
        assert!(shared.len() > 50, "{} shared points", shared.len());
        // The world's edge stays straight: the west region's outer corners
        // are still next to each other.
        let at = |p: (f64, f64)| {
            west.iter()
                .position(|q| *q == DVec2::new(p.0, p.1))
                .unwrap()
        };
        let next = |p: (f64, f64)| (at(p) + 1) % west.len();
        assert_eq!(next((-2000.0, -1500.0)), at((0.0, -1500.0)));
        assert_eq!(next((0.0, 1500.0)), at((-2000.0, 1500.0)));
        assert_eq!(next((-2000.0, 1500.0)), at((-2000.0, -1500.0)));
        // Warping again gives the same, and no warp changes nothing.
        assert_eq!(
            warp_borders(&specs, &strongest(800.0), 4).unwrap()[1].polygon,
            *east
        );
        let off = BorderWarp {
            amplitude: 0.0,
            ..BorderWarp::default()
        };
        let unwarped = warp_borders(&specs, &off, 4).unwrap();
        for (a, b) in unwarped.iter().zip(&specs) {
            assert_eq!(a.polygon, b.polygon);
        }
        let bad = BorderWarp {
            wavelength: 1.0,
            ..BorderWarp::default()
        };
        assert_eq!(
            warp_borders(&specs, &bad, 4).unwrap_err(),
            BorderWarpError::Wavelength(1.0)
        );
    }
}
