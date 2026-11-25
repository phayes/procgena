//! Measuring a finished road: its length, grades, bends, earthworks,
//! river crossings and crowding, so road builders can be compared.

use procgena_core::Ground;

use crate::carve::FeatureLine;

use super::LEG_SPACING;
use super::level::crowded;
use super::search::segments_meet;

/// Steps shorter than this, in metres, are left out of grades and bends:
/// over them a rounding error is a steep grade or a sharp turn.
const SHORT_STEP: f64 = 0.5;

/// Measures of one road line (see [`road_quality`]).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RoadQuality {
    /// Length along the line, in metres.
    pub length: f64,
    /// The steepest grade from one point to the next, as rise over run.
    pub max_grade: f64,
    /// The grade that 95% of the road's length is at or below.
    pub p95_grade: f64,
    /// The tightest bend, as the radius of the circle through three
    /// neighbouring points, in metres. Infinite for a straight road.
    pub min_radius: f64,
    /// The deepest cut below the reference ground, at the road's centre or
    /// either edge, in metres.
    pub max_cut: f64,
    /// The highest fill above the reference ground, at the road's centre
    /// or either edge, in metres.
    pub max_fill: f64,
    /// How many times the road crosses a river.
    pub river_crossings: usize,
    /// The smallest angle between the road and a river where it crosses
    /// one, in degrees (90 is square). 90 when it crosses none.
    pub min_crossing_angle: f64,
    /// How many points come too close to an earlier leg of the same road
    /// (within three widths, beyond a hairpin's own bend).
    pub crowded_points: usize,
}

/// Measures `road` against `ground`, the reference ground before roads
/// were carved (for example the same terrain at
/// [`FeatureTier::Rough`](crate::FeatureTier::Rough)), and `rivers`.
///
/// A road of fewer than two points measures zero, with an infinite
/// radius. Points where `ground` has no height (NaN) don't count towards
/// cut and fill.
pub fn road_quality(
    road: &FeatureLine,
    ground: &dyn Ground,
    rivers: &[FeatureLine],
) -> RoadQuality {
    let points = &road.points;
    let mut quality = RoadQuality {
        min_radius: f64::INFINITY,
        min_crossing_angle: 90.0,
        ..RoadQuality::default()
    };
    if points.len() < 2 {
        return quality;
    }

    // Length and grades, each grade weighted by its step's length.
    let mut grades: Vec<(f64, f64)> = Vec::with_capacity(points.len());
    for i in 1..points.len() {
        let run = points[i].distance(points[i - 1]);
        quality.length += run;
        if run >= SHORT_STEP {
            let grade = f64::from(road.levels[i] - road.levels[i - 1]).abs() / run;
            quality.max_grade = quality.max_grade.max(grade);
            grades.push((grade, run));
        }
    }
    grades.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut covered = 0.0;
    for &(grade, run) in &grades {
        covered += run;
        quality.p95_grade = grade;
        if covered >= 0.95 * quality.length {
            break;
        }
    }

    // Bends: the circle through each point and its neighbours.
    for w in points.windows(3) {
        let (a, b, c) = (w[0], w[1], w[2]);
        let (ab, bc, ca) = (a.distance(b), b.distance(c), c.distance(a));
        if ab < SHORT_STEP || bc < SHORT_STEP {
            continue;
        }
        let twice_area = (b - a).perp_dot(c - a).abs();
        if twice_area > 0.0 {
            quality.min_radius = quality.min_radius.min(ab * bc * ca / (2.0 * twice_area));
        }
    }

    // Cut and fill at the centre and both edges.
    let last = points.len() - 1;
    for i in 0..points.len() {
        let along = points[(i + 1).min(last)] - points[i.saturating_sub(1)];
        let side = along.perp().normalize_or_zero() * f64::from(road.widths[i]) / 2.0;
        let level = f64::from(road.levels[i]);
        for p in [points[i], points[i] + side, points[i] - side] {
            let h = f64::from(ground.height(p));
            if h.is_finite() {
                quality.max_cut = quality.max_cut.max(h - level);
                quality.max_fill = quality.max_fill.max(level - h);
            }
        }
    }

    // River crossings and their angles.
    for a in points.windows(2) {
        for river in rivers {
            for b in river.points.windows(2) {
                if segments_meet(a[0], a[1], b[0], b[1]) {
                    let turn = (a[1] - a[0]).angle_to(b[1] - b[0]).abs().to_degrees();
                    let angle = if turn > 90.0 { 180.0 - turn } else { turn };
                    quality.river_crossings += 1;
                    quality.min_crossing_angle = quality.min_crossing_angle.min(angle);
                }
            }
        }
    }

    // Legs crowding each other, at the spacing the router keeps.
    let width = road.widths.iter().copied().fold(0.0f32, f32::max);
    quality.crowded_points = crowded(points, LEG_SPACING * f64::from(width)).len();
    quality
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carve::FeatureKind;
    use glam::DVec2;
    use procgena_core::FlatGround;

    /// A road through `points` at `levels`, 6 m wide.
    fn road(points: Vec<DVec2>, levels: Vec<f32>) -> FeatureLine {
        FeatureLine {
            kind: FeatureKind::Road,
            widths: vec![6.0; points.len()],
            points,
            levels,
            depth: 0.0,
        }
    }

    #[test]
    fn a_straight_climb_measures_its_length_grade_and_fill() {
        // 100 m east, climbing 5 m, over flat ground at 0.
        let points: Vec<DVec2> = (0..=10)
            .map(|i| DVec2::new(10.0 * f64::from(i), 0.0))
            .collect();
        let levels: Vec<f32> = (0..=10).map(|i| 0.5 * i as f32).collect();
        let q = road_quality(&road(points, levels), &FlatGround, &[]);
        assert!((q.length - 100.0).abs() < 1e-9);
        assert!((q.max_grade - 0.05).abs() < 1e-6);
        assert!((q.p95_grade - 0.05).abs() < 1e-6);
        assert!(q.min_radius.is_infinite());
        assert!((q.max_fill - 5.0).abs() < 1e-6);
        assert_eq!(q.max_cut, 0.0);
        assert_eq!(q.river_crossings, 0);
        assert_eq!(q.crowded_points, 0);
    }

    #[test]
    fn a_circle_measures_its_radius_and_river_crossings() {
        // A half circle of radius 50 m, crossed by a river along x = 0.
        let points: Vec<DVec2> = (0..=36)
            .map(|i| {
                let a = std::f64::consts::PI * f64::from(i) / 36.0 - std::f64::consts::FRAC_PI_2;
                50.0 * DVec2::new(a.cos(), a.sin()) + DVec2::new(-25.0, 0.0)
            })
            .collect();
        let levels = vec![0.0; points.len()];
        let river = FeatureLine {
            kind: FeatureKind::River,
            points: vec![DVec2::new(0.0, -100.0), DVec2::new(0.0, 100.0)],
            levels: vec![0.0; 2],
            widths: vec![4.0; 2],
            depth: 1.0,
        };
        let q = road_quality(&road(points, levels), &FlatGround, &[river]);
        assert!((q.min_radius - 50.0).abs() < 0.1, "radius {}", q.min_radius);
        assert_eq!(q.river_crossings, 2);
        assert!(q.min_crossing_angle > 50.0 && q.min_crossing_angle < 70.0);
    }
}
