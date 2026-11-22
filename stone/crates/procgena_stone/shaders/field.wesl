// Rock shape building blocks, matching the Rust crate's `StoneField`.
//
// Stone space is the cube from -0.5 to 0.5 on each axis. Carve space is stone
// space multiplied by the per-axis carve scale. To sample a rock: start with
// `carve_begin`, apply `carve_step` once per cutting sphere in order, then
// call `stone_finish`.

import package::noise::triplanar;

/// Outward shift of the surface level, in carve-space units.
const SURFACE_OFFSET: f32 = 0.025;
/// Distance from the origin to each face of the stone-space cube.
const HALF_EXTENT: f32 = 0.5;
/// Largest number of cutting spheres a rock can have.
const MAX_CARVE_STEPS: u32 = 72u;
/// Largest finite `f32`.
const F32_MAX: f32 = 3.40282347e38;

/// The carved shape's field value and gradient at one carve-space position.
struct Carve {
    distance: f32,
    gradient: vec3<f32>,
}

/// Surface settings for `stone_finish`.
struct Surface {
    /// Per-axis factors converting stone space to carve space.
    carve_scale: vec3<f32>,
    /// Roughness depth; zero or less disables roughness.
    displacement: f32,
    seed: u32,
    /// Roughness pattern frequency.
    spread: f32,
    octaves: u32,
}

/// Returns `v` scaled to unit length, or `fallback` when its length is zero,
/// too small to invert, or not finite.
fn normalize_or(v: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
    let len = length(v);
    if len > 0.0 {
        let r = 1.0 / len;
        if r > 0.0 && r <= F32_MAX {
            return v * r;
        }
    }
    return fallback;
}

/// Starts the carved shape at carve-space position `q` with a sphere of radius `base_radius`.
fn carve_begin(q: vec3<f32>, base_radius: f32) -> Carve {
    return Carve(length(q) - base_radius, normalize_or(q, vec3<f32>(0.0)));
}

/// Removes one cutting sphere (`xyz` centre, `w` radius) from the shape at `q`,
/// with rounded transitions of width `smoothness`.
fn carve_step(c: Carve, q: vec3<f32>, sphere: vec4<f32>, smoothness: f32) -> Carve {
    let k = smoothness;
    let offset = q - sphere.xyz;
    let b = length(offset) - sphere.w;
    let gb = normalize_or(offset, vec3<f32>(0.0));
    let h = clamp(0.5 + 0.5 * (-b - c.distance) / k, 0.0, 1.0);
    let d = c.distance + (-b - c.distance) * h + k * h * (1.0 - h);
    let g = c.gradient * (1.0 - h) - gb * h;
    return Carve(d, g);
}

/// Adds roughness to the carved shape at stone-space position `p`, shifts the
/// surface level, and clips the result to the stone-space cube.
///
/// Returns a negative value inside the rock and zero or more outside it.
fn stone_finish(p: vec3<f32>, c: Carve, s: Surface) -> f32 {
    var roughness = 0.0;
    if s.displacement > 0.0 {
        let normal = c.gradient * s.carve_scale;
        roughness = s.displacement * length(triplanar(p, normal, s.seed, s.spread, s.octaves));
    }
    let stone = 0.5 * (c.distance + roughness - SURFACE_OFFSET);

    let a = abs(p);
    let cube = max(max(a.x, a.y), a.z) - HALF_EXTENT;
    return max(stone, cube);
}
