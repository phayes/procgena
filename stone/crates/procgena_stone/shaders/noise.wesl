// Repeatable surface patterns, matching the Rust crate's `noise` module.

import package::hash::pcg3d_unit;

/// First row of the transform that rotates and slightly scales coordinates
/// between noise layers.
const FBM_ROTATION_ROW0: vec2<f32> = vec2<f32>(0.87, 0.48);
/// Second row of the transform applied between noise layers.
const FBM_ROTATION_ROW1: vec2<f32> = vec2<f32>(-0.48, 0.87);
/// Coordinate offset applied after doubling the scale of each noise layer.
const FBM_OFFSET: f32 = 100.0;
/// Minimum weight for each triplanar projection before normalization.
const TRIPLANAR_MIN_WEIGHT: f32 = 0.001;

/// Blends `a` and `b` as `a + (b - a) * t`, matching the Rust rounding.
fn lerp3(a: vec3<f32>, b: vec3<f32>, t: f32) -> vec3<f32> {
    return a + (b - a) * t;
}

/// Samples three smooth random channels at 2D position `p` using `seed`.
/// Each channel lies in `[0, 1)`.
fn value_noise(p: vec2<f32>, seed: u32) -> vec3<f32> {
    let ip = floor(p);
    let f = p - ip;
    let u = f * f * (vec2<f32>(3.0) - 2.0 * f);

    // Integer grid coordinates keep their two's-complement bit patterns.
    let x = bitcast<u32>(i32(ip.x));
    let y = bitcast<u32>(i32(ip.y));
    let c00 = pcg3d_unit(vec3<u32>(x, y, seed));
    let c10 = pcg3d_unit(vec3<u32>(x + 1u, y, seed));
    let c01 = pcg3d_unit(vec3<u32>(x, y + 1u, seed));
    let c11 = pcg3d_unit(vec3<u32>(x + 1u, y + 1u, seed));

    let res = lerp3(lerp3(c00, c10, u.x), lerp3(c01, c11, u.x), u.y);
    return res * res;
}

/// Combines `octaves` noise layers into three channels of progressively finer detail.
fn fbm(x_in: vec2<f32>, seed: u32, octaves: u32) -> vec3<f32> {
    var x = x_in;
    var v = vec3<f32>(0.0);
    var a = 0.5;
    for (var i = 0u; i < octaves; i++) {
        v += a * value_noise(x, seed);
        let y = x * 2.0 + vec2<f32>(FBM_OFFSET);
        x = vec2<f32>(
            FBM_ROTATION_ROW0.x * y.x + FBM_ROTATION_ROW0.y * y.y,
            FBM_ROTATION_ROW1.x * y.x + FBM_ROTATION_ROW1.y * y.y,
        );
        a *= 0.5;
    }
    return v;
}

/// Samples roughness at 3D position `p`, blending patterns on the yz, zx, and
/// xy planes according to surface direction `n`, which need not be unit length.
fn triplanar(p: vec3<f32>, n: vec3<f32>, seed: u32, spread: f32, octaves: u32) -> vec3<f32> {
    var w = max(n * n, vec3<f32>(TRIPLANAR_MIN_WEIGHT));
    w = w / (w.x + w.y + w.z);
    return fbm(spread * p.yz, seed, octaves) * w.x
        + fbm(spread * p.zx, seed, octaves) * w.y
        + fbm(spread * p.xy, seed, octaves) * w.z;
}
