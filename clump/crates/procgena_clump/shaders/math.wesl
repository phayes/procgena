// Small vector helpers that round exactly like the Rust crate's glam calls,
// so shader results can be checked bit for bit against the CPU.

/// Largest finite `f32`.
const F32_MAX: f32 = 3.40282347e38f;

/// Smallest positive normal `f32` (Rust's `f32::MIN_POSITIVE`).
const F32_MIN_POSITIVE: f32 = 1.17549435e-38f;

/// Returns the squared length of `v`, like glam's `length_squared`.
fn length_squared(v: vec3<f32>) -> f32 {
    return v.x * v.x + v.y * v.y + v.z * v.z;
}

/// Returns the dot product of `a` and `b`, like glam's `dot`.
fn dot3(a: vec3<f32>, b: vec3<f32>) -> f32 {
    return a.x * b.x + a.y * b.y + a.z * b.z;
}

/// Returns the length of `v`, like glam's `length`.
fn length3(v: vec3<f32>) -> f32 {
    return sqrt(length_squared(v));
}

/// Returns the cross product of `a` and `b`, like glam's `cross`.
fn cross3(a: vec3<f32>, b: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(a.y * b.z - b.y * a.z, a.z * b.x - b.z * a.x, a.x * b.y - b.x * a.y);
}

/// Returns `v` scaled to unit length, or `fallback` if `v` is zero, too
/// small or not finite, like glam's `normalize_or`.
fn normalize_or(v: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
    let rcp = 1.0 / length3(v);
    if rcp <= F32_MAX && rcp > 0.0 {
        return v * rcp;
    }
    return fallback;
}

/// Returns `v` scaled to unit length, or zero if `v` is zero, too small or
/// not finite, like glam's `normalize_or_zero`.
fn normalize_or_zero(v: vec3<f32>) -> vec3<f32> {
    return normalize_or(v, vec3<f32>(0.0));
}

/// Smooth Hermite step: 0 at or below `lo`, 1 at or above `hi`. If `hi` is
/// not above `lo`, it is a hard step at `lo`. Matches the Rust crate's
/// `smoothstep`, not WGSL's built-in `smoothstep`.
fn smooth_step(lo: f32, hi: f32, x: f32) -> f32 {
    if hi <= lo {
        return select(1.0, 0.0, x < lo);
    }
    let t = clamp((x - lo) / (hi - lo), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}
