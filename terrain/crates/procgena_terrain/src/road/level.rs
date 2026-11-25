//! Road levels along a routed road: how high each point sits, within its
//! grades, and checks on a route's shape.

use std::collections::HashMap;

use glam::DVec2;

use crate::carve::FeatureLine;

/// How many metres of cutting road levels accept to save a metre of
/// filling: cuttings may steepen to cliffs, but embankments stay earth.
const CUT_SHARE: f64 = 3.0;

/// How many points either side are averaged into the smoothed ground that
/// road levels keep close to, where they have the choice.
const LEVEL_SMOOTHING: usize = 1;

/// Every point of `points` that comes within `spacing` of an earlier part
/// of the same line more than twice `spacing` back along it: where two legs
/// of a road crowd each other. A hairpin's own bend is not counted.
pub(super) fn crowded(points: &[DVec2], spacing: f64) -> Vec<usize> {
    // Distance along the line to each point.
    let mut along = vec![0.0f64; points.len()];
    for i in 1..points.len() {
        along[i] = along[i - 1] + points[i].distance(points[i - 1]);
    }

    // Points bucketed by square, `spacing` across, so only nearby squares
    // are searched.
    let key = |p: DVec2| {
        (
            (p.x / spacing).floor() as i64,
            (p.y / spacing).floor() as i64,
        )
    };
    let mut squares: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    let mut out = Vec::new();
    for (j, &p) in points.iter().enumerate() {
        let (kx, kz) = key(p);
        let near = (-1..=1)
            .flat_map(|dx| (-1..=1).map(move |dz| (kx + dx, kz + dz)))
            .filter_map(|k| squares.get(&k))
            .flatten()
            .any(|&i| along[j] - along[i] > 2.0 * spacing && points[i].distance(p) < spacing);
        if near {
            out.push(j);
        }
        squares.entry((kx, kz)).or_default().push(j);
    }
    out
}

/// The lowest and highest ground across a road at each of `points`: at its
/// centre (`centre`, the ground there) and at its two edges, `half_width`
/// to either side.
pub(super) fn across_road(
    points: &[DVec2],
    centre: &[f32],
    half_width: f64,
    ground: &dyn Fn(DVec2) -> f32,
) -> Vec<(f32, f32)> {
    let last = points.len() - 1;
    (0..points.len())
        .map(|i| {
            let along = points[(i + 1).min(last)] - points[i.saturating_sub(1)];
            let side = along.perp().normalize_or_zero() * half_width;
            let (left, right) = (ground(points[i] + side), ground(points[i] - side));
            (
                centre[i].min(left).min(right),
                centre[i].max(left).max(right),
            )
        })
        .collect()
}

/// Road levels along `points`, starting at `start` and, if given, ending at
/// `end`, that keep the deepest fill, and the deepest cut divided by
/// [`CUT_SHARE`], as small as the grades allow: where the road can't follow
/// a hump or a dip, it cuts deep sooner than it builds a high embankment.
/// `across[i]` is the lowest and highest ground across the road at point
/// `i`; fill is measured from the lowest, cut from the highest, so on a
/// side slope the road sinks into the hillside rather than standing out
/// on a fill. Within that, each level keeps as close to `target` (the
/// smoothed ground along the road's centre) as it can. `steepest[i]` is
/// the steepest grade allowed from point `i - 1` to point `i`
/// (`steepest[0]` is unused).
///
/// The two end levels are fixed and don't count. Returns `None` if the two
/// ends are too far apart in height for the grades.
pub(super) fn grade_limited(
    points: &[DVec2],
    across: &[(f32, f32)],
    target: &[f32],
    start: f32,
    end: Option<f32>,
    steepest: &[f64],
) -> Option<Vec<f32>> {
    // How far the road may rise or fall along each step.
    let rise: Vec<f64> = (0..points.len())
        .map(|i| {
            if i == 0 {
                0.0
            } else {
                steepest[i] * points[i].distance(points[i - 1])
            }
        })
        .collect();
    let (start, end) = (f64::from(start), end.map(f64::from));
    let last = points.len() - 1;
    if let Some(e) = end
        && (e - start).abs() > rise.iter().sum::<f64>()
    {
        return None;
    }

    // The levels each point can take with every gap at most `gap`, passing
    // forward from the start; `None` if the end can't be reached.
    let bands = |gap: f64| -> Option<Vec<(f64, f64)>> {
        let mut out = vec![(start, start); points.len()];
        for i in 1..points.len() {
            let (lo, hi) = (out[i - 1].0 - rise[i], out[i - 1].1 + rise[i]);
            let (lo, hi) = match (i == last, end) {
                (true, Some(e)) => (lo.max(e), hi.min(e)),
                _ => {
                    let (low, high) = (f64::from(across[i].0), f64::from(across[i].1));
                    (lo.max(high - CUT_SHARE * gap), hi.min(low + gap))
                }
            };
            if lo > hi {
                return None;
            }
            out[i] = (lo, hi);
        }
        Some(out)
    };

    // The smallest gap that works, by bisection: the end is reachable, so
    // a gap as large as the ground's whole height range plus the climb
    // always works.
    let (lo_ground, hi_ground) = across.iter().fold((start, start), |(lo, hi), &(l, h)| {
        (lo.min(f64::from(l)), hi.max(f64::from(h)))
    });
    let mut wide = hi_ground - lo_ground + rise.iter().sum::<f64>() + 1.0;
    let mut narrow = 0.0;
    let mut best = bands(wide).expect("a wide enough gap always works");
    for _ in 0..40 {
        let gap = 0.5 * (narrow + wide);
        match bands(gap) {
            Some(b) => {
                wide = gap;
                best = b;
            }
            None => narrow = gap,
        }
    }

    // Levels from the end backward: each as near its target as its band
    // and the grade to the next level allow. Every band was reached from
    // the one before, so the choice is never empty.
    let mut levels = vec![0.0f64; points.len()];
    levels[last] = f64::from(target[last]).clamp(best[last].0, best[last].1);
    for i in (0..last).rev() {
        let lo = best[i].0.max(levels[i + 1] - rise[i + 1]);
        let hi = best[i].1.min(levels[i + 1] + rise[i + 1]);
        levels[i] = f64::from(target[i]).clamp(lo, hi.max(lo));
    }
    Some(levels.into_iter().map(|l| l as f32).collect())
}

/// A moving average over [`LEVEL_SMOOTHING`] points either side.
pub(super) fn smooth(values: &[f32]) -> Vec<f32> {
    let last = values.len() as isize - 1;
    (0..values.len() as isize)
        .map(|i| {
            let r = LEVEL_SMOOTHING as isize;
            let sum: f32 = (-r..=r)
                .map(|k| values[(i + k).clamp(0, last) as usize])
                .sum();
            sum / (2 * r + 1) as f32
        })
        .collect()
}

/// The level of the nearest point on `roads` to `p`, if there are roads.
pub(super) fn level_on_roads(roads: &[FeatureLine], p: DVec2) -> Option<f32> {
    roads
        .iter()
        .flat_map(|r| r.points.iter().zip(&r.levels))
        .min_by(|a, b| a.0.distance(p).total_cmp(&b.0.distance(p)))
        .map(|(_, &l)| l)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_keep_to_each_steps_grade() {
        // Flat ground, a 20 m climb, then flat: the road may climb 0.1 on the
        // first half and 0.3 on the second.
        let points: Vec<DVec2> = (0..=10)
            .map(|i| DVec2::new(10.0 * f64::from(i), 0.0))
            .collect();
        let ground = vec![0.0; points.len()];
        let across = vec![(0.0, 0.0); points.len()];
        let steepest: Vec<f64> = (0..=10).map(|i| if i <= 5 { 0.1 } else { 0.3 }).collect();
        let levels = grade_limited(&points, &across, &ground, 0.0, Some(20.0), &steepest).unwrap();
        for i in 1..levels.len() {
            let grade = f64::from(levels[i] - levels[i - 1]).abs() / 10.0;
            assert!(grade <= steepest[i] * 1.001, "step {i}: grade {grade}");
        }
        assert_eq!(*levels.last().unwrap(), 20.0);
        // 5 m on the gentle half and 15 m on the steep half is all there is.
        assert!(grade_limited(&points, &across, &ground, 0.0, Some(20.5), &steepest).is_none());
    }

    #[test]
    fn levels_sink_into_a_side_slope() {
        // A level road along a side slope: the ground across it runs from
        // 0 m at its downhill edge to 6 m at its uphill edge, 3 m at the
        // centre. Three metres of cut count as one of fill, so the road sits
        // at 1.5 m: 1.5 m of fill under the downhill edge and 4.5 m of cut at
        // the uphill edge, not on the centre's 3 m with 3 m of fill.
        let points: Vec<DVec2> = (0..=10)
            .map(|i| DVec2::new(10.0 * f64::from(i), 0.0))
            .collect();
        let across = vec![(0.0, 6.0); points.len()];
        let centre = vec![3.0; points.len()];
        let steepest = vec![0.1; points.len()];
        let levels = grade_limited(&points, &across, &centre, 1.5, None, &steepest).unwrap();
        for (i, l) in levels.iter().enumerate() {
            assert!((l - 1.5).abs() < 0.01, "point {i}: level {l}");
        }
    }

    #[test]
    fn crowded_legs_are_found_but_a_hairpin_bend_is_not() {
        // Up the z axis, round a hairpin, and back down 20 m away: legs
        // 20 m apart are fine with a spacing of 18 m, not with 25 m.
        let mut points: Vec<DVec2> = (0..=10)
            .map(|i| DVec2::new(0.0, 10.0 * f64::from(i)))
            .collect();
        points.extend((1..10).map(|i| {
            let a = std::f64::consts::PI * f64::from(i) / 10.0;
            DVec2::new(10.0 - 10.0 * a.cos(), 100.0 + 10.0 * a.sin())
        }));
        points.extend((0..=10).map(|i| DVec2::new(20.0, 100.0 - 10.0 * f64::from(i))));
        assert!(crowded(&points, 18.0).is_empty());
        let crowding = crowded(&points, 25.0);
        assert!(!crowding.is_empty(), "legs 20 m apart crowd at 25 m");
        assert!(crowding.iter().all(|&j| points[j].x == 20.0));
    }

    #[test]
    fn levels_cut_a_hump_sooner_than_fill_beside_it() {
        // Flat ground with a 10 m spike in the middle, a road free to end
        // anywhere, and a grade of 0.1 over 10 m steps: the road can rise
        // only 1 m per step, so it climbs early and cuts the spike's top.
        let points: Vec<DVec2> = (0..=20)
            .map(|i| DVec2::new(10.0 * f64::from(i), 0.0))
            .collect();
        let mut ground = vec![0.0; points.len()];
        ground[10] = 10.0;
        let steepest = vec![0.1; points.len()];
        let across: Vec<(f32, f32)> = ground.iter().map(|&g| (g, g)).collect();
        let levels = grade_limited(&points, &across, &ground, 0.0, None, &steepest).unwrap();
        let (cut, fill) = ground
            .iter()
            .zip(&levels)
            .fold((0.0f32, 0.0f32), |(c, f), (g, l)| {
                (c.max(g - l), f.max(l - g))
            });
        // At level L on the spike, its neighbours can be at L − 1: a cut of
        // 10 − L and a fill of L − 1. Three metres of cut count as one of
        // fill, so L = 3.25: a 6.75 m cut and a 2.25 m fill. Following the
        // ground and then clamping would leave a 9 m cut.
        assert!((cut - 6.75).abs() < 0.01, "cut {cut}");
        assert!((fill - 2.25).abs() < 0.01, "fill {fill}");
        for i in 1..levels.len() {
            assert!((levels[i] - levels[i - 1]).abs() <= 1.0 + 1e-4, "step {i}");
        }
    }
}
