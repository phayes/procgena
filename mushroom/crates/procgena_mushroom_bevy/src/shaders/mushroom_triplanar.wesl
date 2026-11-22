// Triplanar texturing for mushroom caps: samples repeating images projected
// along the x, y and z axes and blends them by surface direction, so a dome
// shows no stretching and no pinch at its top. Adapted from
// procgena_stone_bevy's `stone_triplanar.wesl`.

/// Surface detail sampled from a cap texture set at one point.
struct TriplanarSample {
    /// Linear colour from the colour image.
    color: vec3<f32>,
    /// How matte the surface looks: 0 is polished, 1 is matte (orh green).
    roughness: f32,
    /// Light reaching the surface: 1 leaves it unchanged; lower values darken
    /// hollows (orh red).
    occlusion: f32,
    /// Unit surface normal with the normal map's detail, in the space of the
    /// position and normal passed to `sample_triplanar`.
    normal: vec3<f32>,
}

/// Width of the blend between competing projections, measured after combining
/// height with surface direction. Smaller values give sharper transitions.
const HEIGHT_BLEND_DEPTH: f32 = 0.2;

/// Returns how much of each x, y and z projection to show, with weights that
/// sum to 1. Pass a unit surface normal and each projection's height in 0 to 1.
/// Surface direction favours the best-facing projection; height lets raised
/// details, such as warts, show through where projections meet.
fn triplanar_weights(n: vec3<f32>, heights: vec3<f32>) -> vec3<f32> {
    let facing = abs(n) / (abs(n.x) + abs(n.y) + abs(n.z));
    let raised = heights + facing * 3.0;
    let floor = max(max(raised.x, raised.y), raised.z) - HEIGHT_BLEND_DEPTH;
    let w = max(raised - vec3<f32>(floor), vec3<f32>(0.0));
    return w / (w.x + w.y + w.z);
}

/// Reads a normal map texel as a tangent-space direction: x along the image's
/// right, y towards its top (OpenGL convention), z out of the surface.
fn unpack_normal(texel: vec4<f32>) -> vec3<f32> {
    return texel.rgb * 2.0 - 1.0;
}

/// Returns colour, roughness, occlusion and a detailed normal at `position`.
/// Pass a unit `normal` in the same coordinate space and a positive
/// `texture_size`, measured in position units per image repeat.
///
/// Supply colour as sRGB, an OpenGL (+Y) normal map as linear data, and
/// `orh_texture` as linear occlusion, roughness and height in red, green and
/// blue. Height affects blending, not geometry. Positive `mip_bias` selects
/// coarser texture detail; negative values select finer detail.
fn sample_triplanar(
    color_texture: texture_2d<f32>,
    color_sampler: sampler,
    normal_texture: texture_2d<f32>,
    normal_sampler: sampler,
    orh_texture: texture_2d<f32>,
    orh_sampler: sampler,
    position: vec3<f32>,
    normal: vec3<f32>,
    texture_size: f32,
    mip_bias: f32,
) -> TriplanarSample {
    let p = position / texture_size;
    // Project onto the planes perpendicular to x, y and z. Flip the second
    // coordinate so the image's top follows the positive axis.
    let uv_x = vec2<f32>(p.z, -p.y);
    let uv_y = vec2<f32>(p.x, -p.z);
    let uv_z = vec2<f32>(p.x, -p.y);

    // Read all three projections, then use their heights and the surface
    // direction to decide how much each contributes.
    let orh_x = textureSampleBias(orh_texture, orh_sampler, uv_x, mip_bias).rgb;
    let orh_y = textureSampleBias(orh_texture, orh_sampler, uv_y, mip_bias).rgb;
    let orh_z = textureSampleBias(orh_texture, orh_sampler, uv_z, mip_bias).rgb;
    let w = triplanar_weights(normal, vec3<f32>(orh_x.b, orh_y.b, orh_z.b));

    // Use the same blend for colour, occlusion and roughness to keep them aligned.
    let color = textureSampleBias(color_texture, color_sampler, uv_x, mip_bias).rgb * w.x
        + textureSampleBias(color_texture, color_sampler, uv_y, mip_bias).rgb * w.y
        + textureSampleBias(color_texture, color_sampler, uv_z, mip_bias).rgb * w.z;
    let orh = orh_x * w.x + orh_y * w.y + orh_z * w.z;

    // Add each normal map's bumps to the surface direction (whiteout blending).
    // Put all three normals back in x, y, z order before blending them.
    let t_x = unpack_normal(textureSampleBias(normal_texture, normal_sampler, uv_x, mip_bias));
    let t_y = unpack_normal(textureSampleBias(normal_texture, normal_sampler, uv_y, mip_bias));
    let t_z = unpack_normal(textureSampleBias(normal_texture, normal_sampler, uv_z, mip_bias));
    let n_x = vec3<f32>(t_x.xy + normal.zy, abs(t_x.z) * normal.x);
    let n_y = vec3<f32>(t_y.xy + normal.xz, abs(t_y.z) * normal.y);
    let n_z = vec3<f32>(t_z.xy + normal.xy, abs(t_z.z) * normal.z);
    let blended = normalize(n_x.zyx * w.x + n_y.xzy * w.y + n_z.xyz * w.z);

    return TriplanarSample(color, orh.g, orh.r, blended);
}
