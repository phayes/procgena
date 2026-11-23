// Triangle shape and intersection tests, matching the Rust crate.

import package::math::{cross3, dot3, length3, length_squared};

/// `2 · √3` in `f32`, as the Rust crate computes it.
const TWO_SQRT_3: f32 = 3.4641016f;

/// Returns a triangle's shape quality: 1 for equilateral, 0 for flat.
fn quality(a: vec3<f32>, b: vec3<f32>, c: vec3<f32>) -> f32 {
    let area2 = length3(cross3(b - a, c - a));
    let edges = length_squared(b - a) + length_squared(c - b) + length_squared(a - c);
    if edges <= 0.0 {
        return 0.0;
    }
    // 4·√3·area / Σ edge², with area2 being twice the area.
    return TWO_SQRT_3 * area2 / edges;
}

/// Returns whether the segment from `p` to `q` passes through the interior
/// of triangle `a`, `b`, `c`, away from its edges and the segment's ends.
fn segment_crosses(p: vec3<f32>, q: vec3<f32>, a: vec3<f32>, b: vec3<f32>, c: vec3<f32>) -> bool {
    // Margin, as a fraction of the segment and of the triangle's
    // barycentric range, inside which a meeting counts as touching.
    let margin = 1e-4;
    let dir = q - p;
    let e1 = b - a;
    let e2 = c - a;
    let pvec = cross3(dir, e2);
    let det = dot3(e1, pvec);
    if abs(det) < 1e-12 {
        return false;
    }
    let inv_det = 1.0 / det;
    let tvec = p - a;
    let u = dot3(tvec, pvec) * inv_det;
    let qvec = cross3(tvec, e1);
    let v = dot3(dir, qvec) * inv_det;
    let t = dot3(e2, qvec) * inv_det;
    return u > margin && v > margin && u + v < 1.0 - margin && t > margin && t < 1.0 - margin;
}

/// Returns whether triangles `a` and `b` pass through each other: an edge of
/// one crosses the interior of the other. Triangles that only touch, and
/// triangles lying in the same plane, don't count.
fn triangles_cross(a: array<vec3<f32>, 3>, b: array<vec3<f32>, 3>) -> bool {
    var ta = a;
    var tb = b;
    for (var k = 0u; k < 3u; k++) {
        let next = (k + 1u) % 3u;
        if segment_crosses(ta[k], ta[next], tb[0], tb[1], tb[2]) {
            return true;
        }
    }
    for (var k = 0u; k < 3u; k++) {
        let next = (k + 1u) % 3u;
        if segment_crosses(tb[k], tb[next], ta[0], ta[1], ta[2]) {
            return true;
        }
    }
    return false;
}
