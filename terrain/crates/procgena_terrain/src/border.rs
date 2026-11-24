//! What a region's border edges ask for at a point near them, mixed from
//! every edge within reach, so the result is the same however the edges are
//! cut into straight segments.
//!
//! Each edge pulls on a point with a weight that falls with distance. The
//! pull of a whole edge is the integral of that weight along it, worked out
//! in closed form segment by segment, so cutting a straight edge into
//! pieces changes nothing. The weight reaches zero at a given reach, so
//! edges farther away can be left out exactly.

use glam::DVec2;

use crate::region::Segment;

/// The exponent of the weight that mixes the edges' targets: a point pulls
/// each bit of edge `ds` at distance `r` by `r^−(TARGET_POWER + 1) ds`, so a
/// straight edge pulls like a point weighted by `distance^−TARGET_POWER`.
const TARGET_POWER: u32 = 3;

/// The exponent of the weight that measures the soft distance to the
/// border, as for [`TARGET_POWER`]. Higher is closer to the plain nearest
/// distance, with a sharper (still smooth) crease along the bisectors of
/// concave bends. At 16 the soft distance is within about 6% of the true
/// distance outside convex corners and 4% inside concave ones.
const DISTANCE_POWER: u32 = 16;

/// `∫ (1 + t²)^(−(DISTANCE_POWER + 1) / 2) dt` over the whole line: what a
/// straight edge without end pulls at distance 1, before the cutoff.
const LINE_PULL: f64 = line_pull(DISTANCE_POWER);

/// `∫ (1 + t²)^(−(power + 1) / 2) dt` over the whole line, for a power of at
/// least 1: π for power 1 and 2 for power 2, then times `p / (p + 1)` from
/// power `p` to `p + 2`.
const fn line_pull(power: u32) -> f64 {
    let (mut p, mut pull) = if power % 2 == 1 {
        (1, std::f64::consts::PI)
    } else {
        (2, 2.0)
    };
    while p < power {
        pull *= p as f64 / (p + 1) as f64;
        p += 2;
    }
    pull
}

/// What the edges within reach ask for at one point.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pull<const N: usize> {
    /// The sum of the target weights.
    weight: f64,
    /// The sum of each target value times its weight.
    weighted: [f64; N],
    /// The sum of the distance weights.
    closeness: f64,
}

impl<const N: usize> Default for Pull<N> {
    fn default() -> Self {
        Self {
            weight: 0.0,
            weighted: [0.0; N],
            closeness: 0.0,
        }
    }
}

impl<const N: usize> Pull<N> {
    /// Adds the pull of `segment` on `p`, out to `reach` metres: on the soft
    /// distance and on the targets (see [`add_distance`](Self::add_distance)
    /// and [`add_targets`](Self::add_targets)). `target`
    /// gives the values the edge asks for, from the distance along the
    /// edge and the signed distance of `p` across the segment (positive on
    /// its normal's side).
    #[cfg(test)]
    pub(crate) fn add(
        &mut self,
        segment: &Segment,
        p: DVec2,
        reach: f64,
        target: &impl Fn(f64, f64) -> [f64; N],
    ) {
        self.add_distance(segment, p, reach);
        self.add_targets(segment, p, reach, 8.0, target);
    }

    /// Adds only the pull of `segment` on the soft distance (see
    /// [`add`](Self::add)), which is much cheaper than the targets: first
    /// find whether `p` is near enough for the targets to matter.
    pub(crate) fn add_distance(&mut self, segment: &Segment, p: DVec2, reach: f64) {
        if let Some((a, start, end, _, _)) = Self::stretch(segment, p, reach) {
            self.closeness += power_integral(start, end, a, DISTANCE_POWER) - (end - start);
        }
    }

    /// Where `p` stands against `segment`, in units of `reach`: the distance
    /// across, the stretch of the segment within reach along the line from
    /// the foot, the signed distance across in metres and the distance
    /// along the edge of the foot; `None` if no part is within reach.
    fn stretch(segment: &Segment, p: DVec2, reach: f64) -> Option<(f64, f64, f64, f64, f64)> {
        let offset = p - segment.start;
        let foot = offset.dot(segment.direction);
        let across = offset.dot(segment.normal);
        // On the line beyond the segment's end, `across` is 0; a sliver
        // above it keeps the closed forms finite and changes nothing else.
        let a = (across.abs() / reach).max(1e-12);
        if a >= 1.0 {
            return None;
        }
        let half = (1.0 - a * a).sqrt();
        let start = (-foot / reach).max(-half);
        let end = ((segment.length - foot) / reach).min(half);
        (start < end).then_some((a, start, end, across, segment.offset + foot))
    }

    /// Adds only the pull of `segment` on the targets (see
    /// [`add`](Self::add)), reading them every `step` metres along the edge
    /// near `p` and farther apart beyond: `step` should be about the
    /// shortest distance over which the targets change shape.
    pub(crate) fn add_targets(
        &mut self,
        segment: &Segment,
        p: DVec2,
        reach: f64,
        step: f64,
        target: &impl Fn(f64, f64) -> [f64; N],
    ) {
        if N == 0 {
            return;
        }
        let Some((a, start, end, across, along_foot)) = Self::stretch(segment, p, reach) else {
            return;
        };

        // Piece by piece between the points where targets are read, each
        // target taken as changing linearly between two of them, except
        // across the two pieces either side of the foot: there a parabola
        // through the three nodes, smooth at the foot, so the mix keeps the
        // target's slope across the edge. Each node's targets are read once.
        let h = step / reach;
        let mut middle: Option<[[f64; N]; 3]> = None;
        let mut k = node_index(start * reach, step);
        let mut n0 = node(k, step) / reach;
        let mut t0 = target(along_foot + n0 * reach, across);
        loop {
            let n1 = node(k + 1, step) / reach;
            let t1 = target(along_foot + n1 * reach, across);
            let (x0, x1) = (n0.max(start), n1.min(end));
            if x0 < x1 && (k == -1 || k == 0) {
                let [before, at, after] = *middle.get_or_insert_with(|| {
                    [-1.0, 0.0, 1.0].map(|i| target(along_foot + i * step, across))
                });
                let (w0, w1, w2) = moments(x0, x1, a, TARGET_POWER, true);
                self.weight += w0;
                for i in 0..N {
                    let b = (after[i] - before[i]) / (2.0 * h);
                    let c = (after[i] - 2.0 * at[i] + before[i]) / (2.0 * h * h);
                    self.weighted[i] += at[i] * w0 + b * w1 + c * w2;
                }
            } else if x0 < x1 {
                let (w0, w1, _) = moments(x0, x1, a, TARGET_POWER, false);
                self.weight += w0;
                for i in 0..N {
                    let slope = (t1[i] - t0[i]) / (n1 - n0);
                    self.weighted[i] += t0[i] * w0 + slope * (w1 - n0 * w0);
                }
            }
            if n1 >= end {
                break;
            }
            (k, n0, t0) = (k + 1, n1, t1);
        }
    }

    /// The soft distance to the border, in metres, for pulls added out to
    /// `reach`: about the distance to the nearest edge, but smooth where
    /// two edges are about as near. Infinite where nothing is within reach.
    pub(crate) fn distance(&self, reach: f64) -> f64 {
        if self.closeness <= 0.0 {
            return f64::INFINITY;
        }
        reach * (self.closeness / LINE_PULL).powf(-1.0 / f64::from(DISTANCE_POWER))
    }

    /// The edges' targets mixed by their pull, or `None` where nothing is
    /// within reach.
    pub(crate) fn mixed(&self) -> Option<[f64; N]> {
        (self.weight > 0.0).then(|| self.weighted.map(|v| v / self.weight))
    }
}

/// Whether the soft distance from `p` to `segments`, with pulls out to
/// `reach`, is certainly at least `width`, judged from each segment's plain
/// distance and length alone, which is much cheaper than adding the pulls.
/// `false` means only that it may be nearer: add the pulls to find out.
pub(crate) fn surely_beyond<'a>(
    segments: impl Iterator<Item = &'a Segment>,
    p: DVec2,
    reach: f64,
    width: f64,
) -> bool {
    // Each segment pulls at most as hard as if all of its stretch within
    // reach (at most `2 × reach` long) were at its plain distance.
    let mut most = 0.0;
    for segment in segments {
        let distance = segment.project(p).distance / reach;
        if distance == 0.0 {
            return false;
        }
        if distance >= 1.0 {
            continue;
        }
        let stretch = (segment.length / reach).min(2.0);
        most += stretch * (half_power(distance * distance, -(DISTANCE_POWER as i32 + 1)) - 1.0);
    }
    // The soft distance falls as the pull grows; a margin of 1% in the pull
    // covers rounding in both sums.
    let pull = Pull::<0> {
        closeness: most * 1.01,
        ..Default::default()
    };
    pull.distance(reach) >= width
}

/// Which segments of a set of edges can pull on each part of the plane,
/// out to a reach, so a point visits only those. Segments left out of a
/// point's list are beyond the reach and would add nothing, so the result
/// is exactly what visiting every segment gives.
#[derive(Debug, Clone)]
pub(crate) struct SegmentIndex {
    origin: DVec2,
    cell: f64,
    width: usize,
    depth: usize,
    /// For each cell, the (edge, segment) pairs within reach of it, in
    /// edge and then segment order.
    cells: Vec<Vec<(usize, usize)>>,
}

impl SegmentIndex {
    /// The index of `edges` (each a list of segments) for pulls out to
    /// `reach` metres.
    pub(crate) fn new<'a>(edges: impl Iterator<Item = &'a [Segment]> + Clone, reach: f64) -> Self {
        // A grid of cells `reach` wide over every segment, widened by the
        // reach.
        let (lo, hi) = edges
            .clone()
            .flatten()
            .flat_map(|s| [s.start, s.start + s.direction * s.length])
            .fold((DVec2::INFINITY, DVec2::NEG_INFINITY), |(lo, hi), q| {
                (lo.min(q), hi.max(q))
            });
        let origin = lo - DVec2::splat(reach);
        let cell = reach.max(1.0);
        let size = ((hi + DVec2::splat(reach) - origin) / cell).ceil();
        let (width, depth) = (size.x.max(1.0) as usize, size.y.max(1.0) as usize);
        let mut cells = vec![Vec::new(); width * depth];

        // Each segment in every cell its widened box touches.
        for (e, segments) in edges.enumerate() {
            for (k, s) in segments.iter().enumerate() {
                let end = s.start + s.direction * s.length;
                let (a, b) = (s.start.min(end) - reach, s.start.max(end) + reach);
                let (x0, z0) = Self::cell_of(origin, cell, a);
                let (x1, z1) = Self::cell_of(origin, cell, b);
                for z in z0..=z1.min(depth - 1) {
                    for x in x0..=x1.min(width - 1) {
                        cells[z * width + x].push((e, k));
                    }
                }
            }
        }
        Self {
            origin,
            cell,
            width,
            depth,
            cells,
        }
    }

    /// The cell holding `p`, clamped at zero.
    fn cell_of(origin: DVec2, cell: f64, p: DVec2) -> (usize, usize) {
        let q = ((p - origin) / cell).floor().max(DVec2::ZERO);
        (q.x as usize, q.y as usize)
    }

    /// The (edge, segment) pairs that can reach `p`, in edge and then
    /// segment order.
    pub(crate) fn near(&self, p: DVec2) -> &[(usize, usize)] {
        let q = (p - self.origin) / self.cell;
        if q.x < 0.0 || q.y < 0.0 || q.x >= self.width as f64 || q.y >= self.depth as f64 {
            return &[];
        }
        &self.cells[q.y as usize * self.width + q.x as usize]
    }
}

/// The `k`-th point where targets are read, in metres from the foot along
/// the line (negative before it): every `step` metres for the first four
/// either side, then spreading out by half again each step, where the
/// weight is small and smooth.
fn node(k: i32, step: f64) -> f64 {
    const NEAR: i32 = 4;
    let i = k.unsigned_abs() as i32;
    let x = if i <= NEAR {
        f64::from(i) * step
    } else {
        f64::from(NEAR) * step * 1.5f64.powi(i - NEAR)
    };
    if k < 0 { -x } else { x }
}

/// The index of the node at or before `x` metres from the foot, for nodes
/// `step` apart near it.
fn node_index(x: f64, step: f64) -> i32 {
    let mut k = 0;
    if x >= 0.0 {
        while node(k + 1, step) <= x {
            k += 1;
        }
    } else {
        while node(k, step) > x {
            k -= 1;
        }
    }
    k
}

/// `∫ w(x) dx`, `∫ w(x) x dx` and, with `second`, `∫ w(x) x² dx` (else 0)
/// from `x0` to `x1`, for the weight `w(x) = (x² + a²)^(−(power + 1) / 2) − 1`:
/// zero at distance 1 (the reach), so the pull and its slope fade to
/// nothing there. `a` must be positive and below 1, `x0` to `x1` within the
/// reach, and `power` at least 3 for the second moment.
fn moments(x0: f64, x1: f64, a: f64, power: u32, second: bool) -> (f64, f64, f64) {
    let nu = f64::from(power + 1) / 2.0;
    let zeroth = power_integral(x0, x1, a, power);
    // ∫ x (x² + a²)^(−ν) dx = (x² + a²)^(1 − ν) / (2 (1 − ν)), where
    // 2(1 − ν) = 1 − power.
    let first_at = |x: f64| half_power(x * x + a * a, 1 - power as i32) / (2.0 * (1.0 - nu));
    let first = first_at(x1) - first_at(x0);
    // ∫ x² (x² + a²)^(−ν) dx = ∫ (x² + a²)^(1 − ν) dx − a² ∫ (x² + a²)^(−ν) dx.
    let squared = if second {
        power_integral(x0, x1, a, power - 2) - a * a * zeroth - (x1.powi(3) - x0.powi(3)) / 3.0
    } else {
        0.0
    };
    // Less the constant 1.
    (
        zeroth - (x1 - x0),
        first - 0.5 * (x1 * x1 - x0 * x0),
        squared,
    )
}

/// How far from the foot, as a multiple of the distance across, pulls are
/// worked out by angle: beyond it a series keeps the precision the angle
/// would lose, which grows quickly with the power (see
/// [`power_integral`]).
const NEAR_FOOT: f64 = 2.0;

/// `q^(half / 2)`, for a positive `q` and a whole number of halves.
fn half_power(q: f64, half: i32) -> f64 {
    let whole = q.powi(half.div_euclid(2));
    if half.rem_euclid(2) == 1 {
        whole * q.sqrt()
    } else {
        whole
    }
}

/// `∫ (x² + a²)^(−ν) dx` from `x0` to `x1`, for `ν = (power + 1) / 2`,
/// keeping its precision however small `a` is: by the angle `θ = atan(x/a)`
/// within [`NEAR_FOOT`] × `a` of the foot, and by a series in `(a/x)²`
/// beyond.
fn power_integral(x0: f64, x1: f64, a: f64, power: u32) -> f64 {
    // The integrand is even: work on the positive side.
    if x1 <= 0.0 {
        return power_integral(-x1, -x0, a, power);
    }
    if x0 < 0.0 {
        return power_integral(0.0, -x0, a, power) + power_integral(0.0, x1, a, power);
    }
    let near = NEAR_FOOT * a;
    let mut total = 0.0;
    if x0 < near {
        // With x = a·tan θ, ∫ (x² + a²)^(−ν) dx = a^(1 − 2ν) ∫ cos^(2ν − 2) θ dθ,
        // where 1 − 2ν = −power.
        let top = x1.min(near);
        total += a.powi(-(power as i32)) * cos_power_integral(power - 1, x0 / a, top / a);
    }
    if x1 > near {
        // The powers the pulls use have their series worked out once.
        let computed;
        let series = match power {
            DISTANCE_POWER => &DISTANCE_SERIES,
            TARGET_POWER => &TARGET_SERIES,
            TARGET_SECOND_POWER => &TARGET_SECOND_SERIES,
            _ => {
                computed = FarSeries::new(power);
                &computed
            }
        };
        // From NEAR_FOOT × a, where the antiderivative is the same for every
        // `a` but for a power of `a`, and the series is slowest.
        let start = if x0 <= near {
            a.powi(-(power as i32)) * series.at_near
        } else {
            series.antiderivative(x0, a, power)
        };
        total += series.antiderivative(x1, a, power) - start;
    }
    total
}

/// The most terms a [`FarSeries`] adds.
const FAR_TERMS: usize = 80;

/// The power of the second moment of the target pulls (see [`moments`]).
const TARGET_SECOND_POWER: u32 = TARGET_POWER - 2;

/// The [`FarSeries`] of each power the pulls use.
const DISTANCE_SERIES: FarSeries = FarSeries::new(DISTANCE_POWER);
const TARGET_SERIES: FarSeries = FarSeries::new(TARGET_POWER);
const TARGET_SECOND_SERIES: FarSeries = FarSeries::new(TARGET_SECOND_POWER);

/// An antiderivative of `(x² + a²)^(−ν)`, for `ν = (power + 1) / 2`, valid
/// for `x ≥` [`NEAR_FOOT`] × `a`: from `(x² + a²)^(−ν) =
/// x^(−2ν) Σₖ C(−ν, k) (a/x)^(2k)`, which converges there since
/// `(a/x)² ≤ 1/4`.
struct FarSeries {
    /// `C(−ν, k) / (1 − 2ν − 2k)`: the coefficient of `(a/x)^(2k)`.
    coefficients: [f64; FAR_TERMS],
    /// The antiderivative at [`NEAR_FOOT`] × `a`, divided by `a^(−power)`,
    /// which is the same for every `a`.
    at_near: f64,
}

impl FarSeries {
    /// The series for `power`.
    const fn new(power: u32) -> Self {
        let nu = (power + 1) as f64 / 2.0;
        let mut coefficients = [0.0; FAR_TERMS];
        let mut binomial = 1.0; // C(−ν, k)
        let mut k = 0;
        while k < FAR_TERMS {
            let kf = k as f64;
            coefficients[k] = binomial / (1.0 - 2.0 * nu - 2.0 * kf);
            binomial *= -(nu + kf) / (kf + 1.0);
            k += 1;
        }

        // NEAR_FOOT^(−power) times the sum at (a/x)² = NEAR_FOOT^(−2).
        let mut scale = 1.0;
        let mut i = 0;
        while i < power {
            scale /= NEAR_FOOT;
            i += 1;
        }
        let at_near = scale * sum(&coefficients, 1.0 / (NEAR_FOOT * NEAR_FOOT));
        Self {
            coefficients,
            at_near,
        }
    }

    /// The antiderivative at `x`, for `power` (the one the series is for).
    fn antiderivative(&self, x: f64, a: f64, power: u32) -> f64 {
        x.powi(-(power as i32)) * sum(&self.coefficients, (a / x) * (a / x))
    }
}

/// `Σₖ coefficients[k] rᵏ`, stopping once a term no longer changes the sum.
const fn sum(coefficients: &[f64; FAR_TERMS], r: f64) -> f64 {
    let mut ratio = 1.0; // rᵏ
    let mut total = 0.0;
    let mut k = 0;
    while k < FAR_TERMS {
        let term = coefficients[k] * ratio;
        total += term;
        if term.abs() <= 1e-17 * total.abs() {
            break;
        }
        ratio *= r;
        k += 1;
    }
    total
}

/// `∫ cos^n θ dθ` between `atan(u0)` and `atan(u1)`, by the recursion
/// `∫ cosⁿ = cosⁿ⁻¹ θ sin θ / n + (n − 1)/n ∫ cosⁿ⁻²`, with differences
/// taken in forms that keep their precision far from the foot.
fn cos_power_integral(n: u32, u0: f64, u1: f64) -> f64 {
    let (c0, c1) = (1.0 / (1.0 + u0 * u0).sqrt(), 1.0 / (1.0 + u1 * u1).sqrt());
    let (s0, s1) = (u0 * c0, u1 * c1);
    // Each power needs the one two below it: start from 0 (the angle) or
    // 1 (the sine), whichever matches `n`'s parity.
    let (mut total, mut m, mut p0, mut p1) = if n.is_multiple_of(2) {
        ((u1 - u0).atan2(1.0 + u0 * u1), 0, 1.0 / c0, 1.0 / c1)
    } else {
        (s1 - s0, 1, 1.0, 1.0)
    };
    // `p0`, `p1` hold cos^(m − 1) at the two ends.
    while m < n {
        m += 2;
        p0 *= c0 * c0;
        p1 *= c1 * c1;
        let mf = f64::from(m);
        total = (p1 * s1 - p0 * s0) / mf + (mf - 1.0) / mf * total;
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::segments;

    /// A pull's moments against simple numerical integration.
    #[test]
    fn moments_match_numerical_integration() {
        for power in [TARGET_POWER, DISTANCE_POWER] {
            for (x0, x1, a) in [(-0.5, 0.7, 0.1), (0.2, 0.9, 0.05), (-0.9, -0.1, 0.3)] {
                let steps = 400_000;
                let h = (x1 - x0) / f64::from(steps);
                let (mut w0, mut w1, mut w2) = (0.0, 0.0, 0.0);
                for i in 0..steps {
                    let x = x0 + (f64::from(i) + 0.5) * h;
                    let w = (x * x + a * a).powf(-f64::from(power + 1) / 2.0) - 1.0;
                    w0 += w * h;
                    w1 += w * x * h;
                    w2 += w * x * x * h;
                }
                let (m0, m1, m2) = moments(x0, x1, a, power, true);
                assert!(
                    (m0 / w0 - 1.0).abs() < 1e-6,
                    "{power} {x0} {x1} {a}: {m0} vs {w0}"
                );
                assert!((m1 - w1).abs() < 1e-6 * w0.abs(), "{power}: {m1} vs {w1}");
                assert!((m2 - w2).abs() < 1e-6 * w0.abs(), "{power}: {m2} vs {w2}");
            }
        }
        // Without the cutoff, a whole line pulls `LINE_PULL`.
        let (whole, _, _) = moments(-1.0e6, 1.0e6, 1.0, DISTANCE_POWER, false);
        assert!((whole + 2.0e6 - LINE_PULL).abs() < 1e-9);
        // Far from the foot, with `a` tiny, the pull is `∫ x^(−2ν)` to
        // within `ν (a/x)²` (the case that loses all precision by the angle
        // alone).
        for power in [TARGET_POWER, DISTANCE_POWER] {
            let nu = f64::from(power + 1) / 2.0;
            for a in [1e-12, 1e-9, 1e-6] {
                let (w0, _, _) = moments(0.3, 0.31, a, power, false);
                let bare = (0.31f64.powf(1.0 - 2.0 * nu) - 0.3f64.powf(1.0 - 2.0 * nu))
                    / (1.0 - 2.0 * nu)
                    - 0.01;
                assert!(
                    (w0 / bare - 1.0).abs() < 1e-6,
                    "{power}, a {a}: {w0} vs {bare}"
                );
            }
        }
    }

    #[test]
    fn surely_beyond_never_skips_a_point_within_reach() {
        // A wiggly edge in 100 m pieces with a sharp bend, like a warped
        // border, and points all around it, some exactly on it.
        let mut points: Vec<DVec2> = (0..=30)
            .map(|i| {
                let x = -1500.0 + 100.0 * f64::from(i);
                DVec2::new(x, 130.0 * (x / 127.0).sin())
            })
            .collect();
        let corner = *points.last().unwrap();
        points.extend((1..=10).map(|i| corner + DVec2::new(-60.0, 80.0) * f64::from(i)));
        let edge = segments(&points);
        let (reach, width) = (500.0, 250.0);

        let mut on_edge = vec![points[3], (points[7] + points[8]) / 2.0, corner];
        let grid = (-40..=40).flat_map(|i| (-20..=30).map(move |j| (i, j)));
        on_edge
            .extend(grid.map(|(i, j)| DVec2::new(f64::from(i) * 50.0, f64::from(j) * 50.0 + 3.0)));
        let mut skipped = 0;
        for p in on_edge {
            if !surely_beyond(edge.iter(), p, reach, width) {
                continue;
            }
            skipped += 1;
            let mut pull = Pull::<0>::default();
            for s in &edge {
                pull.add_distance(s, p, reach);
            }
            assert!(
                pull.distance(reach) >= width,
                "skipped {p}: {}",
                pull.distance(reach)
            );
        }
        // It does skip: most of the grid is farther than the width.
        assert!(skipped > 1000, "skipped only {skipped}");
    }

    #[test]
    fn cutting_a_straight_edge_changes_nothing() {
        // One edge 4 km long, cut into 1, 2, 10 and 100 equal pieces, its
        // target curving along it on scales both under and over the spacing
        // targets are read at (8 m for the bending, 64 m for the fade).
        let target = |along: f64, across: f64| {
            [(along / 300.0).sin() * 20.0 + (along / 37.0).sin() * 3.0 + 0.01 * across]
        };
        for step in [8.0, 64.0] {
            let pulls = |pieces: usize, p: DVec2| {
                let points: Vec<DVec2> = (0..=pieces)
                    .map(|i| DVec2::new(-2000.0 + 4000.0 * i as f64 / pieces as f64, 0.0))
                    .collect();
                let mut pull = Pull::<1>::default();
                for s in segments(&points) {
                    pull.add_distance(&s, p, 500.0);
                    pull.add_targets(&s, p, 500.0, step, &target);
                }
                pull
            };
            for d in [0.01, 0.5, 7.0, 60.0, 250.0, 499.0] {
                for x in [-1990.0, -400.0, 3.0, 1999.0] {
                    let p = DVec2::new(x, d);
                    let one = pulls(1, p);
                    for pieces in [2, 10, 100] {
                        let cut = pulls(pieces, p);
                        let (a, b) = (one.distance(500.0), cut.distance(500.0));
                        assert!(
                            (a / b - 1.0).abs() < 1e-9,
                            "{step}, {pieces} at {p}: {a} vs {b}"
                        );
                        let (a, b) = (one.mixed().unwrap()[0], cut.mixed().unwrap()[0]);
                        assert!(
                            (a - b).abs() < 1e-9 * (1.0 + a.abs()),
                            "{step}, {pieces} at {p}: {a} vs {b}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_pull_fades_to_nothing_smoothly_at_the_reach() {
        // A short segment; points moving straight away from it, past the
        // reach. The pull falls to exactly nothing at the reach, with no
        // slope left there (so heights keep a continuous slope), and is
        // nothing beyond.
        let points = [DVec2::new(0.0, 0.0), DVec2::new(100.0, 0.0)];
        let reach = 500.0;
        let closeness = |d: f64| {
            let mut pull = Pull::<1>::default();
            for s in segments(&points) {
                pull.add(&s, DVec2::new(50.0, d), reach, &|_, _| [1.0]);
            }
            (pull.closeness, pull.weight, pull.mixed())
        };
        let (at, weight, mixed) = closeness(reach);
        assert_eq!((at, weight), (0.0, 0.0));
        assert!(mixed.is_none());
        assert!(closeness(reach + 1.0).2.is_none());
        // The slope at the reach: the pull a little inside, over the step.
        let (inside, weight_inside, _) = closeness(reach - 0.01);
        let (further, weight_further, _) = closeness(reach - 0.1);
        assert!(inside > 0.0 && weight_inside > 0.0);
        // The weight grows linearly from the reach and the stretch within
        // reach as the square root of the gap, so the pull grows as the
        // gap^1.5 and its slope as √gap: a tenth of the gap, about a third
        // of the slope (√0.1), falling to zero rather than staying put.
        assert!(inside / 0.01 < 0.4 * further / 0.1, "{inside} {further}");
        assert!(weight_inside / 0.01 < 0.4 * weight_further / 0.1);
    }

    #[test]
    fn the_pull_is_smooth_near_a_segments_line() {
        // A bent edge, and points moving in 1 mm steps past where one
        // segment's line runs, beyond its end: the distance across that
        // segment goes from a few metres to nearly nothing, through every
        // switch between the two ways the pull is worked out. The soft
        // distance and the mixed target change smoothly: second
        // differences far below first differences.
        let points = [
            DVec2::new(150.0, -700.0),
            DVec2::new(-200.0, 0.0),
            DVec2::new(100.0, 800.0),
        ];
        let target = |along: f64, across: f64| [(along / 300.0).sin() * 20.0 + 0.01 * across];
        let at = |p: DVec2| {
            let mut pull = Pull::<1>::default();
            for s in segments(&points) {
                pull.add(&s, p, 500.0, &target);
            }
            (pull.distance(500.0), pull.mixed().unwrap()[0])
        };
        // The segment's line past (−200, 0) runs on through (−550, 700).
        let along_line = DVec2::new(-350.0, 700.0).normalize();
        let across_line = along_line.perp();
        for beyond in [5.0, 190.0, 400.0] {
            let base = DVec2::new(-200.0, 0.0) + along_line * beyond;
            let samples: Vec<(f64, f64)> = (-3000..=3000)
                .map(|k| at(base + across_line * (f64::from(k) * 0.001)))
                .collect();
            for w in samples.windows(3) {
                for (a, b, c) in [(w[0].0, w[1].0, w[2].0), (w[0].1, w[1].1, w[2].1)] {
                    let (first, second) = ((c - a).abs(), (c - 2.0 * b + a).abs());
                    assert!(second <= 1e-3 * first + 1e-9, "{beyond}: {a} {b} {c}");
                }
            }
        }
    }

    /// The soft distance against the true distance, for the distance power
    /// in use: the largest errors (too long, too short) on the bent test
    /// border (both sides) and inside a region-graph cell, for points 10 m
    /// to 500 m out with a 1 km reach; how sharp the crease along the
    /// concave bend's bisector is; and, with `SHADE_DIR` set, a grid of the
    /// soft distance round that bend for a shaded image. Run with
    /// `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn measure_soft_distance_against_true() {
        let reach = 1000.0;
        let soft = |segs: &[Segment], p: DVec2| {
            let mut pull = Pull::<0>::default();
            for s in segs {
                pull.add_distance(s, p, reach);
            }
            pull.distance(reach)
        };
        let nearest = |segs: &[Segment], p: DVec2| {
            segs.iter()
                .map(|s| s.project(p).distance)
                .fold(f64::INFINITY, f64::min)
        };

        // The bent border, and a cell of the region graph.
        let bent = segments(&[
            DVec2::new(0.0, -1500.0),
            DVec2::new(150.0, -700.0),
            DVec2::new(-200.0, 0.0),
            DVec2::new(100.0, 800.0),
            DVec2::new(0.0, 1500.0),
        ]);
        // Unwarped, so the cell's sides are straight lines to measure from.
        let options = crate::graph::RegionGraphOptions {
            warp: crate::warp::BorderWarp {
                amplitude: 0.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let graph = crate::graph::RegionGraph::new(options, 7).unwrap();
        let region = graph.region(crate::graph::RegionKey(glam::IVec2::ZERO));
        let crate::region::Boundary::Polygon(mut cell) = region.boundary.clone() else {
            unreachable!()
        };
        cell.push(cell[0]);
        let cell_segs = segments(&cell);
        let centre = graph.site(crate::graph::RegionKey(glam::IVec2::ZERO));
        // The bent border's two ends, where it just stops (in the test
        // worlds, at the world's edge): points nearest them are left out.
        let ends = [DVec2::new(0.0, -1500.0), DVec2::new(0.0, 1500.0)];
        for (name, segs, inside_only) in [("bent border", &bent, false), ("cell", &cell_segs, true)]
        {
            let (mut long, mut short) = (0.0f64, 0.0f64);
            for i in -300..=300 {
                for j in -300..=300 {
                    let p = centre * f64::from(u8::from(inside_only))
                        + DVec2::new(f64::from(i), f64::from(j)) * 10.0;
                    if inside_only && !region.boundary.contains(p) {
                        continue;
                    }
                    let d = nearest(segs, p);
                    if !(10.0..=500.0).contains(&d)
                        || (!inside_only && ends.iter().any(|e| (e.distance(p) - d).abs() < 1.0))
                    {
                        continue;
                    }
                    let ratio = soft(segs, p) / d - 1.0;
                    long = long.max(ratio);
                    short = short.min(ratio);
                }
            }
            println!(
                "DIST power {DISTANCE_POWER} {name}: up to {:.1} % long, {:.1} % short",
                100.0 * long,
                100.0 * short
            );
        }
        // The points recorded in the review notes, on the bent border.
        for p in [
            DVec2::new(825.0, -525.0),
            DVec2::new(600.0, 0.0),
            DVec2::new(-500.0, 0.0),
            DVec2::new(1000.0, 1000.0),
        ] {
            println!(
                "POINT power {DISTANCE_POWER} ({}, {}): true {:.0} m, soft {:.0} m",
                p.x,
                p.y,
                nearest(&bent, p),
                soft(&bent, p)
            );
        }

        // The crease across the concave bend's bisector, east of (−200, 0).
        let vertex = DVec2::new(-200.0, 0.0);
        let (u1, u2) = (
            (DVec2::new(150.0, -700.0) - vertex).normalize(),
            (DVec2::new(100.0, 800.0) - vertex).normalize(),
        );
        let bisector = (u1 + u2).normalize();
        for s in [100.0, 200.0, 400.0] {
            let at = |t: f64| soft(&bent, vertex + bisector * s + bisector.perp() * t);
            let h = 0.25;
            let curvature = (-80..=80)
                .map(|k| {
                    let t = f64::from(k) * h;
                    ((at(t + h) - 2.0 * at(t) + at(t - h)) / (h * h)).abs()
                })
                .fold(0.0, f64::max);
            let slope = |t: f64| (at(t + h) - at(t - h)) / (2.0 * h);
            println!(
                "CREASE power {DISTANCE_POWER} at {s} m: curvature up to {curvature:.4} per m, \
                 slope change across ±10 m {:.3}",
                slope(10.0) - slope(-10.0)
            );
        }

        // The field round the bend, for an image.
        if let Ok(dir) = std::env::var("SHADE_DIR") {
            let mut out = String::new();
            for j in 0..500 {
                let row: Vec<String> = (0..500)
                    .map(|i| {
                        let p = DVec2::new(-250.0 + f64::from(i), -250.0 + f64::from(j));
                        format!("{:.3}", soft(&bent, p))
                    })
                    .collect();
                out.push_str(&row.join(","));
                out.push('\n');
            }
            std::fs::write(format!("{dir}/soft_{DISTANCE_POWER}.csv"), out).unwrap();
        }
    }

    #[test]
    fn the_mix_has_no_kink_at_the_edge() {
        // A target curving along a long straight edge: just off the edge the
        // mix is the target at the foot plus a change in the square of the
        // distance, with no change in the distance itself, which would put
        // a kink in the ground at the border.
        let points = [DVec2::new(-2000.0, 0.0), DVec2::new(2000.0, 0.0)];
        let target = |along: f64, _: f64| [0.001 * (along - 2000.0).powi(2)];
        for d in [0.01, 0.1, 1.0] {
            for side in [1.0, -1.0] {
                let mut pull = Pull::<1>::default();
                for s in segments(&points) {
                    pull.add(&s, DVec2::new(0.0, side * d), 500.0, &target);
                }
                // The weight's spread along the edge is d², so the change is
                // about f''·d²/2 = 0.001·d²; a kink would add a term in d
                // (about 1.6e-4 at d = 1 cm, against 1e-7 allowed there).
                let change = pull.mixed().unwrap()[0];
                assert!(
                    change.abs() < 1.5e-3 * d * d,
                    "{d} on side {side}: {change}"
                );
            }
        }
    }

    #[test]
    fn a_long_straight_edge_is_its_distance_away() {
        let points = [DVec2::new(-5000.0, 0.0), DVec2::new(5000.0, 0.0)];
        for d in [0.01, 1.0, 30.0, 100.0] {
            let mut pull = Pull::<1>::default();
            for s in segments(&points) {
                pull.add(&s, DVec2::new(0.0, d), 1000.0, &|_, _| [5.0]);
            }
            let soft = pull.distance(1000.0);
            assert!((soft / d - 1.0).abs() < 0.01, "{d}: {soft}");
            assert!((pull.mixed().unwrap()[0] - 5.0).abs() < 1e-9);
        }
        // Nothing within reach.
        let mut pull = Pull::<1>::default();
        for s in segments(&points) {
            pull.add(&s, DVec2::new(0.0, 1000.0), 1000.0, &|_, _| [5.0]);
        }
        assert_eq!(pull.distance(1000.0), f64::INFINITY);
        assert!(pull.mixed().is_none());
    }
}
