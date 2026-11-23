// Repeatable pseudo-random values, matching the Rust crate's `hash` module.

/// Multiplier applied to each input before mixing.
const PCG_MULTIPLIER: u32 = 1664525u;
/// Increment added to each input before mixing.
const PCG_INCREMENT: u32 = 1013904223u;

/// Turns three integers into three repeatable pseudo-random integers (PCG3D,
/// Jarzynski and Olano 2020). Integer overflow wraps, as in Rust.
fn pcg3d(v_in: vec3<u32>) -> vec3<u32> {
    var v = v_in * PCG_MULTIPLIER + PCG_INCREMENT;

    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;

    v ^= v >> vec3<u32>(16u);

    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;

    return v;
}

/// Converts a hash to a number from 0 (inclusive) to 1 (exclusive), using its
/// upper 24 bits, so the result is exact in an `f32`.
fn unit_float(h: u32) -> f32 {
    return f32(h >> 8u) * (1.0 / 16777216.0);
}

/// Returns value number `counter` of the random stream for `seed`, in
/// `[0, 1)`: the value the Rust `Rng::new(seed)` returns on its
/// `counter`-th draw (counting from 0).
fn rng_draw(seed: u32, counter: u32) -> f32 {
    return unit_float(pcg3d(vec3<u32>(counter, seed, 0x5eedu)).x);
}
