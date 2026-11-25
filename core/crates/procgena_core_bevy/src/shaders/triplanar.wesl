// Triplanar texturing math: projecting repeating textures along the x, y
// and z axes, weighting the three by surface direction and height, and
// adding normal-map detail to a surface normal.
//
// Import as `procgena_core_bevy::shaders::triplanar` once `LodPlugin` is
// added. It reads no textures itself, so it works with plain textures and
// texture arrays alike: sample the three projections with the coordinates
// from `triplanar_uvs`, then combine them with the functions below.
//
// Conventions: normal maps are OpenGL (+Y), read with `unpack_normal`; the
// height used for blending runs from 0 (low) to 1 (raised).

/// The texture coordinates of the three projections of `p` (a position
/// divided by the texture's repeat length): seen along x, along y (from
/// above) and along z. The image's top follows the positive axis.
struct TriplanarUvs {
    x: vec2<f32>,
    y: vec2<f32>,
    z: vec2<f32>,
}

/// Returns the three projections' texture coordinates for `p`.
fn triplanar_uvs(p: vec3<f32>) -> TriplanarUvs {
    return TriplanarUvs(vec2<f32>(p.z, -p.y), vec2<f32>(p.x, -p.z), vec2<f32>(p.x, -p.y));
}

/// Width of the blend between competing projections, measured after
/// combining height with surface direction. Smaller values give sharper
/// transitions.
const TRIPLANAR_BLEND_DEPTH: f32 = 0.2;

/// Returns how much of each x, y and z projection to show, with weights
/// that add up to 1. Pass a unit surface normal and each projection's
/// height from 0 to 1. Surface direction favours the best-facing
/// projection; height lets raised texture details show through where
/// projections meet.
fn triplanar_weights(n: vec3<f32>, heights: vec3<f32>) -> vec3<f32> {
    let facing = abs(n) / (abs(n.x) + abs(n.y) + abs(n.z));
    let raised = heights + facing * 3.0;
    let floor = max(max(raised.x, raised.y), raised.z) - TRIPLANAR_BLEND_DEPTH;
    let w = max(raised - vec3<f32>(floor), vec3<f32>(0.0));
    return w / (w.x + w.y + w.z);
}

/// Reads a normal map texel as a tangent-space direction: x along the
/// image's right, y towards its top (OpenGL convention), z out of the
/// surface.
fn unpack_normal(texel: vec4<f32>) -> vec3<f32> {
    return texel.rgb * 2.0 - 1.0;
}

/// Adds normal-map detail to the unit surface normal `n`, for each
/// projection's tangent-space normal (`t_x`, `t_y`, `t_z`, from
/// `unpack_normal`), mixed by the projection weights `w` (whiteout
/// blending). Returns a unit normal.
fn triplanar_normal(t_x: vec3<f32>, t_y: vec3<f32>, t_z: vec3<f32>, n: vec3<f32>, w: vec3<f32>) -> vec3<f32> {
    let n_x = vec3<f32>(t_x.xy + n.zy, abs(t_x.z) * n.x);
    let n_y = vec3<f32>(t_y.xy + n.xz, abs(t_y.z) * n.y);
    let n_z = vec3<f32>(t_z.xy + n.xy, abs(t_z.z) * n.z);
    return normalize(n_x.zyx * w.x + n_y.xzy * w.y + n_z.xyz * w.z);
}

/// Adds normal-map detail to the unit surface normal `n` from the top
/// projection only (`t_y`, from `unpack_normal`), for ground textured from
/// above. Returns a unit normal.
fn top_normal(t_y: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    return normalize(vec3<f32>(t_y.x + n.x, abs(t_y.z) * n.y, t_y.y + n.z));
}
