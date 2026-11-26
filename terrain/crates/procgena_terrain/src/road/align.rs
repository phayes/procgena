//! Fitting a routed road with straights and circular arcs.

use glam::DVec2;

use super::network::{Alignment, AlignmentPiece};

/// How close an arc's radius may come to zero, in metres, before the
/// corner is left sharp.
const LEAST_RADIUS: f64 = 0.5;

/// A road's fitted centre line, and the bends in it tighter than asked.
pub(super) struct Fit {
    pub alignment: Alignment,
    /// For each bend tighter than the radius asked for: its middle, in
    /// metres, and its radius (0 for a corner left sharp).
    pub sharp: Vec<(DVec2, f64)>,
}

/// Fits the routed path `path` (at least two points) with straights and
/// arcs, from its first point to its last.
///
/// The path is simplified to its corners first, keeping within `tolerance`
/// metres of every point. Each corner is then rounded by an arc at least
/// `min_radius` metres across where the straights either side leave room,
/// and wider where that keeps within `tolerance` of the corner. Where the
/// straights are too short, the corner gets the widest arc they hold, and
/// is listed in [`Fit::sharp`].
pub(super) fn fit(path: &[DVec2], min_radius: f64, tolerance: f64) -> Fit {
    // The corners, with the straight legs between them.
    let corners = corners(path, tolerance);
    let n = corners.len();
    let legs: Vec<DVec2> = corners.windows(2).map(|w| w[1] - w[0]).collect();
    let lengths: Vec<f64> = legs.iter().map(|l| l.length()).collect();
    let directions: Vec<DVec2> = legs.iter().map(|l| l.normalize_or_zero()).collect();

    // How far each corner turns, and the tangent of half that.
    let mut turn = vec![0.0f64; n];
    for i in 1..n - 1 {
        turn[i] = directions[i - 1].angle_to(directions[i]);
    }
    let half_tan: Vec<f64> = turn.iter().map(|t| (0.5 * t.abs()).tan()).collect();

    // Each corner's radius: the widest within `tolerance` of the corner,
    // at least `min_radius`, then as much as the legs either side hold,
    // shared with the corner at the leg's other end.
    let mut radius = vec![0.0f64; n];
    for i in 1..n - 1 {
        if half_tan[i] <= 0.0 {
            continue;
        }
        let secant = 1.0 / (0.5 * turn[i].abs()).cos() - 1.0;
        let within = if secant > 0.0 {
            tolerance / secant
        } else {
            f64::INFINITY
        };
        let before = lengths[i - 1] / (half_tan[i - 1] + half_tan[i]);
        let after = lengths[i] / (half_tan[i] + half_tan[i + 1]);
        radius[i] = within.max(min_radius).min(before).min(after);
    }

    // The pieces: a straight to each arc, the arc, and a straight to the end.
    let mut pieces = Vec::new();
    let mut sharp = Vec::new();
    let mut at = corners[0];
    for i in 1..n - 1 {
        if half_tan[i] <= 0.0 {
            continue;
        }
        let r = radius[i];
        if r < min_radius * (1.0 - 1e-9) {
            sharp.push((corners[i], if r < LEAST_RADIUS { 0.0 } else { r }));
        }
        if r < LEAST_RADIUS {
            push_line(&mut pieces, at, corners[i]);
            at = corners[i];
            continue;
        }
        let tangent = r * half_tan[i];
        let (into, out) = (
            corners[i] - directions[i - 1] * tangent,
            corners[i] + directions[i] * tangent,
        );
        push_line(&mut pieces, at, into);
        let side = directions[i - 1].perp() * turn[i].signum();
        let centre = into + side * r;
        let start = (into - centre).to_angle();
        pieces.push(AlignmentPiece::Arc {
            centre,
            radius: r,
            start,
            sweep: turn[i],
        });
        if let Some(last) = sharp.last_mut().filter(|s| s.0 == corners[i]) {
            last.0 = pieces
                .last()
                .expect("just pushed")
                .point_at(0.5 * r * turn[i].abs());
        }
        at = out;
    }
    push_line(&mut pieces, at, corners[n - 1]);
    if pieces.is_empty() {
        // A path that goes nowhere: one straight of no length.
        pieces.push(AlignmentPiece::Line {
            from: corners[0],
            to: corners[0],
        });
    }
    Fit {
        alignment: Alignment { pieces },
        sharp,
    }
}

/// Adds a straight from `from` to `to` to `pieces`, unless it has no
/// length.
fn push_line(pieces: &mut Vec<AlignmentPiece>, from: DVec2, to: DVec2) {
    if from.distance(to) > 1e-9 {
        pieces.push(AlignmentPiece::Line { from, to });
    }
}

/// The corners of `path`: its first and last points and the fewest points
/// between that keep every point within `tolerance` metres of the line
/// through them (Douglas–Peucker). Repeated points are dropped first.
fn corners(path: &[DVec2], tolerance: f64) -> Vec<DVec2> {
    let mut points: Vec<DVec2> = Vec::with_capacity(path.len());
    for &p in path {
        if points.last().is_none_or(|q| q.distance(p) > 1e-9) {
            points.push(p);
        }
    }
    if points.len() < 3 {
        return points;
    }

    // Split each stretch at its point furthest from its chord, while that
    // is further than `tolerance`; ties go to the first.
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0, points.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let mut worst = (tolerance, None);
        for i in a + 1..b {
            let d = distance_to_segment(points[i], points[a], points[b]);
            if d > worst.0 {
                worst = (d, Some(i));
            }
        }
        if let (_, Some(i)) = worst {
            keep[i] = true;
            stack.push((a, i));
            stack.push((i, b));
        }
    }
    points
        .into_iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(p))
        .collect()
}

/// The distance from `p` to the segment from `a` to `b`.
fn distance_to_segment(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Points `step` metres apart along `alignment` (which must have a piece)
/// from its start, and its end, with the distance along to each: always
/// at least two. The first and last are `ends` exactly: the points the
/// alignment was fitted from and to.
pub(super) fn stations(alignment: &Alignment, step: f64, ends: [DVec2; 2]) -> Vec<(f64, DVec2)> {
    let length = alignment.length();
    let at = |s: f64| (s, alignment.point_at(s).expect("alignment has pieces"));
    let mut out = vec![(0.0, ends[0])];
    let mut k = 1;
    while (k as f64) * step < length - 1e-6 {
        out.push(at(k as f64 * step));
        k += 1;
    }
    out.push((length, ends[1]));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tightest radius in `alignment`'s arcs.
    fn tightest(alignment: &Alignment) -> f64 {
        alignment
            .pieces
            .iter()
            .filter_map(|p| match p {
                AlignmentPiece::Arc { radius, .. } => Some(*radius),
                AlignmentPiece::Line { .. } => None,
            })
            .fold(f64::INFINITY, f64::min)
    }

    /// Whether each piece starts where the one before ends, heading the
    /// same way.
    fn joined(alignment: &Alignment) -> bool {
        alignment.pieces.windows(2).all(|w| {
            let end = w[0].point_at(w[0].length());
            let start = w[1].point_at(0.0);
            end.distance(start) < 1e-9
                && w[0]
                    .heading_at(w[0].length())
                    .distance(w[1].heading_at(0.0))
                    < 1e-9
        })
    }

    #[test]
    fn a_wide_corner_gets_an_arc_at_least_as_wide_as_asked() {
        let path = [
            DVec2::ZERO,
            DVec2::new(100.0, 0.0),
            DVec2::new(100.0, 100.0),
        ];
        let fit = fit(&path, 12.0, 4.0);
        assert!(fit.sharp.is_empty());
        assert!(tightest(&fit.alignment) >= 12.0);
        assert!(joined(&fit.alignment));
        let end = fit.alignment.point_at(fit.alignment.length()).unwrap();
        assert!(end.distance(path[2]) < 1e-9);
    }

    #[test]
    fn a_cramped_hairpin_is_listed_as_sharp() {
        // Two legs 6 m apart, turned about by a short link.
        let path = [
            DVec2::ZERO,
            DVec2::new(100.0, 0.0),
            DVec2::new(100.0, 6.0),
            DVec2::new(0.0, 6.0),
        ];
        let fit = fit(&path, 12.0, 1.0);
        assert_eq!(fit.sharp.len(), 2);
        for &(_, r) in &fit.sharp {
            assert!(r > 0.0 && r < 12.0, "radius {r}");
        }
        assert!(joined(&fit.alignment));
    }

    #[test]
    fn a_straight_path_stays_one_straight() {
        let path: Vec<DVec2> = (0..10).map(|i| DVec2::new(8.0 * i as f64, 0.0)).collect();
        let fit = fit(&path, 12.0, 4.0);
        assert_eq!(fit.alignment.pieces.len(), 1);
        assert!(fit.sharp.is_empty());
    }

    #[test]
    fn stations_are_evenly_spaced_and_end_at_the_end() {
        let path = [DVec2::ZERO, DVec2::new(9.0, 0.0)];
        let s = stations(&fit(&path, 12.0, 4.0).alignment, 2.0, path);
        let along: Vec<f64> = s.iter().map(|&(a, _)| a).collect();
        assert_eq!(along, vec![0.0, 2.0, 4.0, 6.0, 8.0, 9.0]);
    }
}
