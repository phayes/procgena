// Checks for merging two points of the working surface, matching the Rust
// crate's `patch` module.

import package::geometry::quality;
import package::math::{cross3, dot3, length3, length_squared, normalize_or_zero, F32_MIN_POSITIVE};

/// Smallest cosine allowed between a simplified triangle's normal and the
/// smooth normal at each of its corners.
const MIN_CORNER_COS: f32 = 0.2;

/// Smallest triangle quality a merge may create, unless the triangle was
/// already worse.
const MIN_QUALITY: f32 = 0.2;

/// Returns the quality of one triangle after a merge, or -1 if the merge may
/// not change it that way.
///
/// The triangle has corners `before` with smooth normals `normals`; the
/// corners flagged in `moved` move to `point` and take `normal`. The merge
/// is refused if an unmoved corner ends up farther than `edge_length` from
/// `point`, if the triangle turns over or far from a corner's normal, or if
/// it becomes a sliver worse than both `MIN_QUALITY` and its old shape.
fn collapse_quality(
    before: array<vec3<f32>, 3>,
    normals: array<vec3<f32>, 3>,
    moved: vec3<bool>,
    point: vec3<f32>,
    normal: vec3<f32>,
    edge_length: f32,
) -> f32 {
    // The triangle after the merge, and its corners' normals.
    var old = before;
    var old_normals = normals;
    var after: array<vec3<f32>, 3>;
    var after_normals: array<vec3<f32>, 3>;
    for (var k = 0u; k < 3u; k++) {
        after[k] = select(old[k], point, moved[k]);
        after_normals[k] = select(old_normals[k], normal, moved[k]);
    }

    // No new edge may be longer than the target.
    for (var k = 0u; k < 3u; k++) {
        if !moved[k] && length3(point - old[k]) > edge_length {
            return -1.0;
        }
    }

    // It may not turn over, or turn far from a corner's normal.
    let n_after = cross3(after[1] - after[0], after[2] - after[0]);
    let n_before = cross3(old[1] - old[0], old[2] - old[0]);
    let unit = normalize_or_zero(n_after);
    if dot3(n_after, n_before) <= 0.0 {
        return -1.0;
    }
    for (var k = 0u; k < 3u; k++) {
        if dot3(unit, after_normals[k]) < MIN_CORNER_COS {
            return -1.0;
        }
    }

    // It may not become a sliver, unless it already was one.
    let q = quality(after[0], after[1], after[2]);
    if q < min(MIN_QUALITY, quality(old[0], old[1], old[2])) {
        return -1.0;
    }
    return q;
}

/// Returns whether `x` lies within `tolerance` of the segment from `p` to `q`.
fn near_segment(p: vec3<f32>, q: vec3<f32>, x: vec3<f32>, tolerance: f32) -> bool {
    let pq = q - p;
    let t = clamp(dot3(x - p, pq) / max(length_squared(pq), F32_MIN_POSITIVE), 0.0, 1.0);
    return length3(x - (p + pq * t)) <= tolerance;
}

/// Returns whether both old border points `old_a` and `old_b` stay within a
/// quarter of `edge_length` of the new border, which runs from the merged
/// point `point` to either outer border neighbour `outer_a` or `outer_b`.
fn border_stays_near(
    point: vec3<f32>,
    outer_a: vec3<f32>,
    outer_b: vec3<f32>,
    old_a: vec3<f32>,
    old_b: vec3<f32>,
    edge_length: f32,
) -> bool {
    let tolerance = 0.25 * edge_length;
    let a_near = near_segment(point, outer_a, old_a, tolerance)
        || near_segment(point, outer_b, old_a, tolerance);
    let b_near = near_segment(point, outer_a, old_b, tolerance)
        || near_segment(point, outer_b, old_b, tolerance);
    return a_near && b_near;
}
