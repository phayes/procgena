// Where a clump covers the surface and how thick it is there, matching the
// Rust crate's `field` and `shell` modules. Lengths are in metres, after the
// clump's `scale`.

import package::hash::{pcg3d, unit_float};
import package::math::{dot3, length_squared, normalize_or_zero, smooth_step};

/// Most directions material may arrive from.
const MAX_DIRECTIONS: u32 = 8u;

/// Smallest lump radius, as a fraction of `lump_size`.
const LUMP_RADIUS_MIN: f32 = 0.3;
/// Largest lump radius, as a fraction of `lump_size`.
const LUMP_RADIUS_MAX: f32 = 0.55;
/// Smallest lump height, as a fraction of a full round lump of its radius.
const LUMP_HEIGHT_MIN: f32 = 0.6;

/// Cosine and sine of the largest angle between a push direction and its
/// surface normal (60°), in `f32` as the Rust crate computes them.
const COS_MAX_PUSH_TILT: f32 = 0.49999997f;
const SIN_MAX_PUSH_TILT: f32 = 0.86602545f;

/// A clump pass's coverage settings. 192 bytes, usable as a uniform.
struct Field {
    /// Per direction: the unit vector toward where the material comes from,
    /// then its strength. Entries past `source_count` are unused.
    sources: array<vec4<f32>, MAX_DIRECTIONS>,
    source_count: u32,
    /// 1 if clumps grow whichever way the surface faces.
    any_facing: u32,
    facing_min: f32,
    facing_full: f32,
    exposure_power: f32,
    /// Thinnest input a clump may grow on.
    min_support: f32,
    thickness: f32,
    sink: f32,
    lumpiness: f32,
    lump_size: f32,
    seed: u32,
    /// Small distance used to step off the input before casting rays.
    epsilon: f32,
    /// 1 if the input encloses a solid.
    closed_input: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

/// A point on the working surface with the measurements its height depends on.
struct Point {
    point: vec3<f32>,
    normal: vec3<f32>,
    /// Exposure toward each direction, from 0 (closed in) to 1 (fully exposed).
    exposure: array<f32, MAX_DIRECTIONS>,
    /// Input thickness straight in along the normal; 0 inside another part.
    room: f32,
    /// How far the input rises above and dips below the simplified surface here.
    rise: f32,
    dip: f32,
}

/// Returns how directly `normal` faces into the source at `toward`, from 0 to 1.
fn facing_term(field: Field, normal: vec3<f32>, toward: vec3<f32>) -> f32 {
    if field.any_facing != 0u {
        return 1.0;
    }
    return smooth_step(field.facing_min, field.facing_full, dot3(normal, toward));
}

/// Returns the facing part of the mask for unit normal `normal`: the sum over
/// directions of strength × facing, as if fully exposed.
fn facing(field: Field, normal: vec3<f32>) -> f32 {
    var f = field;
    var sum = 0.0;
    for (var d = 0u; d < f.source_count; d++) {
        let s = f.sources[d];
        sum += s.w * facing_term(f, normal, s.xyz);
    }
    return sum;
}

/// Returns where clumps may grow at `p`: the sum over directions of
/// strength × facing × exposure. Values above 1 are allowed.
fn mask(field: Field, p: Point) -> f32 {
    // Nothing grows on input too thin, or inside other parts of it.
    if p.room <= 0.0 || p.room < field.min_support {
        return 0.0;
    }
    var f = field;
    var exposure = p.exposure;
    var sum = 0.0;
    for (var d = 0u; d < f.source_count; d++) {
        let s = f.sources[d];
        let open = clamp(exposure[d], 0.0, 1.0);
        // x^0 is 1 for every x, as in Rust; GPU `pow` may not give that.
        let shelter = select(pow(open, f.exposure_power), 1.0, f.exposure_power == 0.0);
        sum += s.w * facing_term(f, p.normal, s.xyz) * shelter;
    }
    return sum;
}

/// Returns the coverage weight at `p`, from 0 (bare) to 1 (full thickness).
fn weight(field: Field, p: Point) -> f32 {
    return clamp(mask(field, p), 0.0, 1.0);
}

/// Returns the top height above the surface for weight `w`, without lumps:
/// `-sink` at 0 and `thickness` at 1.
fn top(field: Field, w: f32) -> f32 {
    return (field.thickness + field.sink) * w - field.sink;
}

/// Returns the visible height fraction for a top height: 0 at or below the
/// surface, 1 at full thickness.
fn fade(field: Field, top_height: f32) -> f32 {
    return clamp(top_height / field.thickness, 0.0, 1.0);
}

/// Returns the height of the rounded lumps at `point`, from 0 to the tallest
/// a lump can be: spheres on a jittered grid of `lump_size` cells.
fn lump_height(field: Field, point: vec3<f32>) -> f32 {
    if field.lumpiness <= 0.0 {
        return 0.0;
    }
    let q = point / field.lump_size;
    let cell = floor(q);
    let seed_bits = (field.seed << 8u) | (field.seed >> 24u);
    var best = 0.0;
    for (var dz = -1; dz <= 1; dz++) {
        for (var dy = -1; dy <= 1; dy++) {
            for (var dx = -1; dx <= 1; dx++) {
                let c = cell + vec3<f32>(f32(dx), f32(dy), f32(dz));
                // Keep the bit pattern of negative cell indices.
                let h = pcg3d(vec3<u32>(
                    bitcast<u32>(i32(c.x)),
                    bitcast<u32>(i32(c.y)),
                    bitcast<u32>(i32(c.z)) ^ seed_bits,
                ));
                let centre = c + vec3<f32>(unit_float(h.x), unit_float(h.y), unit_float(h.z));
                let k = pcg3d(h);
                let r = LUMP_RADIUS_MIN + (LUMP_RADIUS_MAX - LUMP_RADIUS_MIN) * unit_float(k.x);
                let amp = LUMP_HEIGHT_MIN + (1.0 - LUMP_HEIGHT_MIN) * unit_float(k.y);
                let d2 = length_squared(q - centre);
                if d2 < r * r {
                    best = max(best, amp * sqrt(r * r - d2));
                }
            }
        }
    }
    return field.lumpiness * best * field.lump_size;
}

/// Returns the wanted top height of a layer clump at `p`, which has coverage
/// weight `w`, before any limits: the base height plus lumps, raised by the
/// point's rise where visible and lowered by its dip where buried.
fn layer_top(field: Field, p: Point, w: f32) -> f32 {
    let base = top(field, w) + w * lump_height(field, p.point);
    if base > 0.0 {
        return base + p.rise * fade(field, base);
    }
    var buried = 1.0;
    if field.sink > 0.0 {
        buried = min(-base / field.sink, 1.0);
    }
    return base - p.dip * buried;
}

/// Returns unit direction `dir` turned toward unit normal `normal` just
/// enough that they are at most 60° apart.
fn limit_tilt(dir: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    let cos_angle = dot3(dir, normal);
    if cos_angle >= COS_MAX_PUSH_TILT {
        return dir;
    }
    // Rotate within the plane of the two vectors to sit at the limit.
    let side = normalize_or_zero(dir - normal * cos_angle);
    if all(side == vec3<f32>(0.0)) {
        return normal;
    }
    return normal * COS_MAX_PUSH_TILT + side * SIN_MAX_PUSH_TILT;
}

/// Returns a top height moved halfway from `top_height` toward `floor`, or
/// onto `floor` when that close.
fn lowered(top_height: f32, floor_height: f32) -> f32 {
    let half = floor_height + (top_height - floor_height) * 0.5;
    if abs(half - floor_height) < 1e-6 {
        return floor_height;
    }
    return half;
}
